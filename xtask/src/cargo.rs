//! Running cargo for the gate: the poisoned C toolchain environment of [90 §11.1], merged output, and the
//! `--branch` filter that reduces what a role may not read to counts (docs/m0/PLAN.md §2.1, §3.1).
//!
//! The filter ([`LineFilter`], a pure function of cargo's merged output) withholds:
//! - a compiler message (JSON) when its package is hidden or any span names a hidden file: primary and secondary
//!   spans, macro-expansion and definition-site spans, and the spans of every child note, help or suggestion;
//! - a test binary's section when its package or crate-root file is hidden, the binary being attributed through the
//!   `executable` of cargo's `compiler-artifact` messages (so two crates with a `tests/props.rs` are told apart);
//! - in a visible section, each `test <name> ...` line, failure block (`---- <name> stdout ----`) and `failures:`
//!   entry of a test whose module path lies in a hidden file (`bug::tests::x` from `src/lib.rs` is in `src/bug.rs`
//!   or `src/bug/mod.rs`), and each doc-test of a hidden file;
//! - rustfmt's diff of a hidden file, and any other line that names a hidden path.
//!
//! Withheld failures and diagnostics are counted per crate as findings; withheld passing tests are counted apart.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

/// The four targets of GT20 (e) ([90 §11.1]).
pub const TARGETS: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "aarch64-apple-darwin",
];

pub const POISON: &str = "moirai-no-c-compiler";

/// The poisoned compiler variables: `CC_`, `CXX_` and `AR_<target>` for each target (in both the `-` and `_`
/// spellings, since the `cc` crate reads the `-` form first), `TARGET_*`, `HOST_CC`/`HOST_CXX` and the bare `CC`,
/// `CXX` and `AR`, so no build script can reach a C, C++ or assembler toolchain, whatever is installed.
pub fn poisoned_env() -> Vec<(String, String)> {
    let mut v = Vec::new();
    for t in TARGETS {
        for form in [t.to_string(), t.replace('-', "_")] {
            for tool in ["CC", "CXX", "AR"] {
                v.push((format!("{tool}_{form}"), POISON.to_string()));
            }
        }
    }
    for k in [
        "HOST_CC",
        "HOST_CXX",
        "HOST_AR",
        "TARGET_CC",
        "TARGET_CXX",
        "TARGET_AR",
        "CC",
        "CXX",
        "AR",
    ] {
        v.push((k.to_string(), POISON.to_string()));
    }
    v
}

/// The host triple of the installed `rustc` (`rustc -vV`).
pub fn host_triple() -> Option<String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(|h| h.trim().to_string()))
}

