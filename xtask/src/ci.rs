//! `xtask ci commits`: the pull-request and `master`-push checks over every commit (docs/m0/PLAN.md WP-04).
//!
//! From `$GITHUB_EVENT_PATH` (a `pull_request` event: `base.sha..head.sha`; a `push` to `master`: `before..after`,
//! refused loudly when `before` is absent or is not an ancestor of `after`), for every commit of the range:
//! - the AI-marker rules of `commit-msg` over the message and the author and committer addresses;
//! - no path under `private/`, and no file over 1 MiB outside `fixtures/`, merges included through their combined
//!   change list ([`diff_rules`]);
//! - the author's GitHub-resolved login equals the repository owner, and the committer's is the owner or `web-flow`
//!   (a merge made in the web UI). Logins come from `gh api repos/<repo>/commits/<sha>`, with the job's read-only
//!   token.

use crate::diag::Diag;
use crate::gate;
use crate::git;
use crate::private;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

pub const MAX_BYTES: u64 = 1_048_576;

/// Resolves a commit's author and committer logins.
pub trait Logins {
    fn logins(&self, sha: &str) -> Result<(Option<String>, Option<String>), String>;
}

/// `gh api` with the job's token.
pub struct Gh {
    pub repo: String,
}

impl Logins for Gh {
    fn logins(&self, sha: &str) -> Result<(Option<String>, Option<String>), String> {
        let out = Command::new("gh")
            .args([
                "api",
                &format!("repos/{}/commits/{sha}", self.repo),
                "--jq",
                "[(.author.login // \"\"), (.committer.login // \"\")] | @tsv",
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("gh api: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "gh api repos/{}/commits/{sha} failed: {}",
                self.repo,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let mut it = s.trim_end_matches(['\n', '\r']).split('\t');
        let some = |x: Option<&str>| {
            x.map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        Ok((some(it.next()), some(it.next())))
    }
}

pub fn check_logins(commits: &[String], owner: &str, l: &dyn Logins) -> Vec<Diag> {
    let mut out = Vec::new();
    for sha in commits {
        let short = &sha[..sha.len().min(10)];
        match l.logins(sha) {
            Err(e) => out.push(Diag::new("identity", format!("commit {short}: {e}"))),
            Ok((author, committer)) => {
                if author.as_deref() != Some(owner) {
                    out.push(Diag::new(
                        "identity",
                        format!(
                            "commit {short}: author login {} is not the repository owner {owner}",
                            author.as_deref().unwrap_or("(unresolved)")
                        ),
                    ));
                }
                match committer.as_deref() {
                    Some(c) if c == owner || c == "web-flow" => {}
                    c => out.push(Diag::new(
                        "identity",
                        format!("commit {short}: committer login {} is neither the owner {owner} nor web-flow", c.unwrap_or("(unresolved)")),
                    )),
                }
            }
        }
    }
    out
}

/// Object sizes through one `git cat-file --batch-check`.
fn sizes(repo: &Path, ids: &[String]) -> Result<Vec<u64>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch-check=%(objectsize)"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git cat-file: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let owned: Vec<String> = ids.to_vec();
    let w = std::thread::spawn(move || -> std::io::Result<()> {
        for id in owned {
            writeln!(stdin, "{id}")?;
        }
        Ok(())
    });
    let r = BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let mut out = Vec::with_capacity(ids.len());
    for line in r.lines() {
        let line = line.map_err(|e| e.to_string())?;
        out.push(
            line.trim()
                .parse::<u64>()
                .map_err(|_| format!("git cat-file: '{line}'"))?,
        );
    }
    w.join()
        .map_err(|_| "writer panicked".to_string())?
        .map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())?;
    if out.len() != ids.len() {
        return Err("git cat-file: short reply".into());
    }
    Ok(out)
}

/// The diff rules over one commit: `private/**` and files over 1 MiB outside `fixtures/`. A merge is checked on its
/// combined change list: the files whose merged version differs from every parent, which is what the merge itself
/// adds (an evil merge, a conflict resolution); the rest was checked on the commits that brought it.
pub fn diff_rules(repo: &Path, rev: &git::Rev) -> Result<Vec<Diag>, String> {
    let short = &rev.sha[..rev.sha.len().min(10)];
    let raw = git::commit_raw(repo, rev)?;
    let changed: Vec<private::Changed> = private::parse_raw_z(&raw)
        .into_iter()
        .filter(|c| c.mode != "160000" && c.blob.bytes().any(|b| b != b'0'))
        .collect();
    let mut out = Vec::new();
    for c in &changed {
        if c.path.to_ascii_lowercase().starts_with("private/") {
            out.push(Diag::path(
                "diff",
                &c.path,
                format!("commit {short}: owner data under private/ is never committed"),
            ));
        }
    }
    let ids: Vec<String> = changed.iter().map(|c| c.blob.clone()).collect();
    for (c, size) in changed.iter().zip(sizes(repo, &ids)?) {
        if size > MAX_BYTES && !c.path.starts_with("fixtures/") {
            out.push(Diag::path(
                "diff",
                &c.path,
                format!("commit {short}: {size} bytes, over 1 MiB outside fixtures/"),
            ));
        }
    }
    Ok(out)
}

/// `xtask ci commits`.
pub fn commits(
    repo: &Path,
    event: &Path,
    logins: Option<&dyn Logins>,
) -> Result<Vec<Diag>, String> {
    let text = std::fs::read_to_string(event).map_err(|e| format!("{}: {e}", event.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("event JSON: {e}"))?;
    let owner = v
        .pointer("/repository/owner/login")
        .and_then(|x| x.as_str())
        .ok_or("the event has no repository.owner.login")?
        .to_string();
    let full = v
        .pointer("/repository/full_name")
        .and_then(|x| x.as_str())
        .ok_or("the event has no repository.full_name")?
        .to_string();
    let range = gate::ci_range(event)?;
    if v.get("pull_request").is_none() {
        let (before, after) = range.split_once("..").unwrap_or((&range, ""));
        if git::run(repo, &["merge-base", "--is-ancestor", before, after]).is_err() {
            return Err(format!(
                "push {range}: 'before' is not an ancestor of 'after' (a force push?): refused"
            ));
        }
    }
    let (mut out, n) = gate::markers_scan(repo, &range)?;
    println!("ci commits: {n} commits in {range}");
    let revs = git::rev_list(repo, &range)?;
    for rev in &revs {
        out.extend(diff_rules(repo, rev)?);
    }
    let all: Vec<String> = revs.into_iter().map(|r| r.sha).collect();
    let gh = Gh { repo: full };
    out.extend(check_logins(&all, &owner, logins.unwrap_or(&gh)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl Logins for Fake {
        fn logins(&self, sha: &str) -> Result<(Option<String>, Option<String>), String> {
            Ok(match sha {
                "a1" => (Some("owner".into()), Some("owner".into())),
                "a2" => (Some("owner".into()), Some("web-flow".into())),
                "b1" => (Some("someone".into()), Some("owner".into())),
                "b2" => (None, Some("owner".into())),
                "b3" => (Some("owner".into()), Some("someone".into())),
                _ => return Err("not found".into()),
            })
        }
    }

    #[test]
    fn logins() {
        let ok = check_logins(&["a1".into(), "a2".into()], "owner", &Fake);
        assert!(ok.is_empty(), "{ok:?}");
        let bad = check_logins(
            &["b1".into(), "b2".into(), "b3".into(), "zz".into()],
            "owner",
            &Fake,
        );
        assert_eq!(bad.len(), 4, "{bad:#?}");
        assert!(bad[0].message.contains("someone"));
        assert!(bad[1].message.contains("(unresolved)"));
        assert!(bad[2].message.contains("committer"));
    }
}
