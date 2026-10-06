//! The parse of `claude -p --output-format stream-json --verbose`: one JSON object per line ([90 §8.3] item (3):
//! "token accounting comes from Claude Code — input, cache-read, cache-write and output tokens as it reports them").
//!
//! Three messages matter; every other line (user turns, hook and status events, rate-limit notices) is read past:
//!
//! - `{"type":"system","subtype":"init",…}` — the session's `model`, `session_id`, `cwd`, `claude_code_version`,
//!   `apiKeySource`, `tools`, `mcp_servers`, `slash_commands`, `agents`, `skills`, `plugins`, `output_style` and,
//!   while auto-memory is on, `memory_paths`. It is checked as soon as it arrives, so a bad session is stopped before
//!   its first request is paid for: the model against the pin, the version against the optional version pin
//!   ([90 §8.3] item (4)), the API-key source (the subscription login only, PLAN §6.1 #6), and the owner context (an
//!   agent outside the built-in ones, a skill, a plugin, a non-default output style or a memory directory: context
//!   the runner's calls may not carry, PLAN WP-58 acceptance "no project or user CLAUDE.md and no memory text"). When
//!   the call passed `--mcp-config`, every MCP server listed must be `connected`: a server that failed would leave
//!   its tool definitions out of the measured context.
//! - `{"type":"assistant",…}` of the main loop (`parent_tool_use_id` null) — its `message.model` is checked against
//!   the pin, so a call that another model served mid-stream is refused; `<synthetic>` (a message Claude Code writes
//!   itself, such as an API error text) is not a model and is skipped.
//! - `{"type":"result","subtype":…,"is_error":…,…}` — the outcome: `num_turns`, `duration_ms`,
//!   `duration_api_ms`, `result` (absent on an error subtype), `errors`, `stop_reason`, `permission_denials`,
//!   `total_cost_usd` (a list-price reference only, PLAN §6.1 #6), `usage` (`input_tokens`,
//!   `cache_read_input_tokens`, `cache_creation_input_tokens`, `output_tokens`) and `modelUsage` (per model id:
//!   `inputTokens`, `cacheReadInputTokens`, `cacheCreationInputTokens`, `outputTokens`, `costUSD`).
//!
//! Only a line that holds one of the byte strings `"init"`, `"result"` or `"assistant"` is parsed into a JSON tree;
//! the others (large tool results above all) are only scanned for those markers. A line whose first and last
//! non-blank bytes are not `{` and `}`, and a parsed line that is not a JSON object, count in
//! [`CallRecord::unparsed_lines`].

use std::fmt;

use serde_json::{Map, Value, json};

use super::Error;

/// Token usage as Claude Code reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Uncached input tokens (`input_tokens`).
    pub input: u64,
    /// Input tokens read from the prompt cache (`cache_read_input_tokens`): the cached input.
    pub cache_read: u64,
    /// Input tokens written to the prompt cache (`cache_creation_input_tokens`).
    pub cache_write: u64,
    /// Output tokens (`output_tokens`).
    pub output: u64,
}

impl Usage {
    /// Every input token the model read: uncached, cache-read and cache-write (`docs/spec/measurement-protocol.md`
    /// §4.10, the token-usage rule). `input` alone leaves out the cached part, so a with/without difference
    /// ([`text_tokens`]) is taken over this sum.
    // spec: [MP §4.10] (agent token usage: input = input_tokens + cache_read_input_tokens + cache_creation_input_tokens)
    #[must_use]
    pub const fn total_input(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    fn parse(obj: &Map<String, Value>, keys: [&str; 4], at: &str) -> Result<Self, Error> {
        let [input, cache_read, cache_write, output] = keys;
        Ok(Self {
            input: tokens(obj, input, true, at)?,
            cache_read: tokens(obj, cache_read, false, at)?,
            cache_write: tokens(obj, cache_write, false, at)?,
            output: tokens(obj, output, true, at)?,
        })
    }

    fn to_json(self) -> Value {
        json!({
            "input": self.input,
            "cache_read": self.cache_read,
            "cache_write": self.cache_write,
            "output": self.output,
            "total_input": self.total_input(),
        })
    }
}

const RESULT_USAGE_KEYS: [&str; 4] = [
    "input_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
    "output_tokens",
];
const MODEL_USAGE_KEYS: [&str; 4] = [
    "inputTokens",
    "cacheReadInputTokens",
    "cacheCreationInputTokens",
    "outputTokens",
];

/// The model id Claude Code gives the assistant messages it writes itself (an API error text, a refusal notice);
/// no model served them.
const SYNTHETIC_MODEL: &str = "<synthetic>";

/// The byte strings that mark a line the parser reads: the init's subtype, the result's type, an assistant turn.
const MARKERS: [&[u8]; 3] = [b"\"init\"", b"\"result\"", b"\"assistant\""];

/// The agents Claude Code itself defines, which a `system/init` message may list without any owner context
/// (the default of the `builtin-agents` configuration key). An agent outside this list comes from a user, project
/// or plugin definition.
pub const DEFAULT_BUILTIN_AGENTS: [&str; 6] = [
    "general-purpose",
    "statusline-setup",
    "output-style-setup",
    "Explore",
    "Plan",
    "claude-code-guide",
];

/// What a session whose `system/init` shows a context other than the runner's gets: owner context (the
/// `owner-context` configuration key) or an MCP server of `--mcp-config` that is not `connected` (the `mcp-failed`
/// key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextPolicy {
    /// The call is refused and the process killed ([`Error::SessionContext`], [`Error::McpNotConnected`]); the
    /// default.
    Refuse,
    /// The call proceeds and the record shows what was found ([`CallRecord::owner_context`],
    /// [`CallRecord::mcp_failures`]).
    Record,
}

impl ContextPolicy {
    /// The configuration spelling: `refuse` or `record`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Record => "record",
        }
    }
}

/// What the parser checks as the stream arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamChecks {
    /// The model passed with `--model`.
    pub model: String,
    /// The Claude Code version every call must report, when pinned ([90 §8.3] item (4)).
    pub claude_code_version: Option<String>,
    /// The agents that do not count as owner context.
    pub builtin_agents: Vec<String>,
    /// What owner context in the init gets.
    pub owner_context: ContextPolicy,
    /// Whether the call passed `--mcp-config` (set per call): every MCP server the init lists must then be
    /// `connected`.
    pub mcp_config: bool,
    /// What an MCP server of `--mcp-config` that is not `connected` gets.
    pub mcp_failed: ContextPolicy,
}

