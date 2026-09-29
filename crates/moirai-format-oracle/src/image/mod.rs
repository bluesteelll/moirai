//! [F14] the git image: the `.moi` v1 ABNF conformance check for every file of an image tree (§4, §6–§8, §14), the
//! commit objects and trailers (§10) and the gate-0 carrier check that re-derives the canonical items (§12).
//!
//! [`check_file`] dispatches on a file's magic line: it parses the importer's superset (§9.1), applies the `ImageParse`
//! rules of §9.2 that one file decides, and reports whether the bytes are the exporter's canonical encoding (§15 rule 3:
//! parse and re-encode reproduce them). [`tree::check_tree`] adds the rules a whole tree decides (§3.4, §4, §7.2.1).

pub mod commit;
pub mod files;
pub mod git;
pub mod lq;
pub mod node;
pub mod schema;
pub mod text;
pub mod tree;

use crate::prim::Result;
use schema::Schema;

/// What a conformance check found in one image file.
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed {
    /// `.moirai-image`.
    Marker(files::Marker),
    /// A node file.
    Node(Box<node::NodeFile>),
    /// A schema table file.
    Schema(Vec<files::SchemaRow>),
    /// A named-query file.
    Query(files::Query),
    /// A checkpoint ref row file.
    Refs(String, String),
    /// `meta.moi`.
    Meta(files::Meta),
    /// `aliases/<h1>.moi`.
    Aliases(Vec<([u8; 16], u64)>),
    /// `ops.moi`.
    Ops(Vec<String>),
}

/// The result of a conformance check.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    /// The parse.
    pub parsed: Parsed,
    /// The exporter's encoding of the parse ([F14 §15] rule 3).
    pub encoded: Vec<u8>,
    /// True when the input is byte for byte the exporter's encoding.
    pub canonical: bool,
}

/// Where a file sits in an image tree, as far as its name tells ([F14 §3.1]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Place<'a> {
    /// The uid of `nodes/<h1>/<h2>/<uid>.moi`.
    pub node_uid: Option<[u8; 16]>,
    /// The `q` of `schema/queries/<q>.moi`.
    pub query_q: Option<&'a str>,
    /// The byte of `aliases/<h1>.moi`.
    pub alias_h1: Option<u8>,
}

impl<'a> Place<'a> {
    /// Reads the parts of an image path ([F14 §3.1]); a path outside the layout gives no part.
    pub fn of_path(path: &'a str) -> Place<'a> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let stem = name.strip_suffix(".moi").unwrap_or("");
        let mut p = Place::default();
        if path.contains("queries/") && text::is_lhex(stem, 32) {
            p.query_q = Some(stem);
        } else if path.contains("aliases/") && text::is_lhex(stem, 2) {
            p.alias_h1 = u8::from_str_radix(stem, 16).ok();
        } else if text::is_lhex(stem, 32) {
            p.node_uid = crate::prim::unhex(stem).and_then(|b| b.try_into().ok());
        }
        p
    }
}

/// Checks one image file by its magic line ([F14 §9.2]: an unknown magic line or version is `ImageParse`).
pub fn check_file(bytes: &[u8], place: Place<'_>, schema: &Schema) -> Result<Checked> {
    let norm = text::import_normalise(bytes);
    let first = norm.split(|&c| c == b'\n').next().unwrap_or(&[]);
    let first = core::str::from_utf8(first)
        .unwrap_or("")
        .trim_end_matches([' ', '\t']);
    let (parsed, reencoded) = match first {
        "moirai-image 1" => {
            let m = files::parse_marker(bytes)?;
            let e = files::encode_marker(&m);
            (Parsed::Marker(m), e)
        }
        "moirai-node 1" => {
            let cx = node::Ctx {
                schema,
                path_uid: place.node_uid,
            };
            let n = node::parse(bytes, &cx)?;
            let e = node::encode(&n, schema);
            (Parsed::Node(Box::new(n)), e)
        }
        "moirai-schema 1" => {
            let r = files::parse_schema_file(bytes)?;
            let e = files::encode_schema_file(&r);
            (Parsed::Schema(r), e)
        }
        "moirai-query 1" => {
            let q = files::parse_query(bytes, place.query_q)?;
            let e = files::encode_query(&q);
            (Parsed::Query(q), e)
        }
        "moirai-refs 1" => {
            let (n, k) = files::parse_refs(bytes)?;
            let e = files::encode_refs(&n, &k);
            (Parsed::Refs(n, k), e)
        }
        "moirai-meta 1" => {
            let m = files::parse_meta(bytes)?;
            let e = files::encode_meta(&m);
            (Parsed::Meta(m), e)
        }
        "moirai-aliases 1" => {
            let a = files::parse_aliases(bytes, place.alias_h1)?;
            let e = files::encode_aliases(&a);
            (Parsed::Aliases(a), e)
        }
        "moirai-ops 1" => {
            let o = files::parse_ops(bytes)?;
            let mut e = b"moirai-ops 1\n".to_vec();
            for l in &o {
                e.extend_from_slice(l.as_bytes());
                e.push(b'\n');
            }
            (Parsed::Ops(o), e)
        }
        other => {
            return text::parse_err(
                text::Rule::MagicVersion,
                0,
                format!("an unknown magic line {other:?} [F14 §9.2]"),
            );
        }
    };
    Ok(Checked {
        canonical: reencoded == bytes,
        encoded: reencoded,
        parsed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dispatch by magic; an unknown magic is refused; the path's uid is checked.
    #[test]
    fn dispatch() {
        let s = Schema::core();
        let m = check_file(
            b"moirai-image 1\nobject-format: sha1\nschema-version: 1\n",
            Place::default(),
            &s,
        )
        .unwrap();
        assert!(m.canonical);
        assert!(check_file(b"moirai-thing 1\n", Place::default(), &s).is_err());
        let node = b"moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e40\nkind: task\ntitle: t\ndeleted: cca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7a\n";
        let good = Place::of_path("nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e40.moi");
        assert!(check_file(node, good, &s).unwrap().canonical);
        let other = Place::of_path("nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e41.moi");
        assert!(check_file(node, other, &s).is_err());
        let crlf = b"moirai-image 1\r\nobject-format: sha1\r\nschema-version: 1\r\n";
        assert!(!check_file(crlf, Place::default(), &s).unwrap().canonical);
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    const MAGICS: [&str; 8] = [
        "moirai-image 1",
        "moirai-node 1",
        "moirai-schema 1",
        "moirai-query 1",
        "moirai-refs 1",
        "moirai-meta 1",
        "moirai-aliases 1",
        "moirai-ops 1",
    ];

    fn line() -> impl Strategy<Value = String> {
        prop_oneof![
            "(uid|kind|title|status|deleted|field|label|edge|anchor|conflict|body|object-format|schema-version|q|name|def|ref|alias|op)(: | )[ -~]{0,24}",
            "[ -~\u{e9}\u{4e2d}]{0,24}",
            Just(String::new()),
            Just("<<".to_owned()),
            Just("---".to_owned()),
        ]
    }

    proptest! {
        /// [F14 §9.2]: the image checker refuses or accepts any file with every magic line and never panics; every
        /// refusal names its rule.
        #[test]
        fn check_file_never_panics(m in 0..MAGICS.len(), lines in proptest::collection::vec(line(), 0..10)) {
            let s = Schema::core();
            let mut b = format!("{}\n", MAGICS[m]);
            for l in &lines {
                b.push_str(l);
                b.push('\n');
            }
            if let Err(e) = check_file(b.as_bytes(), Place::default(), &s) {
                prop_assert!(e.rule.is_some(), "{}", e);
            }
        }
    }
}
