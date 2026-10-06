//! `cargo xtask worktree <role>`: a role's authoring worktree (docs/m0/PLAN.md §3.1 "Mechanics", WP-01;
//! docs/m0/authors.md §5 items 1, 2 and 5).
//!
//! 1. A full checkout on branch `m0/<role>` (created from `--base`, default `HEAD`, or reused when it exists) at
//!    `<worktree root>/<role>`; the root is `git config moirai.worktree-root`, by default `<drive>:/moirai-wt` beside
//!    a Windows main worktree, or `<parent of the main worktree>/moirai-wt` elsewhere.
//! 2. `.claude/settings.local.json` in it, with
//!    - `permissions.deny`: a `Read(...)` rule for each of the role's `deny_read` patterns (`xtask/roles.toml`) in
//!      the new worktree, the main checkout and every other worktree, as absolute paths in Claude Code's form
//!      (`//d/path/**`). Claude Code applies Read rules to its Grep and Glob tools too;
//!    - `env.CARGO_TARGET_DIR`: the lane's shared target directory, `<target root>/<lane dir>`, the target root being
//!      `git config moirai.target-root` (default `<drive>:/moirai-target`, or `<parent of the main worktree>/
//!      moirai-target`), and `env.CARGO_BUILD_JOBS` from `git config moirai.build-jobs` (default 6, until
//!      measurement 21 sets the lane cap).
//! 3. `git config moirai.xtask`: when unset or not an executable file, a copy of this xtask binary is installed at
//!    `<git common dir>/moirai-hooks/xtask[.exe]` and the key points at it, so `.githooks/pre-commit` can delegate.
//!    This comes before step 4: with `moirai.private-guard` true and a manifest written, the hook fails closed while
//!    the key is unset. The step is idempotent and stays done if the worktree is not handed over.
//! 4. A seeded AI-trailer commit and a seeded private-file commit are attempted in the new worktree. The worktree is
//!    handed over only if the hooks refuse both; otherwise the worktree (and a branch this run created) is removed.
//! 5. "Every other worktree" holds over time: the role whose settings a worktree carries is recorded in its
//!    gitignored `.claude/moirai-role`, and each new worktree's path is added to the `permissions.deny` rules of
//!    every earlier role worktree (merged into its `settings.local.json`, whose other keys and rules, such as the
//!    approvals Claude Code records there, are kept).
//!
//! Tested end to end by `.githooks/test-hooks.sh` with a real xtask (the gate's `hooks` step): a clone with the
//! guard on, a manifest written and no `moirai.xtask` gets its worktree, and `moirai.xtask` is set.

use crate::config::Config;
use crate::git;
use crate::paths::Pattern;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The gitignored file (`/.claude/*`, PLAN §2.5) that names the role whose settings a worktree carries.
pub const ROLE_FILE: &str = ".claude/moirai-role";
const SETTINGS_FILE: &str = ".claude/settings.local.json";

pub struct Opts {
    pub role: String,
    /// Use this known role's settings for a throwaway worktree name (self-tests).
    pub as_role: Option<String>,
    pub base: Option<String>,
    pub root: Option<PathBuf>,
    pub target_root: Option<PathBuf>,
}

/// `D:/x/y` → `//d/x/y` (Claude Code's absolute-path form; Windows paths in POSIX form).
pub fn claude_abs(p: &str) -> String {
    let p = p.replace('\\', "/");
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        format!("//{}{}", (b[0] as char).to_ascii_lowercase(), &p[2..])
    } else {
        format!("/{p}")
    }
}

/// `<drive>:/<leaf>` beside a Windows main worktree, else `<parent of the main worktree>/<leaf>`: the default worktree
/// root and target root (`nightly` resolves the target root the same way).
pub(crate) fn default_root(main: &Path, leaf: &str) -> PathBuf {
    let s = main.to_string_lossy().replace('\\', "/");
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b':' {
        PathBuf::from(format!("{}:/{leaf}", b[0] as char))
    } else {
        main.parent()
            .map(|p| p.join(leaf))
            .unwrap_or_else(|| PathBuf::from(leaf))
    }
}

