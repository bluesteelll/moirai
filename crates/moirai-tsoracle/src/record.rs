//! The JSON Lines records (crate documentation, "Output").
//!
//! Records are written straight to the output stream: fixed key order, no whitespace, strings escaped by
//! `serde_json`, one record per line. Nothing is built in memory besides the [`Scan`] itself.

use std::io::{self, Write};

use crate::scan::Scan;

/// The version of the source records' shape. It changes whenever a key of a source record, a value's meaning or an
/// item rule changes. Format 2 added the items' `ok` key (rule 8) and the `/ *`, `r #` and `\0` spellings of
/// [`crate::canon`] step 3; format 3 widened rule 8 to the errors that leave brackets unbalanced, which take every
/// later item out of the claim. The `--version` record's keys are read by name, so adding one to it (as
/// `manifest_dir` and `bin_manifest_dir` were) leaves the format as it is.
pub const FORMAT: u32 = 3;

/// The `CARGO_MANIFEST_DIR` the library was compiled with: the `moirai-tsoracle` crate directory of the work tree that
/// compiled it (crate documentation, "Output": `manifest_dir`). Cargo does not track it: it fingerprints a workspace
/// member by paths relative to the workspace root, so work trees that share a target directory share this library's
/// compilation, and only this constant tells which tree it came from.
pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

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
/// "language_abi":…,"manifest_dir":…,"bin_manifest_dir":…}` and a line feed (crate documentation, "Output").
/// `manifest_dir` is [`MANIFEST_DIR`]; `bin_manifest_dir` is the `CARGO_MANIFEST_DIR` the binary's own compilation
/// unit (`main.rs`) was compiled with, which the binary passes, because cargo may compile that unit in another work
/// tree than the library it links.
pub fn write_version<W: Write>(
    w: &mut W,
    language_abi: usize,
    bin_manifest_dir: &str,
) -> io::Result<()> {
    write!(
        w,
        "{{\"oracle\":\"moirai-tsoracle\",\"format\":{FORMAT},\"tree_sitter\":\"{TREE_SITTER}\",\
         \"tree_sitter_rust\":\"{TREE_SITTER_RUST}\",\"language_abi\":{language_abi},\"manifest_dir\":"
    )?;
    write_str(w, MANIFEST_DIR)?;
    w.write_all(b",\"bin_manifest_dir\":")?;
    write_str(w, bin_manifest_dir)?;
    w.write_all(b"}\n")
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
        let bin = r#"C:\work "tree"\crates\moirai-tsoracle"#;
        let mut out = Vec::new();
        write_version(&mut out, 14, bin).expect("writes to a Vec");
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.ends_with('\n') && !text[..text.len() - 1].contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(v["oracle"], "moirai-tsoracle");
        assert_eq!(v["format"], 3);
        assert_eq!(v["tree_sitter"], super::TREE_SITTER);
        assert_eq!(v["tree_sitter_rust"], super::TREE_SITTER_RUST);
        assert_eq!(v["language_abi"], 14);
        assert_eq!(v["manifest_dir"], env!("CARGO_MANIFEST_DIR"));
        assert_eq!(v["manifest_dir"], super::MANIFEST_DIR);
        assert_eq!(v["bin_manifest_dir"], bin);
        let keys: Vec<&str> = v
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.len(), 7, "{keys:?}");
        // The documented key order.
        let at = |k: &str| text.find(&format!("\"{k}\":")).expect(k);
        assert!(
            at("language_abi") < at("manifest_dir") && at("manifest_dir") < at("bin_manifest_dir")
        );
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
