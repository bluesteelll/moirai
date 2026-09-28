//! The headless Claude Code invocation (PLAN WP-58) and the parse of the usage it reports.
//!
//! Every `claude -p` call of M0 goes through [`Claude::call`] (PLAN §3.2 item 5): LQ-Bench's runner and generic
//! client (WP-71b), measurement 6's token ratios (WP-54), measurement 20's baseline (WP-56) and the card gate
//! ([LQ/card §7.2]). One call:
//!
//! - runs the **native** executable, never a `.cmd`, `.ps1` or script shim ([`exe`]): the configured path, or the
//!   first native `claude` on `PATH`, or the native installer's `~/.local/bin`;
//! - pins the model with `--model` (default `claude-opus-5-5`, [90 §8.3] plan of record) and refuses a stream whose
//!   `system/init` message or main-loop assistant turn reports another model, so every record carries the model id
//!   Claude Code used; with `claude-code-version` set it also refuses another Claude Code version ([90 §8.3]
//!   item (4));
//! - works in the stable work directory of a free slot under the runner's scratch root (`work-<slot>`, emptied
//!   before and removed after each call), because Claude Code writes the working directory into its system prompt:
//!   a stable one keeps that prompt byte-identical across calls, so its cached part is read, not written again. The
//!   scratch root must lie outside every repository and under no directory that holds Claude Code instructions
//!   (`.git`, `.claude`, `CLAUDE.md`, `CLAUDE.local.md`), checked before anything is created;
//! - runs with the runner-owned `CLAUDE_CONFIG_DIR` (the owner logs in there once, V9), which must hold no
//!   instructions and no rule, skill, agent, command, output-style or installed-plugin definition
//!   ([`CONFIG_DIR_HAZARDS`]; the bookkeeping Claude Code writes itself is accepted), with `--setting-sources`
//!   limited (default: none; with `user`, the runner's `settings.json` may set no `env`, hooks, plugins, output
//!   style or key helper), no managed-policy file in the configured `managed-policy-dirs`, auto-memory off and
//!   auto-update off ([90 §8.3] item (4));
//! - gives the child an environment built from an allow-list of the parent's variables ([`PASSED_ENV`]: system
//!   locations, temporary directories, the user and home, locale, time zone, proxies and certificates) plus the
//!   runner's own and the configured `env`, so no API key (PLAN §6.1 #6), no variable of a surrounding Claude Code
//!   session, no git redirection and no switch that changes the context reaches it;
//! - refuses a session whose `system/init` lists owner context: an agent other than Claude Code's own, a skill, a
//!   plugin, an output style other than `default`, or a memory directory (configuration key `owner-context`, default
//!   `refuse`);
//! - passes `--strict-mcp-config`, with the caller's MCP configuration file or none, and refuses a session in which
//!   a server of that file is not `connected` (configuration key `mcp-failed`, default `refuse`);
//! - sends the prompt on stdin and the appended system text as `--append-system-prompt-file`, never as argv text;
//! - removes the built-in tools with `--tools ""` unless the request asks otherwise (`docs/spec/reviews/a1-A.md` A-M5);
//! - bounds the run with a timeout and every stdout line with a size limit, killing the process tree on either, and
//!   holds at most three stdout lines at once; once the result has arrived, a process that lingers beyond
//!   `post-result-grace-ms` is killed and the complete record kept ([`CallRecord::killed_after_result`]);
//! - reads `--output-format stream-json --verbose` line by line into a [`CallRecord`]: the model id, the Claude Code
//!   version, the working directory, the tools, MCP servers, agents, skills and plugins the session had, and the
//!   reported usage (input, cache-read and cache-write input, output), in total and per model ([90 §8.3]
//!   item (3)).
//!
//! [`text_tokens`] turns two one-turn calls that differ only in a text into that text's Claude token count
//! ([LQ/card §7.2]). [`Claude::isolation_check`] is the mechanical part of the V9 check that no user `CLAUDE.md`
//! or memory text reaches a call.
//!
//! No call is ever made by the tier-`pr` tests: they run `fake-claude` ([`crate::fake`]), which replays hand-written
//! stream fixtures from `testdata/`.
//!
//! # Descendants that outlive Claude Code
//!
//! Claude Code starts MCP servers and tool processes. On a timeout, an over-long line, a refused stream or a kill
//! after the result, the runner ends the whole tree while Claude Code still runs (`taskkill /T /F` on Windows, a
//! `ps` walk elsewhere). When Claude Code exits on its own while a descendant still holds its stdout, the record
//! says [`CallRecord::pipes_held_after_exit`]. On Windows, where a process keeps its parent's id after the parent
//! exits, the runner then finds and ends the descendants still running, so none keeps the work directory. Elsewhere
//! a process whose parent exited has a new parent, the tree cannot be found any more, and such a descendant runs
//! on until it ends by itself (with the runner's stdout and stderr reader threads, which end with it). That residual
//! leak is why a benchmark MCP server (WP-71b) must exit when its stdin closes.
//!
//! # V9 checklist
//!
//! Facts this module relies on that no cited source fixes; the owner confirms them once on the pinned Claude Code
//! version (PLAN V9) and updates the configuration or this crate where they differ:
//!
//! 1. `claude --version` prints the version first on its line; pin it as `claude-code-version`.
//! 2. `--setting-sources ""`, `--tools ""`, `--strict-mcp-config`, `--disable-slash-commands`,
//!    `--append-system-prompt-file`, `--no-session-persistence` and `--max-turns` exist with the meaning used here,
//!    and `-p --output-format stream-json` needs `--verbose`.
//! 3. `--exclude-dynamic-system-prompt-sections` exists and leaves the working directory, the environment section
//!    and the memory paths out of the system prompt; [`text_tokens`] relaxes its working-directory equality on it.
//! 4. `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` turns auto-memory off: the init lists no `memory_paths`.
//! 5. [`DEFAULT_BUILTIN_AGENTS`] are the agents the version defines itself (`builtin-agents` otherwise).
//! 6. The managed-policy directories of [`managed_policy_dirs`] are where the version looks (`managed-policy-dirs`
//!    otherwise).
//! 7. After the one-time `/login`, `config-dir` holds only what [`config_dir_hazard`] accepts (the credentials,
//!    `projects/`, plugin bookkeeping under `plugins/` with an `installed_plugins.json` that lists no plugin); a
//!    call with the default configuration passes the preflight.
//! 8. The init's `mcp_servers[].status` is `connected` for a server that started, and the benchmark server is
//!    connected at init (not `pending`).
//! 9. The call works with the environment allow-list ([`PASSED_ENV`]); a variable the version needs goes into
//!    `env`.
//! 10. `tokcount claude-isolation` passes (difference 0), and the transcript of one real call
//!     (`tokcount claude` prints its path) carries no project or user `CLAUDE.md` and no memory text (PLAN WP-58
//!     acceptance); whether Claude Code writes such text into the transcript at all is noted.
//! 11. The separator Claude Code puts before appended system text: `tokcount claude-delta` on a one-word text gives
//!     its size, which every Claude count of an appended text includes.
//! 12. The stream-json shape of the fixtures in `testdata/` (hand-written, not recorded) matches a real call.

