//! The fuzz workspace's own rules (docs/m0/tools.md §4.4, §4.5; docs/m0/PLAN.md §2.2 `fuzz/`), for the gate's
//! `fuzz` step:
//! - every libFuzzer target records its input before anything else: the body of each `fuzz_target!` closure starts
//!   with `moirai_fuzz::record(<input>)`, the closure's own parameter. Without it a target still finds panics, but on
//!   `x86_64-pc-windows-msvc` with the sanitizer off the crashing input is lost (tools.md §4.4, failure 2). A target
//!   file without any `fuzz_target!` is a finding too;
//! - the pinned nightly is read from `fuzz/rust-toolchain.toml`, the single place it is written (tools.md §2.2).

use crate::diag::Diag;
use crate::rustscan::{Tok, Token, lex};

/// Where cargo-fuzz puts the targets (`cargo fuzz add`); FL-1's work packages add theirs there (authors.md §3).
pub const TARGETS_DIR: &str = "fuzz/fuzz_targets/";

const RULE: &str = "must start with `moirai_fuzz::record(<input>);`, the closure's own input, so a crashing input is written as an artifact on MSVC (docs/m0/tools.md §4.4, §4.5)";

/// Whether a repository path is a target file of [`TARGETS_DIR`] (its direct `.rs` children).
pub fn is_target_file(path: &str) -> bool {
    path.strip_prefix(TARGETS_DIR)
        .is_some_and(|r| r.ends_with(".rs") && !r.contains('/'))
}

