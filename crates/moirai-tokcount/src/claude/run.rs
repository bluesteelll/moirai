//! The runner: preflight checks, the argument vector, the child's environment, the work directories, and one
//! bounded process run.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::Error;
use super::config::RunnerConfig;
use super::exe;
use super::record::{CallRecord, StreamParser, comparable_inputs};

/// Entries whose presence in the scratch root or any of its ancestors disqualifies it as a working directory:
/// Claude Code treats a `.git` ancestor as the project's repository, walks every ancestor for `CLAUDE.md` and
/// `CLAUDE.local.md`, and reads `.claude/` (its `CLAUDE.md`, `rules/`, `skills/`, `agents/`, `commands/`,
/// `settings.json`) as project context. The check does not rely on `--setting-sources` to keep any of them out, and
/// it catches a scratch root under the owner's home, whose `.claude` holds the owner's instructions and memory.
pub const SCRATCH_HAZARDS: [&str; 4] = [".git", ".claude", "CLAUDE.md", "CLAUDE.local.md"];

/// What the runner-owned `CLAUDE_CONFIG_DIR` may not hold, because it reaches the context and the runner's calls
/// carry none of the owner's text (PLAN WP-58 acceptance: no project or user `CLAUDE.md`, no memory text):
///
/// - `CLAUDE.md` and `CLAUDE.local.md`, user instructions: refused whenever the entry exists;
/// - `rules`, `agents`, `commands` and `output-styles`: refused when they hold a Markdown file (`*.md`) at any
///   depth;
/// - `skills`: refused when it holds a `SKILL.md` at any depth;
/// - `plugins/installed_plugins.json`: refused unless it is empty or lists no plugin.
///
/// An empty definition directory, and the plugin bookkeeping Claude Code writes on its own (the marketplace list and
/// the marketplace clones under `plugins/`), are accepted: they put nothing in the context, and the init check
/// refuses a plugin, skill or agent that loads all the same. File names compare without case. Inside a definition
/// directory, a symbolic link or junction, an entry that cannot be read, or a directory [`DEFINITION_DEPTH`] levels
/// down counts as a definition (fail closed). The layout is to be confirmed at V9 on the pinned version (the V9
/// checklist of the [module documentation](super)).
pub const CONFIG_DIR_HAZARDS: [&str; 8] = [
    "CLAUDE.md",
    "CLAUDE.local.md",
    "rules",
    "skills",
    "agents",
    "commands",
    "output-styles",
    INSTALLED_PLUGINS,
];

/// How deep the definition directories of [`CONFIG_DIR_HAZARDS`] are searched: a directory this many levels below
/// the definition directory counts as a definition.
pub const DEFINITION_DEPTH: usize = 8;

const INSTRUCTION_FILES: [&str; 2] = ["CLAUDE.md", "CLAUDE.local.md"];
const MARKDOWN_DIRS: [&str; 4] = ["rules", "agents", "commands", "output-styles"];
const SKILLS_DIR: &str = "skills";
const INSTALLED_PLUGINS: &str = "plugins/installed_plugins.json";
/// A larger `installed_plugins.json` is not read and counts as listing plugins.
const INSTALLED_PLUGINS_MAX: u64 = 4 << 20;

/// Keys the runner-owned `settings.json` may not set when `setting-sources` includes `user`: `env` would bypass
/// the environment allow-list (an `ANTHROPIC_BASE_URL`, a provider switch), `hooks` can add context
/// (`SessionStart` `additionalContext`), `enabledPlugins` and `outputStyle` change the context, and `apiKeyHelper`
/// moves the login off the subscription (PLAN §6.1 #6).
pub const USER_SETTINGS_HAZARDS: [&str; 5] = [
    "env",
    "hooks",
    "enabledPlugins",
    "outputStyle",
    "apiKeyHelper",
];

/// The managed-policy files Claude Code loads whatever `--setting-sources` says: enterprise settings, MCP servers
/// and instructions.
pub const MANAGED_POLICY_FILES: [&str; 3] =
    ["managed-settings.json", "managed-mcp.json", "CLAUDE.md"];

/// The parent's variables the child receives (names compared without case); every other variable of the parent is
/// left out, so no API key, token or endpoint (`ANTHROPIC_*`), no variable of a surrounding Claude Code session
/// (`CLAUDECODE`, `CLAUDE_CODE_*`, `CLAUDE_CONFIG_DIR`), no git redirection (`GIT_DIR`, `GIT_WORK_TREE`,
/// `GIT_INDEX_FILE`, set inside a git hook or `git rebase -x`) and no switch that changes the measured context
/// (MCP and thinking limits, tool deferral, prompt caching, `NODE_OPTIONS`) reaches it. The list holds the system
/// locations, the temporary directories, the user and the home, the locale ([`PASSED_ENV_PREFIXES`]), the time
/// zone, and the network's proxies and certificates. The runner's own variables and the configured `env` come on
/// top.
pub const PASSED_ENV: [&str; 43] = [
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "ProgramData",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "USERNAME",
    "USERDOMAIN",
    "COMPUTERNAME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "TZ",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// Name prefixes of the parent's variables the child receives (compared without case): the locale categories.
pub const PASSED_ENV_PREFIXES: [&str; 1] = ["LC_"];

/// The request of [`Claude::isolation_check`] when the caller has none of its own: one turn, no tools.
pub const ISOLATION_PROMPT: &str = "Reply with the single word OK.";

/// Poll interval of the wait loop.
const POLL: Duration = Duration::from_millis(20);
/// How long the runner waits for stdout to close after the process exited (a descendant may hold it), and after a
/// kill for the killed processes to let go of it.
const EXIT_GRACE: Duration = Duration::from_secs(2);
/// The head of stderr kept for error messages.
const STDERR_KEEP: usize = 64 << 10;
/// The line buffers that circulate between the stdout reader and the parser: at most this many lines are held at
/// once, each at most `max-line-bytes`.
const LINE_BUFFERS: usize = 3;
/// A line buffer that grew beyond this is shrunk back before it is reused.
const KEEP_CAPACITY: usize = 256 << 10;
/// The stdout read buffer.
const READ_BUF: usize = 64 << 10;
/// Tries at removing a work directory that the OS still holds for a moment (a process ending, a scanner), and the
/// pause between them.
const REMOVE_TRIES: u32 = 10;
const REMOVE_PAUSE: Duration = Duration::from_millis(50);

/// Whether the child receives the parent's variable `name` ([`PASSED_ENV`], [`PASSED_ENV_PREFIXES`]).
#[must_use]
pub fn is_passed_env(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    PASSED_ENV
        .iter()
        .any(|passed| name.eq_ignore_ascii_case(passed))
        || PASSED_ENV_PREFIXES.iter().any(|prefix| {
            name.len() > prefix.len()
                && name.is_char_boundary(prefix.len())
                && name[..prefix.len()].eq_ignore_ascii_case(prefix)
        })
}

/// A file or directory that disqualifies a runner directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hazard {
    /// The entry found.
    pub path: PathBuf,
    /// The entry's name as listed in [`SCRATCH_HAZARDS`], [`CONFIG_DIR_HAZARDS`], [`MANAGED_POLICY_FILES`] or
    /// [`USER_SETTINGS_HAZARDS`].
    pub entry: &'static str,
}

/// The first [`SCRATCH_HAZARDS`] entry in `dir` or one of its ancestors, checked on the path as given and on the
/// canonical form of its nearest existing ancestor, or a component of `dir` that bears such a name (a directory
/// still to be created, which would become one). `dir` need not exist, so the check can run before anything is
/// created. An entry that cannot be examined counts as present.
#[must_use]
pub fn scratch_hazard(dir: &Path) -> Option<Hazard> {
    if let Some(named) = named_hazard(dir) {
        return Some(named);
    }
    let existing = dir
        .ancestors()
        .find(|a| !a.as_os_str().is_empty() && fs::symlink_metadata(a).is_ok());
    let canonical = existing.and_then(|a| fs::canonicalize(a).ok());
    [Some(dir), canonical.as_deref()]
        .into_iter()
        .flatten()
        .flat_map(Path::ancestors)
        .find_map(|ancestor| present(ancestor, &SCRATCH_HAZARDS))
}

/// The first component of `dir` whose name is a [`SCRATCH_HAZARDS`] entry (compared without case).
fn named_hazard(dir: &Path) -> Option<Hazard> {
    let mut prefix = PathBuf::new();
    for component in dir.components() {
        prefix.push(component);
        if let Component::Normal(name) = component
            && let Some(&entry) = SCRATCH_HAZARDS
                .iter()
                .find(|hazard| name.eq_ignore_ascii_case(hazard))
        {
            return Some(Hazard {
                path: prefix,
                entry,
            });
        }
    }
    None
}

