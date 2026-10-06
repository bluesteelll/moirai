//! `cargo xtask`: the repository's tool runner (a host tool, checked by GT20 (e) on every target; docs/m0/PLAN.md
//! §2.2). Subcommands, with the work package that specifies each:
//!
//! | Subcommand | WP | Purpose |
//! |---|---|---|
//! | `gate [--branch m0/<role> \| --role <role>] [--ci] [--range a..b] [--only s,..] [--skip s,..] [--strict-coverage] [--list]` | WP-02 | the pre-merge and CI gate ([`gate`]) |
//! | `host-only --list` | WP-02 | the crates of `xtask/host-only.toml`, one per line |
//! | `authors [--range a..b]` | WP-02 | commit paths against the WP's role ([`authors`]) |
//! | `markers [--range a..b]` | WP-02 | the AI-marker scan of a range ([`markers`]) |
//! | `coverage [--strict]` | WP-02 | `docs/spec/COVERAGE.md` ([`coverage`]); `--strict`: no blank cell (WP-80 pass 2, WP-81b) |
//! | `hook pre-commit --guard <bool> [--private-dir <dir>]` | WP-03 | the delegated pre-commit checks ([`hook`]) |
//! | `hook pr-body (--event <path> \| --file <path>)` | WP-03 | the pull-request title and body |
//! | `private index [--private-dir <dir>] [--public-ref <rev> \| --no-public]` | WP-03 | rebuild `/private/MANIFEST.b3` ([`private`]) |
//! | `worktree <role> [--as <role>] [--base <rev>] [--root <dir>] [--target-root <dir>]` | WP-01 | a role's worktree ([`worktree`]) |
//! | `ci commits [--event <path>]` | WP-04 | the pull-request and push checks over every commit ([`ci`]) |
//! | `ucd [--check]` | WP-61 | generate the `fold_v1` tables from `fixtures/ucd/17.0.0/` ([`ucd`]); `--check` compares |
//! | `hex <file.hex>... [-o <out>] \| --digest <file.hex>... \| --check [<path>...]` | WP-20 | the generic fixture assembler ([`hex`], [`hex::USAGE`]) |
//! | `loadrec start [--max-duration <s>] [--private-dir <dir>] \| stop [--private-dir <dir>] \| check <file>` | WP-51a | measurement 16's load fixture in `/private/load/` ([`loadrec`]) |
//! | `nightly check [--private-dir <dir>] [--windows <file>] [--now <t>] [--guard <exe>] [--inject-…]... \| run [--private-dir <dir>] [--guard <exe>]` | WP-05 | the profile-L nightly runner ([`nightly`], [`nightly::USAGE`]) |

mod authors;
mod cargo;
mod ci;
mod config;
mod coverage;
mod diag;
mod gate;
mod git;
mod hex;
mod hook;
mod lint_deps;
mod lint_fuzz;
mod lint_roots;
mod lint_source;
mod loadrec;
mod markers;
mod metadata;
mod nightly;
mod paths;
mod private;
mod rustscan;
mod semver;
mod spdx;
mod toml;
mod ucd;
mod utc;
mod worktree;

#[cfg(test)]
mod scratch_tests;
#[cfg(test)]
mod testdir;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <subcommand>
  gate [--branch m0/<role> | --role <role>] [--ci] [--range <a>..<b>] [--only <step>,..] [--skip <step>,..] [--strict-coverage] [--list]
  host-only --list
  authors [--range <a>..<b>]
  markers [--range <a>..<b>]
  coverage [--strict]
  hook pre-commit --guard true|false [--private-dir <dir>]
  hook pr-body (--event <path> | --file <path>)
  private index [--private-dir <dir>] [--public-ref <rev> | --no-public]
  worktree <role> [--as <role>] [--base <rev>] [--root <dir>] [--target-root <dir>]
  ci commits [--event <path>]
  ucd [--check]
  hex <file.hex>... [-o <out.bin>] | --digest <file.hex>... | --check [<file or directory>...]
      (cargo xtask hex --help: the .hex format and the --check rules)
  loadrec start [--max-duration <seconds>] [--private-dir <dir>] | stop [--private-dir <dir>] | check <file>
  nightly check [--private-dir <dir>] [--windows <file>] [--now <date-time>] [--guard <exe>] [--inject-...]
        | run [--private-dir <dir>] [--guard <exe>]   (cargo xtask nightly --help)";