impl StreamChecks {
    /// The checks of a call pinned to `model`: no version pin, [`DEFAULT_BUILTIN_AGENTS`], owner context refused,
    /// no `--mcp-config`, an MCP server that is not connected refused.
    #[must_use]
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_owned(),
            claude_code_version: None,
            builtin_agents: DEFAULT_BUILTIN_AGENTS.map(str::to_owned).to_vec(),
            owner_context: ContextPolicy::Refuse,
            mcp_config: false,
            mcp_failed: ContextPolicy::Refuse,
        }
    }
}

/// One model's share of a call (`modelUsage`); Claude Code may add requests of a smaller model beside the pinned one.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelUsage {
    /// The model id.
    pub model: String,
    /// Its usage.
    pub usage: Usage,
    /// Its list-price cost in dollars as reported (a reference only).
    pub cost_usd: Option<f64>,
}

/// An MCP server of the session, as the `system/init` message lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServer {
    /// The server name.
    pub name: String,
    /// Its status (`connected`, `failed`, …).
    pub status: String,
}

/// Everything one headless call reported, with the model pin it ran under.
#[derive(Clone, Debug, PartialEq)]
pub struct CallRecord {
    /// The model passed with `--model`.
    pub pinned_model: String,
    /// The model the `system/init` message reported; it matches the pin ([`model_matches`]).
    pub model: String,
    /// The Claude Code version the `system/init` message reported.
    pub claude_code_version: Option<String>,
    /// The session id.
    pub session_id: String,
    /// The working directory the `system/init` message reported. The runner keeps it stable across calls, because
    /// Claude Code writes it into the system prompt.
    pub cwd: Option<String>,
    /// Whether the call passed `--exclude-dynamic-system-prompt-sections` (filled by the runner): the working
    /// directory, the environment section and the memory paths are then left out of the system prompt.
    pub exclude_dynamic_sections: bool,
    /// The API-key source reported; `none` (the subscription login) whenever present.
    pub api_key_source: Option<String>,
    /// The tools the session had.
    pub tools: Vec<String>,
    /// The MCP servers the session had.
    pub mcp_servers: Vec<McpServer>,
    /// Whether the call passed `--mcp-config`, so every server in [`CallRecord::mcp_servers`] should be connected.
    pub mcp_config: bool,
    /// The slash commands the session listed.
    pub slash_commands: Vec<String>,
    /// The agents the session listed.
    pub agents: Vec<String>,
    /// The skills the session listed.
    pub skills: Vec<String>,
    /// The plugins the session listed, by name.
    pub plugins: Vec<String>,
    /// The output style the session reported.
    pub output_style: Option<String>,
    /// The owner context the init listed (`agent <name>`, `skill <name>`, `plugin <name>`, `output style <name>`,
    /// `memory <kind>`); always empty under [`ContextPolicy::Refuse`], which refuses such a call.
    pub owner_context: Vec<String>,
    /// The result subtype: `success`, `error_max_turns`, `error_during_execution`, ….
    pub subtype: String,
    /// Whether Claude Code marked the result as an error.
    pub is_error: bool,
    /// The stop reason of the last turn.
    pub stop_reason: Option<String>,
    /// The number of turns.
    pub num_turns: u64,
    /// Wall-clock milliseconds Claude Code reported.
    pub duration_ms: u64,
    /// Milliseconds spent in API requests.
    pub duration_api_ms: u64,
    /// The final text, when the subtype carries one.
    pub result: Option<String>,
    /// The error texts of an error subtype.
    pub errors: Vec<String>,
    /// The session's usage (`usage`).
    pub usage: Usage,
    /// The usage per model (`modelUsage`), sorted by model id.
    pub model_usage: Vec<ModelUsage>,
    /// The session's list-price cost in dollars (a reference only).
    pub total_cost_usd: Option<f64>,
    /// How many tool uses were denied.
    pub permission_denials: u64,
    /// The process's exit code, filled by the runner; `None` when the process tree was killed after the result
    /// ([`CallRecord::killed_after_result`]).
    pub exit_code: Option<i32>,
    /// The process lingered after its `result` message beyond `post-result-grace-ms` and was killed with its tree
    /// (filled by the runner). The stream was complete, so the record stands.
    pub killed_after_result: bool,
    /// The process exited while a descendant (an MCP server, say) still held its stdout (filled by the runner). On
    /// Windows the runner then killed the descendants still running; elsewhere they were left to end on their own.
    pub pipes_held_after_exit: bool,
    /// Lines of the stream that were not JSON objects.
    pub unparsed_lines: u64,
}

impl CallRecord {
    /// The `modelUsage` entry of the pinned model.
    #[must_use]
    pub fn pinned_usage(&self) -> Option<&ModelUsage> {
        self.model_usage
            .iter()
            .find(|m| model_matches(&self.pinned_model, &m.model))
    }

    /// Whether the call succeeded: no error result, and exit code 0 or a process that was killed only after its
    /// complete result.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        !self.is_error && (self.exit_code == Some(0) || self.killed_after_result)
    }

    /// The MCP servers of `--mcp-config` that did not connect, as `name (status)`; empty when the call passed no
    /// `--mcp-config`.
    #[must_use]
    pub fn mcp_failures(&self) -> Vec<String> {
        mcp_failures(self.mcp_config, &self.mcp_servers)
    }

    /// The record as JSON (`moirai-tokcount.claude-call.v1`).
    #[must_use]
    pub fn to_json(&self) -> Value {
        let model_usage: Vec<Value> = self
            .model_usage
            .iter()
            .map(|m| {
                let mut v = m.usage.to_json();
                if let Value::Object(obj) = &mut v {
                    obj.insert("model".to_owned(), Value::String(m.model.clone()));
                    obj.insert(
                        "cost_usd".to_owned(),
                        m.cost_usd.map_or(Value::Null, Value::from),
                    );
                }
                v
            })
            .collect();
        let mcp: Vec<Value> = self
            .mcp_servers
            .iter()
            .map(|s| json!({ "name": s.name, "status": s.status }))
            .collect();
        json!({
            "record": "moirai-tokcount.claude-call.v1",
            "pinned_model": self.pinned_model,
            "model": self.model,
            "claude_code_version": self.claude_code_version,
            "session_id": self.session_id,
            "cwd": self.cwd,
            "exclude_dynamic_sections": self.exclude_dynamic_sections,
            "api_key_source": self.api_key_source,
            "tools": self.tools,
            "mcp_servers": mcp,
            "mcp_config": self.mcp_config,
            "slash_commands": self.slash_commands,
            "agents": self.agents,
            "skills": self.skills,
            "plugins": self.plugins,
            "output_style": self.output_style,
            "owner_context": self.owner_context,
            "subtype": self.subtype,
            "is_error": self.is_error,
            "stop_reason": self.stop_reason,
            "num_turns": self.num_turns,
            "duration_ms": self.duration_ms,
            "duration_api_ms": self.duration_api_ms,
            "result": self.result,
            "errors": self.errors,
            "usage": self.usage.to_json(),
            "model_usage": model_usage,
            "total_cost_usd": self.total_cost_usd,
            "permission_denials": self.permission_denials,
            "exit_code": self.exit_code,
            "killed_after_result": self.killed_after_result,
            "pipes_held_after_exit": self.pipes_held_after_exit,
            "unparsed_lines": self.unparsed_lines,
        })
    }
}

