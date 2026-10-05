//! The model's own canonical-form encoder ([F07]; [60 §4.2] "Canonical op list and commit ids"; [PLAN §3.2] WP-91),
//! written from the specification text alone: the canonical state of a view (§6), the canonical value encodings with
//! names, uids and full commit ids in place of every store-local number (§7), the edge value and the R-10 anchor
//! selector block (§8), schema items (§9), item 10 as a sorted list of entries and `changeset_digest` (§10), message
//! normalisation (§5) and `commit_id` for every commit kind (§3, §12).
//!
//! The encoder works on its own canonical state [`Cs`]: keys that name uids ([`CKey`]) and values held as their
//! canonical bytes ([`CVal`]), so equality of two values is byte equality of their encodings ([F07 §7.3], the one rule
//! of value equality). [`canonical_state`] projects a model [`State`] onto it; hand-built canonical states (the
//! fixtures of `fixtures/canonical/`) use the same encoder.

use crate::err::{Refusal, Res};
use crate::schema::{
    Acyclic, Card, EdgeClass, EdgeItem, Ends, FieldItem, Item, ItemKey, KindItem, Props, QueryItem,
    Schema, Storage, UidDerivation, type_names,
};
use crate::state::{
    Aspect, Conflict, EdgeProps, Image, KState, KVal, Node, OBSERVATION, Side, State,
};
use crate::value::{Algo, F64, MoveClass, Nid, Oid, PathMove, PathVal, Uid, Value, lp};
use std::collections::{BTreeMap, BTreeSet};

/// BLAKE3-256 of the input ([F01 §7.1]).
pub fn b3_256(x: &[u8]) -> [u8; 32] {
    *blake3::hash(x).as_bytes()
}

/// BLAKE3-128: the first 16 bytes of BLAKE3-256 of the input, unframed ([F01 §7.1]); body hashes and anchor digests.
pub fn b3_128(x: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&blake3::hash(x).as_bytes()[..16]);
    out
}

/// The name of a `pathmove` class ([F07 §2.2]).
pub fn move_class_name(c: MoveClass) -> &'static str {
    match c {
        MoveClass::Explicit => "explicit",
        MoveClass::Confirmed => "confirmed",
        MoveClass::Committed => "committed",
        MoveClass::Observed => "observed",
    }
}

// ---------------------------------------------------------------------------------------------------------------
// §7.1 typed values
// ---------------------------------------------------------------------------------------------------------------

/// The canonical tag of a value ([F07 §7.1]).
fn tag(v: &Value) -> u8 {
    match v {
        Value::Bool(false) => 1,
        Value::Bool(true) => 2,
        Value::Int(_) => 3,
        Value::Counter(_) => 4,
        Value::F64(_) => 5,
        Value::Enum(_) => 6,
        Value::Text(_) => 7,
        Value::Set(_) => 9,
        Value::Ref(_) => 10,
        Value::Commit(_) => 11,
        Value::Path(_) => 12,
        Value::Oid(_) => 13,
        Value::PathMove(_) => 14,
    }
}

fn path_payload(out: &mut Vec<u8>, p: &PathVal) {
    lp(out, p.root.as_bytes());
    lp(out, p.text.as_bytes());
}

fn oid_payload(out: &mut Vec<u8>, o: Option<&Oid>) {
    match o {
        Some(o) => {
            lp(out, o.algo.name().as_bytes());
            lp(out, &o.digest);
        }
        None => {
            lp(out, b"");
            lp(out, b"");
        }
    }
}

