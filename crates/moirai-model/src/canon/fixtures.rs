//! E3 ([PLAN §7]; [PLAN §3.2] WP-21, WP-91): the model's encoder reproduces every case of `fixtures/canonical/`.
//! The harness reads the case format of `fixtures/canonical/INDEX.md` §2–§4, builds model states from the state
//! notation (§3), projects them with [`canonical_state`] and compares the item-10 bytes, `changeset_digest`, the
//! commit-id input C and `commit_id` byte for byte; the merge-family cases also run the model's typed merge
//! ([`crate::merge`]) on their `base-state`, `p-state` and `theirs-state` (E5).

use super::*;
use crate::schema::{Elem, EnumItem, Shape, Ty};
use crate::state::{EdgeKey, Tomb};
use std::path::PathBuf;

/// One case: its id, line directives (repeatable, in order) and blocks.
#[derive(Debug, Default)]
pub struct Case {
    /// The id.
    pub id: String,
    /// Line directives.
    pub lines: BTreeMap<String, Vec<String>>,
    /// Block directives, every line of the block as written.
    pub blocks: BTreeMap<String, Vec<String>>,
}

impl Case {
    /// A line directive's single value.
    pub fn line(&self, name: &str) -> Option<&str> {
        self.lines
            .get(name)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    /// A block's lines (empty when absent).
    pub fn block(&self, name: &str) -> &[String] {
        self.blocks.get(name).map_or(&[], Vec::as_slice)
    }
}

/// The directory of the canonical fixtures.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/canonical/cases")
}

/// Parses a `.cases` file ([INDEX] §2.1).
pub fn parse_cases(text: &str) -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    let mut block: Option<String> = None;
    for line in text.split('\n') {
        if let Some(id) = line.strip_prefix("%% case ") {
            cur = Some(Case {
                id: id.trim().to_string(),
                ..Case::default()
            });
            block = None;
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        if line == "%% end" {
            out.push(cur.take().expect("a case"));
            block = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("%% ") {
            match rest.split_once(' ') {
                Some((name, value)) => {
                    c.lines
                        .entry(name.to_string())
                        .or_default()
                        .push(value.to_string());
                    block = None;
                }
                None => {
                    c.blocks.entry(rest.to_string()).or_default();
                    block = Some(rest.to_string());
                }
            }
            continue;
        }
        if let Some(b) = &block {
            c.blocks
                .get_mut(b)
                .expect("the open block")
                .push(line.to_string());
        }
    }
    out
}

/// Every case of one file.
pub fn cases(file: &str) -> Vec<Case> {
    let p = dir().join(file);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    parse_cases(&text)
}

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(s.len().is_multiple_of(2), "odd hex {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|_| panic!("bad hex {s}")))
        .collect()
}

/// A hex block ([INDEX] §2.3) as byte groups, one per comment line that starts a group, with the comment.
pub fn hex_groups(lines: &[String]) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for l in lines {
        let (data, comment) = match l.split_once(';') {
            Some((d, c)) => (d, Some(c.trim())),
            None => (l.as_str(), None),
        };
        if let Some(c) = comment
            && data.trim().is_empty()
        {
            out.push((c.to_string(), Vec::new()));
            continue;
        }
        if out.is_empty() {
            out.push((String::new(), Vec::new()));
        }
        let bytes = match data.split_once(" * ") {
            Some((h, n)) => unhex(h).repeat(n.trim().parse().expect("a count")),
            None => unhex(data),
        };
        out.last_mut().expect("a group").1.extend_from_slice(&bytes);
    }
    out
}

/// A hex block's bytes.
pub fn hex_bytes(lines: &[String]) -> Vec<u8> {
    hex_groups(lines).into_iter().flat_map(|g| g.1).collect()
}

/// Decodes a JSON string token (with its quotes).
pub fn json(tok: &str) -> String {
    let s = tok
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or_else(|| panic!("not a JSON string: {tok}"));
    let mut out = String::new();
    let mut it = s.chars().peekable();
    let hex4 = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| -> u32 {
        let h: String = (0..4)
            .map(|_| it.next().expect("four hex digits"))
            .collect();
        u32::from_str_radix(&h, 16).expect("hex")
    };
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next().expect("an escape") {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let hi = hex4(&mut it);
                let cp = if (0xD800..0xDC00).contains(&hi) {
                    assert_eq!(it.next(), Some('\\'));
                    assert_eq!(it.next(), Some('u'));
                    let lo = hex4(&mut it);
                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                } else {
                    hi
                };
                out.push(char::from_u32(cp).expect("a scalar value"));
            }
            e => panic!("unknown escape \\{e}"),
        }
    }
    out
}