/// The first [`CONFIG_DIR_HAZARDS`] entry inside `config_dir`: an instruction file, a definition file, or an
/// `installed_plugins.json` that lists a plugin. The hazard's `path` is the file found.
#[must_use]
pub fn config_dir_hazard(config_dir: &Path) -> Option<Hazard> {
    present(config_dir, &INSTRUCTION_FILES)
        .or_else(|| {
            MARKDOWN_DIRS
                .iter()
                .map(|&entry| (entry, false))
                .chain([(SKILLS_DIR, true)])
                .find_map(|(entry, skills)| {
                    definition_in(&config_dir.join(entry), skills)
                        .map(|path| Hazard { path, entry })
                })
        })
        .or_else(|| installed_plugins_hazard(config_dir))
}

/// The first definition file under `dir`: a `SKILL.md` when `skills`, else a `*.md`; `None` when `dir` is absent,
/// not a directory, or holds none. A link, an unreadable entry or a nesting beyond [`DEFINITION_DEPTH`] is returned
/// as found.
fn definition_in(dir: &Path, skills: bool) -> Option<PathBuf> {
    match fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Ok(meta) if meta.is_dir() => {}
        Ok(meta) if !meta.file_type().is_symlink() => return None,
        _ => return Some(dir.to_path_buf()),
    }
    let mut pending = vec![(dir.to_path_buf(), 0usize)];
    while let Some((current, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            return Some(current);
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return Some(current);
            };
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                return Some(path);
            };
            if kind.is_symlink() {
                return Some(path);
            }
            if kind.is_dir() {
                if depth + 1 >= DEFINITION_DEPTH {
                    return Some(path);
                }
                pending.push((path, depth + 1));
            } else if is_definition(&entry.file_name(), skills) {
                return Some(path);
            }
        }
    }
    None
}

fn is_definition(name: &OsStr, skills: bool) -> bool {
    if skills {
        name.eq_ignore_ascii_case("SKILL.md")
    } else {
        Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    }
}

/// `plugins/installed_plugins.json` when it lists a plugin: anything but an absent or blank file, or a JSON object
/// whose `plugins` member is absent, null, an empty object or an empty array. A file that cannot be read, is larger
/// than 4 MiB or is not such JSON counts as listing plugins.
fn installed_plugins_hazard(config_dir: &Path) -> Option<Hazard> {
    let path = config_dir.join("plugins").join("installed_plugins.json");
    let bytes = match File::open(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(_) => None,
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(INSTALLED_PLUGINS_MAX + 1)
                .read_to_end(&mut bytes)
                .ok()
                .filter(|_| bytes.len() as u64 <= INSTALLED_PLUGINS_MAX)
                .map(|_| bytes)
        }
    };
    let lists_none = bytes.is_some_and(|bytes| {
        bytes.trim_ascii().is_empty()
            || match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(obj)) => match obj.get("plugins") {
                    None | Some(Value::Null) => true,
                    Some(Value::Object(plugins)) => plugins.is_empty(),
                    Some(Value::Array(plugins)) => plugins.is_empty(),
                    Some(_) => false,
                },
                _ => false,
            }
    });
    (!lists_none).then_some(Hazard {
        path,
        entry: INSTALLED_PLUGINS,
    })
}

/// The first [`USER_SETTINGS_HAZARDS`] key that `config_dir/settings.json` sets; `None` when the file is absent.
///
/// # Errors
///
/// The file cannot be read, or it is not a JSON object.
pub fn user_settings_hazard(config_dir: &Path) -> Result<Option<Hazard>, Error> {
    let path = config_dir.join("settings.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(format!("reading {}", path.display()))(e)),
    };
    let Ok(Value::Object(settings)) = serde_json::from_slice::<Value>(&bytes) else {
        return Err(Error::Config(format!(
            "{} is not a JSON object",
            path.display()
        )));
    };
    Ok(USER_SETTINGS_HAZARDS
        .iter()
        .find(|key| settings.contains_key(**key))
        .map(|&entry| Hazard { path, entry }))
}

/// The directories that hold Claude Code's managed-policy files on `os` (a [`std::env::consts::OS`] value):
/// `%ProgramFiles%\ClaudeCode` and `%ProgramData%\ClaudeCode` on Windows (`var` reads the two variables; the
/// usual locations when unset), `/Library/Application Support/ClaudeCode` on macOS, `/etc/claude-code` elsewhere.
/// The default of the `managed-policy-dirs` configuration key, with this process's OS and environment; to be
/// confirmed at V9 on the pinned version.
#[must_use]
pub fn managed_policy_dirs(os: &str, var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    match os {
        "windows" => [
            ("ProgramFiles", r"C:\Program Files"),
            ("ProgramData", r"C:\ProgramData"),
        ]
        .into_iter()
        .map(|(name, usual)| {
            var(name)
                .map_or_else(|| PathBuf::from(usual), PathBuf::from)
                .join("ClaudeCode")
        })
        .collect(),
        "macos" => vec![PathBuf::from("/Library/Application Support/ClaudeCode")],
        _ => vec![PathBuf::from("/etc/claude-code")],
    }
}

/// The first [`MANAGED_POLICY_FILES`] entry present in one of `dirs` (the `managed-policy-dirs` configuration key).
#[must_use]
pub fn managed_policy_hazard(dirs: &[PathBuf]) -> Option<Hazard> {
    dirs.iter()
        .find_map(|dir| present(dir, &MANAGED_POLICY_FILES))
}

fn present(dir: &Path, entries: &[&'static str]) -> Option<Hazard> {
    entries.iter().find_map(|&entry| {
        let path = dir.join(entry);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            _ => Some(Hazard { path, entry }),
        }
    })
}

/// Claude Code's directory name under `<config-dir>/projects` for a working directory: every character other than
/// an ASCII letter or digit becomes `-` (`D:\runner\scratch\work-0` → `D--runner-scratch-work-0`).
#[must_use]
pub fn project_slug(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Where the prompt comes from; it always reaches Claude Code on stdin, never in argv.
#[derive(Clone, Copy, Debug)]
pub enum Prompt<'a> {
    /// The prompt text.
    Text(&'a str),
    /// A file whose bytes are the prompt, streamed to stdin.
    File(&'a Path),
}

/// The built-in tools the session keeps (`--tools`).
#[derive(Clone, Copy, Debug)]
pub enum BuiltinTools<'a> {
    /// Claude Code's default set (no `--tools`).
    Default,
    /// None (`--tools ""`): removed, not only denied (`docs/spec/reviews/a1-A.md` A-M5).
    None,
    /// Exactly these.
    Only(&'a [&'a str]),
}

/// One headless call.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// The prompt, sent on stdin.
    pub prompt: Prompt<'a>,
    /// A file appended to Claude Code's system prompt (`--append-system-prompt-file`), e.g. the LQ card.
    pub append_system_file: Option<&'a Path>,
    /// The MCP configuration file (`--mcp-config`); the call always passes `--strict-mcp-config`, so without one the
    /// session has no MCP server. With one, every server the init lists must be `connected` (configuration key
    /// `mcp-failed`).
    pub mcp_config_file: Option<&'a Path>,
    /// The built-in tools the session keeps.
    pub builtin_tools: BuiltinTools<'a>,
    /// Tools allowed without a permission prompt (`--allowed-tools`), e.g. `mcp__<server>__<tool>`.
    pub allowed_tools: &'a [&'a str],
    /// Tools denied (`--disallowed-tools`).
    pub disallowed_tools: &'a [&'a str],
    /// The turn limit (`--max-turns`), at least 1.
    pub max_turns: u32,
}

impl<'a> Request<'a> {
    /// A one-turn call with no built-in tools, no MCP server and no appended system text.
    #[must_use]
    pub const fn new(prompt: Prompt<'a>) -> Self {
        Self {
            prompt,
            append_system_file: None,
            mcp_config_file: None,
            builtin_tools: BuiltinTools::None,
            allowed_tools: &[],
            disallowed_tools: &[],
            max_turns: 1,
        }
    }
}

/// The result of [`Claude::isolation_check`].
#[derive(Clone, Debug)]
pub struct Isolation {
    /// The call as configured.
    pub configured: CallRecord,
    /// The same call with `HOME` and `USERPROFILE` pointing to an empty directory.
    pub isolated: CallRecord,
    /// The pinned model's total input of `configured` minus that of `isolated`: the tokens Claude Code loaded from
    /// the owner's home.
    pub difference: i64,
}