fn mcp_failures(mcp_config: bool, servers: &[McpServer]) -> Vec<String> {
    if !mcp_config {
        return Vec::new();
    }
    servers
        .iter()
        .filter(|s| s.status != "connected")
        .map(|s| format!("{} ({})", s.name, s.status))
        .collect()
}

/// Whether the reported model id is the pinned one: equal, or the pin followed by a date (`-` and exactly 8
/// digits), a bracketed variant (`[1m]`), a date and a variant, or an `@` suffix (`@20261001`). A pin that is a
/// prefix of another model id (`claude-opus-5` of `claude-opus-5-5`) does not match it.
#[must_use]
pub fn model_matches(pinned: &str, reported: &str) -> bool {
    let Some(rest) = reported.strip_prefix(pinned) else {
        return false;
    };
    if let Some(tag) = rest.strip_prefix('@') {
        return !tag.is_empty();
    }
    let rest = match rest.strip_prefix('-') {
        Some(dated) => match dated.as_bytes().get(..8) {
            Some(date) if date.iter().all(u8::is_ascii_digit) => &dated[8..],
            _ => return false,
        },
        None => rest,
    };
    rest.is_empty()
        || (rest.len() > 2
            && rest.starts_with('[')
            && rest.ends_with(']')
            && !rest[1..rest.len() - 1].contains(['[', ']']))
}

/// A text's Claude token count: the pinned model's total input of the call `with` the text minus that of the same
/// call `without` it (PLAN §6.1 #7, [90 §8.3] "tokenizer ledger", [LQ/card §7.2]).
///
/// The two calls must differ in nothing but the text: each is one turn, so one request (the empty benchmark turn of
/// `docs/spec/reviews/a1-A.md` A-M5; every further request would re-read the text), neither is an error, and they
/// report the same pinned and reported model, Claude Code version, tools, MCP servers with their status, slash
/// commands, agents, skills, plugins, output style, recorded owner context and dynamic-section mode, and the same
/// working directory unless the dynamic sections were excluded.
///
/// When the text is appended to the system prompt (`--append-system-prompt-file`, as `tokcount claude-delta` does),
/// the count also holds whatever separator Claude Code puts before appended text (about one token): an over-count
/// that the o200k count of the bare text does not share, and that keeps a "not more than" budget gate on the safe
/// side (to be measured at V9).
///
/// # Errors
///
/// [`Error::Delta`] naming the first condition that fails, or when `with` read fewer input tokens than `without`.
pub fn text_tokens(with: &CallRecord, without: &CallRecord) -> Result<u64, Error> {
    let (a, b) = comparable_inputs((with, "with the text"), (without, "without the text"))?;
    a.checked_sub(b).ok_or_else(|| {
        Error::Delta(format!(
            "the call with the text read {a} input tokens, fewer than the {b} without it"
        ))
    })
}

/// The pinned model's total input of two calls that must differ in nothing but one input (see [`text_tokens`]).
pub(crate) fn comparable_inputs(
    (a, a_name): (&CallRecord, &str),
    (b, b_name): (&CallRecord, &str),
) -> Result<(u64, u64), Error> {
    for (rec, name) in [(a, a_name), (b, b_name)] {
        if rec.is_error {
            return Err(Error::Delta(format!(
                "the call {name} ended in {}",
                rec.subtype
            )));
        }
        if rec.num_turns != 1 {
            return Err(Error::Delta(format!(
                "the call {name} took {} turns; a delta needs one-turn calls, because every further request \
                 re-reads the text",
                rec.num_turns
            )));
        }
    }
    same("pinned_model", &a.pinned_model, &b.pinned_model)?;
    same("model", &a.model, &b.model)?;
    same(
        "claude_code_version",
        &a.claude_code_version,
        &b.claude_code_version,
    )?;
    same("tools", &a.tools, &b.tools)?;
    same("mcp_servers", &a.mcp_servers, &b.mcp_servers)?;
    same("slash_commands", &a.slash_commands, &b.slash_commands)?;
    same("agents", &a.agents, &b.agents)?;
    same("skills", &a.skills, &b.skills)?;
    same("plugins", &a.plugins, &b.plugins)?;
    same("output_style", &a.output_style, &b.output_style)?;
    same("owner_context", &a.owner_context, &b.owner_context)?;
    same(
        "exclude_dynamic_sections",
        &a.exclude_dynamic_sections,
        &b.exclude_dynamic_sections,
    )?;
    if !a.exclude_dynamic_sections {
        same("cwd", &a.cwd, &b.cwd)?;
    }
    let pinned = |rec: &CallRecord, name: &str| {
        rec.pinned_usage()
            .map(|m| m.usage.total_input())
            .ok_or_else(|| {
                Error::Delta(format!(
                    "the call {name} reports no usage for {}",
                    rec.pinned_model
                ))
            })
    };
    Ok((pinned(a, a_name)?, pinned(b, b_name)?))
}

fn same<T: PartialEq + fmt::Debug>(field: &str, a: &T, b: &T) -> Result<(), Error> {
    if a == b {
        Ok(())
    } else {
        Err(Error::Delta(format!(
            "the calls differ in {field}: {a:?} and {b:?}"
        )))
    }
}

#[derive(Debug)]
struct Init {
    model: String,
    session_id: String,
    cwd: Option<String>,
    version: Option<String>,
    api_key_source: Option<String>,
    tools: Vec<String>,
    mcp_servers: Vec<McpServer>,
    slash_commands: Vec<String>,
    agents: Vec<String>,
    skills: Vec<String>,
    plugins: Vec<String>,
    output_style: Option<String>,
    /// The kinds of memory directory the init lists (`auto`, `team`), sorted; present only while auto-memory is on.
    memory: Vec<String>,
    owner_context: Vec<String>,
}

#[derive(Debug)]
struct Outcome {
    session_id: Option<String>,
    subtype: String,
    is_error: bool,
    stop_reason: Option<String>,
    num_turns: u64,
    duration_ms: u64,
    duration_api_ms: u64,
    result: Option<String>,
    errors: Vec<String>,
    usage: Usage,
    model_usage: Vec<ModelUsage>,
    total_cost_usd: Option<f64>,
    permission_denials: u64,
}

/// A line-by-line parser of the stream. It keeps only the init and the result, and parses only the lines that
/// carry a marker, so its memory beyond those two is one parsed line.
#[derive(Debug)]
pub struct StreamParser {
    checks: StreamChecks,
    init: Option<Init>,
    outcome: Option<Outcome>,
    unparsed: u64,
}

