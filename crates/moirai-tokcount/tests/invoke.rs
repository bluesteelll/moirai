//! Tier-`pr` tests of the headless invocation (PLAN WP-58) against `fake-claude`, which replays the hand-written
//! fixtures of `testdata/`. No test contacts a model. Every process a test starts ends before the test does, or
//! within the lifetime a test gives it.
//!
//! The runner refuses a scratch root with Claude Code context above it, so the tests need a base directory with no
//! `.git`, `.claude`, `CLAUDE.md` or `CLAUDE.local.md` in it or any ancestor: `MOIRAI_TOKCOUNT_TEST_ROOT` when set
//! (an absolute directory), else the target directory's `tmp` (`CARGO_TARGET_TMPDIR`), else the system temporary
//! directory. An in-repository target directory, or a temporary directory under a user profile that holds
//! `.claude`, fails the check; set `MOIRAI_TOKCOUNT_TEST_ROOT` or `CARGO_TARGET_DIR` outside both then. The tests
//! set `managed-policy-dirs` to none, so a managed Claude Code policy on the machine does not refuse them.
//!
//! No test depends on how fast the machine is: a call is held open by a release file, a descendant is watched
//! through its heartbeat file, and time limits are far from the times they bound.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use moirai_tokcount::claude::{
    BuiltinTools, Claude, ContextPolicy, Error, ISOLATION_PROMPT, Prompt, RESERVED_ENV, Request,
    RunnerConfig, is_passed_env, scratch_hazard, text_tokens,
};
use serde_json::{Value, json};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-claude");
const TOKCOUNT: &str = env!("CARGO_BIN_EXE_tokcount");
const PIN: &str = "claude-opus-5-5";

fn testdata(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join(name)
}

fn testdata_text(name: &str) -> String {
    testdata(name).display().to_string()
}

/// A runner root for one test, removed when dropped, under the base directory the module documentation names.
struct Root(PathBuf);