mod config;
pub mod exe;
mod record;
mod run;

pub use config::{
    DEFAULT_MAX_LINE_BYTES, DEFAULT_MODEL, DEFAULT_POST_RESULT_GRACE, DEFAULT_SETTING_SOURCES,
    DEFAULT_TIMEOUT, DEFAULT_WORK_SLOTS, MAX_WORK_SLOTS, RESERVED_ENV, RunnerConfig,
};
pub use record::{
    CallRecord, ContextPolicy, DEFAULT_BUILTIN_AGENTS, McpServer, ModelUsage, StreamChecks,
    StreamParser, Usage, model_matches, text_tokens,
};
pub use run::{
    BuiltinTools, CONFIG_DIR_HAZARDS, Claude, DEFINITION_DEPTH, Hazard, ISOLATION_PROMPT,
    Isolation, MANAGED_POLICY_FILES, PASSED_ENV, PASSED_ENV_PREFIXES, Prompt, Request,
    SCRATCH_HAZARDS, USER_SETTINGS_HAZARDS, config_dir_hazard, is_passed_env, managed_policy_dirs,
    managed_policy_hazard, project_slug, scratch_hazard, user_settings_hazard,
};

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// Everything that can make a headless call fail before, during or after the process runs.
#[derive(Debug)]
pub enum Error {
    /// The runner configuration is invalid: the file, a key or a value.
    Config(String),
    /// No native `claude` executable was found, or the configured one is a shim or not a native image.
    Exe(String),
    /// Claude Code would load instructions, rules, settings or memory from `path`, or treat it as part of a
    /// repository.
    Context {
        /// The file or directory found.
        path: PathBuf,
        /// Why it disqualifies the directory.
        reason: &'static str,
    },
    /// The request is invalid: a missing file, a bad tool name, a zero turn limit.
    Request(String),
    /// An I/O operation failed.
    Io {
        /// What the runner was doing.
        what: String,
        /// The underlying error.
        source: io::Error,
    },
    /// Every work slot under the scratch root is held by another call (configuration key `work-slots`).
    Busy {
        /// The scratch root.
        root: PathBuf,
        /// The number of slots.
        slots: u32,
    },
    /// The call exceeded the configured timeout before its result arrived; the process tree was killed.
    Timeout {
        /// The configured timeout.
        after: Duration,
        /// The head of the process's stderr.
        stderr: String,
    },
    /// A stdout line exceeded the configured `max-line-bytes`; the process tree was killed.
    LineTooLong {
        /// The configured limit in bytes, line terminator included.
        limit: usize,
    },
    /// The stream ended without the `system/init` or the `result` message.
    Incomplete {
        /// The message that never arrived.
        missing: &'static str,
        /// The process's exit code, when it exited with one.
        exit_code: Option<i32>,
        /// The head of the process's stderr.
        stderr: String,
    },
    /// A message of the stream had an unexpected shape.
    Protocol(String),
    /// Claude Code reported another model than the pinned one, at init or in a main-loop assistant turn; the process
    /// tree was killed.
    ModelMismatch {
        /// The model passed with `--model`.
        pinned: String,
        /// The model reported.
        reported: String,
    },
    /// Claude Code reported another version than the pinned `claude-code-version` ([90 §8.3] item (4)); a call's
    /// process tree was killed.
    VersionMismatch {
        /// The pinned version.
        pinned: String,
        /// The version reported, if any.
        reported: Option<String>,
    },
    /// Claude Code reported an API-key source other than `none`; the runner uses the subscription login only
    /// (PLAN §6.1 #6). The process tree was killed.
    ApiKeySource(String),
    /// The session's `system/init` listed owner context (configuration key `owner-context` is `refuse`): agents
    /// other than the built-in ones, skills, plugins or an output style other than `default`. The process tree was
    /// killed.
    SessionContext(Vec<String>),
    /// A server of the call's `--mcp-config` is not `connected` at init (configuration key `mcp-failed` is
    /// `refuse`): its tools would be missing from the measured context. The entries read `name (status)`. The
    /// process tree was killed.
    McpNotConnected(Vec<String>),
    /// Two call records cannot be compared for a text's token count.
    Delta(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(msg) => write!(f, "runner configuration: {msg}"),
            Self::Exe(msg) => write!(f, "claude executable: {msg}"),
            Self::Context { path, reason } => write!(f, "{}: {reason}", path.display()),
            Self::Request(msg) => write!(f, "request: {msg}"),
            Self::Io { what, source } => write!(f, "{what}: {source}"),
            Self::Busy { root, slots } => write!(
                f,
                "all {slots} work slots under {} are in use (\"work-slots\")",
                root.display()
            ),
            Self::Timeout { after, stderr } => {
                write!(
                    f,
                    "claude did not finish within {} ms and was killed",
                    after.as_millis()
                )?;
                write_stderr(f, stderr)
            }
            Self::LineTooLong { limit } => {
                write!(
                    f,
                    "a stdout line of claude exceeded {limit} bytes (max-line-bytes); the process was killed"
                )
            }
            Self::Incomplete {
                missing,
                exit_code,
                stderr,
            } => {
                write!(f, "claude's stream ended without its {missing} message")?;
                match exit_code {
                    Some(code) => write!(f, " (exit code {code})")?,
                    None => f.write_str(" (no exit code)")?,
                }
                write_stderr(f, stderr)
            }
            Self::Protocol(msg) => write!(f, "claude stream: {msg}"),
            Self::ModelMismatch { pinned, reported } => {
                write!(
                    f,
                    "claude reported model {reported:?}, not the pinned {pinned:?}; the process was killed"
                )
            }
            Self::VersionMismatch { pinned, reported } => match reported {
                Some(reported) => write!(
                    f,
                    "Claude Code reported version {reported:?}, not the pinned {pinned:?} (\"claude-code-version\")"
                ),
                None => write!(
                    f,
                    "Claude Code reported no version, and {pinned:?} is pinned (\"claude-code-version\")"
                ),
            },
            Self::ApiKeySource(source) => write!(
                f,
                "claude reported the API-key source {source:?}; the runner uses the subscription login only \
                 (no API key anywhere), so the process was killed"
            ),
            Self::SessionContext(found) => write!(
                f,
                "the session loaded owner context ({}); the runner's calls carry none (\"owner-context\"), so the \
                 process was killed",
                found.join(", ")
            ),
            Self::McpNotConnected(found) => write!(
                f,
                "MCP servers of --mcp-config did not connect ({}); their tools would be missing from the context \
                 (\"mcp-failed\"), so the process was killed",
                found.join(", ")
            ),
            Self::Delta(msg) => write!(f, "token delta: {msg}"),
        }
    }
}

fn write_stderr(f: &mut fmt::Formatter<'_>, stderr: &str) -> fmt::Result {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        Ok(())
    } else {
        write!(f, "; stderr: {trimmed}")
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Error {
    pub(crate) fn io(what: impl Into<String>) -> impl FnOnce(io::Error) -> Self {
        let what = what.into();
        move |source| Self::Io { what, source }
    }
}
