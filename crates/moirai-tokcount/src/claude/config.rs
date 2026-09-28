//! The runner configuration: where the native `claude` is, the runner-owned directories, the pinned model and
//! version, and the operational limits. Operational policy is configuration with defaults (AGENTS.md); only the
//! runner root has none, because it is where the owner logs in once (V9).
//!
//! The file is one JSON object with these keys (unknown keys are refused, so a misspelt key cannot fall back to a
//! default unnoticed; every path is absolute):
//!
//! | Key | Type | Default | Meaning |
//! |---|---|---|---|
//! | `runner-root` | path | required | the runner's own directory, outside every repository and user profile |
//! | `claude-exe` | path | discovery ([`super::exe::discover_from_env`]) | the native Claude Code executable |
//! | `config-dir` | path | `<runner-root>/claude-config` | `CLAUDE_CONFIG_DIR` of every call; the owner logs in here |
//! | `scratch-root` | path | `<runner-root>/scratch` | holds the work directories `work-<slot>` and their `work-<slot>.lock` files |
//! | `model` | string | `claude-opus-5-5` | the `--model` pin ([90 §8.3]) |
//! | `claude-code-version` | string | none | the Claude Code version every call must report ([90 §8.3] item (4)); a call or `version()` that reports another is refused |
//! | `timeout-ms` | integer ≥ 1 | 300000 | wall-clock limit of one call until its result arrives; the process tree is killed after it |
//! | `post-result-grace-ms` | integer ≥ 0 | 2000 | how long the process may run on after its `result` message, whatever is left of `timeout-ms`; beyond it the tree is killed and the complete record kept with no exit code (`killed_after_result`) |
//! | `setting-sources` | string | `""` | `--setting-sources`: a comma list of `user`, `project`, `local`, or none; with `user`, `config-dir/settings.json` may not set [`super::USER_SETTINGS_HAZARDS`] |
//! | `disable-slash-commands` | bool | `true` | pass `--disable-slash-commands` (no skills in the context) |
//! | `session-persistence` | bool | `true` | keep the session transcript in `config-dir` (`false`: `--no-session-persistence`) |
//! | `exclude-dynamic-system-prompt-sections` | bool | `false` | pass `--exclude-dynamic-system-prompt-sections`, which leaves the working directory, the environment section and the memory paths out of the system prompt (only for a Claude Code version that has the flag) |
//! | `work-slots` | integer 1–256 | 8 | how many calls may run at once under `scratch-root`; each runs in the stable work directory of its slot |
//! | `owner-context` | `"refuse"` or `"record"` | `"refuse"` | what a session gets whose init lists an agent outside `builtin-agents`, a skill, a plugin, an output style other than `default` or a memory directory: refused and killed, or recorded in the call record |
//! | `builtin-agents` | array of strings | [`super::DEFAULT_BUILTIN_AGENTS`] | the agents an init may list without counting as owner context |
//! | `mcp-failed` | `"refuse"` or `"record"` | `"refuse"` | what a call gets whose init lists a server of its `--mcp-config` that is not `connected`: refused and killed, or recorded (the CLI still fails such a call) |
//! | `managed-policy-dirs` | array of paths | [`super::managed_policy_dirs`] of this OS | the directories where a managed-policy file ([`super::MANAGED_POLICY_FILES`]) refuses every call |
//! | `max-line-bytes` | integer ≥ 1024 | 16777216 | the largest stdout line accepted; the process tree is killed beyond it |
//! | `env` | object of strings | `{}` | extra variables for the child, set on top of the parent's allowed ones ([`super::PASSED_ENV`]; e.g. `CLAUDE_CODE_GIT_BASH_PATH`); never `ANTHROPIC_*`, `CLAUDE_CODE_USE_*` or a variable the runner sets itself |
//!
//! To log in once (V9), run the configured executable interactively with `CLAUDE_CONFIG_DIR` set to `config-dir`
//! and use `/login` with the subscription account.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use super::Error;
use super::record::{ContextPolicy, DEFAULT_BUILTIN_AGENTS, StreamChecks};
use super::run::managed_policy_dirs;

