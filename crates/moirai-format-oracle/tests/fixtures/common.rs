//! Shared helpers of the fixture walks: the fixture root, reading, and the recursive file listing.

use std::path::{Path, PathBuf};

/// `fixtures/<family>`.
pub fn family(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// The bytes of `path`, or a panic naming it.
pub fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The UTF-8 text of `path`.
pub fn text(path: &Path) -> String {
    String::from_utf8(read(path)).unwrap_or_else(|e| panic!("{} is not UTF-8: {e}", path.display()))
}

/// `p` relative to `base`, with `/` separators.
pub fn rel(p: &Path, base: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every file under `dir`, recursively, sorted.
pub fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).unwrap_or_else(|e| panic!("list {}: {e}", d.display()));
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every blob of the tree rooted at `dir` as (path relative to `dir`, bytes).
pub fn blobs(dir: &Path) -> Vec<(String, Vec<u8>)> {
    walk(dir)
        .into_iter()
        .map(|p| (rel(&p, dir), read(&p)))
        .collect()
}

/// Runs `check` over `items`, skipping the `known` mismatches, and panics with every failure; also panics when a known
/// mismatch names no item (a stale entry).
pub fn run_all<T>(
    family: &str,
    items: &[T],
    name: impl Fn(&T) -> String,
    known: &[&str],
    check: impl Fn(&T) -> Result<(), String>,
) {
    let names: Vec<String> = items.iter().map(&name).collect();
    let mut failures: Vec<String> = known
        .iter()
        .filter(|k| !names.iter().any(|n| n == *k))
        .map(|k| format!("{k}: a known mismatch that names no fixture"))
        .collect();
    let mut ran = 0usize;
    for (it, n) in items.iter().zip(&names) {
        if known.contains(&n.as_str()) {
            continue;
        }
        ran += 1;
        if let Err(e) = check(it) {
            failures.push(format!("{n}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{family}: {} of {ran} fixture check(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(ran > 0, "{family}: no fixture was checked");
}
