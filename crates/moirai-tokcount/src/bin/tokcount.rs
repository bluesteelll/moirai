//! `tokcount`: offline o200k counts, and headless Claude Code calls printed as JSON records (PLAN WP-58; the owner's
//! V9 check runs `tokcount claude` and `tokcount claude-isolation` once each, and the first real calls measure the
//! card with `tokcount claude-delta`, [LQ/card §7.2], [50 §7.4] item 5).
//!
//! ```text
//! tokcount o200k [FILE...]
//!     o200k_base tokens of each UTF-8 FILE ("<count>\t<file>"), or of stdin ("<count>")
//! tokcount claude --config FILE --prompt-file FILE [CALL OPTIONS] [--raw FILE]
//!     one call; prints its record (moirai-tokcount.claude-call.v1) as one JSON line, with "transcript" added;
//!     --raw writes the raw stream-json lines to FILE (keep it under /private/)
//! tokcount claude-isolation --config FILE [--prompt-file FILE] [CALL OPTIONS]
//!     the same call as configured and with an empty home; prints both records and the difference in input tokens
//!     (moirai-tokcount.claude-isolation.v1); the prompt defaults to a one-word reply
//! tokcount claude-delta --config FILE --text-file FILE [--prompt-file FILE] [DELTA OPTIONS]
//!     the tokens of the UTF-8 text in --text-file: the same one-turn call without it and with it appended to the
//!     system prompt, its Claude count the difference of the two calls' input (which includes the separator Claude
//!     Code puts before appended text, about one token: "claude_tokens_includes_separator"); prints both records,
//!     the Claude and o200k counts, their maximum and the byte length (moirai-tokcount.claude-delta.v1); the prompt
//!     defaults to a one-word reply
//! tokcount claude-version --config FILE
//!     the Claude Code version, without a model call
//!
//! CALL OPTIONS:  [--append-system-file FILE] [DELTA OPTIONS]
//! DELTA OPTIONS: [--mcp-config FILE] [--tools none|default|NAME,...] [--allowed-tools NAME,...]
//!                [--disallowed-tools NAME,...] [--max-turns N]
//! ```
//!
//! Exit codes: 0 done; 1 the count or the call failed, a call's record reports an error result, a non-zero exit
//! code (a process killed only after its complete result is no failure), owner context or a server of
//! `--mcp-config` that did not connect, the isolation check found a difference, or the two calls of a delta cannot
//! be compared (the records are still printed when there are some); 2 usage error.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use moirai_tokcount::claude::{
    BuiltinTools, CallRecord, Claude, ISOLATION_PROMPT, Prompt, Request, RunnerConfig, text_tokens,
};
use moirai_tokcount::o200k;
use serde_json::{Value, json};

const USAGE: &str = "usage: tokcount o200k [FILE...]
       tokcount claude --config FILE --prompt-file FILE [CALL OPTIONS] [--raw FILE]
       tokcount claude-isolation --config FILE [--prompt-file FILE] [CALL OPTIONS]
       tokcount claude-delta --config FILE --text-file FILE [--prompt-file FILE] [DELTA OPTIONS]
       tokcount claude-version --config FILE
CALL OPTIONS:  [--append-system-file FILE] [DELTA OPTIONS]
DELTA OPTIONS: [--mcp-config FILE] [--tools none|default|NAME,...] [--allowed-tools NAME,...]
               [--disallowed-tools NAME,...] [--max-turns N]";

const CALL_OPTIONS: [&str; 8] = [
    "--config",
    "--prompt-file",
    "--append-system-file",
    "--mcp-config",
    "--tools",
    "--allowed-tools",
    "--disallowed-tools",
    "--max-turns",
];

/// The options of `claude-delta`: [`CALL_OPTIONS`] without `--append-system-file`, whose place the measured text
/// takes, and with `--text-file`.
const DELTA_OPTIONS: [&str; 8] = [
    "--config",
    "--prompt-file",
    "--text-file",
    "--mcp-config",
    "--tools",
    "--allowed-tools",
    "--disallowed-tools",
    "--max-turns",
];

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    let rest: Vec<OsString> = args.collect();
    let outcome = match command.as_ref().and_then(|c| c.to_str()) {
        Some("o200k") => o200k_cmd(&rest),
        Some("claude") => claude_cmd(&rest),
        Some("claude-isolation") => isolation_cmd(&rest),
        Some("claude-delta") => delta_cmd(&rest),
        Some("claude-version") => version_cmd(&rest),
        _ => Err(Failure::Usage(String::new())),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(msg)) => {
            if !msg.is_empty() {
                eprintln!("tokcount: {msg}");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Run(msg)) => {
            eprintln!("tokcount: {msg}");
            ExitCode::from(1)
        }
    }
}

enum Failure {
    Usage(String),
    Run(String),
}

fn run_err(e: impl std::fmt::Display) -> Failure {
    Failure::Run(e.to_string())
}