/// Splits a line into its indentation and tokens ([INDEX] §3.1): a token holding a JSON string runs to its closing
/// quote.
pub fn tokens(line: &str) -> (usize, Vec<String>) {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut esc = false;
    for c in line[indent..].chars() {
        if in_str {
            cur.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            ' ' => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            '"' => {
                in_str = true;
                cur.push(c);
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    (indent, out)
}

fn uid_hex(s: &str) -> Uid {
    let b = unhex(s);
    Uid(b
        .try_into()
        .unwrap_or_else(|_| panic!("a 16-byte uid: {s}")))
}

fn id32(s: &str) -> [u8; 32] {
    unhex(s).try_into().expect("a 32-byte id")
}

fn b16(s: &str) -> [u8; 16] {
    unhex(s).try_into().expect("16 bytes")
}

fn algo(s: &str) -> Algo {
    match s {
        "sha1" => Algo::Sha1,
        "sha256" => Algo::Sha256,
        o => panic!("unknown algorithm {o}"),
    }
}

fn oid_of(a: &str, h: &str) -> Oid {
    Oid {
        algo: algo(a),
        digest: unhex(h),
    }
}

fn colon_oid(s: &str) -> Option<Oid> {
    if s == "-" {
        return None;
    }
    let (a, h) = s.split_once(':').expect("algo:hex");
    Some(oid_of(a, h))
}

fn be_u64(s: &str) -> u64 {
    u64::from_str_radix(s, 16).expect("16 hex digits")
}

/// The uid ↔ `#N` table of one case: every uid a state names gets one `#N`.
#[derive(Default)]
pub struct Ids {
    nid: BTreeMap<Uid, Nid>,
    /// `#N` → uid.
    pub uid: BTreeMap<Nid, Uid>,
}

impl Ids {
    /// The `#N` of a uid, allocated on first sight.
    pub fn nid(&mut self, u: Uid) -> Nid {
        if let Some(n) = self.nid.get(&u) {
            return *n;
        }
        let n = Nid(self.nid.len() as u32 + 1);
        self.nid.insert(u, n);
        self.uid.insert(n, u);
        n
    }

    /// The uid of a `#N`.
    pub fn uid_of(&self, n: Nid) -> Uid {
        self.uid[&n]
    }
}

/// Parses one value from a token stream ([INDEX] §3.3): `None` for `absent`.
fn value(t: &mut std::slice::Iter<'_, String>, ids: &mut Ids) -> Option<Value> {
    let word = t.next().expect("a value").as_str();
    let mut next = || t.next().expect("a value token").clone();
    Some(match word {
        "absent" => return None,
        "bool" => Value::Bool(next() == "true"),
        "int" => Value::Int(next().parse().expect("an int")),
        "f64" => f64_value(f64::from_bits(be_u64(&next()))),
        "enum" => Value::Enum(next()),
        "text" => Value::Text(json(&next())),
        "ref" => Value::Ref(ids.nid(uid_hex(&next()))),
        "commit" => Value::Commit(id32(&next())),
        "path" => {
            let root = next();
            Value::Path(PathVal {
                root,
                text: json(&next()),
            })
        }
        "oid" => {
            let a = next();
            Value::Oid(oid_of(&a, &next()))
        }
        "pathmove" => pathmove(&mut next),
        "set" => {
            let elem = next();
            let n: usize = next().parse().expect("a count");
            let mut items = Vec::new();
            for _ in 0..n {
                items.push(match elem.as_str() {
                    "int" => Value::Int(next().parse().expect("an int")),
                    "enum" => Value::Enum(next()),
                    "text" => Value::Text(json(&next())),
                    "ref" => Value::Ref(ids.nid(uid_hex(&next()))),
                    "commit" => Value::Commit(id32(&next())),
                    "path" => {
                        let root = next();
                        Value::Path(PathVal {
                            root,
                            text: json(&next()),
                        })
                    }
                    "oid" => {
                        let a = next();
                        Value::Oid(oid_of(&a, &next()))
                    }
                    "pathmove" => pathmove(&mut next),
                    o => panic!("unknown set element {o}"),
                });
            }
            return Value::set(items);
        }
        o => panic!("unknown value word {o}"),
    })
}

fn pathmove(next: &mut dyn FnMut() -> String) -> Value {
    let hlc = be_u64(&next());
    let class = match next().as_str() {
        "explicit" => MoveClass::Explicit,
        "confirmed" => MoveClass::Confirmed,
        "committed" => MoveClass::Committed,
        "observed" => MoveClass::Observed,
        o => panic!("unknown pathmove class {o}"),
    };
    let fr = next();
    let from = PathVal {
        root: fr,
        text: json(&next()),
    };
    let tr = next();
    let to = PathVal {
        root: tr,
        text: json(&next()),
    };
    Value::PathMove(Box::new(PathMove {
        hlc,
        class,
        from,
        to,
        git: colon_oid(&next()),
    }))
}

/// base64url ([F14 §2.6]: [RFC 4648] §5 without padding, the unused low bits zero).
fn b64url(s: &str) -> Vec<u8> {
    assert!(s.len() % 4 != 1, "a base64url length of 1 mod 4: {s}");
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => panic!("not base64url: {s}"),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    assert_eq!(acc, 0, "non-zero unused bits: {s}");
    out
}

/// The BLAKE3-128 digest of an anchor text as a fixture writes it: a JSON string, or `%` and the base64url of its bytes
/// when they are not UTF-8 (`fixtures/moi/INDEX.md` §3; [F14 §5.7]).
fn atext_h(v: &str) -> [u8; 16] {
    match v.strip_prefix('%') {
        Some(b) => b3_128(&b64url(b)),
        None => b3_128(json(v).as_bytes()),
    }
}

/// An anchor from its `key=value` attributes ([INDEX] §3.4): texts enter as their BLAKE3-128 digests.
pub fn anchor(attrs: &[String]) -> Anchor {
    let mut a = Anchor::default();
    for t in attrs {
        let (k, v) = t
            .split_once('=')
            .unwrap_or_else(|| panic!("an attribute: {t}"));
        match k {
            "kind" => a.kind = v.into(),
            "mode" => a.mode = v.into(),
            "watch" => a.watch = v.into(),
            "resolver" => a.resolver = v.parse().expect("a version"),
            "captured" => a.captured = b16(v),
            "pred" => a.pred = Some(b16(v)),
            "scope" => a.scope = unhex(v),
            "quote" => a.quote_h = Some(atext_h(v)),
            "prefix" => a.prefix_h = Some(atext_h(v)),
            "suffix" => a.suffix_h = Some(atext_h(v)),
            "end" => a.end_h = Some(atext_h(v)),
            "quote_h" => a.quote_h = Some(b16(v)),
            "prefix_h" => a.prefix_h = Some(b16(v)),
            "suffix_h" => a.suffix_h = Some(b16(v)),
            "end_h" => a.end_h = Some(b16(v)),
            "occurrence" => a.occurrence = Some(v.parse().expect("an occurrence")),
            "hint" => {
                let (f, l) = v.split_once('-').expect("first-last");
                a.hint = Some((f.parse().expect("a line"), l.parse().expect("a line")));
            }
            "window" => a.window = unhex(v),
            "span_hash" => a.span_hash = Some(be_u64(v)),
            "blob" => a.blob = colon_oid(v),
            "git" => a.git = colon_oid(v),
            "marker" => a.marker = json(v),
            o => panic!("unknown anchor attribute {o}"),
        }
    }
    a
}

/// Edge properties from `[pin <hex64>] [flagged] [anchor <attrs>]`.
fn edge_props(t: &[String]) -> EdgeProps {
    let mut p = EdgeProps::default();
    let mut i = 0;
    while i < t.len() {
        match t[i].as_str() {
            "pin" => {
                p.pinned = Some(id32(&t[i + 1]));
                i += 2;
            }
            "flagged" => {
                p.flagged = true;
                i += 1;
            }
            "anchor" => {
                p.anchor = Some(Box::new(anchor(&t[i + 1..])));
                i = t.len();
            }
            o => panic!("unknown edge property {o}"),
        }
    }
    p
}

/// A `&'static str` for a schema name the model's schema rows hold statically.
fn stat(s: &str) -> &'static str {
    const NAMES: &[&str] = &[
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
        "column",
        "bitmap",
        "priority",
        "revision-integer",
        "timestamp",
        "delete-wins",
        "resurrect",
        "restrict",
        "restrict-cascade-reparent",
        "restrict-reassign",
        "restrict-repoint",
        "drop",
        "drop-notify",
        "drop-src-suspect",
        "tombstone",
        "tombstone-src-suspect",
        "drop-rollups",
        "repoint-or-flag",
        "drop-reopen",
        "retain-warn",
        "retain",
        "recompute",
        "retain-anchors",
    ];
    NAMES
        .iter()
        .find(|n| **n == s)
        .copied()
        .unwrap_or_else(|| panic!("no static name {s}"))
}

fn ty_of(t: &str, elem: &str) -> Ty {
    match t {
        "bool" => Ty::Bool,
        "int" => Ty::Int,
        "counter" => Ty::Counter,
        "f64" => Ty::F64,
        "enum" => Ty::Enum,
        "text" => Ty::Text,
        "sym" => Ty::Sym,
        "ref" => Ty::Ref,
        "commitref" => Ty::Commit,
        "path" => Ty::Path,
        "oid" => Ty::Oid,
        "pathmove" => Ty::PathMove,
        "body" => Ty::Body,
        "set" => Ty::Set(match elem {
            "sym" => Elem::Sym,
            "path" => Elem::Path,
            "pathmove" => Elem::PathMove,
            "int" => Elem::Int,
            o => panic!("unknown set element type {o}"),
        }),
        o => panic!("unknown type {o}"),
    }
}

fn storage(s: &str) -> Storage {
    match s {
        "header" => Storage::Header,
        "flag" => Storage::Flag,
        "cold" => Storage::Cold,
        "field" => Storage::Field,
        "title" => Storage::Title,
        "body" => Storage::Body,
        o => panic!("unknown storage {o}"),
    }
}

fn kinds(s: &str) -> Ends {
    if s == "any" {
        Ends::Any
    } else {
        Ends::Kinds(s.split(',').map(str::to_string).collect())
    }
}

fn names(s: &str) -> Vec<String> {
    if s == "-" {
        Vec::new()
    } else {
        s.split(',').map(str::to_string).collect()
    }
}

/// A schema line ([INDEX] §3.6) as a model item.
pub fn schema_item(t: &[String], ids: &mut Ids) -> Item {
    let attr = |k: &str| -> String {
        t.iter()
            .find_map(|x| x.strip_prefix(&format!("{k}=")).map(str::to_string))
            .unwrap_or_else(|| panic!("no attribute {k} in {t:?}"))
    };
    let flag = |k: &str| attr(k) == "true";
    match t[1].as_str() {
        "field" => {
            let default = t.iter().position(|x| x == "default").map(|i| {
                let mut it = t[i + 1..].iter();
                value(&mut it, ids).expect("a default is never absent")
            });
            let range = t.iter().find_map(|x| x.strip_prefix("range=")).map(|r| {
                let (a, b) = r.split_once("..").expect("min..max");
                (a.parse().expect("min"), b.parse().expect("max"))
            });
            Item::Field(FieldItem {
                kind: (t[2] != "*").then(|| t[2].clone()),
                name: t[3].clone(),
                ty: ty_of(&attr("type"), &attr("elem")),
                class: stat(&attr("class")),
                storage: storage(&attr("storage")),
                decl: attr("decl").parse().expect("decl"),
                optional: flag("optional"),
                default,
                range,
                one_line: flag("one_line"),
                ascii: flag("ascii"),
                shape: Shape::Plain,
                index: stat(&attr("index")),
                coerce: stat(&attr("coerce")),
                retired: flag("retired"),
            })
        }
        "query" => Item::Query(QueryItem {
            name: t[2].clone(),
            lq_version: attr("lq").parse().expect("lq"),
            params: json(&attr("params")),
            shape: json(&attr("shape")),
            budget: json(&attr("budget")),
            text: json(&attr("text")),
        }),
        "kind" => Item::Kind(KindItem {
            name: t[2].clone(),
            id: 64,
            uid: match attr("uid_derivation").as_str() {
                "random" => UidDerivation::Random,
                "file-key" => UidDerivation::FileKey,
                o => panic!("unknown uid derivation {o}"),
            },
            root_variant: attr("root_variant") == "root-key",
            existence_policy: stat(&attr("existence_policy")),
            title_derived: flag("title_derived"),
            immutable_fields: flag("immutable_fields"),
            has_done: flag("has_done"),
            done_derived: flag("done_derived"),
            retired: flag("retired"),
        }),
        "enum" => Item::Enum(EnumItem {
            kind: (t[2] != "*").then(|| t[2].clone()),
            field: t[3].clone(),
            name: t[4].clone(),
            rank: attr("sort_rank").parse().expect("rank"),
            side: flag("side"),
            done: flag("done"),
            covers: names(&attr("covers")),
            retired: flag("retired"),
        }),
        "edge" => Item::Edge(EdgeItem {
            name: t[2].clone(),
            id: 64,
            class: match attr("eclass").as_str() {
                "structural" => EdgeClass::Structural,
                _ => EdgeClass::Historical,
            },
            on_dst: stat(&attr("on_dst")),
            on_src: stat(&attr("on_src")),
            acyclic: match attr("acyclic").as_str() {
                "none" => Acyclic::None,
                "forest" => Acyclic::Forest,
                "precedence" => Acyclic::Precedence,
                "dag" => Acyclic::Dag,
                "by-construction" => Acyclic::ByConstruction,
                o => panic!("unknown acyclic {o}"),
            },
            card: match attr("card").as_str() {
                "many" => Card::Many,
                "max-1-per-src" => Card::Max1PerSrc,
                "max-1-active-per-dst" => Card::Max1ActivePerDst,
                "chain-1" => Card::Chain1,
                "typical-1" => Card::Typical1,
                "anchors-min-1" => Card::AnchorsMin1,
                o => panic!("unknown card {o}"),
            },
            max_depth: attr("max_depth").parse().expect("depth"),
            props: match attr("props").as_str() {
                "none" => Props::None,
                "pinned" => Props::Pinned,
                "flagged" => Props::Flagged,
                "anchor" => Props::Anchor,
                o => panic!("unknown props {o}"),
            },
            symmetric: flag("symmetric"),
            same_kind: flag("same_kind"),
            lq_name: attr("lq_name"),
            src: kinds(&attr("src_kinds")),
            dst: kinds(&attr("dst_kinds")),
            reverse: names(&attr("reverse_names")),
            reading: json(&attr("reading")),
            retired: flag("retired"),
        }),
        o => panic!("unknown schema item {o}"),
    }
}

/// The aspect a conflict key names, and the tokens after it ([INDEX] §3.5).
fn conflict_key<'a>(t: &'a [String], ids: &mut Ids) -> (Aspect, &'a [String]) {
    match t[0].as_str() {
        "existence" => (Aspect::Existence, &t[1..]),
        "status" => (Aspect::Status, &t[1..]),
        "hierarchy" => (Aspect::Hierarchy, &t[1..]),
        "observation" => (Aspect::Observation, &t[1..]),
        "body" => (Aspect::Body, &t[1..]),
        "field" => (Aspect::Field(t[1].clone()), &t[2..]),
        "counter" => (Aspect::Counter(t[1].clone()), &t[2..]),
        "edge" => {
            let dst = ids.nid(uid_hex(&t[2]));
            let disc = (t.len() > 4 && t[3].len() == 32).then(|| uid_hex(&t[3]));
            let rest = if disc.is_some() { &t[4..] } else { &t[3..] };
            (
                Aspect::Edge(EdgeKey {
                    kind: t[1].clone(),
                    dst,
                    disc,
                }),
                rest,
            )
        }
        o => panic!("unknown conflict key {o}"),
    }
}

/// One side of a conflict value ([INDEX] §3.5): its key value and, for a `live` existence side, its node image.
fn side(
    a: &Aspect,
    t: &[String],
    image: &[Vec<String>],
    ids: &mut Ids,
    schema: &Schema,
) -> (Option<KVal>, Option<Image>) {
    if t.first().map(String::as_str) == Some("absent") {
        return (None, None);
    }
    let v = match a {
        Aspect::Existence => match t[0].as_str() {
            "live" => {
                let kind = t[1].clone();
                let mut n = Node::new(Uid([1; 16]), &kind, schema, Default::default());
                for l in image {
                    key_line(&mut n, l, ids, schema);
                }
                return (Some(KVal::Live(kind)), Some(node_image(schema, &n)));
            }
            "deleted" => KVal::Deleted {
                kind: t[1].clone(),
                reason: Some(json(&t[2])),
                replaced_by: (t[3] != "-").then(|| ids.nid(uid_hex(&t[3]))),
            },
            o => panic!("unknown existence side {o}"),
        },
        Aspect::Status => KVal::Status {
            status: t[0].clone(),
            resolution: t[1].clone(),
        },
        Aspect::Hierarchy => KVal::Hierarchy {
            parent: (t[0] != "-").then(|| ids.nid(uid_hex(&t[0]))),
            order: (t[1] != "-").then(|| json(&t[1])),
        },
        Aspect::Field(_) | Aspect::Counter(_) => {
            let mut it = t.iter();
            return (value(&mut it, ids).map(KVal::Value), None);
        }
        Aspect::Observation => {
            let mut vs = Vec::new();
            for part in t.split(|x| x == "|") {
                let mut it = part.iter();
                vs.push(value(&mut it, ids));
            }
            assert_eq!(vs.len(), 6, "six observation values");
            KVal::Observation(vs)
        }
        Aspect::Body => KVal::Body(json(&t[0])),
        Aspect::Edge(_) => {
            assert_eq!(t[0], "present");
            KVal::Edge(edge_props(&t[1..]))
        }
    };
    (Some(v), None)
}

/// A key line of a node (or of a node image): status, parent, field, counter, body, edge, at.
fn key_line(n: &mut Node, t: &[String], ids: &mut Ids, schema: &Schema) {
    match t[0].as_str() {
        "status" => {
            n.status = t[1].clone();
            n.resolution = t[2].clone();
        }
        "parent" => {
            n.parent = (t[1] != "-").then(|| ids.nid(uid_hex(&t[1])));
            n.order = (t[2] != "-").then(|| json(&t[2]));
        }
        "field" => {
            let mut it = t[2..].iter();
            let v = value(&mut it, ids);
            n.set_field(schema, &t[1], v);
        }
        "counter" => {
            let c: i64 = t[2].parse().expect("a total");
            if c != 0 {
                n.fields.insert(t[1].clone(), Value::Counter(c));
            }
        }
        "body" => {
            let b = json(&t[1]);
            n.body = (!b.is_empty()).then_some(b);
        }
        "edge" => {
            let dst = ids.nid(uid_hex(&t[2]));
            n.out.insert(
                EdgeKey {
                    kind: t[1].clone(),
                    dst,
                    disc: None,
                },
                edge_props(&t[3..]),
            );
        }
        "at" => {
            let dst = ids.nid(uid_hex(&t[1]));
            n.out.insert(
                EdgeKey {
                    kind: "at".into(),
                    dst,
                    disc: Some(uid_hex(&t[2])),
                },
                EdgeProps {
                    anchor: Some(Box::new(anchor(&t[3..]))),
                    ..EdgeProps::default()
                },
            );
        }
        o => panic!("unknown key line {o}"),
    }
}

/// A state block ([INDEX] §3) as a model state: nodes by `#N` with the case's uid table, schema items first.
pub fn state(lines: &[String], ids: &mut Ids) -> State {
    let toks: Vec<(usize, Vec<String>)> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| tokens(l))
        .collect();
    let mut st = State::default();
    let mut i = 0;
    while i < toks.len() {
        let (ind, t) = &toks[i];
        i += 1;
        if *ind != 0 || t[0] != "schema" {
            continue;
        }
        // A conflicted named query (`fixtures/moi/INDEX.md` §3): `schema query <name> conflict <class>`, then its
        // sides, each `<side> absent` or the item's attributes.
        if t.len() >= 5 && t[1] == "query" && t[3] == "conflict" {
            let mut c = Conflict {
                class: t[4].clone(),
                base: None,
                ours: None,
                theirs: None,
                prov: None,
                images: [None, None, None],
            };
            while i < toks.len() && toks[i].0 >= 2 {
                let s = &toks[i].1;
                i += 1;
                let v = (s[1] != "absent").then(|| {
                    let mut line = vec!["schema".to_string(), "query".to_string(), t[2].clone()];
                    line.extend(s[1..].iter().cloned());
                    KVal::Item(schema_item(&line, ids))
                });
                match s[0].as_str() {
                    "base" => c.base = v,
                    "ours" => c.ours = v,
                    "theirs" => c.theirs = v,
                    o => panic!("unknown side {o}"),
                }
            }
            let key = ItemKey::Query(t[2].clone());
            if let Some(KVal::Item(it)) = crate::merge::flat(&KState::Conflict(Box::new(c.clone())))
            {
                st.schema.items.insert(key.clone(), it);
            }
            st.schema_conflicts.insert(key, c);
            continue;
        }
        let it = schema_item(t, ids);
        st.schema.items.insert(it.key(), it);
    }
    let mut i = 0;
    while i < toks.len() {
        let (ind, t) = &toks[i];
        i += 1;
        if *ind != 0 || t[0] != "node" {
            continue;
        }
        let u = uid_hex(&t[1]);
        let n = ids.nid(u);
        let mut node = Node::new(u, &t[2], &st.schema, Default::default());
        if t.get(3).map(String::as_str) == Some("deleted") {
            node.tomb = Some(Tomb {
                reason: Some(json(&t[4])),
                replaced_by: (t[5] != "-").then(|| ids.nid(uid_hex(&t[5]))),
            });
        }
        while i < toks.len() && toks[i].0 >= 2 {
            let t = &toks[i].1;
            i += 1;
            if t[0] != "conflict" {
                match t[0].as_str() {
                    _ if node.live() => key_line(&mut node, t, ids, &st.schema),
                    // A tombstone's title is kept as written ([F07 §6.4]), whatever the kind's default.
                    "field" if t[1] == "title" => {
                        let mut it = t[2..].iter();
                        if let Some(v) = value(&mut it, ids) {
                            node.fields.insert(t[1].clone(), v);
                        }
                    }
                    // Its retained out-edges: flagged and historical edges, and `at` edges with their anchors.
                    "edge" | "at" => key_line(&mut node, t, ids, &st.schema),
                    o => panic!(
                        "a tombstone has no {o} key ([F07 §6.4]): {} {t:?}",
                        crate::value::hex(&u.0)
                    ),
                }
                continue;
            }
            let (a, rest) = conflict_key(&t[1..], ids);
            let class = rest[0].clone();
            let prov = rest
                .iter()
                .position(|x| x == "prov")
                .map(|p| match rest[p + 1].as_str() {
                    "ours" => Side::Ours,
                    _ => Side::Theirs,
                });
            let mut c = Conflict {
                class,
                base: None,
                ours: None,
                theirs: None,
                prov,
                images: [None, None, None],
            };
            while i < toks.len() && toks[i].0 == 4 {
                let s = toks[i].1.clone();
                i += 1;
                let mut image = Vec::new();
                while i < toks.len() && toks[i].0 >= 6 {
                    image.push(toks[i].1.clone());
                    i += 1;
                }
                let (v, img) = side(&a, &s[1..], &image, ids, &st.schema);
                let k = match s[0].as_str() {
                    "base" => 0,
                    "ours" => 1,
                    "theirs" => 2,
                    o => panic!("unknown side {o}"),
                };
                match k {
                    0 => c.base = v,
                    1 => c.ours = v,
                    _ => c.theirs = v,
                }
                c.images[k] = img;
            }
            node.conflicts.insert(a, c);
        }
        st.nodes.insert(n, node);
    }
    st
}