/// The model every call pins unless the configuration names another: Opus 5.5 ([90 §8.3] plan of record,
/// [AR §11] #38 (a)).
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
/// The default wall-clock limit of one call.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
/// The default time a process may run on after its `result` message.
pub const DEFAULT_POST_RESULT_GRACE: Duration = Duration::from_secs(2);
/// The default `--setting-sources` value: no user, project or local settings, and so no `CLAUDE.md` of either kind.
pub const DEFAULT_SETTING_SOURCES: &str = "";
/// The default limit on one stdout line, terminator included.
pub const DEFAULT_MAX_LINE_BYTES: usize = 16 << 20;
/// The default number of work slots, the calls that may run at once under one scratch root.
pub const DEFAULT_WORK_SLOTS: u32 = 8;
/// The largest `work-slots` value.
pub const MAX_WORK_SLOTS: u32 = 256;
/// Variables the runner sets itself on every call; the `env` key may not override them.
pub const RESERVED_ENV: [&str; 3] = [
    "CLAUDE_CONFIG_DIR",
    "DISABLE_AUTOUPDATER",
    "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
];

/// Prefixes the `env` key may not use: an API key or endpoint (`ANTHROPIC_*`) and a provider switch
/// (`CLAUDE_CODE_USE_BEDROCK`, `…_VERTEX`, `…_FOUNDRY`), which moves the login off the subscription while the
/// init can still report the API-key source `none` (PLAN §6.1 #6).
const REFUSED_ENV_PREFIXES: [&str; 2] = ["ANTHROPIC_", "CLAUDE_CODE_USE_"];

const MIN_MAX_LINE_BYTES: usize = 1024;
const KEYS: [&str; 19] = [
    "runner-root",
    "claude-exe",
    "config-dir",
    "scratch-root",
    "model",
    "claude-code-version",
    "timeout-ms",
    "post-result-grace-ms",
    "setting-sources",
    "disable-slash-commands",
    "session-persistence",
    "exclude-dynamic-system-prompt-sections",
    "work-slots",
    "owner-context",
    "builtin-agents",
    "mcp-failed",
    "managed-policy-dirs",
    "max-line-bytes",
    "env",
];

/// The runner configuration of [`super::Claude`]. Build it with [`RunnerConfig::new`] or load it with
/// [`RunnerConfig::load`]; [`RunnerConfig::validate`] checks a value built by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerConfig {
    /// The native Claude Code executable; `None` discovers it.
    pub claude_exe: Option<PathBuf>,
    /// `CLAUDE_CONFIG_DIR` of every call.
    pub config_dir: PathBuf,
    /// The parent of the work directories and their lock files.
    pub scratch_root: PathBuf,
    /// The `--model` pin.
    pub model: String,
    /// The Claude Code version every call must report, when pinned.
    pub claude_code_version: Option<String>,
    /// The wall-clock limit of one call until its result arrives.
    pub timeout: Duration,
    /// How long the process may run on after its `result` message.
    pub post_result_grace: Duration,
    /// The `--setting-sources` value.
    pub setting_sources: String,
    /// Whether to pass `--disable-slash-commands`.
    pub disable_slash_commands: bool,
    /// Whether Claude Code keeps the session transcript (`false` passes `--no-session-persistence`).
    pub session_persistence: bool,
    /// Whether to pass `--exclude-dynamic-system-prompt-sections`.
    pub exclude_dynamic_sections: bool,
    /// How many calls may run at once under `scratch_root`.
    pub work_slots: u32,
    /// What owner context in a session's init gets.
    pub owner_context: ContextPolicy,
    /// The agents an init may list without counting as owner context.
    pub builtin_agents: Vec<String>,
    /// What a server of the call's `--mcp-config` that is not `connected` gets.
    pub mcp_failed: ContextPolicy,
    /// The directories whose managed-policy files refuse every call.
    pub managed_policy_dirs: Vec<PathBuf>,
    /// The largest stdout line accepted, terminator included.
    pub max_line_bytes: usize,
    /// Extra variables for the child, set after the scrub.
    pub env: Vec<(String, String)>,
}