/// A value's payload without its tag ([F07 §7.1]); `uid` maps a `#N` to its uid ([F07 §2.3]).
fn payload(out: &mut Vec<u8>, v: &Value, uid: &dyn Fn(Nid) -> Uid) {
    match v {
        Value::Bool(_) => {}
        Value::Int(i) | Value::Counter(i) => out.extend_from_slice(&i.to_le_bytes()),
        Value::F64(f) => {
            // −0.0 is held as +0.0 by construction ([`F64::new`]); NaN and the infinities never occur.
            let x = if f.get() == 0.0 { 0.0f64 } else { f.get() };
            out.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        Value::Enum(s) | Value::Text(s) => lp(out, s.as_bytes()),
        Value::Set(items) => {
            let elem = items.first().map_or(7, tag);
            let mut enc: Vec<Vec<u8>> = items
                .iter()
                .map(|x| {
                    let mut b = Vec::new();
                    payload(&mut b, x, uid);
                    b
                })
                .collect();
            enc.sort();
            enc.dedup();
            out.push(elem);
            out.extend_from_slice(&(enc.len() as u32).to_le_bytes());
            for e in enc {
                out.extend_from_slice(&e);
            }
        }
        Value::Ref(n) => out.extend_from_slice(&uid(*n).0),
        Value::Commit(c) => out.extend_from_slice(c),
        Value::Path(p) => path_payload(out, p),
        Value::Oid(o) => oid_payload(out, Some(o)),
        Value::PathMove(m) => pathmove_payload(out, m),
    }
}

fn pathmove_payload(out: &mut Vec<u8>, m: &PathMove) {
    out.extend_from_slice(&m.hlc.to_le_bytes());
    lp(out, move_class_name(m.class).as_bytes());
    path_payload(out, &m.from);
    path_payload(out, &m.to);
    oid_payload(out, m.git.as_ref());
}

/// `cv` of a value ([F07 §7.1]): the tag byte and the payload. The empty text, the empty set and an `oid` without a
/// digest are `absent` (tag 0).
// spec: [F07 §7.1]
pub fn cv(v: Option<&Value>, uid: &dyn Fn(Nid) -> Uid) -> Vec<u8> {
    let mut out = Vec::new();
    match v.filter(|v| !crate::state::is_empty(v)) {
        None => out.push(0),
        Some(v) => {
            out.push(tag(v));
            payload(&mut out, v, uid);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// §8 edge values and the selector block
// ---------------------------------------------------------------------------------------------------------------

/// The hashed fields of an anchor record, the R-10 selector block's content ([F07 §8.2]; [F08 §10.3]; [40 §2.7]).
/// Texts never enter: `quote`, `prefix`, `suffix` and `end` are held as their BLAKE3-128 digests only ([`b3_128`]
/// over the exact bytes of the normalised text), so a store that holds the texts and one that imported them
/// `hash-only` hold the same block ([F07 §8.3]).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Anchor {
    /// `file`, `heading`, `symbol`, `quote`, `range` or `lines`.
    pub kind: String,
    /// `live` or `pinned`.
    pub mode: String,
    /// `header` or `span`.
    pub watch: String,
    /// The scope value's bytes of [F08 §10.3.1]; empty when the anchor has no scope.
    pub scope: Vec<u8>,
    /// BLAKE3-128 of `quote.exact`.
    pub quote_h: Option<[u8; 16]>,
    /// BLAKE3-128 of `prefix.exact`; present exactly when `quote_h` is.
    pub prefix_h: Option<[u8; 16]>,
    /// BLAKE3-128 of `suffix.exact`; present exactly when `quote_h` is.
    pub suffix_h: Option<[u8; 16]>,
    /// BLAKE3-128 of `end.exact`, a `range` anchor's only.
    pub end_h: Option<[u8; 16]>,
    /// The 1-based occurrence index.
    pub occurrence: Option<u16>,
    /// The hint's first and last line.
    pub hint: Option<(u32, u32)>,
    /// The window value W of [F20 §2.7.3]; empty when none.
    pub window: Vec<u8>,
    /// XXH3-64 of the span or header.
    pub span_hash: Option<u64>,
    /// The file's `oid` at capture.
    pub blob: Option<Oid>,
    /// The observed git commit at capture.
    pub git: Option<Oid>,
    /// The capture digest.
    pub captured: [u8; 16],
    /// The predecessor term of the anchor uid.
    pub pred: Option<[u8; 16]>,
    /// The in-file marker id; empty when none.
    pub marker: String,
    /// The resolver version at capture (1 in format v1).
    pub resolver: u16,
    /// The texts whose digests the block carries, for the resolver and the results; `None` for an anchor held without
    /// its text (`text-unavailable`, [F18 §4.6]). They never enter the selector block ([F07 §8.3]).
    pub text: Option<Box<AnchorText>>,
}

/// The texts of an anchor record ([F08 §10.3]): `quote.exact`, `prefix.exact`, `suffix.exact` and `end.exact`, each empty
/// where the kind has none.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AnchorText {
    /// `quote.exact` (for `range`, the start quote).
    pub quote: Vec<u8>,
    /// `prefix.exact`.
    pub prefix: Vec<u8>,
    /// `suffix.exact`.
    pub suffix: Vec<u8>,
    /// `end.exact`.
    pub end: Vec<u8>,
}

/// The selector block of an anchor, its 20 fields in order ([F07 §8.2]); an absent field is `lp("")`.
// spec: [F07 §8.2]
pub fn selector_block(a: &Anchor) -> Vec<u8> {
    let mut out = Vec::new();
    lp(&mut out, a.kind.as_bytes());
    lp(&mut out, a.mode.as_bytes());
    lp(&mut out, a.watch.as_bytes());
    lp(&mut out, &a.scope);
    for h in [a.quote_h, a.prefix_h, a.suffix_h, a.end_h] {
        lp(&mut out, h.as_ref().map_or(&[][..], |h| &h[..]));
    }
    lp(
        &mut out,
        &a.occurrence
            .map_or(Vec::new(), |o| o.to_le_bytes().to_vec()),
    );
    lp(
        &mut out,
        &a.hint.map_or(Vec::new(), |(f, l)| {
            let mut b = f.to_le_bytes().to_vec();
            b.extend_from_slice(&l.to_le_bytes());
            b
        }),
    );
    lp(&mut out, &a.window);
    lp(
        &mut out,
        &a.span_hash.map_or(Vec::new(), |h| h.to_le_bytes().to_vec()),
    );
    oid_payload(&mut out, a.blob.as_ref());
    oid_payload(&mut out, a.git.as_ref());
    out.extend_from_slice(&a.captured);
    lp(&mut out, a.pred.as_ref().map_or(&[][..], |p| &p[..]));
    lp(&mut out, a.marker.as_bytes());
    out.extend_from_slice(&a.resolver.to_le_bytes());
    out
}

/// The edge value `present(props)` ([F07 §8.1]): `ef` 1, `pflags` (bit 0 `has_pin`, bit 1 `flagged`, bit 2 `anchor`),
/// the pinned commit's full id and the selector block when present.
// spec: [F07 §8.1]
pub fn edge_value(p: &EdgeProps) -> Vec<u8> {
    let mut out = vec![1u8];
    let flags = u8::from(p.pinned.is_some())
        | (u8::from(p.flagged) << 1)
        | (u8::from(p.anchor.is_some()) << 2);
    out.push(flags);
    if let Some(c) = &p.pinned {
        out.extend_from_slice(c);
    }
    if let Some(a) = &p.anchor {
        out.extend_from_slice(&selector_block(a));
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// §9 schema items
// ---------------------------------------------------------------------------------------------------------------

fn bool8(out: &mut Vec<u8>, b: bool) {
    out.push(u8::from(b));
}

/// Names written one by one in the bytewise order of their encodings `lp(name)` ([F07 §2.4]).
fn sorted_names(out: &mut Vec<u8>, names: &[String]) {
    let mut enc: Vec<Vec<u8>> = names
        .iter()
        .map(|n| {
            let mut b = Vec::new();
            lp(&mut b, n.as_bytes());
            b
        })
        .collect();
    enc.sort();
    enc.dedup();
    out.extend_from_slice(&(enc.len() as u32).to_le_bytes());
    for e in enc {
        out.extend_from_slice(&e);
    }
}

fn kind_set(out: &mut Vec<u8>, e: &Ends) {
    match e {
        Ends::Any => {
            bool8(out, true);
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        Ends::Kinds(k) => {
            bool8(out, false);
            sorted_names(out, k);
        }
    }
}

fn storage_name(s: Storage) -> &'static str {
    match s {
        Storage::Header => "header",
        Storage::Flag => "flag",
        Storage::Cold => "cold",
        Storage::Field => "field",
        Storage::Title => "title",
        Storage::Body => "body",
    }
}

fn kind_item(out: &mut Vec<u8>, k: &KindItem) {
    bool8(out, k.retired);
    let uidd = match k.uid {
        UidDerivation::Random => "random",
        UidDerivation::FileKey => "file-key",
    };
    lp(out, uidd.as_bytes());
    lp(
        out,
        if k.root_variant {
            b"root-key".as_slice()
        } else {
            b"none".as_slice()
        },
    );
    lp(out, k.existence_policy.as_bytes());
    bool8(out, k.title_derived);
    bool8(out, k.immutable_fields);
    bool8(out, k.has_done);
    bool8(out, k.done_derived);
}

fn field_item(out: &mut Vec<u8>, f: &FieldItem, uid: &dyn Fn(Nid) -> Uid) {
    bool8(out, f.retired);
    let (ty, elem) = type_names(f.ty);
    lp(out, ty.as_bytes());
    lp(out, elem.unwrap_or("").as_bytes());
    lp(out, f.class.as_bytes());
    lp(out, storage_name(f.storage).as_bytes());
    out.extend_from_slice(&f.decl.to_le_bytes());
    bool8(out, f.optional);
    lp(out, f.index.as_bytes());
    lp(out, f.coerce.as_bytes());
    bool8(out, f.one_line);
    bool8(out, f.ascii);
    bool8(out, f.default.is_some());
    if let Some(d) = &f.default {
        out.extend_from_slice(&cv(Some(d), uid));
    }
    bool8(out, f.range.is_some());
    if let Some((lo, hi)) = f.range {
        out.extend_from_slice(&lo.to_le_bytes());
        out.extend_from_slice(&hi.to_le_bytes());
    }
}

fn enum_item(out: &mut Vec<u8>, e: &crate::schema::EnumItem) {
    bool8(out, e.retired);
    out.extend_from_slice(&e.rank.to_le_bytes());
    bool8(out, e.side);
    bool8(out, e.done);
    sorted_names(out, &e.covers);
}

fn acyclic_name(a: Acyclic) -> &'static str {
    match a {
        Acyclic::None => "none",
        Acyclic::Forest => "forest",
        Acyclic::Precedence => "precedence",
        Acyclic::Dag => "dag",
        Acyclic::ByConstruction => "by-construction",
    }
}

fn card_name(c: Card) -> &'static str {
    match c {
        Card::Many => "many",
        Card::Max1PerSrc => "max-1-per-src",
        Card::Max1ActivePerDst => "max-1-active-per-dst",
        Card::Chain1 => "chain-1",
        Card::Typical1 => "typical-1",
        Card::AnchorsMin1 => "anchors-min-1",
    }
}

fn props_name(p: Props) -> &'static str {
    match p {
        Props::None => "none",
        Props::Pinned => "pinned",
        Props::Flagged => "flagged",
        Props::Anchor => "anchor",
    }
}

fn edge_item(out: &mut Vec<u8>, e: &EdgeItem) {
    bool8(out, e.retired);
    lp(
        out,
        match e.class {
            EdgeClass::Structural => b"structural".as_slice(),
            EdgeClass::Historical => b"historical".as_slice(),
        },
    );
    lp(out, e.on_dst.as_bytes());
    lp(out, e.on_src.as_bytes());
    lp(out, acyclic_name(e.acyclic).as_bytes());
    lp(out, card_name(e.card).as_bytes());
    out.push(e.max_depth);
    lp(
        out,
        if e.name == "at" {
            b"anchor-key".as_slice()
        } else {
            b"none".as_slice()
        },
    );
    lp(out, props_name(e.props).as_bytes());
    bool8(out, e.symmetric);
    bool8(out, e.same_kind);
    lp(out, e.lq_name.as_bytes());
    kind_set(out, &e.src);
    kind_set(out, &e.dst);
    sorted_names(out, &e.reverse);
    lp(out, e.reading.as_bytes());
}

fn query_item(out: &mut Vec<u8>, q: &QueryItem) {
    out.extend_from_slice(&q.lq_version.to_le_bytes());
    lp(out, q.params.as_bytes());
    lp(out, q.shape.as_bytes());
    lp(out, q.budget.as_bytes());
    lp(out, q.text.as_bytes());
}

/// A schema item's value: `sf` 1 and the item of §9.2–§9.7 ([F07 §9.1]).
// spec: [F07 §9]
pub fn item_value(it: &Item, uid: &dyn Fn(Nid) -> Uid) -> Vec<u8> {
    let mut out = vec![1u8];
    match it {
        Item::Kind(k) => kind_item(&mut out, k),
        Item::Field(f) => field_item(&mut out, f, uid),
        Item::Enum(e) => enum_item(&mut out, e),
        Item::Edge(e) => edge_item(&mut out, e),
        Item::Query(q) => query_item(&mut out, q),
        // [F07 §9.7]: the value in its canonical form; the name is the key.
        Item::Policy(p) => lp(&mut out, p.value.as_deref().unwrap_or("").as_bytes()),
    }
    out
}

/// The class code and key components of a schema key ([F07 §9.1]).
pub fn schema_ckey(k: &ItemKey) -> CKey {
    let (class, key) = match k {
        ItemKey::Kind(n) => (1, vec![n.clone()]),
        ItemKey::Field(k, f) => (2, vec![k.clone(), f.clone()]),
        ItemKey::Enum(k, f, v) => (3, vec![k.clone(), f.clone(), v.clone()]),
        ItemKey::Edge(e) => (4, vec![e.clone()]),
        ItemKey::Query(q) => (5, vec![q.clone()]),
        ItemKey::Policy(p) => (6, vec![p.clone()]),
    };
    CKey::Schema { class, key }
}

// ---------------------------------------------------------------------------------------------------------------
// §6 the canonical state
// ---------------------------------------------------------------------------------------------------------------

/// A key of the canonical state ([F07 §6.1]), ordered as item 10's entries are (§10.3): node keys by (uid, class, name,
/// destination uid, discriminator — none first), then schema keys by (item class, components), bytewise.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum CKey {
    /// A node key: class 1–8; `name` is the field name of classes 4 and 6 and the edge-kind name of class 7; `dst` and
    /// `disc` are set on class 7 only.
    Node {
        /// The owner uid (an edge's source).
        uid: Uid,
        /// The class code.
        class: u8,
        /// The field or edge-kind name; empty otherwise.
        name: String,
        /// The destination uid of an edge.
        dst: Option<Uid>,
        /// The anchor uid of an `at` edge.
        disc: Option<Uid>,
    },
    /// A schema key.
    Schema {
        /// The item class, 1–5.
        class: u8,
        /// The key components.
        key: Vec<String>,
    },
}