/// The `commit` block ([INDEX] §4.2) as a header.
pub fn header(lines: &[String]) -> Header {
    let mut h = Header {
        kind: String::new(),
        parents: Vec::new(),
        hlc: 0,
        actor: String::new(),
        role: String::new(),
        session: String::new(),
        git: None,
        message: String::new(),
        schema_version: 0,
        origin: None,
        foreign: None,
    };
    let (mut head, mut base, mut branch, mut worktree) = (None, None, String::new(), String::new());
    for l in lines.iter().filter(|l| !l.trim().is_empty()) {
        let (_, t) = tokens(l);
        match t[0].as_str() {
            "kind" => h.kind = t[1].clone(),
            "parent" => h.parents.push(id32(&t[1])),
            "hlc" => h.hlc = be_u64(&t[1]),
            "actor" => h.actor = json(&t[1]),
            "role" => h.role = json(&t[1]),
            "session" => h.session = json(&t[1]),
            "git-head" => head = (t[1] != "-").then(|| oid_of(&t[1], &t[2])),
            "git-base" => base = (t[1] != "-").then(|| oid_of(&t[1], &t[2])),
            "git-branch" => branch = json(&t[1]),
            "git-worktree" => worktree = json(&t[1]),
            "message" => h.message = json(&t[1]),
            "schema-version" => h.schema_version = t[1].parse().expect("n"),
            "origin" => h.origin = (t[1] != "-").then(|| id32(&t[1])),
            "foreign" => h.foreign = (t[1] != "-").then(|| oid_of(&t[1], &t[2])),
            o => panic!("unknown commit line {o}"),
        }
    }
    if head.is_some() || base.is_some() || !branch.is_empty() || !worktree.is_empty() {
        let a = head
            .as_ref()
            .or(base.as_ref())
            .map_or(Algo::Sha1, |o| o.algo);
        h.git = Some(Git {
            algo: a,
            head: head.map(|o| o.digest),
            branch,
            worktree,
            base: base.map(|o| o.digest),
        });
    }
    h
}