impl RunnerConfig {
    /// The defaults for the runner root `runner_root`; the managed-policy directories are those of this process's
    /// OS and environment.
    ///
    /// # Errors
    ///
    /// `runner_root` is not absolute.
    pub fn new(runner_root: &Path) -> Result<Self, Error> {
        absolute("runner-root", runner_root)?;
        Ok(Self {
            claude_exe: None,
            config_dir: runner_root.join("claude-config"),
            scratch_root: runner_root.join("scratch"),
            model: DEFAULT_MODEL.to_owned(),
            claude_code_version: None,
            timeout: DEFAULT_TIMEOUT,
            post_result_grace: DEFAULT_POST_RESULT_GRACE,
            setting_sources: DEFAULT_SETTING_SOURCES.to_owned(),
            disable_slash_commands: true,
            session_persistence: true,
            exclude_dynamic_sections: false,
            work_slots: DEFAULT_WORK_SLOTS,
            owner_context: ContextPolicy::Refuse,
            builtin_agents: DEFAULT_BUILTIN_AGENTS.map(str::to_owned).to_vec(),
            mcp_failed: ContextPolicy::Refuse,
            managed_policy_dirs: managed_policy_dirs(std::env::consts::OS, |name| {
                std::env::var_os(name)
            }),
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
            env: Vec::new(),
        })
    }

