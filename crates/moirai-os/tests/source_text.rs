//! The crate's own source text: no file carries double-encoded UTF-8 (mojibake). The spec citations of the doc comments
//! (`[OS/path §4.1]`, `[OS/clock §2.1]`) must stay readable; a file once re-saved through a legacy code page turned `§`
//! into U+00C2 U+00A7 and `–` into U+00E2 U+20AC U+201C.

use std::path::Path;

/// U+00C2 (A with circumflex), the first character of every double-encoded U+0080–U+00BF (`§`, `¹`, …).
const LATIN_A_CIRCUMFLEX: char = '\u{C2}';
/// U+00E2 U+20AC (a with circumflex, euro sign), the start of every double-encoded U+2000–U+203F (`–`, `—`, `‖`, `…`, …).
const E2_EURO: &str = "\u{E2}\u{20AC}";

/// Records every line of `path` that holds a double-encoded sequence.
fn scan_file(path: &Path, bad: &mut Vec<String>, seen: &mut usize) {
    *seen += 1;
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} is not UTF-8: {e}", path.display()));
    for (n, line) in text.lines().enumerate() {
        if line.contains(LATIN_A_CIRCUMFLEX) || line.contains(E2_EURO) {
            bad.push(format!("{}:{}", path.display(), n + 1));
        }
    }
}

/// Scans every `.rs` file under `dir`.
fn scan_dir(dir: &Path, bad: &mut Vec<String>, seen: &mut usize) {
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            scan_dir(&path, bad, seen);
        } else if path.extension().is_some_and(|x| x == "rs") {
            scan_file(&path, bad, seen);
        }
    }
}

#[test]
fn no_source_file_is_double_encoded() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (mut bad, mut seen) = (Vec::new(), 0usize);
    for sub in ["src", "tests"] {
        scan_dir(&root.join(sub), &mut bad, &mut seen);
    }
    scan_file(&root.join("Cargo.toml"), &mut bad, &mut seen);
    assert!(seen >= 20, "the scan found the sources ({seen} files)");
    assert!(bad.is_empty(), "double-encoded UTF-8 at {bad:#?}");
}