/// The `git-commit` block ([INDEX] §4.3).
pub fn git_commit(lines: &[String]) -> GitCommit {
    let mut g = GitCommit {
        parents: Vec::new(),
        committer_secs: 0,
        author_email: Vec::new(),
        message: Vec::new(),
        schema_version: 0,
        oid: oid_of("sha1", ""),
    };
    let mut fmt = "sha1".to_string();
    let mut oid = String::new();
    for l in lines.iter().filter(|l| !l.trim().is_empty()) {
        let (_, t) = tokens(l);
        match t[0].as_str() {
            "object-format" => fmt = t[1].clone(),
            "oid" => oid = t[1].clone(),
            "parent" => g.parents.push((id32(&t[1]), be_u64(&t[3]))),
            "committer-time" => g.committer_secs = t[1].parse().expect("T"),
            "author-email-hex" => g.author_email = unhex(&t[1]),
            "message-hex" => g.message = unhex(t.get(1).map_or("", String::as_str)),
            "marker-schema-version" => g.schema_version = t[1].parse().expect("n"),
            o => panic!("unknown git-commit line {o}"),
        }
    }
    g.oid = oid_of(&fmt, &oid);
    g
}

/// The item-10 input of a case: `lp(domain) ‖ entries ‖ u64(n)`.
fn digest_input(es: &[Vec<u8>]) -> Vec<u8> {
    let mut v = Vec::new();
    lp(&mut v, b"moirai-changeset-v1");
    for e in es {
        v.extend_from_slice(e);
    }
    v.extend_from_slice(&(es.len() as u64).to_le_bytes());
    v
}