    /// Reads and validates the JSON configuration file at `path`.
    ///
    /// # Errors
    ///
    /// The file cannot be read, is not a JSON object, or fails [`RunnerConfig::from_json`].
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text =
            fs::read_to_string(path).map_err(Error::io(format!("reading {}", path.display())))?;
        Self::from_json(&text).map_err(|e| match e {
            Error::Config(msg) => Error::Config(format!("{}: {msg}", path.display())),
            other => other,
        })
    }

    /// Parses and validates a configuration object (the module documentation lists the keys).
    ///
    /// # Errors
    ///
    /// The text is not a JSON object, a key is unknown or missing, or a value is invalid.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| Error::Config(format!("not JSON: {e}")))?;
        let Value::Object(obj) = value else {
            return Err(Error::Config(
                "the configuration must be a JSON object".to_owned(),
            ));
        };
        if let Some(key) = obj.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(Error::Config(format!(
                "unknown key {key:?}; the keys are {}",
                KEYS.join(", ")
            )));
        }
        let root = path(&obj, "runner-root")?
            .ok_or_else(|| Error::Config("\"runner-root\" is required".to_owned()))?;
        let mut cfg = Self::new(&root)?;
        cfg.claude_exe = path(&obj, "claude-exe")?;
        if let Some(dir) = path(&obj, "config-dir")? {
            cfg.config_dir = dir;
        }
        if let Some(dir) = path(&obj, "scratch-root")? {
            cfg.scratch_root = dir;
        }
        if let Some(model) = string(&obj, "model")? {
            cfg.model = model.to_owned();
        }
        cfg.claude_code_version = string(&obj, "claude-code-version")?.map(str::to_owned);
        if let Some(ms) = integer(&obj, "timeout-ms")? {
            cfg.timeout = Duration::from_millis(ms);
        }
        if let Some(ms) = integer(&obj, "post-result-grace-ms")? {
            cfg.post_result_grace = Duration::from_millis(ms);
        }
        if let Some(sources) = string(&obj, "setting-sources")? {
            cfg.setting_sources = sources.to_owned();
        }
        if let Some(flag) = boolean(&obj, "disable-slash-commands")? {
            cfg.disable_slash_commands = flag;
        }
        if let Some(flag) = boolean(&obj, "session-persistence")? {
            cfg.session_persistence = flag;
        }
        if let Some(flag) = boolean(&obj, "exclude-dynamic-system-prompt-sections")? {
            cfg.exclude_dynamic_sections = flag;
        }
        if let Some(slots) = integer(&obj, "work-slots")? {
            cfg.work_slots = u32::try_from(slots).unwrap_or(u32::MAX);
        }
        if let Some(policy) = policy(&obj, "owner-context")? {
            cfg.owner_context = policy;
        }
        if let Some(policy) = policy(&obj, "mcp-failed")? {
            cfg.mcp_failed = policy;
        }
        if let Some(agents) = strings(&obj, "builtin-agents")? {
            cfg.builtin_agents = agents;
        }
        if let Some(dirs) = strings(&obj, "managed-policy-dirs")? {
            cfg.managed_policy_dirs = dirs.into_iter().map(PathBuf::from).collect();
        }
        if let Some(bytes) = integer(&obj, "max-line-bytes")? {
            cfg.max_line_bytes = usize::try_from(bytes).map_err(|_| {
                Error::Config(format!(
                    "\"max-line-bytes\" {bytes} does not fit this platform"
                ))
            })?;
        }
        if let Some(env) = obj.get("env") {
            let Value::Object(vars) = env else {
                return Err(Error::Config(
                    "\"env\" must be an object of strings".to_owned(),
                ));
            };
            for (name, value) in vars {
                let Value::String(value) = value else {
                    return Err(Error::Config(format!("\"env\".{name} must be a string")));
                };
                cfg.env.push((name.clone(), value.clone()));
            }
        }
        cfg.validate()?;
        Ok(cfg)
    }

    /// Checks every field: absolute paths, a plain model id and version, known setting sources, positive limits,
    /// a work-slot count in range, non-empty built-in agent names, and `env` names that are well formed, are not
    /// reserved ([`RESERVED_ENV`]) and carry no `ANTHROPIC_*` or `CLAUDE_CODE_USE_*` variable, so no API key,
    /// endpoint or other provider reaches the child (PLAN §6.1 #6).
    ///
    /// # Errors
    ///
    /// The first field that fails.
    pub fn validate(&self) -> Result<(), Error> {
        if let Some(exe) = &self.claude_exe {
            absolute("claude-exe", exe)?;
        }
        absolute("config-dir", &self.config_dir)?;
        absolute("scratch-root", &self.scratch_root)?;
        for dir in &self.managed_policy_dirs {
            absolute("managed-policy-dirs", dir)?;
        }
        if !plain_id(&self.model) {
            return Err(Error::Config(format!(
                "\"model\" {:?} is not a model id",
                self.model
            )));
        }
        if let Some(version) = &self.claude_code_version
            && !plain_id(version)
        {
            return Err(Error::Config(format!(
                "\"claude-code-version\" {version:?} is not a version"
            )));
        }
        if !self.setting_sources.is_empty() {
            for source in self.setting_sources.split(',') {
                if !matches!(source, "user" | "project" | "local") {
                    return Err(Error::Config(format!(
                        "\"setting-sources\" {:?}: {source:?} is not user, project or local",
                        self.setting_sources
                    )));
                }
            }
        }
        if self.timeout.is_zero() {
            return Err(Error::Config(
                "\"timeout-ms\" must be at least 1".to_owned(),
            ));
        }
        if !(1..=MAX_WORK_SLOTS).contains(&self.work_slots) {
            return Err(Error::Config(format!(
                "\"work-slots\" must be from 1 to {MAX_WORK_SLOTS}"
            )));
        }
        if self.builtin_agents.iter().any(String::is_empty) {
            return Err(Error::Config(
                "\"builtin-agents\" may not hold an empty name".to_owned(),
            ));
        }
        if self.max_line_bytes < MIN_MAX_LINE_BYTES {
            return Err(Error::Config(format!(
                "\"max-line-bytes\" must be at least {MIN_MAX_LINE_BYTES}"
            )));
        }
        for (name, value) in &self.env {
            let well_formed =
                !name.is_empty() && !name.contains(['=', '\0']) && !value.contains('\0');
            if !well_formed {
                return Err(Error::Config(format!(
                    "\"env\" variable {name:?} is not a valid name and value"
                )));
            }
            let upper = name.to_ascii_uppercase();
            if REFUSED_ENV_PREFIXES.iter().any(|p| upper.starts_with(p)) {
                return Err(Error::Config(format!(
                    "\"env\" may not set {name}: no API key, endpoint or other provider reaches the runner, which \
                     uses the subscription login only (PLAN §6.1 #6)"
                )));
            }
            if RESERVED_ENV.contains(&upper.as_str()) {
                return Err(Error::Config(format!(
                    "\"env\" may not set {name}: the runner sets it on every call"
                )));
            }
        }
        Ok(())
    }

    /// Whether `setting-sources` includes `user`, so Claude Code reads `config-dir/settings.json`.
    #[must_use]
    pub fn reads_user_settings(&self) -> bool {
        self.setting_sources.split(',').any(|s| s == "user")
    }

    /// The checks the stream parser applies to every call of this configuration.
    #[must_use]
    pub fn stream_checks(&self) -> StreamChecks {
        StreamChecks {
            model: self.model.clone(),
            claude_code_version: self.claude_code_version.clone(),
            builtin_agents: self.builtin_agents.clone(),
            owner_context: self.owner_context,
            mcp_config: false,
            mcp_failed: self.mcp_failed,
        }
    }
}