fn fwd(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The `Read(...)` deny rules for a role's `deny_read` patterns in each of `worktrees`. Claude Code applies Read
/// rules to its Grep and Glob tools as well.
pub fn deny_rules(deny_read: &[String], worktrees: &[String]) -> Vec<String> {
    let mut rules: Vec<String> = Vec::new();
    for wt in worktrees {
        let base = claude_abs(wt.trim_end_matches('/'));
        for d in deny_read {
            for g in Pattern::new(d).globs() {
                let r = format!("Read({base}/{g})");
                if !rules.contains(&r) {
                    rules.push(r);
                }
            }
        }
    }
    rules
}

/// The settings file for a role.
pub fn settings_json(
    deny_read: &[String],
    worktrees: &[String],
    target_dir: &str,
    jobs: &str,
) -> String {
    let v = serde_json::json!({
        "permissions": { "deny": deny_rules(deny_read, worktrees) },
        "env": { "CARGO_TARGET_DIR": target_dir, "CARGO_BUILD_JOBS": jobs },
    });
    let mut s = serde_json::to_string_pretty(&v).unwrap_or_default();
    s.push('\n');
    s
}

/// Adds `rules` to the `permissions.deny` array of an existing settings file, keeping every other key and every
/// rule already there. `Ok(None)` when every rule is already present.
pub fn merge_deny(existing: &str, rules: &[String]) -> Result<Option<String>, String> {
    let mut v: serde_json::Value =
        serde_json::from_str(existing).map_err(|e| format!("not JSON: {e}"))?;
    let obj = v.as_object_mut().ok_or("not a JSON object")?;
    let perms = obj
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("permissions is not an object")?;
    let deny = perms
        .entry("deny")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or("permissions.deny is not an array")?;
    let mut changed = false;
    for r in rules {
        if !deny.iter().any(|x| x.as_str() == Some(r.as_str())) {
            deny.push(serde_json::Value::String(r.clone()));
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut s = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    s.push('\n');
    Ok(Some(s))
}

fn worktree_paths(repo: &Path) -> Result<Vec<String>, String> {
    let out = git::run(repo, &["worktree", "list", "--porcelain"])?;
    Ok(out
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(|p| p.replace('\\', "/"))
        .collect())
}

/// Adds the deny rules for `new_wt` to every other role worktree (step 5). Returns one line per worktree touched;
/// a worktree without `.claude/moirai-role` (not made by this command) is left alone.
fn refresh_others(
    cfg: &Config,
    worktrees: &[String],
    main: &str,
    new_wt: &str,
) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for wt in worktrees {
        if wt.eq_ignore_ascii_case(new_wt) || wt.eq_ignore_ascii_case(main) {
            continue;
        }
        let dir = Path::new(wt);
        let Ok(role_name) = std::fs::read_to_string(dir.join(ROLE_FILE)) else {
            continue;
        };
        let role_name = role_name.trim();
        let Some(role) = cfg.roles.role(role_name) else {
            notes.push(format!(
                "{wt}: {ROLE_FILE} names '{role_name}', which xtask/roles.toml does not list: not updated"
            ));
            continue;
        };
        let rules = deny_rules(&role.deny_read, &[new_wt.to_string()]);
        if rules.is_empty() {
            continue;
        }
        let path = dir.join(SETTINGS_FILE);
        let existing =
            std::fs::read_to_string(&path).map_err(|e| format!("{wt}/{SETTINGS_FILE}: {e}"))?;
        if let Some(s) =
            merge_deny(&existing, &rules).map_err(|e| format!("{wt}/{SETTINGS_FILE}: {e}"))?
        {
            std::fs::write(&path, s).map_err(|e| format!("{wt}/{SETTINGS_FILE}: {e}"))?;
            notes.push(format!(
                "{wt} ({role_name}): {} deny rules added for {new_wt}",
                rules.len()
            ));
        }
    }
    Ok(notes)
}

/// Runs a git commit attempt in `wt`; returns (succeeded, combined output).
fn try_commit(wt: &Path, args: &[&str]) -> Result<(bool, String), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(wt)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git commit: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), text))
}