/// The index of the token that closes the delimiter at `open`.
fn closing(toks: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, t) in toks.iter().enumerate().skip(open) {
        match t.tok {
            Tok::Punct(b'(' | b'[' | b'{') => depth += 1,
            Tok::Punct(b')' | b']' | b'}') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Checks one `fuzz_target!` invocation whose opening delimiter is at `open`: the closure's parameter and the first
/// statement of its body.
fn check_invocation(toks: &[Token], open: usize) -> Result<(), String> {
    let end = closing(toks, open).ok_or("the fuzz_target! invocation is not closed")?;
    // The closure starts at the first `|` outside any nested group (after an optional `init: { .. },`).
    let mut k = open + 1;
    while k < end && toks[k].tok != Tok::Punct(b'|') {
        if matches!(toks[k].tok, Tok::Punct(b'(' | b'[' | b'{')) {
            k = closing(toks, k).ok_or("an unclosed group")?;
        }
        k += 1;
    }
    if k >= end {
        return Err(format!("no closure in fuzz_target!: the body {RULE}"));
    }
    k += 1;
    if toks.get(k).map(|t| &t.tok) == Some(&Tok::Ident("mut".into())) {
        k += 1;
    }
    let Some(Tok::Ident(input)) = toks.get(k).map(|t| &t.tok) else {
        return Err(format!(
            "the fuzz_target! closure takes the input as one named parameter; its body {RULE}"
        ));
    };
    // The parameter list ends at the next `|` outside a group (`&[u8]` holds brackets).
    k += 1;
    while k < end && toks[k].tok != Tok::Punct(b'|') {
        if matches!(toks[k].tok, Tok::Punct(b'(' | b'[' | b'{')) {
            k = closing(toks, k).ok_or("an unclosed group")?;
        }
        k += 1;
    }
    k += 1;
    if toks.get(k).map(|t| &t.tok) != Some(&Tok::Punct(b'{')) {
        return Err(format!(
            "the fuzz_target! closure's body is a block, and it {RULE}"
        ));
    }
    let want = [
        Tok::Ident("moirai_fuzz".into()),
        Tok::PathSep,
        Tok::Ident("record".into()),
        Tok::Punct(b'('),
        Tok::Ident(input.clone()),
        Tok::Punct(b')'),
    ];
    let got = toks.get(k + 1..k + 1 + want.len());
    if got.is_some_and(|g| g.iter().map(|t| &t.tok).eq(want.iter())) {
        Ok(())
    } else {
        Err(format!(
            "the body of fuzz_target!(|{input}: ..| {{ .. }}) {RULE}"
        ))
    }
}

/// The findings for one target file (`path` is repository-relative).
pub fn check_target(path: &str, src: &str) -> Vec<Diag> {
    let toks = lex(src);
    let mut out = Vec::new();
    let mut found = false;
    for k in 0..toks.len().saturating_sub(2) {
        if toks[k].tok == Tok::Ident("fuzz_target".into())
            && toks[k + 1].tok == Tok::Punct(b'!')
            && matches!(toks[k + 2].tok, Tok::Punct(b'(' | b'[' | b'{'))
        {
            found = true;
            if let Err(e) = check_invocation(&toks, k + 2) {
                out.push(Diag {
                    line: Some(toks[k].line),
                    ..Diag::path("fuzz", path, e)
                });
            }
        }
    }
    if !found {
        out.push(Diag::path(
            "fuzz",
            path,
            format!(
                "no fuzz_target! invocation: a file of {TARGETS_DIR} is a libFuzzer target, and its body {RULE}"
            ),
        ));
    }
    out
}

/// The `channel` of `fuzz/rust-toolchain.toml`'s `[toolchain]` table.
pub fn toolchain_channel(text: &str) -> Result<String, String> {
    let t = crate::toml::parse(text).map_err(|e| format!("fuzz/rust-toolchain.toml: {e}"))?;
    t.get("toolchain")
        .and_then(|v| v.get_path(&["channel"]))
        .and_then(|v| v.as_str())
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "fuzz/rust-toolchain.toml: no [toolchain] channel".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(src: &str) -> Vec<String> {
        check_target("fuzz/fuzz_targets/path_spec.rs", src)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn targets_record_their_input_first() {
        for ok in [
            "#![no_main]\nuse libfuzzer_sys::fuzz_target;\nfuzz_target!(|data: &[u8]| {\n    moirai_fuzz::record(data);\n    let _ = data.len();\n});\n",
            // A comment before the call, `mut`, an `init:` block and the path form of the macro.
            "libfuzzer_sys::fuzz_target!(init: { let _ = 1; }, |mut input: &[u8]| { /* first */ moirai_fuzz::record(input); input = &input[1..]; });",
            "fuzz_target! { |d: &[u8]| { moirai_fuzz::record(d); } }",
        ] {
            assert!(findings(ok).is_empty(), "{ok}: {:?}", findings(ok));
        }
        // The seeded violations: the call is missing, comes second, records something else, or the body is not a
        // block; and a file with no target at all.
        for (bad, what) in [
            (
                "fuzz_target!(|data: &[u8]| { let _ = moirai_files::text::is_text(data); });",
                "must start with `moirai_fuzz::record",
            ),
            (
                "fuzz_target!(|data: &[u8]| { let n = data.len(); moirai_fuzz::record(data); });",
                "must start with",
            ),
            (
                "fuzz_target!(|data: &[u8]| { moirai_fuzz::record(&data[1..]); });",
                "must start with",
            ),
            ("fuzz_target!(|data: &[u8]| run(data));", "body is a block"),
            ("fn main() {}", "no fuzz_target! invocation"),
        ] {
            let f = findings(bad);
            assert_eq!(f.len(), 1, "{bad}: {f:?}");
            assert!(
                f[0].contains(what) && f[0].contains("fuzz/fuzz_targets/path_spec.rs"),
                "{bad}: {f:?}"
            );
        }
        // The finding names the invocation's line.
        let f = check_target(
            "fuzz/fuzz_targets/x.rs",
            "#![no_main]\n\nfuzz_target!(|data: &[u8]| {});",
        );
        assert_eq!(f[0].line, Some(3));
    }

    #[test]
    fn target_files_and_the_channel() {
        assert!(is_target_file("fuzz/fuzz_targets/path_spec.rs"));
        assert!(!is_target_file("fuzz/fuzz_targets/common/mod.rs"));
        assert!(!is_target_file("fuzz/src/lib.rs"));
        assert!(!is_target_file("fuzz/fuzz_targets/notes.md"));
        assert_eq!(
            toolchain_channel(
                "[toolchain]\nchannel = \"nightly-2026-09-27\"\nprofile = \"minimal\"\n"
            )
            .unwrap(),
            "nightly-2026-09-27"
        );
        assert!(toolchain_channel("[toolchain]\nprofile = \"minimal\"\n").is_err());
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let text = std::fs::read_to_string(repo.join("fuzz/rust-toolchain.toml")).unwrap();
        assert!(toolchain_channel(&text).unwrap().starts_with("nightly-"));
    }
}