/// The canonical states P and Q of a commit case.
pub fn states(c: &Case) -> (Cs, Cs, Ids) {
    let mut ids = Ids::default();
    let p = state(c.block("p-state"), &mut ids);
    let q = state(c.block("q-state"), &mut ids);
    let (cp, cq) = {
        let uid = |n: Nid| ids.uid_of(n);
        (canonical_state(&p, &uid), canonical_state(&q, &uid))
    };
    (cp, cq, ids)
}

/// Checks one commit case (INDEX §5.1 items 1, 2, 4): the entries, the digest, C and the id; the derived items of a
/// `git-commit` block; N of a `message-input-hex`. Returns the case's `c` bytes.
pub fn check_commit_case(c: &Case) -> Vec<u8> {
    let (cp, cq, _) = states(c);
    let es = entries(&cp, &cq);
    let want = hex_groups(c.block("digest-input"));
    // want[0] is the domain; want[1..n+1] the entries; the last group the count.
    let want_entries: Vec<&(String, Vec<u8>)> =
        want.iter().filter(|g| g.0.starts_with("entry ")).collect();
    for (i, w) in want_entries.iter().enumerate() {
        let got = es.get(i).map(|e| crate::value::hex(e)).unwrap_or_default();
        assert_eq!(got, crate::value::hex(&w.1), "{}: {} differs", c.id, w.0);
    }
    assert_eq!(es.len(), want_entries.len(), "{}: the entry count", c.id);
    let input = digest_input(&es);
    assert_eq!(
        input,
        hex_bytes(c.block("digest-input")),
        "{}: digest-input",
        c.id
    );
    let digest = digest_of(&es);
    assert_eq!(
        crate::value::hex(&digest),
        c.line("changeset-digest").expect("a digest"),
        "{}: changeset-digest",
        c.id
    );
    assert_eq!(
        es.len().to_string(),
        c.line("entry-count").expect("a count"),
        "{}",
        c.id
    );
    let h = header(c.block("commit"));
    let input = commit_input(&h, &digest);
    let want_c = hex_bytes(c.block("c"));
    assert_eq!(
        crate::value::hex(&input),
        crate::value::hex(&want_c),
        "{}: the commit input C",
        c.id
    );
    assert_eq!(
        crate::value::hex(&commit_id(&h, &digest)),
        c.line("commit-id").expect("an id"),
        "{}: commit-id",
        c.id
    );
    if !c.block("git-commit").is_empty() {
        let g = git_commit(c.block("git-commit"));
        let derived = if h.kind == "checkpoint" {
            checkpoint_header(&g)
        } else {
            foreign_header(&g)
        }
        .unwrap_or_else(|| panic!("{}: the git commit derives no header", c.id));
        assert_eq!(
            derived, h,
            "{}: the items derived from the git commit",
            c.id
        );
    }
    if !c.block("message-input-hex").is_empty() {
        let m = normalise_message_bytes(&hex_bytes(c.block("message-input-hex")))
            .unwrap_or_else(|e| panic!("{}: N refused the message: {e:?}", c.id));
        assert_eq!(m, h.message, "{}: N of the input message", c.id);
    }
    want_c
}

const COMMIT_FILES: [&str; 5] = [
    "commits.cases",
    "tombstones.cases",
    "anchors.cases",
    "foreign.cases",
    "checkpoint.cases",
];

/// E3: every commit case of `fixtures/canonical/` reproduces its item 10, `changeset_digest`, C and `commit_id`, and
/// the pairs that share an id by design give identical bytes (INDEX §5.1 item 5).
#[test]
fn every_commit_case_reproduces_its_id() {
    let mut by_id: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    let mut n = 0;
    let mut files: Vec<&str> = COMMIT_FILES.to_vec();
    files.push("normalisation.cases");
    for f in files {
        for c in cases(f).iter().filter(|c| c.blocks.contains_key("c")) {
            let bytes = check_commit_case(c);
            by_id
                .entry(c.line("commit-id").unwrap().to_string())
                .or_default()
                .push((c.id.clone(), bytes));
            n += 1;
        }
    }
    assert_eq!(n, 25, "INDEX §5 lists 25 commit cases");
    let pairs: Vec<_> = by_id.values().filter(|v| v.len() > 1).collect();
    assert_eq!(pairs.len(), 3, "three pairs share an id by design");
    for p in pairs {
        assert!(p.windows(2).all(|w| w[0].1 == w[1].1), "{p:?}");
    }
}