fn o200k_cmd(files: &[OsString]) -> Result<(), Failure> {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if files.is_empty() {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| Failure::Run(format!("reading stdin: {e}")))?;
        writeln!(out, "{}", o200k::count(&text)).map_err(run_err)?;
    }
    for file in files {
        let path = PathBuf::from(file);
        let n = o200k::count_file(&path)
            .map_err(|e| Failure::Run(format!("{}: {e}", path.display())))?;
        writeln!(out, "{n}\t{}", path.display()).map_err(run_err)?;
    }
    out.flush().map_err(run_err)
}

/// The options of `claude`, `claude-isolation`, `claude-delta` and `claude-version`, each at most once.
#[derive(Default)]
struct Options {
    config: Option<PathBuf>,
    prompt_file: Option<PathBuf>,
    text_file: Option<PathBuf>,
    append_system_file: Option<PathBuf>,
    mcp_config: Option<PathBuf>,
    tools: Option<String>,
    allowed_tools: Option<String>,
    disallowed_tools: Option<String>,
    max_turns: Option<u32>,
    raw: Option<PathBuf>,
}

fn parse_options(args: &[OsString], allowed: &[&str]) -> Result<Options, Failure> {
    let mut opts = Options::default();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let name = flag
            .to_str()
            .filter(|n| allowed.contains(n))
            .ok_or_else(|| {
                Failure::Usage(format!("unexpected argument {}", flag.to_string_lossy()))
            })?;
        let value = it
            .next()
            .ok_or_else(|| Failure::Usage(format!("{name} needs a value")))?;
        let text = || {
            value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| Failure::Usage(format!("{name}: not UTF-8")))
        };
        let duplicate = match name {
            "--config" => opts.config.replace(PathBuf::from(value)).is_some(),
            "--prompt-file" => opts.prompt_file.replace(PathBuf::from(value)).is_some(),
            "--text-file" => opts.text_file.replace(PathBuf::from(value)).is_some(),
            "--append-system-file" => opts
                .append_system_file
                .replace(PathBuf::from(value))
                .is_some(),
            "--mcp-config" => opts.mcp_config.replace(PathBuf::from(value)).is_some(),
            "--raw" => opts.raw.replace(PathBuf::from(value)).is_some(),
            "--tools" => opts.tools.replace(text()?).is_some(),
            "--allowed-tools" => opts.allowed_tools.replace(text()?).is_some(),
            "--disallowed-tools" => opts.disallowed_tools.replace(text()?).is_some(),
            "--max-turns" => {
                let n = text()?
                    .parse()
                    .map_err(|_| Failure::Usage("--max-turns needs a whole number".to_owned()))?;
                opts.max_turns.replace(n).is_some()
            }
            _ => false,
        };
        if duplicate {
            return Err(Failure::Usage(format!("{name} given twice")));
        }
    }
    Ok(opts)
}

fn runner(opts: &Options) -> Result<Claude, Failure> {
    let config = opts
        .config
        .as_ref()
        .ok_or_else(|| Failure::Usage("--config is required".to_owned()))?;
    let config = RunnerConfig::load(config).map_err(run_err)?;
    Claude::new(config).map_err(run_err)
}

fn split_tools(list: Option<&str>) -> Vec<&str> {
    list.map(|l| l.split(',').filter(|t| !t.is_empty()).collect())
        .unwrap_or_default()
}

/// The tool lists of a request, split once so the request can borrow them.
struct Tools<'a> {
    only: Vec<&'a str>,
    allowed: Vec<&'a str>,
    disallowed: Vec<&'a str>,
}

impl<'a> Tools<'a> {
    fn new(opts: &'a Options) -> Self {
        Self {
            only: split_tools(opts.tools.as_deref()),
            allowed: split_tools(opts.allowed_tools.as_deref()),
            disallowed: split_tools(opts.disallowed_tools.as_deref()),
        }
    }

