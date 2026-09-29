//! [F14 §6] node files: the grammar of §6.1, the header lines (§6.2), provenance (§6.3), field and label lines (§6.4),
//! ledgers (§6.5), edges (§6.6), anchors (§6.7), conflicts with existence sides (§6.8), the body (§6.9), tombstones
//! (§6.10) and R4's file and root nodes (§6.11); values in their text forms (§5).
//!
//! [`parse`] reads the importer's superset (§9.1) and applies every `ImageParse` rule of §9.2 that one file and the
//! effective schema decide; [`encode`] writes the exporter's canonical bytes. A file is canonical when
//! `encode(parse(f)) == f`.

use std::collections::{BTreeMap, BTreeSet};

use super::schema::{Elem, FieldDef, Props, Schema, Storage, Sub, Ty};
use super::text::*;
use crate::prim::{Oid, Result, blake3_128, hex, unhex};
use crate::value::{Scope, ScopeSeg, Window};

/// A provenance value ([F14 §6.3]): a commit id and its time text as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prov {
    /// The commit.
    pub commit: [u8; 32],
    /// The `rfc3339ms` text, when written.
    pub time: Option<String>,
}

/// A `path` in its image text form ([F14 §5.2]).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IPath {
    /// The explicit root name; `None` when the root is implied.
    pub root: Option<String>,
    /// The exact path text.
    pub text: String,
}

/// A `pathmove` ([F14 §5.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IPathMove {
    /// The candidate HLC.
    pub hlc: u64,
    /// The class name.
    pub class: String,
    /// Directory prefix moved from.
    pub from: IPath,
    /// Directory prefix moved to.
    pub to: IPath,
    /// The git commit, or `none`.
    pub git: Oid,
}

/// An image value ([F14 §5.1]).
#[derive(Clone, Debug, PartialEq)]
pub enum IVal {
    /// `bool`.
    Bool(bool),
    /// `int`.
    Int(i64),
    /// `f64`.
    F64(f64),
    /// `enum` by name.
    Enum(String),
    /// `text` or `sym`.
    Text(String),
    /// `ref` by uid.
    Ref([u8; 16]),
    /// `commitref`.
    CommitRef([u8; 32]),
    /// `path`.
    Path(IPath),
    /// `oid`.
    Oid(Oid),
    /// `pathmove`.
    PathMove(Box<IPathMove>),
    /// `set`, elements in written order.
    Set(Vec<IVal>),
}

/// An `edge` line ([F14 §6.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Stored edge-kind name.
    pub kind: String,
    /// Destination uid.
    pub dst: [u8; 16],
    /// `pin=`.
    pub pin: Option<[u8; 32]>,
    /// `flagged`.
    pub flagged: bool,
}

/// The properties of an `anchor` line ([F14 §6.7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AProps {
    /// `kind`.
    pub kind: String,
    /// `mode`.
    pub mode: String,
    /// `watch`.
    pub watch: String,
    /// `scope` decoded to [F08 §10.3.1]'s value.
    pub scope: Option<Scope>,
    /// `quote_h`, `prefix_h`, `suffix_h`.
    pub digests: Option<[[u8; 16]; 3]>,
    /// `end_h`.
    pub end_h: Option<[u8; 16]>,
    /// `quote`, `prefix`, `suffix` texts.
    pub texts: Option<[Vec<u8>; 3]>,
    /// `end` text.
    pub end: Option<Vec<u8>>,
    /// `occurrence`.
    pub occurrence: Option<u16>,
    /// `hint`.
    pub hint: Option<(u32, u32)>,
    /// `window` bytes.
    pub window: Option<Vec<u8>>,
    /// `span` value.
    pub span: Option<u64>,
    /// `blob`.
    pub blob: Option<Oid>,
    /// `git`.
    pub git: Option<Oid>,
    /// `captured`.
    pub captured: [u8; 16],
    /// `pred`.
    pub pred: Option<[u8; 16]>,
    /// `marker`.
    pub marker: Option<String>,
    /// `v`.
    pub v: u64,
}

/// An `anchor` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorLine {
    /// The anchor uid.
    pub uid: [u8; 16],
    /// The file node's uid.
    pub dst: [u8; 16],
    /// Its properties.
    pub props: AProps,
}

/// One side of a conflict line ([F14 §6.8]), by key.
#[derive(Clone, Debug, PartialEq)]
pub enum Side {
    /// `field.<name>`: the value.
    Field(IVal),
    /// `status`: (status, resolution).
    Status(String, Option<String>),
    /// `parent`: (parent uid, order).
    Parent(Option<[u8; 16]>, Option<String>),
    /// `body`: the body text.
    Body(String),
    /// `edge.<kind>.<dst>`: `present`, `pin=…` or `flagged`.
    Edge(Option<[u8; 32]>, bool),
    /// `edge.at.<dst>.<anchor>`: the anchor's properties.
    Anchor(Box<AProps>),
    /// `observation`: the six members, absent ones left out.
    Observation(BTreeMap<String, IVal>),
    /// `existence`: the side's text.
    Existence(Box<ExSide>),
}

/// An existence side ([F14 §6.8.1]).
#[derive(Clone, Debug, PartialEq)]
pub enum ExSide {
    /// `live <kind>` with its snapshot.
    Live {
        /// Kind.
        kind: String,
        /// (status, resolution); `None` = the initial status with `none`.
        status: Option<(String, Option<String>)>,
        /// Title and field keys by name.
        fields: BTreeMap<String, IVal>,
        /// Labels.
        labels: BTreeSet<String>,
        /// Counter totals.
        totals: BTreeMap<String, i64>,
        /// Body.
        body: Option<String>,
    },
    /// `deleted <kind>` with reason and replacement.
    Deleted {
        /// Kind.
        kind: String,
        /// Reason.
        reason: Option<String>,
        /// Replacement uid.
        replaced_by: Option<[u8; 16]>,
    },
}

/// A conflict line.
#[derive(Clone, Debug, PartialEq)]
pub struct ConflictLine {
    /// The class name.
    pub class: String,
    /// base, ours, theirs; `None` = absent.
    pub sides: [Option<Side>; 3],
}

/// A parsed node file.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NodeFile {
    /// `uid:`.
    pub uid: [u8; 16],
    /// `kind:`.
    pub kind: String,
    /// The tombstone form.
    pub tomb: bool,
    /// `title:`.
    pub title: Option<String>,
    /// (status, resolution); `None` = the initial status with `none`.
    pub status: Option<(String, Option<String>)>,
    /// Non-default header enumerations by field name.
    pub headers: BTreeMap<String, String>,
    /// `parent:`.
    pub parent: Option<[u8; 16]>,
    /// `order:`.
    pub order: Option<String>,
    /// `created:`.
    pub created: Option<Prov>,
    /// `updated:`.
    pub updated: Option<Prov>,
    /// `deleted:`.
    pub deleted: Option<Prov>,
    /// `flags:`.
    pub flags: BTreeSet<String>,
    /// `field` lines of a live file, by name.
    pub fields: BTreeMap<String, IVal>,
    /// `label` lines.
    pub labels: BTreeSet<String>,
    /// `incr` lines (field, delta, token), sorted.
    pub ledger: Vec<(String, i128, String)>,
    /// `edge` lines, sorted.
    pub edges: Vec<Edge>,
    /// `anchor` lines, sorted.
    pub anchors: Vec<AnchorLine>,
    /// `conflict` lines by key text.
    pub conflicts: BTreeMap<String, ConflictLine>,
    /// The body.
    pub body: Option<String>,
    /// A tombstone's `field reason:`.
    pub t_reason: Option<String>,
    /// A tombstone's `field replaced_by:`.
    pub t_replaced: Option<[u8; 16]>,
}

const OBS: [&str; 6] = [
    "path",
    "oid",
    "bytes",
    "observed_git",
    "observed_blob",
    "relink",
];
const HEADER_ENUMS: [&str; 4] = ["priority", "criticality", "confidence", "authority"];
const CLASSES: [&str; 7] = [
    "FieldEdit",
    "StatusFork",
    "TextHunk",
    "DeleteVsModify",
    "SupersedeFork",
    "OwnerFieldEdited",
    "PathClaim",
];
const AKINDS: [&str; 6] = ["file", "heading", "symbol", "quote", "range", "lines"];

fn parse_uid(s: &str, at: usize) -> Result<[u8; 16]> {
    if !is_lhex(s, 32) {
        return parse_err(
            Rule::NoProduction,
            at,
            format!("{s:?} is not a uid of 32 lower-case hex digits [F14 §2.3]"),
        );
    }
    Ok(unhex(s).expect("hex").try_into().expect("16"))
}

fn parse_prov(s: &str, at: usize) -> Result<Prov> {
    let (c, t) = s.split_once(' ').map_or((s, None), |(a, b)| (a, Some(b)));
    let Some(commit) = parse_commit_id(c) else {
        return parse_err(
            Rule::NoProduction,
            at,
            "a provenance commit id is not c + 64 lower-case hex [F14 §6.3]",
        );
    };
    if let Some(t) = t
        && !is_rfc3339ms(t)
    {
        return parse_err(
            Rule::NoProduction,
            at,
            "a provenance time is not rfc3339ms [F14 §2.7]",
        );
    }
    Ok(Prov {
        commit,
        time: t.map(str::to_owned),
    })
}

fn prov_text(p: &Prov) -> String {
    match &p.time {
        Some(t) => format!("c{} {t}", hex(&p.commit)),
        None => format!("c{}", hex(&p.commit)),
    }
}

/// [F08 §5.4.1] `RelPath` rules for a path of a root other than `abs`; `abs`: non-empty without control characters.
fn check_path_text(root: Option<&str>, t: &str, at: usize) -> Result<()> {
    if t.is_empty() || t.chars().any(|c| (c as u32) < 0x20) {
        return parse_err(
            Rule::PathRules,
            at,
            "a path is empty or holds a C0 control character [F18 §2.8]",
        );
    }
    if root == Some("abs") {
        return Ok(());
    }
    if t.starts_with('/')
        || t.ends_with('/')
        || t.contains('\\')
        || t.split('/').any(|s| s.is_empty() || s == "." || s == "..")
    {
        return parse_err(
            Rule::PathRules,
            at,
            format!("{t:?} breaks the RelPath rules [F18 §2.8], [F08 §5.4.1]"),
        );
    }
    Ok(())
}

fn parse_path(t: &str, implied: bool, at: usize) -> Result<IPath> {
    if implied {
        check_path_text(None, t, at)?;
        return Ok(IPath {
            root: None,
            text: t.to_owned(),
        });
    }
    let Some((root, text)) = t.split_once(':') else {
        return parse_err(
            Rule::PathRules,
            at,
            "an explicit-root path has no `<root>:` [F14 §5.2]",
        );
    };
    if !is_rootname(root) {
        return parse_err(
            Rule::PathRules,
            at,
            "an explicit root is not a root name [F08 §5.4.1]",
        );
    }
    check_path_text(Some(root), text, at)?;
    Ok(IPath {
        root: Some(root.to_owned()),
        text: text.to_owned(),
    })
}

fn path_text(p: &IPath) -> String {
    match &p.root {
        Some(r) => format!("{r}:{}", p.text),
        None => p.text.clone(),
    }
}

/// A directory prefix: a RelPath followed by one `/` ([F08 §5.4.2]).
fn parse_prefix(t: &str, implied: bool, at: usize) -> Result<IPath> {
    let Some(body) = t.strip_suffix('/') else {
        return parse_err(
            Rule::ValueType,
            at,
            "a pathmove prefix does not end in / [F14 §5.2]",
        );
    };
    let p = parse_path(body, implied, at)?;
    if p.root.as_deref() == Some("abs") {
        return parse_err(
            Rule::ValueConstraint,
            at,
            "a pathmove prefix of root abs [F08 §5.4.2]",
        );
    }
    Ok(IPath {
        root: p.root,
        text: format!("{}/", p.text),
    })
}