/// Decides what a `--branch` run may show.
pub trait Filter {
    /// Whether a repository-relative path is hidden.
    fn hides_path(&self, path: &str) -> bool;
    /// Whether a whole crate (by package name) is hidden.
    fn hides_crate(&self, name: &str) -> bool;
    /// Whether anything can be hidden at all.
    fn active(&self) -> bool {
        true
    }
    /// The crate a repository-relative path belongs to, for grouping withheld findings.
    fn crate_of(&self, _path: &str) -> Option<String> {
        None
    }
    /// The package whose library target has this name (`Doc-tests <lib>`).
    fn lib_owner(&self, _target: &str) -> Option<String> {
        None
    }
    /// The packages with a target of this name (`-` read as `_`), for a test binary cargo did not announce.
    fn target_owners(&self, _target: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Shows everything.
pub struct NoFilter;

impl Filter for NoFilter {
    fn hides_path(&self, _: &str) -> bool {
        false
    }
    fn hides_crate(&self, _: &str) -> bool {
        false
    }
    fn active(&self) -> bool {
        false
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub success: bool,
    /// Findings withheld per crate (failed tests, errors and warnings).
    pub hidden: BTreeMap<String, usize>,
    /// Passing test results withheld per crate (not findings).
    pub withheld: BTreeMap<String, usize>,
    pub errors: usize,
}

/// Maps cargo's package ids to package names, and absolute paths to repository-relative ones.
pub struct Names<'a> {
    pub ids: &'a dyn Fn(&str) -> Option<String>,
    pub repo: &'a str,
}

impl Names<'_> {
    pub fn rel(&self, p: &str) -> String {
        let p = p.replace('\\', "/");
        let p = p.strip_prefix("//?/").unwrap_or(&p).to_string();
        let lower = p.to_ascii_lowercase();
        let root = self.repo.to_ascii_lowercase();
        if lower.starts_with(&format!("{root}/")) {
            p[root.len() + 1..].to_string()
        } else {
            p
        }
    }
}

/// A test executable announced by a `compiler-artifact` message.
struct Exe {
    pkg: String,
    /// Repository-relative crate root file (`crates/x/src/lib.rs`, `crates/x/tests/props.rs`).
    root: String,
}

/// The test binary or doc-test run whose output is being read.
struct Section {
    pkg: String,
    /// The directory module paths resolve from (the crate root file's directory), when known.
    root_dir: Option<String>,
    /// Everything of the section is withheld.
    whole: bool,
    doc: bool,
    /// Names of the tests withheld so far (for the `failures:` list).
    hidden_names: HashSet<String>,
}

#[derive(PartialEq, Eq)]
enum Block {
    None,
    Shown,
    Hidden,
}

/// The pure part of [`run`]: reads cargo's merged output line by line and emits what the filter lets through.
pub struct LineFilter<'a> {
    filter: &'a dyn Filter,
    names: &'a Names<'a>,
    json: bool,
    exes: HashMap<String, Exe>,
    section: Option<Section>,
    in_hidden_fmt: bool,
    block: Block,
    pub out: Outcome,
}

/// Every file a compiler message names: its spans, their expansion and definition-site spans, and the same for
/// every child message, recursively.
fn message_files(msg: &serde_json::Value, out: &mut Vec<String>) {
    fn span_files(span: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(f) = span.get("file_name").and_then(|f| f.as_str()) {
            out.push(f.to_string());
        }
        if let Some(e) = span.get("expansion").filter(|e| !e.is_null()) {
            for k in ["span", "def_site_span"] {
                if let Some(s) = e.get(k).filter(|s| !s.is_null()) {
                    span_files(s, out);
                }
            }
        }
    }
    if let Some(spans) = msg.get("spans").and_then(|s| s.as_array()) {
        for s in spans {
            span_files(s, out);
        }
    }
    if let Some(children) = msg.get("children").and_then(|c| c.as_array()) {
        for c in children {
            message_files(c, out);
        }
    }
}

/// `path:12:5` → `path`.
fn strip_line_col(w: &str) -> &str {
    let mut w = w;
    loop {
        match w.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|c| c.is_ascii_digit()) => {
                w = head;
            }
            _ => return w,
        }
    }
}

/// The test name of a libtest result line (`test a::b ... ok`, `test a::b has been running for over 60 seconds`).
fn test_line(t: &str) -> Option<(&str, bool)> {
    let rest = t.strip_prefix("test ")?;
    if rest.starts_with("result:") {
        return None;
    }
    if let Some((name, res)) = rest.split_once(" ... ") {
        return Some((name, res.trim_start().starts_with("FAILED")));
    }
    rest.split_once(" has been running for ")
        .map(|(name, _)| (name, false))
}

