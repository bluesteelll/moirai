//! Shared helpers of the fixture walks: the fixture root, reading, the recursive file listing, and item 10 as a case
//! states it.

use std::path::{Path, PathBuf};

use moirai_format_oracle::canon;
use moirai_format_oracle::fixture::hex_block;
use moirai_format_oracle::prim::lp;

/// Item 10 as a case states it ([F07 §10.4]): the `digest-input` block holds `lp("moirai-changeset-v1") ‖ E_1 ‖ … ‖ E_n
/// ‖ u64(n)` with one comment line before each part (`fixtures/canonical/INDEX.md` §2.2, §4.1). The block is split at
/// its comment lines; it must hold the domain, `n` = `entry-count` non-empty entries and the count, and
/// [`canon::changeset_digest`] of the entries must be `digest`.
pub fn check_digest_parts(block: &[String], n: u64, digest: &[u8; 32]) -> Result<(), String> {
    let starts: Vec<usize> = (0..block.len())
        .filter(|&i| block[i].trim_start().starts_with(';'))
        .collect();
    if block[..starts.first().copied().unwrap_or(block.len())]
        .iter()
        .any(|l| !l.trim().is_empty())
    {
        return Err("digest-input holds bytes before its first comment line".into());
    }
    let mut parts = Vec::with_capacity(starts.len());
    for (k, &s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(block.len());
        parts.push(hex_block(&block[s + 1..end]).map_err(|e| format!("digest-input: {e}"))?);
    }
    let mut domain = Vec::new();
    lp(&mut domain, b"moirai-changeset-v1");
    let [first, entries @ .., last] = parts.as_slice() else {
        return Err("digest-input has fewer than two commented parts".into());
    };
    if *first != domain || *last != n.to_le_bytes() {
        return Err(
            "digest-input's first part is not lp(\"moirai-changeset-v1\") or its last is not u64(entry-count) [F07 §10.4]"
                .into(),
        );
    }
    if entries.len() as u64 != n || entries.iter().any(Vec::is_empty) {
        return Err(format!(
            "digest-input holds {} entry parts, not entry-count {n} non-empty ones",
            entries.len()
        ));
    }
    if canon::changeset_digest(entries) != *digest {
        return Err(
            "changeset_digest of digest-input's entries is not changeset-digest [F07 §10.4]".into(),
        );
    }
    Ok(())
}

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

/// Runs `check` over every item of `items` and panics with every failure. A `known` mismatch (a fixture whose conclusion
/// differs from the oracle's reading of the specification, reported as a spec finding) is run too, as an expected
/// failure: it fails the walk once it passes (the ruling has landed and the entry is stale), and so does a known entry
/// that names no item. Nothing is skipped, so no test of a walk is ignored.
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
    for (it, n) in items.iter().zip(&names) {
        match (check(it), known.contains(&n.as_str())) {
            (Err(e), false) => failures.push(format!("{n}: {e}")),
            (Ok(()), true) => failures.push(format!(
                "{n}: a known mismatch that now passes; remove it from KNOWN"
            )),
            _ => {}
        }
    }
    assert!(
        failures.is_empty(),
        "{family}: {} of {} fixture check(s) failed:\n{}",
        failures.len(),
        items.len(),
        failures.join("\n")
    );
    assert!(
        items.len() > known.len(),
        "{family}: no fixture was checked"
    );
}