fn parse_pathmove(s: &str, implied: bool, at: usize) -> Result<(IPathMove, usize)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'[') {
        return parse_err(
            Rule::ValueType,
            at,
            "a pathmove is not a JSON array [F14 §5.2]",
        );
    }
    let mut i = 1;
    let mut parts = Vec::with_capacity(5);
    for k in 0..5 {
        let (t, n) = read_jstring(&s[i..], at + i)?;
        parts.push(t);
        i += n;
        let want = if k < 4 { b',' } else { b']' };
        if b.get(i) != Some(&want) {
            return parse_err(
                Rule::ValueType,
                at + i,
                "a pathmove array breaks v-pathmove (no white space, five strings) [F14 §5.2]",
            );
        }
        i += 1;
    }
    if parts[0].len() != 20 || !parts[0].bytes().all(|c| c.is_ascii_digit()) {
        return parse_err(
            Rule::ValueType,
            at,
            "a pathmove hlc is not 20 decimal digits [F14 §5.2]",
        );
    }
    let Ok(hlc) = parts[0].parse::<u64>() else {
        return parse_err(Rule::ValueType, at, "a pathmove hlc exceeds u64 [F14 §5.2]");
    };
    if !["explicit", "confirmed", "committed", "observed"].contains(&parts[1].as_str()) {
        return parse_err(
            Rule::ValueType,
            at,
            "a pathmove class is not a class name [F14 §5.2]",
        );
    }
    let from = parse_prefix(&parts[2], implied, at)?;
    let to = parse_prefix(&parts[3], implied, at)?;
    if from.root != to.root || from.text == to.text {
        return parse_err(
            Rule::ValueConstraint,
            at,
            "pathmove from and to differ in root or are equal [F08 §5.2]",
        );
    }
    let git = if parts[4].is_empty() {
        Oid::None
    } else {
        match parse_oid_text(&parts[4]) {
            Some(o) => o,
            None => {
                return parse_err(
                    Rule::ValueType,
                    at,
                    "a pathmove git is not an oid-text [F14 §5.2]",
                );
            }
        }
    };
    Ok((
        IPathMove {
            hlc,
            class: parts[1].clone(),
            from,
            to,
            git,
        },
        i,
    ))
}

fn pathmove_text(m: &IPathMove) -> String {
    format!(
        "[{},{},{},{},{}]",
        jstring(&format!("{:020}", m.hlc)),
        jstring(&m.class),
        jstring(&path_text(&m.from)),
        jstring(&path_text(&m.to)),
        jstring(&oid_text(&m.git))
    )
}

/// The context a value is parsed in: its field and whether its paths carry an implied root.
#[derive(Clone, Copy)]
struct VCtx<'a> {
    field: &'a FieldDef,
    implied: bool,
    values: &'a [&'a str],
}

fn elem_ty(e: Elem) -> Ty {
    match e {
        Elem::Int => Ty::Int,
        Elem::Enum => Ty::Enum,
        Elem::Text => Ty::Text,
        Elem::Ref => Ty::Ref,
        Elem::CommitRef => Ty::CommitRef,
        Elem::Path => Ty::Path,
        Elem::Oid => Ty::Oid,
        Elem::PathMove => Ty::PathMove,
    }
}

/// Parses the text of one single-line form of type `ty` ([F14 §5.1]).
fn parse_single(ty: Ty, t: &str, c: VCtx<'_>, at: usize) -> Result<IVal> {
    let refuse = |rule: Rule, what: &str| {
        parse_err(
            rule,
            at,
            format!(
                "{t:?} is not a {what} value of field {} [F14 §5.1]",
                c.field.name
            ),
        )
    };
    let bad = |what: &str| refuse(Rule::ValueType, what);
    Ok(match ty {
        Ty::Bool => match t {
            "true" => IVal::Bool(true),
            "false" => IVal::Bool(false),
            _ => return bad("bool"),
        },
        Ty::Int | Ty::Counter => match parse_sdec(t) {
            Some(v) => IVal::Int(v),
            // [F14 §9.2]: an integer written by `sdec` that leaves `i64` is an out-of-range number.
            None if is_sdec_text(t) => return refuse(Rule::NumberRange, "int (it leaves i64)"),
            None => return bad("int"),
        },
        Ty::F64 => IVal::F64(parse_f64(t, at)?),
        Ty::Enum => {
            if !is_vname(t) {
                return bad("enumeration");
            }
            if !c.values.is_empty() && !c.values.contains(&t) {
                return refuse(Rule::UnknownName, "known enumeration");
            }
            IVal::Enum(t.to_owned())
        }
        Ty::Text => IVal::Text(t.to_owned()),
        Ty::Ref => IVal::Ref(parse_uid(t, at)?),
        Ty::CommitRef => match parse_commit_id(t) {
            Some(x) => IVal::CommitRef(x),
            None => return bad("commitref"),
        },
        Ty::Path => IVal::Path(parse_path(t, c.implied, at)?),
        Ty::Oid => match parse_oid_text(t) {
            Some(o) => IVal::Oid(o),
            None => return bad("oid"),
        },
        Ty::PathMove => {
            let (m, n) = parse_pathmove(t, c.implied, at)?;
            if n != t.len() {
                return bad("pathmove");
            }
            IVal::PathMove(Box::new(m))
        }
        Ty::Set(_) => return bad("set element"),
    })
}

/// The single-line form of a value ([F14 §5.1]).
fn single_text(v: &IVal) -> String {
    match v {
        IVal::Bool(b) => b.to_string(),
        IVal::Int(i) => i.to_string(),
        IVal::F64(x) => f64_text(*x),
        IVal::Enum(s) | IVal::Text(s) => s.clone(),
        IVal::Ref(u) => hex(u),
        IVal::CommitRef(c) => format!("c{}", hex(c)),
        IVal::Path(p) => path_text(p),
        IVal::Oid(o) => oid_text(o),
        IVal::PathMove(m) => pathmove_text(m),
        IVal::Set(v) => set_text(v),
    }
}

fn is_textish(v: &IVal) -> bool {
    matches!(v, IVal::Text(_) | IVal::Path(_))
}

fn set_elem_text(v: &IVal) -> String {
    let t = single_text(v);
    let bare = !t.is_empty()
        && !t.contains([',', '[', ']'])
        && !t.chars().any(is_control)
        && !t.starts_with(' ')
        && !t.ends_with(' ')
        && !t.starts_with('"');
    if !is_textish(v) || bare {
        t
    } else {
        jstring(&t)
    }
}

fn set_text(v: &[IVal]) -> String {
    let mut els: Vec<String> = v.iter().map(set_elem_text).collect();
    els.sort();
    format!("[{}]", els.join(", "))
}

fn parse_set(s: &str, el: Elem, c: VCtx<'_>, at: usize) -> Result<Vec<IVal>> {
    let b = s.as_bytes();
    if b.first() != Some(&b'[') || b.last() != Some(&b']') {
        return parse_err(Rule::ValueType, at, "a set is not [ … ] [F14 §5.3]");
    }
    let inner = &s[1..s.len() - 1];
    let mut out = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        let (t, n) = if inner[i..].starts_with('"') {
            read_jstring(&inner[i..], at + 1 + i)?
        } else {
            let n = inner[i..].find([',', ']']).unwrap_or(inner.len() - i);
            (inner[i..i + n].to_owned(), n)
        };
        if t.is_empty() && !inner[i..].starts_with('"') {
            return parse_err(
                Rule::NoProduction,
                at + 1 + i,
                "an empty bare set element [F14 §5.3]",
            );
        }
        out.push(parse_single(elem_ty(el), &t, c, at + 1 + i)?);
        i += n;
        if i < inner.len() {
            if !inner[i..].starts_with(", ") {
                return parse_err(
                    Rule::ValueType,
                    at + 1 + i,
                    "set elements are separated by `, ` [F14 §5.3]",
                );
            }
            i += 2;
            if i == inner.len() {
                return parse_err(
                    Rule::ValueType,
                    at + 1 + i,
                    "a set ends with a separator [F14 §5.3]",
                );
            }
        }
    }
    // [F14 §5.3]: the importer re-sorts the elements into the written order ([F07] open point 25).
    let mut keyed: Vec<(String, IVal)> = out.into_iter().map(|v| (set_elem_text(&v), v)).collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    if keyed.windows(2).any(|w| w[0].0 == w[1].0) {
        return parse_err(
            Rule::ValueConstraint,
            at,
            "a set holds one element twice [F14 §5.3]",
        );
    }
    Ok(keyed.into_iter().map(|(_, v)| v).collect())
}

/// [F08 §5.4.3] the path-glob grammar.
fn is_glob(g: &str) -> bool {
    if g.is_empty() {
        return false;
    }
    g.split('/').all(|seg| {
        if seg == "**" {
            return true;
        }
        if seg.is_empty() || seg.contains("**") {
            return false;
        }
        let cs: Vec<char> = seg.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if (c as u32) < 0x20 || c == '\\' {
                return false;
            }
            if c == '[' {
                let mut j = i + 1;
                if cs.get(j) == Some(&'!') {
                    j += 1;
                }
                let start = j;
                while j < cs.len() && cs[j] != ']' {
                    if (cs[j] as u32) < 0x20 || cs[j] == '\\' || cs[j] == '/' {
                        return false;
                    }
                    if cs.get(j + 1) == Some(&'-') && cs.get(j + 2).is_some_and(|x| *x != ']') {
                        if cs[j + 2] < cs[j] {
                            return false;
                        }
                        j += 3;
                    } else {
                        j += 1;
                    }
                }
                if j == start || j >= cs.len() {
                    return false;
                }
                i = j + 1;
            } else if c == ']' {
                return false;
            } else {
                i += 1;
            }
        }
        true
    })
}

/// [F08 §5.4.5] record-list shapes: (member count, required members).
fn record_list_ok(which: &str, t: &str) -> bool {
    let (n, req): (usize, &[usize]) = match which {
        "targets" => (4, &[0, 1]),
        "readiness" => (3, &[0]),
        "alternatives" => (5, &[0]),
        _ => (1, &[0]),
    };
    t.split('\n').all(|rec| {
        let m: Vec<&str> = rec.split('\t').collect();
        m.len() == n
            && req.iter().all(|&i| !m[i].is_empty())
            && m.iter().all(|x| !x.chars().any(is_control))
            && match which {
                "targets" => {
                    parse_f64(m[1], 0).is_ok() && ["", "<", "<=", "=", ">=", ">"].contains(&m[3])
                }
                "alternatives" => {
                    m[2].is_empty() || m[2].strip_prefix("#u:").is_some_and(|h| is_lhex(h, 32))
                }
                _ => true,
            }
    })
}