impl StreamParser {
    /// A parser that applies `checks`.
    #[must_use]
    pub const fn new(checks: StreamChecks) -> Self {
        Self {
            checks,
            init: None,
            outcome: None,
            unparsed: 0,
        }
    }

    /// Whether the `result` message has arrived: the stream is complete.
    #[must_use]
    pub const fn has_result(&self) -> bool {
        self.outcome.is_some()
    }

    /// Takes one line of the stream, with or without its terminator.
    ///
    /// # Errors
    ///
    /// An init or a main-loop assistant message reports another model ([`Error::ModelMismatch`]); an init reports
    /// another Claude Code version than the pinned one ([`Error::VersionMismatch`]), an API-key source
    /// ([`Error::ApiKeySource`]) or, under [`ContextPolicy::Refuse`], owner context ([`Error::SessionContext`]) or an
    /// MCP server of `--mcp-config` that is not connected ([`Error::McpNotConnected`]); an init or result message is
    /// malformed, or a second result arrives ([`Error::Protocol`]).
    pub fn feed_line(&mut self, line: &[u8]) -> Result<(), Error> {
        let line = line.trim_ascii();
        if line.is_empty() {
            return Ok(());
        }
        if !(line.starts_with(b"{") && line.ends_with(b"}")) {
            self.unparsed += 1;
            return Ok(());
        }
        if !has_marker(line) {
            return Ok(());
        }
        let Ok(Value::Object(obj)) = serde_json::from_slice::<Value>(line) else {
            self.unparsed += 1;
            return Ok(());
        };
        match (text(&obj, "type"), text(&obj, "subtype")) {
            (Some("system"), Some("init")) => self.on_init(Init::parse(&obj)?),
            (Some("assistant"), _) => self.on_assistant(&obj),
            (Some("result"), _) => {
                if self.outcome.is_some() {
                    return Err(Error::Protocol("a second result message".to_owned()));
                }
                self.outcome = Some(Outcome::parse(&obj)?);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn on_init(&mut self, mut init: Init) -> Result<(), Error> {
        let checks = &self.checks;
        if !model_matches(&checks.model, &init.model) {
            return Err(Error::ModelMismatch {
                pinned: checks.model.clone(),
                reported: init.model,
            });
        }
        if let Some(pin) = &checks.claude_code_version
            && init.version.as_ref() != Some(pin)
        {
            return Err(Error::VersionMismatch {
                pinned: pin.clone(),
                reported: init.version,
            });
        }
        if let Some(source) = init.api_key_source.as_deref().filter(|s| *s != "none") {
            return Err(Error::ApiKeySource(source.to_owned()));
        }
        let found = init.owner_context(&checks.builtin_agents);
        if !found.is_empty() && checks.owner_context == ContextPolicy::Refuse {
            return Err(Error::SessionContext(found));
        }
        let unconnected = mcp_failures(checks.mcp_config, &init.mcp_servers);
        if !unconnected.is_empty() && checks.mcp_failed == ContextPolicy::Refuse {
            return Err(Error::McpNotConnected(unconnected));
        }
        init.owner_context = found;
        if self.init.is_none() {
            self.init = Some(init);
        }
        Ok(())
    }

    fn on_assistant(&self, obj: &Map<String, Value>) -> Result<(), Error> {
        let main_loop = obj.get("parent_tool_use_id").is_none_or(Value::is_null);
        let model = obj
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(Value::as_str);
        match model {
            Some(model)
                if main_loop
                    && model != SYNTHETIC_MODEL
                    && !model_matches(&self.checks.model, model) =>
            {
                Err(Error::ModelMismatch {
                    pinned: self.checks.model.clone(),
                    reported: model.to_owned(),
                })
            }
            _ => Ok(()),
        }
    }

    /// Ends the stream and builds the record.
    ///
    /// # Errors
    ///
    /// [`Error::Incomplete`] when the init or the result message never arrived (without the exit code and stderr,
    /// which the runner adds); [`Error::Protocol`] when the two carry different session ids, or when a successful
    /// result of at least one turn reports no `modelUsage` entry for the pinned model.
    pub fn finish(self, exit_code: Option<i32>) -> Result<CallRecord, Error> {
        let incomplete = |missing| Error::Incomplete {
            missing,
            exit_code,
            stderr: String::new(),
        };
        let init = self.init.ok_or_else(|| incomplete("system/init"))?;
        let outcome = self.outcome.ok_or_else(|| incomplete("result"))?;
        if let Some(id) = outcome
            .session_id
            .as_deref()
            .filter(|id| *id != init.session_id)
        {
            return Err(Error::Protocol(format!(
                "the result's session {id} is not the init's {}",
                init.session_id
            )));
        }
        let record = CallRecord {
            pinned_model: self.checks.model,
            model: init.model,
            claude_code_version: init.version,
            session_id: init.session_id,
            cwd: init.cwd,
            exclude_dynamic_sections: false,
            api_key_source: init.api_key_source,
            tools: init.tools,
            mcp_servers: init.mcp_servers,
            mcp_config: self.checks.mcp_config,
            slash_commands: init.slash_commands,
            agents: init.agents,
            skills: init.skills,
            plugins: init.plugins,
            output_style: init.output_style,
            owner_context: init.owner_context,
            subtype: outcome.subtype,
            is_error: outcome.is_error,
            stop_reason: outcome.stop_reason,
            num_turns: outcome.num_turns,
            duration_ms: outcome.duration_ms,
            duration_api_ms: outcome.duration_api_ms,
            result: outcome.result,
            errors: outcome.errors,
            usage: outcome.usage,
            model_usage: outcome.model_usage,
            total_cost_usd: outcome.total_cost_usd,
            permission_denials: outcome.permission_denials,
            exit_code,
            killed_after_result: false,
            pipes_held_after_exit: false,
            unparsed_lines: self.unparsed,
        };
        if !record.is_error && record.num_turns >= 1 && record.pinned_usage().is_none() {
            return Err(Error::Protocol(format!(
                "the successful result reports no modelUsage entry for the pinned {}",
                record.pinned_model
            )));
        }
        Ok(record)
    }
}

/// Whether `line` holds one of [`MARKERS`]; one pass, stopping at each `"`.
fn has_marker(line: &[u8]) -> bool {
    line.iter()
        .enumerate()
        .any(|(i, &b)| b == b'"' && MARKERS.iter().any(|marker| line[i..].starts_with(marker)))
}

impl Init {
    fn parse(obj: &Map<String, Value>) -> Result<Self, Error> {
        let at = "system/init";
        let mcp_servers = match obj.get("mcp_servers") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|s| {
                    let s = s.as_object().ok_or_else(|| malformed(at, "mcp_servers"))?;
                    Ok(McpServer {
                        name: required_text(s, "name", at)?.to_owned(),
                        status: required_text(s, "status", at)?.to_owned(),
                    })
                })
                .collect::<Result<_, Error>>()?,
            Some(_) => return Err(malformed(at, "mcp_servers")),
        };
        let mut memory: Vec<String> = match obj.get("memory_paths") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Object(kinds)) => kinds.keys().cloned().collect(),
            Some(_) => return Err(malformed(at, "memory_paths")),
        };
        memory.sort();
        Ok(Self {
            model: required_text(obj, "model", at)?.to_owned(),
            session_id: required_text(obj, "session_id", at)?.to_owned(),
            cwd: optional_text(obj, "cwd", at)?,
            version: optional_text(obj, "claude_code_version", at)?,
            api_key_source: optional_text(obj, "apiKeySource", at)?,
            tools: names(obj, "tools", at)?,
            mcp_servers,
            slash_commands: names(obj, "slash_commands", at)?,
            agents: names(obj, "agents", at)?,
            skills: names(obj, "skills", at)?,
            plugins: names(obj, "plugins", at)?,
            output_style: optional_text(obj, "output_style", at)?,
            memory,
            owner_context: Vec::new(),
        })
    }

    /// The owner context the init lists: agents outside `builtin_agents`, every skill and plugin, an output style
    /// other than `default`, and every memory directory (auto-memory is on, so memory text can reach the context).
    fn owner_context(&self, builtin_agents: &[String]) -> Vec<String> {
        let agents = self
            .agents
            .iter()
            .filter(|a| !builtin_agents.contains(a))
            .map(|a| format!("agent {a}"));
        let skills = self.skills.iter().map(|s| format!("skill {s}"));
        let plugins = self.plugins.iter().map(|p| format!("plugin {p}"));
        let style = self
            .output_style
            .as_deref()
            .filter(|s| *s != "default")
            .map(|s| format!("output style {s}"));
        let memory = self.memory.iter().map(|m| format!("memory {m}"));
        agents
            .chain(skills)
            .chain(plugins)
            .chain(style)
            .chain(memory)
            .collect()
    }
}