    fn request(&'a self, opts: &'a Options, prompt: Prompt<'a>) -> Request<'a> {
        let mut request = Request::new(prompt);
        request.append_system_file = opts.append_system_file.as_deref();
        request.mcp_config_file = opts.mcp_config.as_deref();
        request.builtin_tools = match opts.tools.as_deref() {
            None | Some("none") => BuiltinTools::None,
            Some("default") => BuiltinTools::Default,
            Some(_) => BuiltinTools::Only(&self.only),
        };
        request.allowed_tools = &self.allowed;
        request.disallowed_tools = &self.disallowed;
        if let Some(n) = opts.max_turns {
            request.max_turns = n;
        }
        request
    }
}

/// Why a printed record still fails the command: an error result, a non-zero exit code (a process killed only
/// after its complete result is none), owner context, or a server of `--mcp-config` that did not connect (both
/// recorded under the `record` policies).
fn record_failure(record: &CallRecord) -> Option<String> {
    let mcp_failures = record.mcp_failures();
    if record.is_error {
        Some(format!("the call ended in {}", record.subtype))
    } else if !record.succeeded() {
        Some(format!(
            "claude exited with {}",
            record
                .exit_code
                .map_or_else(|| "no exit code".to_owned(), |c| c.to_string())
        ))
    } else if !record.owner_context.is_empty() {
        Some(format!(
            "the session loaded owner context: {}",
            record.owner_context.join(", ")
        ))
    } else if !mcp_failures.is_empty() {
        Some(format!(
            "MCP servers of --mcp-config did not connect: {}",
            mcp_failures.join(", ")
        ))
    } else {
        None
    }
}

fn claude_cmd(args: &[OsString]) -> Result<(), Failure> {
    let mut allowed = CALL_OPTIONS.to_vec();
    allowed.push("--raw");
    let opts = parse_options(args, &allowed)?;
    let prompt_file = opts
        .prompt_file
        .as_deref()
        .ok_or_else(|| Failure::Usage("--prompt-file is required".to_owned()))?;
    let runner = runner(&opts)?;
    let tools = Tools::new(&opts);
    let request = tools.request(&opts, Prompt::File(prompt_file));
    let record = match &opts.raw {
        Some(path) => {
            let file =
                File::create(path).map_err(|e| Failure::Run(format!("{}: {e}", path.display())))?;
            let mut raw = BufWriter::new(file);
            runner.call(&request, Some(&mut raw)).map_err(run_err)?
        }
        None => runner.call(&request, None).map_err(run_err)?,
    };
    let mut json = record.to_json();
    let transcript = runner.transcript_path(&record).map_err(run_err)?;
    json["transcript"] = transcript.map_or(Value::Null, |p| Value::from(p.display().to_string()));
    println!("{json}");
    record_failure(&record).map_or(Ok(()), |why| Err(Failure::Run(why)))
}

/// A text's tokens by both families the gates use ([LQ/card §7.2], [90 §8.3] "tokenizer ledger"): the Claude count
/// from two one-turn calls that differ only in the text appended to the system prompt ([`text_tokens`]), the o200k
/// count offline; a static-text gate takes the larger ([90 §9.1] "How it is used"). The Claude count includes the
/// separator Claude Code puts before appended text (about one token, measured at V9), which the record states with
/// `claude_tokens_includes_separator`: an over-count on the safe side of a "not more than" gate. The text is read
/// first, so a file that is not UTF-8 costs no call.
fn delta_cmd(args: &[OsString]) -> Result<(), Failure> {
    let opts = parse_options(args, &DELTA_OPTIONS)?;
    let text_file = opts
        .text_file
        .as_deref()
        .ok_or_else(|| Failure::Usage("--text-file is required".to_owned()))?;
    let text = fs::read_to_string(text_file)
        .map_err(|e| Failure::Run(format!("{}: {e}", text_file.display())))?;
    let o200k_tokens = u64::try_from(o200k::count(&text)).unwrap_or(u64::MAX);
    let runner = runner(&opts)?;
    let tools = Tools::new(&opts);
    let prompt = opts
        .prompt_file
        .as_deref()
        .map_or(Prompt::Text(ISOLATION_PROMPT), |p: &Path| Prompt::File(p));
    let without_request = tools.request(&opts, prompt);
    let mut with_request = without_request;
    with_request.append_system_file = Some(text_file);
    let without = runner.call(&without_request, None).map_err(run_err)?;
    let with = runner.call(&with_request, None).map_err(run_err)?;
    let claude = text_tokens(&with, &without);
    let claude_tokens = claude.as_ref().ok().copied();
    let record = json!({
        "record": "moirai-tokcount.claude-delta.v1",
        "text_file": text_file.display().to_string(),
        "bytes": text.len(),
        "claude_tokens": claude_tokens,
        "claude_tokens_includes_separator": true,
        "o200k_tokens": o200k_tokens,
        "max_tokens": claude_tokens.map(|c| c.max(o200k_tokens)),
        "without": without.to_json(),
        "with": with.to_json(),
    });
    println!("{record}");
    if let Some(why) = record_failure(&without).or_else(|| record_failure(&with)) {
        return Err(Failure::Run(why));
    }
    claude.map(drop).map_err(run_err)
}

fn isolation_cmd(args: &[OsString]) -> Result<(), Failure> {
    let opts = parse_options(args, &CALL_OPTIONS)?;
    let runner = runner(&opts)?;
    let tools = Tools::new(&opts);
    let prompt = opts
        .prompt_file
        .as_deref()
        .map_or(Prompt::Text(ISOLATION_PROMPT), |p: &Path| Prompt::File(p));
    let request = tools.request(&opts, prompt);
    let check = runner.isolation_check(&request).map_err(run_err)?;
    println!("{}", check.to_json());
    if let Some(why) = record_failure(&check.configured).or_else(|| record_failure(&check.isolated))
    {
        return Err(Failure::Run(why));
    }
    if check.passed() {
        Ok(())
    } else {
        Err(Failure::Run(format!(
            "the call's input differs by {:+} tokens between the owner's home and an empty one: Claude Code loads \
             context from the home directory",
            check.difference
        )))
    }
}

fn version_cmd(args: &[OsString]) -> Result<(), Failure> {
    let opts = parse_options(args, &["--config"])?;
    let version = runner(&opts)?.version().map_err(run_err)?;
    println!("{version}");
    Ok(())
}