/// Field constraints of [F08 §5.3], §5.4, §9.3 ([F14 §9.2] "breaks the field's constraints").
fn check_value(v: &IVal, c: VCtx<'_>, at: usize) -> Result<()> {
    let f = c.field;
    let text_rule = |t: &str| -> Result<()> {
        if t.contains('\0') || t.contains('\r') || (f.one_line && t.contains('\n')) {
            return parse_err(
                Rule::ValueConstraint,
                at,
                format!(
                    "field {} holds U+0000, CR, or LF in a one-line value [F08 §5.3]",
                    f.name
                ),
            );
        }
        Ok(())
    };
    match v {
        IVal::Int(i) => {
            if let Some((lo, hi)) = f.range
                && (*i < lo || *i > hi)
            {
                return parse_err(
                    Rule::ValueConstraint,
                    at,
                    format!("field {} value {i} outside {lo}..={hi} [F08 §9.3]", f.name),
                );
            }
        }
        IVal::Text(t) => {
            text_rule(t)?;
            let ok = match f.sub {
                Sub::RecordList(w) => record_list_ok(w, t),
                Sub::Globs => is_glob(t),
                Sub::Tagged => {
                    ["role:", "phase:", "lane:"].iter().any(|p| {
                        t.strip_prefix(p).is_some_and(|r| {
                            !r.is_empty()
                                && r.len() <= 64
                                && r.bytes().all(|b| {
                                    b.is_ascii_lowercase()
                                        || b.is_ascii_digit()
                                        || b"_./-".contains(&b)
                                })
                        })
                    }) || t.strip_prefix("path:").is_some_and(is_glob)
                }
                Sub::RootName => is_rootname(t),
                Sub::Ascii => t.bytes().all(|b| (0x20..0x7F).contains(&b)),
                Sub::Relink => is_relink(t),
                _ => true,
            };
            if !ok {
                // [F14 §9.2]: a `relink` outside R-17's grammar is its own clause ([F18 §5.7]).
                let rule = if f.sub == Sub::Relink {
                    Rule::RelinkGrammar
                } else {
                    Rule::ValueConstraint
                };
                return parse_err(
                    rule,
                    at,
                    format!(
                        "field {} value {t:?} breaks its constraint [F08 §5.4]",
                        f.name
                    ),
                );
            }
        }
        IVal::Path(p) => {
            if f.sub == Sub::AbsPath && p.root.as_deref() != Some("abs") {
                return parse_err(
                    Rule::PathRules,
                    at,
                    format!("field {} must have root abs [F08 §9.3]", f.name),
                );
            }
        }
        IVal::Set(els) => {
            for e in els {
                check_value(e, c, at)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Reads a field line's value (after `: `) by type ([F14 §6.4]).
fn parse_fval(s: &str, c: VCtx<'_>, at: usize) -> Result<Option<IVal>> {
    let v = match c.field.ty {
        Ty::Set(el) => {
            if !s.starts_with('[') {
                return parse_err(Rule::ValueType, at, "a set value must be [ … ] [F14 §5.3]");
            }
            let els = parse_set(s, el, c, at)?;
            if els.is_empty() {
                return Ok(None);
            }
            IVal::Set(els)
        }
        Ty::PathMove => parse_single(Ty::PathMove, s, c, at)?,
        ty => {
            let t = read_sval(s, at)?;
            if t.is_empty() {
                return Ok(None);
            }
            parse_single(ty, &t, c, at)?
        }
    };
    check_value(&v, c, at)?;
    Ok(Some(v))
}

fn is_default(v: &IVal, f: &FieldDef) -> bool {
    f.default.as_deref().is_some_and(|d| single_text(v) == d)
}

/// [F14 §5.6] the scope text to [F08 §10.3.1]'s value.
fn parse_scope(t: &str, at: usize) -> Result<Scope> {
    let Some((lang, rest)) = t.split_once(':') else {
        return parse_err(
            Rule::ScopeText,
            at,
            "a scope text has no language [F14 §5.6]",
        );
    };
    let (lang_b, kinds): (u8, &[&str]) = match lang {
        "rust" => (
            1,
            &[
                "mod",
                "impl",
                "fn",
                "struct",
                "enum",
                "trait",
                "const",
                "static",
                "macro_rules",
            ],
        ),
        "markdown" => (2, &["h1", "h2", "h3", "h4", "h5", "h6"]),
        "toml" => (3, &["table", "array_table", "key"]),
        _ => {
            return parse_err(
                Rule::ScopeText,
                at,
                "a scope language is not rust, markdown or toml [F14 §5.6]",
            );
        }
    };
    let unpct = |s: &str| -> Result<String> {
        let b = s.as_bytes();
        let mut o = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'%' => {
                    let h = s.get(i + 1..i + 3).filter(|h| is_lhex(h, 2));
                    let Some(h) = h else {
                        return parse_err(
                            Rule::ScopeText,
                            at,
                            "a scope %-escape is not two lower-case hex digits [F14 §5.6]",
                        );
                    };
                    let v = u8::from_str_radix(h, 16).expect("hex");
                    if !(v == b'%' || v == b'/' || v == b'[' || v == b']' || v < 0x20 || v == 0x7F)
                    {
                        return parse_err(
                            Rule::ScopeText,
                            at,
                            "a scope %-escape of a byte written as itself [F14 §5.6]",
                        );
                    }
                    o.push(v);
                    i += 3;
                }
                b'/' | b'[' | b']' => {
                    return parse_err(
                        Rule::ScopeText,
                        at,
                        "an unescaped / [ ] in a scope name [F14 §5.6]",
                    );
                }
                c => {
                    o.push(c);
                    i += 1;
                }
            }
        }
        String::from_utf8(o).or_else(|_| {
            parse_err(
                Rule::ScopeText,
                at,
                "a scope name is not UTF-8 [F08 §10.3.1]",
            )
        })
    };
    let mut segments = Vec::new();
    for seg in rest.split('/') {
        let Some((sk, tail)) = seg.split_once(' ') else {
            return parse_err(
                Rule::ScopeText,
                at,
                "a scope segment has no SP after its kind [F14 §5.6]",
            );
        };
        let Some(skind) = kinds.iter().position(|k| *k == sk) else {
            return parse_err(
                Rule::ScopeText,
                at,
                "a scope segment kind is not one of its language [F14 §5.6]",
            );
        };
        let (name, qual) = match tail.strip_suffix(']').and_then(|x| x.rsplit_once('[')) {
            Some((n, q)) => (unpct(n)?, unpct(q)?),
            None => (unpct(tail)?, String::new()),
        };
        if name.is_empty() || (tail.ends_with(']') && qual.is_empty()) {
            return parse_err(
                Rule::ScopeText,
                at,
                "a scope name or written qualifier is empty [F14 §5.6]",
            );
        }
        crate::value::text_rules(&name, at, true).map_err(|e| e.or_rule(Rule::ScopeText))?;
        crate::value::text_rules(&qual, at, true).map_err(|e| e.or_rule(Rule::ScopeText))?;
        segments.push(ScopeSeg {
            skind: skind as u8 + 1,
            name,
            qual,
        });
    }
    if segments.is_empty() || segments.len() > 64 {
        return parse_err(
            Rule::ScopeText,
            at,
            "a scope has 0 or more than 64 segments [F08 §10.3.1]",
        );
    }
    Ok(Scope {
        lang: lang_b,
        segments,
    })
}

fn scope_text(s: &Scope) -> String {
    let pct = |t: &str| -> String {
        let mut o = Vec::with_capacity(t.len());
        for &b in t.as_bytes() {
            if b == b'%' || b == b'/' || b == b'[' || b == b']' || b < 0x20 || b == 0x7F {
                o.extend_from_slice(format!("%{b:02x}").as_bytes());
            } else {
                o.push(b);
            }
        }
        String::from_utf8(o).expect("escaping keeps UTF-8")
    };
    let (lang, kinds): (&str, &[&str]) = match s.lang {
        1 => (
            "rust",
            &[
                "mod",
                "impl",
                "fn",
                "struct",
                "enum",
                "trait",
                "const",
                "static",
                "macro_rules",
            ],
        ),
        2 => ("markdown", &["h1", "h2", "h3", "h4", "h5", "h6"]),
        _ => ("toml", &["table", "array_table", "key"]),
    };
    let segs: Vec<String> = s
        .segments
        .iter()
        .map(|g| {
            let q = if g.qual.is_empty() {
                String::new()
            } else {
                format!("[{}]", pct(&g.qual))
            };
            format!("{} {}{q}", kinds[usize::from(g.skind) - 1], pct(&g.name))
        })
        .collect();
    format!("{lang}:{}", segs.join("/"))
}

fn atext(b: &[u8]) -> String {
    match core::str::from_utf8(b) {
        Ok(s) if !s.is_empty() => token(s),
        Ok(_) => "\"\"".into(),
        Err(_) => format!("%{}", b64url(b)),
    }
}

fn read_atext(s: &str, at: usize) -> Result<(Vec<u8>, usize)> {
    if let Some(r) = s.strip_prefix('%') {
        let n = r.find(' ').unwrap_or(r.len());
        let b = un_b64url(&r[..n], at + 1)?;
        if core::str::from_utf8(&b).is_ok() {
            return parse_err(
                Rule::NoProduction,
                at,
                "an anchor text in % form is valid UTF-8 [F14 §5.7]",
            );
        }
        Ok((b, n + 1))
    } else {
        let (t, n, _) = read_token(s, at)?;
        Ok((t.into_bytes(), n))
    }
}

/// A cursor over an `a-props` text ([F14 §6.7]).
struct PropCursor<'a> {
    rest: &'a str,
    pos: usize,
}

impl<'a> PropCursor<'a> {
    fn take(&mut self, key: &str) -> bool {
        let p = format!(" {key}=");
        match self.rest.strip_prefix(p.as_str()) {
            Some(r) => {
                self.rest = r;
                self.pos += p.len();
                true
            }
            None => false,
        }
    }

    fn word(&mut self) -> &'a str {
        let n = self.rest.find(' ').unwrap_or(self.rest.len());
        let w = &self.rest[..n];
        self.rest = &self.rest[n..];
        self.pos += n;
        w
    }

    fn advance(&mut self, n: usize) {
        let n = n.min(self.rest.len());
        self.rest = &self.rest[n..];
        self.pos += n;
    }

    fn token(&mut self) -> Result<String> {
        let (t, n, _) = read_token(self.rest, self.pos)?;
        self.advance(n);
        Ok(t)
    }

    fn atext(&mut self) -> Result<Vec<u8>> {
        let (b, n) = read_atext(self.rest, self.pos)?;
        self.advance(n);
        Ok(b)
    }

    fn digest(&mut self) -> Result<[u8; 16]> {
        let p = self.pos;
        let w = self.word();
        parse_uid(w, p)
    }
}

/// Parses `a-props` ([F14 §6.7]) from `s`, which begins with ` kind=`.
fn parse_aprops(s: &str, at: usize) -> Result<AProps> {
    let mut c = PropCursor { rest: s, pos: at };
    let fail = |rule: Rule, m: &str| {
        parse_err::<AProps>(rule, at, format!("anchor properties: {m} [F14 §6.7]"))
    };
    if !c.take("kind") {
        return fail(Rule::NoProduction, "kind= missing");
    }
    let kind = c.word().to_owned();
    if !AKINDS.contains(&kind.as_str()) {
        return fail(Rule::UnknownName, "unknown kind");
    }
    if !c.take("mode") {
        return fail(Rule::NoProduction, "mode= missing");
    }
    let mode = c.word().to_owned();
    if !c.take("watch") {
        return fail(Rule::NoProduction, "watch= missing");
    }
    let watch = c.word().to_owned();
    if !["live", "pinned"].contains(&mode.as_str()) || !["header", "span"].contains(&watch.as_str())
    {
        return fail(Rule::NoProduction, "mode or watch invalid");
    }
    let scope = if c.take("scope") {
        let p = c.pos;
        let t = c.token()?;
        Some(parse_scope(&t, p)?)
    } else {
        None
    };
    let digests = if c.take("quote_h") {
        let a = c.digest()?;
        if !c.take("prefix_h") {
            return fail(Rule::AnchorDigests, "prefix_h= missing after quote_h");
        }
        let b = c.digest()?;
        if !c.take("suffix_h") {
            return fail(Rule::AnchorDigests, "suffix_h= missing after prefix_h");
        }
        Some([a, b, c.digest()?])
    } else {
        None
    };
    let end_h = if c.take("end_h") {
        Some(c.digest()?)
    } else {
        None
    };
    let texts = if c.take("quote") {
        let q = c.atext()?;
        if !c.take("prefix") {
            return fail(
                Rule::AnchorTextsPartial,
                "quote, prefix and suffix texts come together",
            );
        }
        let p = c.atext()?;
        if !c.take("suffix") {
            return fail(
                Rule::AnchorTextsPartial,
                "quote, prefix and suffix texts come together",
            );
        }
        Some([q, p, c.atext()?])
    } else {
        None
    };
    let end = if c.take("end") {
        Some(c.atext()?)
    } else {
        None
    };
    let occurrence = if c.take("occurrence") {
        match parse_dec(c.word()).filter(|v| (1..=65_535).contains(v)) {
            Some(v) => Some(v as u16),
            None => return fail(Rule::NoProduction, "occurrence not 1-65535"),
        }
    } else {
        None
    };
    let hint = if c.take("hint") {
        match c
            .word()
            .split_once('-')
            .and_then(|(a, b)| Some((parse_dec(a)?, parse_dec(b)?)))
        {
            Some((a, b)) if a >= 1 && a <= b && b <= u64::from(u32::MAX) => {
                Some((a as u32, b as u32))
            }
            _ => {
                return fail(
                    Rule::AnchorHintOrder,
                    "hint is not first-last with 1 <= first <= last",
                );
            }
        }
    } else {
        None
    };
    let mut window_hashes = None;
    let window = if c.take("window") {
        let p = c.pos;
        let b = un_b64url(c.word(), p)?;
        let w = Window::decode(&b, p).map_err(|e| e.or_rule(Rule::ValueConstraint))?;
        window_hashes = Some(w.before.len() + w.after.len());
        Some(b)
    } else {
        None
    };
    let span = if c.take("span") {
        match c.word().strip_prefix("xxh3:").filter(|h| is_lhex(h, 16)) {
            Some(h) => Some(u64::from_str_radix(h, 16).expect("hex")),
            None => {
                return fail(
                    Rule::NoProduction,
                    "span is not xxh3: and 16 lower-case hex digits",
                );
            }
        }
    } else {
        None
    };
    let blob = if c.take("blob") {
        match parse_oid_text(c.word()) {
            Some(o) => Some(o),
            None => return fail(Rule::NoProduction, "blob is not an oid-text"),
        }
    } else {
        None
    };
    let git = if c.take("git") {
        match parse_oid_text(c.word()) {
            Some(o) => Some(o),
            None => return fail(Rule::NoProduction, "git is not an oid-text"),
        }
    } else {
        None
    };
    if !c.take("captured") {
        return fail(Rule::AnchorDigests, "captured= missing");
    }
    let captured = c.digest()?;
    let pred = if c.take("pred") {
        Some(c.digest()?)
    } else {
        None
    };
    let marker = if c.take("marker") {
        let t = c.token()?;
        if t.is_empty() || t.len() > 64 || t.contains(['\n', '\r', '\0']) {
            return fail(Rule::NoProduction, "marker is not one line of 1-64 bytes");
        }
        Some(t)
    } else {
        None
    };
    if !c.take("v") {
        return fail(Rule::NoProduction, "v= missing");
    }
    let v = match parse_dec(c.word()) {
        Some(v) if v >= 1 && v <= u64::from(u16::MAX) => v,
        _ => return fail(Rule::AnchorV0, "v is not a resolver version >= 1"),
    };
    if !c.rest.is_empty() {
        return fail(
            Rule::NoProduction,
            "bytes after v=, or properties out of order",
        );
    }
    let quoted = ["heading", "symbol", "quote", "range"].contains(&kind.as_str());
    let file = kind == "file";
    if digests.is_some() != quoted {
        return fail(
            Rule::AnchorDigests,
            "the quote, prefix and suffix digests are present exactly for heading, symbol, quote and range",
        );
    }
    if end_h.is_some() && kind != "range" {
        return fail(Rule::AnchorEndH, "end_h on a kind other than range");
    }
    if end_h.is_none() && kind == "range" {
        return fail(Rule::AnchorDigests, "a range anchor lacks end_h");
    }
    if (texts.is_some() && !quoted) || end.is_some() != (texts.is_some() && kind == "range") {
        return fail(Rule::AnchorTextsPartial, "texts partly present");
    }
    if hint.is_some() == file || window.is_some() == file || span.is_some() == file {
        return fail(
            Rule::NoProduction,
            "hint, window and span are present exactly when kind is not file",
        );
    }
    if blob.is_none() && !file {
        return fail(Rule::NoProduction, "blob absent on a kind other than file");
    }
    // [F18 §2.9] I-F9: a quoted kind carries a non-empty quote (its digest is not the digest of the empty text), and a
    // `lines` anchor a window with at least one hash.
    if digests.as_ref().is_some_and(|d| d[0] == blake3_128(b"")) {
        return fail(
            Rule::AnchorIf9,
            "an anchor whose quote is empty breaks I-F9 [F18 §2.9]",
        );
    }
    if kind == "lines" && window_hashes == Some(0) {
        return fail(
            Rule::AnchorIf9,
            "a lines anchor whose window holds no hash breaks I-F9 [F18 §2.9]",
        );
    }
    if let (Some(d), Some(t)) = (&digests, &texts)
        && (0..3).any(|i| blake3_128(&t[i]) != d[i])
    {
        return fail(Rule::AnchorTextDigest, "a text does not match its digest");
    }
    if let (Some(d), Some(t)) = (&end_h, &end)
        && blake3_128(t) != *d
    {
        return fail(Rule::AnchorTextDigest, "the end text does not match end_h");
    }
    Ok(AProps {
        kind,
        mode,
        watch,
        scope,
        digests,
        end_h,
        texts,
        end,
        occurrence,
        hint,
        window,
        span,
        blob,
        git,
        captured,
        pred,
        marker,
        v,
    })
}