impl Isolation {
    /// Whether nothing from the owner's home reached the context (a difference of 0).
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.difference == 0
    }

    /// The check as JSON (`moirai-tokcount.claude-isolation.v1`).
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "record": "moirai-tokcount.claude-isolation.v1",
            "passed": self.passed(),
            "difference": self.difference,
            "configured": self.configured.to_json(),
            "isolated": self.isolated.to_json(),
        })
    }
}

/// The headless Claude Code runner (see the [module documentation](super)).
#[derive(Debug)]
pub struct Claude {
    config: RunnerConfig,
    exe: PathBuf,
}

impl Claude {
    /// Validates `config`, resolves the native executable and runs the preflight checks: the config directory
    /// exists and holds no [`CONFIG_DIR_HAZARDS`] entry ([`config_dir_hazard`]); with `setting-sources` including
    /// `user`, its `settings.json` sets no [`USER_SETTINGS_HAZARDS`] key; no managed-policy file exists in the
    /// configured `managed-policy-dirs` ([`managed_policy_hazard`]); the scratch root has no [`SCRATCH_HAZARDS`]
    /// entry in itself or any ancestor, checked before it is created and again after.
    ///
    /// # Errors
    ///
    /// The first check that fails.
    pub fn new(config: RunnerConfig) -> Result<Self, Error> {
        config.validate()?;
        let exe = exe::resolve(config.claude_exe.as_deref())?;
        let runner = Self { config, exe };
        runner.preflight()?;
        Ok(runner)
    }

    /// The configuration.
    #[must_use]
    pub const fn config(&self) -> &RunnerConfig {
        &self.config
    }

    /// The native executable every call runs.
    #[must_use]
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    fn preflight(&self) -> Result<(), Error> {
        exe::check_native(&self.exe)?;
        let cfg = &self.config.config_dir;
        if !cfg.is_dir() {
            return Err(Error::Config(format!(
                "\"config-dir\" {} does not exist; create it and log in once: run {} with CLAUDE_CONFIG_DIR set to it \
                 and use /login (V9)",
                cfg.display(),
                self.exe.display()
            )));
        }
        if let Some(h) = config_dir_hazard(cfg) {
            return Err(Error::Context {
                path: h.path,
                reason: "the runner-owned CLAUDE_CONFIG_DIR may hold no instructions and no rule, skill, agent, \
                         command, output-style or installed-plugin definition",
            });
        }
        if self.config.reads_user_settings()
            && let Some(h) = user_settings_hazard(cfg)?
        {
            return Err(Error::Context {
                path: h.path,
                reason: "with setting-sources `user`, the runner-owned settings.json may not set env, hooks, \
                         enabledPlugins, outputStyle or apiKeyHelper: they bypass the environment allow-list and the \
                         context checks",
            });
        }
        if let Some(h) = managed_policy_hazard(&self.config.managed_policy_dirs) {
            return Err(Error::Context {
                path: h.path,
                reason: "Claude Code loads managed policy whatever --setting-sources says; the runner's calls may \
                         carry no settings, servers or instructions but their own",
            });
        }
        let scratch = &self.config.scratch_root;
        let refuse = |h: Hazard| Error::Context {
            path: h.path,
            reason: "Claude Code would read this as project context or a repository; put the runner root outside \
                     every repository and user profile",
        };
        // Checked before anything is created, so a refused root leaves no directory behind; and again once it
        // exists, through any link on its way.
        if let Some(h) = scratch_hazard(scratch) {
            return Err(refuse(h));
        }
        fs::create_dir_all(scratch)
            .map_err(Error::io(format!("creating {}", scratch.display())))?;
        if let Some(h) = scratch_hazard(scratch) {
            return Err(refuse(h));
        }
        Ok(())
    }

    /// The argument vector of `request`: no prompt or system text, only flags, the model pin and absolute paths.
    ///
    /// # Errors
    ///
    /// [`Error::Request`] for a missing file, a tool name that is empty, starts with `-` or holds a comma or a
    /// control character, or a turn limit of 0.
    pub fn args(&self, request: &Request<'_>) -> Result<Vec<OsString>, Error> {
        if request.max_turns == 0 {
            return Err(Error::Request("max_turns must be at least 1".to_owned()));
        }
        let cfg = &self.config;
        let mut args: Vec<OsString> = Vec::with_capacity(24);
        let mut push = |a: &str| args.push(OsString::from(a));
        push("-p");
        push("--model");
        push(&cfg.model);
        push("--output-format");
        push("stream-json");
        push("--verbose");
        push("--setting-sources");
        push(&cfg.setting_sources);
        push("--strict-mcp-config");
        push("--max-turns");
        push(&request.max_turns.to_string());
        match request.builtin_tools {
            BuiltinTools::Default => {}
            BuiltinTools::None => {
                push("--tools");
                push("");
            }
            BuiltinTools::Only(tools) => {
                push("--tools");
                push(&tool_list(tools)?);
            }
        }
        if !request.allowed_tools.is_empty() {
            push("--allowed-tools");
            push(&tool_list(request.allowed_tools)?);
        }
        if !request.disallowed_tools.is_empty() {
            push("--disallowed-tools");
            push(&tool_list(request.disallowed_tools)?);
        }
        if cfg.disable_slash_commands {
            push("--disable-slash-commands");
        }
        if !cfg.session_persistence {
            push("--no-session-persistence");
        }
        if cfg.exclude_dynamic_sections {
            push("--exclude-dynamic-system-prompt-sections");
        }
        if let Some(file) = request.mcp_config_file {
            args.push(OsString::from("--mcp-config"));
            args.push(input_file("the MCP configuration", file)?.into_os_string());
        }
        if let Some(file) = request.append_system_file {
            args.push(OsString::from("--append-system-prompt-file"));
            args.push(input_file("the appended system text", file)?.into_os_string());
        }
        Ok(args)
    }

    /// The Claude Code version (`claude --version`, no model call), e.g. `2.1.110`, recorded beside every result
    /// ([90 §8.3] item (4)).
    ///
    /// # Errors
    ///
    /// The preflight fails, the process fails or times out, or it prints nothing; [`Error::VersionMismatch`] when
    /// `claude-code-version` pins another version.
    pub fn version(&self) -> Result<String, Error> {
        self.preflight()?;
        let command = self.command(&self.config.scratch_root, &[OsString::from("--version")]);
        let mut first: Option<String> = None;
        let finished = run_child(command, None, &self.limits(), &mut |line| {
            if first.is_none() {
                first = Some(String::from_utf8_lossy(line).trim().to_owned());
            }
            Ok(Progress::Streaming)
        })?;
        let Some(status) = finished.status else {
            return Err(Error::Protocol(
                "`claude --version` was killed before it exited".to_owned(),
            ));
        };
        if !status.success() {
            return Err(Error::Protocol(format!(
                "`claude --version` exited with {status}: {}",
                String::from_utf8_lossy(&finished.stderr).trim()
            )));
        }
        let line = first
            .filter(|l| !l.is_empty())
            .ok_or_else(|| Error::Protocol("`claude --version` printed nothing".to_owned()))?;
        let version = line.split(" (").next().unwrap_or(&line).trim().to_owned();
        match &self.config.claude_code_version {
            Some(pin) if *pin != version => Err(Error::VersionMismatch {
                pinned: pin.clone(),
                reported: Some(version),
            }),
            _ => Ok(version),
        }
    }

    /// Runs one headless call and returns its record. Each raw stdout line is also written to `tee` when given (the
    /// raw stream belongs in `/private/`, PLAN §3.2 item 5); `tee` holds every line up to a refusal too.
    ///
    /// The call runs in the stable work directory of a free slot (`<scratch-root>/work-<slot>`, the lowest slot
    /// whose lock is free), emptied before the call and removed after it. Claude Code writes the working directory
    /// into its system prompt, so a fresh directory per call would make every system prompt differ, cache-write the
    /// environment section and the appended text on every call, and add a project entry to `config-dir` per call.
    ///
    /// Once the `result` message has arrived, the process gets `post-result-grace-ms` to exit; a process that
    /// lingers beyond it (an MCP server's shutdown, a telemetry flush) is killed with its tree, and the complete
    /// record is returned with no exit code and [`CallRecord::killed_after_result`] set, instead of a timeout that
    /// would throw the paid call away. When the process exits while a descendant still holds its stdout, the
    /// record says [`CallRecord::pipes_held_after_exit`]; on Windows the descendants still running are then killed
    /// (the [module documentation](super) states the residual case elsewhere).
    ///
    /// # Errors
    ///
    /// The preflight or the request fails; no work slot is free ([`Error::Busy`]); the process cannot start, times
    /// out before its result or prints an over-long line; the stream reports another model or version, an API-key
    /// login, owner context or an MCP server that did not connect, is malformed or incomplete; or `tee` fails. The
    /// process tree is killed on every error that arrives while it runs.
    pub fn call(
        &self,
        request: &Request<'_>,
        tee: Option<&mut dyn Write>,
    ) -> Result<CallRecord, Error> {
        self.run_call(request, tee, false)
    }

