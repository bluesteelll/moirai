//! `xtask hook pre-commit` and `xtask hook pr-body` (docs/m0/PLAN.md §3.2 item 7, WP-03; `.githooks/pre-commit`).
//!
//! `hook pre-commit --guard true|false [--private-dir <main worktree>/private]` is called by `.githooks/pre-commit`
//! from the worktree's top level with git's hook environment. It runs the delegated checks 3-6 of that hook over
//! the staged changes: a stale manifest, a staged file whose BLAKE3 (or LF-normalised BLAKE3) the manifest lists, a
//! staged added line that contains a listed 8-word shingle, and the report scrub of `docs/measurements/**` and report
//! files. The patch is read with fixed `a/`/`b/` prefixes and no textconv ([`private::PATCH_FLAGS`]), so the user's
//! `diff.*` settings cannot hide a path, and an added line whose path is not recognised refuses the commit. Without
//! a manifest it fails closed when the guard is true, and otherwise runs the report scrub alone. A non-zero exit
//! refuses the commit.
//!
//! `hook pr-body --event <$GITHUB_EVENT_PATH> | --file <path>` checks a pull request's title and body with the
//! `commit-msg` rules (WP-04: the body is read from the event file, never interpolated into a workflow step).

use crate::diag::Diag;
use crate::git;
use crate::markers::{self, Refusal};
use crate::private;
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn pre_commit(
    repo: &Path,
    guard: bool,
    private_dir: Option<&Path>,
) -> Result<Vec<Diag>, String> {
    let loaded = match private_dir {
        Some(pd) => private::load_current(pd)?,
        None => None,
    };
    if loaded.is_none() && guard {
        return Err(match private_dir {
            Some(pd) => format!("moirai.private-guard is true and {} is missing: failing closed", pd.join(private::MANIFEST).display()),
            None => "moirai.private-guard is true but the main worktree's /private/ is not known: failing closed".into(),
        });
    }
    let base = git::head_or_empty(repo)?;
    let raw = git::run_bytes(
        repo,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--cached",
            "--raw",
            "-z",
            "--no-relative",
            "--no-renames",
            "--no-abbrev",
            "--diff-filter=ACMT",
            &base,
            "--",
        ],
    )?;
    let changed = private::parse_raw_z(&raw);
    let names = private::machine_names();
    let pd = private_dir.map(Path::to_path_buf).unwrap_or_default();
    let g = private::Guards {
        manifest: loaded.as_ref().map(|(m, p)| (pd.as_path(), m, p.clone())),
        names: &names,
    };
    let hashes = if g.manifest.is_some() {
        private::blob_hashes(repo, &changed)?
    } else {
        Vec::new()
    };
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotePath=false", "diff", "--cached"])
        .args(private::PATCH_FLAGS)
        .args(["--diff-filter=ACMT", &base, "--"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git diff: {e}"))?;
    let out = child.stdout.take().ok_or("git diff: no stdout")?;
    let mut hits = private::Hits::default();
    let res = private::check_changes(
        "staged",
        &changed,
        &hashes,
        BufReader::with_capacity(64 * 1024, out),
        &g,
        &mut hits,
    );
    let status = child.wait().map_err(|e| e.to_string())?;
    let mut diags = res?;
    if !status.success() {
        return Err(format!("git diff --cached failed ({status})"));
    }
    if let Some((_, _, mpath)) = &g.manifest {
        hits.resolve(mpath, &mut diags)?;
    }
    Ok(diags)
}

/// A pull request's title and body as one message: the title is its subject.
pub fn pr_text_from_event(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let pr = v
        .get("pull_request")
        .ok_or("the event has no pull_request")?;
    let title = pr.get("title").and_then(|t| t.as_str()).unwrap_or("");
    let body = pr.get("body").and_then(|t| t.as_str()).unwrap_or("");
    Ok(format!("{title}\n\n{body}\n"))
}

pub fn pr_body(text: &str) -> Vec<Refusal> {
    markers::check_message(text, "", "#")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_bodies() {
        let d = std::env::temp_dir().join(format!("moirai-xtask-pr-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("event.json");
        let ev = serde_json::json!({"pull_request": {"title": "WP-02: the gate", "body": "Summary of the change.\n\n\u{1F916} Generated with [Claude Code](https://claude.com/claude-code)\n"}});
        std::fs::write(&p, ev.to_string()).unwrap();
        let r = pr_body(&pr_text_from_event(&p).unwrap());
        assert!(
            r.iter().any(|x| x.rule == 5) && r.iter().any(|x| x.rule == 2),
            "{r:?}"
        );
        let ok = serde_json::json!({"pull_request": {"title": "WP-02: the gate", "body": null}});
        std::fs::write(&p, ok.to_string()).unwrap();
        assert!(pr_body(&pr_text_from_event(&p).unwrap()).is_empty());
        let edited = serde_json::json!({"pull_request": {"title": "WP-02: the gate", "body": "Done.\n\nCo-authored-by: Claude <noreply@anthropic.com>"}});
        std::fs::write(&p, edited.to_string()).unwrap();
        assert!(!pr_body(&pr_text_from_event(&p).unwrap()).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