/// Step 3: `git config moirai.xtask`. A key that names another executable (the gate worktree's prebuilt binary, set
/// by the owner) is left as it is; otherwise this binary is copied to `<git common dir>/moirai-hooks/` (replacing an
/// earlier copy, so the hook never runs a stale build of this command) and the key points at the copy.
fn install_xtask(repo: &Path) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let dir = git::common_dir(repo)?.join("moirai-hooks");
    let name = exe
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "xtask".into());
    let dst = dir.join(name);
    let dst_s = fwd(&dst);
    if let Some(x) = git::probe(repo, &["config", "--get", "moirai.xtask"])
        && Path::new(&x).is_file()
        && !x.replace('\\', "/").eq_ignore_ascii_case(&dst_s)
    {
        return Ok(x);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::copy(&exe, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    git::run(repo, &["config", "moirai.xtask", &dst_s])?;
    Ok(dst_s)
}

/// Removes a worktree this run made, and its branch when this run created it.
fn cleanup(repo: &Path, wt: &Path, branch: &str, created_branch: bool) {
    let _ = git::run(repo, &["worktree", "remove", "--force", &fwd(wt)]);
    if created_branch {
        let _ = git::run(repo, &["branch", "-D", branch]);
    }
}

/// The seeded commits; `Ok(())` when both are refused by the hooks.
fn seeded_checks(wt: &Path) -> Result<(), String> {
    let head = git::run(wt, &["rev-parse", "HEAD"])?.trim().to_string();
    // 1. An AI trailer.
    let (ok, out) = try_commit(
        wt,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "WP-00: seeded AI-trailer commit (xtask worktree self-test)",
            "-m",
            "Co-authored-by: Claude <noreply@anthropic.com>",
        ],
    )?;
    let after = git::run(wt, &["rev-parse", "HEAD"])?.trim().to_string();
    if ok || after != head {
        let _ = git::run(wt, &["reset", "-q", "--soft", &head]);
        return Err("the seeded AI-trailer commit was NOT refused: the commit-msg hook is not active in this worktree (git config core.hooksPath)".into());
    }
    if !out.contains("commit-msg: refused") {
        return Err(format!(
            "the seeded AI-trailer commit failed, but not through the commit-msg hook:\n{out}"
        ));
    }
    // 2. A private file.
    let seed_rel = "private/xtask-worktree-seed.txt";
    let seed = wt.join(seed_rel);
    std::fs::create_dir_all(seed.parent().unwrap_or(wt)).map_err(|e| e.to_string())?;
    std::fs::write(&seed, "synthetic seed for the private-file refusal check\n")
        .map_err(|e| e.to_string())?;
    let res = (|| -> Result<(bool, String), String> {
        git::run(wt, &["add", "-f", "--", seed_rel])?;
        try_commit(
            wt,
            &[
                "commit",
                "-m",
                "WP-00: seeded private-file commit (xtask worktree self-test)",
            ],
        )
    })();
    let after = git::run(wt, &["rev-parse", "HEAD"])
        .unwrap_or_default()
        .trim()
        .to_string();
    if after != head {
        let _ = git::run(wt, &["reset", "-q", "--soft", &head]);
    }
    let _ = git::run(
        wt,
        &["rm", "-q", "--cached", "--ignore-unmatch", "--", seed_rel],
    );
    let _ = std::fs::remove_file(&seed);
    let _ = std::fs::remove_dir(wt.join("private"));
    let (ok, out) = res?;
    if ok || after != head {
        return Err("the seeded private-file commit was NOT refused: the pre-commit hook is not active in this worktree".into());
    }
    if !out.contains("pre-commit: refused") {
        return Err(format!(
            "the seeded private-file commit failed, but not through the pre-commit hook:\n{out}"
        ));
    }
    Ok(())
}

