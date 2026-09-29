//! `xtask authors`: every path a `WP-xx:` commit touches may be written by that work package's role
//! (docs/m0/PLAN.md §3.1 "Writes"; docs/m0/authors.md §1, §2, §3, §4, §5 item 6, §6 items 2, 5 and 10).
//!
//! The ledger and the path map are read from authors.md itself, so the file stays the single record: §2's table gives
//! each WP its roles, §3's table each path pattern its writers and, in its notes column, two parseable limits:
//! - `only in WP-81a and WP-99`: the path may be written only by a commit that names one of those WPs;
//! - `named with its lens`: a review lens (`R-REV-P`, `-S`, `-A`) writes only files whose name carries its lens
//!   letter as a `-`, `_` or `.`-separated token (`P-pass1.md`, `a1-s.md`).
//!
//! A path takes the writers of its most specific entries (authors.md §3 "Matching"); a role matches a writer when the
//! names are equal or the writer is the role's prefix (`R-HARN` covers `R-HARN-I`). Entries naming no role (`none`,
//! `owner only`, `no M0 role`, `the gate worktree only`) admit no WP.
//!
//! Subjects: `WP-02: …`, `WP-02, WP-03: …` and `WP-02+03b: …` name WPs; a suffixed id that §2 does not list
//! (`WP-03b`) is checked as its numbered row. A subject that starts with `WP-` but does not parse (`WP-02 fix: …`) is
//! a finding. Outside `--branch` mode a commit whose subject names no WP is an owner commit and is skipped; in
//! `--branch m0/<role>` mode every commit must name a WP whose ledger roles include the branch's role (merges
//! excepted), and its paths are checked against that role alone (authors.md §5 item 6).
//!
//! Three rules sit beside the tables:
//! - authors.md §4: the crate skeletons (each crate's `Cargo.toml` and crate root) may be *added* by a commit whose
//!   subject names `WP-01` itself (not a suffixed `WP-01b` read as WP-01), and only where no ancestor had the path;
//! - `Cargo.lock` and `fuzz/Cargo.lock` change only as a side effect of cargo in the gate worktree (authors.md §3):
//!   any WP commit may carry them, and every gate command runs `--locked`, so a lockfile that disagrees with the
//!   manifests cannot pass;
//! - `NOTICE` is append-only (authors.md §6 item 5): a change must keep the old text as its prefix.
//!
//! A merge commit is checked on its combined change list: the files whose merged version differs from every parent,
//! which is what the merge itself writes (an evil merge, a conflict resolution).