/// A `"refuse"` or `"record"` value.
fn policy(obj: &Map<String, Value>, key: &str) -> Result<Option<ContextPolicy>, Error> {
    match string(obj, key)? {
        None => Ok(None),
        Some("refuse") => Ok(Some(ContextPolicy::Refuse)),
        Some("record") => Ok(Some(ContextPolicy::Record)),
        Some(other) => Err(Error::Config(format!(
            "{key:?} {other:?} is not \"refuse\" or \"record\""
        ))),
    }
}

/// An array of strings.
fn strings(obj: &Map<String, Value>, key: &str) -> Result<Option<Vec<String>>, Error> {
    let Some(value) = obj.get(key) else {
        return Ok(None);
    };
    let not_strings = || Error::Config(format!("{key:?} must be an array of strings"));
    value
        .as_array()
        .ok_or_else(not_strings)?
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or_else(not_strings))
        .collect::<Result<_, _>>()
        .map(Some)
}

/// A plain identifier: printable ASCII without spaces or commas, not starting with `-` (so it cannot read as a flag).
fn plain_id(id: &str) -> bool {
    !id.is_empty() && !id.starts_with('-') && id.bytes().all(|b| b.is_ascii_graphic() && b != b',')
}

fn absolute(key: &str, path: &Path) -> Result<(), Error> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "{key:?} must be an absolute path, not {}",
            path.display()
        )))
    }
}

fn string<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, Error> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(Error::Config(format!("{key:?} must be a string"))),
    }
}

fn path(obj: &Map<String, Value>, key: &str) -> Result<Option<PathBuf>, Error> {
    let Some(text) = string(obj, key)? else {
        return Ok(None);
    };
    let path = PathBuf::from(text);
    absolute(key, &path)?;
    Ok(Some(path))
}

fn integer(obj: &Map<String, Value>, key: &str) -> Result<Option<u64>, Error> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| Error::Config(format!("{key:?} must be a whole number"))),
    }
}