impl<'a> LineFilter<'a> {
    pub fn new(filter: &'a dyn Filter, names: &'a Names<'a>, json: bool) -> LineFilter<'a> {
        LineFilter {
            filter,
            names,
            json,
            exes: HashMap::new(),
            section: None,
            in_hidden_fmt: false,
            block: Block::None,
            out: Outcome::default(),
        }
    }

    fn count(map: &mut BTreeMap<String, usize>, key: &str) {
        *map.entry(key.to_string()).or_default() += 1;
    }

    fn mentions_hidden(&self, line: &str) -> Option<String> {
        line.split([' ', '(', ')', '\'', '"', '`']).find_map(|w| {
            let w = w.trim_end_matches([':', ',', ';']);
            if !(w.contains('/') || w.contains('\\')) {
                return None;
            }
            let rel = self.names.rel(strip_line_col(w));
            self.filter.hides_path(&rel).then_some(rel)
        })
    }

    fn test_hidden(&self, s: &Section, name: &str) -> bool {
        if s.whole {
            return true;
        }
        if s.doc {
            let path = name.split(" - ").next().unwrap_or(name).trim();
            return self.filter.hides_path(&self.names.rel(path));
        }
        let Some(dir) = &s.root_dir else {
            return false;
        };
        let name = name.split(" - ").next().unwrap_or(name);
        let segs: Vec<&str> = name.split("::").collect();
        let mut p = dir.clone();
        for m in &segs[..segs.len().saturating_sub(1)] {
            p = if p.is_empty() {
                (*m).to_string()
            } else {
                format!("{p}/{m}")
            };
            if self.filter.hides_path(&format!("{p}.rs"))
                || self.filter.hides_path(&format!("{p}/mod.rs"))
            {
                return true;
            }
        }
        false
    }

    fn json_line(&mut self, v: &serde_json::Value, emit: &mut dyn FnMut(&str)) {
        match v.get("reason").and_then(|r| r.as_str()) {
            Some("compiler-artifact") => {
                if let (Some(exe), Some(src)) = (
                    v.get("executable").and_then(|e| e.as_str()),
                    v.pointer("/target/src_path").and_then(|s| s.as_str()),
                ) {
                    let pkg = v
                        .get("package_id")
                        .and_then(|p| p.as_str())
                        .and_then(|id| (self.names.ids)(id))
                        .unwrap_or_default();
                    let base = exe.rsplit(['/', '\\']).next().unwrap_or(exe);
                    self.exes.insert(
                        base.to_ascii_lowercase(),
                        Exe {
                            pkg,
                            root: self.names.rel(src),
                        },
                    );
                }
            }
            Some("compiler-message") => {
                let msg = &v["message"];
                let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("");
                let pkg = v
                    .get("package_id")
                    .and_then(|p| p.as_str())
                    .and_then(|id| (self.names.ids)(id))
                    .unwrap_or_default();
                let mut files = Vec::new();
                message_files(msg, &mut files);
                let hidden_file = files
                    .iter()
                    .map(|f| self.names.rel(f))
                    .find(|f| self.filter.hides_path(f));
                if self.filter.hides_crate(&pkg) || hidden_file.is_some() {
                    if matches!(level, "error" | "warning") {
                        let key = if pkg.is_empty() {
                            hidden_file
                                .as_deref()
                                .and_then(|f| self.filter.crate_of(f))
                                .unwrap_or_else(|| "(unknown crate)".into())
                        } else {
                            pkg
                        };
                        Self::count(&mut self.out.hidden, &key);
                    }
                } else {
                    if level == "error" {
                        self.out.errors += 1;
                    }
                    if let Some(rendered) = msg.get("rendered").and_then(|r| r.as_str()) {
                        emit(rendered);
                    }
                }
            }
            _ => {}
        }
    }

    fn start_section(&mut self, running: &str) {
        let exe = running
            .rsplit_once('(')
            .map(|(_, e)| e.trim_end().trim_end_matches(')'))
            .unwrap_or(running);
        let base = exe.rsplit(['/', '\\']).next().unwrap_or(exe);
        let s = match self.exes.get(&base.to_ascii_lowercase()) {
            Some(e) => Section {
                whole: self.filter.hides_crate(&e.pkg) || self.filter.hides_path(&e.root),
                root_dir: Some(
                    e.root
                        .rsplit_once('/')
                        .map(|(d, _)| d.to_string())
                        .unwrap_or_default(),
                ),
                pkg: e.pkg.clone(),
                doc: false,
                hidden_names: HashSet::new(),
            },
            None => {
                // Not announced by cargo: attributed by target name only when that is unambiguous, and withheld
                // whole whenever something may be hidden, since its module paths cannot be placed.
                let stem = base.split('.').next().unwrap_or(base);
                let target = stem.rsplit_once('-').map_or(stem, |(t, _)| t);
                let owners = self.filter.target_owners(target);
                let pkg = match owners.as_slice() {
                    [one] => one.clone(),
                    _ => format!("(unattributed test binary {target})"),
                };
                Section {
                    whole: self.filter.active(),
                    root_dir: None,
                    pkg,
                    doc: false,
                    hidden_names: HashSet::new(),
                }
            }
        };
        self.section = Some(s);
        self.block = Block::None;
    }

    /// Feeds one line of cargo's merged output (without its line end); `emit` receives what may be shown.
    pub fn line(&mut self, line: &str, emit: &mut dyn FnMut(&str)) {
        // Cargo's JSON messages: one object per line with a "reason" (cargo writes it first; any order is read).
        if self.json && line.starts_with("{\"") {
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) if v.get("reason").is_some_and(|r| r.is_string()) => {
                    self.json_line(&v, emit);
                    return;
                }
                Ok(_) => {}
                Err(_) if line.starts_with("{\"reason\"") => {
                    if self.filter.active() {
                        Self::count(&mut self.out.withheld, "(unparsed cargo message)");
                    } else {
                        emit(&format!("{line}\n"));
                    }
                    return;
                }
                Err(_) => {}
            }
        }
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("Running ") {
            self.start_section(rest);
        } else if let Some(c) = t.strip_prefix("Doc-tests ") {
            let c = c.trim();
            let pkg = self
                .filter
                .lib_owner(c)
                .unwrap_or_else(|| c.replace('_', "-"));
            self.section = Some(Section {
                whole: self.filter.hides_crate(&pkg),
                root_dir: None,
                pkg,
                doc: true,
                hidden_names: HashSet::new(),
            });
            self.block = Block::None;
        }
        // rustfmt: `Diff in <path>...`.
        if let Some(rest) = t.strip_prefix("Diff in ") {
            let rel = self.names.rel(fmt_diff_path(rest));
            self.in_hidden_fmt = self.filter.hides_path(&rel);
            if self.in_hidden_fmt {
                let key = self.filter.crate_of(&rel).unwrap_or(rel);
                Self::count(&mut self.out.hidden, &key);
                return;
            }
        }
        if self.in_hidden_fmt {
            return;
        }
        if let Some(sec) = self.section.take() {
            let keep = self.section_line(sec, line, t);
            if !keep {
                return;
            }
        }
        if let Some(rel) = self.mentions_hidden(line) {
            let key = self
                .section
                .as_ref()
                .map(|s| s.pkg.clone())
                .or_else(|| self.filter.crate_of(&rel))
                .unwrap_or(rel);
            if t.starts_with("error") || t.contains("panicked at") || t.contains("FAILED") {
                Self::count(&mut self.out.hidden, &key);
            } else {
                Self::count(&mut self.out.withheld, &key);
            }
            return;
        }
        emit(&format!("{line}\n"));
    }

    /// Handles a line inside a test section; returns whether it goes on to the general rules.
    fn section_line(&mut self, mut sec: Section, line: &str, t: &str) -> bool {
        let mut keep = true;
        if let Some((name, failed)) = test_line(t) {
            self.block = Block::None;
            if self.test_hidden(&sec, name) {
                sec.hidden_names
                    .insert(name.split(" - ").next().unwrap_or(name).to_string());
                if failed {
                    Self::count(&mut self.out.hidden, &sec.pkg);
                } else if t.contains(" ... ") {
                    Self::count(&mut self.out.withheld, &sec.pkg);
                }
                keep = false;
            }
        } else if let Some(h) = t.strip_prefix("---- ") {
            let name = h
                .rsplit_once(" std")
                .map_or(h, |(n, _)| n)
                .trim_end_matches(" ----");
            self.block = if self.test_hidden(&sec, name) {
                sec.hidden_names
                    .insert(name.split(" - ").next().unwrap_or(name).to_string());
                Block::Hidden
            } else {
                Block::Shown
            };
            keep = self.block == Block::Shown;
        } else if t == "failures:" || t == "successes:" || t.starts_with("test result:") {
            self.block = Block::None;
            keep = !sec.whole;
        } else if self.block == Block::Hidden || sec.whole || sec.hidden_names.contains(line.trim())
        {
            keep = false;
        }
        self.section = Some(sec);
        keep
    }
}