use crate::diag::Diag;
use crate::paths::Pattern;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Entry {
    pub pattern: Pattern,
    pub writers: Vec<String>,
    pub label: String,
    /// `only in WP-…` in the notes: the WPs that may write the path.
    pub only_wps: Vec<String>,
    /// `named with its lens` in the notes: a lens writes only files named with its letter.
    pub lens_named: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Ledger {
    /// WP id (`WP-01`, `WP-53c`) to the role names of its row.
    pub wps: BTreeMap<String, Vec<String>>,
    pub entries: Vec<Entry>,
}

/// Role names (`R-HARN-I`, `R-FL1A`) in a table cell.
pub fn roles_in(cell: &str) -> Vec<String> {
    let b = cell.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < b.len() {
        if b[i] == b'R' && b[i + 1] == b'-' && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let mut j = i + 2;
            while j < b.len()
                && (b[j].is_ascii_uppercase() || b[j].is_ascii_digit() || b[j] == b'-')
            {
                j += 1;
            }
            let tok = cell[i..j].trim_end_matches('-');
            if tok.len() > 2 && !out.iter().any(|o| o == tok) {
                out.push(tok.to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// `WP-53a–e` → `WP-53a` … `WP-53e`; `WP-01` → `WP-01`.
fn expand_wp(cell: &str) -> Vec<String> {
    let c = cell.trim();
    let Some(rest) = c.strip_prefix("WP-") else {
        return Vec::new();
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let tail = &rest[digits.len()..];
    let range: Vec<char> = tail
        .chars()
        .filter(|ch| ch.is_ascii_lowercase() || *ch == '–' || *ch == '-')
        .collect();
    if range.len() == 3 && (range[1] == '–' || range[1] == '-') {
        let (a, b) = (range[0], range[2]);
        return (a..=b).map(|l| format!("WP-{digits}{l}")).collect();
    }
    let letters: String = tail.chars().take_while(char::is_ascii_lowercase).collect();
    vec![format!("WP-{digits}{letters}")]
}

/// The WP ids of a notes cell's `only in WP-… [and|, WP-…]` clause (up to the next `,` or `;` that is not followed by
/// another WP id).
fn only_in(notes: &str) -> Vec<String> {
    let Some(i) = notes.find("only in ") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for word in notes[i + "only in ".len()..].split_whitespace() {
        let w = word.trim_matches(|c: char| c == ',' || c == ';' || c == '.' || c == '`');
        if w == "and" || w == "or" {
            continue;
        }
        if !w.starts_with("WP-") {
            break;
        }
        out.extend(expand_wp(w));
        if word.ends_with(';') || word.ends_with('.') {
            break;
        }
    }
    out
}

pub fn parse(md: &str) -> Result<Ledger, String> {
    let mut l = Ledger::default();
    let mut section = 0;
    let mut in_map = false;
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            section = h
                .split('.')
                .next()
                .and_then(|n| n.trim().parse::<u32>().ok())
                .unwrap_or(0);
            in_map = false;
            continue;
        }
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let c = cells(line);
        if section == 2 && c.first().is_some_and(|x| x.starts_with("WP-")) && c.len() >= 5 {
            let roles = roles_in(&c[3]);
            for wp in expand_wp(&c[0]) {
                l.wps.insert(wp, roles.clone());
            }
        } else if section == 3 {
            if c.first().map(String::as_str) == Some("Path")
                && c.get(1).map(String::as_str) == Some("Writers")
            {
                in_map = true;
                continue;
            }
            if !in_map || c.first().is_some_and(|x| x.starts_with("---")) || c.len() < 2 {
                continue;
            }
            let writers = roles_in(&c[1]);
            let notes = c.get(2).map(String::as_str).unwrap_or("");
            let only_wps = only_in(notes);
            let lens_named = notes.contains("named with its lens");
            for p in backticked(&c[0]) {
                l.entries.push(Entry {
                    pattern: Pattern::new(&p),
                    writers: writers.clone(),
                    label: c[1].clone(),
                    only_wps: only_wps.clone(),
                    lens_named,
                });
            }
        }
    }
    if l.wps.is_empty() {
        return Err("docs/m0/authors.md: no WP rows found in §2".into());
    }
    if l.entries.is_empty() {
        return Err("docs/m0/authors.md: no path entries found in §3".into());
    }
    Ok(l)
}

/// A WP a subject names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WpRef {
    /// As the subject writes it (`WP-01b`).
    pub named: String,
    /// The §2 row it is checked as (`WP-01` for an unlisted `WP-01b`).
    pub row: String,
}

/// What a commit subject says about its work packages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// No `WP-` prefix: an owner commit (outside `--branch` mode).
    Plain,
    Wps(Vec<WpRef>),
    /// Starts with `WP-` but does not parse; the reason.
    Malformed(String),
}

impl Ledger {
    /// The most specific entries for a path (several when they are equally specific: a shared path).
    pub fn entries_for(&self, path: &str) -> Vec<&Entry> {
        let hits: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| e.pattern.matches(path))
            .collect();
        let Some(best) = hits.iter().map(|e| e.pattern.specificity()).max() else {
            return Vec::new();
        };
        hits.into_iter()
            .filter(|e| e.pattern.specificity() == best)
            .collect()
    }

    /// Parses a commit subject: `WP-02: …`, `WP-02, WP-03: …`, `WP-02+03b: …`.
    pub fn subject(&self, subject: &str) -> Subject {
        let s = subject.trim_start();
        if !s.starts_with("WP-") {
            return Subject::Plain;
        }
        let Some((head, _)) = s.split_once(':') else {
            return Subject::Malformed("no ':' after the WP ids".into());
        };
        let mut out = Vec::new();
        for part in head
            .split(['+', ',', '/', ' '])
            .filter(|p| !p.is_empty() && *p != "and")
        {
            let p = part.strip_prefix("WP-").unwrap_or(part);
            let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
            let letters: String = p[digits.len()..]
                .chars()
                .take_while(char::is_ascii_lowercase)
                .collect();
            if digits.is_empty() || digits.len() + letters.len() != p.len() {
                return Subject::Malformed(format!("'{part}' is not a WP id"));
            }
            let named = format!("WP-{digits}{letters}");
            let row = if self.wps.contains_key(&named) {
                named.clone()
            } else {
                format!("WP-{digits}")
            };
            out.push(WpRef { named, row });
        }
        if out.is_empty() {
            return Subject::Malformed("no WP id before ':'".into());
        }
        Subject::Wps(out)
    }
}

pub fn role_matches(role: &str, writer: &str) -> bool {
    role == writer || role.starts_with(&format!("{writer}-"))
}

/// WP-01's crate skeletons (authors.md §4).
fn skeleton_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    matches!(
        parts.as_slice(),
        ["crates", _, "Cargo.toml"]
            | ["crates", _, "src", "lib.rs"]
            | ["crates", "moirai-probes-bin", "src", "bin", "empty.rs"]
            | ["xtask", "Cargo.toml"]
            | ["xtask", "src", "main.rs"]
    )
}