pub fn run(repo: &Path, o: &Opts) -> Result<(), String> {
    let cfg = Config::load(repo)?;
    let settings_role = o.as_role.as_deref().unwrap_or(&o.role);
    let role = cfg.roles.role(settings_role).ok_or_else(|| {
        format!(
            "unknown role '{settings_role}'; xtask/roles.toml lists: {}",
            cfg.roles
                .roles
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    if o.role.is_empty()
        || !o
            .role
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(format!(
            "worktree name '{}' must be lower-case letters, digits and '-'",
            o.role
        ));
    }
    let main =
        git::main_worktree(repo).ok_or("cannot locate the main worktree (git common dir)")?;
    let wt_root = o
        .root
        .clone()
        .or_else(|| {
            git::probe(repo, &["config", "--get", "moirai.worktree-root"]).map(PathBuf::from)
        })
        .unwrap_or_else(|| default_root(&main, "moirai-wt"));
    let target_root = o
        .target_root
        .clone()
        .or_else(|| git::probe(repo, &["config", "--get", "moirai.target-root"]).map(PathBuf::from))
        .unwrap_or_else(|| default_root(&main, "moirai-target"));
    let lane_dir = cfg.roles.lane_dir(&role.lane).ok_or("unknown lane")?;
    let target_dir = fwd(&target_root.join(lane_dir));
    let jobs =
        git::probe(repo, &["config", "--get", "moirai.build-jobs"]).unwrap_or_else(|| "6".into());
    let wt = wt_root.join(&o.role);
    if wt.exists() {
        return Err(format!("{} already exists", wt.display()));
    }
    std::fs::create_dir_all(&wt_root).map_err(|e| format!("{}: {e}", wt_root.display()))?;
    let branch = format!("m0/{}", o.role);
    let exists = git::probe(
        repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_some();
    let wt_s = fwd(&wt);
    if exists {
        git::run(repo, &["worktree", "add", &wt_s, &branch])?;
    } else {
        let base = o.base.clone().unwrap_or_else(|| "HEAD".into());
        git::run(repo, &["worktree", "add", "-b", &branch, &wt_s, &base])?;
    }
    let created = !exists;
    let mut wts = Vec::new();
    let result = (|| -> Result<String, String> {
        wts = worktree_paths(repo)?;
        if !wts.iter().any(|w| w.eq_ignore_ascii_case(&wt_s)) {
            wts.push(wt_s.clone());
        }
        let json = settings_json(&role.deny_read, &wts, &target_dir, &jobs);
        let dir = wt.join(".claude");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(wt.join(SETTINGS_FILE), json)
            .map_err(|e| format!("{SETTINGS_FILE}: {e}"))?;
        std::fs::write(wt.join(ROLE_FILE), format!("{}\n", role.name))
            .map_err(|e| format!("{ROLE_FILE}: {e}"))?;
        // Before the seeded commits: with the guard on and a manifest written, the pre-commit hook fails closed
        // while `moirai.xtask` is unset, so the seeded AI-trailer commit would never reach commit-msg.
        let xtask = install_xtask(repo)?;
        seeded_checks(&wt)?;
        Ok(xtask)
    })();
    match result {
        Ok(xtask) => {
            // Handed over; the earlier role worktrees now deny the same paths in this one.
            let refreshed = refresh_others(&cfg, &wts, &fwd(&main), &wt_s)?;
            println!("worktree ready: {wt_s}");
            println!(
                "  branch:            {branch}{}",
                if created { " (new)" } else { " (existing)" }
            );
            println!(
                "  role settings:     {} ({}), lane {}",
                role.name, role.title, role.lane
            );
            println!(
                "  deny_read:         {}",
                if role.deny_read.is_empty() {
                    "(none)".to_string()
                } else {
                    role.deny_read.join(", ")
                }
            );
            println!("  CARGO_TARGET_DIR:  {target_dir}");
            println!("  CARGO_BUILD_JOBS:  {jobs}");
            println!("  moirai.xtask:      {xtask}");
            println!("  seeded AI-trailer and private-file commits: refused");
            for n in &refreshed {
                println!("  updated:           {n}");
            }
            Ok(())
        }
        Err(e) => {
            cleanup(repo, &wt, &branch, created);
            Err(format!("worktree not handed over (removed): {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_paths_and_settings() {
        assert_eq!(claude_abs("D:/moirai-wt/r-fix"), "//d/moirai-wt/r-fix");
        assert_eq!(claude_abs(r"C:\x\y"), "//c/x/y");
        assert_eq!(claude_abs("/home/u/m"), "//home/u/m");
        let s = settings_json(
            &[
                "crates/moirai-model/**".into(),
                "crates/moirai-toylog/src/{bug,bugs}".into(),
            ],
            &["D:/claude/moirai".into(), "D:/moirai-wt/r-harn-s".into()],
            "D:/moirai-target/laneA",
            "6",
        );
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let deny: Vec<&str> = v["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(deny.len(), 2 * 5);
        assert!(deny.contains(&"Read(//d/claude/moirai/crates/moirai-model/**)"));
        assert!(deny.contains(&"Read(//d/moirai-wt/r-harn-s/crates/moirai-toylog/src/bugs.rs)"));
        assert!(deny.contains(&"Read(//d/moirai-wt/r-harn-s/crates/moirai-toylog/src/bug/**)"));
        assert_eq!(v["env"]["CARGO_TARGET_DIR"], "D:/moirai-target/laneA");
        assert_eq!(
            default_root(Path::new("D:/claude/moirai"), "moirai-wt"),
            PathBuf::from("D:/moirai-wt")
        );
    }

    #[test]
    fn merging_keeps_other_keys_and_rules() {
        let existing = r#"{
  "env": { "CARGO_TARGET_DIR": "D:/moirai-target/laneA" },
  "permissions": {
    "allow": ["Bash(cargo test:*)"],
    "deny": ["Read(//d/claude/moirai/crates/moirai-model/**)", "Bash(curl:*)"]
  }
}"#;
        let rules = deny_rules(
            &["crates/moirai-model/**".into()],
            &["D:/claude/moirai".into(), "D:/moirai-wt/r-fix".into()],
        );
        assert_eq!(rules.len(), 2);
        let merged = merge_deny(existing, &rules).unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
        let deny = v["permissions"]["deny"].as_array().unwrap();
        assert_eq!(deny.len(), 3, "{merged}");
        assert!(deny.iter().any(|x| x == "Bash(curl:*)"));
        assert!(
            deny.iter()
                .any(|x| x == "Read(//d/moirai-wt/r-fix/crates/moirai-model/**)")
        );
        assert_eq!(v["permissions"]["allow"][0], "Bash(cargo test:*)");
        assert_eq!(v["env"]["CARGO_TARGET_DIR"], "D:/moirai-target/laneA");
        // Idempotent; a missing permissions table is created; a malformed file is an error.
        assert!(merge_deny(&merged, &rules).unwrap().is_none());
        let fresh = merge_deny("{}", &rules).unwrap().unwrap();
        assert!(fresh.contains("r-fix/crates/moirai-model"));
        assert!(merge_deny("[1]", &rules).is_err());
        assert!(merge_deny(r#"{"permissions":{"deny":"x"}}"#, &rules).is_err());
    }
}