fn aprops_text(p: &AProps) -> String {
    let mut o = format!(" kind={} mode={} watch={}", p.kind, p.mode, p.watch);
    if let Some(s) = &p.scope {
        o.push_str(&format!(" scope={}", token(&scope_text(s))));
    }
    if let Some(d) = &p.digests {
        o.push_str(&format!(
            " quote_h={} prefix_h={} suffix_h={}",
            hex(&d[0]),
            hex(&d[1]),
            hex(&d[2])
        ));
    }
    if let Some(d) = &p.end_h {
        o.push_str(&format!(" end_h={}", hex(d)));
    }
    if let Some(t) = &p.texts {
        o.push_str(&format!(
            " quote={} prefix={} suffix={}",
            atext(&t[0]),
            atext(&t[1]),
            atext(&t[2])
        ));
    }
    if let Some(t) = &p.end {
        o.push_str(&format!(" end={}", atext(t)));
    }
    if let Some(x) = p.occurrence {
        o.push_str(&format!(" occurrence={x}"));
    }
    if let Some((a, b)) = p.hint {
        o.push_str(&format!(" hint={a}-{b}"));
    }
    if let Some(w) = &p.window {
        o.push_str(&format!(" window={}", b64url(w)));
    }
    if let Some(s) = p.span {
        o.push_str(&format!(" span=xxh3:{s:016x}"));
    }
    if let Some(b) = &p.blob {
        o.push_str(&format!(" blob={}", oid_text(b)));
    }
    if let Some(g) = &p.git {
        o.push_str(&format!(" git={}", oid_text(g)));
    }
    o.push_str(&format!(" captured={}", hex(&p.captured)));
    if let Some(x) = &p.pred {
        o.push_str(&format!(" pred={}", hex(x)));
    }
    if let Some(m) = &p.marker {
        o.push_str(&format!(" marker={}", token(m)));
    }
    o.push_str(&format!(" v={}", p.v));
    o
}

/// Splits the 32-byte uid text off the front of `s` without cutting a UTF-8 scalar value; a head that is short or not
/// hexadecimal then fails [`parse_uid`].
fn split32(s: &str) -> (&str, &str) {
    let mut n = s.len().min(32);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s.split_at(n)
}

/// [F18 §5.1]: a `relink` value matches R-17's grammar exactly (every other byte string is invalid, the empty one
/// included).
pub fn is_relink(t: &str) -> bool {
    const LAZY: [&str; 5] = ["file-id", "dir-id", "oid+ctime", "prefix", "pending"];
    const GIT: [&str; 2] = ["r100", "case"];
    const HOOK: [&str; 2] = ["file-id", "move"];
    const UNSCORED: [&str; 9] = [
        "identical-copy",
        "file-id-edited",
        "prefix-strong",
        "split",
        "argv",
        "tie",
        "rename-over",
        "swap",
        "path-reused",
    ];
    const SCORED: [&str; 5] = ["git-pair", "edited+moved", "similarity", "weak", "merged"];
    let score = |s: &str| {
        s == "1.00"
            || s.strip_prefix("0.")
                .is_some_and(|d| d.len() == 2 && d.bytes().all(|b| b.is_ascii_digit()))
    };
    let Some((how, rest)) = t.split_once('/') else {
        return false;
    };
    match how {
        "explicit" => rest == "intent-recovered" || rest == "intent",
        "lazy" => LAZY.contains(&rest),
        "git" => GIT.contains(&rest),
        "hook" => HOOK.contains(&rest),
        "journal" => rest == "usn" || rest == "fsevents",
        "merge-observation" => LAZY.contains(&rest) || GIT.contains(&rest) || HOOK.contains(&rest),
        "merge-compose" => rest == "prefix",
        "owner" | "agent" | "policy" | "confirmed" => match rest.split_once('/') {
            None => UNSCORED.contains(&rest) || rest == "manual" || rest == "replacement",
            Some((ev, sc)) => SCORED.contains(&ev) && score(sc),
        },
        _ => false,
    }
}

/// The context a node file is read in.
pub struct Ctx<'a> {
    /// The effective schema.
    pub schema: &'a Schema,
    /// The uid the file's path names, when known ([F14 §3.4], §9.2).
    pub path_uid: Option<[u8; 16]>,
}

fn field_ctx<'a>(
    schema: &'a Schema,
    kind: &'a str,
    f: &'a FieldDef,
    root_node: bool,
) -> (VCtx<'a>, Vec<&'a str>) {
    let implied = f.implied_root && (kind == "artifact" || root_node);
    let values = schema.enum_values(kind, f);
    (
        VCtx {
            field: f,
            implied,
            values: &[],
        },
        values,
    )
}

fn with_values<'a>(c: VCtx<'a>, values: &'a [&'a str]) -> VCtx<'a> {
    VCtx { values, ..c }
}

/// Parses a field value of `kind` from a line value (`field` line, conflict side, snapshot).
fn read_field(
    schema: &Schema,
    kind: &str,
    name: &str,
    s: &str,
    root_node: bool,
    at: usize,
) -> Result<Option<IVal>> {
    let Some(f) = schema.field(kind, name) else {
        return unknown_field(kind, name, at);
    };
    let (c, vals) = field_ctx(schema, kind, f, root_node);
    parse_fval(s, with_values(c, &vals), at)
}

fn field_line_value(v: &IVal) -> String {
    match v {
        IVal::Text(t) => sval(t),
        IVal::Path(p) => sval(&path_text(p)),
        v => single_text(v),
    }
}

fn side_token(v: &IVal) -> String {
    if is_textish(v) {
        token(&single_text(v))
    } else {
        let t = single_text(v);
        if tbare_ok(&t) { t } else { jstring(&t) }
    }
}

fn parse_status_side(
    schema: &Schema,
    kind: &str,
    t: &str,
    at: usize,
) -> Result<Option<(String, Option<String>)>> {
    let (s, r) = t.split_once('/').map_or((t, None), |(a, b)| (a, Some(b)));
    let k = schema.kinds.get(kind);
    if !is_vname(s) || k.is_some_and(|k| !k.statuses.iter().any(|x| x == s)) {
        return parse_err(
            Rule::UnknownName,
            at,
            format!("unknown status {s:?} for kind {kind} [F14 §9.2]"),
        );
    }
    let res_values = &schema.common["resolution"].values;
    if let Some(r) = r
        && !res_values.iter().any(|x| x == r)
    {
        return parse_err(
            Rule::UnknownName,
            at,
            format!("unknown resolution {r:?} [F14 §9.2]"),
        );
    }
    let r = r.filter(|r| *r != "none").map(str::to_owned);
    if k.is_some_and(|k| k.initial == s) && r.is_none() {
        return Ok(None);
    }
    Ok(Some((s.to_owned(), r)))
}

fn status_text(schema: &Schema, kind: &str, st: &Option<(String, Option<String>)>) -> String {
    match st {
        Some((s, Some(r))) => format!("{s}/{r}"),
        Some((s, None)) => s.clone(),
        None => schema
            .kinds
            .get(kind)
            .map_or_else(String::new, |k| k.initial.clone()),
    }
}

fn parse_ex(schema: &Schema, t: &str, at: usize) -> Result<ExSide> {
    let mut lines = t.split('\n');
    let first = lines.next().unwrap_or("");
    if let Some(kind) = first.strip_prefix("deleted ") {
        check_kind(schema, kind, at)?;
        let mut reason = None;
        let mut replaced_by = None;
        for l in lines {
            if let Some(v) = l.strip_prefix("reason: ") {
                if reason.is_some() || replaced_by.is_some() {
                    return parse_err(
                        Rule::LineRepeated,
                        at,
                        "existence side lines out of order or repeated [F14 §6.8.1]",
                    );
                }
                reason = Some(read_sval(v, at)?);
            } else if let Some(v) = l.strip_prefix("replaced_by: ") {
                if replaced_by.is_some() {
                    return parse_err(
                        Rule::LineRepeated,
                        at,
                        "existence side line repeated [F14 §6.8.1]",
                    );
                }
                replaced_by = Some(parse_uid(v, at)?);
            } else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    format!("an existence side line matches no production: {l:?} [F14 §6.8.1]"),
                );
            }
        }
        return Ok(ExSide::Deleted {
            kind: kind.to_owned(),
            reason,
            replaced_by,
        });
    }
    let Some(kind) = first.strip_prefix("live ") else {
        return parse_err(
            Rule::NoProduction,
            at,
            "an existence side is not live or deleted [F14 §6.8.1]",
        );
    };
    check_kind(schema, kind, at)?;
    let mut status = None;
    let mut have_status = false;
    let mut fields = BTreeMap::new();
    let mut labels = BTreeSet::new();
    let mut totals = BTreeMap::new();
    let mut body = None;
    let root_node = t.contains("\nfield root: ") && kind == "area";
    for l in lines {
        if let Some(v) = l.strip_prefix("status: ") {
            if have_status {
                return parse_err(
                    Rule::LineRepeated,
                    at,
                    "a snapshot status repeated [F14 §6.8.1]",
                );
            }
            have_status = true;
            status = parse_status_side(schema, kind, v, at)?;
        } else if let Some(v) = l.strip_prefix("title: ") {
            let t = read_sval(v, at)?;
            if fields.insert("title".to_owned(), IVal::Text(t)).is_some() {
                return parse_err(
                    Rule::LineRepeated,
                    at,
                    "a snapshot title repeated [F14 §6.8.1]",
                );
            }
        } else if let Some(rest) = l.strip_prefix("field ") {
            let Some((name, v)) = rest.split_once(": ") else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "a snapshot field line lacks `: ` [F14 §6.8.1]",
                );
            };
            if let Some(val) = read_field(schema, kind, name, v, root_node, at)?
                && fields.insert(name.to_owned(), val).is_some()
            {
                return parse_err(
                    Rule::LineRepeated,
                    at,
                    "a snapshot field repeated [F14 §6.8.1]",
                );
            }
        } else if let Some(v) = l.strip_prefix("label ") {
            if !labels.insert(read_sval(v, at)?) {
                return parse_err(
                    Rule::LineRepeated,
                    at,
                    "a snapshot label repeated [F14 §6.8.1]",
                );
            }
        } else if let Some(rest) = l.strip_prefix("total ") {
            let Some((name, v)) = rest.split_once(' ') else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "a snapshot total lacks its value [F14 §6.8.1]",
                );
            };
            let Some(n) = parse_sdec(v) else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "a snapshot total is not an sdec [F14 §6.8.1]",
                );
            };
            if n != 0 && totals.insert(name.to_owned(), n).is_some() {
                return parse_err(
                    Rule::LineRepeated,
                    at,
                    "a snapshot total repeated [F14 §6.8.1]",
                );
            }
        } else if let Some(v) = l.strip_prefix("body ") {
            let (b, n) = read_jstring(v, at)?;
            if n != v.len() || body.is_some() {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "a snapshot body is not one JSON string [F14 §6.8.1]",
                );
            }
            if !b.is_empty() {
                body = Some(b);
            }
        } else {
            return parse_err(
                Rule::NoProduction,
                at,
                format!("an existence side line matches no production: {l:?} [F14 §6.8.1]"),
            );
        }
    }
    Ok(ExSide::Live {
        kind: kind.to_owned(),
        status,
        fields,
        labels,
        totals,
        body,
    })
}