/// The lens letter of a review role (`R-REV-P` → `P`).
fn lens_of(role: &str) -> Option<&str> {
    role.strip_prefix("R-REV-")
}

/// Whether a file name carries `lens` as a token (split at `-`, `_` and `.`).
fn named_with_lens(path: &str, lens: &str) -> bool {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .split(['-', '_', '.'])
        .any(|t| t.eq_ignore_ascii_case(lens))
}

/// One commit to check.
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub merge: bool,
    /// `(status letter, path)` from [`crate::git::commit_changes`].
    pub changes: Vec<(char, String)>,
}

/// What `check_commit` needs besides the ledger.
pub struct Checks<'a> {
    /// `--branch m0/<role>` mode: the branch role's title (`R-HARN-I`).
    pub branch_role: Option<&'a str>,
    /// Whether the commit's `NOTICE` keeps its parent's text as a prefix.
    pub notice_appended: &'a dyn Fn(&str) -> Result<bool, String>,
    /// Whether no ancestor of the commit `(sha, path)` had the path: the commit is the first to add it.
    pub first_added: &'a dyn Fn(&str, &str) -> Result<bool, String>,
}

/// Checks one commit. Returns the findings and whether the commit was checked (`false`: an owner commit skipped
/// outside `--branch` mode).
pub fn check_commit(l: &Ledger, c: &Commit, x: &Checks<'_>) -> (Vec<Diag>, bool) {
    let short = &c.sha[..c.sha.len().min(10)];
    let mut out = Vec::new();
    let refs = match l.subject(&c.subject) {
        Subject::Malformed(why) => {
            out.push(Diag::new(
                "authors",
                format!(
                    "commit {short} ('{}'): the subject starts with WP- but does not parse as `WP-xx[, WP-yy]: …` ({why})",
                    c.subject
                ),
            ));
            return (out, true);
        }
        Subject::Plain if c.merge || x.branch_role.is_none() => {
            // A merge's own changes are checked against the branch's role; outside --branch mode a subject
            // without a WP is an owner commit.
            if !(c.merge && x.branch_role.is_some()) {
                return (out, false);
            }
            Vec::new()
        }
        Subject::Plain => {
            out.push(Diag::new(
                "authors",
                format!(
                    "commit {short} ('{}') on a --branch run has no `WP-xx:` subject: every commit of a role branch names its WP (PLAN §3.1 \"Writes\")",
                    c.subject
                ),
            ));
            return (out, true);
        }
        Subject::Wps(r) => r,
    };
    let mut roles: Vec<String> = Vec::new();
    for w in &refs {
        match l.wps.get(&w.row) {
            Some(r) => roles.extend(r.iter().cloned()),
            None => out.push(Diag::new(
                "authors",
                format!(
                    "commit {short} names {}, which docs/m0/authors.md §2 does not list",
                    w.named
                ),
            )),
        }
    }
    if let Some(b) = x.branch_role {
        if refs.is_empty() || roles.iter().any(|r| role_matches(b, r)) {
            roles = vec![b.to_string()];
        } else if !roles.is_empty() {
            out.push(Diag::new(
                "authors",
                format!(
                    "commit {short} names {} ({}), not a WP of the branch's role {b} (authors.md §2, §5 item 6)",
                    refs.iter().map(|w| w.named.as_str()).collect::<Vec<_>>().join("+"),
                    roles.join(", ")
                ),
            ));
            return (out, true);
        }
    }
    if roles.is_empty() {
        return (out, true);
    }
    let rows: Vec<&str> = refs.iter().map(|w| w.row.as_str()).collect();
    let names_wp01 = refs.iter().any(|w| w.named == "WP-01");
    let what = if refs.is_empty() {
        "merge".to_string()
    } else {
        refs.iter()
            .map(|w| w.named.as_str())
            .collect::<Vec<_>>()
            .join("+")
    };
    for (status, path) in &c.changes {
        if path == "Cargo.lock" || path == "fuzz/Cargo.lock" {
            continue;
        }
        if *status == 'A' && names_wp01 && skeleton_path(path) {
            match (x.first_added)(&c.sha, path) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => {
                    out.push(Diag::path(
                        "authors",
                        path,
                        format!("commit {short}: cannot look up the path's history: {e}"),
                    ));
                    continue;
                }
            }
        }
        let entries = l.entries_for(path);
        let mut limit: Option<String> = None;
        let ok = entries.iter().any(|e| {
            let by_role = roles.iter().any(|r| {
                e.writers.iter().any(|w| {
                    role_matches(r, w)
                        && (!e.lens_named || lens_of(r).is_some_and(|k| named_with_lens(path, k)))
                })
            });
            if !by_role {
                if e.lens_named {
                    limit.get_or_insert_with(|| {
                        "each lens writes only the files named with its lens letter".into()
                    });
                }
                return false;
            }
            if !e.only_wps.is_empty() && !rows.iter().any(|w| e.only_wps.iter().any(|o| o == w)) {
                limit.get_or_insert_with(|| format!("only in {}", e.only_wps.join(", ")));
                return false;
            }
            true
        });
        if ok {
            if path == "NOTICE" && *status == 'M' {
                match (x.notice_appended)(&c.sha) {
                    Ok(true) => {}
                    Ok(false) => out.push(Diag::path(
                        "authors",
                        path,
                        format!("commit {short} changes NOTICE's existing text; NOTICE is append-only (authors.md §6 item 5)"),
                    )),
                    Err(e) => out.push(Diag::path("authors", path, format!("commit {short}: cannot compare NOTICE: {e}"))),
                }
            }
            continue;
        }
        let who = if entries.is_empty() {
            "no role (no authors.md §3 entry covers it)".to_string()
        } else {
            entries
                .iter()
                .map(|e| format!("{} (`{}`)", e.label, e.pattern.raw))
                .collect::<Vec<_>>()
                .join("; ")
        };
        out.push(Diag::path(
            "authors",
            path,
            format!(
                "commit {short} ({what}; {}) {} a path that may be written by: {who}{}",
                roles.join(", "),
                verb(*status),
                limit.map(|m| format!(" ({m})")).unwrap_or_default()
            ),
        ));
    }
    (out, true)
}

