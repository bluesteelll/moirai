//! [F14 §3], §4, §11.1: an image tree as a whole. The checks of §9.2 that a single file cannot decide: the marker (§4),
//! the layout under `nodes/`, `schema/` and `refs/` (§3.1, §3.4), a node file's uid against its file name, a query
//! file's name against its hash (§7.2.1); and the effective schema a tree's node files are parsed under (§11.1 step 1).
//! Root entries outside the layout are ignored (§3.4).

use super::files::{self, Marker, SchemaRow};
use super::schema::Schema;
use super::text::{Rule, is_lhex, parse_err};
use super::{Checked, Parsed, Place, check_file};
use crate::prim::{Algo, Result};

/// The four schema table files in the order their rows extend the schema, with the rows each holds ([F14 §3.1], §7.1;
/// `schema/policy.moi` since spec sync 2b).
const TABLES: [(&str, &[&str]); 4] = [
    ("schema/kinds.moi", &["kind"]),
    ("schema/fields.moi", &["field", "value"]),
    ("schema/edges.moi", &["edge"]),
    ("schema/policy.moi", &["policy"]),
];

/// What a tree check found.
#[derive(Clone, Debug)]
pub struct Tree {
    /// The marker.
    pub marker: Marker,
    /// The effective schema: the core schema of the marker's version with the tree's schema rows.
    pub schema: Schema,
    /// Every file of the layout with its check, by path, in path order.
    pub files: Vec<(String, Checked)>,
    /// The root entries outside the layout, which the importer ignores (§3.4).
    pub ignored: Vec<String>,
}

impl Tree {
    /// True when every file of the layout is the exporter's encoding of its parse ([F14 §15] rule 3).
    pub fn canonical(&self) -> bool {
        self.files.iter().all(|(_, c)| c.canonical)
    }
}

/// Where an entry of the tree belongs ([F14 §3.1], §3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Marker,
    Node,
    Table,
    Query,
    Refs,
    Ignored,
}

/// Classifies one blob path; an entry under `nodes/`, `schema/` or `refs/` outside the layout is `ImageParse`.
fn slot_of(path: &str) -> Result<Slot> {
    let parts: Vec<&str> = path.split('/').collect();
    let layout_err = || {
        parse_err(
            Rule::Layout,
            0,
            format!("{path}: an entry outside the layout of §3.1 [F14 §3.4, §9.2]"),
        )
    };
    match parts[0] {
        ".moirai-image" if parts.len() == 1 => Ok(Slot::Marker),
        "nodes" => {
            let ok = parts.len() == 4
                && is_lhex(parts[1], 2)
                && is_lhex(parts[2], 2)
                && parts[3].strip_suffix(".moi").is_some_and(|u| {
                    is_lhex(u, 32) && u[..4] == format!("{}{}", parts[1], parts[2])
                });
            if ok { Ok(Slot::Node) } else { layout_err() }
        }
        "schema" => match parts.as_slice() {
            [_, "kinds.moi" | "fields.moi" | "edges.moi" | "policy.moi"] => Ok(Slot::Table),
            [_, "queries", q] if q.strip_suffix(".moi").is_some_and(|q| is_lhex(q, 32)) => {
                Ok(Slot::Query)
            }
            _ => layout_err(),
        },
        "refs" => match parts.as_slice() {
            [_, "heads.moi" | "tags.moi"] => Ok(Slot::Refs),
            _ => layout_err(),
        },
        _ => Ok(Slot::Ignored),
    }
}

/// Checks one schema table file at `path` (`schema/kinds.moi`, `schema/fields.moi`, `schema/edges.moi` or
/// `schema/policy.moi`): a schema file ([F14 §7.1]) with at least one row (§3.1), each of its table's item class (§7.1:
/// kinds, fields and enumeration values, edge kinds, policy rows).
pub fn check_table_file(path: &str, bytes: &[u8], schema: &Schema) -> Result<Checked> {
    let Some((_, allowed)) = TABLES.iter().find(|t| t.0 == path) else {
        return parse_err(
            Rule::Layout,
            0,
            format!("{path}: not a schema table file [F14 §3.1]"),
        );
    };
    let c = with_path(path, check_file(bytes, Place::default(), schema))?;
    let Parsed::Schema(r) = &c.parsed else {
        return parse_err(
            Rule::Layout,
            0,
            format!("{path}: not a schema table file [F14 §7.1]"),
        );
    };
    if r.is_empty() {
        return parse_err(
            Rule::Layout,
            0,
            format!("{path}: a table file with no row [F14 §3.1, §3.4]"),
        );
    }
    if let Some(bad) = r.iter().find(|x| !allowed.contains(&x.what.as_str())) {
        return parse_err(
            Rule::NoProduction,
            0,
            format!(
                "{path}: a {} row matches no production of this table [F14 §7.1, §9.2]",
                bad.what
            ),
        );
    }
    Ok(c)
}

fn with_path<T>(path: &str, r: Result<T>) -> Result<T> {
    r.map_err(|mut e| {
        e.reason = format!("{path}: {}", e.reason);
        e
    })
}