/// The message cases of `normalisation.cases`: N and N_imp ([F07 §5]).
#[test]
fn every_message_case_normalises() {
    let mut n = 0;
    for c in cases("normalisation.cases") {
        let Some(f) = c.line("function") else {
            continue;
        };
        n += 1;
        let input = hex_bytes(c.block("input-hex"));
        match f {
            "N" => match (normalise_message_bytes(&input), c.line("refused")) {
                // The fixture names [F07 §15]'s three proposed cases; [F19 §10.3] has one case, `message`, whose
                // [F19 §10.2] text tells them apart (spec sync 2b S2B-F-16).
                (Err(e), Some(case)) => {
                    assert_eq!(e.get_str("case"), Some("message"), "{}", c.id);
                    let text = match case {
                        "message-utf8" => crate::canon::MESSAGE_UTF8,
                        "message-length" => crate::canon::MESSAGE_LENGTH,
                        "message-trailer" => crate::canon::MESSAGE_TRAILER,
                        other => panic!("{}: refused {other} is not a message case", c.id),
                    };
                    assert_eq!(e.detail, text, "{}", c.id);
                }
                (Ok(out), None) => {
                    assert_eq!(out.as_bytes(), hex_bytes(c.block("output-hex")), "{}", c.id);
                }
                (got, want) => panic!("{}: got {got:?}, want refusal {want:?}", c.id),
            },
            "N_imp" => {
                let out = normalise_imported(&input);
                assert_eq!(out.as_bytes(), hex_bytes(c.block("output-hex")), "{}", c.id);
            }
            o => panic!("unknown function {o}"),
        }
        if let Some(l) = c.line("output-length") {
            assert_eq!(
                hex_bytes(c.block("output-hex")).len().to_string(),
                l,
                "{}",
                c.id
            );
        }
    }
    assert_eq!(n, 25);
}

/// The unit encodings of `values.cases` ([INDEX] §4.4).
#[test]
fn every_value_case_encodes() {
    let mut n = 0;
    for c in cases("values.cases") {
        n += 1;
        let mut ids = Ids::default();
        let lines: Vec<(usize, Vec<String>)> = c
            .block("input")
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| tokens(l))
            .collect();
        let t = &lines[0].1;
        let schema = Schema::default();
        let got: Vec<u8> = match c.line("encoding").expect("an encoding") {
            "cv" => {
                let mut it = t.iter();
                let v = value(&mut it, &mut ids);
                let uid = |n: Nid| ids.uid_of(n);
                cv(v.as_ref(), &uid)
            }
            "delta" => {
                let d: i128 = t[0].trim_start_matches('+').parse().expect("a delta");
                let mut b = vec![u8::from(d < 0)];
                b.extend_from_slice(&(d.unsigned_abs() as u64).to_le_bytes());
                b
            }
            "body" if t[0] == "hash" => {
                let mut b = vec![1u8];
                b.extend_from_slice(&b16(&t[1]));
                b
            }
            "selector" => selector_block(&anchor(&t[1..])),
            "schema-item" => {
                let it = schema_item(t, &mut ids);
                let uid = |n: Nid| ids.uid_of(n);
                item_value(&it, &uid)
            }
            "cstate" => {
                let (a, rest) = conflict_key(&t[1..], &mut ids);
                let prov = rest.iter().position(|x| x == "prov").map(|p| {
                    if rest[p + 1] == "ours" {
                        Side::Ours
                    } else {
                        Side::Theirs
                    }
                });
                let mut cf = Conflict {
                    class: rest[0].clone(),
                    base: None,
                    ours: None,
                    theirs: None,
                    prov,
                    images: [None, None, None],
                };
                let mut i = 1;
                while i < lines.len() {
                    let s = lines[i].1.clone();
                    i += 1;
                    let mut image = Vec::new();
                    while i < lines.len() && lines[i].0 > lines[1].0 {
                        image.push(lines[i].1.clone());
                        i += 1;
                    }
                    let (v, img) = side(&a, &s[1..], &image, &mut ids, &schema);
                    let k = ["base", "ours", "theirs"]
                        .iter()
                        .position(|x| *x == s[0])
                        .expect("a side");
                    match k {
                        0 => cf.base = v,
                        1 => cf.ours = v,
                        _ => cf.theirs = v,
                    }
                    cf.images[k] = img;
                }
                let uid = |n: Nid| ids.uid_of(n);
                cstate(Some(&conflict_cval(&a, &cf, &uid)))
            }
            class => {
                let a = match class {
                    "existence" => Aspect::Existence,
                    "status" => Aspect::Status,
                    "hierarchy" => Aspect::Hierarchy,
                    "observation" => Aspect::Observation,
                    "body" => Aspect::Body,
                    "edge" => Aspect::Edge(EdgeKey {
                        kind: "cites".into(),
                        dst: Nid(1),
                        disc: None,
                    }),
                    o => panic!("unknown encoding {o}"),
                };
                let image: Vec<Vec<String>> = lines[1..].iter().map(|l| l.1.clone()).collect();
                let (v, img) = side(&a, t, &image, &mut ids, &schema);
                // A plain `live` value carries no image (`snap` = 0); one written with image lines is a conflict
                // side's (`snap` = 1).
                let img = img.filter(|_| !image.is_empty());
                let uid = |n: Nid| ids.uid_of(n);
                kval_bytes(&a, v.as_ref(), img.as_ref(), &uid)
            }
        };
        let want = hex_bytes(c.block("hex"));
        assert_eq!(
            crate::value::hex(&got),
            crate::value::hex(&want),
            "{}",
            c.id
        );
        assert_eq!(
            c.line("length"),
            Some(want.len().to_string().as_str()),
            "{}",
            c.id
        );
    }
    assert_eq!(n, 60);
}

/// E5 on the fixtures (INDEX §5.1 item 3): the typed three-way merge of `base-state`, `p-state` (o) and
/// `theirs-state` gives CS(`q-state`), and each key the three states disagree on is decided by the row, or a row of
/// the case, that `merge-rows` names.
#[test]
fn every_merge_case_merges() {
    use crate::merge::Op;
    let mut n = 0;
    for (file, id, op, dst_main) in [
        ("commits.cases", "merge", Op::Merge, true),
        ("commits.cases", "sync", Op::Sync, false),
        ("commits.cases", "revert", Op::Revert, true),
        ("commits.cases", "cherry-pick", Op::CherryPick, true),
        ("tombstones.cases", "undelete", Op::Revert, false),
        ("foreign.cases", "foreign-merge", Op::Merge, false),
    ] {
        let c = cases(file)
            .into_iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("no case {id}"));
        check_merge_case(&c, op, dst_main);
        n += 1;
    }
    assert_eq!(n, 6);
}

