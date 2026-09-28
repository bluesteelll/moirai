//! The JSON Lines records (crate documentation, "Output").
//!
//! Records are written straight to the output stream: fixed key order, no whitespace, strings escaped by
//! `serde_json`, one record per line. Nothing is built in memory besides the [`Scan`] itself.

use std::io::{self, Write};

use crate::scan::Scan;

/// The version of the record shape. It changes whenever a key, a value's meaning or an item rule changes. Format 2
/// added the items' `ok` key (rule 8) and the `/ *`, `r #` and `\0` spellings of [`crate::canon`] step 3.
pub const FORMAT: u32 = 2;

/// The `tree-sitter` crate version this oracle is built and pinned with (`Cargo.lock`; `docs/m0/tools.md`).
pub const TREE_SITTER: &str = "0.27.0";

/// The `tree-sitter-rust` grammar version this oracle is built and pinned with (`Cargo.lock`; `docs/m0/tools.md`).
pub const TREE_SITTER_RUST: &str = "0.24.2";

/// Writes the record of one source: `{"path":…,"errors":…,"items":[…]}` and a line feed (crate documentation,
/// "Output").
pub fn write_record<W: Write>(w: &mut W, path: &str, scan: &Scan) -> io::Result<()> {
    w.write_all(b"{\"path\":")?;
    write_str(w, path)?;
    write!(w, ",\"errors\":{},\"items\":[", scan.errors())?;
    for (i, item) in scan.items().enumerate() {
        if i > 0 {
            w.write_all(b",")?;
        }
        write!(w, "{{\"kind\":\"{}\",\"name\":", item.kind.name())?;
        write_str(w, item.name)?;
        w.write_all(b",\"qual\":")?;
        write_str(w, item.qual)?;
        write!(
            w,
            ",\"start\":{},\"end\":{},\"parent\":",
            item.start, item.end
        )?;
        match item.parent {
            Some(p) => write!(w, "{p}")?,
            None => w.write_all(b"null")?,
        }
        write!(w, ",\"ok\":{}}}", item.ok)?;
    }
    w.write_all(b"]}\n")
}

/// Writes the `--version` record: `{"oracle":"moirai-tsoracle","format":…,"tree_sitter":…,"tree_sitter_rust":…,
/// "language_abi":…}` and a line feed.
pub fn write_version<W: Write>(w: &mut W, language_abi: usize) -> io::Result<()> {
    writeln!(
        w,
        "{{\"oracle\":\"moirai-tsoracle\",\"format\":{FORMAT},\"tree_sitter\":\"{TREE_SITTER}\",\
         \"tree_sitter_rust\":\"{TREE_SITTER_RUST}\",\"language_abi\":{language_abi}}}"
    )
}

fn write_str<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    serde_json::to_writer(&mut *w, s).map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::{write_record, write_version};
    use crate::scan::{Oracle, Scan};

    fn record(src: &str, path: &str) -> String {
        let mut oracle = Oracle::new().expect("grammar loads");
        let mut scan = Scan::new();
        oracle.scan(src.as_bytes(), &mut scan).expect("parses");
        let mut out = Vec::new();
        write_record(&mut out, path, &scan).expect("writes to a Vec");
        String::from_utf8(out).expect("records are UTF-8")
    }

    #[test]
    fn exact_bytes_of_a_record() {
        let got = record(
            "mod a {\n    fn f() {}\n}\nimpl Display for S {}\n",
            "src/x.rs",
        );
        let want = concat!(
            r#"{"path":"src/x.rs","errors":0,"items":["#,
            r#"{"kind":"mod","name":"a","qual":"","start":1,"end":3,"parent":null,"ok":true},"#,
            r#"{"kind":"fn","name":"f","qual":"","start":2,"end":2,"parent":0,"ok":true},"#,
            r#"{"kind":"impl","name":"S","qual":"Display","start":4,"end":4,"parent":null,"ok":true}"#,
            "]}\n"
        );
        assert_eq!(got, want);
    }

    /// An item that a syntax error touches is written with `"ok":false`; its clean siblings keep `"ok":true`.
    #[test]
    fn items_touched_by_an_error_are_not_ok() {
        let got = record("fn a() {}\nfn b(x: Box<dyn 'a + Send>) {}\n", "e.rs");
        let want = concat!(
            r#"{"path":"e.rs","errors":1,"items":["#,
            r#"{"kind":"fn","name":"a","qual":"","start":1,"end":1,"parent":null,"ok":true},"#,
            r#"{"kind":"fn","name":"b","qual":"","start":2,"end":2,"parent":null,"ok":false}"#,
            "]}\n"
        );
        assert_eq!(got, want);
    }

    #[test]
    fn empty_source_has_no_items() {
        assert_eq!(
            record("", "e.rs"),
            "{\"path\":\"e.rs\",\"errors\":0,\"items\":[]}\n"
        );
    }

    #[test]
    fn strings_are_escaped_and_the_record_is_valid_json() {
        let got = record("impl Tr for extern \"C\" fn() {}\n", "dir\\quo\"te\u{1}.rs");
        assert!(got.ends_with('\n') && !got[..got.len() - 1].contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&got).expect("valid JSON");
        assert_eq!(v["path"], "dir\\quo\"te\u{1}.rs");
        assert_eq!(v["items"][0]["name"], "extern \"C\" fn()");
        assert_eq!(v["items"][0]["qual"], "Tr");
    }

    #[test]
    fn version_record_is_valid_json() {
        let mut out = Vec::new();
        write_version(&mut out, 14).expect("writes to a Vec");
        let v: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
        assert_eq!(v["oracle"], "moirai-tsoracle");
        assert_eq!(v["format"], 2);
        assert_eq!(v["tree_sitter"], super::TREE_SITTER);
        assert_eq!(v["tree_sitter_rust"], super::TREE_SITTER_RUST);
        assert_eq!(v["language_abi"], 14);
    }

    /// The pinned versions are the ones `Cargo.lock` resolves, so `--version` cannot drift from the build.
    #[test]
    fn pinned_versions_match_the_lockfile() {
        let lock_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
        let lock = std::fs::read_to_string(lock_path).expect("the workspace lockfile is readable");
        for (name, version) in [
            ("tree-sitter", super::TREE_SITTER),
            ("tree-sitter-rust", super::TREE_SITTER_RUST),
        ] {
            let header = format!("name = \"{name}\"\nversion = \"");
            let versions: Vec<&str> = lock
                .split("[[package]]\n")
                .filter_map(|p| p.strip_prefix(header.as_str()))
                .filter_map(|rest| rest.split('"').next())
                .collect();
            assert_eq!(
                versions,
                [version],
                "Cargo.lock resolves {name} to {versions:?}"
            );
        }
    }
}