/// Checks a whole image tree, given as (path relative to the tree root with `/` separators, bytes) for every blob, for
/// a destination of object format `algo` ([F14 §4]: a marker of another format is `ImageParse`).
pub fn check_tree(blobs: &[(String, Vec<u8>)], algo: Algo) -> Result<Tree> {
    let mut sorted: Vec<&(String, Vec<u8>)> = blobs.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut slots = Vec::with_capacity(sorted.len());
    for (p, _) in &sorted {
        slots.push(slot_of(p)?);
    }
    let Some(mi) = slots.iter().position(|s| *s == Slot::Marker) else {
        return parse_err(
            Rule::Marker,
            0,
            "the tree has no .moirai-image marker [F14 §4, §9.2]",
        );
    };
    let marker_bytes = &sorted[mi].1;
    let marker = with_path(".moirai-image", files::parse_marker(marker_bytes))?;
    if marker.object_format != algo.name() {
        return parse_err(
            Rule::Marker,
            0,
            format!(
                ".moirai-image: object format {} is not the destination's {} [F14 §4]",
                marker.object_format,
                algo.name()
            ),
        );
    }
    let mut schema = Schema::core();
    let mut out: Vec<(String, Checked)> = Vec::with_capacity(sorted.len());
    out.push((
        ".moirai-image".into(),
        check_file(marker_bytes, Place::default(), &schema)?,
    ));
    let mut rows: Vec<SchemaRow> = Vec::new();
    for (table, _) in TABLES {
        let Some(i) = sorted.iter().position(|(p, _)| p == table) else {
            continue;
        };
        let c = check_table_file(table, &sorted[i].1, &schema)?;
        if let Parsed::Schema(r) = &c.parsed {
            rows.extend(r.iter().cloned());
        }
        out.push((table.to_owned(), c));
    }
    with_path("schema/", files::extend_schema(&mut schema, &rows))?;
    let mut ignored = Vec::new();
    for ((p, b), slot) in sorted.iter().zip(&slots) {
        let c = match slot {
            Slot::Marker | Slot::Table => continue,
            Slot::Ignored => {
                ignored.push(p.clone());
                continue;
            }
            Slot::Node => {
                let c = with_path(p, check_file(b, Place::of_path(p), &schema))?;
                if !matches!(c.parsed, Parsed::Node(_)) {
                    return parse_err(Rule::Layout, 0, format!("{p}: not a node file [F14 §6.1]"));
                }
                c
            }
            Slot::Query => {
                let c = with_path(p, check_file(b, Place::of_path(p), &schema))?;
                if !matches!(c.parsed, Parsed::Query(_)) {
                    return parse_err(
                        Rule::Layout,
                        0,
                        format!("{p}: not a named-query file [F14 §7.2]"),
                    );
                }
                c
            }
            Slot::Refs => {
                let c = with_path(p, check_file(b, Place::default(), &schema))?;
                if !matches!(c.parsed, Parsed::Refs(..)) {
                    return parse_err(Rule::Layout, 0, format!("{p}: not a ref row file [F14 §8]"));
                }
                c
            }
        };
        out.push((p.clone(), c));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Tree {
        marker,
        schema,
        files: out,
        ignored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(p: &str, b: &str) -> (String, Vec<u8>) {
        (p.to_owned(), b.as_bytes().to_vec())
    }

    const MARKER: &str = "moirai-image 1\nobject-format: sha1\nschema-version: 1\n";
    const NODE: &str = "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e40\nkind: task\ntitle: t\ndeleted: cca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7a\n";

    /// [F14 §3.4], §4: the marker, the layout and the file-name rules; root entries outside the layout are ignored.
    #[test]
    fn tree_rules() {
        let good = vec![
            blob(".moirai-image", MARKER),
            blob("nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e40.moi", NODE),
            blob("README.md", "hello"),
        ];
        let t = check_tree(&good, Algo::Sha1).unwrap();
        assert!(t.canonical());
        assert_eq!(t.ignored, vec!["README.md".to_owned()]);
        assert!(check_tree(&good, Algo::Sha256).is_err());
        assert!(check_tree(&good[1..], Algo::Sha1).is_err());
        let mut stray = good.clone();
        stray.push(blob("nodes/01/8f/notes.txt", "x"));
        assert!(check_tree(&stray, Algo::Sha1).is_err());
        let mut prefix = good.clone();
        prefix[1].0 = "nodes/01/90/018f3c2e7a117b3c9d5e4c2f1a0b9e40.moi".into();
        assert!(check_tree(&prefix, Algo::Sha1).is_err());
        let mut upper = good.clone();
        upper[1].0 = "nodes/01/8F/018f3c2e7a117b3c9d5e4c2f1a0b9e40.moi".into();
        assert!(check_tree(&upper, Algo::Sha1).is_err());
        let mut other_uid = good.clone();
        other_uid[1].0 = "nodes/01/8f/018f3c2e7a117b3c9d5e4c2f1a0b9e41.moi".into();
        assert!(check_tree(&other_uid, Algo::Sha1).is_err());
        let mut refs = good.clone();
        refs.push(blob(
            "refs/remotes.moi",
            "moirai-refs 1\nref main kind=work\n",
        ));
        assert!(check_tree(&refs, Algo::Sha1).is_err());
        let mut empty_table = good.clone();
        empty_table.push(blob("schema/kinds.moi", "moirai-schema 1\n"));
        assert!(check_tree(&empty_table, Algo::Sha1).is_err());
        // [F14 §3.1], §7.1 (spec sync 2b S2B-R-28): `schema/policy.moi` is a table file of policy rows only.
        let mut policy = good.clone();
        policy.push(blob(
            "schema/policy.moi",
            "moirai-schema 1\npolicy merge.policy.task delete-wins\n",
        ));
        assert!(check_tree(&policy, Algo::Sha1).unwrap().canonical());
        let mut kind_in_policy = good.clone();
        kind_in_policy.push(blob(
            "schema/policy.moi",
            "moirai-schema 1\nkind incident derivation=random root-variant=none existence=resurrect\n",
        ));
        assert!(check_tree(&kind_in_policy, Algo::Sha1).is_err());
        let mut policy_in_kinds = good;
        policy_in_kinds.push(blob(
            "schema/kinds.moi",
            "moirai-schema 1\npolicy merge.policy.task delete-wins\n",
        ));
        assert!(check_tree(&policy_in_kinds, Algo::Sha1).is_err());
    }
}