fn boolean(obj: &Map<String, Value>, key: &str) -> Result<Option<bool>, Error> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(Error::Config(format!("{key:?} must be true or false"))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_BUILTIN_AGENTS, DEFAULT_MODEL, DEFAULT_POST_RESULT_GRACE, DEFAULT_TIMEOUT,
        DEFAULT_WORK_SLOTS, Error, RunnerConfig,
    };
    use crate::claude::{ContextPolicy, managed_policy_dirs};
    use std::path::PathBuf;
    use std::time::Duration;

    /// An absolute root on every target: the tests only parse, they never touch it.
    fn root() -> PathBuf {
        std::env::temp_dir().join("moirai-runner")
    }

    fn json_path(p: &std::path::Path) -> String {
        serde_json::Value::String(p.display().to_string()).to_string()
    }

    fn config_error(text: &str) -> String {
        match RunnerConfig::from_json(text) {
            Err(Error::Config(msg)) => msg,
            other => panic!("expected a configuration error, got {other:?}"),
        }
    }

    #[test]
    fn defaults_derive_from_the_runner_root() {
        let cfg = RunnerConfig::from_json(&format!("{{\"runner-root\": {}}}", json_path(&root())))
            .unwrap();
        assert_eq!(cfg.claude_exe, None);
        assert_eq!(cfg.config_dir, root().join("claude-config"));
        assert_eq!(cfg.scratch_root, root().join("scratch"));
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert_eq!(cfg.claude_code_version, None);
        assert_eq!(cfg.timeout, DEFAULT_TIMEOUT);
        assert_eq!(cfg.post_result_grace, DEFAULT_POST_RESULT_GRACE);
        assert_eq!(cfg.setting_sources, "");
        assert!(!cfg.reads_user_settings());
        assert!(cfg.disable_slash_commands && cfg.session_persistence);
        assert!(!cfg.exclude_dynamic_sections);
        assert_eq!(cfg.work_slots, DEFAULT_WORK_SLOTS);
        assert_eq!(cfg.owner_context, ContextPolicy::Refuse);
        assert_eq!(cfg.builtin_agents, DEFAULT_BUILTIN_AGENTS);
        assert_eq!(cfg.mcp_failed, ContextPolicy::Refuse);
        assert_eq!(
            cfg.managed_policy_dirs,
            managed_policy_dirs(std::env::consts::OS, |name| std::env::var_os(name))
        );
        assert!(cfg.env.is_empty());
        let checks = cfg.stream_checks();
        assert_eq!(checks.model, DEFAULT_MODEL);
        assert_eq!(checks.owner_context, ContextPolicy::Refuse);
        assert_eq!(checks.mcp_failed, ContextPolicy::Refuse);
        assert!(!checks.mcp_config);
    }

    #[test]
    fn every_key_is_read() {
        let text = format!(
            "{{\"runner-root\": {r}, \"claude-exe\": {e}, \"config-dir\": {c}, \"scratch-root\": {s}, \
             \"model\": \"claude-opus-5-5-20261001\", \"claude-code-version\": \"2.1.110\", \"timeout-ms\": 1500, \
             \"setting-sources\": \"user,local\", \"disable-slash-commands\": false, \"session-persistence\": false, \
             \"exclude-dynamic-system-prompt-sections\": true, \"work-slots\": 2, \"owner-context\": \"record\", \
             \"builtin-agents\": [\"general-purpose\"], \"max-line-bytes\": 4096, \"post-result-grace-ms\": 0, \
             \"mcp-failed\": \"record\", \"managed-policy-dirs\": [{p}], \
             \"env\": {{\"CLAUDE_CODE_GIT_BASH_PATH\": \"x\"}}}}",
            r = json_path(&root()),
            e = json_path(&root().join("bin").join("claude")),
            c = json_path(&root().join("cfg")),
            s = json_path(&root().join("tmp")),
            p = json_path(&root().join("policy")),
        );
        let cfg = RunnerConfig::from_json(&text).unwrap();
        assert_eq!(cfg.claude_exe, Some(root().join("bin").join("claude")));
        assert_eq!(cfg.config_dir, root().join("cfg"));
        assert_eq!(cfg.scratch_root, root().join("tmp"));
        assert_eq!(cfg.model, "claude-opus-5-5-20261001");
        assert_eq!(cfg.claude_code_version.as_deref(), Some("2.1.110"));
        assert_eq!(cfg.timeout, Duration::from_millis(1500));
        assert_eq!(cfg.setting_sources, "user,local");
        assert!(cfg.reads_user_settings());
        assert!(!cfg.disable_slash_commands && !cfg.session_persistence);
        assert!(cfg.exclude_dynamic_sections);
        assert_eq!(cfg.work_slots, 2);
        assert_eq!(cfg.owner_context, ContextPolicy::Record);
        assert_eq!(cfg.builtin_agents, ["general-purpose"]);
        assert_eq!(cfg.max_line_bytes, 4096);
        assert_eq!(cfg.post_result_grace, Duration::ZERO);
        assert_eq!(cfg.mcp_failed, ContextPolicy::Record);
        assert_eq!(cfg.managed_policy_dirs, [root().join("policy")]);
        assert_eq!(
            cfg.env,
            [("CLAUDE_CODE_GIT_BASH_PATH".to_owned(), "x".to_owned())]
        );
        let checks = cfg.stream_checks();
        assert_eq!(checks.claude_code_version.as_deref(), Some("2.1.110"));
        assert_eq!(checks.builtin_agents, ["general-purpose"]);
        assert_eq!(checks.mcp_failed, ContextPolicy::Record);
    }

    #[test]
    fn refusals() {
        let r = json_path(&root());
        let with = |extra: &str| config_error(&format!("{{\"runner-root\": {r}, {extra}}}"));
        assert!(config_error("[]").contains("JSON object"));
        assert!(config_error("{}").contains("required"));
        assert!(with("\"modle\": \"x\"").contains("unknown key"));
        assert!(config_error("{\"runner-root\": \"relative/dir\"}").contains("absolute"));
        assert!(with("\"model\": \"--help\"").contains("model id"));
        assert!(with("\"claude-code-version\": \"\"").contains("not a version"));
        assert!(with("\"claude-code-version\": \"2.1 beta\"").contains("not a version"));
        assert!(with("\"setting-sources\": \"user,all\"").contains("all"));
        assert!(with("\"timeout-ms\": 0").contains("at least 1"));
        assert!(with("\"timeout-ms\": -5").contains("whole number"));
        assert!(with("\"max-line-bytes\": 10").contains("at least"));
        assert!(with("\"work-slots\": 0").contains("from 1 to"));
        assert!(with("\"work-slots\": 257").contains("from 1 to"));
        assert!(with("\"work-slots\": 99999999999").contains("from 1 to"));
        assert!(with("\"owner-context\": \"ignore\"").contains("refuse"));
        assert!(with("\"builtin-agents\": \"Plan\"").contains("array of strings"));
        assert!(with("\"builtin-agents\": [1]").contains("array of strings"));
        assert!(with("\"builtin-agents\": [\"\"]").contains("empty name"));
        assert!(with("\"exclude-dynamic-system-prompt-sections\": 1").contains("true or false"));
        assert!(with("\"env\": {\"anthropic_api_key\": \"k\"}").contains("API key"));
        assert!(with("\"env\": {\"CLAUDE_CODE_USE_BEDROCK\": \"1\"}").contains("provider"));
        assert!(with("\"env\": {\"claude_code_use_vertex\": \"1\"}").contains("provider"));
        assert!(with("\"env\": {\"CLAUDE_CONFIG_DIR\": \"d\"}").contains("sets it"));
        assert!(with("\"env\": {\"A=B\": \"d\"}").contains("valid name"));
        assert!(with("\"env\": {\"A\": 1}").contains("string"));
        assert!(with("\"session-persistence\": 1").contains("true or false"));
        assert!(with("\"post-result-grace-ms\": -1").contains("whole number"));
        assert!(with("\"mcp-failed\": \"ignore\"").contains("refuse"));
        assert!(with("\"managed-policy-dirs\": \"/etc\"").contains("array of strings"));
        assert!(with("\"managed-policy-dirs\": [\"relative\"]").contains("absolute"));
    }
}
