//! The row tags of the model's source against the rule tables ([RULES/README] §8 "Procedure tables", "Row coverage";
//! [60 §4.6]): every `rule:` tag names a row that exists, and every row of a procedure table is tagged by the function
//! that implements it, or is listed here with the work package that implements it.

use crate::rules::{TableKind, rules};
use std::collections::{BTreeMap, BTreeSet};

/// The rows of procedure tables that later work packages implement, with the package.
const LATER: &[(&str, &str)] = &[
    // Packs, briefs and notices: LQ-3's evaluator and the reference renderer (WP-93b, WP-71a).
    ("pack-bytes", "WP-93b"),
    ("pack-floors", "WP-93b"),
    ("pack-members", "WP-93b"),
    ("pack-levels", "WP-93b"),
    ("pack-order", "WP-93b"),
    ("pack-header", "WP-93b"),
    ("pack-render", "WP-93b"),
    ("pack-fill", "WP-93b"),
    ("hook-pack", "WP-93b"),
    ("brief-classes", "WP-93b"),
    ("delta-rules", "WP-93b"),
    ("notice-sets", "WP-93b"),
    ("notice-rules", "WP-93b"),
    ("notice-digest", "WP-93b"),
    ("notice-entry", "WP-93b"),
];

/// Single rows of tables this package implements, left to a later package.
const LATER_ROWS: &[(&str, &str)] = &[
    ("FL-008", "M5: tombstone files"),
    ("FL-011", "WP-71a: rendering"),
    ("GR-016", "no machine: phase_state is an ordinary field"),
    (
        "DM-010",
        "M5: a foreign two-parent commit imported as a typed merge",
    ),
    ("DM-011", "M5: an import merge"),
    (
        "DM-014",
        "M5: a foreign merge's TextHunk taken from the foreign tree",
    ),
    (
        "DM-015",
        "M5: a foreign merge's counters taken from the typed merge",
    ),
    ("HT-003", "M5: the ForeignMerge hint of an import"),
    (
        "LH-007",
        "M5: a hand-edited `field path:` in an image is a foreign SetField",
    ),
];

/// Every `// rule:` tag of the crate's source, by row id, with the file it is in.
fn tags() -> BTreeMap<String, BTreeSet<String>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)
            .expect("the source directory")
            .flatten()
        {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).expect("a source file");
                for line in text.lines() {
                    if let Some(rest) = line.trim_start().strip_prefix("// rule: ") {
                        for id in rest.split(", ") {
                            out.entry(id.trim().to_string())
                                .or_default()
                                .insert(p.file_name().unwrap().to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn every_tag_names_a_row_and_every_procedure_row_is_tagged() {
    let r = rules();
    let t = tags();
    for id in t.keys() {
        assert!(r.row(id).is_some(), "the tag rule: {id} names no row");
    }
    let mut missing = Vec::new();
    for table in r.tables().filter(|x| x.kind == TableKind::Procedure) {
        if LATER.iter().any(|(n, _)| *n == table.id) {
            continue;
        }
        for row in &table.rows {
            if row.basis() == Some("withdrawn") {
                continue;
            }
            if !t.contains_key(&row.id) && !LATER_ROWS.iter().any(|(id, _)| *id == row.id) {
                missing.push(format!("{} ({})", row.id, table.id));
            }
        }
    }
    assert!(missing.is_empty(), "untagged procedure rows: {missing:?}");
}
