//! \[F14\] the image's other files: the `.moirai-image` marker (§4), the schema tables (§7.1), named-query files (§7.2),
//! checkpoint ref rows (§8) and the side-ref files `meta.moi`, `aliases/<h1>.moi` and `ops.moi` (§14). Each parser reads
//! the importer's superset (§9.1 rules 1–4) and each encoder writes the canonical bytes.

use super::schema::{FieldDef, KindDef, Schema, Storage, Sub, Ty, ty_of_name};
use super::text::*;
use crate::prim::{Result, blake3_256, hex};

/// Normalises (§9.1 rules 1–4) and splits a line-oriented file; trailing SP/HT removed from each line.
fn lines_of(b: &[u8]) -> Result<Vec<String>> {
    let norm = import_normalise(b);
    let t = utf8_file(&norm)?;
    if t.contains('\0') {
        return parse_err(Rule::NulOutsideBody, 0, "U+0000 in a line file [F14 §9.2]");
    }
    let t = t.strip_suffix('\n').unwrap_or(t);
    Ok(t.split('\n')
        .map(|l| l.trim_end_matches([' ', '\t']).to_owned())
        .collect())
}

fn join(lines: &[String]) -> Vec<u8> {
    let mut o = String::new();
    for l in lines {
        o.push_str(l);
        o.push('\n');
    }
    o.into_bytes()
}

/// The `.moirai-image` marker ([F14 §4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    /// `sha1` or `sha256`.
    pub object_format: String,
    /// The schema version (1 in format v1).
    pub schema_version: u64,
}

/// Parses the marker; a version other than 1 or a schema version other than 1 is `ImageParse`.
pub fn parse_marker(b: &[u8]) -> Result<Marker> {
    let l = lines_of(b)?;
    if l.len() != 3 || l[0] != "moirai-image 1" {
        return parse_err(
            Rule::Marker,
            0,
            "the marker is not three lines beginning `moirai-image 1` [F14 §4]",
        );
    }
    let Some(of) = l[1]
        .strip_prefix("object-format: ")
        .filter(|v| *v == "sha1" || *v == "sha256")
    else {
        return parse_err(
            Rule::Marker,
            0,
            "the marker's object-format is not sha1 or sha256 [F14 §4]",
        );
    };
    let Some(sv) = l[2].strip_prefix("schema-version: ").and_then(parse_dec) else {
        return parse_err(
            Rule::Marker,
            0,
            "the marker's schema-version is not a dec [F14 §4]",
        );
    };
    if sv != 1 {
        return parse_err(Rule::Marker, 0, "a schema version other than 1 [F14 §4]");
    }
    Ok(Marker {
        object_format: of.to_owned(),
        schema_version: sv,
    })
}

/// Encodes the marker.
pub fn encode_marker(m: &Marker) -> Vec<u8> {
    format!(
        "moirai-image 1\nobject-format: {}\nschema-version: {}\n",
        m.object_format, m.schema_version
    )
    .into_bytes()
}

/// One schema row ([F14 §7.1]) as its property list, kept to re-encode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaRow {
    /// `kind`, `field`, `value`, `edge` or `policy`.
    pub what: String,
    /// The positional words after the row keyword (a `policy` row: its `pname`).
    pub keys: Vec<String>,
    /// `k=v` properties in grammar order (values decoded); a `policy` row's token as `value`.
    pub props: Vec<(String, String)>,
    /// `retired`.
    pub retired: bool,
}

const KIND_PROPS: [(&str, bool); 4] = [
    ("derivation", true),
    ("root-variant", true),
    ("existence", true),
    ("flags", false),
];
const FIELD_PROPS: [(&str, bool); 12] = [
    ("type", true),
    ("elem", false),
    ("class", true),
    ("storage", true),
    ("decl", true),
    ("optional", true),
    ("index", true),
    ("coerce", true),
    ("flags", false),
    ("default", false),
    ("min", false),
    ("max", false),
];
const VALUE_PROPS: [(&str, bool); 3] = [("rank", true), ("flags", false), ("covers", false)];
const EDGE_PROPS: [(&str, bool); 14] = [
    ("class", true),
    ("on-dst", true),
    ("on-src", true),
    ("acyclic", true),
    ("card", true),
    ("max-depth", true),
    ("derivation", true),
    ("props", true),
    ("flags", false),
    ("lq", true),
    ("src", true),
    ("dst", true),
    ("reverse", false),
    ("reading", true),
];

fn one_of(v: &str, set: &[&str]) -> bool {
    set.contains(&v)
}

fn sorted_list(v: &str, item: fn(&str) -> bool) -> bool {
    let parts: Vec<&str> = v.split(',').collect();
    parts.iter().all(|p| item(p)) && parts.windows(2).all(|w| w[0] < w[1])
}