/// E5 on one case: the typed merge of its `base-state`, `p-state` (o) and `theirs-state` gives CS(`q-state`) with no
/// violation, and each key of its `merge-rows` block was decided by the row, or a row of the case, it names.
fn check_merge_case(c: &Case, op: crate::merge::Op, dst_main: bool) {
    use crate::merge::{Ctx, Fresh, merge};
    let id = c.id.as_str();
    let mut ids = Ids::default();
    let b = state(c.block("base-state"), &mut ids);
    let o = state(c.block("p-state"), &mut ids);
    let t = state(c.block("theirs-state"), &mut ids);
    let q = state(c.block("q-state"), &mut ids);
    let empty = BTreeMap::new();
    let nids: BTreeMap<Uid, Nid> = ids.uid.iter().map(|(n, u)| (*u, *n)).collect();
    let uid = |n: Nid| ids.uid_of(n);
    let nid = |u: Uid| nids.get(&u).copied();
    // RS-007: a merge or a `sync` with no commit steps replays each side's value in its (0, 0) step; a revert or a
    // cherry-pick replays from o, with src's one step, C's: the hierarchy keys where src differs from the base, valued
    // on src. The case holds no dst commit after C, and with one step its order key decides nothing.
    let src: Vec<crate::merge::Step> = match op {
        crate::merge::Op::Revert | crate::merge::Op::CherryPick => {
            crate::merge::Step::of((0, [0; 32]), &crate::state::diff(&b, &t))
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    };
    let cx = Ctx {
        op,
        dst_main,
        dst_plan: false,
        policy: None,
        auto: &empty,
        start: crate::merge::Start::Base,
        moves: [&[], &src],
        uid: &uid,
        nid: &nid,
    };
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    let got = canonical_state(&m.st, &uid);
    let want = canonical_state(&q, &uid);
    let diff = entries(&want, &got);
    assert!(
        diff.is_empty(),
        "{id}: the merge differs from q-state in {} keys: {:?}\nconflicts {:?}\nviolations {:?}",
        diff.len(),
        got.iter()
            .filter(|(k, v)| want.get(k) != Some(v))
            .collect::<Vec<_>>(),
        m.conflicts,
        m.violations
    );
    assert!(m.violations.is_empty(), "{id}: {:?}", m.violations);
    // merge-rows: `<uid> <key…> : <row or case> …`.
    for line in c
        .block("merge-rows")
        .iter()
        .filter(|l| !l.trim().is_empty())
    {
        let (key, rest) = line.split_once(" : ").expect("key : row");
        let want_row = rest.split_whitespace().next().expect("a row");
        let (_, kt) = tokens(key);
        let u = uid_hex(&kt[0]);
        let nd = nids[&u];
        let a = match kt[1].as_str() {
            "existence" => Aspect::Existence,
            "status" => Aspect::Status,
            "hierarchy" => Aspect::Hierarchy,
            "observation" => Aspect::Observation,
            "body" => Aspect::Body,
            "field" => Aspect::Field(kt[2].clone()),
            "counter" => Aspect::Counter(kt[2].clone()),
            "edge" => Aspect::Edge(EdgeKey {
                kind: kt[2].clone(),
                dst: nids[&uid_hex(&kt[3])],
                disc: (kt[4] != "-").then(|| uid_hex(&kt[4])),
            }),
            o => panic!("unknown merge-rows key {o}"),
        };
        let k = crate::state::Key::Node(nd, a);
        let got_row = m
            .rows
            .get(&k)
            .unwrap_or_else(|| panic!("{id}: no decision recorded for {key}"));
        let got_id = got_row.split('+').next().unwrap_or("");
        let case = crate::rules::rules()
            .row(got_id)
            .map(|(_, r)| r.tok("case").to_string())
            .unwrap_or_default();
        assert!(
            got_id == want_row
                || case == want_row
                || ((got_id == "MR-040" || case == "any")
                    && ["ours-only", "theirs-only", "both"].contains(&want_row)),
            "{id}: {key} was decided by {got_row} (case {case}), the fixture says {want_row}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// fixtures/carrier ([PLAN §3.2] WP-21 E3: WP-91 reproduces every `changeset-digest` and `commit-id`)
// ---------------------------------------------------------------------------------------------------------------

/// The directory of the carrier fixtures.
fn carrier_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/carrier")
}

/// One carrier case (`fixtures/carrier/INDEX.md` §2–§3): its `case.txt` and the bytes of its git commit object.
struct Carrier {
    case: Case,
    commit: Vec<u8>,
}

/// Every carrier case by name.
fn carrier_cases() -> BTreeMap<String, Carrier> {
    let mut out = BTreeMap::new();
    let dir = carrier_dir();
    for e in std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
    {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let text = std::fs::read_to_string(p.join("case.txt"))
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let mut cs = parse_cases(&text);
        assert_eq!(cs.len(), 1, "{}: one case", p.display());
        let case = cs.remove(0);
        let name = p
            .file_name()
            .expect("a directory name")
            .to_string_lossy()
            .into_owned();
        assert_eq!(case.id, name, "the case is named for its directory");
        let commit =
            std::fs::read(p.join("commit")).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        out.insert(name, Carrier { case, commit });
    }
    out
}

/// The fields of a git commit object an importer reads for [F07 §12.3] and §12.4 ([git-objects]): its parents' object
/// ids, the bytes between `<` and `>` of the author line, the committer time and the message bytes.
struct GitObject {
    parents: Vec<String>,
    author_email: Vec<u8>,
    committer_secs: i64,
    message: Vec<u8>,
}

fn git_object(bytes: &[u8]) -> GitObject {
    let split = bytes
        .windows(2)
        .position(|w| w == b"\n\n")
        .expect("a commit object has a message");
    let mut g = GitObject {
        parents: Vec::new(),
        author_email: Vec::new(),
        committer_secs: 0,
        message: bytes[split + 2..].to_vec(),
    };
    for line in bytes[..split].split(|b| *b == b'\n') {
        let text = String::from_utf8_lossy(line);
        if let Some(p) = text.strip_prefix("parent ") {
            g.parents.push(p.to_string());
        } else if line.starts_with(b"author ") {
            let lt = line
                .iter()
                .position(|b| *b == b'<')
                .expect("an author email");
            let gt = lt + line[lt..].iter().position(|b| *b == b'>').expect("its end");
            g.author_email = line[lt + 1..gt].to_vec();
        } else if text.starts_with("committer ") {
            let words: Vec<&str> = text.split(' ').collect();
            g.committer_secs = words[words.len() - 2].parse().expect("a committer time");
        }
    }
    g
}

/// The message part of a git message before its trailer block ([F14 §10.3]): the lines, a final empty piece dropped,
/// before the empty line that precedes the final paragraph, joined by LF; empty when the message has no empty line.
fn message_part(m: &[u8]) -> Vec<u8> {
    let mut lines: Vec<&[u8]> = m.split(|b| *b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    match lines.iter().rposition(|l| l.is_empty()) {
        Some(i) => lines[..i].join(&b'\n'),
        None => Vec::new(),
    }
}

/// The `Moirai-Parent` trailers of a git message: 1-based parent index → the stated id ([F14 §10.4]).
fn stated_parents(m: &[u8]) -> BTreeMap<usize, [u8; 32]> {
    String::from_utf8_lossy(m)
        .lines()
        .filter_map(|l| l.strip_prefix("Moirai-Parent: "))
        .map(|v| {
            let (i, id) = v.split_once(' ').expect("an index and an id");
            let id = id.strip_prefix('c').expect("a commit name");
            (i.parse().expect("an index"), id32(id))
        })
        .collect()
}

/// The `Moirai-Ops` trailer of a git message ([F14 §10.4]), if it has one.
fn moirai_ops(m: &[u8]) -> Option<String> {
    String::from_utf8_lossy(m)
        .lines()
        .find_map(|l| l.strip_prefix("Moirai-Ops: ").map(str::to_string))
}

/// The `schema-version:` of the `.moirai-image` marker in a case's tree ([F14 §4]): the case's own new marker file,
/// or, when its tree kept the marker unchanged, its first git parent's.
fn marker_schema_version(name: &str, all: &BTreeMap<String, Carrier>) -> u32 {
    let p = carrier_dir().join(name).join("new/.moirai-image");
    if let Ok(text) = std::fs::read_to_string(&p) {
        return text
            .lines()
            .find_map(|l| l.strip_prefix("schema-version: "))
            .expect("a schema version")
            .parse()
            .expect("a number");
    }
    let parent = all[name].case.lines["git-parent"][0]
        .split_whitespace()
        .nth(1)
        .expect("the parent's case")
        .to_string();
    marker_schema_version(&parent, all)
}

/// Checks one carrier case beyond [`check_commit_case`]'s entries, digest, C and id (`fixtures/carrier/INDEX.md` §4
/// items 4–6): its stated parents, the items 1–9 a foreign, checkpoint or demoted commit derives from its git commit,
/// the typed merge of `foreign-merge`, a demoted commit's failed native reconstruction, and `same-id`,
/// `same-changeset` and `canonical`.
fn check_carrier_case(name: &str, all: &BTreeMap<String, Carrier>) {
    let Carrier { case: c, commit } = &all[name];
    check_commit_case(c);
    let class = c.line("class").expect("a class");
    let h = header(c.block("commit"));
    let digest = id32(c.line("changeset-digest").expect("a digest"));
    let id = c.line("commit-id").expect("an id");
    let g = git_object(commit);
    // The parents' cases, in git order.
    let parent_cases: Vec<&str> = c
        .lines
        .get("git-parent")
        .map(|v| {
            v.iter()
                .map(|l| {
                    let (oid, case) = l.split_once(' ').expect("an object id and a case");
                    assert!(g.parents.iter().any(|p| p == oid), "{name}: parent {oid}");
                    case
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        parent_cases.len(),
        g.parents.len(),
        "{name}: the git parents"
    );
    let own = |case: &str| id32(all[case].case.line("commit-id").expect("an id"));
    // Item 2: a native commit states its parents (its `Moirai-Parent` trailers, else each parent's `Moirai-Commit`);
    // a foreign or checkpoint commit, and a demoted one by its foreign derivation, names the ids this store holds.
    let stated = stated_parents(&g.message);
    let want: Vec<[u8; 32]> = parent_cases
        .iter()
        .enumerate()
        .map(|(i, p)| match (class, stated.get(&(i + 1))) {
            ("native", Some(s)) => *s,
            ("native", None) => all[*p]
                .case
                .line("trailer-commit")
                .map_or_else(|| own(p), id32),
            _ => own(p),
        })
        .collect();
    assert_eq!(h.parents, want, "{name}: item 2");
    match class {
        "native" => assert_eq!(
            Some(id),
            c.line("trailer-commit"),
            "{name}: a native commit verifies"
        ),
        "foreign" | "checkpoint" | "demoted" => {
            let dest: Vec<&str> = c
                .line("destination")
                .expect("a destination")
                .split(' ')
                .collect();
            let gc = GitCommit {
                parents: parent_cases
                    .iter()
                    .map(|p| (own(p), header(all[*p].case.block("commit")).hlc))
                    .collect(),
                committer_secs: g.committer_secs,
                author_email: g.author_email.clone(),
                message: if class == "checkpoint" {
                    message_part(&g.message)
                } else {
                    g.message.clone()
                },
                schema_version: marker_schema_version(name, all),
                oid: oid_of(
                    dest[1],
                    c.line("git-commit").expect("the commit's object id"),
                ),
            };
            let derived = if class == "checkpoint" {
                checkpoint_header(&gc)
            } else {
                foreign_header(&gc)
            }
            .unwrap_or_else(|| panic!("{name}: the git commit derives no header"));
            assert_eq!(derived, h, "{name}: items 1-9 from the git commit");
        }
        o => panic!("{name}: unknown class {o}"),
    }
    if class == "demoted" {
        // The native reconstruction over the same item 10 gives an id other than the trailer's, so the commit is
        // demoted ([F07 §12.5]); its id is the foreign one. A candidate whose `Moirai-Ops` is not its entry count is
        // demoted without hashing ([F14 §10.9], §12.1), so its rebuilt id may equal the trailer's.
        let native = crate::value::hex(&commit_id(&header(c.block("native-commit")), &digest));
        assert_eq!(
            Some(native.as_str()),
            c.line("native-commit-id"),
            "{name}: native-commit-id"
        );
        let ops = moirai_ops(&g.message);
        let count = c.line("entry-count").expect("an entry count");
        if ops.as_deref() == Some(count) {
            assert_ne!(
                Some(native.as_str()),
                c.line("trailer-commit"),
                "{name}: demoted"
            );
        }
    }
    if name == "foreign-merge" {
        check_merge_case(c, crate::merge::Op::Merge, false);
    }
    for other in c.lines.get("same-id").into_iter().flatten() {
        let o = other.split_whitespace().next().expect("a case");
        assert_eq!(
            all[o].case.line("commit-id"),
            Some(id),
            "{name}: same-id {o}"
        );
    }
    for other in c.lines.get("same-changeset").into_iter().flatten() {
        let o = other.split_whitespace().next().expect("a case");
        assert_eq!(
            all[o].case.line("changeset-digest"),
            c.line("changeset-digest"),
            "{name}: same-changeset {o}"
        );
    }
    if let Some(l) = c.line("canonical") {
        let w: Vec<&str> = l.split_whitespace().collect();
        let twin = cases(w[0])
            .into_iter()
            .find(|x| x.id == w[1])
            .unwrap_or_else(|| panic!("{name}: no canonical case {l}"));
        assert_eq!(twin.line(w[2]), c.line(w[2]), "{name}: canonical {l}");
    }
}

/// E3 on `fixtures/carrier/` (its INDEX's acceptance): every one of the 45 cases reproduces its item 10,
/// `changeset_digest`, C and `commit_id`; a native case verifies against its `Moirai-Commit`; a foreign, checkpoint or
/// demoted case's items 1–9 follow from its git commit and its parents' cases; `foreign-merge`'s item 10 is the typed
/// merge's; a demoted case's native reconstruction fails, or its `Moirai-Ops` is not its entry count ([F14 §12.1]);
/// twins share their ids and the cases `canonical` names agree.
#[test]
fn every_carrier_case_reproduces_its_id() {
    let all = carrier_cases();
    assert_eq!(all.len(), 45, "INDEX §5 lists 45 cases");
    for name in all.keys() {
        check_carrier_case(name, &all);
    }
}

/// base64url decodes the anchor texts of the image ([F14 §2.6]), and a text written that way hashes its bytes.
#[test]
fn base64url_texts_decode() {
    assert_eq!(b64url("Q2Fm6SBsb2NraW5n"), b"Caf\xe9 locking");
    assert_eq!(b64url(""), b"");
    assert_eq!(b64url("_-8"), [0xff, 0xef]);
    assert_eq!(atext_h("%Q2Fm6SBsb2NraW5n"), b3_128(b"Caf\xe9 locking"));
    assert_eq!(atext_h("\"x\""), b3_128(b"x"));
}