fn ex_text(schema: &Schema, e: &ExSide) -> String {
    match e {
        ExSide::Deleted {
            kind,
            reason,
            replaced_by,
        } => {
            let mut o = format!("deleted {kind}");
            if let Some(r) = reason {
                o.push_str(&format!("\nreason: {}", sval(r)));
            }
            if let Some(u) = replaced_by {
                o.push_str(&format!("\nreplaced_by: {}", hex(u)));
            }
            o
        }
        ExSide::Live {
            kind,
            status,
            fields,
            labels,
            totals,
            body,
        } => {
            let mut o = format!("live {kind}\nstatus: {}", status_text(schema, kind, status));
            for (name, v) in fields {
                let t = match v {
                    IVal::Text(t) | IVal::Enum(t) if name == "title" => {
                        format!("title: {}", sval(t))
                    }
                    IVal::Text(t) => format!("field {name}: {}", sval(t)),
                    IVal::Path(p) => format!("field {name}: {}", sval(&path_text(p))),
                    v => format!("field {name}: {}", single_text(v)),
                };
                o.push('\n');
                o.push_str(&t);
            }
            let mut ls: Vec<String> = labels.iter().map(|l| sval(l)).collect();
            ls.sort();
            for l in ls {
                o.push_str(&format!("\nlabel {l}"));
            }
            for (n, v) in totals {
                o.push_str(&format!("\ntotal {n} {v}"));
            }
            if let Some(b) = body {
                o.push_str(&format!("\nbody {}", jstring(b)));
            }
            o
        }
    }
}

fn check_kind(schema: &Schema, kind: &str, at: usize) -> Result<()> {
    if !is_iname(kind) || !schema.kinds.contains_key(kind) {
        return parse_err(
            Rule::UnknownName,
            at,
            format!("unknown kind {kind:?} [F14 §9.2]"),
        );
    }
    Ok(())
}

/// Parses one conflict side for `key` of a node of `kind`.
fn parse_side(
    schema: &Schema,
    kind: &str,
    key: &str,
    t: &str,
    root_node: bool,
    at: usize,
) -> Result<Option<Side>> {
    if t.is_empty() {
        return Ok(None);
    }
    Ok(Some(if let Some(name) = key.strip_prefix("field.") {
        let Some(f) = schema.field(kind, name) else {
            return parse_err(
                Rule::UnknownName,
                at,
                format!("unknown field {name} in a conflict key [F14 §9.2]"),
            );
        };
        let (c, vals) = field_ctx(schema, kind, f, root_node);
        let c = with_values(c, &vals);
        let v = match f.ty {
            Ty::Set(el) => IVal::Set(parse_set(t, el, c, at)?),
            ty => parse_single(ty, t, c, at)?,
        };
        check_value(&v, c, at)?;
        if is_default(&v, f) {
            return Ok(None);
        }
        Side::Field(v)
    } else if key == "status" {
        match parse_status_side(schema, kind, t, at)? {
            Some((s, r)) => Side::Status(s, r),
            None => return Ok(None),
        }
    } else if key == "parent" {
        let Some((p, o)) = t.split_once(',') else {
            return parse_err(
                Rule::NoProduction,
                at,
                "a parent side is not <uid|->,<order|-> [F14 §6.8]",
            );
        };
        let p = if p == "-" {
            None
        } else {
            Some(parse_uid(p, at)?)
        };
        let o = if o == "-" {
            None
        } else if !o.is_empty() && o.bytes().all(|b| b.is_ascii_alphanumeric()) {
            Some(o.to_owned())
        } else {
            return parse_err(
                Rule::NoProduction,
                at,
                "a parent side's order is not an order key [F14 §6.8]",
            );
        };
        if p.is_none() && o.is_none() {
            return Ok(None);
        }
        Side::Parent(p, o)
    } else if key == "body" {
        Side::Body(t.to_owned())
    } else if key == "existence" {
        Side::Existence(Box::new(parse_ex(schema, t, at)?))
    } else if key == "observation" {
        let mut m = BTreeMap::new();
        for l in t.split('\n') {
            let Some((name, v)) = l.strip_prefix("field ").and_then(|r| r.split_once(": ")) else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "an observation side line is not a field line [F14 §6.8]",
                );
            };
            if !OBS.contains(&name) {
                return parse_err(
                    Rule::UnknownName,
                    at,
                    "an observation side names a field outside the composite [F14 §6.8]",
                );
            }
            if let Some(v) = read_field(schema, kind, name, v, root_node, at)? {
                m.insert(name.to_owned(), v);
            }
        }
        if m.is_empty() {
            return Ok(None);
        }
        Side::Observation(m)
    } else if let Some(rest) = key.strip_prefix("edge.at.") {
        let _ = rest;
        Side::Anchor(Box::new(parse_aprops(&format!(" {t}"), at)?))
    } else {
        match t {
            "present" => Side::Edge(None, false),
            "flagged" => Side::Edge(None, true),
            _ => match t.strip_prefix("pin=").and_then(parse_commit_id) {
                Some(c) => Side::Edge(Some(c), false),
                None => {
                    return parse_err(
                        Rule::NoProduction,
                        at,
                        "an edge side is not present, pin=… or flagged [F14 §6.8]",
                    );
                }
            },
        }
    }))
}