/// A non-absent value of a canonical key.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CVal {
    /// A plain value: the class's value encoding (§7.2, §8.1, §9), never the absent byte.
    Plain(Vec<u8>),
    /// A conflict value: its class name and three plain sides as class value encodings (absent = `00`), and the
    /// provisional side on an existence key ([F07 §7.3]).
    Conflict {
        /// The conflict class name ([F12 §6.1]).
        class: String,
        /// Base side.
        base: Vec<u8>,
        /// Ours.
        ours: Vec<u8>,
        /// Theirs.
        theirs: Vec<u8>,
        /// `prov`, on an existence key.
        prov: Option<Side>,
    },
    /// A counter's total, never 0 (class 6).
    Counter(i64),
}

/// The canonical state CS(V) of a view: every key with a non-absent value ([F07 §6.1]).
pub type Cs = BTreeMap<CKey, CVal>;

/// `cstate` of a value, or of `absent` ([F07 §7.3]); counters have none (their entries carry a delta).
// spec: [F07 §7.3]
pub fn cstate(v: Option<&CVal>) -> Vec<u8> {
    match v {
        None => vec![0, 0],
        Some(CVal::Plain(b)) => {
            let mut out = vec![0];
            out.extend_from_slice(b);
            out
        }
        Some(CVal::Conflict {
            class,
            base,
            ours,
            theirs,
            prov,
        }) => {
            let mut out = vec![1];
            lp(&mut out, class.as_bytes());
            out.extend_from_slice(base);
            out.extend_from_slice(ours);
            out.extend_from_slice(theirs);
            if let Some(p) = prov {
                lp(
                    &mut out,
                    match p {
                        Side::Ours => b"ours".as_slice(),
                        Side::Theirs => b"theirs".as_slice(),
                    },
                );
            }
            out
        }
        Some(CVal::Counter(_)) => panic!("a counter key has no cstate: its entries carry a delta"),
    }
}

/// The existence value `live(kind)`, with the node image when it is a side of an existence conflict (§7.2).
pub fn ex_live(kind: &str, image: Option<&[u8]>) -> Vec<u8> {
    let mut out = vec![1u8];
    lp(&mut out, kind.as_bytes());
    match image {
        Some(i) => {
            out.push(1);
            out.extend_from_slice(i);
        }
        None => out.push(0),
    }
    out
}

/// The existence value `deleted(kind, reason, replaced_by)` (§7.2).
pub fn ex_deleted(kind: &str, reason: &str, replaced_by: Option<Uid>) -> Vec<u8> {
    let mut out = vec![2u8];
    lp(&mut out, kind.as_bytes());
    lp(&mut out, reason.as_bytes());
    lp(&mut out, replaced_by.as_ref().map_or(&[][..], |u| &u.0[..]));
    out
}

/// The status value (§7.2).
pub fn status_value(status: &str, resolution: &str) -> Vec<u8> {
    let mut out = vec![1u8];
    lp(&mut out, status.as_bytes());
    lp(&mut out, resolution.as_bytes());
    out
}

/// The hierarchy value (§7.2).
pub fn hierarchy_value(parent: Option<Uid>, order: Option<&str>) -> Vec<u8> {
    let mut out = vec![1u8];
    lp(&mut out, parent.as_ref().map_or(&[][..], |u| &u.0[..]));
    lp(&mut out, order.unwrap_or("").as_bytes());
    out
}

/// The body value: `bf` 1 and the BLAKE3-128 of the body bytes (§7.2).
pub fn body_value(body: &[u8]) -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend_from_slice(&b3_128(body));
    out
}

/// The observation value: `of` 1 and six `cv`s (§7.2).
pub fn observation_value(v: &[Option<Value>], uid: &dyn Fn(Nid) -> Uid) -> Vec<u8> {
    let mut out = vec![1u8];
    for x in v {
        out.extend_from_slice(&cv(x.as_ref(), uid));
    }
    out
}

/// One entry of a node image: its class code, name and bytes (§7.4).
pub type ImageEntry = (u8, String, Vec<u8>);

/// A node image from its entries: `n` and the entries sorted by (class code, name bytes) (§7.4).
// spec: [F07 §7.4]
pub fn image_bytes(mut entries: Vec<ImageEntry>) -> Vec<u8> {
    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut out = (entries.len() as u32).to_le_bytes().to_vec();
    for (class, name, bytes) in entries {
        out.push(class);
        match class {
            2 | 8 => {}
            _ => lp(&mut out, name.as_bytes()),
        }
        out.extend_from_slice(&bytes);
    }
    out
}

/// A model node image ([`Image`]) as image entries.
pub fn image_entries(img: &Image, uid: &dyn Fn(Nid) -> Uid) -> Vec<ImageEntry> {
    let mut out = Vec::new();
    for (a, v) in img {
        match (a, v) {
            (Aspect::Status, KVal::Status { status, resolution }) => {
                let mut b = Vec::new();
                lp(&mut b, status.as_bytes());
                lp(&mut b, resolution.as_bytes());
                out.push((2, String::new(), b));
            }
            (Aspect::Field(f), KVal::Value(v)) => out.push((4, f.clone(), cv(Some(v), uid))),
            (Aspect::Counter(f), KVal::Value(Value::Counter(t))) => {
                let mut b = vec![4u8];
                b.extend_from_slice(&t.to_le_bytes());
                out.push((6, f.clone(), b));
            }
            (Aspect::Body, KVal::Body(t)) => {
                out.push((8, String::new(), b3_128(t.as_bytes()).to_vec()))
            }
            (a, v) => panic!("a node image holds value keys only, not {a:?} = {v:?}"),
        }
    }
    out
}

/// The class code of a node aspect ([F07 §6.1]).
pub fn class_of(a: &Aspect) -> u8 {
    match a {
        Aspect::Existence => 1,
        Aspect::Status => 2,
        Aspect::Hierarchy => 3,
        Aspect::Field(_) => 4,
        Aspect::Observation => 5,
        Aspect::Counter(_) => 6,
        Aspect::Edge(_) => 7,
        Aspect::Body => 8,
    }
}

/// The canonical key of a node aspect: a symmetric edge is keyed at its bytewise smaller endpoint ([F07 §6.6]).
pub fn node_ckey(src: Uid, a: &Aspect, uid: &dyn Fn(Nid) -> Uid, symmetric: bool) -> CKey {
    let class = class_of(a);
    match a {
        Aspect::Field(f) | Aspect::Counter(f) => CKey::Node {
            uid: src,
            class,
            name: f.clone(),
            dst: None,
            disc: None,
        },
        Aspect::Edge(k) => {
            let d = uid(k.dst);
            let (s, d) = if symmetric && d < src {
                (d, src)
            } else {
                (src, d)
            };
            CKey::Node {
                uid: s,
                class,
                name: k.kind.clone(),
                dst: Some(d),
                disc: k.disc,
            }
        }
        _ => CKey::Node {
            uid: src,
            class,
            name: String::new(),
            dst: None,
            disc: None,
        },
    }
}

