//! `xtask coverage`: the rows of `docs/spec/COVERAGE.md` (docs/m0/PLAN.md §3.2 item 1; [F01 §2.7]).
//!
//! The file merges every chapter's Coverage rows into one table with the columns `item | chapter § | fixture |
//! model function` ([F01 §2.7]). For each row:
//! - every fixture the fixture column cites (backticked; relative to the repository root, or to `fixtures/`;
//!   `*` allowed) must exist;
//! - every model function the last column cites (backticked, e.g. `moirai_model::idem::lookup` or `canon::commit_id`,
//!   argument lists ignored) must be a `fn` in `crates/moirai-model/src/`, and the comments or attributes directly
//!   above it must carry a `spec:` tag that cites one of the row's chapter sections (`[F17 §11.1]`) or names the
//!   row's item.
//!
//! Blank cells (empty, `—`, `-`, `n/a`) are tolerated while the chapters are being written; the report counts them.
//! With `--strict` (`xtask coverage --strict`, `xtask gate --strict-coverage`), for WP-80's pass 2 and WP-81b's
//! freeze, every blank cell is a finding: E1's "COVERAGE.md has no unmapped row" (PLAN §7).
//! A missing `COVERAGE.md` is reported as a notice, not a failure, until WP-10 adds it.

use crate::diag::Diag;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Report {
    pub diags: Vec<Diag>,
    pub rows: usize,
    pub blank_fixture: usize,
    pub blank_model: usize,
    pub notice: Option<String>,
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    // A `\|` inside a cell is an escaped pipe.
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev = ' ';
    for ch in t.chars() {
        if ch == '|' && prev != '\\' {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
        prev = ch;
    }
    out.push(cur.trim().to_string());
    out
}

fn blank(cell: &str) -> bool {
    let c = cell.trim();
    c.is_empty() || matches!(c, "—" | "-" | "–" | "n/a" | "N/A" | "none")
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `[F17 §11.1]`-style citations in a cell.
fn citations(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cell;
    while let Some(o) = rest.find('[') {
        let Some(c) = rest[o..].find(']') else { break };
        let inner = &rest[o + 1..o + c];
        if inner.contains('§')
            || inner.starts_with('F')
            || inner.starts_with("OS/")
            || inner.starts_with("LQ/")
        {
            out.push(format!(
                "[{}]",
                inner.split_whitespace().collect::<Vec<_>>().join(" ")
            ));
        }
        rest = &rest[o + c + 1..];
    }
    out
}

/// A model source file: repository-relative path and text.
pub struct ModelFile {
    pub path: String,
    pub text: String,
}

/// Checks the table. `exists(path)` answers whether a repository-relative fixture path (possibly with `*`) exists;
/// `strict` makes every blank cell a finding.
pub fn check(
    coverage: &str,
    exists: &dyn Fn(&str) -> bool,
    model: &[ModelFile],
    strict: bool,
) -> Report {
    let mut r = Report::default();
    let mut cols: Option<(usize, usize, usize, usize)> = None;
    for (ln, line) in coverage.lines().enumerate() {
        let lineno = u32::try_from(ln + 1).unwrap_or(u32::MAX);
        if !line.trim_start().starts_with('|') {
            cols = None;
            continue;
        }
        let c = cells(line);
        if cols.is_none() {
            let find =
                |pred: &dyn Fn(&str) -> bool| c.iter().position(|h| pred(&h.to_ascii_lowercase()));
            let item = find(&|h| h == "item" || h.starts_with("item"));
            let chap = find(&|h| h.contains("chapter"));
            let fix = find(&|h| h.contains("fixture"));
            let model = find(&|h| h.contains("model"));
            if let (Some(a), Some(b), Some(f), Some(m)) = (item, chap, fix, model) {
                cols = Some((a, b, f, m));
            }
            continue;
        }
        if c.iter()
            .all(|x| x.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
        {
            continue;
        }
        let Some((ci, cc, cf, cm)) = cols else {
            continue;
        };
        let get = |i: usize| c.get(i).map(String::as_str).unwrap_or("");
        let (item, chapter, fixture, modelc) = (get(ci), get(cc), get(cf), get(cm));
        r.rows += 1;
        if blank(fixture) {
            r.blank_fixture += 1;
            if strict {
                r.diags.push(Diag::path(
                    "coverage",
                    "docs/spec/COVERAGE.md",
                    format!(
                        "line {lineno}, item {item}: no fixture (--strict: every row is mapped)"
                    ),
                ));
            }
        } else {
            for f in backticked(fixture) {
                let alt = format!("fixtures/{}", f.trim_start_matches("./"));
                if !exists(&f) && !exists(&alt) {
                    r.diags.push(Diag::path(
                        "coverage",
                        "docs/spec/COVERAGE.md",
                        format!("line {lineno}, item {item}: fixture `{f}` does not exist"),
                    ));
                }
            }
        }
        if blank(modelc) {
            r.blank_model += 1;
            if strict {
                r.diags.push(Diag::path(
                    "coverage",
                    "docs/spec/COVERAGE.md",
                    format!("line {lineno}, item {item}: no model function (--strict: every row is mapped)"),
                ));
            }
            continue;
        }
        let cites = citations(chapter);
        for f in backticked(modelc) {
            let path = f.split('(').next().unwrap_or(&f).trim();
            let segs: Vec<&str> = path
                .split("::")
                .filter(|s| !s.is_empty() && *s != "moirai_model" && *s != "crate")
                .collect();
            let Some((name, mods)) = segs.split_last() else {
                continue;
            };
            match find_fn(model, mods, name) {
                None => r.diags.push(Diag::path(
                    "coverage",
                    "docs/spec/COVERAGE.md",
                    format!("line {lineno}, item {item}: model function `{path}` not found in crates/moirai-model/src/"),
                )),
                Some((file, fnline, tags)) => {
                    let tagged = tags.iter().any(|t| {
                        let norm = t.split_whitespace().collect::<Vec<_>>().join(" ");
                        cites.iter().any(|c| norm.contains(c.as_str())) || (!item.is_empty() && norm.contains(item))
                    });
                    if !tagged {
                        r.diags.push(Diag {
                            line: Some(fnline),
                            ..Diag::path(
                                "coverage",
                                &file,
                                format!(
                                    "`{path}` (COVERAGE.md line {lineno}, item {item}) carries no `spec:` tag citing {}",
                                    if cites.is_empty() { format!("item {item}") } else { cites.join(" or ") }
                                ),
                            )
                        });
                    }
                }
            }
        }
    }
    r
}

/// Finds `fn name` in the model files, preferring a file whose path contains the module segments. Returns the file,
/// the line and the `spec:` tags in the comment and attribute block directly above it.
fn find_fn(model: &[ModelFile], mods: &[&str], name: &str) -> Option<(String, u32, Vec<String>)> {
    let mut best: Option<(usize, String, u32, Vec<String>)> = None;
    for f in model {
        let score = mods
            .iter()
            .filter(|m| {
                f.path
                    .split('/')
                    .any(|seg| seg.trim_end_matches(".rs") == **m)
            })
            .count();
        let lines: Vec<&str> = f.text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if !declares_fn(l, name) {
                continue;
            }
            let mut tags = Vec::new();
            let mut k = i;
            while k > 0 {
                let p = lines[k - 1].trim();
                if p.starts_with("//") || p.starts_with("#[") || p.starts_with("#![") {
                    if let Some(pos) = p.find("spec:") {
                        tags.push(p[pos + 5..].trim().to_string());
                    }
                    k -= 1;
                } else {
                    break;
                }
            }
            let line = u32::try_from(i + 1).unwrap_or(u32::MAX);
            if best.as_ref().is_none_or(|b| score > b.0) {
                best = Some((score, f.path.clone(), line, tags));
            }
        }
    }
    best.map(|(_, p, l, t)| (p, l, t))
}

fn declares_fn(line: &str, name: &str) -> bool {
    let mut rest = line;
    while let Some(i) = rest.find("fn ") {
        let before_ok = i == 0
            || !rest.as_bytes()[i - 1].is_ascii_alphanumeric() && rest.as_bytes()[i - 1] != b'_';
        let after = rest[i + 3..].trim_start();
        if before_ok && after.starts_with(name) {
            let next = after[name.len()..].chars().next();
            if matches!(next, Some('(' | '<' | ' ')) {
                return true;
            }
        }
        rest = &rest[i + 3..];
    }
    false
}

/// Runs the check on the repository.
pub fn run(repo: &Path, strict: bool) -> Result<Report, String> {
    let cov = repo.join("docs/spec/COVERAGE.md");
    if !cov.exists() {
        return Ok(Report {
            notice: Some(
                "docs/spec/COVERAGE.md does not exist yet (WP-10 adds it); nothing to check".into(),
            ),
            ..Report::default()
        });
    }
    let text = std::fs::read_to_string(&cov).map_err(|e| format!("{}: {e}", cov.display()))?;
    let mut model = Vec::new();
    let src = repo.join("crates/moirai-model/src");
    if src.is_dir() {
        let mut stack = vec![src];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
                let e = e.map_err(|e| e.to_string())?;
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let rel = p
                        .strip_prefix(repo)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/");
                    model.push(ModelFile {
                        path: rel,
                        text: std::fs::read_to_string(&p)
                            .map_err(|e| format!("{}: {e}", p.display()))?,
                    });
                }
            }
        }
    }
    let exists = |p: &str| {
        if p.contains('*') {
            glob_exists(repo, p)
        } else {
            repo.join(p).exists()
        }
    };
    Ok(check(&text, &exists, &model, strict))
}