    /// The mechanical canary of the V9 check (PLAN WP-58 acceptance: no user `CLAUDE.md` and no memory text): runs
    /// `request` twice, once as configured and once with `HOME` and `USERPROFILE` pointing to an empty directory,
    /// and compares the pinned model's total input. Whatever Claude Code would load from the owner's home — a
    /// user `CLAUDE.md`, rules or memory under `~/.claude` — enters only the first call, so a difference of 0 shows
    /// that none reached the context.
    ///
    /// It adds to the owner's reading of one call's transcript ([`Claude::transcript_path`]), which the acceptance
    /// names, and does not replace it: whether Claude Code writes `CLAUDE.md` or memory text into the saved
    /// transcript, or adds it only to the request it sends, is to be confirmed at V9. The canary cannot see context
    /// that enters both calls alike: the runner-owned `config-dir`, where the login lives, a managed policy, or the
    /// environment. The preflight's hazard lists, the environment allow-list and the init's owner-context check
    /// cover those.
    ///
    /// # Errors
    ///
    /// Either call fails ([`Claude::call`]), or the two cannot be compared ([`Error::Delta`], e.g. they ran in
    /// different work slots, took more than one turn, or ended in an error).
    pub fn isolation_check(&self, request: &Request<'_>) -> Result<Isolation, Error> {
        let configured = self.run_call(request, None, false)?;
        let isolated = self.run_call(request, None, true)?;
        let (a, b) = comparable_inputs(
            (&configured, "as configured"),
            (&isolated, "with an empty home"),
        )?;
        let difference = i64::try_from(i128::from(a) - i128::from(b)).unwrap_or(if a > b {
            i64::MAX
        } else {
            i64::MIN
        });
        Ok(Isolation {
            configured,
            isolated,
            difference,
        })
    }

    fn limits(&self) -> Limits {
        Limits {
            timeout: self.config.timeout,
            max_line: self.config.max_line_bytes,
            post_result_grace: self.config.post_result_grace,
        }
    }

    fn run_call(
        &self,
        request: &Request<'_>,
        mut tee: Option<&mut dyn Write>,
        empty_home: bool,
    ) -> Result<CallRecord, Error> {
        self.preflight()?;
        let args = self.args(request)?;
        let stdin = match request.prompt {
            Prompt::Text(text) => StdinSource::Bytes(text.as_bytes()),
            Prompt::File(path) => {
                let path = input_file("the prompt", path)?;
                StdinSource::File(
                    File::open(&path).map_err(Error::io(format!("opening {}", path.display())))?,
                )
            }
        };
        let work = WorkDir::acquire(&self.config.scratch_root, self.config.work_slots)?;
        let mut command = self.command(&work.path, &args);
        if empty_home {
            fresh_dir(&work.home)
                .map_err(Error::io(format!("creating {}", work.home.display())))?;
            command
                .env("HOME", &work.home)
                .env("USERPROFILE", &work.home);
        }
        let mut checks = self.config.stream_checks();
        checks.mcp_config = request.mcp_config_file.is_some();
        let mut parser = StreamParser::new(checks);
        let outcome = {
            let mut on_line = |line: &[u8]| -> Result<Progress, Error> {
                if let Some(out) = tee.as_mut() {
                    out.write_all(line)
                        .map_err(Error::io("writing the raw stream"))?;
                }
                parser.feed_line(line)?;
                Ok(if parser.has_result() {
                    Progress::Complete
                } else {
                    Progress::Streaming
                })
            };
            run_child(command, Some(stdin), &self.limits(), &mut on_line)
        };
        if let Some(out) = tee {
            let flushed = out.flush();
            if outcome.is_ok() {
                flushed.map_err(Error::io("writing the raw stream"))?;
            }
        }
        let finished = outcome?;
        drop(work);
        let exit_code = finished.status.and_then(|status| status.code());
        let mut record = parser.finish(exit_code).map_err(|e| match e {
            Error::Incomplete {
                missing, exit_code, ..
            } => Error::Incomplete {
                missing,
                exit_code,
                stderr: String::from_utf8_lossy(&finished.stderr).into_owned(),
            },
            other => other,
        })?;
        record.exclude_dynamic_sections = self.config.exclude_dynamic_sections;
        record.killed_after_result = finished.killed_after_result;
        record.pipes_held_after_exit = finished.pipes_held_after_exit;
        Ok(record)
    }

    /// The session transcript Claude Code kept for `record` in the config directory
    /// (`projects/<cwd slug>/<session_id>.jsonl`, [`project_slug`]), for the owner's V9 reading; `None` when there
    /// is none (e.g. `session-persistence` is off). When the slug of the reported working directory does not name
    /// it (a Claude Code version that shortens long names), the project directories are searched; the stable work
    /// directories keep them to about one per work slot.
    ///
    /// # Errors
    ///
    /// The record's session id is not a plain id, or the projects directory cannot be read.
    pub fn transcript_path(&self, record: &CallRecord) -> Result<Option<PathBuf>, Error> {
        let session_id = record.session_id.as_str();
        let plain = !session_id.is_empty()
            && session_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !plain {
            return Err(Error::Request(format!(
                "{session_id:?} is not a session id"
            )));
        }
        let projects = self.config.config_dir.join("projects");
        let file = format!("{session_id}.jsonl");
        if let Some(cwd) = &record.cwd {
            let direct = projects.join(project_slug(cwd)).join(&file);
            if direct.is_file() {
                return Ok(Some(direct));
            }
        }
        let entries = match fs::read_dir(&projects) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(format!("reading {}", projects.display()))(e)),
        };
        for entry in entries {
            let candidate = entry
                .map_err(Error::io(format!("reading {}", projects.display())))?
                .path()
                .join(&file);
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// The child command: the native executable, `args`, the working directory `cwd`, an environment built from
    /// nothing: the parent's variables that [`is_passed_env`] allows, then `CLAUDE_CONFIG_DIR`, auto-update off
    /// (`DISABLE_AUTOUPDATER=1`, [90 §8.3] item (4)), auto-memory off (`CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`) and the
    /// configured `env`.
    fn command(&self, cwd: &Path, args: &[OsString]) -> Command {
        let mut command = Command::new(&self.exe);
        command.args(args).current_dir(cwd).env_clear();
        for (name, value) in std::env::vars_os() {
            if is_passed_env(&name) {
                command.env(name, value);
            }
        }
        command
            .env("CLAUDE_CONFIG_DIR", &self.config.config_dir)
            .env("DISABLE_AUTOUPDATER", "1")
            .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1");
        for (name, value) in &self.config.env {
            command.env(name, value);
        }
        command
    }
}

fn tool_list(tools: &[&str]) -> Result<String, Error> {
    for tool in tools {
        let ok = !tool.is_empty()
            && !tool.starts_with('-')
            && !tool.contains(',')
            && !tool.contains(char::is_control);
        if !ok {
            return Err(Error::Request(format!("{tool:?} is not a tool name")));
        }
    }
    Ok(tools.join(","))
}

fn input_file(what: &str, path: &Path) -> Result<PathBuf, Error> {
    let absolute =
        std::path::absolute(path).map_err(Error::io(format!("resolving {}", path.display())))?;
    if absolute.is_file() {
        Ok(absolute)
    } else {
        Err(Error::Request(format!(
            "{what} {} is not a file",
            absolute.display()
        )))
    }
}

/// Removes `dir` with its contents when it exists, and creates it empty.
fn fresh_dir(dir: &Path) -> io::Result<()> {
    if let Err(e) = fs::remove_dir_all(dir)
        && e.kind() != io::ErrorKind::NotFound
    {
        return Err(e);
    }
    fs::create_dir(dir)
}

/// Removes `dir` with its contents, trying again for a moment while the OS still holds it (a killed process whose
/// handles are being closed, a scanner reading a file).
fn remove_dir(dir: &Path) {
    for attempt in 0..REMOVE_TRIES {
        match fs::remove_dir_all(dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound && attempt + 1 < REMOVE_TRIES => {
                thread::sleep(REMOVE_PAUSE);
            }
            _ => return,
        }
    }
}