/// The class value encoding of a model key value in canonical form (absent = `00`); a `live` existence value carries
/// `image` when it is a side of an existence conflict.
pub fn kval_bytes(
    a: &Aspect,
    v: Option<&KVal>,
    image: Option<&Image>,
    uid: &dyn Fn(Nid) -> Uid,
) -> Vec<u8> {
    let Some(v) = v else { return vec![0] };
    match (a, v) {
        (Aspect::Existence, KVal::Live(kind)) => {
            let img = image.map(|i| image_bytes(image_entries(i, uid)));
            ex_live(kind, img.as_deref())
        }
        (
            Aspect::Existence,
            KVal::Deleted {
                kind,
                reason,
                replaced_by,
            },
        ) => ex_deleted(kind, reason.as_deref().unwrap_or(""), replaced_by.map(uid)),
        (Aspect::Status, KVal::Status { status, resolution }) => status_value(status, resolution),
        (Aspect::Hierarchy, KVal::Hierarchy { parent, order }) => {
            hierarchy_value(parent.map(uid), order.as_deref())
        }
        (Aspect::Field(_), KVal::Value(v)) => cv(Some(v), uid),
        (Aspect::Counter(_), KVal::Value(v)) => cv(Some(v), uid),
        (Aspect::Observation, KVal::Observation(vs)) => observation_value(vs, uid),
        (Aspect::Edge(_), KVal::Edge(p)) => edge_value(p),
        (Aspect::Body, KVal::Body(b)) => body_value(b.as_bytes()),
        (a, v) => panic!("key value {v:?} does not fit aspect {a:?}"),
    }
}

/// A model conflict value in canonical form.
pub fn conflict_cval(a: &Aspect, c: &Conflict, uid: &dyn Fn(Nid) -> Uid) -> CVal {
    CVal::Conflict {
        class: c.class.clone(),
        base: kval_bytes(a, c.base.as_ref(), c.images[0].as_ref(), uid),
        ours: kval_bytes(a, c.ours.as_ref(), c.images[1].as_ref(), uid),
        theirs: kval_bytes(a, c.theirs.as_ref(), c.images[2].as_ref(), uid),
        prov: if matches!(a, Aspect::Existence) {
            Some(c.prov.unwrap_or(Side::Ours))
        } else {
            None
        },
    }
}

/// The canonical value of one model key state: `None` for absent. A counter's plain value is its total.
pub fn kstate_cval(a: &Aspect, v: &KState, uid: &dyn Fn(Nid) -> Uid) -> Option<CVal> {
    match v {
        KState::Conflict(c) => Some(conflict_cval(a, c, uid)),
        KState::Plain(None) => None,
        KState::Plain(Some(KVal::Value(Value::Counter(t)))) if matches!(a, Aspect::Counter(_)) => {
            (*t != 0).then_some(CVal::Counter(*t))
        }
        KState::Plain(Some(k)) => Some(CVal::Plain(kval_bytes(a, Some(k), None, uid))),
    }
}

/// Whether a live node's field value is part of its canonical state ([F07 §6.3]): the field is hashed and the value is
/// not the field's default in the effective schema of the state described — the schema in force now, not the one the
/// value was written under.
fn field_present(schema: &Schema, node: &Node, f: &str, v: &Value) -> bool {
    hashed_field(schema, node, f) && schema.default_of(&node.kind, f).as_ref() != Some(v)
}

/// Whether a field of a node is a key of the canonical changeset ([F07 §6.2]): not of merge class `none` or `derived`,
/// and not the derived title of a kind with `title_derived` while the node is live.
fn hashed_field(schema: &Schema, node: &Node, f: &str) -> bool {
    if f == "title" && node.live() && schema.kind(&node.kind).is_some_and(|k| k.title_derived) {
        return false;
    }
    !schema
        .field(&node.kind, f)
        .is_some_and(|fi| fi.class == "none" || fi.class == "derived")
}

/// The node image of a live node: its status, fields, counters and body in canonical form ([F06 §6.3]; [F07 §7.4]):
/// a value equal to its field's default under `schema` is absent.
pub fn node_image(schema: &Schema, node: &Node) -> Image {
    let mut img = Image::new();
    if let Some(s) = node.get(schema, &Aspect::Status) {
        img.insert(Aspect::Status, s);
    }
    for (f, v) in &node.fields {
        if !field_present(schema, node, f, v) {
            continue;
        }
        let a = if matches!(v, Value::Counter(_)) {
            Aspect::Counter(f.clone())
        } else {
            Aspect::Field(f.clone())
        };
        img.insert(a, KVal::Value(v.clone()));
    }
    if let Some(b) = &node.body {
        img.insert(Aspect::Body, KVal::Body(b.clone()));
    }
    img
}

/// The canonical keys of one node, added to `out` ([F07 §6.3]–§6.6): a live node's existence, status, hierarchy,
/// fields (a value equal to the field's default in the state's schema is absent), counters, body and out-edges; a
/// tombstone's existence, `title` and retained out-edges; a key that holds a conflict value holds it and nothing else,
/// and while the observation key holds one its six member fields are absent.
// spec: [F07 §6.3]
// spec: [F07 §6.4]
// spec: [F07 §6.5]
pub fn node_keys(st: &State, node: &Node, uid: &dyn Fn(Nid) -> Uid, out: &mut Cs) {
    let schema = &st.schema;
    let u = node.uid;
    let symmetric = |kind: &str| schema.edge(kind).is_some_and(|e| e.symmetric);
    let mut put = |a: &Aspect, v: Option<CVal>| {
        let sym = matches!(a, Aspect::Edge(k) if symmetric(&k.kind));
        let k = node_ckey(u, a, uid, sym);
        match v {
            Some(v) => {
                out.insert(k, v);
            }
            None => {
                out.remove(&k);
            }
        }
    };
    // Existence.
    let ex = node.get(schema, &Aspect::Existence);
    put(
        &Aspect::Existence,
        Some(CVal::Plain(kval_bytes(
            &Aspect::Existence,
            ex.as_ref(),
            None,
            uid,
        ))),
    );
    let obs_conflict = node.conflicts.contains_key(&Aspect::Observation);
    if node.live() {
        for a in [Aspect::Status, Aspect::Hierarchy, Aspect::Body] {
            let v = node.get(schema, &a);
            put(
                &a,
                v.map(|v| CVal::Plain(kval_bytes(&a, Some(&v), None, uid))),
            );
        }
        for (f, v) in &node.fields {
            if !field_present(schema, node, f, v) {
                continue;
            }
            if let Value::Counter(t) = v {
                put(
                    &Aspect::Counter(f.clone()),
                    (*t != 0).then_some(CVal::Counter(*t)),
                );
                continue;
            }
            if obs_conflict && node.kind == "artifact" && OBSERVATION.contains(&f.as_str()) {
                continue;
            }
            put(
                &Aspect::Field(f.clone()),
                Some(CVal::Plain(cv(Some(v), uid))),
            );
        }
    } else if let Some(t) = node.fields.get("title") {
        put(
            &Aspect::Field("title".into()),
            Some(CVal::Plain(cv(Some(t), uid))),
        );
    }
    for (k, p) in &node.out {
        put(&Aspect::Edge(k.clone()), Some(CVal::Plain(edge_value(p))));
    }
    for (a, c) in &node.conflicts {
        put(a, Some(conflict_cval(a, c, uid)));
    }
}

/// The schema keys of a view: its project items and the schema keys that hold conflict values ([F07 §9]).
pub fn schema_keys(st: &State, uid: &dyn Fn(Nid) -> Uid, out: &mut Cs) {
    for (k, it) in &st.schema.items {
        out.insert(schema_ckey(k), CVal::Plain(item_value(it, uid)));
    }
    for (k, c) in &st.schema_conflicts {
        let side = |v: &Option<KVal>| match v {
            Some(KVal::Item(i)) => item_value(i, uid),
            None => vec![0],
            Some(other) => panic!("a schema side holds {other:?}"),
        };
        out.insert(
            schema_ckey(k),
            CVal::Conflict {
                class: c.class.clone(),
                base: side(&c.base),
                ours: side(&c.ours),
                theirs: side(&c.theirs),
                prov: None,
            },
        );
    }
}