/// Checks one property value by the row grammar ([F14 §7.1]).
fn prop_ok(what: &str, k: &str, v: &str) -> bool {
    match (what, k) {
        ("kind", "derivation") => one_of(v, &["random", "file-key", "root-key"]),
        ("kind", "root-variant") => one_of(v, &["none", "root-key"]),
        ("kind", "existence") => one_of(v, &["delete-wins", "resurrect", "none"]),
        ("kind", "flags") => sorted_list(v, |x| {
            one_of(
                x,
                &[
                    "done_derived",
                    "has_done",
                    "immutable_fields",
                    "title_derived",
                ],
            )
        }),
        ("field", "type") | ("field", "elem") => one_of(
            v,
            &[
                "bool",
                "int",
                "counter",
                "f64",
                "enum",
                "text",
                "sym",
                "set",
                "ref",
                "commitref",
                "path",
                "oid",
                "pathmove",
            ],
        ),
        ("field", "class") => one_of(
            v,
            &[
                "none",
                "scalar",
                "owner",
                "authority",
                "status",
                "counter",
                "set",
                "text",
                "section-text",
                "hierarchy",
                "identity",
                "observation",
                "alias-set",
                "glob-set",
                "pathmove-set",
                "derived",
            ],
        ),
        ("field", "storage") => one_of(v, &["header", "flag", "cold", "field", "title", "body"]),
        ("field", "decl") | ("value", "rank") | ("edge", "max-depth") => parse_dec(v).is_some(),
        ("field", "optional") => one_of(v, &["true", "false"]),
        ("field", "index") => one_of(v, &["none", "column", "bitmap"]),
        ("field", "coerce") => one_of(v, &["none", "priority", "revision-integer", "timestamp"]),
        ("field", "flags") => sorted_list(v, |x| one_of(x, &["ascii", "one_line"])),
        ("field", "default") => true,
        ("field", "min") | ("field", "max") => parse_sdec(v).is_some(),
        ("value", "flags") => sorted_list(v, |x| one_of(x, &["done", "side"])),
        ("value", "covers") => sorted_list(v, is_vname),
        ("edge", "class") => one_of(v, &["structural", "historical"]),
        ("edge", "on-dst") => one_of(
            v,
            &[
                "restrict",
                "restrict-cascade-reparent",
                "restrict-reassign",
                "restrict-repoint",
                "drop",
                "drop-notify",
                "drop-src-suspect",
                "tombstone",
                "tombstone-src-suspect",
            ],
        ),
        ("edge", "on-src") => one_of(
            v,
            &[
                "drop",
                "drop-rollups",
                "repoint-or-flag",
                "drop-reopen",
                "retain-warn",
                "retain",
                "recompute",
                "retain-anchors",
            ],
        ),
        ("edge", "acyclic") => one_of(
            v,
            &["none", "forest", "precedence", "dag", "by-construction"],
        ),
        ("edge", "card") => one_of(
            v,
            &[
                "many",
                "max-1-per-src",
                "max-1-active-per-dst",
                "chain-1",
                "typical-1",
                "anchors-min-1",
            ],
        ),
        ("edge", "derivation") => one_of(v, &["none", "anchor-key"]),
        ("edge", "props") => one_of(v, &["none", "pinned", "flagged", "anchor"]),
        ("edge", "flags") => sorted_list(v, |x| one_of(x, &["same_kind", "symmetric"])),
        ("edge", "lq") => is_lqname(v),
        ("edge", "src") | ("edge", "dst") => v == "*" || sorted_list(v, is_iname),
        ("edge", "reverse") => sorted_list(v, is_lqname),
        ("edge", "reading") => {
            !v.is_empty()
                && v.len() <= 200
                && v.bytes().all(|b| (0x20..0x7F).contains(&b))
                && v.matches("{a}").count() == 1
                && v.matches("{b}").count() == 1
        }
        _ => false,
    }
}

/// [F14 §7.1] `pname`: 1–16 `pseg`s joined by `.`, at most 255 bytes, `pseg` = (LALPHA / DIGIT) *63(LALPHA / DIGIT /
/// `-` / `_`): [CFG §3.3]'s canonical key-name form of a [CFG §10.13] row instance.
pub fn is_pname(s: &str) -> bool {
    s.len() <= 255
        && s.split('.').count() <= 16
        && s.split('.').all(|p| {
            let b = p.as_bytes();
            let word = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
            !b.is_empty()
                && b.len() <= 64
                && word(&b[0])
                && b.iter().all(|c| word(c) || *c == b'-' || *c == b'_')
        })
}

/// [F14 §7.1] `policy-row` = `policy ` pname SP token: the row's name and its canonical value ([CFG §4.1]) as a §5.5
/// token, which the row keeps decoded.
fn parse_policy_row(rest: &str, at: usize) -> Result<SchemaRow> {
    let Some((name, value)) = rest.split_once(' ') else {
        return parse_err(
            Rule::NoProduction,
            at,
            "a policy row is `policy <pname> <token>` [F14 §7.1]",
        );
    };
    if !is_pname(name) {
        return parse_err(
            Rule::NoProduction,
            at,
            format!("policy row name {name:?} is not a pname [F14 §7.1]"),
        );
    }
    let (v, n, _) = read_token(value, at)?;
    if n != value.len() {
        return parse_err(
            Rule::NoProduction,
            at,
            "bytes after a policy row's value [F14 §7.1]",
        );
    }
    Ok(SchemaRow {
        what: "policy".into(),
        keys: vec![name.to_owned()],
        props: vec![("value".into(), v)],
        retired: false,
    })
}