/// The working directory of one call: `<scratch-root>/work-<slot>`, owned through an exclusive lock on
/// `<scratch-root>/work-<slot>.lock` for the call's duration (a lock the OS drops when a crashed runner's process
/// ends), emptied before the call and removed after it, with `<scratch-root>/home-<slot>` as the empty home of an
/// isolation call. The lowest free slot is taken, so sequential calls always run in the same directory.
struct WorkDir {
    path: PathBuf,
    home: PathBuf,
    lock: File,
}

impl WorkDir {
    fn acquire(root: &Path, slots: u32) -> Result<Self, Error> {
        let mut last_error = None;
        for slot in 0..slots {
            let lock_path = root.join(format!("work-{slot}.lock"));
            let lock = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .map_err(Error::io(format!("opening {}", lock_path.display())))?;
            match lock.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => continue,
                Err(TryLockError::Error(e)) => {
                    return Err(Error::io(format!("locking {}", lock_path.display()))(e));
                }
            }
            let work = Self {
                path: root.join(format!("work-{slot}")),
                home: root.join(format!("home-{slot}")),
                lock,
            };
            // A process left from an earlier call can still hold the directory; the next slot is then taken.
            match fresh_dir(&work.path) {
                Ok(()) => return Ok(work),
                Err(e) => last_error = Some((work.path.clone(), e)),
            }
        }
        match last_error {
            Some((path, e)) => Err(Error::io(format!("emptying {}", path.display()))(e)),
            None => Err(Error::Busy {
                root: root.to_path_buf(),
                slots,
            }),
        }
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        remove_dir(&self.path);
        remove_dir(&self.home);
        // Closing the handle also unlocks, but Windows may release a closed handle's lock late; the next call of
        // this process must find the slot free.
        let _ = self.lock.unlock();
    }
}

enum StdinSource<'a> {
    Bytes(&'a [u8]),
    File(File),
}

/// The limits of one process run.
struct Limits {
    /// The wall-clock limit before the result arrives.
    timeout: Duration,
    /// The longest stdout line, terminator included.
    max_line: usize,
    /// How long the process may run on once `on_line` reported [`Progress::Complete`].
    post_result_grace: Duration,
}

/// What `on_line` tells the wait loop after a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Progress {
    /// The stream goes on.
    Streaming,
    /// The stream's final message has arrived: the process now gets the post-result grace to exit.
    Complete,
}

struct Finished {
    /// The exit status; `None` when the process tree was killed after the result.
    status: Option<ExitStatus>,
    /// The process lingered past the post-result grace and was killed with its tree.
    killed_after_result: bool,
    /// The process exited while a descendant still held its stdout.
    pipes_held_after_exit: bool,
    stderr: Vec<u8>,
}

/// Kills the child's process tree and reaps the child unless it was reaped already. While it lives it holds the
/// child's process handle, so the child's id is not reused even after the child exited.
struct Reaper {
    child: Child,
    reaped: bool,
}

impl Reaper {
    fn try_wait(&mut self) -> Result<Option<ExitStatus>, Error> {
        let status = self
            .child
            .try_wait()
            .map_err(Error::io("waiting for claude"))?;
        self.reaped |= status.is_some();
        Ok(status)
    }

