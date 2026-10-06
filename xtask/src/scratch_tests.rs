//! Scratch-repository and scratch-workspace tests of the gate, the hooks and the CI checks (docs/m0/PLAN.md WP-02,
//! WP-03 E10, WP-04). Each test builds its own git repository or cargo workspace under the system temporary
//! directory, with an isolated git configuration and synthetic data, and removes it afterwards.

use crate::{ci, config, gate, hook, lint_deps, metadata, private};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("moirai-xtask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("gitconfig"), "").unwrap();
        Scratch { dir }
    }

    fn git(&self, repo: &Path, args: &[&str]) -> (bool, String) {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.dir.join("gitconfig"))
            .env("GIT_AUTHOR_NAME", "Test Author")
            .env("GIT_AUTHOR_EMAIL", "author@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test Author")
            .env("GIT_COMMITTER_EMAIL", "author@example.invalid")
            .output()
            .unwrap();
        let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.success(), s)
    }

    fn ok(&self, repo: &Path, args: &[&str]) -> String {
        let (ok, out) = self.git(repo, args);
        assert!(ok, "git {args:?}: {out}");
        out
    }

    /// A repository with one commit on `master`.
    fn repo(&self) -> PathBuf {
        let r = self.dir.join("repo");
        std::fs::create_dir_all(&r).unwrap();
        self.ok(&r, &["init", "-q", "-b", "master"]);
        self.ok(&r, &["config", "core.autocrlf", "false"]);
        std::fs::write(r.join(".gitignore"), "/private/\n").unwrap();
        std::fs::create_dir_all(r.join("docs/m0")).unwrap();
        let authors = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/m0/authors.md"),
        )
        .unwrap();
        std::fs::write(r.join("docs/m0/authors.md"), authors).unwrap();
        self.ok(&r, &["add", "-A"]);
        self.ok(&r, &["commit", "-q", "-m", "Initial commit"]);
        r
    }

    fn commit_file(&self, repo: &Path, path: &str, content: &[u8], msg: &str) -> String {
        let p = repo.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        self.ok(repo, &["add", "-f", "--", path]);
        self.ok(repo, &["commit", "-q", "-m", msg]);
        self.ok(repo, &["rev-parse", "HEAD"]).trim().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_branch_with_an_ai_trailer_is_refused() {
    let s = Scratch::new("markers");
    let r = s.repo();
    s.ok(&r, &["checkout", "-q", "-b", "m0/r-harn-i"]);
    s.commit_file(&r, "xtask/a.txt", b"a\n", "WP-02: a clean commit");
    let (d, n) = gate::markers_scan(&r, "master..HEAD").unwrap();
    assert_eq!(n, 1);
    assert!(d.is_empty(), "{d:?}");
    s.commit_file(
        &r,
        "xtask/b.txt",
        b"b\n",
        "WP-02: add b\n\nCo-authored-by: Claude <noreply@anthropic.com>",
    );
    let (d, n) = gate::markers_scan(&r, "master..HEAD").unwrap();
    assert_eq!(n, 2);
    assert!(d.iter().any(|x| x.message.contains("rule 1")), "{d:?}");
    // A marker only git's trailer parser sees (a folded trailer).
    s.commit_file(
        &r,
        "xtask/c.txt",
        b"c\n",
        "WP-02: add c\n\nBody.\n\nTested-on: a\n  Codex",
    );
    let (d, _) = gate::markers_scan(&r, "HEAD~1..HEAD").unwrap();
    assert!(d.iter().any(|x| x.message.contains("git trailer")), "{d:?}");
}

#[test]
fn authors_scan_on_a_scratch_branch() {
    let s = Scratch::new("authors");
    let r = s.repo();
    s.ok(&r, &["checkout", "-q", "-b", "m0/r-harn-i"]);
    s.commit_file(&r, "xtask/src/x.rs", b"fn x() {}\n", "WP-02: tool");
    let (d, n, skipped) = gate::authors_scan(&r, "master..HEAD", None).unwrap();
    assert!(d.is_empty() && n == 1 && skipped == 0, "{d:?}");
    s.commit_file(
        &r,
        "crates/moirai-model/src/lib.rs",
        b"//! m\n",
        "WP-02: not my crate",
    );
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", None).unwrap();
    assert_eq!(d.len(), 1, "{d:?}");
    // WP-01's skeleton exemption: a first add under WP-01 passes; a re-add after a deletion, and any WP-01b commit,
    // do not.
    s.commit_file(
        &r,
        "crates/moirai-diff/Cargo.toml",
        b"[package]\n",
        "WP-01: skeleton",
    );
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", None).unwrap();
    assert!(d.is_empty(), "{d:?}");
    s.ok(&r, &["rm", "-q", "crates/moirai-diff/Cargo.toml"]);
    s.ok(&r, &["commit", "-q", "-m", "Owner: drop the manifest"]);
    s.commit_file(
        &r,
        "crates/moirai-diff/Cargo.toml",
        b"[package]\nname = \"x\"\n",
        "WP-01: skeleton again",
    );
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", None).unwrap();
    assert_eq!(d.len(), 1, "{d:?}");
    s.commit_file(
        &r,
        "crates/moirai-diff/Cargo.toml",
        b"[package]\nname = \"y\"\n",
        "WP-01b: rewrite a crate root",
    );
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", None).unwrap();
    assert_eq!(d.len(), 1, "{d:?}");
}

#[test]
fn authors_in_branch_mode_and_malformed_subjects() {
    let s = Scratch::new("authors-branch");
    let r = s.repo();
    s.ok(&r, &["checkout", "-q", "-b", "m0/r-harn-i"]);
    // A subject that starts with WP- but does not parse is a finding in every mode.
    s.commit_file(&r, "xtask/src/a.rs", b"a\n", "WP-02 fix: a");
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", None).unwrap();
    assert!(
        d.len() == 1 && d[0].message.contains("does not parse"),
        "{d:?}"
    );
    // An owner-style subject is skipped outside branch mode and refused in it.
    s.commit_file(&r, "xtask/src/b.rs", b"b\n", "tidy the tool");
    let (d, n, skipped) = gate::authors_scan(&r, "HEAD~1..HEAD", None).unwrap();
    assert!(d.is_empty() && n == 0 && skipped == 1, "{d:?}");
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", Some("R-HARN-I")).unwrap();
    assert!(
        d.len() == 1 && d[0].message.contains("no `WP-xx:`"),
        "{d:?}"
    );
    // A WP of another role is refused on the branch, even on a path that WP's role may write.
    s.commit_file(
        &r,
        "crates/moirai-model/src/x.rs",
        b"x\n",
        "WP-90: model code",
    );
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", None).unwrap();
    assert!(d.is_empty(), "{d:?}");
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", Some("R-HARN-I")).unwrap();
    assert!(
        d.len() == 1 && d[0].message.contains("not a WP of the branch's role"),
        "{d:?}"
    );
    // An evil merge: the lines and files it adds are checked against the branch's role.
    s.ok(&r, &["checkout", "-q", "-b", "side", "HEAD~3"]);
    s.commit_file(&r, "xtask/src/side.rs", b"side\n", "WP-02: side");
    s.ok(&r, &["checkout", "-q", "m0/r-harn-i"]);
    s.ok(&r, &["merge", "-q", "--no-commit", "--no-ff", "side"]);
    std::fs::create_dir_all(r.join("crates/moirai-model/src")).unwrap();
    std::fs::write(r.join("crates/moirai-model/src/evil.rs"), "evil\n").unwrap();
    s.ok(&r, &["add", "crates/moirai-model/src/evil.rs"]);
    s.ok(&r, &["commit", "-q", "-m", "Merge branch 'side'"]);
    let (d, _, _) = gate::authors_scan(&r, "HEAD~1..HEAD", Some("R-HARN-I")).unwrap();
    assert!(
        d.iter()
            .any(|x| x.path.as_deref() == Some("crates/moirai-model/src/evil.rs")),
        "{d:?}"
    );
    assert!(
        !d.iter()
            .any(|x| x.path.as_deref() == Some("xtask/src/side.rs")),
        "{d:?}"
    );
}

#[test]
fn diff_settings_cannot_hide_a_pasted_line() {
    let s = Scratch::new("noprefix");
    let r = s.repo();
    let pd = r.join("private");
    std::fs::create_dir_all(&pd).unwrap();
    std::fs::write(
        pd.join("session.txt"),
        "the owner wrote this private sentence about the storage engine and its crash recovery rules\r\n",
    )
    .unwrap();
    private::index(&pd, &[]).unwrap();
    std::fs::create_dir_all(r.join("docs")).unwrap();
    std::fs::write(
        r.join("docs/notes.md"),
        "Intro.\nprivate sentence about the storage engine and its crash\n",
    )
    .unwrap();
    s.ok(&r, &["add", "docs/notes.md"]);
    for (key, value) in [
        ("diff.mnemonicPrefix", "true"),
        ("diff.noprefix", "true"),
        ("diff.srcPrefix", "q/"),
        ("diff.dstPrefix", "w/"),
        ("diff.relative", "true"),
        ("diff.external", "false"),
    ] {
        s.ok(&r, &["config", key, value]);
        let d = hook::pre_commit(&r, true, Some(&pd)).unwrap();
        assert!(
            d.iter().any(|x| x.path.as_deref() == Some("docs/notes.md")
                && x.message.contains("partial copy")),
            "{key}: {d:?}"
        );
    }
    // The same settings cannot hide it from the gate's scan of a commit either.
    s.ok(&r, &["commit", "-q", "-m", "WP-02: notes"]);
    let (d, _) = gate::private_scan(&r, "HEAD~1..HEAD").unwrap();
    assert!(
        d.iter().any(|x| x.message.contains("partial copy")),
        "{d:?}"
    );
    // A CRLF private file copied into the repository is stored as LF (text=auto) and still refused.
    std::fs::write(r.join(".gitattributes"), "* text=auto eol=lf\n").unwrap();
    std::fs::copy(pd.join("session.txt"), r.join("copied.txt")).unwrap();
    s.ok(&r, &["add", ".gitattributes", "copied.txt"]);
    let d = hook::pre_commit(&r, true, Some(&pd)).unwrap();
    assert!(
        d.iter()
            .any(|x| x.path.as_deref() == Some("copied.txt")
                && x.message.contains("BLAKE3 is listed")),
        "{d:?}"
    );
}

#[test]
fn evil_merges_are_checked() {
    let s = Scratch::new("evil");
    let r = s.repo();
    let pd = r.join("private");
    std::fs::create_dir_all(&pd).unwrap();
    std::fs::write(
        pd.join("session.txt"),
        "the owner wrote this private sentence about the storage engine and its crash recovery rules\n",
    )
    .unwrap();
    private::index(&pd, &[]).unwrap();
    let base = s.commit_file(&r, "docs/a.md", b"one\ntwo\n", "WP-02: a");
    s.ok(&r, &["checkout", "-q", "-b", "side"]);
    s.commit_file(&r, "docs/b.md", b"b\n", "WP-02: b");
    s.ok(&r, &["checkout", "-q", "master"]);
    s.commit_file(&r, "docs/c.md", b"c\n", "WP-02: c");
    s.ok(&r, &["merge", "-q", "--no-commit", "--no-ff", "side"]);
    std::fs::write(
        r.join("docs/a.md"),
        "one\nprivate sentence about the storage engine and its crash\ntwo\n",
    )
    .unwrap();
    std::fs::write(r.join("big.bin"), vec![7u8; 1_048_577]).unwrap();
    s.ok(&r, &["add", "docs/a.md", "big.bin"]);
    s.ok(&r, &["commit", "-q", "-m", "Merge branch 'side'"]);
    let merge = s.ok(&r, &["rev-parse", "HEAD"]).trim().to_string();
    let (d, note) = gate::private_scan(&r, &format!("{base}..HEAD")).unwrap();
    assert!(note.contains("(3 commits)"), "{note}");
    assert!(
        d.iter().any(|x| x.path.as_deref() == Some("docs/a.md")
            && x.message.contains(&merge[..10])
            && x.message.contains("partial copy")),
        "{d:?}"
    );
    let rev = crate::git::Rev {
        sha: merge,
        merge: true,
    };
    let d = ci::diff_rules(&r, &rev).unwrap();
    assert!(
        d.iter().any(|x| x.path.as_deref() == Some("big.bin")),
        "{d:?}"
    );
    // The merge's clean side (docs/b.md, taken as is) is not its own change.
    let changes = crate::git::commit_changes(&r, &rev).unwrap();
    assert!(
        !changes.iter().any(|(_, p)| p == "docs/b.md"),
        "{changes:?}"
    );
    assert!(
        changes.contains(&('A', "big.bin".to_string())),
        "{changes:?}"
    );
}

#[test]
fn pre_commit_checks_in_a_scratch_repository() {
    let s = Scratch::new("precommit");
    let r = s.repo();
    let pd = r.join("private");
    std::fs::create_dir_all(pd.join("notes")).unwrap();
    std::fs::write(
        pd.join("notes/session.txt"),
        "the owner wrote this private sentence about the storage engine and its crash recovery rules\n",
    )
    .unwrap();
    std::fs::write(pd.join("notes/blob.bin"), [0u8, 7, 7, 7]).unwrap();
    private::index(&pd, &[]).unwrap();

    // Guard true and no manifest: fails closed.
    let other = s.dir.join("empty-private");
    std::fs::create_dir_all(&other).unwrap();
    assert!(
        hook::pre_commit(&r, true, Some(&other))
            .unwrap_err()
            .contains("failing closed")
    );
    assert!(hook::pre_commit(&r, true, None).is_err());
    assert!(
        hook::pre_commit(&r, false, Some(&other))
            .unwrap()
            .is_empty()
    );

    // A partial copy: one pasted line of a private file, re-wrapped.
    std::fs::create_dir_all(r.join("docs")).unwrap();
    std::fs::write(
        r.join("docs/notes.md"),
        "Intro.\nprivate sentence about the storage engine and its crash\n",
    )
    .unwrap();
    s.ok(&r, &["add", "docs/notes.md"]);
    let d = hook::pre_commit(&r, true, Some(&pd)).unwrap();
    assert!(
        d.iter()
            .any(|x| x.path.as_deref() == Some("docs/notes.md") && x.line == Some(2)),
        "{d:?}"
    );
    s.ok(&r, &["reset", "-q"]);

    // A copied private file.
    std::fs::copy(pd.join("notes/blob.bin"), r.join("copied.bin")).unwrap();
    s.ok(&r, &["add", "copied.bin"]);
    let d = hook::pre_commit(&r, true, Some(&pd)).unwrap();
    assert!(
        d.iter().any(|x| x.path.as_deref() == Some("copied.bin")),
        "{d:?}"
    );
    s.ok(&r, &["reset", "-q"]);
    std::fs::remove_file(r.join("copied.bin")).unwrap();

    // A report line with a user path.
    std::fs::create_dir_all(r.join("docs/measurements/m0")).unwrap();
    std::fs::write(
        r.join("docs/measurements/m0/11.md"),
        "median 3 ms\nraw data under C:\\Users\\someone\\x.json\n",
    )
    .unwrap();
    s.ok(&r, &["add", "docs/measurements/m0/11.md"]);
    let d = hook::pre_commit(&r, false, Some(&pd)).unwrap();
    assert!(
        d.iter()
            .any(|x| x.lint == "report-scrub" && x.line == Some(2)),
        "{d:?}"
    );
    s.ok(&r, &["reset", "-q"]);

    // Clean changes pass; then a stale manifest refuses every commit.
    std::fs::write(
        r.join("docs/clean.md"),
        "Nothing private here at all, only public words.\n",
    )
    .unwrap();
    s.ok(&r, &["add", "docs/clean.md"]);
    assert!(hook::pre_commit(&r, true, Some(&pd)).unwrap().is_empty());
    std::fs::write(pd.join("notes/new.txt"), "added after indexing\n").unwrap();
    assert!(
        hook::pre_commit(&r, true, Some(&pd))
            .unwrap_err()
            .contains("stale")
    );
}

#[test]
fn empty_files_pass_the_hook_and_the_gate() {
    // An empty file under /private/ (cargo-mutants' `timeout.txt` of a GT16 shard in which nothing timed out) makes
    // neither an added empty file nor an emptied one "a copy"; a copied non-empty result is still refused.
    let s = Scratch::new("emptyfiles");
    let r = s.repo();
    let pd = r.join("private");
    let out = pd.join("nightly/20261013T210000Z/gt16/mutants.out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("timeout.txt"), b"").unwrap();
    std::fs::write(
        out.join("caught.txt"),
        b"src/lib.rs:3:5: replace f -> u8 with 0\n",
    )
    .unwrap();
    private::index(&pd, &[]).unwrap();
    s.commit_file(&r, "docs/full.md", b"some text\n", "WP-05: full");
    std::fs::create_dir_all(r.join("docs/empty")).unwrap();
    std::fs::write(r.join("docs/empty/.gitkeep"), b"").unwrap();
    std::fs::write(r.join("docs/full.md"), b"").unwrap();
    s.ok(&r, &["add", "docs/empty/.gitkeep", "docs/full.md"]);
    let d = hook::pre_commit(&r, true, Some(&pd)).unwrap();
    assert!(d.is_empty(), "{d:?}");
    s.ok(&r, &["commit", "-q", "-m", "WP-05: empty files"]);
    let (d, note) = gate::private_scan(&r, "HEAD~1..HEAD").unwrap();
    assert!(d.is_empty(), "{d:?}");
    assert!(note.contains("manifest current"), "{note}");
    std::fs::copy(out.join("caught.txt"), r.join("copied.txt")).unwrap();
    s.ok(&r, &["add", "copied.txt"]);
    let d = hook::pre_commit(&r, true, Some(&pd)).unwrap();
    assert!(
        d.iter()
            .any(|x| x.path.as_deref() == Some("copied.txt")
                && x.message.contains("BLAKE3 is listed")),
        "{d:?}"
    );
}

#[test]
fn ci_commit_rules_on_a_scratch_repository() {
    let s = Scratch::new("ci");
    let r = s.repo();
    let base = s.ok(&r, &["rev-parse", "HEAD"]).trim().to_string();
    let c1 = s.commit_file(&r, "private/leak.txt", b"x\n", "WP-02: leak");
    let c2 = s.commit_file(&r, "big.bin", &vec![0u8; 1_048_577], "WP-02: big");
    let c3 = s.commit_file(
        &r,
        "fixtures/big.bin",
        &vec![1u8; 1_048_577],
        "WP-02: big fixture",
    );
    s.commit_file(
        &r,
        "x.txt",
        b"y\n",
        "WP-02: x\n\nGenerated with Claude Code",
    );
    let head = s.ok(&r, &["rev-parse", "HEAD"]).trim().to_string();
    let rev = |sha: &str| crate::git::Rev {
        sha: sha.to_string(),
        merge: false,
    };
    assert!(
        ci::diff_rules(&r, &rev(&c1))
            .unwrap()
            .iter()
            .any(|d| d.message.contains("private/"))
    );
    assert!(
        ci::diff_rules(&r, &rev(&c2))
            .unwrap()
            .iter()
            .any(|d| d.message.contains("over 1 MiB"))
    );
    assert!(ci::diff_rules(&r, &rev(&c3)).unwrap().is_empty());
    struct Owner;
    impl ci::Logins for Owner {
        fn logins(&self, _: &str) -> Result<(Option<String>, Option<String>), String> {
            Ok((Some("owner".into()), Some("owner".into())))
        }
    }
    let ev = s.dir.join("event.json");
    let event = serde_json::json!({
        "pull_request": {"base": {"sha": base}, "head": {"sha": head}, "title": "t", "body": ""},
        "repository": {"full_name": "owner/moirai", "owner": {"login": "owner"}},
    });
    std::fs::write(&ev, event.to_string()).unwrap();
    let d = ci::commits(&r, &ev, Some(&Owner)).unwrap();
    let m: Vec<_> = d.iter().filter(|x| x.lint == "markers").collect();
    assert!(
        !m.is_empty() && m.iter().all(|x| x.message.contains("(WP-02: x)")),
        "{d:#?}"
    );
    assert_eq!(d.iter().filter(|x| x.lint == "diff").count(), 2, "{d:#?}");
    // A push whose 'before' is not an ancestor fails loudly.
    let push = serde_json::json!({"before": c3, "after": c2, "repository": {"full_name": "owner/moirai", "owner": {"login": "owner"}}});
    std::fs::write(&ev, push.to_string()).unwrap();
    assert!(
        ci::commits(&r, &ev, Some(&Owner))
            .unwrap_err()
            .contains("not an ancestor")
    );
}

/// Runs cargo in a scratch copy of a build-case workspace, offline first: `(success, stdout, stdout and stderr)`.
fn scratch_cargo(
    s: &Scratch,
    case: &str,
    args: &[&str],
    env: &[(String, String)],
) -> (bool, String, String) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/build")
        .join(case);
    let dst = s.dir.join(case);
    if !dst.exists() {
        copy_dir(&src, &dst);
    }
    let attempt = |offline: bool| {
        let mut c = Command::new("cargo");
        c.current_dir(&dst)
            .args(args)
            .env("CARGO_TARGET_DIR", s.dir.join("target"))
            .env("CARGO_TERM_COLOR", "never");
        if offline {
            c.arg("--offline");
        }
        for (k, v) in env {
            c.env(k, v);
        }
        let out = c.output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut all = stdout.clone();
        all.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.success(), stdout, all)
    };
    let (ok, stdout, all) = attempt(true);
    if !ok
        && (all.contains("offline")
            || all.contains("no matching package")
            || all.contains("failed to download"))
    {
        return attempt(false);
    }
    (ok, stdout, all)
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let t = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

#[test]
fn build_cases_in_scratch_workspaces() {
    let s = Scratch::new("build");
    // A `cc` build dependency: rules 1 and 2 refuse it (resolved by cargo, in the scratch copy).
    let (ok, json, all) = scratch_cargo(
        &s,
        "cc-build-dep",
        &["metadata", "--format-version", "1"],
        &[],
    );
    assert!(ok, "{all}");
    let md = metadata::Metadata::from_json(&json).unwrap();
    let cfg = config::Config::default();
    let targets = vec![("host".to_string(), md.clone())];
    let d = lint_deps::gt20b(&lint_deps::DepInputs {
        targets: &targets,
        full: &md,
        fuzz: None,
        lockfiles: &[("Cargo.lock".into(), "version = 4\n".into())],
        found_lockfiles: &[],
        config: &cfg,
    });
    assert!(
        d.iter().any(|x| x
            .message
            .contains("seeded-cc-build-dep 0.0.0 has a build script")),
        "{d:#?}"
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("build dependency on cc")),
        "{d:#?}"
    );
    // C source under the poisoned compiler: the build fails deterministically.
    let (ok, _, out) = scratch_cargo(&s, "c-source", &["check"], &crate::cargo::poisoned_env());
    assert!(
        !ok,
        "the C build must fail under the poisoned compiler:\n{out}"
    );
    assert!(out.contains(crate::cargo::POISON), "{out}");
}