fn parse_row(line: &str, at: usize) -> Result<SchemaRow> {
    let (what, rest) = line.split_once(' ').unwrap_or((line, ""));
    if what == "policy" {
        return parse_policy_row(rest, at);
    }
    let (nkeys, table): (usize, &[(&str, bool)]) = match what {
        "kind" => (1, &KIND_PROPS),
        "field" => (2, &FIELD_PROPS),
        "value" => (3, &VALUE_PROPS),
        "edge" => (1, &EDGE_PROPS),
        _ => {
            return parse_err(
                Rule::NoProduction,
                at,
                format!("an unknown schema row {what:?} [F14 §7.1]"),
            );
        }
    };
    let mut rest = rest;
    let mut keys = Vec::new();
    for i in 0..nkeys {
        if i > 0 {
            let Some(r) = rest.strip_prefix(' ') else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "schema row keys are separated by SP [F14 §7.1]",
                );
            };
            rest = r;
        }
        let n = rest.find(' ').unwrap_or(rest.len());
        let k = &rest[..n];
        let ok = match (what, i) {
            ("value", 0) => k == "*" || is_iname(k),
            ("value", 2) => is_vname(k),
            _ => is_iname(k),
        };
        if !ok {
            return parse_err(
                Rule::NoProduction,
                at,
                format!("schema row key {k:?} is not a name [F14 §7.1]"),
            );
        }
        keys.push(k.to_owned());
        rest = &rest[n..];
    }
    let mut props = Vec::new();
    let mut retired = false;
    let mut ti = 0;
    while !rest.is_empty() {
        if rest == " retired" {
            retired = true;
            break;
        }
        let Some(r) = rest.strip_prefix(' ') else {
            return parse_err(
                Rule::NoProduction,
                at,
                "schema row properties are separated by SP [F14 §7.1]",
            );
        };
        let Some((k, v_and_rest)) = r.split_once('=') else {
            return parse_err(
                Rule::NoProduction,
                at,
                "a schema row property lacks = [F14 §7.1]",
            );
        };
        let Some(pos) = table[ti..].iter().position(|(n, _)| *n == k) else {
            return parse_err(
                Rule::NoProduction,
                at,
                format!("schema row property {k:?} unknown or out of order [F14 §7.1]"),
            );
        };
        if table[ti..ti + pos].iter().any(|(_, req)| *req) {
            return parse_err(
                Rule::NoProduction,
                at,
                format!("a required schema row property before {k:?} is missing [F14 §7.1]"),
            );
        }
        ti += pos + 1;
        let (v, n) = if (k == "default" || k == "reading") && v_and_rest.starts_with('"') {
            let (t, n, _) = read_token(v_and_rest, at)?;
            (t, n)
        } else {
            let n = v_and_rest.find(' ').unwrap_or(v_and_rest.len());
            (v_and_rest[..n].to_owned(), n)
        };
        if !prop_ok(what, k, &v) {
            return parse_err(
                Rule::NoProduction,
                at,
                format!("schema row property {k}={v:?} breaks the grammar [F14 §7.1]"),
            );
        }
        props.push((k.to_owned(), v));
        rest = &v_and_rest[n..];
    }
    if table[ti..].iter().any(|(_, req)| *req) {
        return parse_err(
            Rule::NoProduction,
            at,
            "a required schema row property is missing [F14 §7.1]",
        );
    }
    let has = |k: &str| props.iter().any(|(n, _)| n == k);
    if what == "field" {
        let ty = &props.iter().find(|p| p.0 == "type").expect("required").1;
        if (ty == "set") != has("elem") || has("min") != has("max") {
            return parse_err(
                Rule::NoProduction,
                at,
                "elem= exactly for set, min= and max= together [F14 §7.1]",
            );
        }
    }
    Ok(SchemaRow {
        what: what.to_owned(),
        keys,
        props,
        retired,
    })
}

fn row_text(r: &SchemaRow) -> String {
    let mut o = format!("{} {}", r.what, r.keys.join(" "));
    if r.what == "policy" {
        let v = r.props.first().map_or("", |p| p.1.as_str());
        o.push(' ');
        o.push_str(&token(v));
        return o;
    }
    for (k, v) in &r.props {
        let v = if k == "default" || k == "reading" {
            token(v)
        } else {
            v.clone()
        };
        o.push_str(&format!(" {k}={v}"));
    }
    if r.retired {
        o.push_str(" retired");
    }
    o
}

/// A row's place in [F14 §7.1]'s order: its table (kinds, fields and values, edges, policy rows), then its key; a field
/// row sorts before the value rows of its field (its value name counts as empty). Two rows of one key are a line
/// repeated (§9.2), two `policy` rows of one name included.
fn row_order_key(r: &SchemaRow) -> (u8, Vec<Vec<u8>>) {
    match r.what.as_str() {
        "field" => (
            1,
            vec![
                r.keys[0].clone().into_bytes(),
                r.keys[1].clone().into_bytes(),
                Vec::new(),
            ],
        ),
        "value" => (1, r.keys.iter().map(|k| k.clone().into_bytes()).collect()),
        w => (
            match w {
                "kind" => 0,
                "edge" => 2,
                _ => 3,
            },
            vec![r.keys[0].clone().into_bytes()],
        ),
    }
}

/// Parses a schema table file (`kinds.moi`, `fields.moi`, `edges.moi`, `policy.moi`, [F14 §7.1]); rows are returned
/// in canonical order.
pub fn parse_schema_file(b: &[u8]) -> Result<Vec<SchemaRow>> {
    let l = lines_of(b)?;
    if l.first().map(String::as_str) != Some("moirai-schema 1") {
        return parse_err(
            Rule::MagicVersion,
            0,
            "a schema file does not begin `moirai-schema 1` [F14 §7.1]",
        );
    }
    let mut rows = Vec::with_capacity(l.len() - 1);
    for (i, line) in l.iter().enumerate().skip(1) {
        rows.push(parse_row(line, i)?);
    }
    rows.sort_by_key(row_order_key);
    for w in rows.windows(2) {
        if row_order_key(&w[0]) == row_order_key(&w[1]) {
            return parse_err(
                Rule::LineRepeated,
                0,
                "two schema rows of one key [F14 §9.2]",
            );
        }
    }
    Ok(rows)
}

/// Encodes a schema table file.
pub fn encode_schema_file(rows: &[SchemaRow]) -> Vec<u8> {
    let mut l = vec!["moirai-schema 1".to_owned()];
    l.extend(rows.iter().map(row_text));
    join(&l)
}