/// A minimal option reader: `--name value` pairs and flags.
struct Args {
    rest: Vec<String>,
}

impl Args {
    fn flag(&mut self, name: &str) -> bool {
        match self.rest.iter().position(|a| a == name) {
            Some(i) => {
                self.rest.remove(i);
                true
            }
            None => false,
        }
    }

    fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        let Some(i) = self
            .rest
            .iter()
            .position(|a| a == name || a.starts_with(&format!("{name}=")))
        else {
            return Ok(None);
        };
        let a = self.rest.remove(i);
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            return Ok(Some(v.to_string()));
        }
        if i < self.rest.len() {
            Ok(Some(self.rest.remove(i)))
        } else {
            Err(format!("{name} needs a value"))
        }
    }

    fn list(&mut self, name: &str) -> Result<Vec<String>, String> {
        Ok(self
            .value(name)?
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default())
    }

    fn done(&self) -> Result<(), String> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(format!("unexpected arguments: {}", self.rest.join(" ")))
        }
    }
}

fn repo_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    git::toplevel(&cwd)
}

/// `--private-dir`, or the main worktree's `/private/` (PLAN §2.5), found as the pre-commit hook finds it.
fn private_dir(repo: &Path, given: Option<PathBuf>) -> Result<PathBuf, String> {
    match given {
        Some(p) => Ok(p),
        None => Ok(git::main_worktree(repo)
            .ok_or("cannot locate the main worktree")?
            .join("private")),
    }
}

fn print_diags(diags: &[diag::Diag]) {
    for d in diags {
        println!("{d}");
    }
}