/// CS(V) of a model state ([F07 §6]); `uid` maps every `#N` a value or an edge names to its uid.
// spec: [F07 §6.1]
pub fn canonical_state(st: &State, uid: &dyn Fn(Nid) -> Uid) -> Cs {
    let mut out = Cs::new();
    for node in st.nodes.values() {
        node_keys(st, node, uid, &mut out);
    }
    schema_keys(st, uid, &mut out);
    out
}

/// CS(V) restricted to the keys of the given nodes and the schema: the part of the canonical state a net changeset
/// over those nodes can change.
pub fn canonical_of(st: &State, nodes: &BTreeSet<Nid>, uid: &dyn Fn(Nid) -> Uid) -> Cs {
    let mut out = Cs::new();
    for n in nodes {
        if let Some(node) = st.nodes.get(n) {
            node_keys(st, node, uid, &mut out);
        }
    }
    schema_keys(st, uid, &mut out);
    out
}

// ---------------------------------------------------------------------------------------------------------------
// §10 item 10
// ---------------------------------------------------------------------------------------------------------------

/// The encoding of one entry (k, Q(k)) of item 10 ([F07 §10.2]); `p` is k's value in the first parent's state, used
/// for a counter's delta.
// spec: [F07 §10.2]
pub fn entry(k: &CKey, p: Option<&CVal>, q: Option<&CVal>) -> Vec<u8> {
    let mut out = Vec::new();
    match k {
        CKey::Node {
            uid,
            class,
            name,
            dst,
            disc,
        } => {
            out.push(1);
            out.extend_from_slice(&uid.0);
            out.push(*class);
            match class {
                4 | 6 => lp(&mut out, name.as_bytes()),
                7 => {
                    lp(&mut out, name.as_bytes());
                    out.extend_from_slice(&dst.expect("an edge key has a destination").0);
                    lp(&mut out, disc.as_ref().map_or(&[][..], |d| &d.0[..]));
                }
                _ => {}
            }
            if *class == 6 {
                let total = |v: Option<&CVal>| match v {
                    None => 0i128,
                    Some(CVal::Counter(t)) => i128::from(*t),
                    Some(other) => panic!("a counter key holds {other:?}"),
                };
                let d = total(q) - total(p);
                out.push(u8::from(d < 0));
                out.extend_from_slice(&(d.unsigned_abs() as u64).to_le_bytes());
            } else {
                out.extend_from_slice(&cstate(q));
            }
        }
        CKey::Schema { class, key } => {
            out.push(2);
            out.push(*class);
            for c in key {
                lp(&mut out, c.as_bytes());
            }
            out.extend_from_slice(&cstate(q));
        }
    }
    out
}

/// Whether two values of one key differ ([F07 §10.1]): counters as integers, every other value as its `cstate`.
fn differs(p: Option<&CVal>, q: Option<&CVal>) -> bool {
    match (p, q) {
        (Some(CVal::Counter(a)), Some(CVal::Counter(b))) => a != b,
        (Some(CVal::Counter(_)), None) | (None, Some(CVal::Counter(_))) => true,
        (a, b) => cstate(a) != cstate(b),
    }
}

/// The entries of item 10 between the first parent's canonical state `p` and the commit's `q`, in the order of
/// [F07 §10.3] (the order of [`CKey`]): one per key whose value differs.
// spec: [F07 §10.1]
// spec: [F07 §10.3]
pub fn entries(p: &Cs, q: &Cs) -> Vec<Vec<u8>> {
    let mut keys: BTreeSet<&CKey> = p.keys().collect();
    keys.extend(q.keys());
    keys.into_iter()
        .filter_map(|k| {
            let (a, b) = (p.get(k), q.get(k));
            differs(a, b).then(|| entry(k, a, b))
        })
        .collect()
}

/// `changeset_digest` over entries already in the order of §10.3 ([F07 §10.4]).
// spec: [F07 §10.4]
pub fn digest_of(entries: &[Vec<u8>]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    let mut dom = Vec::new();
    lp(&mut dom, b"moirai-changeset-v1");
    h.update(&dom);
    for e in entries {
        h.update(e);
    }
    h.update(&(entries.len() as u64).to_le_bytes());
    *h.finalize().as_bytes()
}

/// `changeset_digest` of the state diff from `p` to `q` ([F07 §10]).
pub fn changeset_digest(p: &Cs, q: &Cs) -> [u8; 32] {
    digest_of(&entries(p, q))
}

// ---------------------------------------------------------------------------------------------------------------
// §5 message normalisation
// ---------------------------------------------------------------------------------------------------------------