/// Adds a tree's project schema rows to the effective schema ([F14 §11.1] step 1). A project kind's initial status is
/// its non-retired `status` value with the least `rank`, ties by value name bytewise ([F08 §8.5.1]). Policy rows are
/// policy data ([F08 §8.5.6]) and change no parse.
pub fn extend_schema(schema: &mut Schema, rows: &[SchemaRow]) -> Result<()> {
    let mut initial: std::collections::BTreeMap<String, (u64, String)> = Default::default();
    for r in rows {
        let p = |k: &str| r.props.iter().find(|x| x.0 == k).map(|x| x.1.as_str());
        match r.what.as_str() {
            "kind" => {
                if schema.kinds.contains_key(&r.keys[0]) {
                    return parse_err(
                        Rule::NoProduction,
                        0,
                        "a project kind shadows a core kind [F08 §8.1]",
                    );
                }
                schema.kinds.insert(
                    r.keys[0].clone(),
                    KindDef {
                        name: r.keys[0].clone(),
                        statuses: Vec::new(),
                        initial: String::new(),
                        title_derived: p("flags")
                            .is_some_and(|f| f.split(',').any(|x| x == "title_derived")),
                        fields: Default::default(),
                        retired: r.retired,
                    },
                );
            }
            "field" => {
                let Some(ty) = ty_of_name(p("type").unwrap_or(""), p("elem")) else {
                    return parse_err(
                        Rule::NoProduction,
                        0,
                        "a field row's type is not a closed-set type [F14 §7.1]",
                    );
                };
                let storage = match p("storage").unwrap_or("") {
                    "header" => Storage::Header,
                    "flag" => Storage::Flag,
                    "cold" => Storage::Cold,
                    "title" => Storage::Title,
                    "body" => Storage::Body,
                    _ => Storage::Field,
                };
                let range = match (p("min").and_then(parse_sdec), p("max").and_then(parse_sdec)) {
                    (Some(a), Some(b)) => Some((a, b)),
                    _ => None,
                };
                let d = FieldDef {
                    name: r.keys[1].clone(),
                    ty,
                    storage,
                    values: Vec::new(),
                    default: p("default").map(str::to_owned),
                    one_line: p("flags").is_some_and(|f| f.contains("one_line")),
                    range,
                    sub: if p("flags").is_some_and(|f| f.contains("ascii")) {
                        Sub::Ascii
                    } else {
                        Sub::None
                    },
                    implied_root: false,
                    retired: r.retired,
                };
                if r.keys[0] == "*" {
                    return parse_err(
                        Rule::NoProduction,
                        0,
                        "a project field names no kind [F14 §7.1]",
                    );
                }
                let Some(k) = schema.kinds.get_mut(&r.keys[0]) else {
                    return parse_err(
                        Rule::UnknownName,
                        0,
                        format!("a field row of unknown kind {} [F14 §9.2]", r.keys[0]),
                    );
                };
                k.fields.insert(d.name.clone(), d);
            }
            "value" => {
                let (kind, field, value) = (&r.keys[0], &r.keys[1], &r.keys[2]);
                if field == "status" && kind != "*" {
                    let Some(k) = schema.kinds.get_mut(kind) else {
                        return parse_err(
                            Rule::UnknownName,
                            0,
                            "a status value of an unknown kind [F14 §9.2]",
                        );
                    };
                    k.statuses.push(value.clone());
                    let rank = p("rank").and_then(parse_dec).unwrap_or(u64::MAX);
                    let cand = (rank, value.clone());
                    if !r.retired && initial.get(kind).is_none_or(|best| cand < *best) {
                        initial.insert(kind.clone(), cand);
                    }
                    continue;
                }
                let target = if kind == "*" {
                    schema.common.get_mut(field)
                } else {
                    schema
                        .kinds
                        .get_mut(kind)
                        .and_then(|k| k.fields.get_mut(field))
                };
                match target {
                    Some(f) if f.ty == Ty::Enum || f.ty == Ty::Set(super::schema::Elem::Enum) => {
                        f.values.push(value.clone())
                    }
                    Some(f) if !f.values.is_empty() => f.values.push(value.clone()),
                    _ => {
                        if let Some(f) = schema
                            .kinds
                            .get_mut(kind)
                            .and_then(|k| k.fields.get_mut(field))
                        {
                            f.values.push(value.clone());
                        }
                    }
                }
            }
            "policy" => {}
            _ => {
                let props = match p("props") {
                    Some("pinned") => super::schema::Props::Pinned,
                    Some("flagged") => super::schema::Props::Flagged,
                    Some("anchor") => super::schema::Props::Anchor,
                    _ => super::schema::Props::None,
                };
                let sym = p("flags").is_some_and(|f| f.contains("symmetric"));
                if schema
                    .edges
                    .insert(r.keys[0].clone(), (props, sym))
                    .is_some()
                {
                    return parse_err(
                        Rule::NoProduction,
                        0,
                        "a project edge kind shadows a core one [F08 §8.1]",
                    );
                }
            }
        }
    }
    for (kind, (_, value)) in initial {
        if let Some(k) = schema.kinds.get_mut(&kind) {
            k.initial = value;
        }
    }
    Ok(())
}

/// A named-query file ([F14 §7.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    /// A definition.
    Def {
        /// The name's canonical spelling.
        name: String,
        /// `lq:`.
        lq: u64,
        /// `params:`.
        params: Option<String>,
        /// `shape:`.
        shape: String,
        /// `budget:`.
        budget: String,
        /// The portable text.
        text: String,
    },
    /// A conflicted definition: the name, the class and each side's complete `query-def` file.
    Conflict {
        /// The name.
        name: String,
        /// The class name.
        class: String,
        /// base, ours, theirs; `None` = absent.
        sides: [Option<String>; 3],
    },
}

/// The file name hash `q` of a query name ([F14 §7.2.1]).
pub fn query_q(name: &str) -> String {
    hex(&blake3_256(name.as_bytes())[..16])
}