impl Outcome {
    fn parse(obj: &Map<String, Value>) -> Result<Self, Error> {
        let at = "result";
        let is_error = obj
            .get("is_error")
            .and_then(Value::as_bool)
            .ok_or_else(|| malformed(at, "is_error"))?;
        let usage = obj
            .get("usage")
            .and_then(Value::as_object)
            .ok_or_else(|| malformed(at, "usage"))?;
        let mut model_usage: Vec<ModelUsage> = match obj.get("modelUsage") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Object(models)) => models
                .iter()
                .map(|(model, entry)| {
                    let entry = entry
                        .as_object()
                        .ok_or_else(|| malformed(at, "modelUsage"))?;
                    Ok(ModelUsage {
                        model: model.clone(),
                        usage: Usage::parse(entry, MODEL_USAGE_KEYS, "result.modelUsage")?,
                        cost_usd: number(entry, "costUSD", "result.modelUsage")?,
                    })
                })
                .collect::<Result<_, Error>>()?,
            Some(_) => return Err(malformed(at, "modelUsage")),
        };
        // The order must not depend on serde_json's map (a `preserve_order` feature anywhere in the build would
        // change it).
        model_usage.sort_by(|a, b| a.model.cmp(&b.model));
        let errors = match obj.get("errors") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|e| e.as_str().map_or_else(|| e.to_string(), str::to_owned))
                .collect(),
            Some(_) => return Err(malformed(at, "errors")),
        };
        let permission_denials = match obj.get("permission_denials") {
            None | Some(Value::Null) => 0,
            Some(Value::Array(items)) => items.len() as u64,
            Some(_) => return Err(malformed(at, "permission_denials")),
        };
        Ok(Self {
            session_id: optional_text(obj, "session_id", at)?,
            subtype: required_text(obj, "subtype", at)?.to_owned(),
            is_error,
            stop_reason: optional_text(obj, "stop_reason", at)?,
            num_turns: tokens(obj, "num_turns", false, at)?,
            duration_ms: tokens(obj, "duration_ms", false, at)?,
            duration_api_ms: tokens(obj, "duration_api_ms", false, at)?,
            result: optional_text(obj, "result", at)?,
            errors,
            usage: Usage::parse(usage, RESULT_USAGE_KEYS, "result.usage")?,
            model_usage,
            total_cost_usd: number(obj, "total_cost_usd", at)?,
            permission_denials,
        })
    }
}

fn malformed(at: &str, key: &str) -> Error {
    Error::Protocol(format!("{at}: {key:?} is missing or malformed"))
}

fn text<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(Value::as_str)
}

fn required_text<'a>(obj: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a str, Error> {
    text(obj, key).ok_or_else(|| malformed(at, key))
}

fn optional_text(obj: &Map<String, Value>, key: &str, at: &str) -> Result<Option<String>, Error> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(malformed(at, key)),
    }
}

/// A list of names: each item a string, or an object with a string `name` (as `plugins` lists them).
fn names(obj: &Map<String, Value>, key: &str, at: &str) -> Result<Vec<String>, Error> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => Ok(s.clone()),
                Value::Object(o) => text(o, "name")
                    .map(str::to_owned)
                    .ok_or_else(|| malformed(at, key)),
                _ => Err(malformed(at, key)),
            })
            .collect(),
        Some(_) => Err(malformed(at, key)),
    }
}

/// A non-negative integer field; a missing or null optional one is 0.
fn tokens(obj: &Map<String, Value>, key: &str, required: bool, at: &str) -> Result<u64, Error> {
    match obj.get(key) {
        None | Some(Value::Null) => {
            if required {
                Err(malformed(at, key))
            } else {
                Ok(0)
            }
        }
        Some(v) => v.as_u64().ok_or_else(|| malformed(at, key)),
    }
}