/// Steps 2–4 of N ([F07 §5.1]): CR LF and every remaining CR become LF; HT, VT, FF and SP are stripped from the end
/// of every line; every trailing LF is removed.
fn steps_2_to_4(m: &str) -> String {
    let lf = m.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = trim_lines(&lf);
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

fn trim_lines(s: &str) -> String {
    s.split('\n')
        .map(|l| l.trim_end_matches(['\t', '\u{b}', '\u{c}', ' ']))
        .collect::<Vec<_>>()
        .join("\n")
}

/// [F19 §10.2] `bad_value`, the commit-message row: invalid UTF-8 or U+0000 ([F07 §5.2]).
pub const MESSAGE_UTF8: &str = "the commit message is not valid UTF-8 or contains U+0000";
/// [F19 §10.2] `bad_value`, the commit-message row: a result above 65,535 bytes ([F07 §5.2]).
pub const MESSAGE_LENGTH: &str = "the commit message is longer than 65,535 bytes";
/// [F19 §10.2] `bad_value`, the commit-message row: a last paragraph that begins with `Moirai-` ([F07 §5.2]).
pub const MESSAGE_TRAILER: &str = "the commit message ends in a paragraph that begins with Moirai-";

/// N(m) at write time with the refusals of [F07 §5.2] (`bad_value`, exit 2, case `message` of [F19 §10.3], with
/// [F19 §10.2]'s texts; spec sync 2b S2B-F-16): U+0000; a result above 65,535 bytes; a last paragraph whose first line
/// begins with `Moirai-`. The empty result is the absent message. A `&str` is valid UTF-8; [`normalise_message_bytes`]
/// takes raw bytes.
// spec: [F07 §5.1]
// spec: [F07 §5.2]
pub fn normalise_message(m: &str) -> Res<String> {
    if m.contains('\0') {
        return Err(Refusal::bad_value("message", MESSAGE_UTF8));
    }
    let out = steps_2_to_4(m);
    if out.len() > 65_535 {
        return Err(Refusal::bad_value("message", MESSAGE_LENGTH));
    }
    let lines: Vec<&str> = out.split('\n').collect();
    let start = lines
        .iter()
        .rposition(|l| l.is_empty())
        .map_or(0, |i| i + 1);
    if lines.get(start).is_some_and(|l| l.starts_with("Moirai-")) {
        return Err(Refusal::bad_value("message", MESSAGE_TRAILER));
    }
    Ok(out)
}

/// N of raw message bytes ([F07 §5.1] step 1): bytes that are not valid UTF-8 are refused (`bad_value`, case
/// `message`).
pub fn normalise_message_bytes(m: &[u8]) -> Res<String> {
    let s = std::str::from_utf8(m).map_err(|_| Refusal::bad_value("message", MESSAGE_UTF8))?;
    normalise_message(s)
}

/// N_imp of an imported message ([F07 §5.3]), which never refuses: lossy UTF-8 decoding with U+FFFD for every
/// ill-formed subsequence (maximal subparts), U+0000 → U+FFFD, steps 2–4, then a cut at the last scalar boundary at or
/// before 65,535 bytes followed by steps 3 and 4 once more.
// spec: [F07 §5.3]
pub fn normalise_imported(m: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(m).replace('\0', "\u{FFFD}");
    let mut out = steps_2_to_4(&decoded);
    if out.len() > 65_535 {
        let mut cut = 65_535;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out = trim_lines(&out);
        while out.ends_with('\n') {
            out.pop();
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// §3 the commit id; §12 per kind
// ---------------------------------------------------------------------------------------------------------------

/// The git provenance group of a commit ([F07 §3.6]; [F06 §4.4.6]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Git {
    /// The repository's object format.
    pub algo: Algo,
    /// The tree's git `HEAD` commit.
    pub head: Option<Vec<u8>>,
    /// The short branch name, or empty.
    pub branch: String,
    /// The canonical root text of the tree.
    pub worktree: String,
    /// The lane's base commit.
    pub base: Option<Vec<u8>>,
}

/// The items 1–9 of a commit ([F07 §3.1]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Header {
    /// Item 1: the kind's canonical name (§4).
    pub kind: String,
    /// Item 2: the parents' stated ids.
    pub parents: Vec<[u8; 32]>,
    /// Item 3.
    pub hlc: u64,
    /// Item 4.
    pub actor: String,
    /// Item 4.
    pub role: String,
    /// Item 4.
    pub session: String,
    /// Item 5.
    pub git: Option<Git>,
    /// Item 6: the normalised message.
    pub message: String,
    /// Item 7.
    pub schema_version: u32,
    /// Item 8: the reverted or picked commit.
    pub origin: Option<[u8; 32]>,
    /// Item 9: the foreign object format and object id.
    pub foreign: Option<Oid>,
}

/// The commit-id input C of [F07 §3.1]: items 1–9 and `changeset_digest`, in order.
// spec: [F07 §3.1]
pub fn commit_input(h: &Header, digest: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    lp(&mut out, b"moirai-commit-v1");
    lp(&mut out, h.kind.as_bytes());
    out.extend_from_slice(&(h.parents.len() as u32).to_le_bytes());
    for p in &h.parents {
        out.extend_from_slice(p);
    }
    out.extend_from_slice(&h.hlc.to_le_bytes());
    lp(&mut out, h.actor.as_bytes());
    lp(&mut out, h.role.as_bytes());
    lp(&mut out, h.session.as_bytes());
    match &h.git {
        None => {
            for _ in 0..5 {
                lp(&mut out, b"");
            }
        }
        Some(g) => {
            let digests = g.head.is_some() || g.base.is_some();
            lp(
                &mut out,
                if digests {
                    g.algo.name().as_bytes()
                } else {
                    b""
                },
            );
            lp(&mut out, g.head.as_deref().unwrap_or(&[]));
            lp(&mut out, g.branch.as_bytes());
            lp(&mut out, g.worktree.as_bytes());
            lp(&mut out, g.base.as_deref().unwrap_or(&[]));
        }
    }
    lp(&mut out, h.message.as_bytes());
    out.extend_from_slice(&h.schema_version.to_le_bytes());
    lp(&mut out, h.origin.as_ref().map_or(&[][..], |o| &o[..]));
    oid_payload(&mut out, h.foreign.as_ref());
    out.extend_from_slice(digest);
    out
}

/// `commit_id = BLAKE3-256(C)` ([F07 §3.1]).
// spec: [F07 §3.1]
pub fn commit_id(h: &Header, digest: &[u8; 32]) -> [u8; 32] {
    b3_256(&commit_input(h, digest))
}

/// The deterministic `hlc` of a foreign or import-checkpoint commit ([F06 §4.4.4]):
/// `max((T × 1000) << 16, max over its parents of (p.hlc + 1))`; `None` (an `ImageParse` violation) when T is negative,
/// `T × 1000` is 2^48 or more, or a parent term overflows.
// spec: [F06 §4.4.4]
pub fn foreign_hlc(committer_secs: i64, parent_hlcs: &[u64]) -> Option<u64> {
    let t = u64::try_from(committer_secs).ok()?;
    let ms = t.checked_mul(1000)?;
    if ms >= 1 << 48 {
        return None;
    }
    let mut h = ms << 16;
    for p in parent_hlcs {
        h = h.max(p.checked_add(1)?);
    }
    Some(h)
}

/// A git commit as an importer reads it for §12.3 and §12.4: its parents as this store holds them (full id and
/// `hlc`), the committer time, the author's e-mail bytes, the message bytes, the tree marker's schema version and
/// its own object id in the destination's object format.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GitCommit {
    /// Each git parent's id in this store and its `hlc`, in git order.
    pub parents: Vec<([u8; 32], u64)>,
    /// The committer timestamp in seconds.
    pub committer_secs: i64,
    /// The bytes between `<` and `>` of the author line.
    pub author_email: Vec<u8>,
    /// The message bytes (the whole message, or the part before the trailer block for a checkpoint).
    pub message: Vec<u8>,
    /// The `.moirai-image` marker's `schema-version:`.
    pub schema_version: u32,
    /// The commit's own object id.
    pub oid: Oid,
}

/// The header of a foreign commit ([F07 §12.3]); `None` when §12.3 stages `ImageParse` (more than two parents, or an
/// `hlc` out of range).
// spec: [F07 §12.3]
pub fn foreign_header(g: &GitCommit) -> Option<Header> {
    if g.parents.len() > 2 {
        return None;
    }
    let hlcs: Vec<u64> = g.parents.iter().map(|p| p.1).collect();
    Some(Header {
        kind: if g.parents.len() == 2 {
            "merge".into()
        } else {
            "ordinary".into()
        },
        parents: g.parents.iter().map(|p| p.0).collect(),
        hlc: foreign_hlc(g.committer_secs, &hlcs)?,
        actor: format!(
            "git:{}",
            String::from_utf8_lossy(&g.author_email).replace('\0', "\u{FFFD}")
        ),
        role: String::new(),
        session: String::new(),
        git: None,
        message: normalise_imported(&g.message),
        schema_version: g.schema_version,
        origin: None,
        foreign: Some(g.oid.clone()),
    })
}

/// The id of a foreign commit ([F07 §12.3]), given its item 10.
pub fn foreign_id(g: &GitCommit, digest: &[u8; 32]) -> Option<[u8; 32]> {
    Some(commit_id(&foreign_header(g)?, digest))
}

/// The header of an import-checkpoint commit ([F07 §12.4]): at most one parent (the ref's previous checkpoint), actor
/// `image:checkpoint`, N_imp of the message part before the trailer block.
// spec: [F07 §12.4]
pub fn checkpoint_header(g: &GitCommit) -> Option<Header> {
    if g.parents.len() > 1 {
        return None;
    }
    let hlcs: Vec<u64> = g.parents.iter().map(|p| p.1).collect();
    Some(Header {
        kind: "checkpoint".into(),
        parents: g.parents.iter().map(|p| p.0).collect(),
        hlc: foreign_hlc(g.committer_secs, &hlcs)?,
        actor: "image:checkpoint".into(),
        role: String::new(),
        session: String::new(),
        git: None,
        message: normalise_imported(&g.message),
        schema_version: g.schema_version,
        origin: None,
        foreign: Some(g.oid.clone()),
    })
}

/// The id of an import-checkpoint commit ([F07 §12.4]), given its item 10.
pub fn checkpoint_id(g: &GitCommit, digest: &[u8; 32]) -> Option<[u8; 32]> {
    Some(commit_id(&checkpoint_header(g)?, digest))
}

/// A float value in canonical form, for fixtures and tests.
pub fn f64_value(x: f64) -> Value {
    Value::F64(F64::new(x).expect("a finite float"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{EdgeKey, Tomb};

    fn uid_of(n: Nid) -> Uid {
        let mut b = [0u8; 16];
        b[12..].copy_from_slice(&n.0.to_be_bytes());
        Uid(b)
    }

    /// [F07 §16]'s status entry of 43 bytes and its 74-byte digest input.
    #[test]
    fn the_status_entry_of_section_16() {
        let u = Uid([
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF,
        ]);
        let k = CKey::Node {
            uid: u,
            class: 2,
            name: String::new(),
            dst: None,
            disc: None,
        };
        let q = CVal::Plain(status_value("in_progress", "none"));
        let e = entry(&k, None, Some(&q));
        assert_eq!(e.len(), 43);
        let want: Vec<u8> = [
            &[0x01][..],
            &u.0,
            &[0x02, 0x00, 0x01, 0x0B, 0, 0, 0],
            b"in_progress",
            &[4, 0, 0, 0],
            b"none",
        ]
        .concat();
        assert_eq!(e, want);
        let mut input = Vec::new();
        lp(&mut input, b"moirai-changeset-v1");
        input.extend_from_slice(&e);
        input.extend_from_slice(&1u64.to_le_bytes());
        assert_eq!(input.len(), 74);
        assert_eq!(digest_of(&[e]), b3_256(&input));
    }

    /// [F07 §16]'s commit input C of 196 bytes.
    #[test]
    fn the_commit_input_of_section_16() {
        let h = Header {
            kind: "ordinary".into(),
            parents: vec![[7; 32]],
            hlc: 0x01A0_C450_6C00_0003,
            actor: "dev#1".into(),
            role: "developer".into(),
            session: "claude:s1".into(),
            git: None,
            message: "claim --start".into(),
            schema_version: 1,
            origin: None,
            foreign: None,
        };
        let c = commit_input(&h, &[9; 32]);
        assert_eq!(c.len(), 196);
        assert_eq!(&c[..4], &[0x10, 0, 0, 0]);
        assert_eq!(
            &c[68..76],
            &[0x03, 0x00, 0x00, 0x6C, 0x50, 0xC4, 0xA0, 0x01]
        );
        assert_eq!(commit_id(&h, &[9; 32]), b3_256(&c));
    }

    /// [F07 §16]'s value examples: a `path`, a `labels` set in the order of §2.4, a counter delta of −3 and a
    /// `pathmove`.
    #[test]
    fn the_values_of_section_16() {
        let path = Value::Path(PathVal {
            root: "project".into(),
            text: "docs/a.md".into(),
        });
        let mut want = vec![0x0C, 7, 0, 0, 0];
        want.extend_from_slice(b"project");
        want.extend_from_slice(&[9, 0, 0, 0]);
        want.extend_from_slice(b"docs/a.md");
        assert_eq!(cv(Some(&path), &uid_of), want);
        let labels = Value::set(vec![
            Value::Text("storage".into()),
            Value::Text("l5".into()),
            Value::Text("perf".into()),
        ])
        .unwrap();
        let mut want = vec![0x09, 0x07, 3, 0, 0, 0, 2, 0, 0, 0];
        want.extend_from_slice(b"l5");
        want.extend_from_slice(&[4, 0, 0, 0]);
        want.extend_from_slice(b"perf");
        want.extend_from_slice(&[7, 0, 0, 0]);
        want.extend_from_slice(b"storage");
        assert_eq!(cv(Some(&labels), &uid_of), want);
        let k = CKey::Node {
            uid: Uid([1; 16]),
            class: 6,
            name: "n".into(),
            dst: None,
            disc: None,
        };
        let e = entry(&k, Some(&CVal::Counter(5)), Some(&CVal::Counter(2)));
        assert_eq!(&e[e.len() - 9..], &[1, 3, 0, 0, 0, 0, 0, 0, 0]);
        let pm = Value::PathMove(Box::new(PathMove {
            hlc: 0x01A0_C450_6C00_0003,
            class: MoveClass::Explicit,
            from: PathVal {
                root: "project".into(),
                text: "docs/plan/".into(),
            },
            to: PathVal {
                root: "project".into(),
                text: "docs/archive/plan/".into(),
            },
            git: None,
        }));
        let got = cv(Some(&pm), &uid_of);
        assert_eq!(got[0], 0x0E);
        assert_eq!(&got[1..9], &0x01A0_C450_6C00_0003u64.to_le_bytes());
        assert_eq!(&got[got.len() - 8..], &[0; 8]);
        // −0.0 enters as +0.0; empty values are absent.
        assert_eq!(
            cv(Some(&f64_value(-0.0)), &uid_of),
            vec![5, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(cv(Some(&Value::Text(String::new())), &uid_of), vec![0]);
    }

    /// An empty prefix has the digest of the empty string, which the block writes present ([F07 §16]).
    #[test]
    fn an_empty_prefix_is_present_in_the_block() {
        let a = Anchor {
            kind: "quote".into(),
            mode: "live".into(),
            watch: "span".into(),
            quote_h: Some(b3_128(b"x")),
            prefix_h: Some(b3_128(b"")),
            suffix_h: Some(b3_128(b"y")),
            captured: [3; 16],
            resolver: 1,
            ..Anchor::default()
        };
        let b = selector_block(&a);
        let empty = [
            0xAF, 0x13, 0x49, 0xB9, 0xF5, 0xF9, 0xA1, 0xA6, 0xA0, 0x40, 0x4D, 0xEA, 0x36, 0xDC,
            0xC9, 0x49,
        ];
        assert!(
            b.windows(20)
                .any(|w| w[..4] == [16, 0, 0, 0] && w[4..] == empty)
        );
    }

    #[test]
    fn messages_normalise_as_section_5_4_says() {
        for (m, want) in [
            (
                "fix lock\r\n\r\nsee #12  \r\n\r\n\r\n",
                Some("fix lock\n\nsee #12"),
            ),
            ("a\rb\t\n", Some("a\nb")),
            ("\n\nsubject", Some("\n\nsubject")),
            (" \t\n", Some("")),
            ("done\n\nMoirai-Ref: main", None),
            (
                "done\n\nnote\nMoirai-Ref: main",
                Some("done\n\nnote\nMoirai-Ref: main"),
            ),
        ] {
            assert_eq!(normalise_message(m).ok().as_deref(), want, "{m:?}");
        }
        assert_eq!(
            normalise_imported(b"a\0b\xFF\r\nMoirai-X: y\n\n"),
            "a\u{FFFD}b\u{FFFD}\nMoirai-X: y"
        );
        let long = "é".repeat(40_000);
        let n = normalise_imported(long.as_bytes());
        assert!(n.len() <= 65_535 && n.is_char_boundary(n.len()));
    }

    #[test]
    fn foreign_hlc_takes_milliseconds_and_the_parents() {
        assert_eq!(foreign_hlc(1, &[]), Some(1000 << 16));
        assert_eq!(foreign_hlc(1, &[(1000 << 16) + 5]), Some((1000 << 16) + 6));
        assert_eq!(foreign_hlc(-1, &[]), None);
        assert_eq!(foreign_hlc(1 << 40, &[]), None);
        assert_eq!(foreign_hlc(1, &[u64::MAX]), None);
    }

    fn st_with(nodes: Vec<(u32, Node)>) -> State {
        let mut st = State::default();
        for (n, x) in nodes {
            st.nodes.insert(Nid(n), x);
        }
        st
    }

    fn task(n: u32) -> Node {
        let s = Schema::default();
        let mut x = Node::new(uid_of(Nid(n)), "task", &s, Default::default());
        x.set_field(&s, "title", Some(Value::Text(format!("t{n}"))));
        x
    }

    /// Deleting a node yields its existence entry and an absent entry for each of its value keys (a counter's delta is
    /// minus its total); the tombstone keeps only its title and retained edges ([F07 §6.4], open point 2).
    #[test]
    fn a_tombstone_keeps_its_title_and_retained_edges_only() {
        let s = Schema::default();
        let mut live = task(1);
        live.set_field(&s, "priority", Some(Value::Enum("P1".into())));
        live.fields.insert("reopen_count".into(), Value::Counter(2));
        live.body = Some("b".into());
        live.out.insert(
            EdgeKey {
                kind: "cites".into(),
                dst: Nid(2),
                disc: None,
            },
            EdgeProps::default(),
        );
        let mut dead = live.clone();
        dead.tomb = Some(Tomb {
            reason: None,
            replaced_by: None,
        });
        dead.fields.remove("reopen_count");
        dead.body = None;
        let p = canonical_state(&st_with(vec![(1, live)]), &uid_of);
        let q = canonical_state(&st_with(vec![(1, dead)]), &uid_of);
        assert_eq!(
            q.len(),
            3,
            "existence, title and the historical edge: {q:?}"
        );
        let e = entries(&p, &q);
        // existence, priority (absent), counter (−2), body (absent).
        assert_eq!(e.len(), 4);
        let counter = e.iter().find(|x| x[17] == 6).unwrap();
        assert_eq!(&counter[counter.len() - 9..], &[1, 2, 0, 0, 0, 0, 0, 0, 0]);
    }

    /// A symmetric edge is one key whichever endpoint the model keeps it on ([F07 §6.6]).
    #[test]
    fn symmetric_edges_key_at_the_smaller_uid() {
        let k = |src: u32, dst: u32| {
            node_ckey(
                uid_of(Nid(src)),
                &Aspect::Edge(EdgeKey {
                    kind: "relates".into(),
                    dst: Nid(dst),
                    disc: None,
                }),
                &uid_of,
                true,
            )
        };
        assert_eq!(k(1, 2), k(2, 1));
    }

    /// [F07 §6.3]: a field value equal to the field's default in the state's own effective schema is absent, also when
    /// it was written under an older default; the node image follows. A default change is one schema entry and one
    /// field entry.
    #[test]
    fn a_value_equal_to_the_current_default_is_absent() {
        use crate::schema::{Shape, Ty};
        let item = |default: i64| {
            Item::Field(FieldItem {
                kind: Some("task".into()),
                name: "effort".into(),
                ty: Ty::Int,
                class: "scalar",
                storage: Storage::Field,
                decl: 1,
                optional: false,
                default: Some(Value::Int(default)),
                range: None,
                one_line: false,
                ascii: false,
                shape: Shape::Plain,
                index: "none",
                coerce: "none",
                retired: false,
            })
        };
        let ik = ItemKey::Field("task".into(), "effort".into());
        let mut st = State::default();
        st.schema.items.insert(ik.clone(), item(3));
        let mut x = task(1);
        x.set_field(&st.schema, "effort", Some(Value::Int(5)));
        assert_eq!(x.fields.get("effort"), Some(&Value::Int(5)));
        st.nodes.insert(Nid(1), x);
        let key = CKey::Node {
            uid: uid_of(Nid(1)),
            class: 4,
            name: "effort".into(),
            dst: None,
            disc: None,
        };
        let p = canonical_state(&st, &uid_of);
        assert!(p.contains_key(&key));
        // The default becomes 5: the stored 5 is now the default.
        let mut after = st.clone();
        after.schema.items.insert(ik, item(5));
        let q = canonical_state(&after, &uid_of);
        assert!(!q.contains_key(&key), "{q:?}");
        assert!(
            !node_image(&after.schema, &after.nodes[&Nid(1)])
                .contains_key(&Aspect::Field("effort".into()))
        );
        assert_eq!(entries(&p, &q).len(), 2, "the item and the field");
    }

    use proptest::prelude::*;

    /// A commit message built from pieces that reach every step of N and N_imp: line ends of each kind, trailing blanks,
    /// empty lines, `Moirai-` paragraphs and multi-byte characters.
    fn message() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            Just("fix lock"),
            Just("a"),
            Just(" "),
            Just("\t"),
            Just("\u{b}"),
            Just("\u{c}"),
            Just("\n"),
            Just("\r\n"),
            Just("\r"),
            Just("\n\n"),
            Just("Moirai-"),
            Just("Moirai-Ref: main"),
            Just("é"),
            Just("жук"),
            Just("日本"),
            Just("🙂"),
        ];
        proptest::collection::vec(piece, 0..16).prop_map(|v| v.concat())
    }

    /// The first line of the last paragraph of a normalised message (the lines after its last empty line).
    fn last_paragraph_head(n: &str) -> &str {
        let lines: Vec<&str> = n.split('\n').collect();
        let start = lines
            .iter()
            .rposition(|l| l.is_empty())
            .map_or(0, |i| i + 1);
        lines.get(start).copied().unwrap_or("")
    }

    /// Keys of every class ([F07 §6.1]): node keys of classes 1–8 over a few uids (field and counter names, edges with
    /// destinations, `at` edges with anchor uids, symmetric edges keyed at their smaller endpoint) and schema keys of
    /// every item class.
    fn ckey() -> impl Strategy<Value = CKey> {
        let uid = (1u8..4).prop_map(|b| {
            let mut u = [0u8; 16];
            u[15] = b;
            u[0] = b.wrapping_mul(0x51);
            Uid(u)
        });
        let name = prop_oneof![Just("a"), Just("b"), Just("aa"), Just("é"), Just("z")];
        let classes = proptest::sample::select(vec![1u8, 2, 3, 4, 5, 6, 8]);
        let node = (uid.clone(), classes, name.clone()).prop_map(|(u, class, n)| CKey::Node {
            uid: u,
            class,
            name: if matches!(class, 4 | 6) {
                n.to_string()
            } else {
                String::new()
            },
            dst: None,
            disc: None,
        });
        let edge = (
            uid.clone(),
            uid.clone(),
            prop_oneof![Just("at"), Just("blocks"), Just("relates"), Just("cites")],
            any::<u8>(),
        )
            .prop_map(|(s, d, kind, a)| {
                let nid = |u: Uid| Nid(u32::from(u.0[15]));
                let back = |n: Nid| {
                    let b = n.0 as u8;
                    let mut u = [0u8; 16];
                    u[15] = b;
                    u[0] = b.wrapping_mul(0x51);
                    Uid(u)
                };
                let disc = (kind == "at").then_some(Uid([a; 16]));
                node_ckey(
                    s,
                    &Aspect::Edge(EdgeKey {
                        kind: kind.into(),
                        dst: nid(d),
                        disc,
                    }),
                    &back,
                    kind == "relates",
                )
            });
        let schema = (1u8..6, name.clone(), name).prop_map(|(class, a, b)| {
            schema_ckey(&match class {
                1 => ItemKey::Kind(a.into()),
                2 => ItemKey::Field(a.into(), b.into()),
                3 => ItemKey::Enum(a.into(), b.into(), "v".into()),
                4 => ItemKey::Edge(a.into()),
                _ => ItemKey::Query(a.into()),
            })
        });
        prop_oneof![node, edge, schema]
    }

    /// [F07 §10.3]'s order, written from its text: node entries before schema entries; node entries by (uid bytewise,
    /// class code, name bytes, dst uid bytewise, disc with the empty disc first); schema entries by (item class, then
    /// each key component's bytes).
    fn spec_order(a: &CKey, b: &CKey) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let bytes = |u: &Option<Uid>| u.map_or(Vec::new(), |u| u.0.to_vec());
        match (a, b) {
            (CKey::Node { .. }, CKey::Schema { .. }) => Ordering::Less,
            (CKey::Schema { .. }, CKey::Node { .. }) => Ordering::Greater,
            (
                CKey::Node {
                    uid: u1,
                    class: c1,
                    name: n1,
                    dst: d1,
                    disc: s1,
                },
                CKey::Node {
                    uid: u2,
                    class: c2,
                    name: n2,
                    dst: d2,
                    disc: s2,
                },
            ) => {
                u1.0.cmp(&u2.0)
                    .then(c1.cmp(c2))
                    .then(n1.as_bytes().cmp(n2.as_bytes()))
                    .then(bytes(d1).cmp(&bytes(d2)))
                    .then(bytes(s1).cmp(&bytes(s2)))
            }
            (CKey::Schema { class: c1, key: k1 }, CKey::Schema { class: c2, key: k2 }) => {
                let comps = |k: &Vec<String>| -> Vec<Vec<u8>> {
                    k.iter().map(|c| c.as_bytes().to_vec()).collect()
                };
                c1.cmp(c2).then(comps(k1).cmp(&comps(k2)))
            }
        }
    }

    /// A value for a key: a counter total on class 6, a plain value otherwise.
    fn cval_for(k: &CKey, v: u8) -> CVal {
        match k {
            CKey::Node { class: 6, .. } => CVal::Counter(i64::from(v) + 1),
            _ => CVal::Plain(vec![1, v]),
        }
    }

    proptest! {
        /// N is idempotent, N_imp of a message N accepts is N, and N refuses exactly the messages whose last paragraph
        /// begins with `Moirai-`, which N_imp keeps ([F07 §5.1]–§5.3).
        #[test]
        fn normalisation_is_idempotent(m in message()) {
            match normalise_message(&m) {
                Ok(n) => {
                    prop_assert_eq!(normalise_message(&n).unwrap(), n.clone());
                    prop_assert_eq!(normalise_imported(m.as_bytes()), n.clone());
                    prop_assert!(!last_paragraph_head(&n).starts_with("Moirai-"));
                }
                Err(e) => {
                    prop_assert_eq!(e.get_str("case"), Some("message"));
                    let imp = normalise_imported(m.as_bytes());
                    prop_assert!(last_paragraph_head(&imp).starts_with("Moirai-"), "{imp:?}");
                    prop_assert_eq!(normalise_imported(imp.as_bytes()), imp.clone());
                }
            }
        }

        /// The entries follow [F07 §10.3]'s order over keys of every class, a symmetric edge is keyed at its smaller
        /// endpoint, an unchanged state has no entry, and the digest counts the entries (§10.4).
        #[test]
        fn entries_follow_key_order(ks in proptest::collection::vec((ckey(), any::<u8>(), any::<u8>(), 0u8..4), 0..24)) {
            let mut p = Cs::new();
            let mut q = Cs::new();
            for (k, a, b, present) in &ks {
                if let CKey::Node { class: 7, name, uid, dst: Some(d), .. } = k
                    && name == "relates"
                {
                    prop_assert!(uid <= d, "a symmetric edge is keyed at its smaller endpoint");
                }
                if present & 1 == 1 {
                    p.insert(k.clone(), cval_for(k, *a));
                }
                if present & 2 == 2 {
                    q.insert(k.clone(), cval_for(k, *b));
                }
            }
            let mut keys: Vec<&CKey> = p.keys().chain(q.keys()).collect();
            keys.sort_by(|a, b| spec_order(a, b));
            keys.dedup();
            let want: Vec<Vec<u8>> = keys
                .into_iter()
                .filter(|k| differs(p.get(k), q.get(k)))
                .map(|k| entry(k, p.get(k), q.get(k)))
                .collect();
            let e = entries(&p, &q);
            prop_assert_eq!(&e, &want);
            prop_assert!(entries(&p, &p).is_empty());
            prop_assert_eq!(digest_of(&e), changeset_digest(&p, &q));
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