/// Parses a named-query file; `q` is the file-name hash when known ([F14 §9.2]).
pub fn parse_query(b: &[u8], q: Option<&str>) -> Result<Query> {
    let norm = import_normalise(b);
    let t = utf8_file(&norm)?;
    let (head, text) = match t.find("\n---\n") {
        Some(i) => (&t[..i], Some(&t[i + 5..])),
        None => (t.strip_suffix('\n').unwrap_or(t), None),
    };
    let l: Vec<String> = head
        .split('\n')
        .map(|x| x.trim_end_matches([' ', '\t']).to_owned())
        .collect();
    if l.first().map(String::as_str) != Some("moirai-query 1") {
        return parse_err(
            Rule::MagicVersion,
            0,
            "a query file does not begin `moirai-query 1` [F14 §7.2.2]",
        );
    }
    let Some(name) = l.get(1).and_then(|x| x.strip_prefix("name: ")) else {
        return parse_err(
            Rule::RequiredLineMissing,
            0,
            "a query file lacks its name: line [F14 §7.2.2]",
        );
    };
    let name = read_sval(name, 0)?;
    if q.is_some_and(|q| q != query_q(&name)) {
        return parse_err(
            Rule::QueryFileName,
            0,
            "a query file's name does not hash to its file name [F14 §9.2]",
        );
    }
    if let Some(c) = l
        .get(2)
        .and_then(|x| x.strip_prefix("conflict definition class="))
    {
        if l.len() != 3 || text.is_some() {
            return parse_err(
                Rule::NoProduction,
                0,
                "a conflicted query file holds more than its conflict line [F14 §7.2.4]",
            );
        }
        let n = c.find(' ').unwrap_or(c.len());
        let class = &c[..n];
        if !["FieldEdit", "DeleteVsModify"].contains(&class) {
            return parse_err(
                Rule::UnknownName,
                0,
                "a query conflict class other than FieldEdit or DeleteVsModify [F14 §7.2.4]",
            );
        }
        let mut rest = &c[n..];
        let mut sides: [Option<String>; 3] = Default::default();
        for (i, k) in [" base=", " ours=", " theirs="].iter().enumerate() {
            let Some(r) = rest.strip_prefix(k) else {
                return parse_err(
                    Rule::NoProduction,
                    0,
                    format!("a query conflict line lacks{k} [F14 §7.2.2]"),
                );
            };
            rest = r;
            if rest.starts_with('"') {
                let (s, m) = read_jstring(rest, 0)?;
                parse_query(s.as_bytes(), None)?;
                sides[i] = Some(s);
                rest = &rest[m..];
            }
        }
        if !rest.is_empty() {
            return parse_err(
                Rule::NoProduction,
                0,
                "bytes after a query conflict line [F14 §7.2.2]",
            );
        }
        return Ok(Query::Conflict {
            name,
            class: class.to_owned(),
            sides,
        });
    }
    let Some(text) = text else {
        return parse_err(
            Rule::NoProduction,
            0,
            "a query definition lacks its --- and text [F14 §7.2.2]",
        );
    };
    let mut i = 2;
    let Some(lq) = l
        .get(i)
        .and_then(|x| x.strip_prefix("lq: "))
        .and_then(parse_dec)
        .filter(|v| *v >= 1)
    else {
        return parse_err(
            Rule::RequiredLineMissing,
            0,
            "a query lq: is missing or not >= 1 [F14 §7.2.2]",
        );
    };
    i += 1;
    let params = match l.get(i).and_then(|x| x.strip_prefix("params: ")) {
        Some(p) => {
            i += 1;
            Some(read_sval(p, 0)?)
        }
        None => None,
    };
    let Some(shape) = l
        .get(i)
        .and_then(|x| x.strip_prefix("shape: "))
        .filter(|v| is_vname(v))
    else {
        return parse_err(
            Rule::RequiredLineMissing,
            0,
            "a query shape: is missing [F14 §7.2.2]",
        );
    };
    let Some(budget) = l
        .get(i + 1)
        .and_then(|x| x.strip_prefix("budget: "))
        .filter(|v| is_vname(v))
    else {
        return parse_err(
            Rule::RequiredLineMissing,
            0,
            "a query budget: is missing [F14 §7.2.2]",
        );
    };
    if l.len() != i + 2 {
        return parse_err(
            Rule::UnknownHeaderKey,
            0,
            "a query header holds an unknown line [F14 §9.2]",
        );
    }
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() || text.contains('\0') || text.split('\n').any(|x| x.ends_with([' ', '\t']))
    {
        return parse_err(
            Rule::QueryPortability,
            0,
            "a query text is empty, holds U+0000, or has trailing white space [F14 §7.2.2]",
        );
    }
    check_definition(&name, params.as_deref(), shape, budget, text)?;
    Ok(Query::Def {
        name,
        lq,
        params,
        shape: shape.to_owned(),
        budget: budget.to_owned(),
        text: text.to_owned(),
    })
}

/// [F14 §7.2.2] consistency and portability of a definition: the text is a `define_stmt` whose `qname` is the file's
/// name, whose `param_decl` list renders to `params:` (§7.2.3; the line is absent exactly when there is no parameter),
/// whose `SHAPE` and `BUDGET` words equal `shape:` and `budget:` ASCII-case-insensitively (a text without `SHAPE` stores
/// `table`, [LQ/std §2.3], and one without `BUDGET` stores `medium`, [LQ/std §2.4]; spec sync 2b), and which holds no
/// node literal ([LQ/lexical §10.2] pre-check).
fn check_definition(
    name: &str,
    params: Option<&str>,
    shape: &str,
    budget: &str,
    text: &str,
) -> Result<()> {
    let bad = |m: String| parse_err(Rule::QueryConsistency, 0, format!("{m} [F14 §7.2.2]"));
    let unportable = |m: String| parse_err(Rule::QueryPortability, 0, format!("{m} [F14 §7.2.2]"));
    let h = match super::lq::define_head(text) {
        Ok(h) => h,
        Err(e) => return bad(format!("the query text is not a define_stmt: {e}")),
    };
    if h.name != name {
        return bad(format!("the define_stmt names {}, the file {name}", h.name));
    }
    let want = if h.params.is_empty() {
        None
    } else {
        Some(h.params.as_str())
    };
    if want != params {
        return bad(format!(
            "params: {params:?} is not the rendered param_decl list {want:?}"
        ));
    }
    if !h
        .shape
        .as_deref()
        .unwrap_or("table")
        .eq_ignore_ascii_case(shape)
    {
        return bad(format!("shape: {shape} differs from the SHAPE word"));
    }
    if !h
        .budget
        .as_deref()
        .unwrap_or("medium")
        .eq_ignore_ascii_case(budget)
    {
        return bad(format!("budget: {budget} differs from the BUDGET word"));
    }
    match super::lq::has_node_literal(text) {
        Ok(false) => Ok(()),
        Ok(true) => unportable("a node literal in the query text: not portable".into()),
        Err(e) => unportable(format!("the query text breaks LQ's lexical rules: {e}")),
    }
}