fn side_text(schema: &Schema, kind: &str, s: &Option<Side>, key: &str) -> String {
    match s {
        None if key == "status" => status_text(schema, kind, &None),
        None => String::new(),
        Some(Side::Field(v)) => side_token(v),
        Some(Side::Status(st, r)) => status_text(schema, kind, &Some((st.clone(), r.clone()))),
        Some(Side::Parent(p, o)) => format!(
            "{},{}",
            p.map_or_else(|| "-".into(), |u| hex(&u)),
            o.clone().unwrap_or_else(|| "-".into())
        ),
        Some(Side::Body(b)) => jstring(b),
        Some(Side::Edge(pin, flagged)) => match (pin, flagged) {
            (Some(c), _) => format!("pin=c{}", hex(c)),
            (None, true) => "flagged".into(),
            (None, false) => "present".into(),
        },
        Some(Side::Anchor(p)) => jstring(&aprops_text(p)[1..]),
        Some(Side::Observation(m)) => jstring(
            &OBS.iter()
                .filter_map(|n| {
                    m.get(*n)
                        .map(|v| format!("field {n}: {}", field_line_value(v)))
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Some(Side::Existence(e)) => jstring(&ex_text(schema, e)),
    }
}

fn check_ckey(schema: &Schema, kind: &str, key: &str, at: usize) -> Result<()> {
    let ok = match key {
        "status" | "body" | "parent" | "existence" | "observation" => true,
        k if k.starts_with("field.") => is_iname(&k[6..]) && schema.field(kind, &k[6..]).is_some(),
        k if k.starts_with("edge.") => {
            let parts: Vec<&str> = k[5..].split('.').collect();
            match parts.as_slice() {
                [ek, dst] => {
                    is_iname(ek)
                        && *ek != "at"
                        && schema.edges.contains_key(*ek)
                        && is_lhex(dst, 32)
                }
                [ek, dst, anc] => *ek == "at" && is_lhex(dst, 32) && is_lhex(anc, 32),
                _ => false,
            }
        }
        _ => false,
    };
    if !ok {
        return parse_err(
            Rule::UnknownName,
            at,
            format!("conflict key {key:?} matches no c-key of the schema [F14 §6.8]"),
        );
    }
    Ok(())
}

/// Splits a conflict line's tail (after the key) into class and three side texts.
fn split_conflict(s: &str, at: usize) -> Result<(String, [String; 3])> {
    let Some(rest) = s.strip_prefix(" class=") else {
        return parse_err(
            Rule::NoProduction,
            at,
            "a conflict line lacks class= [F14 §6.1]",
        );
    };
    let n = rest.find(' ').unwrap_or(rest.len());
    let class = rest[..n].to_owned();
    if class.is_empty() || !class.bytes().all(|b| b.is_ascii_alphabetic()) {
        return parse_err(
            Rule::NoProduction,
            at,
            "a conflict class is not 1*ALPHA [F14 §6.1]",
        );
    }
    let mut rest = &rest[n..];
    let mut sides: [String; 3] = Default::default();
    for (i, k) in [" base=", " ours=", " theirs="].iter().enumerate() {
        let Some(r) = rest.strip_prefix(k) else {
            return parse_err(
                Rule::NoProduction,
                at,
                format!("a conflict line lacks{k} [F14 §6.1]"),
            );
        };
        rest = r;
        if rest.is_empty() || rest.starts_with(' ') {
            continue;
        }
        let (t, n, _) = read_token(rest, at)?;
        sides[i] = t;
        rest = &rest[n..];
    }
    if !rest.is_empty() {
        return parse_err(
            Rule::NoProduction,
            at,
            "bytes after a conflict line's theirs= [F14 §6.1]",
        );
    }
    Ok((class, sides))
}

/// Derived state, which is never written ([F14 §6.2]: I36′, [AR §5b.2] rule 6; the flags of [F08 §3.3] and the
/// counters of [F08 §3.4]): a `field` line for one is a line not allowed in its form ([F14 §9.2]), not an unknown field.
const DERIVED_STATE: [&str; 15] = [
    "open_blockers",
    "open_blockers_exo",
    "children_total",
    "children_done",
    "topo",
    "ready",
    "suspect",
    "is_blocker",
    "conflicted",
    "has_dangling",
    "container",
    "rev_seq",
    "claimed",
    "settled",
    "stale",
];

/// The refusal of a `field` line whose name the kind does not have: derived state ([`DERIVED_STATE`]) or an unknown
/// field ([F14 §9.2]).
fn unknown_field<T>(kind: &str, name: &str, at: usize) -> Result<T> {
    if DERIVED_STATE.contains(&name) {
        return parse_err(
            Rule::LineNotAllowed,
            at,
            format!("field {name} is derived state, which is never written [F14 §6.2, §9.2]"),
        );
    }
    parse_err(
        Rule::UnknownName,
        at,
        format!("unknown field {name} for kind {kind} [F14 §9.2]"),
    )
}

/// The rule of a node-file line that no production takes ([F14 §9.2]): a leftover merge marker, a `key: value` line
/// (every known header, field, edge, anchor and conflict line was tried before, so its key is an unknown header key),
/// or any other line.
fn unmatched_line_rule(line: &str) -> Rule {
    if ["<<<<<<<", "=======", ">>>>>>>"]
        .iter()
        .any(|m| line.starts_with(m))
    {
        return Rule::MergeMarkers;
    }
    let header_key = |k: &str| {
        k.starts_with(|c: char| c.is_ascii_lowercase())
            && k.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    match line.split_once(": ") {
        Some((k, _)) if header_key(k) => Rule::UnknownHeaderKey,
        _ => Rule::NoProduction,
    }
}

/// Parses a node file from the importer's superset ([F14 §9.1]) with the `ImageParse` rules of §9.2.
pub fn parse(bytes: &[u8], cx: &Ctx<'_>) -> Result<NodeFile> {
    let norm = import_normalise(bytes);
    let text = utf8_file(&norm)?;
    let schema = cx.schema;
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut off = 0;
    for l in text.split_inclusive('\n') {
        lines.push((off, l.strip_suffix('\n').unwrap_or(l)));
        off += l.len();
    }
    if lines.first().map(|x| x.1.trim_end_matches([' ', '\t'])) != Some("moirai-node 1") {
        return parse_err(
            Rule::MagicVersion,
            0,
            "the first line is not `moirai-node 1` [F14 §6.1]",
        );
    }
    let mut nf = NodeFile::default();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut once = |k: String, at: usize| -> Result<()> {
        if !seen.insert(k.clone()) {
            return parse_err(
                Rule::LineRepeated,
                at,
                format!("line {k:?} repeated [F14 §9.2]"),
            );
        }
        Ok(())
    };
    let mut uid = None;
    let mut kind: Option<String> = None;
    let mut raw_fields: Vec<(usize, String, FieldRaw)> = Vec::new();
    let mut raw_status: Option<(usize, String)> = None;
    let mut raw_resolution: Option<(usize, String)> = None;
    let mut raw_headers: Vec<(usize, String, String)> = Vec::new();
    let mut raw_conflicts: Vec<(usize, String, String)> = Vec::new();
    let mut raw_edges: Vec<(usize, String, String)> = Vec::new();
    let mut raw_labels: Vec<(usize, String)> = Vec::new();
    let mut i = 1;
    while i < lines.len() {
        let (at, raw) = lines[i];
        if raw == "---" || raw.trim_end_matches([' ', '\t']) == "---" {
            let body_start = at + raw.len() + 1;
            let body = &text[body_start.min(text.len())..];
            let body = body.strip_suffix('\n').unwrap_or(body);
            if body.contains('\r') {
                return parse_err(Rule::NoProduction, body_start, "a body holds CR [F08 §7.2]");
            }
            if !body.is_empty() {
                nf.body = Some(body.to_owned());
            }
            once("---".into(), at)?;
            break;
        }
        let line = raw.trim_end_matches([' ', '\t']);
        if line.contains('\0') {
            return parse_err(
                Rule::NulOutsideBody,
                at,
                "U+0000 outside a body or a block [F14 §9.2]",
            );
        }
        if let Some(v) = line.strip_prefix("uid: ") {
            once("uid".into(), at)?;
            uid = Some(parse_uid(v, at)?);
        } else if let Some(v) = line.strip_prefix("kind: ") {
            once("kind".into(), at)?;
            check_kind(schema, v, at)?;
            kind = Some(v.to_owned());
        } else if let Some(v) = line.strip_prefix("title: ") {
            once("title".into(), at)?;
            let t = read_sval(v, at)?;
            if t.is_empty() || t.len() > 200 || t.contains(['\n', '\r', '\0']) {
                return parse_err(
                    Rule::ValueConstraint,
                    at,
                    "a title is not one line of 1-200 bytes [F08 §7.1]",
                );
            }
            nf.title = Some(t);
        } else if let Some(v) = line.strip_prefix("status: ") {
            once("status".into(), at)?;
            raw_status = Some((at, v.to_owned()));
        } else if let Some(v) = line.strip_prefix("resolution: ") {
            once("resolution".into(), at)?;
            raw_resolution = Some((at, v.to_owned()));
        } else if let Some((h, v)) = HEADER_ENUMS
            .iter()
            .find_map(|h| line.strip_prefix(&format!("{h}: ")).map(|v| (*h, v)))
        {
            once(h.into(), at)?;
            raw_headers.push((at, h.to_owned(), v.to_owned()));
        } else if let Some(v) = line.strip_prefix("parent: ") {
            once("parent".into(), at)?;
            nf.parent = Some(parse_uid(v, at)?);
        } else if let Some(v) = line.strip_prefix("order: ") {
            once("order".into(), at)?;
            if v.is_empty() || !v.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "an order key is not 1*(DIGIT / ALPHA) [F14 §6.1]",
                );
            }
            nf.order = Some(v.to_owned());
        } else if let Some(v) = line.strip_prefix("created: ") {
            once("created".into(), at)?;
            nf.created = Some(parse_prov(v, at)?);
        } else if let Some(v) = line.strip_prefix("updated: ") {
            once("updated".into(), at)?;
            nf.updated = Some(parse_prov(v, at)?);
        } else if let Some(v) = line.strip_prefix("deleted: ") {
            once("deleted".into(), at)?;
            nf.deleted = Some(parse_prov(v, at)?);
        } else if let Some(v) = line.strip_prefix("flags: [") {
            once("flags".into(), at)?;
            let Some(inner) = v.strip_suffix(']') else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "a flags line does not end with ] [F14 §6.1]",
                );
            };
            for fl in inner.split(", ") {
                if !["archived", "frozen", "pinned"].contains(&fl)
                    || !nf.flags.insert(fl.to_owned())
                {
                    return parse_err(
                        Rule::UnknownName,
                        at,
                        "a flags line holds an unknown or repeated flag [F14 §6.1]",
                    );
                }
            }
        } else if let Some(rest) = line.strip_prefix("field ") {
            let Some((name, v)) = rest.split_once(':') else {
                return parse_err(Rule::NoProduction, at, "a field line lacks `:` [F14 §6.1]");
            };
            if !is_iname(name) {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "a field name is not an iname [F14 §6.1]",
                );
            }
            once(format!("field {name}"), at)?;
            if v == " <<" {
                let mut block = Vec::new();
                i += 1;
                loop {
                    let Some(&(bat, bl)) = lines.get(i) else {
                        return parse_err(
                            Rule::NoProduction,
                            at,
                            "a block is not closed by >> [F14 §5.4]",
                        );
                    };
                    if let Some(c) = bl.strip_prefix("  ") {
                        block.push(c.to_owned());
                        i += 1;
                    } else if bl.trim_end_matches([' ', '\t']) == ">>" {
                        break;
                    } else {
                        return parse_err(
                            Rule::NoProduction,
                            bat,
                            "a block line does not begin with two SP [F14 §5.4]",
                        );
                    }
                }
                raw_fields.push((at, name.to_owned(), FieldRaw::Block(block)));
            } else if let Some(v) = v.strip_prefix(' ') {
                raw_fields.push((at, name.to_owned(), FieldRaw::Line(v.to_owned())));
            } else {
                return parse_err(Rule::NoProduction, at, "a field line lacks `: ` [F14 §6.1]");
            }
        } else if let Some(v) = line.strip_prefix("label ") {
            raw_labels.push((at, read_sval(v, at)?));
        } else if let Some(rest) = line.strip_prefix("incr ") {
            let parts: Vec<&str> = rest.splitn(3, ' ').collect();
            let [name, delta, tok] = parts.as_slice() else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "an incr line breaks the grammar [F14 §6.1]",
                );
            };
            let (sign, mag) = match (delta.strip_prefix('+'), delta.strip_prefix('-')) {
                (Some(m), _) => (1i128, m),
                (_, Some(m)) => (-1i128, m),
                _ => {
                    return parse_err(
                        Rule::NoProduction,
                        at,
                        "an incr delta lacks its sign [F14 §6.5]",
                    );
                }
            };
            let Some(m) = parse_dec(mag).filter(|v| *v != 0) else {
                return parse_err(
                    Rule::NumberRange,
                    at,
                    "an incr delta is not NZDIGIT *DIGIT within 2^64 - 1 [F14 §6.5]",
                );
            };
            if tok.is_empty() || tok.chars().any(|c| c == ' ' || is_control(c)) || !is_iname(name) {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "an incr token is not 1*vis, or the field is not an iname [F14 §6.5]",
                );
            }
            once(format!("incr {name} {tok}"), at)?;
            nf.ledger
                .push(((*name).to_owned(), sign * i128::from(m), (*tok).to_owned()));
        } else if let Some(rest) = line.strip_prefix("edge ") {
            let Some((ek, tail)) = rest.split_once(" -> ") else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "an edge line lacks ` -> ` [F14 §6.1]",
                );
            };
            raw_edges.push((at, ek.to_owned(), tail.to_owned()));
        } else if let Some(rest) = line.strip_prefix("anchor ") {
            let (au, tail) = split32(rest);
            let a_uid = parse_uid(au, at)?;
            let Some(tail) = tail.strip_prefix(" -> ") else {
                return parse_err(
                    Rule::NoProduction,
                    at,
                    "an anchor line lacks ` -> ` [F14 §6.1]",
                );
            };
            let (du, props) = split32(tail);
            let dst = parse_uid(du, at)?;
            once(format!("anchor {au}"), at)?;
            nf.anchors.push(AnchorLine {
                uid: a_uid,
                dst,
                props: parse_aprops(props, at)?,
            });
        } else if let Some(rest) = line.strip_prefix("conflict ") {
            let n = rest.find(' ').unwrap_or(rest.len());
            raw_conflicts.push((at, rest[..n].to_owned(), rest[n..].to_owned()));
        } else {
            return parse_err(
                unmatched_line_rule(line),
                at,
                format!("a line matches no production: {line:?} [F14 §9.2]"),
            );
        }
        i += 1;
    }
    let Some(uid) = uid else {
        return parse_err(
            Rule::RequiredLineMissing,
            0,
            "the uid: line is missing [F14 §9.2]",
        );
    };
    let Some(kind) = kind else {
        return parse_err(
            Rule::RequiredLineMissing,
            0,
            "the kind: line is missing [F14 §9.2]",
        );
    };
    if cx.path_uid.is_some_and(|p| p != uid) {
        return parse_err(
            Rule::UidFileName,
            0,
            "uid: differs from the file name [F14 §9.2]",
        );
    }
    nf.uid = uid;
    nf.tomb = nf.deleted.is_some();
    let kdef = &schema.kinds[&kind];
    let root_node = kind == "area" && raw_fields.iter().any(|(_, n, _)| n == "root");
    // Form rules (§6.10, §6.11, §9.2).
    if nf.tomb {
        let bad = nf.created.is_some()
            || nf.updated.is_some()
            || raw_status.is_some()
            || raw_resolution.is_some()
            || !raw_headers.is_empty()
            || nf.parent.is_some()
            || nf.order.is_some()
            || !nf.flags.is_empty()
            || !raw_labels.is_empty()
            || !nf.ledger.is_empty()
            || nf.body.is_some()
            || raw_fields
                .iter()
                .any(|(_, n, _)| n != "reason" && n != "replaced_by");
        if bad {
            return parse_err(
                Rule::LineNotAllowed,
                0,
                "a line not allowed in a tombstone file [F14 §6.10]",
            );
        }
        if nf.title.is_none() {
            return parse_err(
                Rule::RequiredLineMissing,
                0,
                "a tombstone file lacks its title: [F14 §9.2]",
            );
        }
        for (at, name, raw) in &raw_fields {
            let FieldRaw::Line(v) = raw else {
                return parse_err(
                    Rule::LineNotAllowed,
                    *at,
                    "a tombstone reason or replacement in a block [F14 §6.10]",
                );
            };
            if name == "reason" {
                let r = read_sval(v, *at)?;
                if !r.is_empty() {
                    nf.t_reason = Some(r);
                }
            } else {
                nf.t_replaced = Some(parse_uid(v, *at)?);
            }
        }
    } else {
        if kdef.title_derived && nf.title.is_some() {
            return parse_err(
                Rule::LineNotAllowed,
                0,
                "a title: line in a live file of a title-derived kind [F14 §9.2]",
            );
        }
        if !kdef.title_derived
            && nf.title.is_none()
            && !raw_conflicts.iter().any(|c| c.1 == "field.title")
        {
            return parse_err(
                Rule::RequiredLineMissing,
                0,
                "a live file lacks the title: its kind requires [F14 §9.2]",
            );
        }
        if let Some((at, v)) = &raw_status {
            let s = match &raw_resolution {
                Some((_, r)) => format!("{v}/{r}"),
                None => v.clone(),
            };
            nf.status = parse_status_side(schema, &kind, &s, *at)?;
        } else if let Some((at, _)) = &raw_resolution {
            return parse_err(
                Rule::RequiredLineMissing,
                *at,
                "a resolution: line without status: [F14 §6.2]",
            );
        }
        for (at, h, v) in &raw_headers {
            let f = &schema.common[h];
            if !f.values.iter().any(|x| x == v) {
                return parse_err(
                    Rule::UnknownName,
                    *at,
                    format!("unknown {h} value {v:?} [F14 §9.2]"),
                );
            }
            if f.default.as_deref() != Some(v.as_str()) {
                nf.headers.insert(h.clone(), v.clone());
            }
        }
        for (at, name, raw) in &raw_fields {
            let Some(f) = schema.field(&kind, name) else {
                return unknown_field(&kind, name, *at);
            };
            let form_ok = matches!(f.storage, Storage::Field | Storage::Cold)
                && name != "order"
                && name != "labels"
                && f.ty != Ty::Counter;
            if !form_ok {
                return parse_err(
                    Rule::LineNotAllowed,
                    *at,
                    format!("field {name} is not written as a field line [F14 §6.4]"),
                );
            }
            let (c, vals) = field_ctx(schema, &kind, f, root_node);
            let c = with_values(c, &vals);
            let v = match raw {
                FieldRaw::Line(v) => parse_fval(v, c, *at)?,
                FieldRaw::Block(b) => {
                    let v = if f.ty == Ty::Set(Elem::PathMove) {
                        let mut els = Vec::with_capacity(b.len());
                        for l in b {
                            els.push(parse_single(Ty::PathMove, l, c, *at)?);
                        }
                        if els.is_empty() {
                            return parse_err(
                                Rule::ValueType,
                                *at,
                                "an empty pathmove block [F14 §5.4]",
                            );
                        }
                        // [F14 §5.4]: entries re-sorted into the bytewise order of their lines; one entry once.
                        let mut keyed: Vec<(String, IVal)> =
                            els.into_iter().map(|v| (single_text(&v), v)).collect();
                        keyed.sort_by(|a, b| a.0.cmp(&b.0));
                        if keyed.windows(2).any(|w| w[0].0 == w[1].0) {
                            return parse_err(
                                Rule::LineRepeated,
                                *at,
                                "a pathmove block holds one entry twice [F14 §5.4]",
                            );
                        }
                        IVal::Set(keyed.into_iter().map(|(_, v)| v).collect())
                    } else if f.ty == Ty::Text {
                        if b.len() < 2 {
                            return parse_err(
                                Rule::NoProduction,
                                *at,
                                "a text block holds fewer than two lines [F14 §5.4]",
                            );
                        }
                        IVal::Text(b.join("\n"))
                    } else {
                        return parse_err(
                            Rule::LineNotAllowed,
                            *at,
                            "a block for a field that is neither text nor a set of pathmove [F14 §5.4]",
                        );
                    };
                    check_value(&v, c, *at)?;
                    Some(v)
                }
            };
            if let Some(v) = v {
                if f.name == "path_moves" && !root_node {
                    return parse_err(
                        Rule::LineNotAllowed,
                        *at,
                        "path_moves on a node that is not a root node [F08 §9.3]",
                    );
                }
                if !is_default(&v, f) {
                    nf.fields.insert(name.clone(), v);
                }
            }
        }
        for (at, l) in &raw_labels {
            if !nf.labels.insert(l.clone()) {
                return parse_err(Rule::LineRepeated, *at, "a label line repeated [F14 §9.2]");
            }
            let lf = &schema.common["labels"];
            check_value(
                &IVal::Text(l.clone()),
                VCtx {
                    field: lf,
                    implied: false,
                    values: &[],
                },
                *at,
            )?;
        }
        let mut sums: BTreeMap<&str, i128> = BTreeMap::new();
        for (name, d, _) in &nf.ledger {
            if schema
                .field(&kind, name)
                .is_none_or(|f| f.ty != Ty::Counter)
            {
                return parse_err(
                    Rule::LineNotAllowed,
                    0,
                    format!("an incr line for {name}, which is not a counter [F14 §9.2]"),
                );
            }
            *sums.entry(name).or_default() += d;
        }
        if sums
            .values()
            .any(|s| *s < i128::from(i64::MIN) || *s > i128::from(i64::MAX))
        {
            return parse_err(
                Rule::NumberRange,
                0,
                "a ledger whose sum leaves i64 [F14 §6.5]",
            );
        }
    }
    for (at, ek, tail) in &raw_edges {
        let (du, flags) = split32(tail);
        let dst = parse_uid(du, *at)?;
        let mut rest = flags;
        let pin = if let Some(r) = rest.strip_prefix(" pin=") {
            let n = r.find(' ').unwrap_or(r.len());
            let Some(c) = parse_commit_id(&r[..n]) else {
                return parse_err(
                    Rule::NoProduction,
                    *at,
                    "an edge pin= is not a commit id [F14 §6.6]",
                );
            };
            rest = &r[n..];
            Some(c)
        } else {
            None
        };
        let flagged = rest == " flagged";
        if !(rest.is_empty() || flagged) {
            return parse_err(
                Rule::NoProduction,
                *at,
                "an edge line breaks the grammar [F14 §6.1]",
            );
        }
        let Some((props, _)) = schema.edges.get(ek) else {
            return parse_err(
                Rule::UnknownName,
                *at,
                format!("unknown edge kind {ek:?} [F14 §9.2]"),
            );
        };
        if ek == "at" || ek == "parent" {
            return parse_err(
                Rule::LineNotAllowed,
                *at,
                "an edge at or edge parent line [F14 §9.2]",
            );
        }
        if (pin.is_some() && *props != Props::Pinned) || (flagged && *props != Props::Flagged) {
            return parse_err(
                Rule::LineNotAllowed,
                *at,
                "pin= or flagged not admitted by the edge kind's props [F14 §6.6]",
            );
        }
        if flagged && !nf.tomb {
            return parse_err(
                Rule::LineNotAllowed,
                *at,
                "flagged on a live node's edge [F14 §9.2]",
            );
        }
        once(format!("edge {ek} {du}"), *at)?;
        nf.edges.push(Edge {
            kind: ek.clone(),
            dst,
            pin,
            flagged,
        });
    }
    for (at, key, tail) in &raw_conflicts {
        check_ckey(schema, &kind, key, *at)?;
        let (class, sides) = split_conflict(tail, *at)?;
        if !CLASSES.contains(&class.as_str()) {
            return parse_err(
                Rule::UnknownName,
                *at,
                format!("unknown conflict class {class:?} [F14 §6.8]"),
            );
        }
        let mut parsed: [Option<Side>; 3] = Default::default();
        for j in 0..3 {
            parsed[j] = parse_side(schema, &kind, key, &sides[j], root_node, *at)?;
        }
        if nf
            .conflicts
            .insert(
                key.clone(),
                ConflictLine {
                    class,
                    sides: parsed,
                },
            )
            .is_some()
        {
            return parse_err(
                Rule::LineRepeated,
                *at,
                "two conflict lines of one key [F14 §9.2]",
            );
        }
    }
    // An ordinary line and a conflict line for one key (§9.2).
    for key in nf.conflicts.keys() {
        let clash = match key.as_str() {
            "status" => raw_status.is_some() || raw_resolution.is_some(),
            "parent" => nf.parent.is_some() || nf.order.is_some(),
            "body" => nf.body.is_some(),
            "observation" => OBS
                .iter()
                .any(|o| nf.fields.contains_key(*o) || raw_fields.iter().any(|f| f.1 == *o)),
            k if k.starts_with("field.") => {
                let n = &k[6..];
                raw_fields.iter().any(|f| f.1 == n)
                    || (n == "title" && nf.title.is_some())
                    || raw_headers.iter().any(|h| h.1 == n)
                    || nf.flags.contains(n)
                    || (n == "labels" && !nf.labels.is_empty())
            }
            k if k.starts_with("edge.at.") => {
                let anc = &k[k.len() - 32..];
                nf.anchors.iter().any(|a| hex(&a.uid) == anc)
            }
            k if k.starts_with("edge.") => {
                let parts: Vec<&str> = k[5..].split('.').collect();
                nf.edges
                    .iter()
                    .any(|e| e.kind == parts[0] && hex(&e.dst) == parts[1])
            }
            _ => false,
        };
        if clash {
            // [F14 §9.2]: a body with a body conflict is its own clause.
            let rule = if key == "body" {
                Rule::BodyAndConflict
            } else {
                Rule::OrdinaryAndConflict
            };
            return parse_err(
                rule,
                0,
                format!("an ordinary line and a conflict line for key {key} [F14 §9.2]"),
            );
        }
    }
    nf.kind = kind;
    nf.ledger
        .sort_by(|a, b| (a.0.as_bytes(), a.2.as_bytes()).cmp(&(b.0.as_bytes(), b.2.as_bytes())));
    nf.edges.sort_by(|a, b| {
        (a.kind.as_bytes(), a.dst, edge_rest(a)).cmp(&(b.kind.as_bytes(), b.dst, edge_rest(b)))
    });
    nf.anchors.sort_by_key(|a| (a.dst, a.uid));
    Ok(nf)
}