impl Root {
    fn new(tag: &str) -> Self {
        let usable = |base: &Path| {
            base.is_absolute() && fs::create_dir_all(base).is_ok() && scratch_hazard(base).is_none()
        };
        let base = match std::env::var_os("MOIRAI_TOKCOUNT_TEST_ROOT") {
            Some(base) => {
                let base = PathBuf::from(base);
                assert!(
                    usable(&base),
                    "MOIRAI_TOKCOUNT_TEST_ROOT {} must be an absolute directory with no .git, .claude, CLAUDE.md \
                     or CLAUDE.local.md in it or above it",
                    base.display()
                );
                base
            }
            None => [
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
                std::env::temp_dir(),
            ]
            .into_iter()
            .find(|base| usable(base))
            .expect(
                "no temporary directory free of .git, .claude and CLAUDE.md ancestors; set \
                 MOIRAI_TOKCOUNT_TEST_ROOT (see the module documentation)",
            ),
        };
        let root = base.join(format!("moirai-tokcount-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("claude-config")).unwrap();
        Self(root)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn config_dir(&self) -> PathBuf {
        self.0.join("claude-config")
    }

    fn echo_path(&self) -> PathBuf {
        self.0.join("echo.json")
    }

    fn echo(&self) -> Value {
        read_json(&self.echo_path())
    }

    /// The defaults, with `fake-claude` as the executable replaying `fixture` and echoing to [`Root::echo_path`],
    /// and no managed-policy directory.
    fn config(&self, fixture: &str) -> RunnerConfig {
        let mut cfg = RunnerConfig::new(&self.0).unwrap();
        cfg.claude_exe = Some(PathBuf::from(FAKE));
        cfg.managed_policy_dirs = Vec::new();
        set_env(
            &mut cfg,
            "MOIRAI_FAKE_CLAUDE_FIXTURE",
            &testdata_text(fixture),
        );
        set_env(
            &mut cfg,
            "MOIRAI_FAKE_CLAUDE_ECHO",
            &self.echo_path().display().to_string(),
        );
        cfg
    }

    fn with_env(&self, fixture: &str, vars: &[(&str, &str)]) -> RunnerConfig {
        let mut cfg = self.config(fixture);
        for (name, value) in vars {
            set_env(&mut cfg, name, value);
        }
        cfg
    }

    /// A configuration file for the CLI: `fake-claude` replaying `fixture`, no managed-policy directory, and
    /// `extra` keys (whose `env` object, if any, adds to the fake's).
    fn cli_config(&self, name: &str, fixture: &str, extra: &Value) -> PathBuf {
        let mut config = json!({
            "runner-root": self.0.display().to_string(),
            "claude-exe": FAKE,
            "managed-policy-dirs": [],
            "env": {
                "MOIRAI_FAKE_CLAUDE_FIXTURE": testdata_text(fixture),
                "MOIRAI_FAKE_CLAUDE_ECHO": self.echo_path().display().to_string(),
            },
        });
        for (key, value) in extra.as_object().unwrap() {
            match (key.as_str(), value) {
                ("env", Value::Object(vars)) => {
                    for (var, text) in vars {
                        config["env"][var] = text.clone();
                    }
                }
                _ => config[key] = value.clone(),
            }
        }
        self.write(name, &config.to_string())
    }

    /// Whether the scratch root holds nothing but the slots' lock files: every work and home directory is gone.
    fn no_work_left(&self) -> bool {
        fs::read_dir(self.0.join("scratch")).unwrap().all(|entry| {
            let entry = entry.unwrap();
            entry.file_type().unwrap().is_file()
                && entry.file_name().to_string_lossy().ends_with(".lock")
        })
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Sets `name` in the configuration's `env`, replacing an earlier value.
fn set_env(cfg: &mut RunnerConfig, name: &str, value: &str) {
    cfg.env.retain(|(n, _)| n != name);
    cfg.env.push((name.to_owned(), value.to_owned()));
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

/// Every variable the fake received is one the runner passes on, sets itself, or the test's control of the fake
/// (Windows' `=C:`-style per-drive directories aside).
fn assert_child_env(echo: &Value) {
    for name in strings(&echo["names"]) {
        let allowed = is_passed_env(OsStr::new(&name))
            || RESERVED_ENV.contains(&name.as_str())
            || name.starts_with("MOIRAI_FAKE_CLAUDE_")
            || name.starts_with('=');
        assert!(allowed, "{name} reached the child");
    }
}

fn call(cfg: RunnerConfig) -> Result<moirai_tokcount::claude::CallRecord, Error> {
    Claude::new(cfg)?.call(&Request::new(Prompt::Text("q")), None)
}

/// Waits up to `limit` for `path` to exist.
fn wait_for(path: &Path, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    path.exists()
}

/// Whether the descendant's heartbeat has stopped: its `beat` counter stays the same over a second, ten of its
/// periods. A slow machine can only make a live descendant look dead, never a dead one alive.
fn beats_stopped(marks: &Path) -> bool {
    let beat = || fs::read_to_string(marks.join("beat")).unwrap_or_default();
    thread::sleep(Duration::from_millis(300));
    let before = beat();
    thread::sleep(Duration::from_secs(1));
    before == beat()
}

#[test]
fn a_call_pins_the_model_and_keeps_prompt_and_system_text_out_of_argv() {
    let root = Root::new("call");
    let system = root.write("card.md", "APPENDED-SYSTEM-TEXT");
    let mcp = root.write("mcp.json", "{\"mcpServers\":{}}");
    let claude = Claude::new(root.config("success.jsonl")).unwrap();
    let mut request = Request::new(Prompt::Text("PROMPT-TEXT-SENTINEL"));
    request.append_system_file = Some(&system);
    request.mcp_config_file = Some(&mcp);
    request.allowed_tools = &["mcp__bench__moirai_q"];
    let mut raw = Vec::new();
    let rec = claude.call(&request, Some(&mut raw)).unwrap();

    assert_eq!(raw, fs::read(testdata("success.jsonl")).unwrap());
    assert_eq!(rec.pinned_model, PIN);
    assert_eq!(rec.model, PIN);
    assert_eq!(rec.claude_code_version.as_deref(), Some("2.1.110"));
    assert_eq!(rec.session_id, "0d6f2c1a-5b7e-4c3d-9a8b-1e2f3a4b5c01");
    assert_eq!(rec.api_key_source.as_deref(), Some("none"));
    assert_eq!(
        (rec.subtype.as_str(), rec.is_error, rec.num_turns),
        ("success", false, 1)
    );
    assert_eq!(
        (
            rec.usage.input,
            rec.usage.cache_read,
            rec.usage.cache_write,
            rec.usage.output
        ),
        (3, 11206, 2841, 4)
    );
    assert_eq!(rec.pinned_usage().unwrap().usage.total_input(), 14050);
    assert_eq!(rec.exit_code, Some(0));
    assert_eq!(rec.unparsed_lines, 0);
    assert!(rec.mcp_config && rec.mcp_failures().is_empty());
    assert!(!rec.killed_after_result && !rec.pipes_held_after_exit);
    assert!(rec.owner_context.is_empty() && rec.succeeded());

    let echo = root.echo();
    let expected: Vec<String> = [
        "-p",
        "--model",
        PIN,
        "--output-format",
        "stream-json",
        "--verbose",
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--max-turns",
        "1",
        "--tools",
        "",
        "--allowed-tools",
        "mcp__bench__moirai_q",
        "--disable-slash-commands",
        "--mcp-config",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .chain([
        std::path::absolute(&mcp).unwrap().display().to_string(),
        "--append-system-prompt-file".to_owned(),
        std::path::absolute(&system).unwrap().display().to_string(),
    ])
    .collect();
    let argv = strings(&echo["argv"]);
    assert_eq!(argv, expected);
    assert!(
        argv.iter()
            .all(|a| !a.contains("SENTINEL") && !a.contains("APPENDED"))
    );
    assert_eq!(echo["stdin"], "PROMPT-TEXT-SENTINEL");
    assert_eq!(echo["append_system_text"], "APPENDED-SYSTEM-TEXT");
    assert_eq!(
        echo["env"]["CLAUDE_CONFIG_DIR"],
        root.config_dir().display().to_string()
    );
    assert_eq!(echo["env"]["DISABLE_AUTOUPDATER"], "1");
    assert_eq!(echo["env"]["CLAUDE_CODE_DISABLE_AUTO_MEMORY"], "1");
    assert_child_env(&echo);
    assert_eq!(
        PathBuf::from(echo["cwd"].as_str().unwrap()),
        root.0.join("scratch").join("work-0")
    );
    assert!(root.no_work_left(), "the work directory was not removed");
}

#[test]
fn a_benchmark_shaped_call_records_tools_servers_and_models() {
    let root = Root::new("mcp");
    let mut cfg = root.with_env(
        "mcp-three-turns.jsonl",
        &[("MOIRAI_FAKE_CLAUDE_EXTRA", "x")],
    );
    cfg.session_persistence = false;
    cfg.disable_slash_commands = false;
    cfg.exclude_dynamic_sections = true;
    let claude = Claude::new(cfg).unwrap();
    let prompt = root.write("prompt.txt", "How many tasks are there?");
    let mut request = Request::new(Prompt::File(&prompt));
    request.builtin_tools = BuiltinTools::Only(&["Read"]);
    request.disallowed_tools = &["Bash"];
    request.max_turns = 3;
    let rec = claude.call(&request, None).unwrap();

    assert_eq!(
        rec.tools,
        ["mcp__bench__moirai_q", "mcp__bench__moirai_named"]
    );
    assert_eq!(
        (
            rec.mcp_servers[0].name.as_str(),
            rec.mcp_servers[0].status.as_str()
        ),
        ("bench", "connected")
    );
    assert_eq!(rec.num_turns, 3);
    assert_eq!(rec.result.as_deref(), Some("There are 12 tasks."));
    assert!(rec.exclude_dynamic_sections);
    let models: Vec<&str> = rec.model_usage.iter().map(|m| m.model.as_str()).collect();
    assert_eq!(models, ["claude-haiku-4-5", PIN]);
    let pinned = rec.pinned_usage().unwrap();
    assert_eq!(pinned.usage.total_input(), 11 + 26514 + 4173);
    assert_eq!(pinned.cost_usd, Some(0.0516));

    let echo = root.echo();
    let argv = strings(&echo["argv"]);
    let pair = |flag: &str| {
        argv.iter()
            .position(|a| a == flag)
            .map(|i| argv[i + 1].clone())
    };
    assert_eq!(pair("--tools").as_deref(), Some("Read"));
    assert_eq!(pair("--disallowed-tools").as_deref(), Some("Bash"));
    assert_eq!(pair("--max-turns").as_deref(), Some("3"));
    assert!(argv.iter().any(|a| a == "--no-session-persistence"));
    assert!(
        argv.iter()
            .any(|a| a == "--exclude-dynamic-system-prompt-sections")
    );
    assert!(
        !argv
            .iter()
            .any(|a| a == "--disable-slash-commands" || a == "--mcp-config")
    );
    assert_eq!(echo["stdin"], "How many tasks are there?");
    assert_eq!(echo["env"]["MOIRAI_FAKE_CLAUDE_EXTRA"], "x");
}

#[test]
fn an_error_subtype_is_a_record_not_a_failure() {
    let root = Root::new("maxturns");
    let rec =
        call(root.with_env("error-max-turns.jsonl", &[("MOIRAI_FAKE_CLAUDE_EXIT", "1")])).unwrap();
    assert!(rec.is_error && !rec.succeeded());
    assert_eq!(rec.subtype, "error_max_turns");
    assert_eq!(rec.result, None);
    assert_eq!(rec.errors, ["Reached maximum number of turns (1)"]);
    assert_eq!(rec.permission_denials, 1);
    assert_eq!(rec.exit_code, Some(1));
}

#[test]
fn another_model_or_an_api_key_login_is_refused() {
    let root = Root::new("refuse");
    let wrong = call(root.config("wrong-model.jsonl"));
    assert!(
        matches!(wrong, Err(Error::ModelMismatch { ref reported, .. }) if reported == "claude-sonnet-5")
    );
    // The init reports the pin, and a main-loop turn is then served by another model.
    let fallback = call(root.config("fallback-model.jsonl"));
    assert!(
        matches!(fallback, Err(Error::ModelMismatch { ref pinned, ref reported }) if pinned == PIN && reported == "claude-sonnet-5")
    );
    let key = call(root.config("api-key.jsonl"));
    assert!(matches!(key, Err(Error::ApiKeySource(ref s)) if s == "ANTHROPIC_API_KEY"));
    assert!(root.no_work_left());
}

#[test]
fn owner_context_in_the_session_is_refused_or_recorded() {
    let root = Root::new("context");
    let expected = [
        "agent fixture-reviewer",
        "skill fixture-skill",
        "plugin fixture-plugin",
    ];
    match call(root.config("owner-context.jsonl")) {
        Err(Error::SessionContext(found)) => assert_eq!(found, expected),
        other => panic!("expected a context refusal, got {other:?}"),
    }
    let mut cfg = root.config("owner-context.jsonl");
    cfg.owner_context = ContextPolicy::Record;
    let rec = call(cfg).unwrap();
    assert_eq!(rec.owner_context, expected);
    assert_eq!(rec.slash_commands, ["fixture-command"]);
    assert_eq!(rec.to_json()["owner_context"][1], "skill fixture-skill");
    // A configured agent list that names the fixture's agent leaves only the skill and the plugin.
    let mut cfg = root.config("owner-context.jsonl");
    cfg.builtin_agents.push("fixture-reviewer".to_owned());
    match call(cfg) {
        Err(Error::SessionContext(found)) => assert_eq!(found, expected[1..]),
        other => panic!("expected a context refusal, got {other:?}"),
    }
    assert!(root.no_work_left());
}

/// A server of `--mcp-config` that did not connect would leave its tools out of the measured context.
#[test]
fn an_mcp_server_that_did_not_connect_is_refused_or_recorded() {
    let root = Root::new("mcpfailed");
    let mcp = root.write("mcp.json", "{\"mcpServers\":{}}");
    let mut request = Request::new(Prompt::Text("q"));
    request.mcp_config_file = Some(&mcp);
    let refused = Claude::new(root.config("mcp-failed.jsonl"))
        .unwrap()
        .call(&request, None);
    match refused {
        Err(Error::McpNotConnected(found)) => assert_eq!(found, ["bench (failed)"]),
        other => panic!("expected an MCP refusal, got {other:?}"),
    }
    let mut cfg = root.config("mcp-failed.jsonl");
    cfg.mcp_failed = ContextPolicy::Record;
    let rec = Claude::new(cfg).unwrap().call(&request, None).unwrap();
    assert_eq!(rec.mcp_failures(), ["bench (failed)"]);
    // Without --mcp-config no server is expected, and the status is only recorded.
    let rec = call(root.config("mcp-failed.jsonl")).unwrap();
    assert!(!rec.mcp_config && rec.mcp_failures().is_empty());
    assert_eq!(rec.mcp_servers[0].status, "failed");
    assert!(root.no_work_left());
}

#[test]
fn the_version_pin_is_enforced() {
    let root = Root::new("pin");
    let mut cfg = root.config("success.jsonl");
    cfg.claude_code_version = Some("2.1.110".to_owned());
    let claude = Claude::new(cfg.clone()).unwrap();
    assert!(claude.call(&Request::new(Prompt::Text("q")), None).is_ok());
    // `fake-claude --version` prints 0.0.0-fake, which the pin refuses.
    assert!(matches!(
        claude.version(),
        Err(Error::VersionMismatch { reported: Some(ref r), .. }) if r == "0.0.0-fake"
    ));
    cfg.claude_code_version = Some("2.1.281".to_owned());
    assert!(matches!(
        call(cfg),
        Err(Error::VersionMismatch { ref pinned, reported: Some(ref r) }) if pinned == "2.1.281" && r == "2.1.110"
    ));
}

#[test]
fn a_stream_without_its_result_reports_exit_code_and_stderr() {
    let root = Root::new("incomplete");
    let cfg = root.with_env(
        "no-result.jsonl",
        &[
            ("MOIRAI_FAKE_CLAUDE_EXIT", "1"),
            ("MOIRAI_FAKE_CLAUDE_STDERR", "fixture: API Error 529"),
        ],
    );
    match call(cfg).unwrap_err() {
        Error::Incomplete {
            missing,
            exit_code,
            stderr,
        } => {
            assert_eq!(missing, "result");
            assert_eq!(exit_code, Some(1));
            assert!(stderr.contains("API Error 529"), "{stderr}");
        }
        other => panic!("expected an incomplete stream, got {other:?}"),
    }
}

/// On a timeout the whole tree dies: `fake-claude`, held open by a release file that never comes, and the
/// descendant it started, which stands for an MCP server and lives well beyond the timeout.
#[test]
fn a_timeout_kills_the_process_tree() {
    let root = Root::new("timeout");
    let marks = root.dir("marks");
    let marks_text = marks.display().to_string();
    let never = root.0.join("never-released").display().to_string();
    let mut cfg = root.with_env(
        "success.jsonl",
        &[
            ("MOIRAI_FAKE_CLAUDE_WAIT_FOR", &never),
            ("MOIRAI_FAKE_CLAUDE_GRANDCHILD_MS", "30000"),
            ("MOIRAI_FAKE_CLAUDE_GRANDCHILD_DIR", &marks_text),
        ],
    );
    cfg.timeout = Duration::from_secs(5);
    let claude = Claude::new(cfg).unwrap();
    let err = claude
        .call(&Request::new(Prompt::Text("q")), None)
        .unwrap_err();
    assert!(
        matches!(err, Error::Timeout { after, .. } if after == Duration::from_secs(5)),
        "{err}"
    );
    assert!(
        marks.join("started").exists(),
        "the descendant never started"
    );
    assert!(beats_stopped(&marks), "the descendant outlived the kill");
    assert!(!marks.join("survived").exists());
    assert!(root.no_work_left());
}

/// A descendant that keeps stdout open after Claude Code exits holds the call for the exit grace only. On Windows
/// the runner then ends it, so it no longer holds the work directory; elsewhere it runs on (the documented residual
/// leak) and ends by itself.
#[test]
fn a_descendant_holding_the_pipes_does_not_hold_the_call() {
    let root = Root::new("grandchild");
    let marks = root.dir("marks");
    let marks_text = marks.display().to_string();
    let cfg = root.with_env(
        "success.jsonl",
        &[
            ("MOIRAI_FAKE_CLAUDE_GRANDCHILD_MS", "20000"),
            ("MOIRAI_FAKE_CLAUDE_GRANDCHILD_DIR", &marks_text),
        ],
    );
    let rec = call(cfg).unwrap();
    assert_eq!(rec.model, PIN);
    assert_eq!(rec.exit_code, Some(0));
    assert!(rec.pipes_held_after_exit && !rec.killed_after_result);
    assert_eq!(rec.to_json()["pipes_held_after_exit"], true);
    assert!(marks.join("started").exists());
    assert!(
        !marks.join("survived").exists(),
        "the call waited for the descendant"
    );
    // The same run-time choice the runner makes (`kill_survivors`); no compile-time OS code outside moirai-os
    // (PLAN §2.1, GT20 (d)).
    if std::env::consts::OS == "windows" {
        assert!(beats_stopped(&marks), "the descendant was not ended");
        assert!(
            root.no_work_left(),
            "the descendant kept the work directory"
        );
    } else {
        assert!(wait_for(&marks.join("survived"), Duration::from_secs(60)));
    }
}

/// A Claude Code that lingers after its complete result is killed after the post-result grace, and the paid call's
/// record is kept, not thrown away in a timeout.
#[test]
fn a_process_lingering_after_its_result_is_killed_and_its_record_kept() {
    let root = Root::new("linger");
    let mut cfg = root.with_env(
        "success.jsonl",
        &[("MOIRAI_FAKE_CLAUDE_LINGER_MS", "120000")],
    );
    cfg.post_result_grace = Duration::from_millis(500);
    cfg.timeout = Duration::from_secs(60);
    let rec = call(cfg).unwrap();
    assert!(rec.killed_after_result && !rec.pipes_held_after_exit);
    assert_eq!(rec.exit_code, None);
    assert!(rec.succeeded());
    assert_eq!(rec.pinned_usage().unwrap().usage.total_input(), 14050);
    assert_eq!(rec.to_json()["killed_after_result"], true);
    assert!(root.no_work_left());
}

#[test]
fn an_over_long_line_kills_the_process() {
    let root = Root::new("longline");
    let long = format!(
        "{{\"type\":\"assistant\",\"pad\":\"{}\"}}\n",
        "x".repeat(4096)
    );
    let fixture = root.write("long.jsonl", &long);
    let mut cfg = root.with_env(
        "success.jsonl",
        &[("MOIRAI_FAKE_CLAUDE_FIXTURE", &fixture.display().to_string())],
    );
    cfg.max_line_bytes = 1024;
    let err = call(cfg).unwrap_err();
    assert!(matches!(err, Error::LineTooLong { limit: 1024 }), "{err}");
    assert!(root.no_work_left());
}

#[test]
fn unknown_events_and_non_json_lines_are_read_past() {
    let root = Root::new("noise");
    let rec = call(root.config("noise.jsonl")).unwrap();
    assert_eq!(rec.unparsed_lines, 2);
    assert_eq!(rec.usage.cache_read, 14047);
}

#[test]
fn a_texts_claude_tokens_are_the_with_without_difference() {
    let root = Root::new("delta");
    let without = call(root.config("success.jsonl")).unwrap();
    let with = call(root.config("success-with-text.jsonl")).unwrap();
    assert_eq!(text_tokens(&with, &without).unwrap(), 1017);
    assert!(matches!(text_tokens(&without, &with), Err(Error::Delta(_))));
    let three_turns = call(root.config("mcp-three-turns.jsonl")).unwrap();
    assert!(matches!(
        text_tokens(&three_turns, &without),
        Err(Error::Delta(ref m)) if m.contains("3 turns")
    ));
}

#[test]
fn the_isolation_check_compares_the_owners_home_with_an_empty_one() {
    let root = Root::new("isolation");
    let empty_home = testdata_text("success.jsonl");
    let clean = root.with_env(
        "success.jsonl",
        &[("MOIRAI_FAKE_CLAUDE_EMPTY_HOME_FIXTURE", &empty_home)],
    );
    let check = Claude::new(clean)
        .unwrap()
        .isolation_check(&Request::new(Prompt::Text(ISOLATION_PROMPT)))
        .unwrap();
    assert!(check.passed());
    assert_eq!(check.difference, 0);
    // The echo is the second call's: its home was the slot's empty directory.
    let echo = root.echo();
    let home = root.0.join("scratch").join("home-0").display().to_string();
    assert_eq!(echo["env"]["USERPROFILE"], home.as_str());
    assert_eq!(echo["env"]["HOME"], home.as_str());
    assert_eq!(echo["stdin"], ISOLATION_PROMPT);
    assert!(root.no_work_left());

    // Something in the owner's home reached the context: 1,017 more input tokens than with an empty home.
    let leaky = root.with_env(
        "success-with-text.jsonl",
        &[("MOIRAI_FAKE_CLAUDE_EMPTY_HOME_FIXTURE", &empty_home)],
    );
    let check = Claude::new(leaky)
        .unwrap()
        .isolation_check(&Request::new(Prompt::Text(ISOLATION_PROMPT)))
        .unwrap();
    assert!(!check.passed());
    assert_eq!(check.difference, 1017);
    let json = check.to_json();
    assert_eq!(json["record"], "moirai-tokcount.claude-isolation.v1");
    assert_eq!(json["passed"], false);
    assert_eq!(json["configured"]["usage"]["cache_write"], 3858);
    assert_eq!(json["isolated"]["usage"]["cache_write"], 2841);
}

/// A call held open by a release file keeps slot 0; a one-slot runner is then busy, and another call takes slot 1.
#[test]
fn concurrent_calls_take_separate_work_slots() {
    let root = Root::new("slots");
    let slow_echo = root.0.join("echo-slow.json");
    let release = root.0.join("release");
    let mut slow = root.with_env(
        "success.jsonl",
        &[(
            "MOIRAI_FAKE_CLAUDE_WAIT_FOR",
            &release.display().to_string(),
        )],
    );
    set_env(
        &mut slow,
        "MOIRAI_FAKE_CLAUDE_ECHO",
        &slow_echo.display().to_string(),
    );
    let slow = Claude::new(slow).unwrap();
    let mut one_slot = root.config("success.jsonl");
    one_slot.work_slots = 1;
    let one_slot = Claude::new(one_slot).unwrap();
    let quick = Claude::new(root.config("success.jsonl")).unwrap();
    thread::scope(|s| {
        let first = s.spawn(|| slow.call(&Request::new(Prompt::Text("slow")), None));
        // The fake writes its echo before it waits for the release, so slot 0 is held from here on.
        let started = wait_for(&slow_echo, Duration::from_secs(60));
        let busy = one_slot.call(&Request::new(Prompt::Text("busy")), None);
        let second = quick.call(&Request::new(Prompt::Text("quick")), None);
        fs::write(&release, "").unwrap();
        assert!(started, "the held call never started");
        assert!(first.join().unwrap().is_ok());
        assert!(second.is_ok(), "{second:?}");
        assert!(
            matches!(busy, Err(Error::Busy { slots: 1, .. })),
            "{busy:?}"
        );
    });
    let scratch = root.0.join("scratch");
    assert_eq!(
        PathBuf::from(read_json(&slow_echo)["cwd"].as_str().unwrap()),
        scratch.join("work-0")
    );
    assert_eq!(
        PathBuf::from(root.echo()["cwd"].as_str().unwrap()),
        scratch.join("work-1")
    );
    assert!(root.no_work_left());
    // With every call done, the next one takes slot 0 again, so sequential calls share one working directory.
    call(root.config("success.jsonl")).unwrap();
    assert_eq!(
        PathBuf::from(root.echo()["cwd"].as_str().unwrap()),
        scratch.join("work-0")
    );
}

#[test]
fn the_version_needs_no_model_call() {
    let root = Root::new("version");
    assert_eq!(
        Claude::new(root.config("success.jsonl"))
            .unwrap()
            .version()
            .unwrap(),
        "0.0.0-fake"
    );
    assert!(
        !root.echo_path().exists(),
        "--version must not reach the call path"
    );
}

/// What Claude Code writes into its config directory on its own (plugin bookkeeping with a marketplace clone, empty
/// definition directories, the login, the projects) puts nothing in the context and is accepted; a definition in
/// any definition directory, or an installed plugin, is refused.
#[test]
fn a_config_dir_with_bookkeeping_only_is_accepted() {
    let root = Root::new("bookkeeping");
    let cfg_dir = root.config_dir();
    let plugins = cfg_dir.join("plugins");
    let clone: PathBuf = [
        "marketplaces",
        "fixture-marketplace",
        "plugins",
        "fixture",
        "agents",
    ]
    .iter()
    .collect();
    fs::create_dir_all(plugins.join(&clone)).unwrap();
    fs::write(
        plugins.join(&clone).join("fixture.md"),
        "a synthetic definition inside a marketplace clone",
    )
    .unwrap();
    fs::write(
        plugins.join("known_marketplaces.json"),
        "{\"fixture-marketplace\":{\"source\":{\"source\":\"directory\",\"path\":\"/fixture\"}}}",
    )
    .unwrap();
    let installed = plugins.join("installed_plugins.json");
    fs::write(&installed, "{\"version\":2,\"plugins\":{}}").unwrap();
    for dir in [
        "agents",
        "commands",
        "skills",
        "rules",
        "output-styles",
        "projects",
    ] {
        fs::create_dir_all(cfg_dir.join(dir)).unwrap();
    }
    fs::write(cfg_dir.join(".credentials.json"), "{}").unwrap();
    let rec = call(root.config("success.jsonl")).unwrap();
    assert!(rec.succeeded());

    let refused = |expected: &Path| match Claude::new(root.config("success.jsonl")) {
        Err(Error::Context { path, .. }) => assert_eq!(path, expected),
        other => panic!(
            "expected a refusal of {}, got {other:?}",
            expected.display()
        ),
    };
    let agent = cfg_dir.join("agents").join("reviewer.md");
    fs::write(&agent, "a synthetic agent").unwrap();
    refused(&agent);
    fs::remove_file(&agent).unwrap();
    fs::write(
        &installed,
        "{\"version\":2,\"plugins\":{\"fixture@fixture-marketplace\":[{\"scope\":\"user\"}]}}",
    )
    .unwrap();
    refused(&installed);
}

#[test]
fn a_managed_policy_file_refuses_every_call() {
    let root = Root::new("policy");
    let policy = root.dir("policy");
    let mut cfg = root.config("success.jsonl");
    cfg.managed_policy_dirs = vec![root.0.join("absent"), policy.clone()];
    assert!(Claude::new(cfg.clone()).is_ok());
    fs::write(policy.join("managed-mcp.json"), "{\"mcpServers\":{}}").unwrap();
    match Claude::new(cfg) {
        Err(Error::Context { path, .. }) => assert_eq!(path, policy.join("managed-mcp.json")),
        other => panic!("expected a managed-policy refusal, got {other:?}"),
    }
}

#[test]
fn preflight_refusals() {
    let root = Root::new("preflight");
    let cfg_dir = root.config_dir();

    let mut missing = root.config("success.jsonl");
    missing.config_dir = root.0.join("absent-config");
    assert!(matches!(Claude::new(missing), Err(Error::Config(ref m)) if m.contains("/login")));

    fs::write(cfg_dir.join("CLAUDE.md"), "owner text").unwrap();
    assert!(matches!(
        Claude::new(root.config("success.jsonl")),
        Err(Error::Context { .. })
    ));
    fs::remove_file(cfg_dir.join("CLAUDE.md")).unwrap();
    let skill = cfg_dir.join("skills").join("fixture");
    fs::create_dir_all(&skill).unwrap();
    assert!(Claude::new(root.config("success.jsonl")).is_ok());
    fs::write(skill.join("SKILL.md"), "a synthetic skill").unwrap();
    assert!(matches!(
        Claude::new(root.config("success.jsonl")),
        Err(Error::Context { .. })
    ));
    fs::remove_dir_all(cfg_dir.join("skills")).unwrap();

    // settings.json is read only with setting-sources `user`, and then it may not set env, hooks and the like.
    fs::write(
        cfg_dir.join("settings.json"),
        "{\"env\":{\"ANTHROPIC_BASE_URL\":\"http://127.0.0.1:9\"}}",
    )
    .unwrap();
    assert!(Claude::new(root.config("success.jsonl")).is_ok());
    let mut user = root.config("success.jsonl");
    user.setting_sources = "user".to_owned();
    match Claude::new(user.clone()) {
        Err(Error::Context { path, .. }) => assert!(path.ends_with("settings.json")),
        other => panic!("expected a settings refusal, got {other:?}"),
    }
    fs::write(cfg_dir.join("settings.json"), "{\"permissions\":{}}").unwrap();
    assert!(Claude::new(user).is_ok());

    // A scratch root with Claude Code context above it is refused before any directory of it is created.
    for (dir, entry, is_dir) in [
        ("repo", ".git", true),
        ("project", ".claude", true),
        ("notes", "CLAUDE.md", false),
        ("local", "CLAUDE.local.md", false),
    ] {
        let parent = root.dir(dir);
        if is_dir {
            fs::create_dir(parent.join(entry)).unwrap();
        } else {
            fs::write(parent.join(entry), "x").unwrap();
        }
        let mut cfg = root.config("success.jsonl");
        cfg.scratch_root = parent.join("deep").join("scratch");
        match Claude::new(cfg) {
            Err(Error::Context { path, .. }) => {
                assert!(path.ends_with(entry), "{}", path.display());
            }
            other => panic!("{entry}: expected a context refusal, got {other:?}"),
        }
        assert!(
            !parent.join("deep").exists(),
            "{entry}: the refused scratch root was created"
        );
    }
    let mut cfg = root.config("success.jsonl");
    cfg.scratch_root = root.0.join("free").join(".claude").join("scratch");
    assert!(matches!(Claude::new(cfg), Err(Error::Context { .. })));
    assert!(!root.0.join("free").exists());

    let shim = root.write("claude.cmd", "@ECHO off\r\n");
    let mut cfg = root.config("success.jsonl");
    cfg.claude_exe = Some(shim);
    assert!(matches!(Claude::new(cfg), Err(Error::Exe(ref m)) if m.contains("shim")));

    let mut cfg = root.config("success.jsonl");
    set_env(&mut cfg, "ANTHROPIC_API_KEY", "not-a-key");
    assert!(matches!(Claude::new(cfg), Err(Error::Config(ref m)) if m.contains("API key")));

    let claude = Claude::new(root.config("success.jsonl")).unwrap();
    let absent = root.0.join("no-such-prompt.txt");
    let mut request = Request::new(Prompt::File(&absent));
    assert!(matches!(
        claude.call(&request, None),
        Err(Error::Request(_))
    ));
    request.prompt = Prompt::Text("q");
    request.append_system_file = Some(&absent);
    assert!(matches!(
        claude.call(&request, None),
        Err(Error::Request(_))
    ));
    request.append_system_file = None;
    request.max_turns = 0;
    assert!(matches!(
        claude.call(&request, None),
        Err(Error::Request(_))
    ));
    request.max_turns = 1;
    request.allowed_tools = &["a,b"];
    assert!(matches!(
        claude.call(&request, None),
        Err(Error::Request(_))
    ));
    assert!(
        !root.echo_path().exists(),
        "a refused request must not start the process"
    );
}

#[test]
fn the_transcript_is_found_by_session_id() {
    let root = Root::new("transcript");
    let claude = Claude::new(root.config("success.jsonl")).unwrap();
    let rec = claude.call(&Request::new(Prompt::Text("q")), None).unwrap();
    assert_eq!(claude.transcript_path(&rec).unwrap(), None);
    let file = format!("{}.jsonl", rec.session_id);
    let projects = root.config_dir().join("projects");
    // A project directory the working directory's slug does not name (a shortened name) is found by search.
    let shortened = projects.join("-runner-scratch-w-1a2b3c");
    fs::create_dir_all(&shortened).unwrap();
    fs::write(shortened.join(&file), "{}\n").unwrap();
    assert_eq!(
        claude.transcript_path(&rec).unwrap(),
        Some(shortened.join(&file))
    );
    // The slug of the reported working directory (`/runner/scratch/work-0` in the fixture) is looked at first.
    let direct = projects.join("-runner-scratch-work-0");
    fs::create_dir_all(&direct).unwrap();
    fs::write(direct.join(&file), "{}\n").unwrap();
    assert_eq!(
        claude.transcript_path(&rec).unwrap(),
        Some(direct.join(&file))
    );
    let mut forged = rec;
    forged.session_id = "../x".to_owned();
    assert!(matches!(
        claude.transcript_path(&forged),
        Err(Error::Request(_))
    ));
}

#[test]
fn the_cli_counts_calls_and_checks_isolation() {
    let root = Root::new("cli");
    let text = root.write("text.txt", "hello world");
    let out = Command::new(TOKCOUNT)
        .arg("o200k")
        .arg(&text)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("2\t{}\n", text.display())
    );

    let empty_home = json!({ "env": { "MOIRAI_FAKE_CLAUDE_EMPTY_HOME_FIXTURE": testdata_text("success.jsonl") } });
    let success = root.cli_config("runner.json", "success.jsonl", &empty_home);
    let prompt = root.write("prompt.txt", "Reply OK.");
    let raw = root.0.join("raw.jsonl");
    // Only the allowed variables of the parent reach the child: no API key, Claude Code or MCP variable, no git
    // redirection, no switch of the measured context.
    let leaks = [
        ("ANTHROPIC_API_KEY", "not-a-key"),
        ("CLAUDE_CODE_OAUTH_TOKEN", "not-a-token"),
        ("CLAUDECODE", "1"),
        ("MCP_TIMEOUT", "1"),
        ("MAX_THINKING_TOKENS", "1"),
        ("GIT_DIR", "D:/elsewhere/.git"),
        ("GIT_WORK_TREE", "D:/elsewhere"),
        ("GIT_INDEX_FILE", "D:/elsewhere/.git/index"),
        ("ENABLE_TOOL_SEARCH", "true"),
        ("DISABLE_PROMPT_CACHING", "1"),
        ("NODE_OPTIONS", "--max-old-space-size=64"),
    ];
    let out = Command::new(TOKCOUNT)
        .arg("claude")
        .arg("--config")
        .arg(&success)
        .arg("--prompt-file")
        .arg(&prompt)
        .arg("--raw")
        .arg(&raw)
        .envs(leaks)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["record"], "moirai-tokcount.claude-call.v1");
    assert_eq!(record["pinned_model"], PIN);
    assert_eq!(record["model"], PIN);
    assert_eq!(record["usage"]["total_input"], 14050);
    assert_eq!(record["transcript"], Value::Null);
    assert_eq!(
        fs::read(&raw).unwrap(),
        fs::read(testdata("success.jsonl")).unwrap()
    );
    let echo = root.echo();
    assert_eq!(echo["stdin"], "Reply OK.");
    let names = strings(&echo["names"]);
    for (leak, _) in leaks {
        assert!(
            !names.iter().any(|n| n.eq_ignore_ascii_case(leak)),
            "{leak} reached the child"
        );
    }
    assert_child_env(&echo);

    let error = root.cli_config("runner-error.json", "error-max-turns.jsonl", &json!({}));
    let out = Command::new(TOKCOUNT)
        .arg("claude")
        .arg("--config")
        .arg(&error)
        .arg("--prompt-file")
        .arg(&prompt)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["subtype"], "error_max_turns");

    // A process that lingers after its result is killed, and the call still succeeds.
    let linger = root.cli_config(
        "runner-linger.json",
        "success.jsonl",
        &json!({ "post-result-grace-ms": 200, "env": { "MOIRAI_FAKE_CLAUDE_LINGER_MS": "120000" } }),
    );
    let out = Command::new(TOKCOUNT)
        .arg("claude")
        .arg("--config")
        .arg(&linger)
        .arg("--prompt-file")
        .arg(&prompt)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["killed_after_result"], true);
    assert_eq!(record["exit_code"], Value::Null);

    let out = Command::new(TOKCOUNT)
        .arg("claude-isolation")
        .arg("--config")
        .arg(&success)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let check: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(check["passed"], true);
    assert_eq!(root.echo()["stdin"], ISOLATION_PROMPT);

    let version = Command::new(TOKCOUNT)
        .arg("claude-version")
        .arg("--config")
        .arg(&success)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "0.0.0-fake"
    );

    for usage in [
        &["claude", "--prompt-file"][..],
        &["claude", "--config", "a", "--config", "b"],
        &["claude-version", "--prompt-file", "p"],
        &["frobnicate"],
    ] {
        let out = Command::new(TOKCOUNT).args(usage).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{usage:?}");
    }
    let failed = Command::new(TOKCOUNT)
        .arg("o200k")
        .arg(root.0.join("absent.txt"))
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
}

/// With `--mcp-config`, a server that did not connect fails the command: refused by default, and printed but still
/// failed under `"mcp-failed": "record"`. Without `--mcp-config`, its status is only recorded.
#[test]
fn the_cli_fails_a_call_whose_mcp_server_did_not_connect() {
    let root = Root::new("climcp");
    let mcp = root.write("mcp.json", "{\"mcpServers\":{}}");
    let prompt = root.write("prompt.txt", "Reply OK.");
    let claude = |config: &Path, with_mcp: bool| {
        let mut command = Command::new(TOKCOUNT);
        command
            .arg("claude")
            .arg("--config")
            .arg(config)
            .arg("--prompt-file")
            .arg(&prompt);
        if with_mcp {
            command.arg("--mcp-config").arg(&mcp);
        }
        command.output().unwrap()
    };
    let refuse = root.cli_config("runner.json", "mcp-failed.jsonl", &json!({}));
    let out = claude(&refuse, true);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bench (failed)"));

    let record = root.cli_config(
        "runner-record.json",
        "mcp-failed.jsonl",
        &json!({ "mcp-failed": "record" }),
    );
    let out = claude(&record, true);
    assert_eq!(out.status.code(), Some(1));
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed["mcp_config"], true);
    assert_eq!(printed["mcp_servers"][0]["status"], "failed");
    assert!(String::from_utf8_lossy(&out.stderr).contains("did not connect"));

    let out = claude(&refuse, false);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `tokcount claude-delta` measures a text by both families ([LQ/card §7.2]): one call without the text and one with
/// it appended to the system prompt, the Claude count their input difference, the o200k count offline.
#[test]
fn the_cli_measures_a_texts_tokens_by_both_families() {
    const CARD: &str = "CARD-TEXT-SENTINEL: a synthetic card body.\n";
    let root = Root::new("clidelta");
    let card = root.write("card.md", CARD);
    let mcp = root.write("mcp.json", "{\"mcpServers\":{}}");
    let config = |without: &str, with_text: &str, name: &str| {
        root.cli_config(
            name,
            without,
            &json!({ "env": { "MOIRAI_FAKE_CLAUDE_APPEND_FIXTURE": testdata_text(with_text) } }),
        )
    };
    let delta = |config: &Path, text: &Path| {
        Command::new(TOKCOUNT)
            .arg("claude-delta")
            .arg("--config")
            .arg(config)
            .arg("--text-file")
            .arg(text)
            .arg("--mcp-config")
            .arg(&mcp)
            .output()
            .unwrap()
    };

    let runner = config("success.jsonl", "success-with-text.jsonl", "runner.json");
    let out = delta(&runner, &card);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["record"], "moirai-tokcount.claude-delta.v1");
    assert_eq!(record["claude_tokens"], 1017);
    assert_eq!(record["claude_tokens_includes_separator"], true);
    let o200k = moirai_tokcount::o200k::count(CARD);
    assert!(o200k > 0 && o200k < 1017);
    assert_eq!(record["o200k_tokens"], o200k);
    assert_eq!(record["max_tokens"], 1017);
    assert_eq!(record["bytes"], CARD.len());
    assert_eq!(record["without"]["usage"]["cache_write"], 2841);
    assert_eq!(record["with"]["usage"]["cache_write"], 3858);
    // The echo is the second call's: the text reached Claude Code as the appended file, never in argv.
    let echo = root.echo();
    assert_eq!(echo["append_system_text"], CARD);
    assert_eq!(echo["stdin"], ISOLATION_PROMPT);
    let argv = strings(&echo["argv"]);
    assert!(argv.iter().all(|a| !a.contains("SENTINEL")));
    assert!(argv.iter().any(|a| a == "--mcp-config"));
    assert!(root.no_work_left());

    // Two calls that cannot be compared (the second took three turns): both records are printed, and it fails.
    let three_turns = config(
        "success.jsonl",
        "mcp-three-turns.jsonl",
        "runner-three.json",
    );
    let out = delta(&three_turns, &card);
    assert_eq!(out.status.code(), Some(1));
    let record: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(record["claude_tokens"], Value::Null);
    assert_eq!(record["max_tokens"], Value::Null);
    assert_eq!(record["with"]["num_turns"], 3);
    assert!(String::from_utf8_lossy(&out.stderr).contains("3 turns"));

    // The benchmark server did not connect: the measured context would lack its tools, so no count is made.
    let failed = config("mcp-failed.jsonl", "mcp-failed.jsonl", "runner-failed.json");
    let out = delta(&failed, &card);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("bench (failed)"));

    // A text that is not UTF-8 is refused before any call.
    let bad = root.0.join("bad.txt");
    fs::write(&bad, [0x66, 0xFF, 0x6F]).unwrap();
    fs::remove_file(root.echo_path()).unwrap();
    assert_eq!(delta(&runner, &bad).status.code(), Some(1));
    assert!(
        !root.echo_path().exists(),
        "a refused text must cost no call"
    );

    // No text file; the appended-file option, whose place the text takes.
    for usage in [
        &["claude-delta", "--config", "c"][..],
        &[
            "claude-delta",
            "--config",
            "c",
            "--text-file",
            "t",
            "--append-system-file",
            "a",
        ],
    ] {
        let out = Command::new(TOKCOUNT).args(usage).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{usage:?}");
    }
}
