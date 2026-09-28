//! Running `git` for the lints, the hooks and `xtask worktree` (the git CLI is an external program, PLAN §2.4).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Runs `git -C <repo> <args>` and returns its standard output; a non-zero exit is an error carrying stderr.
pub fn run_bytes(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed ({}): {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

pub fn run(repo: &Path, args: &[&str]) -> Result<String, String> {
    run_bytes(repo, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Like `run`, but a failure is `None` (for probes such as `rev-parse --verify`).
pub fn probe(repo: &Path, args: &[&str]) -> Option<String> {
    run(repo, args).ok().map(|s| s.trim().to_string())
}

pub fn toplevel(dir: &Path) -> Result<PathBuf, String> {
    run(dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))
}

/// The main worktree, found as the hook finds it: the parent of the absolute git common dir, when that is `.git`.
pub fn main_worktree(repo: &Path) -> Option<PathBuf> {
    let common = probe(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let p = PathBuf::from(common);
    if p.file_name().is_some_and(|n| n == ".git") {
        p.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

pub fn common_dir(repo: &Path) -> Result<PathBuf, String> {
    run(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map(|s| PathBuf::from(s.trim()))
}

/// The empty tree's id in this repository's object format.
pub fn empty_tree(repo: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "-t", "tree", "--stdin"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("git hash-object -t tree failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `HEAD`, or the empty tree when the repository has no commit yet.
pub fn head_or_empty(repo: &Path) -> Result<String, String> {
    match probe(repo, &["rev-parse", "--verify", "-q", "HEAD"]) {
        Some(h) if !h.is_empty() => Ok(h),
        _ => empty_tree(repo),
    }
}

/// One commit of a range.
#[derive(Clone, Debug)]
pub struct Rev {
    pub sha: String,
    /// More than one parent: its own changes are its combined diff (`diff-tree --cc`), the lines and files that
    /// differ from every parent (an evil merge's additions, a conflict resolution).
    pub merge: bool,
}

/// The commits of a range, oldest first, merges included.
pub fn rev_list(repo: &Path, range: &str) -> Result<Vec<Rev>, String> {
    Ok(run(repo, &["rev-list", "--reverse", "--parents", range])?
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let sha = it.next()?.to_string();
            Some(Rev {
                sha,
                merge: it.count() > 1,
            })
        })
        .collect())
}

/// The `--raw -z` change list of one commit: against its parent (every path of a root commit), or, for a merge,
/// the combined list of the files that differ from every parent.
pub fn commit_raw(repo: &Path, rev: &Rev) -> Result<Vec<u8>, String> {
    let mut args = vec!["diff-tree", "-r", "--root", "--no-commit-id", "--raw", "-z"];
    if rev.merge {
        args.push("-c");
    }
    args.extend(["--no-renames", "--no-abbrev", rev.sha.as_str()]);
    run_bytes(repo, &args)
}

/// The arguments of one commit's patch for the private checks: `diff-tree -p` against its parent, or the combined
/// `--cc` patch of a merge, with the fixed prefixes and options of [`crate::private::PATCH_FLAGS`].
pub fn commit_patch_args(rev: &Rev) -> Vec<String> {
    let mut a: Vec<String> = [
        "-c",
        "core.quotePath=false",
        "diff-tree",
        "--root",
        "--no-commit-id",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(if rev.merge { "--cc" } else { "-p" }.into());
    a.extend(crate::private::PATCH_FLAGS.iter().map(|s| s.to_string()));
    a.push(rev.sha.clone());
    a
}

/// One commit's message, identities and git's own trailer parse.
pub struct CommitText {
    pub sha: String,
    pub author: String,
    pub committer: String,
    pub subject: String,
    pub message: String,
    pub trailers: String,
}

/// Every commit of a range (merges included: their messages are checked too), oldest first.
pub fn commit_texts(repo: &Path, range: &str) -> Result<Vec<CommitText>, String> {
    let fmt = "--format=%H%x1f%an <%ae>%x1f%cn <%ce>%x1f%s%x1f%(trailers:only,unfold)%x1f%B%x1e";
    let out = run(
        repo,
        &[
            "-c",
            "i18n.logOutputEncoding=UTF-8",
            "log",
            "--reverse",
            fmt,
            range,
        ],
    )?;
    let mut v = Vec::new();
    for rec in out.split('\u{1e}') {
        let rec = rec.trim_start_matches('\n');
        if rec.is_empty() {
            continue;
        }
        let f: Vec<&str> = rec.splitn(6, '\u{1f}').collect();
        if f.len() != 6 {
            return Err(format!("git log: unexpected record in {range}"));
        }
        v.push(CommitText {
            sha: f[0].to_string(),
            author: f[1].to_string(),
            committer: f[2].to_string(),
            subject: f[3].to_string(),
            trailers: f[4].trim_end().to_string(),
            message: f[5].to_string(),
        });
    }
    Ok(v)
}

/// `(status, path)` of one commit's changes, renames split into delete and add. For a merge, the files that differ
/// from every parent, with `A` when every parent lacks the file, `D` when the merge removes it from every parent
/// and `M` otherwise.
pub fn commit_changes(repo: &Path, rev: &Rev) -> Result<Vec<(char, String)>, String> {
    let raw = commit_raw(repo, rev)?;
    Ok(crate::private::parse_raw_z(&raw)
        .into_iter()
        .map(|c| {
            let mut letters = c.status.chars();
            let first = letters.next().unwrap_or('M');
            let status = if letters.all(|l| l == first) {
                first
            } else {
                'M'
            };
            (status, c.path)
        })
        .collect())
}

/// The comment prefix git uses (`core.commentString`, `core.commentChar`, else `#`).
pub fn comment_prefix(repo: &Path) -> String {
    let v = probe(repo, &["config", "--get", "core.commentString"])
        .or_else(|| probe(repo, &["config", "--get", "core.commentChar"]))
        .unwrap_or_default();
    if v.is_empty() || v == "auto" {
        "#".into()
    } else {
        v
    }
}

/// Every file named `Cargo.lock` that git tracks or would add (ignored files left out), repository-relative.
pub fn lockfiles(repo: &Path) -> Result<Vec<String>, String> {
    let out = run_bytes(
        repo,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut v: Vec<String> = out
        .split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .filter(|p| p == "Cargo.lock" || p.ends_with("/Cargo.lock"))
        .collect();
    v.sort();
    v.dedup();
    Ok(v)
}

/// Tracked and untracked-but-not-ignored files, repository-relative.
pub fn files(repo: &Path) -> Result<Vec<String>, String> {
    let out = run_bytes(
        repo,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut v: Vec<String> = out
        .split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    v.sort();
    v.dedup();
    Ok(v)
}