enum FieldRaw {
    Line(String),
    Block(Vec<String>),
}

fn edge_rest(e: &Edge) -> String {
    let mut o = String::new();
    if let Some(p) = &e.pin {
        o.push_str(&format!(" pin=c{}", hex(p)));
    }
    if e.flagged {
        o.push_str(" flagged");
    }
    o
}

/// Encodes a node file in the exporter's canonical bytes ([F14 §6]).
pub fn encode(nf: &NodeFile, schema: &Schema) -> Vec<u8> {
    let mut o = String::new();
    let mut line = |s: String| {
        o.push_str(&s);
        o.push('\n');
    };
    line("moirai-node 1".into());
    line(format!("uid: {}", hex(&nf.uid)));
    line(format!("kind: {}", nf.kind));
    if let Some(t) = &nf.title {
        line(format!("title: {}", sval(t)));
    }
    if nf.tomb {
        if let Some(p) = &nf.deleted {
            line(format!("deleted: {}", prov_text(p)));
        }
        if let Some(r) = &nf.t_reason {
            line(format!("field reason: {}", sval(r)));
        }
        if let Some(u) = &nf.t_replaced {
            line(format!("field replaced_by: {}", hex(u)));
        }
    } else {
        if !nf.conflicts.contains_key("status") {
            let st = status_text(schema, &nf.kind, &nf.status);
            match &nf.status {
                Some((s, Some(r))) => {
                    line(format!("status: {s}"));
                    line(format!("resolution: {r}"));
                }
                _ => line(format!("status: {st}")),
            }
        }
        for h in HEADER_ENUMS {
            if let Some(v) = nf.headers.get(h) {
                line(format!("{h}: {v}"));
            }
        }
        if let Some(p) = &nf.parent {
            line(format!("parent: {}", hex(p)));
        }
        if let Some(r) = &nf.order {
            line(format!("order: {r}"));
        }
        if let Some(p) = &nf.created {
            line(format!("created: {}", prov_text(p)));
        }
        if let Some(p) = &nf.updated {
            line(format!("updated: {}", prov_text(p)));
        }
        if !nf.flags.is_empty() {
            line(format!(
                "flags: [{}]",
                nf.flags.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        for (name, v) in &nf.fields {
            match v {
                IVal::Text(t) if t.contains('\n') => {
                    line(format!("field {name}: <<"));
                    for l in t.split('\n') {
                        line(format!("  {l}"));
                    }
                    line(">>".into());
                }
                IVal::Set(els) if matches!(els.first(), Some(IVal::PathMove(_))) => {
                    line(format!("field {name}: <<"));
                    let mut ls: Vec<String> = els.iter().map(single_text).collect();
                    ls.sort();
                    for l in ls {
                        line(format!("  {l}"));
                    }
                    line(">>".into());
                }
                v => line(format!("field {name}: {}", field_line_value(v))),
            }
        }
        let mut ls: Vec<String> = nf.labels.iter().map(|l| sval(l)).collect();
        ls.sort();
        for l in ls {
            line(format!("label {l}"));
        }
        for (name, d, tok) in &nf.ledger {
            let sign = if *d < 0 { '-' } else { '+' };
            line(format!("incr {name} {sign}{} {tok}", d.unsigned_abs()));
        }
    }
    for e in &nf.edges {
        line(format!(
            "edge {} -> {}{}",
            e.kind,
            hex(&e.dst),
            edge_rest(e)
        ));
    }
    for a in &nf.anchors {
        line(format!(
            "anchor {} -> {}{}",
            hex(&a.uid),
            hex(&a.dst),
            aprops_text(&a.props)
        ));
    }
    for (key, c) in &nf.conflicts {
        line(format!(
            "conflict {key} class={} base={} ours={} theirs={}",
            c.class,
            side_text(schema, &nf.kind, &c.sides[0], key),
            side_text(schema, &nf.kind, &c.sides[1], key),
            side_text(schema, &nf.kind, &c.sides[2], key)
        ));
    }
    if let Some(b) = &nf.body {
        line("---".into());
        line(b.clone());
    }
    o.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_canonical(src: &str) -> NodeFile {
        let s = Schema::core();
        let cx = Ctx {
            schema: &s,
            path_uid: None,
        };
        let nf = parse(src.as_bytes(), &cx).unwrap_or_else(|e| panic!("{e}\n{src}"));
        let back = String::from_utf8(encode(&nf, &s)).unwrap();
        assert_eq!(back, src, "canonical re-encode");
        nf
    }

    /// [F14 §17.1] the corrected task node.
    pub(super) const TASK_EXAMPLE: &str = "moirai-node 1\n\
uid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\n\
kind: task\n\
title: Byte-range lock protocol\n\
status: in_progress\n\
priority: P1\n\
parent: 018f3c2e7a117b3c9d5e4c2f1a0b9e09\n\
order: a0V\n\
created: c9b2e6c1d4f0a7e3b5c8d1f2a9e4b7c6d3f0a1e2b5c8d7f4a3e6b9c2d5f8a1b47 2026-09-21T14:02:11.483Z\n\
updated: c4470a11e2f3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4de 2026-09-25T14:02:40.011Z\n\
field acceptance: writer and flush bytes locked per range; a dead holder's range freed by the OS\n\
field assignee: dev#1\n\
field estimate: 3\n\
field files_owned: [src/lock.rs, src/vfs/lock_bytes.rs]\n\
field phase_state: implementing\n\
field work_kind: impl\n\
label l5np\n\
label storage\n\
incr reopen_count +1 c4470a11e2f3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4de\n\
edge blocks -> 018f3c2e7a117b3c9d5e4c2f1a0b9e51\n\
edge cites -> 018f3c2e7a117b3c9d5e4c2f1a0bd3a5 pin=c4410f0e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b65\n\
edge implements -> 018f3c2e7a117b3c9d5e4c2f1a0b9e40\n\
edge mentions -> 018f3c2e7a117b3c9d5e4c2f1a0b9e52\n\
---\n\
Writers lock byte ranges of LOCK, never the whole file.\n\
See #52 for the reader registry this depends on.\n\
\n";

    /// [F14 §17.1]: the task node parses, re-encodes byte-identically and carries the body and fields it spells.
    #[test]
    fn task_example() {
        let nf = check_canonical(TASK_EXAMPLE);
        assert_eq!(
            nf.body.as_deref(),
            Some(
                "Writers lock byte ranges of LOCK, never the whole file.\nSee #52 for the reader registry this depends on.\n"
            )
        );
        assert_eq!(nf.fields["estimate"], IVal::Int(3));
    }

    /// [F14 §17.2]: the file node, the root node, both anchor lines in `full` mode; the `hash-only` line; the derived
    /// uids, the anchor digests and the window.
    #[test]
    fn file_root_and_anchors() {
        check_canonical(
            "moirai-node 1\n\
uid: d706fcde60f0b6cbf56c23297162b652\n\
kind: artifact\n\
status: present\n\
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df 2026-09-21T14:02:11.483Z\n\
updated: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad 2026-09-26T09:12:40.118Z\n\
field aliases: [crates/engine/src/lock.rs]\n\
field artifact_kind: source\n\
field bytes: 18231\n\
field observed_blob: sha1:de177738b58e970465382658e69b18745029e248\n\
field observed_git: sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1\n\
field oid: sha1:de177738b58e970465382658e69b18745029e248\n\
field origin_path: crates/engine/src/lock.rs\n\
field path: crates/engine/src/sync/lock.rs\n\
field relink: lazy/file-id\n\
field root: project\n",
        );
        check_canonical(
            "moirai-node 1\n\
uid: 5bd0e29e6afc4a73557e4cbdf7d34c33\n\
kind: area\n\
title: root:project\n\
status: active\n\
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df 2026-09-21T14:02:11.483Z\n\
updated: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad 2026-09-26T09:12:40.118Z\n\
field path_moves: <<\n  [\"00117336598351118336\",\"explicit\",\"docs/plan/\",\"docs/archive/plan/\",\"sha1:80bb9dad56a25b8ec857f344172c71c9d8fe4ca6\"]\n>>\n\
field root: project\n",
        );
        let head = "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e51\nkind: task\ntitle: t\nstatus: open\n\
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n\
updated: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n";
        let file_a = "anchor 6b8c72caccb5d1cb7d3c9fc37f1ac98e -> d706fcde60f0b6cbf56c23297162b652 kind=file mode=live watch=header blob=sha1:de177738b58e970465382658e69b18745029e248 captured=bcf306557fb04ecf570128703b7ac5fe v=1\n";
        let hash_only = "anchor a11bbeaab98f3fa9a695506f8ded56e0 -> d706fcde60f0b6cbf56c23297162b652 kind=symbol mode=live watch=header scope=\"rust:struct LockFile/impl LockFile/fn acquire\" quote_h=3e747c4056197b57203a29bb675bdccf prefix_h=2622bf4975bc646f601cc36a812296a0 suffix_h=1144314642f739afb24bcd5aeaec0b44 hint=88-131 window=AwACAB86Apx9DrRRyNI span=xxh3:9c1f0a2b3c4d5e6f blob=sha1:de177738b58e970465382658e69b18745029e248 git=sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1 captured=f6ba05cb0b8d5577c95cd8082b4367c4 v=1\n";
        let nf = check_canonical(&format!("{head}{file_a}{hash_only}"));
        assert_eq!(nf.anchors.len(), 2);
        let full = "anchor a11bbeaab98f3fa9a695506f8ded56e0 -> d706fcde60f0b6cbf56c23297162b652 kind=symbol mode=live watch=header scope=\"rust:struct LockFile/impl LockFile/fn acquire\" quote_h=3e747c4056197b57203a29bb675bdccf prefix_h=2622bf4975bc646f601cc36a812296a0 suffix_h=1144314642f739afb24bcd5aeaec0b44 quote=\"pub fn acquire(&self, timeout: Duration) -> Result<Guard>\" prefix=\" Blocks until the byte is ours.\\n\" suffix=\" {\\nlet mut spins = 0u32;\\nloop {\\n\" hint=88-131 window=AwACAB86Apx9DrRRyNI span=xxh3:9c1f0a2b3c4d5e6f blob=sha1:de177738b58e970465382658e69b18745029e248 git=sha1:75bea42e34a5942eb6593b2867480e8fbc95eed1 captured=f6ba05cb0b8d5577c95cd8082b4367c4 v=1\n";
        let nf2 = check_canonical(&format!("{head}{file_a}{full}"));
        assert_eq!(
            nf2.anchors[1].props.digests, nf.anchors[1].props.digests,
            "full and hash-only carry one selector block"
        );
        assert_eq!(
            crate::value::uid_file("project", "crates/engine/src/lock.rs", None),
            <[u8; 16]>::try_from(unhex("d706fcde60f0b6cbf56c23297162b652").unwrap()).unwrap()
        );
        assert_eq!(
            hex(&crate::value::uid_root("project")),
            "5bd0e29e6afc4a73557e4cbdf7d34c33"
        );
    }

    /// [F14 §17.3] the tombstone; [F14 §17.4] conflict lines.
    #[test]
    fn tombstone_and_conflicts() {
        check_canonical(
            "moirai-node 1\n\
uid: 018f3c2e7a117b3c9d5e4c2f1a0b9e40\n\
kind: task\n\
title: Reader registry\n\
deleted: ca0f95ceb140682ca1c5c708146ed5080c9dbee17d53c7bd9eb509b1207f4f7ad 2026-09-26T09:12:40.118Z\n\
field reason: dup of the lock protocol task\n\
edge blocks -> 018f3c2e7a117b3c9d5e4c2f1a0b9e77 flagged\n\
edge cites -> 018f3c2e7a117b3c9d5e4c2f1a0bd3a5\n\
edge mentions -> 018f3c2e7a117b3c9d5e4c2f1a0b9e52\n",
        );
        let nf = check_canonical(
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\n\
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n\
updated: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n\
conflict body class=TextHunk base=\"Pairs are batched per frame.\\n\" ours=\"Pairs are batched per archetype.\\n\" theirs=\"Pairs are batched per grid cell.\\n\"\n\
conflict field.estimate class=FieldEdit base=3 ours=5 theirs=\n\
conflict status class=StatusFork base=in_progress ours=done/completed theirs=cancelled/obsolete\n",
        );
        assert_eq!(nf.conflicts.len(), 3);
    }

    /// [F14 §9.1] superset inputs parse to the canonical file; §9.2 negatives are refused.
    #[test]
    fn superset_and_negatives() {
        let s = Schema::core();
        let cx = Ctx {
            schema: &s,
            path_uid: None,
        };
        let canon = "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\nstatus: open\n\
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n\
updated: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n";
        let sloppy = "\u{FEFF}moirai-node 1\r\nkind: task  \r\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\r\ntitle: \"\\u0074\"\r\npriority: P2\r\n\
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\r\n\
updated: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df";
        let nf = parse(sloppy.as_bytes(), &cx).unwrap();
        assert_eq!(String::from_utf8(encode(&nf, &s)).unwrap(), canon);
        let negatives = [
            "moirai-node 2\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: widget\ntitle: t\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\nfield estimate: 70000\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\nfield bogus: 1\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\ntitle: u\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\n<<<<<<< ours\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\nfield reopen_count: 3\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\nedge blocks -> 018f3c2e7a117b3c9d5e4c2f1a0b9e51 flagged\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: task\ntitle: t\nconflict status class=DATA base= ours= theirs=\n",
            "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e77\nkind: artifact\ntitle: t\n",
        ];
        for n in negatives {
            assert!(parse(n.as_bytes(), &cx).is_err(), "should refuse:\n{n}");
        }
    }

    /// [F18 §5.1] R-17's grammar; [F18 §2.9] I-F9: an empty quote and a `lines` window without a hash are refused.
    #[test]
    fn relink_and_if9() {
        for ok in [
            "explicit/intent",
            "lazy/oid+ctime",
            "merge-observation/move",
            "merge-compose/prefix",
            "agent/similarity/0.81",
            "confirmed/edited+moved/1.00",
            "policy/manual",
            "owner/path-reused",
        ] {
            assert!(is_relink(ok), "{ok}");
        }
        for bad in [
            "",
            "lazy/magic",
            "lazy",
            "agent/similarity",
            "agent/tie/0.50",
            "agent/weak/0.5",
            "agent/weak/1.0",
            "merge-compose/file-id",
            "Lazy/file-id",
        ] {
            assert!(!is_relink(bad), "{bad}");
        }
        let s = Schema::core();
        let cx = Ctx {
            schema: &s,
            path_uid: None,
        };
        let head = "moirai-node 1\nuid: 018f3c2e7a117b3c9d5e4c2f1a0b9e51\nkind: task\ntitle: t\nstatus: open\n\
created: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n\
updated: cc4e084fb20d5ba50ba22ca5874d846c8ccda0ca92161fac5ae87e00eb07440df\n";
        let lines = |w: &str| {
            format!(
                "{head}anchor a11bbeaab98f3fa9a695506f8ded56e0 -> d706fcde60f0b6cbf56c23297162b652 kind=lines mode=live watch=header hint=88-131 window={w} span=xxh3:9c1f0a2b3c4d5e6f blob=sha1:de177738b58e970465382658e69b18745029e248 captured=f6ba05cb0b8d5577c95cd8082b4367c4 v=1\n"
            )
        };
        assert!(parse(lines("AQAAAHeq").as_bytes(), &cx).is_ok());
        assert!(parse(lines("AAAAAA").as_bytes(), &cx).is_err());
        let empty_quote = format!(
            "{head}anchor a11bbeaab98f3fa9a695506f8ded56e0 -> d706fcde60f0b6cbf56c23297162b652 kind=quote mode=live watch=span quote_h=af1349b9f5f9a1a6a0404dea36dcc949 prefix_h=2622bf4975bc646f601cc36a812296a0 suffix_h=1144314642f739afb24bcd5aeaec0b44 hint=88-131 window=AwACAB86Apx9DrRRyNI span=xxh3:9c1f0a2b3c4d5e6f blob=sha1:de177738b58e970465382658e69b18745029e248 captured=f6ba05cb0b8d5577c95cd8082b4367c4 v=1\n"
        );
        assert!(parse(empty_quote.as_bytes(), &cx).is_err());
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    fn edit_byte() -> impl Strategy<Value = u8> {
        prop_oneof![
            8 => 0x20u8..0x7F,
            1 => Just(b'\n'),
            1 => Just(b'\r'),
            1 => Just(b'\t'),
        ]
    }

    proptest! {
        /// [F14 §9.1], §15 rule 3: the §17.1 node with up to three bytes changed, inserted or removed is refused, or its
        /// parse re-encodes to a canonical file that parses back to the same node.
        #[test]
        fn damaged_node_reencodes_canonically(
            edits in proptest::collection::vec((any::<usize>(), 0u8..3, edit_byte()), 1..4),
        ) {
            let s = Schema::core();
            let cx = Ctx { schema: &s, path_uid: None };
            let mut b = super::tests::TASK_EXAMPLE.as_bytes().to_vec();
            for (at, op, v) in edits {
                let i = at % b.len();
                match op {
                    0 => b[i] = v,
                    1 => b.insert(i, v),
                    _ => {
                        b.remove(i);
                    }
                }
            }
            if let Ok(n) = parse(&b, &cx) {
                let e = encode(&n, &s);
                let back = parse(&e, &cx).unwrap_or_else(|x| panic!("{x}\n{}", String::from_utf8_lossy(&e)));
                prop_assert_eq!(&back, &n);
                prop_assert_eq!(encode(&back, &s), e);
            }
        }
    }
}