/// Encodes a named-query file.
pub fn encode_query(q: &Query) -> Vec<u8> {
    match q {
        Query::Def {
            name,
            lq,
            params,
            shape,
            budget,
            text,
        } => {
            let mut o = format!("moirai-query 1\nname: {}\nlq: {lq}\n", sval(name));
            if let Some(p) = params {
                o.push_str(&format!("params: {}\n", sval(p)));
            }
            o.push_str(&format!("shape: {shape}\nbudget: {budget}\n---\n{text}\n"));
            o.into_bytes()
        }
        Query::Conflict { name, class, sides } => {
            let s = |x: &Option<String>| x.as_deref().map(jstring).unwrap_or_default();
            format!(
                "moirai-query 1\nname: {}\nconflict definition class={class} base={} ours={} theirs={}\n",
                sval(name),
                s(&sides[0]),
                s(&sides[1]),
                s(&sides[2])
            )
            .into_bytes()
        }
    }
}

/// A checkpoint ref row file ([F14 §8]): (ref name, ref kind).
pub fn parse_refs(b: &[u8]) -> Result<(String, String)> {
    let l = lines_of(b)?;
    if l.len() != 2 || l[0] != "moirai-refs 1" {
        return parse_err(
            Rule::NoProduction,
            0,
            "a refs file is not `moirai-refs 1` and one row [F14 §8]",
        );
    }
    let Some(rest) = l[1].strip_prefix("ref ") else {
        return parse_err(
            Rule::NoProduction,
            0,
            "a refs row does not begin `ref ` [F14 §8]",
        );
    };
    let Some((name, kind)) = rest.rsplit_once(" kind=") else {
        return parse_err(Rule::NoProduction, 0, "a refs row lacks kind= [F14 §8]");
    };
    if name.is_empty() || name.contains(' ') || !["work", "plan", "tag"].contains(&kind) {
        return parse_err(
            Rule::NoProduction,
            0,
            "a refs row's name or kind breaks the grammar [F14 §8]",
        );
    }
    Ok((name.to_owned(), kind.to_owned()))
}

/// Encodes a refs row file.
pub fn encode_refs(name: &str, kind: &str) -> Vec<u8> {
    format!("moirai-refs 1\nref {name} kind={kind}\n").into_bytes()
}

/// `meta.moi` ([F14 §14.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// The store id.
    pub store_id: [u8; 16],
    /// `moi-format`.
    pub moi_format: u64,
    /// `full` or `hash-only`.
    pub anchor_text: String,
    /// `last-export-seq`.
    pub last_export_seq: u64,
    /// (ref, granularity), sorted by name.
    pub refs: Vec<(String, String)>,
}

/// Parses `meta.moi`.
pub fn parse_meta(b: &[u8]) -> Result<Meta> {
    let l = lines_of(b)?;
    if l.len() < 5 || l[0] != "moirai-meta 1" {
        return parse_err(
            Rule::NoProduction,
            0,
            "a meta file does not begin `moirai-meta 1` with four header lines [F14 §14.1]",
        );
    }
    let sid = l[1].strip_prefix("store-id: ").filter(|v| is_lhex(v, 32));
    let fmt = l[2].strip_prefix("moi-format: ").and_then(parse_dec);
    let at = l[3]
        .strip_prefix("anchor-text: ")
        .filter(|v| *v == "full" || *v == "hash-only");
    let les = l[4].strip_prefix("last-export-seq: ").and_then(parse_dec);
    let (Some(sid), Some(fmt), Some(at), Some(les)) = (sid, fmt, at, les) else {
        return parse_err(
            Rule::NoProduction,
            0,
            "a meta header line breaks the grammar [F14 §14.1]",
        );
    };
    let mut refs = Vec::new();
    for x in &l[5..] {
        let Some((n, g)) = x.strip_prefix("ref ").and_then(|r| r.rsplit_once(' ')) else {
            return parse_err(
                Rule::NoProduction,
                0,
                "a meta ref row breaks the grammar [F14 §14.1]",
            );
        };
        if !["checkpoint", "commit"].contains(&g) || n.is_empty() || n.contains(' ') {
            return parse_err(
                Rule::NoProduction,
                0,
                "a meta ref row breaks the grammar [F14 §14.1]",
            );
        }
        refs.push((n.to_owned(), g.to_owned()));
    }
    refs.sort();
    if refs.windows(2).any(|w| w[0].0 == w[1].0) {
        return parse_err(Rule::LineRepeated, 0, "a meta ref row repeated [F14 §14.1]");
    }
    Ok(Meta {
        store_id: crate::prim::unhex(sid)
            .expect("hex")
            .try_into()
            .expect("16"),
        moi_format: fmt,
        anchor_text: at.to_owned(),
        last_export_seq: les,
        refs,
    })
}

/// Encodes `meta.moi`.
pub fn encode_meta(m: &Meta) -> Vec<u8> {
    let mut l = vec![
        "moirai-meta 1".to_owned(),
        format!("store-id: {}", hex(&m.store_id)),
        format!("moi-format: {}", m.moi_format),
        format!("anchor-text: {}", m.anchor_text),
        format!("last-export-seq: {}", m.last_export_seq),
    ];
    l.extend(m.refs.iter().map(|(n, g)| format!("ref {n} {g}")));
    join(&l)
}