/// Runs cargo with `args` in `repo` under `env`; with `json`, adds `--message-format=json` and renders the
/// diagnostics itself. Output is printed unless the filter hides it.
pub fn run(
    repo: &Path,
    args: &[String],
    env: &[(String, String)],
    json: bool,
    filter: &dyn Filter,
    names: &Names<'_>,
) -> Result<Outcome, String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(repo);
    let mut full: Vec<String> = Vec::new();
    // `--message-format` goes before a `--` separator.
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    full.extend(args[..split].iter().cloned());
    if json {
        full.push("--message-format=json".into());
    }
    full.extend(args[split..].iter().cloned());
    cmd.args(&full);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.env("CARGO_TERM_COLOR", "never");
    let (reader, writer) = std::io::pipe().map_err(|e| format!("pipe: {e}"))?;
    let w2 = writer.try_clone().map_err(|e| format!("pipe: {e}"))?;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(w2));
    println!("$ cargo {}", full.join(" "));
    let mut child = cmd.spawn().map_err(|e| format!("cargo: {e}"))?;
    // The command holds the pipe's write ends; drop them so the reader sees EOF when cargo exits.
    drop(cmd);
    let mut lf = LineFilter::new(filter, names, json);
    let mut emit = |s: &str| print!("{s}");
    let r = BufReader::new(reader);
    for line in r.split(b'\n') {
        let line = line.map_err(|e| e.to_string())?;
        let line = String::from_utf8_lossy(&line);
        lf.line(line.trim_end_matches('\r'), &mut emit);
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let mut out = lf.out;
    out.success = status.success();
    Ok(out)
}