#[test]
fn clippy_refuses_the_method_forms_in_product_crates() {
    let s = Scratch::new("clippy");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build/clippy-methods");
    let run = |case: &str, config: Option<&str>| {
        let dst = s.dir.join(case);
        copy_dir(&src, &dst);
        if let Some(c) = config {
            std::fs::copy(repo.join(c), dst.join("clippy.toml")).unwrap();
        }
        let out = Command::new("cargo")
            .current_dir(&dst)
            .args(["clippy", "--offline", "--quiet", "--", "-D", "warnings"])
            .env("CARGO_TARGET_DIR", s.dir.join("target"))
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .unwrap();
        let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
        all.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.success(), all)
    };
    let refused = |text: &str, m: &str| text.contains(&format!("use of a disallowed method `{m}`"));
    // A product crate other than moirai-os: every seeded call is refused.
    let (ok, out) = run("product", Some("crates/moirai-vfs/clippy.toml"));
    assert!(!ok, "{out}");
    for m in [
        "std::fs::File::lock",
        "std::fs::File::try_lock",
        "std::fs::File::unlock",
        "std::path::Path::exists",
    ] {
        assert!(refused(&out, m), "{m}:\n{out}");
    }
    assert!(!out.contains("std::sync::Mutex"), "{out}");
    // moirai-os: the lock calls are refused, the Path I/O is not.
    let (ok, out) = run("os", Some("crates/moirai-os/clippy.toml"));
    assert!(!ok, "{out}");
    assert!(refused(&out, "std::fs::File::lock"), "{out}");
    assert!(!refused(&out, "std::path::Path::exists"), "{out}");
    // A crate without the file (a test-only or tool crate): nothing is refused.
    let (ok, out) = run("test-only", None);
    assert!(ok, "{out}");
}

#[test]
fn ps1_files_need_a_bom_when_not_ascii() {
    let s = Scratch::new("ps1");
    let r = s.repo();
    std::fs::write(r.join("ascii.ps1"), "Write-Output 'plain'\r\n").unwrap();
    std::fs::write(r.join("bom.ps1"), "\u{feff}Write-Output 'Grüße'\r\n").unwrap();
    std::fs::write(r.join("nobom.ps1"), "Write-Output 'Grüße'\r\n").unwrap();
    std::fs::write(r.join("Upper.PS1"), "Write-Output 'Привет'\r\n").unwrap();
    let d = gate::ps1_bom(&r).unwrap();
    let paths: Vec<&str> = d.iter().filter_map(|x| x.path.as_deref()).collect();
    assert_eq!(paths, vec!["Upper.PS1", "nobom.ps1"], "{d:?}");
}