/// Parses `aliases/<h1>.moi` ([F14 §14.1]): (uid, #N) rows sorted by uid; `h1` checks the first byte when known.
pub fn parse_aliases(b: &[u8], h1: Option<u8>) -> Result<Vec<([u8; 16], u64)>> {
    let l = lines_of(b)?;
    if l.len() < 2 || l[0] != "moirai-aliases 1" {
        return parse_err(
            Rule::NoProduction,
            0,
            "an aliases file is not `moirai-aliases 1` and at least one row [F14 §14.1]",
        );
    }
    let mut rows = Vec::with_capacity(l.len() - 1);
    for x in &l[1..] {
        let Some((u, n)) = x.split_once(" #") else {
            return parse_err(
                Rule::NoProduction,
                0,
                "an aliases row is not `uid #N` [F14 §14.1]",
            );
        };
        let Some(n) = parse_dec(n).filter(|v| *v >= 1) else {
            return parse_err(
                Rule::NoProduction,
                0,
                "an aliases #N is not NZDIGIT *DIGIT [F14 §14.1]",
            );
        };
        if !is_lhex(u, 32) {
            return parse_err(
                Rule::NoProduction,
                0,
                "an aliases uid is not 32 lower-case hex [F14 §14.1]",
            );
        }
        let uid: [u8; 16] = crate::prim::unhex(u).expect("hex").try_into().expect("16");
        if h1.is_some_and(|h| h != uid[0]) {
            return parse_err(
                Rule::NoProduction,
                0,
                "an aliases uid does not begin with the file's byte [F14 §14.1]",
            );
        }
        rows.push((uid, n));
    }
    rows.sort();
    if rows.windows(2).any(|w| w[0].0 == w[1].0) {
        return parse_err(Rule::LineRepeated, 0, "an aliases uid repeated [F14 §14.1]");
    }
    Ok(rows)
}

/// Encodes an aliases file.
pub fn encode_aliases(rows: &[([u8; 16], u64)]) -> Vec<u8> {
    let mut l = vec!["moirai-aliases 1".to_owned()];
    l.extend(rows.iter().map(|(u, n)| format!("{} #{n}", hex(u))));
    join(&l)
}