fn number(obj: &Map<String, Value>, key: &str, at: &str) -> Result<Option<f64>, Error> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().map(Some).ok_or_else(|| malformed(at, key)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CallRecord, ContextPolicy, Error, McpServer, StreamChecks, StreamParser, Usage,
        model_matches, text_tokens,
    };
    use proptest::prelude::*;

    const PIN: &str = "claude-opus-5-5";

    fn init(model: &str, extra: &str) -> String {
        format!(
            "{{\"type\":\"system\",\"subtype\":\"init\",\"cwd\":\"/s/work-0\",\"session_id\":\"s-1\",\"tools\":[],\
             \"mcp_servers\":[{{\"name\":\"bench\",\"status\":\"connected\"}}],\"model\":\"{model}\",\
             \"claude_code_version\":\"2.1.110\"{extra}}}\n"
        )
    }

    fn result(cache_write: u64) -> String {
        format!(
            "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"duration_ms\":900,\
             \"duration_api_ms\":800,\"num_turns\":1,\"result\":\"OK\",\"stop_reason\":\"end_turn\",\
             \"session_id\":\"s-1\",\"total_cost_usd\":0.25,\"usage\":{{\"input_tokens\":3,\
             \"cache_creation_input_tokens\":{cache_write},\"cache_read_input_tokens\":100,\"output_tokens\":2}},\
             \"modelUsage\":{{\"claude-opus-5-5\":{{\"inputTokens\":3,\"outputTokens\":2,\
             \"cacheReadInputTokens\":100,\"cacheCreationInputTokens\":{cache_write},\"costUSD\":0.25}},\
             \"claude-haiku-4-5\":{{\"inputTokens\":50,\"outputTokens\":5}}}},\"permission_denials\":[{{}}]}}\r\n"
        )
    }

    fn assistant(model: &str, parent: &str) -> String {
        format!(
            "{{\"type\":\"assistant\",\"message\":{{\"model\":\"{model}\",\"content\":[]}},\
             \"parent_tool_use_id\":{parent},\"session_id\":\"s-1\"}}\n"
        )
    }

    fn parse_with(checks: StreamChecks, lines: &[&str]) -> Result<CallRecord, Error> {
        let mut parser = StreamParser::new(checks);
        for line in lines {
            parser.feed_line(line.as_bytes())?;
        }
        parser.finish(Some(0))
    }

    fn parse(lines: &[&str]) -> Result<CallRecord, Error> {
        parse_with(StreamChecks::new(PIN), lines)
    }

    fn delta_error(with: &CallRecord, without: &CallRecord) -> String {
        match text_tokens(with, without) {
            Err(Error::Delta(msg)) => msg,
            other => panic!("expected a delta refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_complete_stream_parses() {
        let rec = parse(&[
            "not json at all\n",
            &init(
                PIN,
                ",\"apiKeySource\":\"none\",\"slash_commands\":[\"compact\"],\"agents\":[\"general-purpose\"],\
                 \"skills\":[],\"plugins\":[],\"output_style\":\"default\"",
            ),
            &assistant(PIN, "null"),
            "{\"type\":\"user\",\"message\":{\"content\":\"a tool result\"}}\n",
            "[1,2]\n",
            "{\"type\":\"user\",\"text\":\"an unmarked { broken line\"}\n",
            "{\"type\":\"result\" broken}\n",
            "\n",
            &result(40),
        ])
        .unwrap();
        assert_eq!(rec.model, PIN);
        assert_eq!(rec.claude_code_version.as_deref(), Some("2.1.110"));
        assert_eq!(rec.cwd.as_deref(), Some("/s/work-0"));
        assert_eq!(rec.api_key_source.as_deref(), Some("none"));
        assert_eq!(rec.mcp_servers[0].name, "bench");
        assert_eq!(rec.slash_commands, ["compact"]);
        assert_eq!(rec.agents, ["general-purpose"]);
        assert_eq!(rec.output_style.as_deref(), Some("default"));
        assert!(rec.owner_context.is_empty());
        assert_eq!(
            rec.usage,
            Usage {
                input: 3,
                cache_read: 100,
                cache_write: 40,
                output: 2
            }
        );
        assert_eq!(rec.usage.total_input(), 143);
        assert_eq!(rec.model_usage.len(), 2);
        assert_eq!(rec.model_usage[0].model, "claude-haiku-4-5");
        assert_eq!(rec.model_usage[0].usage.cache_read, 0);
        assert_eq!(rec.pinned_usage().unwrap().usage.total_input(), 143);
        assert_eq!(rec.permission_denials, 1);
        // "not json at all", "[1,2]" and the marked object line that fails to parse.
        assert_eq!(rec.unparsed_lines, 3);
        assert_eq!(rec.result.as_deref(), Some("OK"));
        assert!(rec.succeeded());
        let json = rec.to_json();
        assert_eq!(json["usage"]["total_input"], 143);
        assert_eq!(json["model_usage"][1]["model"], PIN);
        assert_eq!(json["exit_code"], 0);
        assert_eq!(json["agents"][0], "general-purpose");
        assert_eq!(json["exclude_dynamic_sections"], false);
    }

    #[test]
    fn the_pin_and_the_login_are_checked_at_init() {
        let wrong = parse(&[&init("claude-sonnet-5", "")]).unwrap_err();
        assert!(
            matches!(wrong, Error::ModelMismatch { ref reported, .. } if reported == "claude-sonnet-5")
        );
        let key = parse(&[&init(PIN, ",\"apiKeySource\":\"ANTHROPIC_API_KEY\"")]).unwrap_err();
        assert!(matches!(key, Error::ApiKeySource(ref s) if s == "ANTHROPIC_API_KEY"));
    }

    #[test]
    fn the_version_pin_is_checked_at_init() {
        let mut checks = StreamChecks::new(PIN);
        checks.claude_code_version = Some("2.1.110".to_owned());
        assert!(parse_with(checks.clone(), &[&init(PIN, ""), &result(1)]).is_ok());
        checks.claude_code_version = Some("2.1.281".to_owned());
        let other = parse_with(checks.clone(), &[&init(PIN, "")]).unwrap_err();
        assert!(matches!(
            other,
            Error::VersionMismatch { ref pinned, reported: Some(ref r) } if pinned == "2.1.281" && r == "2.1.110"
        ));
        let unversioned = init(PIN, "").replace(",\"claude_code_version\":\"2.1.110\"", "");
        assert!(matches!(
            parse_with(checks, &[&unversioned]).unwrap_err(),
            Error::VersionMismatch { reported: None, .. }
        ));
    }

    #[test]
    fn owner_context_is_refused_or_recorded() {
        let context = ",\"agents\":[\"general-purpose\",\"fixture-agent\"],\"skills\":[\"fixture-skill\"],\
                       \"plugins\":[{\"name\":\"fixture-plugin\",\"path\":\"/p\"}],\"output_style\":\"Explanatory\"";
        let expected = [
            "agent fixture-agent",
            "skill fixture-skill",
            "plugin fixture-plugin",
            "output style Explanatory",
        ];
        match parse(&[&init(PIN, context)]).unwrap_err() {
            Error::SessionContext(found) => assert_eq!(found, expected),
            other => panic!("expected a context refusal, got {other:?}"),
        }
        let mut checks = StreamChecks::new(PIN);
        checks.owner_context = ContextPolicy::Record;
        let rec = parse_with(checks, &[&init(PIN, context), &result(1)]).unwrap();
        assert_eq!(rec.owner_context, expected);
        assert_eq!(rec.plugins, ["fixture-plugin"]);
        let mut checks = StreamChecks::new(PIN);
        checks.builtin_agents.push("fixture-agent".to_owned());
        let only_agent = ",\"agents\":[\"general-purpose\",\"fixture-agent\"]";
        assert!(parse_with(checks, &[&init(PIN, only_agent), &result(1)]).is_ok());
        let memory = ",\"memory_paths\":{\"team\":\"/c/team\",\"auto\":\"/c/projects/p/memory\"}";
        match parse(&[&init(PIN, memory)]).unwrap_err() {
            Error::SessionContext(found) => assert_eq!(found, ["memory auto", "memory team"]),
            other => panic!("expected a memory refusal, got {other:?}"),
        }
        let bad_memory = ",\"memory_paths\":\"/c/projects/p/memory\"";
        assert!(
            matches!(parse(&[&init(PIN, bad_memory)]).unwrap_err(), Error::Protocol(ref m) if m.contains("memory_paths"))
        );
        assert!(parse(&[&init(PIN, ",\"memory_paths\":null"), &result(1)]).is_ok());
        let bad_plugins = ",\"plugins\":[7]";
        assert!(
            matches!(parse(&[&init(PIN, bad_plugins)]).unwrap_err(), Error::Protocol(ref m) if m.contains("plugins"))
        );
    }

    #[test]
    fn the_main_loop_model_is_checked_on_every_assistant_turn() {
        let fallback = parse(&[&init(PIN, ""), &assistant("claude-sonnet-5", "null")]).unwrap_err();
        assert!(
            matches!(fallback, Error::ModelMismatch { ref reported, .. } if reported == "claude-sonnet-5")
        );
        let rec = parse(&[
            &init(PIN, ""),
            &assistant("claude-haiku-4-5", "\"toolu_1\""),
            &assistant("<synthetic>", "null"),
            &assistant("claude-opus-5-5-20261001", "null"),
            &result(1),
        ])
        .unwrap();
        assert_eq!(rec.model, PIN);
    }

    #[test]
    fn missing_and_malformed_messages() {
        assert!(matches!(
            parse(&[]).unwrap_err(),
            Error::Incomplete {
                missing: "system/init",
                ..
            }
        ));
        assert!(matches!(
            parse(&[&init(PIN, "")]).unwrap_err(),
            Error::Incomplete {
                missing: "result",
                ..
            }
        ));
        let second = parse(&[&init(PIN, ""), &result(1), &result(1)]).unwrap_err();
        assert!(matches!(second, Error::Protocol(ref m) if m.contains("second")));
        let no_usage = "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}";
        assert!(
            matches!(parse(&[&init(PIN, ""), no_usage]).unwrap_err(), Error::Protocol(ref m) if m.contains("usage"))
        );
        let negative = "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\
                        \"usage\":{\"input_tokens\":-1,\"output_tokens\":0}}";
        assert!(
            matches!(parse(&[&init(PIN, ""), negative]).unwrap_err(), Error::Protocol(ref m) if m.contains("input_tokens"))
        );
        let other_session = result(1).replace("\"session_id\":\"s-1\"", "\"session_id\":\"s-2\"");
        assert!(
            matches!(parse(&[&init(PIN, ""), &other_session]).unwrap_err(), Error::Protocol(ref m) if m.contains("s-2"))
        );
        let no_model = "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s\"}";
        assert!(
            matches!(parse(&[no_model]).unwrap_err(), Error::Protocol(ref m) if m.contains("model"))
        );
        let unpinned = result(1).replace("\"claude-opus-5-5\":", "\"claude-opus-4-1\":");
        assert!(
            matches!(parse(&[&init(PIN, ""), &unpinned]).unwrap_err(), Error::Protocol(ref m) if m.contains("modelUsage"))
        );
        let errored = unpinned.replace("\"is_error\":false", "\"is_error\":true");
        assert!(parse(&[&init(PIN, ""), &errored]).is_ok());
    }

    #[test]
    fn model_ids() {
        assert!(model_matches(PIN, PIN));
        assert!(model_matches(PIN, "claude-opus-5-5-20261001"));
        assert!(model_matches(PIN, "claude-opus-5-5[1m]"));
        assert!(model_matches(PIN, "claude-opus-5-5-20261001[1m]"));
        assert!(model_matches(PIN, "claude-opus-5-5@20261001"));
        assert!(!model_matches(PIN, "claude-opus-5-50"));
        assert!(!model_matches(PIN, "claude-opus-5"));
        assert!(!model_matches(PIN, "claude-sonnet-5"));
        assert!(!model_matches("claude-opus-5", "claude-opus-5-5"));
        assert!(!model_matches("claude-sonnet-4", "claude-sonnet-4-5"));
        assert!(!model_matches(
            "claude-sonnet-4",
            "claude-sonnet-4-5-20250929"
        ));
        assert!(!model_matches(PIN, "claude-opus-5-5-2026100"));
        assert!(!model_matches(PIN, "claude-opus-5-5-20261001x"));
        assert!(!model_matches(PIN, "claude-opus-5-5[]"));
        assert!(!model_matches(PIN, "claude-opus-5-5[1m][2m]"));
        assert!(!model_matches(PIN, "claude-opus-5-5@"));
    }

    #[test]
    fn text_tokens_is_the_input_difference() {
        let without = parse(&[&init(PIN, ""), &result(40)]).unwrap();
        let with = parse(&[&init(PIN, ""), &result(1057)]).unwrap();
        assert_eq!(text_tokens(&with, &without).unwrap(), 1017);
        assert!(delta_error(&without, &with).contains("fewer"));
        let mut failed = with.clone();
        failed.is_error = true;
        assert!(delta_error(&failed, &without).contains("ended in"));
        let mut unpriced = with.clone();
        unpriced.model_usage.clear();
        assert!(delta_error(&unpriced, &without).contains("no usage"));
    }

    #[test]
    fn text_tokens_refuses_calls_that_differ_in_more_than_the_text() {
        let without = parse(&[&init(PIN, ""), &result(40)]).unwrap();
        let with = parse(&[&init(PIN, ""), &result(1057)]).unwrap();

        let (mut a, mut b) = (with.clone(), without.clone());
        a.num_turns = 3;
        b.num_turns = 3;
        assert!(delta_error(&a, &b).contains("3 turns"));

        let mut other = with.clone();
        other.claude_code_version = Some("2.1.281".to_owned());
        assert!(delta_error(&other, &without).contains("claude_code_version"));

        let mut other = with.clone();
        other.tools = vec!["Read".to_owned()];
        assert!(delta_error(&other, &without).contains("tools"));

        let mut other = with.clone();
        other.mcp_servers = vec![McpServer {
            name: "bench".to_owned(),
            status: "failed".to_owned(),
        }];
        assert!(delta_error(&other, &without).contains("mcp_servers"));

        let mut other = with.clone();
        other.cwd = Some("/s/work-1".to_owned());
        assert!(delta_error(&other, &without).contains("cwd"));
        let (mut a, mut b) = (other, without.clone());
        a.exclude_dynamic_sections = true;
        b.exclude_dynamic_sections = true;
        assert_eq!(text_tokens(&a, &b).unwrap(), 1017);
        a.exclude_dynamic_sections = false;
        assert!(delta_error(&a, &b).contains("exclude_dynamic_sections"));

        let mut other = with.clone();
        other.agents = vec!["general-purpose".to_owned()];
        assert!(delta_error(&other, &without).contains("agents"));

        let mut other = with.clone();
        other.pinned_model = "claude-opus-5".to_owned();
        assert!(delta_error(&other, &without).contains("pinned_model"));

        // A custom command reaches the context through the command or skill tool's description.
        let mut other = with.clone();
        other.slash_commands = vec!["fixture-command".to_owned()];
        assert!(delta_error(&other, &without).contains("slash_commands"));

        // Recorded owner context (`owner-context: record`), such as a memory directory, reaches the context too.
        let mut other = with;
        other.owner_context = vec!["memory auto".to_owned()];
        assert!(delta_error(&other, &without).contains("owner_context"));
    }

    #[test]
    fn mcp_servers_of_an_mcp_config_must_connect() {
        let failed = init(PIN, "").replace("\"connected\"", "\"failed\"");
        // Without --mcp-config the status is only recorded.
        let rec = parse(&[&failed, &result(1)]).unwrap();
        assert!(!rec.mcp_config && rec.mcp_failures().is_empty());
        let mut checks = StreamChecks::new(PIN);
        checks.mcp_config = true;
        let rec = parse_with(checks.clone(), &[&init(PIN, ""), &result(1)]).unwrap();
        assert!(rec.mcp_config && rec.mcp_failures().is_empty());
        assert_eq!(rec.to_json()["mcp_config"], true);
        match parse_with(checks.clone(), &[&failed]).unwrap_err() {
            Error::McpNotConnected(found) => assert_eq!(found, ["bench (failed)"]),
            other => panic!("expected an MCP refusal, got {other:?}"),
        }
        checks.mcp_failed = ContextPolicy::Record;
        let rec = parse_with(checks, &[&failed, &result(1)]).unwrap();
        assert_eq!(rec.mcp_failures(), ["bench (failed)"]);
    }

    #[test]
    fn a_result_marks_the_stream_complete() {
        let mut parser = StreamParser::new(StreamChecks::new(PIN));
        parser.feed_line(init(PIN, "").as_bytes()).unwrap();
        assert!(!parser.has_result());
        parser.feed_line(result(1).as_bytes()).unwrap();
        assert!(parser.has_result());
        let mut rec = parser.finish(None).unwrap();
        assert!(!rec.succeeded());
        rec.killed_after_result = true;
        assert!(rec.succeeded());
        let json = rec.to_json();
        assert_eq!(json["killed_after_result"], true);
        assert_eq!(json["pipes_held_after_exit"], false);
    }

    /// The suffix grammar [`model_matches`] accepts after the pin, written out independently.
    fn allowed_suffix(suffix: &str) -> bool {
        let variant = |s: &str| {
            s.len() > 2
                && s.starts_with('[')
                && s.ends_with(']')
                && !s[1..s.len() - 1].contains(['[', ']'])
        };
        if suffix.is_empty() || variant(suffix) {
            return true;
        }
        if let Some(tag) = suffix.strip_prefix('@') {
            return !tag.is_empty();
        }
        let Some(dated) = suffix.strip_prefix('-') else {
            return false;
        };
        let digits = dated.bytes().take_while(u8::is_ascii_digit).count();
        digits >= 8 && dated.is_char_boundary(8) && (dated.len() == 8 || variant(&dated[8..]))
    }

    /// One noise line and whether it counts as unparsed.
    fn noise_line(kind: u8, text: &str) -> (String, bool) {
        match kind % 5 {
            0 => (format!("x{text}"), true),
            1 => ("[1,2]".to_owned(), true),
            2 => (" \t".repeat(usize::from(kind % 3)), false),
            3 => (
                format!(
                    "{{\"type\":\"system\",\"subtype\":\"hook_response\",\"text\":\"{text}\"}}"
                ),
                false,
            ),
            _ => ("{\"type\":\"user\" \"result\" broken}".to_owned(), true),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// A reported id matches the pin exactly when it is the pin followed by an allowed suffix: a date, a
        /// bracketed variant, both, or an `@` tag. A longer id of another model never matches.
        #[test]
        fn model_ids_match_only_with_the_allowed_suffixes(
            pin in "[a-z]{1,6}(-[0-9]{1,2}){0,3}",
            suffix in "[-0-9@\\[\\]a-z]{0,14}",
        ) {
            let reported = format!("{pin}{suffix}");
            prop_assert_eq!(model_matches(&pin, &reported), allowed_suffix(&suffix));
            // An id that does not start with the pin never matches it.
            let other = format!("z{pin}");
            prop_assert!(!model_matches(&pin, &other));
        }

        /// Noise lines anywhere in the stream (non-JSON text, non-object JSON, blank lines, unmarked events, a
        /// marked line that fails to parse) and CRLF terminators on any line change nothing but the count of
        /// unparsed lines.
        #[test]
        fn the_parse_ignores_noise_and_line_endings(
            noise in proptest::collection::vec((0usize..4, any::<u8>(), "[a-z0-9 :,.]{0,20}"), 0..12),
            crlf in proptest::collection::vec(any::<bool>(), 3),
        ) {
            let clean = [init(PIN, ""), assistant(PIN, "null"), result(40)];
            let baseline = parse(&[&clean[0], &clean[1], &clean[2]]).unwrap();
            let mut lines: Vec<Vec<String>> = vec![Vec::new(); 4];
            let mut unparsed = 0u64;
            for (slot, kind, text) in &noise {
                let (line, counted) = noise_line(*kind, text);
                lines[*slot].push(format!("{line}\n"));
                unparsed += u64::from(counted);
            }
            let mut stream: Vec<String> = Vec::new();
            for (i, message) in clean.iter().enumerate() {
                stream.append(&mut lines[i]);
                let body = message.trim_end();
                stream.push(if crlf[i] { format!("{body}\r\n") } else { format!("{body}\n") });
            }
            stream.append(&mut lines[3]);
            let refs: Vec<&str> = stream.iter().map(String::as_str).collect();
            let rec = parse(&refs).unwrap();
            prop_assert_eq!(rec.unparsed_lines, unparsed);
            let mut expected = baseline;
            expected.unparsed_lines = unparsed;
            prop_assert_eq!(rec, expected);
        }
    }
}