fn run(argv: Vec<String>) -> Result<ExitCode, String> {
    let Some((cmd, rest)) = argv.split_first() else {
        eprintln!("{USAGE}");
        return Ok(ExitCode::from(2));
    };
    let mut a = Args {
        rest: rest.to_vec(),
    };
    match cmd.as_str() {
        "gate" => {
            if a.flag("--list") {
                a.done()?;
                for (n, w) in gate::STEPS {
                    println!("{n:<12} {w}");
                }
                return Ok(ExitCode::SUCCESS);
            }
            let o = gate::Opts {
                branch: a.value("--branch")?,
                role: a.value("--role")?,
                ci: a.flag("--ci"),
                range: a.value("--range")?,
                only: a.list("--only")?,
                skip: a.list("--skip")?,
                strict_coverage: a.flag("--strict-coverage"),
            };
            a.done()?;
            for s in o.only.iter().chain(&o.skip) {
                if !gate::STEPS.iter().any(|(n, _)| n == s) {
                    return Err(format!("unknown step '{s}' (see cargo xtask gate --list)"));
                }
            }
            let ok = gate::run(&repo_root()?, &o)?;
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "host-only" => {
            if !a.flag("--list") {
                return Err("usage: cargo xtask host-only --list".into());
            }
            a.done()?;
            let repo = repo_root()?;
            let t = config::read_toml(&repo.join("xtask/host-only.toml"))?;
            for c in config::HostOnly::from_table(&t)?.crates {
                println!("{c}");
            }
            Ok(ExitCode::SUCCESS)
        }
        "authors" | "markers" => {
            let range = a.value("--range")?.unwrap_or_else(|| "master..HEAD".into());
            a.done()?;
            let repo = repo_root()?;
            let step = if cmd == "authors" {
                "authors"
            } else {
                "markers"
            };
            let o = gate::Opts {
                branch: None,
                role: None,
                ci: false,
                range: Some(range),
                only: vec![step.to_string()],
                skip: Vec::new(),
                strict_coverage: false,
            };
            let ok = gate::run(&repo, &o)?;
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "coverage" => {
            let strict = a.flag("--strict");
            a.done()?;
            let r = coverage::run(&repo_root()?, strict)?;
            print_diags(&r.diags);
            match &r.notice {
                Some(n) => println!("coverage: {n}"),
                None => println!(
                    "coverage: {} rows, {} findings; {} blank fixture cells and {} blank model cells {}",
                    r.rows,
                    r.diags.len(),
                    r.blank_fixture,
                    r.blank_model,
                    if strict {
                        "(--strict: findings)"
                    } else {
                        "tolerated"
                    }
                ),
            }
            Ok(if r.diags.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "hook" => {
            let Some(which) = a.rest.first().cloned() else {
                return Err("usage: cargo xtask hook pre-commit | pr-body".into());
            };
            a.rest.remove(0);
            match which.as_str() {
                "pre-commit" => {
                    let guard = match a.value("--guard")?.as_deref() {
                        Some("true") => true,
                        Some("false") | None => false,
                        Some(o) => return Err(format!("--guard takes true or false, not '{o}'")),
                    };
                    let pd = a.value("--private-dir")?.map(PathBuf::from);
                    a.done()?;
                    let repo = repo_root()?;
                    match hook::pre_commit(&repo, guard, pd.as_deref()) {
                        Ok(d) if d.is_empty() => Ok(ExitCode::SUCCESS),
                        Ok(d) => {
                            for x in &d {
                                eprintln!("pre-commit: refused: {x}");
                            }
                            Ok(ExitCode::FAILURE)
                        }
                        Err(e) => {
                            eprintln!("pre-commit: refused: {e}");
                            Ok(ExitCode::FAILURE)
                        }
                    }
                }
                "pr-body" => {
                    let ev = a.value("--event")?;
                    let file = a.value("--file")?;
                    a.done()?;
                    let text =
                        match (ev, file) {
                            (Some(e), None) => hook::pr_text_from_event(Path::new(&e))?,
                            (None, Some(f)) => {
                                std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?
                            }
                            _ => return Err(
                                "hook pr-body takes exactly one of --event <path> or --file <path>"
                                    .into(),
                            ),
                        };
                    let r = hook::pr_body(&text);
                    for x in &r {
                        println!("pr-body: refused ({x})");
                    }
                    if r.is_empty() {
                        println!("pr-body: no AI markers in the title or body");
                        Ok(ExitCode::SUCCESS)
                    } else {
                        println!(
                            "pr-body: pull requests carry no AI co-author or attribution (AGENTS.md, \"Git\"); edit the title or body"
                        );
                        Ok(ExitCode::FAILURE)
                    }
                }
                other => Err(format!("unknown hook '{other}'")),
            }
        }
        "private" => {
            if a.rest.first().map(String::as_str) != Some("index") {
                return Err("usage: cargo xtask private index [--private-dir <dir>] [--public-ref <rev> | --no-public]".into());
            }
            a.rest.remove(0);
            let pd = a.value("--private-dir")?.map(PathBuf::from);
            let public_ref = a.value("--public-ref")?;
            let no_public = a.flag("--no-public");
            a.done()?;
            let repo = repo_root()?;
            let pd = private_dir(&repo, pd)?;
            let public_ref = (!no_public).then(|| public_ref.unwrap_or_else(|| "master".into()));
            let (st, note) = private::rebuild(&repo, &pd, public_ref.as_deref())?;
            if let Some(n) = note {
                println!("private index: {n}");
            }
            println!(
                "private index: {} written: {} files ({} text), {} shingles ({} public ones left out; {} sorted runs spilled)",
                pd.join(private::MANIFEST).display(),
                st.files,
                st.text_files,
                st.shingles,
                st.public_removed,
                st.runs
            );
            Ok(ExitCode::SUCCESS)
        }
        "worktree" => {
            let Some(role) = a.rest.first().cloned().filter(|r| !r.starts_with('-')) else {
                return Err("usage: cargo xtask worktree <role> [--as <role>] [--base <rev>] [--root <dir>] [--target-root <dir>]".into());
            };
            a.rest.remove(0);
            let o = worktree::Opts {
                role,
                as_role: a.value("--as")?,
                base: a.value("--base")?,
                root: a.value("--root")?.map(PathBuf::from),
                target_root: a.value("--target-root")?.map(PathBuf::from),
            };
            a.done()?;
            worktree::run(&repo_root()?, &o)?;
            Ok(ExitCode::SUCCESS)
        }
        "ci" => {
            if a.rest.first().map(String::as_str) != Some("commits") {
                return Err("usage: cargo xtask ci commits [--event <path>]".into());
            }
            a.rest.remove(0);
            let ev = match a.value("--event")? {
                Some(e) => e,
                None => std::env::var("GITHUB_EVENT_PATH")
                    .map_err(|_| "no --event and no GITHUB_EVENT_PATH")?,
            };
            a.done()?;
            let d = ci::commits(&repo_root()?, Path::new(&ev), None)?;
            print_diags(&d);
            println!("ci commits: {}", if d.is_empty() { "PASS" } else { "FAIL" });
            Ok(if d.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "ucd" => {
            let check = a.flag("--check");
            a.done()?;
            let ok = ucd::run(&repo_root()?, check)?;
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "hex" => hex_cmd(a),
        "loadrec" => loadrec_cmd(a),
        "nightly" => nightly::cli(&a.rest, &repo_root()?),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        other => {
            eprintln!("xtask: unknown subcommand '{other}'\n{USAGE}");
            Ok(ExitCode::from(2))
        }
    }
}

/// `cargo xtask loadrec` ([`loadrec`], [`loadrec::USAGE`]).
fn loadrec_cmd(mut a: Args) -> Result<ExitCode, String> {
    let Some(which) = a.rest.first().cloned() else {
        return Err(loadrec::USAGE.into());
    };
    a.rest.remove(0);
    match which.as_str() {
        "start" => {
            let max = match a.value("--max-duration")? {
                Some(v) => match v.parse::<u64>() {
                    Ok(s) if s > 0 => std::time::Duration::from_secs(s),
                    _ => {
                        return Err(format!(
                            "--max-duration takes a positive number of seconds, not '{v}'"
                        ));
                    }
                },
                None => loadrec::DEFAULT_MAX_DURATION,
            };
            let pd = a.value("--private-dir")?.map(PathBuf::from);
            a.done()?;
            let repo = repo_root()?;
            loadrec::start(&repo, &private_dir(&repo, pd)?, max)?;
            Ok(ExitCode::SUCCESS)
        }
        "stop" => {
            let pd = a.value("--private-dir")?.map(PathBuf::from);
            a.done()?;
            let repo = repo_root()?;
            loadrec::stop(&repo, &private_dir(&repo, pd)?)?;
            Ok(ExitCode::SUCCESS)
        }
        "check" => {
            let [file] = a.rest.as_slice() else {
                return Err(loadrec::USAGE.into());
            };
            Ok(if loadrec::check(Path::new(file))? {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "--help" | "help" => {
            println!("{}", loadrec::USAGE);
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!(
            "unknown loadrec command '{other}'
{}",
            loadrec::USAGE
        )),
    }
}

/// `cargo xtask hex` ([`hex::cli`], [`hex::USAGE`]).
fn hex_cmd(a: Args) -> Result<ExitCode, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let ok = hex::cli(&a.rest, &cwd, &repo_root, &mut std::io::stdout().lock())?;
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