/// The path of a rustfmt `Diff in <path>:<line>:` or `Diff in <path> at line <n>:` header.
fn fmt_diff_path(rest: &str) -> &str {
    let r = rest.trim_end();
    if let Some((p, _)) = r.split_once(" at line ") {
        return p;
    }
    let r = r.strip_suffix(':').unwrap_or(r);
    let r = r.trim_end_matches(|c: char| c.is_ascii_digit());
    r.strip_suffix(':').unwrap_or(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Pattern;

    #[test]
    fn rustfmt_headers() {
        assert_eq!(
            fmt_diff_path(r"\\?\D:\m\src\lib.rs:487:"),
            r"\\?\D:\m\src\lib.rs"
        );
        assert_eq!(fmt_diff_path("/m/src/lib.rs at line 3:"), "/m/src/lib.rs");
        assert_eq!(fmt_diff_path("D:/m/src/lib.rs:12:"), "D:/m/src/lib.rs");
    }

    #[test]
    fn poison_covers_every_target_and_the_host() {
        let env = poisoned_env();
        let has = |k: &str| env.iter().any(|(a, b)| a == k && b == POISON);
        for k in [
            "CC_x86_64_unknown_linux_musl",
            "CXX_aarch64-apple-darwin",
            "AR_aarch64_unknown_linux_musl",
            "CC_x86_64_pc_windows_msvc",
            "HOST_CC",
            "HOST_CXX",
        ] {
            assert!(has(k), "{k}");
        }
    }

    #[test]
    fn relative_paths() {
        let ids = |_: &str| None;
        let n = Names {
            ids: &ids,
            repo: "D:/claude/moirai",
        };
        assert_eq!(
            n.rel(r"\\?\D:\claude\moirai\crates\x\src\lib.rs"),
            "crates/x/src/lib.rs"
        );
        assert_eq!(
            n.rel("d:/claude/moirai/xtask/src/main.rs"),
            "xtask/src/main.rs"
        );
        assert_eq!(n.rel("crates/x/src/lib.rs"), "crates/x/src/lib.rs");
        assert_eq!(strip_line_col(r"D:\m\src\bug.rs:7:41"), r"D:\m\src\bug.rs");
        assert_eq!(strip_line_col("src/a.rs"), "src/a.rs");
    }

    /// The R-HARN view of a workspace with a partly hidden `toy` crate (its `src/{bug,bugs}` module) and a wholly
    /// hidden `model` crate; `vis` is visible, and both `toy` and `vis` have a `tests/props.rs`.
    struct Harn {
        hidden: Vec<Pattern>,
    }

    impl Filter for Harn {
        fn hides_path(&self, p: &str) -> bool {
            self.hidden.iter().any(|h| h.matches(p))
        }
        fn hides_crate(&self, n: &str) -> bool {
            n == "model"
        }
        fn crate_of(&self, p: &str) -> Option<String> {
            p.strip_prefix("crates/")
                .and_then(|r| r.split('/').next())
                .map(str::to_string)
        }
        fn lib_owner(&self, t: &str) -> Option<String> {
            Some(t.to_string())
        }
        fn target_owners(&self, t: &str) -> Vec<String> {
            match t {
                "props" => vec!["toy".into(), "vis".into()],
                o => vec![o.to_string()],
            }
        }
    }

    fn artifact(pkg: &str, src: &str, exe: &str) -> String {
        serde_json::json!({
            "reason": "compiler-artifact",
            "package_id": format!("path+file:///D:/m/crates/{pkg}#0.0.0"),
            "target": {"kind": ["lib"], "name": pkg, "src_path": format!("D:\\m\\crates\\{pkg}\\{src}")},
            "profile": {"test": true},
            "executable": format!("D:/moirai-target/laneA\\debug\\deps\\{exe}"),
        })
        .to_string()
    }

    fn message(pkg: &str, level: &str, primary: &str, child: Option<&str>, text: &str) -> String {
        let span = |f: &str, primary: bool| serde_json::json!({"file_name": f, "is_primary": primary, "line_start": 3, "expansion": null});
        let children: Vec<serde_json::Value> = child
            .map(|c| vec![serde_json::json!({"level": "note", "message": "defined here", "spans": [span(c, true)], "children": []})])
            .unwrap_or_default();
        serde_json::json!({
            "reason": "compiler-message",
            "package_id": format!("path+file:///D:/m/crates/{pkg}#0.0.0"),
            "message": {"level": level, "message": text, "spans": [span(primary, true)], "children": children, "rendered": format!("{level}: {text}\n")},
        })
        .to_string()
    }

    /// A `cargo test --message-format=json` transcript in the shape cargo 1.98 and libtest print (stdout and stderr
    /// merged), recorded from a scratch workspace and renamed to synthetic paths.
    fn transcript() -> Vec<String> {
        let mut v = vec![
            artifact("toy", "src\\lib.rs", "toy-9c8e2a43a960b21b.exe"),
            artifact("toy", "tests\\props.rs", "props-64594897a342d838.exe"),
            artifact("vis", "src\\lib.rs", "vis-b39563073d012d70.exe"),
            artifact("vis", "tests\\props.rs", "props-9ff1ec98cbf9f25e.exe"),
            artifact("model", "src\\lib.rs", "model-0123456789abcdef.exe"),
            message(
                "toy",
                "warning",
                "crates/toy/src/log.rs",
                Some("crates/toy/src/bug.rs"),
                "SECRET-A deprecated item from the bug module",
            ),
            message(
                "toy",
                "warning",
                "crates/toy/src/log.rs",
                None,
                "visible warning in log",
            ),
            message(
                "vis",
                "error",
                "crates/vis/src/lib.rs",
                Some("D:\\m\\crates\\toy\\src\\bugs\\two.rs"),
                "SECRET-B mismatched types",
            ),
            message(
                "model",
                "warning",
                "crates/model/src/lib.rs",
                None,
                "SECRET-C model warning",
            ),
        ];
        for l in [
            "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.70s",
            "     Running unittests src\\lib.rs (D:\\moirai-target\\laneA\\debug\\deps\\toy-9c8e2a43a960b21b.exe)",
            "",
            "running 5 tests",
            "test log::tests::log_ok ... ok",
            "test bug::tests::torn_write_detected ... ok",
            "test bugs::two::tests::SECRET_D_name ... ok",
            "test tests::root_ok ... ok",
            "test bug::tests::lost_rename_detected ... FAILED",
            "",
            "failures:",
            "",
            "---- bug::tests::lost_rename_detected stdout ----",
            "",
            "thread 'bug::tests::lost_rename_detected' (11008) panicked at crates\\toy\\src\\bug.rs:7:41:",
            "SECRET-E failure text",
            "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
            "",
            "",
            "failures:",
            "    bug::tests::lost_rename_detected",
            "",
            "test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
            "",
            "error: test failed, to rerun pass `-p toy --lib`",
            "     Running tests\\props.rs (D:\\moirai-target\\laneA\\debug\\deps\\props-64594897a342d838.exe)",
            "",
            "running 1 test",
            "test toy_prop ... ok",
            "",
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
            "",
            "     Running unittests src\\lib.rs (D:\\moirai-target\\laneA\\debug\\deps\\model-0123456789abcdef.exe)",
            "",
            "running 1 test",
            "test SECRET_F::model_test ... FAILED",
            "---- SECRET_F::model_test stdout ----",
            "SECRET-G model failure",
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
            "     Running tests\\props.rs (D:\\moirai-target\\laneA\\debug\\deps\\props-9ff1ec98cbf9f25e.exe)",
            "",
            "running 1 test",
            "test vis_prop ... ok",
            "",
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
            "",
            "   Doc-tests toy",
            "",
            "running 2 tests",
            "test crates\\toy\\src\\bug.rs - bug::SECRET_H (line 3) ... ok",
            "test crates\\toy\\src\\lib.rs - visible (line 9) ... ok",
            "",
            "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
            "     Running unittests src\\main.rs (D:\\moirai-target\\laneA\\debug\\deps\\props-ffffffffffffffff.exe)",
            "test SECRET_I ... ok",
        ] {
            v.push(l.to_string());
        }
        v
    }

    #[test]
    fn the_branch_filter_withholds_hidden_modules_spans_and_binaries() {
        let f = Harn {
            hidden: vec![Pattern::new("crates/toy/src/{bug,bugs}")],
        };
        let ids = |id: &str| {
            id.rsplit_once('#')
                .and_then(|(p, _)| p.rsplit('/').next())
                .map(str::to_string)
        };
        let names = Names {
            ids: &ids,
            repo: "D:/m",
        };
        let mut lf = LineFilter::new(&f, &names, true);
        let mut shown = String::new();
        for l in transcript() {
            lf.line(&l, &mut |s| shown.push_str(s));
        }
        assert!(!shown.contains("SECRET"), "{shown}");
        for visible in [
            "visible warning in log",
            "test log::tests::log_ok ... ok",
            "test tests::root_ok ... ok",
            "test toy_prop ... ok",
            "test vis_prop ... ok",
            "test result: FAILED. 4 passed; 1 failed",
            "error: test failed, to rerun pass `-p toy --lib`",
            "lib.rs - visible (line 9) ... ok",
        ] {
            assert!(shown.contains(visible), "missing {visible:?} in\n{shown}");
        }
        assert!(!shown.contains("lost_rename"), "{shown}");
        assert!(!shown.contains("torn_write"), "{shown}");
        let o = lf.out;
        // Findings: the bug-module warning (child span) and the vis error (child span in bugs/), the failing
        // bug test, the model warning and the model test failure.
        assert_eq!(o.hidden.get("toy"), Some(&2), "{o:?}");
        assert_eq!(o.hidden.get("vis"), Some(&1), "{o:?}");
        assert_eq!(o.hidden.get("model"), Some(&2), "{o:?}");
        assert_eq!(o.withheld.get("toy"), Some(&3), "{o:?}");
        assert_eq!(
            o.withheld
                .get("(unattributed test binary props)")
                .copied()
                .unwrap_or(0)
                + o.hidden
                    .get("(unattributed test binary props)")
                    .copied()
                    .unwrap_or(0),
            1,
            "{o:?}"
        );
        assert_eq!(o.errors, 0);
        // Without a filter everything is shown.
        let mut all = LineFilter::new(&NoFilter, &names, true);
        let mut shown = String::new();
        for l in transcript() {
            all.line(&l, &mut |s| shown.push_str(s));
        }
        assert!(shown.contains("SECRET-E") && shown.contains("SECRET-C"));
        assert_eq!(all.out.errors, 1);
    }
}