fn verb(status: char) -> &'static str {
    match status {
        'A' => "adds",
        'D' => "deletes",
        _ => "changes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> Ledger {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/m0/authors.md"),
        )
        .unwrap();
        parse(&text).unwrap()
    }

    fn commit(subject: &str, changes: &[(char, &str)]) -> Commit {
        Commit {
            sha: "0123456789abcdef".into(),
            subject: subject.into(),
            merge: false,
            changes: changes.iter().map(|(s, p)| (*s, p.to_string())).collect(),
        }
    }

    fn yes(_: &str) -> Result<bool, String> {
        Ok(true)
    }

    fn yes2(_: &str, _: &str) -> Result<bool, String> {
        Ok(true)
    }

    const PLAIN: Checks<'static> = Checks {
        branch_role: None,
        notice_appended: &yes,
        first_added: &yes2,
    };

    fn check(l: &Ledger, c: &Commit) -> Vec<Diag> {
        check_commit(l, c, &PLAIN).0
    }

    #[test]
    fn reads_the_repository_ledger() {
        let l = ledger();
        assert_eq!(l.wps["WP-02"], vec!["R-HARN-I".to_string()]);
        assert_eq!(l.wps["WP-53c"], vec!["R-HARN-M".to_string()]);
        assert!(l.wps["WP-80a"].contains(&"R-REV-S".to_string()));
        assert!(l.wps["WP-73"].contains(&"R-BENCH".to_string()));
        assert_eq!(l.wps["WP-14b"], vec!["R-SPEC-R".to_string()]);
        let w = |p: &str| {
            l.entries_for(p)
                .iter()
                .flat_map(|e| e.writers.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(w("xtask/src/gate.rs"), vec!["R-HARN".to_string()]);
        assert_eq!(w("xtask/src/ucd.rs"), vec!["R-FL1A".to_string()]);
        assert_eq!(
            w("crates/moirai-files/src/scan/rust.rs"),
            vec!["R-FL1B".to_string()]
        );
        let shared = w("crates/moirai-files/src/lib.rs");
        assert!(shared.contains(&"R-FL1A".to_string()) && shared.contains(&"R-FL1B".to_string()));
        assert_eq!(
            w("fuzz/fuzz_targets/anchor_selector.rs"),
            vec!["R-FL1A".to_string()]
        );
        assert!(w("docs/m0/PLAN.md").is_empty());
        assert!(w("private/x.txt").is_empty());
        assert!(w("LICENSE").is_empty());
        assert_eq!(
            w("docs/measurements/m0/replay/x.md"),
            vec!["R-REPLAY".to_string()]
        );
        assert_eq!(
            w("crates/moirai-files/clippy.toml"),
            vec!["R-HARN-I".to_string()]
        );
        assert_eq!(
            w("crates/moirai-toylog/src/bug/torn.rs"),
            vec!["R-TOY".to_string()]
        );
        let arch = l.entries_for("docs/ARCHITECTURE-RESEARCH.md");
        assert_eq!(arch[0].only_wps, vec!["WP-81a".to_string(), "WP-99".into()]);
        assert!(l.entries_for("docs/spec/reviews/P-pass1.md")[0].lens_named);
    }

    #[test]
    fn accepts_own_paths_and_refuses_others() {
        let l = ledger();
        let ok = commit(
            "WP-02: the gate",
            &[
                ('A', "xtask/src/gate.rs"),
                ('M', "Cargo.lock"),
                ('A', ".github/workflows/pr.yml"),
            ],
        );
        assert!(check(&l, &ok).is_empty());
        let bad = commit(
            "WP-02: the gate",
            &[
                ('M', "crates/moirai-model/src/lib.rs"),
                ('M', "docs/m0/PLAN.md"),
                ('A', "zzz/unknown.txt"),
            ],
        );
        let d = check(&l, &bad);
        assert_eq!(d.len(), 3, "{d:#?}");
        assert!(d[2].message.contains("no role"));
        let fl1b = commit(
            "WP-63: scanners",
            &[
                ('A', "crates/moirai-files/src/scan.rs"),
                ('M', "crates/moirai-files/src/lib.rs"),
            ],
        );
        assert!(check(&l, &fl1b).is_empty());
        let wrong = commit(
            "WP-63: scanners",
            &[('M', "crates/moirai-files/src/anchor.rs")],
        );
        assert_eq!(check(&l, &wrong).len(), 1);
        let spec = commit("WP-17: os", &[('A', "docs/spec/os/fs.md")]);
        assert!(check(&l, &spec).is_empty());
        let rules = commit("WP-10: conventions", &[('A', "docs/spec/rules/x.md")]);
        assert_eq!(check(&l, &rules).len(), 1);
    }

    #[test]
    fn wp01_skeletons_notice_and_subjects() {
        let l = ledger();
        let sk = commit(
            "WP-01: skeleton",
            &[
                ('A', "crates/moirai-model/Cargo.toml"),
                ('A', "crates/moirai-model/src/lib.rs"),
            ],
        );
        assert!(check(&l, &sk).is_empty());
        let not01 = commit("WP-02: x", &[('A', "crates/moirai-model/src/lib.rs")]);
        assert_eq!(check(&l, &not01).len(), 1);
        // The exemption is WP-01's own: not a suffixed id read as WP-01, not a change, not a re-add.
        let suffixed = commit("WP-01b: x", &[('M', "crates/moirai-model/src/lib.rs")]);
        assert_eq!(check(&l, &suffixed).len(), 1);
        let suffixed_add = commit(
            "WP-02+03b+01b+04: x",
            &[('A', "crates/moirai-format-oracle/Cargo.toml")],
        );
        assert_eq!(check(&l, &suffixed_add).len(), 1);
        let changed = commit("WP-01: x", &[('M', "crates/moirai-model/src/lib.rs")]);
        assert_eq!(check(&l, &changed).len(), 1);
        let deleted = commit("WP-01: x", &[('D', "crates/moirai-model/Cargo.toml")]);
        assert_eq!(check(&l, &deleted).len(), 1);
        let readd = Checks {
            first_added: &|_, _| Ok(false),
            ..PLAIN
        };
        assert_eq!(check_commit(&l, &sk, &readd).0.len(), 2);
        let notice = commit("WP-61: tables", &[('M', "NOTICE")]);
        assert!(check(&l, &notice).is_empty());
        let rewrote = Checks {
            notice_appended: &|_| Ok(false),
            ..PLAIN
        };
        assert_eq!(check_commit(&l, &notice, &rewrote).0.len(), 1);
        let owner = commit("M0 wave 1: plan", &[('A', "docs/m0/PLAN.md")]);
        assert_eq!(check_commit(&l, &owner, &PLAIN), (Vec::new(), false));
        let wp = |n: &str, r: &str| WpRef {
            named: n.into(),
            row: r.into(),
        };
        assert_eq!(
            l.subject("WP-02+03b+01b+04: infra"),
            Subject::Wps(vec![
                wp("WP-02", "WP-02"),
                wp("WP-03b", "WP-03"),
                wp("WP-01b", "WP-01"),
                wp("WP-04", "WP-04")
            ])
        );
        assert_eq!(
            l.subject("WP-14b: chapter 20"),
            Subject::Wps(vec![wp("WP-14b", "WP-14b")])
        );
        assert_eq!(
            l.subject("WP-02, WP-03: x"),
            Subject::Wps(vec![wp("WP-02", "WP-02"), wp("WP-03", "WP-03")])
        );
        assert_eq!(l.subject("Fix WP-02: x"), Subject::Plain);
        for bad in [
            "WP-X: nope",
            "WP-02 fix: x",
            "WP-02 (part 2): x",
            "WP-02 and more: x",
            "WP-02 without a colon",
        ] {
            assert!(matches!(l.subject(bad), Subject::Malformed(_)), "{bad}");
            let d = check(&l, &commit(bad, &[('A', "xtask/a.rs")]));
            assert!(
                d.len() == 1 && d[0].message.contains("does not parse"),
                "{d:?}"
            );
        }
        let unknown = commit("WP-999: x", &[('A', "xtask/a.rs")]);
        assert_eq!(check(&l, &unknown).len(), 1);
    }

    #[test]
    fn branch_mode_binds_commits_to_the_branch_role() {
        let l = ledger();
        let on = |role: &'static str| Checks {
            branch_role: Some(role),
            ..PLAIN
        };
        let harn = on("R-HARN-I");
        // A commit with no WP subject is refused on a role branch; a merge is checked on its own changes.
        let plain = commit("tidy up", &[('M', "xtask/src/gate.rs")]);
        let (d, checked) = check_commit(&l, &plain, &harn);
        assert!(
            checked && d.len() == 1 && d[0].message.contains("no `WP-xx:`"),
            "{d:?}"
        );
        let mut merge = commit("Merge branch 'master' into m0/r-harn-i", &[]);
        merge.merge = true;
        assert!(check_commit(&l, &merge, &harn).0.is_empty());
        merge.changes = vec![('M', "crates/moirai-model/src/lib.rs".into())];
        assert_eq!(check_commit(&l, &merge, &harn).0.len(), 1);
        merge.changes = vec![('M', "xtask/src/gate.rs".into())];
        assert!(check_commit(&l, &merge, &harn).0.is_empty());
        // A WP of another role is refused even when the path would pass as that role.
        let model = commit("WP-90: model", &[('M', "crates/moirai-model/src/lib.rs")]);
        assert!(check(&l, &model).is_empty());
        let d = check_commit(&l, &model, &harn).0;
        assert!(
            d.len() == 1 && d[0].message.contains("not a WP of the branch's role"),
            "{d:?}"
        );
        // A WP whose row names a family of roles: the branch's role is the one checked.
        let spec = commit("WP-73: gold", &[('A', "fixtures/lq/x.json")]);
        assert!(check_commit(&l, &spec, &on("R-FIX")).0.is_empty());
        assert_eq!(check_commit(&l, &spec, &on("R-SPEC-F")).0.len(), 1);
        let arch = commit(
            "WP-81a: fold back",
            &[('M', "docs/ARCHITECTURE-RESEARCH.md")],
        );
        assert!(check_commit(&l, &arch, &on("R-SPEC-F")).0.is_empty());
        // Outside branch mode an owner commit is skipped.
        assert_eq!(check_commit(&l, &plain, &PLAIN), (Vec::new(), false));
    }

    #[test]
    fn notes_limits_are_enforced() {
        let l = ledger();
        let arch = commit("WP-10: x", &[('M', "docs/ARCHITECTURE-RESEARCH.md")]);
        let d = check(&l, &arch);
        assert!(
            d.len() == 1 && d[0].message.contains("only in WP-81a, WP-99"),
            "{d:?}"
        );
        let ok = commit(
            "WP-99: exit",
            &[('M', "docs/research/design/60-roadmap.md")],
        );
        assert!(check(&l, &ok).is_empty());
        let rev = |role: &'static str, path: &str| {
            let c = commit("WP-80: pass 2", &[('A', path)]);
            check_commit(
                &l,
                &c,
                &Checks {
                    branch_role: Some(role),
                    ..PLAIN
                },
            )
            .0
        };
        assert!(rev("R-REV-P", "docs/spec/reviews/P-pass2.md").is_empty());
        assert!(rev("R-REV-S", "docs/spec/reviews/a1-s.md").is_empty());
        let d = rev("R-REV-P", "docs/spec/reviews/S-pass2.md");
        assert!(d.len() == 1 && d[0].message.contains("lens"), "{d:?}");
        assert_eq!(
            rev("R-REV-A", "docs/spec/reviews/pass2-closure.md").len(),
            1
        );
        assert_eq!(
            only_in("only in WP-81a and WP-99, with owner review"),
            vec!["WP-81a".to_string(), "WP-99".into()]
        );
        assert!(only_in("shared: each role adds its own targets").is_empty());
    }

    #[test]
    fn role_cells() {
        assert_eq!(
            roles_in("R-REV-P, R-REV-S, R-REV-A; owner (V2)"),
            vec!["R-REV-P", "R-REV-S", "R-REV-A"]
        );
        assert_eq!(roles_in("R-SPEC with the owner"), vec!["R-SPEC"]);
        assert!(roles_in("the gate worktree only").is_empty());
        assert!(role_matches("R-HARN-I", "R-HARN"));
        assert!(!role_matches("R-HARNESS", "R-HARN"));
        assert_eq!(expand_wp("WP-53a–e").len(), 5);
        assert!(named_with_lens("docs/spec/reviews/A-pass1.md", "A"));
        assert!(!named_with_lens("docs/spec/reviews/a1-s.md", "A"));
    }
}