/// Parses `ops.moi` ([F14 §14.2]); rows are kept in log order.
pub fn parse_ops(b: &[u8]) -> Result<Vec<String>> {
    let l = lines_of(b)?;
    if l.first().map(String::as_str) != Some("moirai-ops 1") {
        return parse_err(
            Rule::NoProduction,
            0,
            "an ops file does not begin `moirai-ops 1` [F14 §14.2]",
        );
    }
    let cid = |s: &str| s == "-" || parse_commit_id(s).is_some();
    for x in &l[1..] {
        let ok = if let Some(r) = x.strip_prefix("ref ") {
            let p: Vec<&str> = r.splitn(4, ' ').collect();
            p.len() == 4
                && ["create", "delete", "undo", "op-restore"].contains(&p[1])
                && cid(p[2])
                && {
                    let rest = p[3];
                    rest.split_once(' ').is_some_and(|(n, t)| {
                        cid(n) && {
                            let (tok, m, _) = read_token(t, 0).unwrap_or_default();
                            let _ = tok;
                            t.get(m..)
                                .and_then(|x| x.strip_prefix(' '))
                                .and_then(parse_dec)
                                .is_some()
                        }
                    })
                }
        } else if let Some(r) = x.strip_prefix("head ") {
            let key_rest = r
                .strip_prefix("client:")
                .or_else(|| r.strip_prefix("session:"));
            key_rest.is_some_and(|kr| {
                read_token(kr, 0).is_ok_and(|(_, m, _)| {
                    let t = &kr[m..];
                    let (action, hlc) = t.rsplit_once(' ').unwrap_or(("", ""));
                    parse_dec(hlc).is_some()
                        && (action == " remove"
                            || action
                                .strip_prefix(" set ")
                                .is_some_and(|x| !x.is_empty() && !x.contains(' ')))
                })
            })
        } else {
            false
        };
        if !ok {
            return parse_err(
                Rule::NoProduction,
                0,
                format!("an ops row breaks the grammar: {x:?} [F14 §14.2]"),
            );
        }
    }
    Ok(l[1..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [F14 §17.5] the named-query file and its `q`.
    #[test]
    fn query_example() {
        let src = "moirai-query 1\nname: lane_ready\nlq: 1\nparams: $scope: node = #u:018f3c2e7a117b3c9d5e4c2f1a0b9e09, $limit: int = 20\nshape: node\nbudget: light\n---\nDEFINE QUERY lane_ready($scope: node = #u:018f3c2e7a117b3c9d5e4c2f1a0b9e09, $limit: int = 20) SHAPE node BUDGET light AS {\n  MATCH (t:task)\n  WHERE t.ready AND t IN subtree($scope)\n  RETURN t ORDER BY t.priority, t.id LIMIT $limit\n}\n";
        assert_eq!(query_q("lane_ready"), "40cad467926a6dce0d6b5b93a4b8d71c");
        let q = parse_query(src.as_bytes(), Some("40cad467926a6dce0d6b5b93a4b8d71c")).unwrap();
        assert_eq!(encode_query(&q), src.as_bytes());
        assert!(parse_query(src.as_bytes(), Some("00000000000000000000000000000000")).is_err());
    }

    /// [F14 §17.7] `meta.moi`; §4 the marker; §8 a refs file.
    #[test]
    fn meta_marker_refs() {
        let src = "moirai-meta 1\nstore-id: 0123456789abcdef0123456789abcdef\nmoi-format: 1\nanchor-text: full\nlast-export-seq: 4471\nref lane/l5np checkpoint\nref main checkpoint\nref tags/v1 checkpoint\n";
        assert_eq!(
            encode_meta(&parse_meta(src.as_bytes()).unwrap()),
            src.as_bytes()
        );
        let m = b"moirai-image 1\nobject-format: sha256\nschema-version: 1\n";
        assert_eq!(encode_marker(&parse_marker(m).unwrap()), m);
        assert!(parse_marker(b"moirai-image 1\nobject-format: sha1\nschema-version: 3\n").is_err());
        assert_eq!(
            parse_refs(b"moirai-refs 1\nref main kind=work\n").unwrap(),
            ("main".into(), "work".into())
        );
    }

    /// [F14 §7.1]: rows of each class, a reading that needs JSON, canonical order.
    #[test]
    fn schema_rows() {
        let src = "moirai-schema 1\nedge spawns class=historical on-dst=tombstone on-src=retain acyclic=none card=many max-depth=0 derivation=none props=none lq=SPAWNS src=* dst=task reading=\"{a} spawns {b}\"\n";
        let rows = parse_schema_file(src.as_bytes()).unwrap();
        assert_eq!(encode_schema_file(&rows), src.as_bytes());
        let fields = "moirai-schema 1\nfield task effort type=int class=scalar storage=field decl=29 optional=true index=none coerce=none min=0 max=10\nvalue task effort_level high rank=2\n";
        let rows = parse_schema_file(fields.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(parse_schema_file(b"moirai-schema 1\nkind x existence=none\n").is_err());
    }

    /// [F14 §7.1] (spec sync 2b S2B-R-28): `policy <pname> <token>` rows in name order; a value that needs JSON; a
    /// repeated name is a line repeated (§9.2); `pname` is 1–16 lower-case segments of at most 64 bytes, 255 in all.
    #[test]
    fn policy_rows() {
        let src = "moirai-schema 1\npolicy merge.policy.task delete-wins\npolicy policy.role.developer.fields \"files_owned, title\"\n";
        let rows = parse_schema_file(src.as_bytes()).unwrap();
        assert_eq!(
            rows[1].keys,
            vec!["policy.role.developer.fields".to_owned()]
        );
        assert_eq!(rows[1].props[0].1, "files_owned, title");
        assert_eq!(encode_schema_file(&rows), src.as_bytes());
        let swapped = "moirai-schema 1\npolicy policy.self-claim-roles developer\npolicy merge.policy.task delete-wins\n";
        let rows = parse_schema_file(swapped.as_bytes()).unwrap();
        assert_eq!(rows[0].keys[0], "merge.policy.task");
        assert_ne!(encode_schema_file(&rows), swapped.as_bytes());
        let twice = "moirai-schema 1\npolicy merge.policy.task delete-wins\npolicy merge.policy.task resurrect\n";
        let e = parse_schema_file(twice.as_bytes()).unwrap_err();
        assert_eq!(e.rule, Some(Rule::LineRepeated));
        for name in ["a", "0x", "policy.role.dev_1.fields", "a-b.c_d"] {
            assert!(is_pname(name), "{name}");
        }
        let long_seg = "a".repeat(65);
        let many = vec!["a"; 17].join(".");
        let wide = vec!["abcdefgh"; 29].join(".");
        assert!(wide.len() > 255);
        for name in [
            "",
            "Policy.x",
            "a..b",
            "a.",
            ".a",
            "-a",
            "_a",
            "a b",
            long_seg.as_str(),
            many.as_str(),
            wide.as_str(),
        ] {
            assert!(!is_pname(name), "{name:?}");
        }
        for bad in [
            "moirai-schema 1\npolicy Merge.policy x\n",
            "moirai-schema 1\npolicy merge.policy.task\n",
            "moirai-schema 1\npolicy merge.policy.task a b\n",
            "moirai-schema 1\npolicy merge.policy.task %x\n",
        ] {
            assert_eq!(
                parse_schema_file(bad.as_bytes()).unwrap_err().rule,
                Some(Rule::NoProduction),
                "{bad:?}"
            );
        }
    }

    /// [F08 §8.5.1] (spec sync 2b S2B-F-19): a project kind's initial status is its non-retired status value with the
    /// least `rank`, ties by value name bytewise, whatever order the rows come in.
    #[test]
    fn project_initial_status() {
        let src = "moirai-schema 1\nkind incident derivation=random root-variant=none existence=resurrect\n\
                   value incident status alpha rank=3\nvalue incident status beta rank=0 retired\n\
                   value incident status open rank=1\nvalue incident status new rank=1\n";
        let rows = parse_schema_file(src.as_bytes()).unwrap();
        let mut schema = Schema::core();
        extend_schema(&mut schema, &rows).unwrap();
        assert_eq!(schema.kinds["incident"].initial, "new");
        assert_eq!(schema.kinds["incident"].statuses.len(), 4);
    }

    /// [F14 §7.2.2] (spec sync 2b S2B-R-18): a definition without `SHAPE` stores `table`, one without `BUDGET` stores
    /// `medium`; another word on the `shape:` or `budget:` line is a mismatch.
    #[test]
    fn default_shape_and_budget() {
        let file = |shape: &str, budget: &str| {
            format!(
                "moirai-query 1\nname: q\nlq: 1\nshape: {shape}\nbudget: {budget}\n---\nDEFINE QUERY q() AS {{\n  MATCH (n:note)\n  RETURN n\n}}\n"
            )
        };
        assert!(parse_query(file("table", "medium").as_bytes(), None).is_ok());
        for (shape, budget) in [("node", "medium"), ("table", "light")] {
            let e = parse_query(file(shape, budget).as_bytes(), None).unwrap_err();
            assert_eq!(e.rule, Some(Rule::QueryConsistency), "{shape} {budget}");
        }
    }
}