/// Whether some path matches `pat`, whose components may each hold `*` (e.g. `hex/chain/*/HEAD.hex`).
fn glob_exists(repo: &Path, pat: &str) -> bool {
    let comps: Vec<&str> = pat.split('/').filter(|c| !c.is_empty()).collect();
    glob_walk(repo, &comps)
}

fn glob_walk(dir: &Path, comps: &[&str]) -> bool {
    let Some((first, rest)) = comps.split_first() else {
        return dir.exists();
    };
    if !first.contains('*') {
        return glob_walk(&dir.join(first), rest);
    }
    let pattern = crate::paths::Pattern::new(first);
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok).any(|e| {
                pattern.matches(&e.file_name().to_string_lossy()) && glob_walk(&e.path(), rest)
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# Coverage\n\n| item | chapter § | fixture | model function |\n|---|---|---|---|\n| R-7 | [F05 §4.2] | `hex/log/r7.hex` | `moirai_model::log::apply_r7` |\n| F14 | [F06 §3.1] | — | |\n| X-F3 | [F05 §2] | `hex/missing.hex` | `chain::check(a, b)` |\n| I-5 | [F13 §2] | `canonical/*.bin` | `untagged` |\n| G1 | [F16 §1] | | `nowhere` |\n";

    fn model() -> Vec<ModelFile> {
        vec![
            ModelFile {
                path: "crates/moirai-model/src/log.rs".into(),
                text:
                    "/// Applies R-7.\n// spec: [F05 §4.2]\n#[inline]\npub fn apply_r7(x: u8) {}\n"
                        .into(),
            },
            ModelFile {
                path: "crates/moirai-model/src/chain.rs".into(),
                text: "/// spec: [F05  §2] X-F3\npub(crate) fn check<T>(a: T, b: T) {}\n".into(),
            },
            ModelFile {
                path: "crates/moirai-model/src/misc.rs".into(),
                text: "// spec: [F99 §1]\n\nfn untagged() {}\n".into(),
            },
        ]
    }

    #[test]
    fn checks_rows() {
        let exists = |p: &str| p == "fixtures/hex/log/r7.hex" || p == "fixtures/canonical/*.bin";
        let r = check(TABLE, &exists, &model(), false);
        assert_eq!(r.rows, 5);
        assert_eq!(r.blank_fixture, 2);
        assert_eq!(r.blank_model, 1);
        let msgs: Vec<String> = r.diags.iter().map(|d| d.to_string()).collect();
        assert_eq!(r.diags.len(), 3, "{msgs:#?}");
        assert!(msgs[0].contains("hex/missing.hex"));
        assert!(msgs[1].contains("`untagged`") && msgs[1].contains("[F13 §2]"));
        assert!(msgs[2].contains("`nowhere` not found"));
        // --strict: each blank cell is a finding too.
        let s = check(TABLE, &exists, &model(), true);
        let blank: Vec<String> = s
            .diags
            .iter()
            .map(|d| d.to_string())
            .filter(|m| m.contains("--strict"))
            .collect();
        assert_eq!(blank.len(), 3, "{blank:#?}");
        assert!(blank.iter().any(|m| m.contains("item F14: no fixture")));
        assert!(
            blank
                .iter()
                .any(|m| m.contains("item F14: no model function"))
        );
        assert!(blank.iter().any(|m| m.contains("item G1: no fixture")));
    }

    #[test]
    fn globs_in_any_component() {
        let t = crate::testdir::TestDir::new("coverage-glob");
        t.write("fixtures/hex/chain/lazy-tail/HEAD.hex", "");
        t.write("fixtures/hex/chain/corrupt/log.1.hex", "");
        let root = t.path();
        assert!(glob_exists(root, "fixtures/hex/chain/*/HEAD.hex"));
        assert!(glob_exists(root, "fixtures/hex/chain/lazy-*/*.hex"));
        assert!(glob_exists(root, "fixtures/hex/chain/corrupt/*.hex"));
        assert!(!glob_exists(root, "fixtures/hex/chain/*/log.2.hex"));
        assert!(!glob_exists(root, "fixtures/hex/head/*/HEAD.hex"));
    }

    #[test]
    fn fn_declarations() {
        assert!(declares_fn("pub fn lookup(k: u8)", "lookup"));
        assert!(declares_fn("    pub(crate) async fn lookup<T>()", "lookup"));
        assert!(!declares_fn("fn lookups()", "lookup"));
        assert!(!declares_fn("// define fn lookup here", "lookups"));
    }
}