    fn kill(&mut self) {
        if !self.reaped {
            kill_descendants(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Ends the running process `pid` with its descendants (MCP servers, tool processes), which would otherwise
/// outlive it and hold its stdout and stderr open: `taskkill /T /F` on Windows, chosen at run time; elsewhere a
/// `ps` listing walked from `pid` and one `kill -KILL`. Best effort: when it fails, the caller's kill of the process
/// itself remains. It needs `pid` alive and unreaped, so the tree is still linked and the id cannot be reused.
fn kill_descendants(pid: u32) {
    if std::env::consts::OS == "windows" {
        quiet(
            Command::new(system32("taskkill.exe"))
                .args(["/T", "/F", "/PID"])
                .arg(pid.to_string()),
        );
    } else {
        let Ok(listing) = Command::new("ps")
            .args(["-A", "-o", "pid=", "-o", "ppid="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            return;
        };
        let pids = descendants(pid, &ps_pairs(&String::from_utf8_lossy(&listing.stdout)));
        if !pids.is_empty() {
            quiet(
                Command::new("kill")
                    .arg("-KILL")
                    .args(pids.iter().map(u32::to_string)),
            );
        }
    }
}

/// The Windows PowerShell script that lists every process as `pid ppid creation`, the creation time as a UTC
/// FILETIME (100 ns units since 1601).
const CIM_LISTING: &str = "Get-CimInstance Win32_Process | ForEach-Object { if ($_.CreationDate) { '{0} {1} {2}' -f \
                           $_.ProcessId, $_.ParentProcessId, $_.CreationDate.ToFileTimeUtc() } }";

/// Ends the descendants of the exited process `pid` that still run (an MCP server that holds its stdout, say),
/// which would otherwise hold the pipes and, on Windows, the work directory they run in. On Windows a process keeps
/// its parent's id after the parent exits, so the tree is still there to walk: the processes are listed with their
/// parents and creation times (`Get-CimInstance Win32_Process` through Windows PowerShell, about a second, and only
/// on this rare path), the tree is walked from `pid` over the processes created at or after `since` (the spawn; an
/// older process that names `pid` as its parent had an earlier holder of that id as its parent), and each is ended
/// with `taskkill /T /F`. The caller holds the exited process's handle, so `pid` cannot be reused meanwhile; a
/// descendant that ends between the listing and the kill frees its own id for those few milliseconds, the residual
/// risk of doing this without a job object (which needs the Win32 API, outside this crate's dependencies).
/// Elsewhere the exited process's children were handed to another parent at its exit, so the tree cannot be found
/// any more; they are left to end on their own. Best effort.
fn kill_survivors(pid: u32, since: SystemTime) {
    if std::env::consts::OS != "windows" {
        return;
    }
    let powershell = system32("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let Ok(listing) = Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            CIM_LISTING,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    let pairs = cim_pairs(&String::from_utf8_lossy(&listing.stdout), filetime(since));
    let pids = descendants(pid, &pairs);
    if !pids.is_empty() {
        let mut taskkill = Command::new(system32("taskkill.exe"));
        taskkill.args(["/T", "/F"]);
        for survivor in pids {
            taskkill.arg("/PID").arg(survivor.to_string());
        }
        quiet(&mut taskkill);
    }
}

/// `%SystemRoot%\System32\<name>` (`C:\Windows` when the variable is unset).
fn system32(name: &str) -> PathBuf {
    std::env::var_os("SystemRoot")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
        .join("System32")
        .join(name)
}

/// Runs `command` to its end with no input and its output discarded.
fn quiet(command: &mut Command) {
    let _ = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// `time` as a Windows FILETIME (100 ns units since 1601-01-01 UTC), rounded down to the microsecond, the precision
/// of `Win32_Process.CreationDate`.
fn filetime(time: SystemTime) -> u64 {
    const UNIX_EPOCH_AS_FILETIME: u64 = 116_444_736_000_000_000;
    let ticks = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() / 100);
    u64::try_from(ticks)
        .unwrap_or(u64::MAX)
        .saturating_add(UNIX_EPOCH_AS_FILETIME)
        / 10
        * 10
}

/// The `(pid, ppid)` pairs of a `ps -o pid= -o ppid=` listing; other lines are skipped.
fn ps_pairs(listing: &str) -> Vec<(u32, u32)> {
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            Some((pid, ppid))
        })
        .collect()
}

/// The `(pid, ppid)` pairs of a [`CIM_LISTING`] output whose creation time is at or after `since` (a FILETIME);
/// other lines are skipped.
fn cim_pairs(listing: &str, since: u64) -> Vec<(u32, u32)> {
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let created: u64 = fields.next()?.parse().ok()?;
            (created >= since).then_some((pid, ppid))
        })
        .collect()
}

/// The descendants of `root` among `(pid, ppid)` pairs, parents first, each once, `root` itself never.
fn descendants(root: u32, pairs: &[(u32, u32)]) -> Vec<u32> {
    let mut found: Vec<u32> = Vec::new();
    let mut next = 0;
    let mut parent = root;
    loop {
        for &(pid, ppid) in pairs {
            if ppid == parent && pid != root && !found.contains(&pid) {
                found.push(pid);
            }
        }
        let Some(&child) = found.get(next) else {
            return found;
        };
        parent = child;
        next += 1;
    }
}

enum PumpError {
    TooLong,
    Io(io::Error),
}

type LineResult = Result<Vec<u8>, PumpError>;

/// The child's stdout lines, read on a thread of their own into [`LINE_BUFFERS`] circulating buffers.
struct Lines {
    rx: Receiver<LineResult>,
    free: SyncSender<Vec<u8>>,
}

impl Lines {
    fn start(stdout: impl Read + Send + 'static, max_line: usize) -> Result<Self, Error> {
        let (line_tx, rx) = mpsc::sync_channel(LINE_BUFFERS);
        let (free, free_rx) = mpsc::sync_channel(LINE_BUFFERS);
        for _ in 0..LINE_BUFFERS {
            let _ = free.send(Vec::new());
        }
        spawn("claude-stdout", move || {
            pump_lines(stdout, READ_BUF, max_line, &line_tx, &free_rx);
        })?;
        Ok(Self { rx, free })
    }

    /// Hands a line's buffer back to the reader.
    fn recycle(&self, mut line: Vec<u8>) {
        line.clear();
        line.shrink_to(KEEP_CAPACITY);
        let _ = self.free.send(line);
    }

    /// Discards lines until stdout closes, for at most `wait`; whether it closed.
    fn drain_until_closed(&self, wait: Duration) -> bool {
        let end = Instant::now() + wait;
        loop {
            match self
                .rx
                .recv_timeout(end.saturating_duration_since(Instant::now()))
            {
                Ok(Ok(line)) => self.recycle(line),
                Ok(Err(_)) => {}
                Err(RecvTimeoutError::Disconnected) => return true,
                Err(RecvTimeoutError::Timeout) => return false,
            }
        }
    }
}

/// Kills the process tree and waits (up to [`EXIT_GRACE`]) for stdout to close, which it does once every process
/// that held it is gone: the work directory is then free again.
fn stop(reaper: &mut Reaper, lines: &Lines) {
    reaper.kill();
    lines.drain_until_closed(EXIT_GRACE);
}

/// The head of the child's stderr, filled by a reader thread and readable before that thread ends (a descendant may
/// hold the pipe open).
struct StderrHead {
    head: Arc<Mutex<Vec<u8>>>,
    done: Receiver<()>,
}

impl StderrHead {
    fn start(stderr: ChildStderr) -> Result<Self, Error> {
        let head = Arc::new(Mutex::new(Vec::new()));
        let (done_tx, done) = mpsc::sync_channel(1);
        let shared = Arc::clone(&head);
        spawn("claude-stderr", move || {
            read_head(stderr, STDERR_KEEP, &shared);
            let _ = done_tx.send(());
        })?;
        Ok(Self { head, done })
    }

    /// Waits up to `wait` for stderr to close, then takes what was read.
    fn take(&self, wait: Duration) -> Vec<u8> {
        let _ = self.done.recv_timeout(wait);
        std::mem::take(&mut *self.head.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn take_text(&self, wait: Duration) -> String {
        String::from_utf8_lossy(&self.take(wait)).into_owned()
    }
}

/// Runs `command` with piped stdio, feeding `stdin` and handing each stdout line (terminator included) to
/// `on_line`, until the process exits and its stdout closes.
///
/// - Before `on_line` reports [`Progress::Complete`], the run is bounded by `limits.timeout`: the process tree is
///   then killed and the run fails with [`Error::Timeout`].
/// - From the first [`Progress::Complete`] on, the process gets `limits.post_result_grace` to exit, whatever time
///   is left of the timeout; beyond it the tree is killed and the run ends with no status and `killed_after_result`
///   (the stream is complete; lines not read before the kill are dropped).
/// - When the process has exited but a descendant still holds stdout, the runner waits [`EXIT_GRACE`] after the
///   exit, then records `pipes_held_after_exit` and ends the descendants still running ([`kill_survivors`]).
/// - On an over-long line and on any error `on_line` returns, the tree is killed.
///
/// After every kill the runner waits up to [`EXIT_GRACE`] for stdout to close, so the killed processes have let go
/// of the work directory.
fn run_child(
    mut command: Command,
    stdin: Option<StdinSource<'_>>,
    limits: &Limits,
    on_line: &mut dyn FnMut(&[u8]) -> Result<Progress, Error>,
) -> Result<Finished, Error> {
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let spawned_at = SystemTime::now();
    let deadline = Instant::now() + limits.timeout;
    let mut child = command.spawn().map_err(Error::io("starting claude"))?;
    let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let mut reaper = Reaper {
        child,
        reaped: false,
    };
    let (Some(stdout), Some(stderr)) = (pipes.1, pipes.2) else {
        return Err(Error::Protocol(
            "the child has no stdout or stderr pipe".to_owned(),
        ));
    };
    let lines = Lines::start(stdout, limits.max_line)?;
    let stderr = StderrHead::start(stderr)?;
    let fed = match (pipes.0, stdin) {
        (Some(pipe), Some(source)) => Some(feed(pipe, source)?),
        _ => None,
    };
    // Kills the tree once the phase's limit passed with the process still running: a complete stream ends
    // without a status, an incomplete one in a timeout.
    let expire = |reaper: &mut Reaper, complete: bool| -> Result<Finished, Error> {
        stop(reaper, &lines);
        if complete {
            Ok(Finished {
                status: None,
                killed_after_result: true,
                pipes_held_after_exit: false,
                stderr: stderr.take(EXIT_GRACE),
            })
        } else {
            Err(Error::Timeout {
                after: limits.timeout,
                stderr: stderr.take_text(EXIT_GRACE),
            })
        }
    };

    let mut complete_at: Option<Instant> = None;
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut pipes_held = false;
    let running_limit = |complete_at: Option<Instant>| {
        complete_at.map_or(deadline, |at| at + limits.post_result_grace)
    };
    loop {
        let now = Instant::now();
        let limit = match exited {
            Some((_, at)) => at + EXIT_GRACE,
            None => running_limit(complete_at),
        };
        if now >= limit {
            if exited.is_some() {
                pipes_held = true;
                break;
            }
            if let Some(status) = reaper.try_wait()? {
                exited = Some((status, now));
                continue;
            }
            return expire(&mut reaper, complete_at.is_some());
        }
        match lines.rx.recv_timeout((limit - now).min(POLL)) {
            Ok(Ok(line)) => {
                let progress = on_line(&line);
                lines.recycle(line);
                match progress {
                    Ok(Progress::Complete) if complete_at.is_none() => {
                        complete_at = Some(Instant::now());
                    }
                    Ok(_) => {}
                    Err(e) => {
                        stop(&mut reaper, &lines);
                        return Err(e);
                    }
                }
            }
            Ok(Err(e)) => {
                stop(&mut reaper, &lines);
                return Err(match e {
                    PumpError::TooLong => Error::LineTooLong {
                        limit: limits.max_line,
                    },
                    PumpError::Io(e) => Error::io("reading claude's stdout")(e),
                });
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if exited.is_none() {
                    exited = reaper.try_wait()?.map(|status| (status, now));
                }
            }
        }
    }
    let (status, exited_at) = match exited {
        Some(exit) => exit,
        // Stdout closed while the process runs on: wait for its exit within the phase's limit.
        None => loop {
            if let Some(status) = reaper.try_wait()? {
                break (status, Instant::now());
            }
            let now = Instant::now();
            let limit = running_limit(complete_at);
            if now >= limit {
                return expire(&mut reaper, complete_at.is_some());
            }
            thread::sleep((limit - now).min(POLL));
        },
    };
    if pipes_held {
        kill_survivors(reaper.child.id(), spawned_at);
        lines.drain_until_closed(EXIT_GRACE);
    }
    let grace_end = exited_at + EXIT_GRACE;
    if let Some(Ok(Err(e))) =
        fed.map(|rx| rx.recv_timeout(grace_end.saturating_duration_since(Instant::now())))
    {
        return Err(Error::io("writing the prompt to claude's stdin")(e));
    }
    let stderr = stderr.take(grace_end.saturating_duration_since(Instant::now()));
    Ok(Finished {
        status: Some(status),
        killed_after_result: false,
        pipes_held_after_exit: pipes_held,
        stderr,
    })
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> Result<(), Error> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(drop)
        .map_err(Error::io(format!("starting the {name} thread")))
}

/// Reads `reader` through a buffer of `capacity` bytes and sends each line (terminator included; the last line also
/// without one) until end of file, filling the buffers that come back on `free` (emptied first); stops at the first
/// line over `max` bytes, which is never held whole.
fn pump_lines<R: Read>(
    reader: R,
    capacity: usize,
    max: usize,
    tx: &SyncSender<LineResult>,
    free: &Receiver<Vec<u8>>,
) {
    let mut reader = BufReader::with_capacity(capacity, reader);
    let Ok(mut line) = free.recv() else {
        return;
    };
    line.clear();
    loop {
        let buf = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let _ = tx.send(Err(PumpError::Io(e)));
                return;
            }
        };
        if buf.is_empty() {
            if !line.is_empty() {
                let _ = tx.send(Ok(line));
            }
            return;
        }
        let (take, complete) = buf
            .iter()
            .position(|&b| b == b'\n')
            .map_or((buf.len(), false), |i| (i + 1, true));
        if line.len() + take > max {
            let _ = tx.send(Err(PumpError::TooLong));
            return;
        }
        line.extend_from_slice(&buf[..take]);
        reader.consume(take);
        if complete {
            if tx.send(Ok(line)).is_err() {
                return;
            }
            line = match free.recv() {
                Ok(buf) => buf,
                Err(_) => return,
            };
            line.clear();
        }
    }
}

/// Reads stderr to its end, keeping the first `keep` bytes in `head`.
fn read_head(mut stderr: ChildStderr, keep: usize, head: &Mutex<Vec<u8>>) {
    let mut chunk = [0u8; 8192];
    loop {
        match stderr.read(&mut chunk) {
            Ok(0) => return,
            Ok(n) => {
                let mut head = head.lock().unwrap_or_else(PoisonError::into_inner);
                let room = keep.saturating_sub(head.len());
                head.extend_from_slice(&chunk[..n.min(room)]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// Starts writing the prompt to stdin on a thread of its own, which closes stdin when done; the receiver gets the
/// outcome. A process that exits before reading all of the prompt is not an error here: its stream and exit code
/// tell what happened.
fn feed(mut pipe: ChildStdin, source: StdinSource<'_>) -> Result<Receiver<io::Result<()>>, Error> {
    let (tx, rx) = mpsc::sync_channel(1);
    let source = match source {
        StdinSource::Bytes(bytes) => OwnedStdin::Bytes(bytes.to_vec()),
        StdinSource::File(file) => OwnedStdin::File(file),
    };
    spawn("claude-stdin", move || {
        let written = match source {
            OwnedStdin::Bytes(bytes) => pipe.write_all(&bytes),
            OwnedStdin::File(mut file) => io::copy(&mut file, &mut pipe).map(drop),
        };
        drop(pipe);
        let _ = tx.send(match written {
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            other => other,
        });
    })?;
    Ok(rx)
}

enum OwnedStdin {
    Bytes(Vec<u8>),
    File(File),
}

#[cfg(test)]
mod tests {
    use super::{
        BuiltinTools, INSTALLED_PLUGINS, LINE_BUFFERS, MANAGED_POLICY_FILES, PumpError, Request,
        cim_pairs, config_dir_hazard, descendants, filetime, is_passed_env, managed_policy_dirs,
        project_slug, ps_pairs, pump_lines, scratch_hazard, tool_list,
    };
    use crate::claude::Error;
    use proptest::prelude::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io::{self, Read};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, UNIX_EPOCH};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("moirai-tokcount-run-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_environment_is_an_allow_list() {
        for name in [
            "PATH",
            "Path",
            "SystemRoot",
            "SYSTEMROOT",
            "windir",
            "USERPROFILE",
            "HOME",
            "TEMP",
            "LANG",
            "LC_ALL",
            "lc_ctype",
            "https_proxy",
            "ProgramFiles(x86)",
        ] {
            assert!(is_passed_env(OsStr::new(name)), "{name}");
        }
        for name in [
            "ANTHROPIC_API_KEY",
            "anthropic_base_url",
            "CLAUDECODE",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CONFIG_DIR",
            "MCP_TIMEOUT",
            "MAX_THINKING_TOKENS",
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "ENABLE_TOOL_SEARCH",
            "DISABLE_PROMPT_CACHING",
            "NODE_OPTIONS",
            "DISABLE_AUTOUPDATER",
            "LC_",
            "PATHS",
            "",
        ] {
            assert!(!is_passed_env(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn tool_names() {
        assert_eq!(
            tool_list(&["mcp__bench__moirai_q", "Read"]).unwrap(),
            "mcp__bench__moirai_q,Read"
        );
        assert_eq!(tool_list(&[]).unwrap(), "");
        for bad in ["", "-p", "a,b", "a\nb"] {
            assert!(
                matches!(tool_list(&[bad]), Err(Error::Request(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_new_request_is_one_turn_without_tools() {
        let r = Request::new(super::Prompt::Text("x"));
        assert_eq!(r.max_turns, 1);
        assert!(matches!(r.builtin_tools, BuiltinTools::None));
        assert!(r.mcp_config_file.is_none() && r.append_system_file.is_none());
    }

    #[test]
    fn managed_policy_locations_per_os() {
        let set = |name: &str| match name {
            "ProgramFiles" => Some(OsString::from(r"E:\Apps")),
            _ => None,
        };
        assert_eq!(
            managed_policy_dirs("windows", set),
            [
                PathBuf::from(r"E:\Apps").join("ClaudeCode"),
                PathBuf::from(r"C:\ProgramData").join("ClaudeCode"),
            ]
        );
        assert_eq!(
            managed_policy_dirs("macos", |_| None),
            [PathBuf::from("/Library/Application Support/ClaudeCode")]
        );
        assert_eq!(
            managed_policy_dirs("linux", |_| None),
            [PathBuf::from("/etc/claude-code")]
        );
        assert!(MANAGED_POLICY_FILES.contains(&"CLAUDE.md"));
        assert!(MANAGED_POLICY_FILES.contains(&"managed-settings.json"));
    }

    #[test]
    fn project_slugs() {
        assert_eq!(
            project_slug(r"D:\runner\scratch\work-0"),
            "D--runner-scratch-work-0"
        );
        assert_eq!(
            project_slug("/runner/scratch/work-0"),
            "-runner-scratch-work-0"
        );
        assert_eq!(project_slug("/r/Ärger_1.x"), "-r--rger-1-x");
    }

    #[test]
    fn process_tree_walk() {
        let listing = "  1     0\n 100     1\n 101   100\n 102   101\n 103   100\n 200     1\n garbage\n 104   103\n";
        let pairs = ps_pairs(listing);
        assert_eq!(pairs.len(), 7);
        assert_eq!(descendants(100, &pairs), [101, 103, 102, 104]);
        assert_eq!(descendants(200, &pairs), Vec::<u32>::new());
        assert_eq!(descendants(999, &pairs), Vec::<u32>::new());
    }

    #[test]
    fn the_windows_listing_keeps_processes_created_since_the_spawn() {
        // 100 exited; 101 and 102 are its children, 90 an older process whose parent was an earlier holder of 100.
        let listing = "4 0 133000000000000000\n90 100 133000000000000000\n101 100 133000000000000010\n\
                       102 101 133000000000000020\nbad line\n103 102\n";
        let pairs = cim_pairs(listing, 133_000_000_000_000_010);
        assert_eq!(pairs, [(101, 100), (102, 101)]);
        assert_eq!(descendants(100, &pairs), [101, 102]);
        // 1601-01-01 is 0; the Unix epoch is 11,644,473,600 s later; sub-microsecond parts are dropped.
        assert_eq!(filetime(UNIX_EPOCH), 116_444_736_000_000_000);
        assert_eq!(
            filetime(UNIX_EPOCH + Duration::from_nanos(1_234_567)),
            116_444_736_000_012_340
        );
    }

    #[test]
    fn config_dir_bookkeeping_is_not_a_definition() {
        let tmp = TempDir::new("cfg");
        let cfg = &tmp.0;
        for dir in [
            "agents",
            "commands/sub",
            "rules",
            "output-styles",
            "skills/empty",
        ] {
            fs::create_dir_all(cfg.join(dir)).unwrap();
        }
        fs::write(cfg.join("skills").join("empty").join("notes.txt"), "x").unwrap();
        fs::write(cfg.join("commands").join("README.txt"), "x").unwrap();
        let clone = cfg.join("plugins/marketplaces/official/plugins/p/agents");
        fs::create_dir_all(&clone).unwrap();
        fs::write(clone.join("reviewer.md"), "a marketplace clone").unwrap();
        fs::write(
            cfg.join("plugins").join("known_marketplaces.json"),
            "{\"official\":{}}",
        )
        .unwrap();
        let installed = cfg.join("plugins").join("installed_plugins.json");
        for empty in [
            "",
            " \n",
            "{\"version\":2,\"plugins\":{}}",
            "{\"plugins\":[]}",
            "{\"version\":2}",
        ] {
            fs::write(&installed, empty).unwrap();
            assert_eq!(config_dir_hazard(cfg), None, "{empty:?}");
        }
        for listing in [
            "{\"version\":2,\"plugins\":{\"p@official\":[]}}",
            "[]",
            "not json",
            "{\"plugins\":7}",
        ] {
            fs::write(&installed, listing).unwrap();
            let hazard = config_dir_hazard(cfg).unwrap();
            assert_eq!(hazard.entry, INSTALLED_PLUGINS, "{listing:?}");
        }
        fs::remove_file(&installed).unwrap();
        assert_eq!(config_dir_hazard(cfg), None);

        let refused = [
            ("agents/reviewer.md", "agents"),
            ("commands/sub/deploy.MD", "commands"),
            ("rules/style.md", "rules"),
            ("output-styles/terse.md", "output-styles"),
            ("skills/empty/SKILL.md", "skills"),
            ("CLAUDE.md", "CLAUDE.md"),
            ("CLAUDE.local.md", "CLAUDE.local.md"),
        ];
        for (file, entry) in refused {
            let path = cfg.join(file);
            fs::write(&path, "definition").unwrap();
            let hazard = config_dir_hazard(cfg).unwrap();
            assert_eq!(
                (hazard.entry, hazard.path.as_path()),
                (entry, path.as_path())
            );
            fs::remove_file(&path).unwrap();
        }
        // A skill directory's other Markdown files are not skills.
        fs::write(cfg.join("skills").join("empty").join("README.md"), "x").unwrap();
        assert_eq!(config_dir_hazard(cfg), None);
        // Nesting beyond the searched depth counts as a definition.
        let deep: PathBuf = ["rules", "1", "2", "3", "4", "5", "6", "7", "8"]
            .iter()
            .collect();
        fs::create_dir_all(cfg.join(&deep)).unwrap();
        assert_eq!(config_dir_hazard(cfg).unwrap().entry, "rules");
    }

    #[test]
    fn a_scratch_root_is_checked_before_it_exists() {
        // The nearest hazards are inside the temporary directory, so one above it (a user profile) is never reached.
        let tmp = TempDir::new("scratch");
        let base = &tmp.0;
        fs::create_dir(base.join("repo")).unwrap();
        fs::create_dir(base.join("repo").join(".git")).unwrap();
        let under_repo = base.join("repo").join("deep").join("scratch");
        let found = scratch_hazard(&under_repo).unwrap();
        assert_eq!(
            (found.entry, found.path),
            (".git", base.join("repo").join(".git"))
        );
        assert!(!base.join("repo").join("deep").exists());
        let named = base.join("free").join(".Claude").join("scratch");
        let found = scratch_hazard(&named).unwrap();
        assert_eq!(
            (found.entry, found.path),
            (".claude", base.join("free").join(".Claude"))
        );
        assert!(!base.join("free").exists());
    }

    /// A reader that returns the input in pieces of the given sizes (cycled), as a pipe does.
    struct Chunked {
        data: Vec<u8>,
        at: usize,
        sizes: Vec<usize>,
        turn: usize,
    }

    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let want = self.sizes[self.turn % self.sizes.len()].max(1);
            self.turn += 1;
            let n = want.min(buf.len()).min(self.data.len() - self.at);
            buf[..n].copy_from_slice(&self.data[self.at..self.at + n]);
            self.at += n;
            Ok(n)
        }
    }

    /// Everything [`pump_lines`] sends for `data`, read in pieces of `sizes` through a buffer of `capacity` bytes:
    /// the lines, and whether it stopped at an over-long line.
    fn pump(data: &[u8], sizes: Vec<usize>, capacity: usize, max: usize) -> (Vec<Vec<u8>>, bool) {
        let (tx, rx) = mpsc::sync_channel(LINE_BUFFERS);
        let (free_tx, free_rx) = mpsc::sync_channel(LINE_BUFFERS);
        for _ in 0..LINE_BUFFERS {
            free_tx.send(Vec::new()).unwrap();
        }
        let reader = Chunked {
            data: data.to_vec(),
            at: 0,
            sizes,
            turn: 0,
        };
        let pumper = std::thread::spawn(move || pump_lines(reader, capacity, max, &tx, &free_rx));
        let mut lines = Vec::new();
        let mut too_long = false;
        for item in rx {
            match item {
                Ok(line) => {
                    lines.push(line.clone());
                    // Handed back as it is: the pump empties every buffer it gets.
                    let _ = free_tx.send(line);
                }
                Err(PumpError::TooLong) => too_long = true,
                Err(PumpError::Io(e)) => panic!("{e}"),
            }
        }
        pumper.join().unwrap();
        (lines, too_long)
    }

    #[test]
    fn line_framing_at_the_limits() {
        // A line spanning many reads of the 64 KiB buffer, then a last line with no line feed.
        let long = [vec![b'x'; 200_000], b"\n".to_vec()].concat();
        let data = [long.clone(), b"tail".to_vec()].concat();
        let (lines, too_long) = pump(&data, vec![65_536, 3, 70_000], super::READ_BUF, 1 << 20);
        assert!(!too_long);
        assert_eq!(lines, [long, b"tail".to_vec()]);
        // Exactly the limit (terminator included) is accepted; one byte more is refused.
        let max = 1024;
        let exact = [vec![b'a'; max - 1], b"\n".to_vec()].concat();
        assert_eq!(
            pump(&exact, vec![100], 64, max),
            (vec![exact.clone()], false)
        );
        let over = [vec![b'a'; max], b"\n".to_vec()].concat();
        assert_eq!(pump(&over, vec![100], 64, max), (Vec::new(), true));
        let exact_unterminated = vec![b'a'; max];
        assert_eq!(
            pump(&exact_unterminated, vec![7], 64, max),
            (vec![exact_unterminated.clone()], false)
        );
        let lines_then_over = [b"ok\n".to_vec(), over].concat();
        assert_eq!(
            pump(&lines_then_over, vec![5], 16, max),
            (vec![b"ok\n".to_vec()], true)
        );
        assert_eq!(pump(b"", vec![1], 16, max), (Vec::new(), false));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// However the reads split the input, the lines are the input's `\n`-terminated pieces, in order, the last
        /// one possibly unterminated, and a line over the limit stops the pump after the lines before it.
        #[test]
        fn line_framing_is_independent_of_the_reads(
            data in proptest::collection::vec(prop_oneof![Just(b'\n'), Just(b'\r'), Just(b'{'), any::<u8>()], 0..600),
            sizes in proptest::collection::vec(1usize..40, 1..8),
            capacity in 1usize..64,
            max in 1usize..80,
        ) {
            let mut expected: Vec<Vec<u8>> = data.split_inclusive(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
            let over = expected.iter().position(|line| line.len() > max);
            if let Some(i) = over {
                expected.truncate(i);
            }
            let (lines, too_long) = pump(&data, sizes, capacity, max);
            prop_assert_eq!(too_long, over.is_some());
            prop_assert_eq!(lines, expected);
        }

        /// The walk finds exactly the transitive children of the root, parents before children, each once.
        #[test]
        fn the_tree_walk_is_the_transitive_closure(
            parents in proptest::collection::vec(0u32..40, 1..40),
            root in 0u32..40,
        ) {
            // Process i + 1 has parent parents[i] (a forest over 0..=40, cycles allowed through the draw).
            let pairs: Vec<(u32, u32)> = parents
                .iter()
                .enumerate()
                .map(|(i, &ppid)| (u32::try_from(i).unwrap() + 1, ppid))
                .collect();
            let found = descendants(root, &pairs);
            let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
            for &(pid, ppid) in &pairs {
                children.entry(ppid).or_default().push(pid);
            }
            let mut closure = BTreeSet::new();
            let mut stack = vec![root];
            while let Some(p) = stack.pop() {
                for &c in children.get(&p).map_or(&[][..], Vec::as_slice) {
                    if c != root && closure.insert(c) {
                        stack.push(c);
                    }
                }
            }
            let unique: BTreeSet<u32> = found.iter().copied().collect();
            prop_assert_eq!(unique.len(), found.len());
            prop_assert_eq!(unique, closure);
            for (i, &pid) in found.iter().enumerate() {
                let ppid = pairs.iter().find(|p| p.0 == pid).unwrap().1;
                prop_assert!(ppid == root || found[..i].contains(&ppid));
            }
        }
    }
}
