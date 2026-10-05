//! Schema as data ([F08 §8], [F08 §9]; [60 §2.5] "Schema as data"): the core schema of version 1, transcribed row for
//! row from [F08 §9], and the project items a view carries ([F08 §8.1]). The statuses of every core kind — their
//! order, side flags, covers and `done` values — are read from the rule tables `statuses` and `status-lattice`
//! ([RULES/status-machines], [RULES/merge-table] §6), and the merge class of every core field and edge kind is checked
//! against `field-class` and `edge-class` ([RULES/merge-table] §8) by this module's tests.
//!
//! The **effective schema** of a view is [`core`] together with the view's items ([`Schema`]); a weakening item adds
//! a kind, a field, an enumeration value, an edge kind or a named query, and never changes, retires or shadows a core
//! item ([F08 §8.1]).

use crate::err::Refusal;
use crate::idem::Cj;
use crate::rules::rules;
use crate::value::{Nid, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The stored type of a field ([F08 §5.1]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    /// `bool`.
    Bool,
    /// `int`.
    Int,
    /// `counter`.
    Counter,
    /// `f64`.
    F64,
    /// `enum`.
    Enum,
    /// `text`, inline.
    Text,
    /// `sym`: interned one-line text.
    Sym,
    /// A set of the element type.
    Set(Elem),
    /// `ref`.
    Ref,
    /// `commitref`.
    Commit,
    /// `path`.
    Path,
    /// `oid`.
    Oid,
    /// `pathmove`.
    PathMove,
    /// The body.
    Body,
}

/// The element type of a set field ([F08 §5.2] `set`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Elem {
    /// `sym` elements.
    Sym,
    /// `path` elements.
    Path,
    /// `pathmove` elements.
    PathMove,
    /// `int` elements.
    Int,
}

/// Where a field's value lives ([F08 §8.4.2]); it decides nothing in the model except which fields are header
/// enumerations, flags, the title and the body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Storage {
    /// A `NodeHdr` column.
    Header,
    /// A source-truth flag bit.
    Flag,
    /// A cold column.
    Cold,
    /// The field block.
    Field,
    /// The title.
    Title,
    /// The body.
    Body,
}

/// The shape rule of a text-valued field ([F08 §5.4]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// No sub-format.
    Plain,
    /// A record list with its members: (name, required) ([F08 §5.4.5]).
    Records(&'static [(&'static str, bool)]),
    /// Path globs rooted at `project` ([F08 §5.4.3]).
    Globs,
    /// Tagged scope elements `role:`, `phase:`, `lane:`, `path:` ([F08 §5.4.6]).
    Tagged,
    /// A base-62 order key ([F08 §5.4.4]).
    OrderKey,
}

/// One field row ([F08 §8.5.2]).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldItem {
    /// The kind, or `None` for `*`.
    pub kind: Option<String>,
    /// The field name.
    pub name: String,
    /// Its type.
    pub ty: Ty,
    /// Its merge class, a class of [RULES/merge-table] `merge-classes`.
    pub class: &'static str,
    /// Its storage.
    pub storage: Storage,
    /// Declaration order.
    pub decl: u16,
    /// F2 `optional`.
    pub optional: bool,
    /// The default of a non-optional field; `None` with `optional` false is a required field.
    pub default: Option<Value>,
    /// `int` range.
    pub range: Option<(i64, i64)>,
    /// `one_line`.
    pub one_line: bool,
    /// `ascii`: every byte 20–7E.
    pub ascii: bool,
    /// The text shape.
    pub shape: Shape,
    /// F2/F5 `index` by name: `none`, `column`, `bitmap` ([F08 §8.4.3]).
    pub index: &'static str,
    /// F2 `coerce` by name: `none`, `priority`, `revision-integer`, `timestamp` ([F08 §8.4.4]).
    pub coerce: &'static str,
    /// Retired by a strengthening migration.
    pub retired: bool,
}

/// One enumeration value ([F08 §8.5.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumItem {
    /// The kind, or `None` for `*`.
    pub kind: Option<String>,
    /// The field (`status` for a kind's statuses).
    pub field: String,
    /// The value's name.
    pub name: String,
    /// F2 `sort_rank`.
    pub rank: u16,
    /// A side state ([RULES/merge-table] §6).
    pub side: bool,
    /// Makes the virtual `done` true.
    pub done: bool,
    /// The values it lies immediately above in the merge lattice.
    pub covers: Vec<String>,
    /// Retired.
    pub retired: bool,
}

/// `uid_derivation` of a kind ([F08 §8.4.7]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UidDerivation {
    /// Random under the injected entropy ([API §17.4]).
    Random,
    /// The file-node derivation (R4).
    FileKey,
}

/// One kind row ([F08 §8.5.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindItem {
    /// The kind name.
    pub name: String,
    /// The kind id: 1–13 core, 64–254 project (store-local).
    pub id: u8,
    /// The uid derivation.
    pub uid: UidDerivation,
    /// `root_variant`: instances with a `root` field are root nodes (`area`).
    pub root_variant: bool,
    /// The existence policy name (`delete-wins`, `resurrect`, `none`).
    pub existence_policy: &'static str,
    /// `title_derived`.
    pub title_derived: bool,
    /// `immutable_fields`.
    pub immutable_fields: bool,
    /// `has_done`.
    pub has_done: bool,
    /// `done_derived`: `done` follows `answered`.
    pub done_derived: bool,
    /// Retired.
    pub retired: bool,
}

/// Edge class ([F08 §8.4.6]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeClass {
    /// `structural`.
    Structural,
    /// `historical`.
    Historical,
}

/// Acyclicity of an edge kind ([F08 §8.4.6]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Acyclic {
    /// `none`.
    None,
    /// `forest` with `max_depth`.
    Forest,
    /// `precedence`: part of I5′'s combined graph.
    Precedence,
    /// `dag`.
    Dag,
    /// `by-construction`.
    ByConstruction,
}

/// Cardinality ([F08 §8.4.6]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    /// `many`.
    Many,
    /// `max-1-per-src`.
    Max1PerSrc,
    /// `max-1-active-per-dst`.
    Max1ActivePerDst,
    /// `chain-1`.
    Chain1,
    /// `typical-1`: informative, not enforced.
    Typical1,
    /// `anchors-min-1`.
    AnchorsMin1,
}

/// Edge properties an edge kind admits ([F08 §8.4.6]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Props {
    /// No property.
    None,
    /// An optional `pinned_commit`.
    Pinned,
    /// `flagged`.
    Flagged,
    /// The anchor record.
    Anchor,
}

/// The endpoint kinds of an edge kind (F1 `src_kinds`, `dst_kinds`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ends {
    /// Every kind, project kinds included.
    Any,
    /// These kinds.
    Kinds(Vec<String>),
}

impl Ends {
    /// Whether a kind is an allowed endpoint.
    pub fn allows(&self, kind: &str) -> bool {
        match self {
            Ends::Any => true,
            Ends::Kinds(k) => k.iter().any(|x| x == kind),
        }
    }
}

/// One edge-kind row with its F1 columns ([F08 §8.5.4], §9.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeItem {
    /// The stored name.
    pub name: String,
    /// The edge id: 1–25 core, 64–254 project.
    pub id: u8,
    /// The class.
    pub class: EdgeClass,
    /// `on_dst` by name ([F08 §8.4.6]).
    pub on_dst: &'static str,
    /// `on_src` by name.
    pub on_src: &'static str,
    /// Acyclicity.
    pub acyclic: Acyclic,
    /// Cardinality.
    pub card: Card,
    /// `max_depth` of a forest.
    pub max_depth: u8,
    /// Properties.
    pub props: Props,
    /// `symmetric`.
    pub symmetric: bool,
    /// `same_kind`.
    pub same_kind: bool,
    /// F1 `lq_name`.
    pub lq_name: String,
    /// F1 source kinds.
    pub src: Ends,
    /// F1 destination kinds.
    pub dst: Ends,
    /// F1 reverse names.
    pub reverse: Vec<String>,
    /// F1 reading.
    pub reading: String,
    /// Retired.
    pub retired: bool,
}

/// A project named query ([F08 §8.5.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryItem {
    /// The query name.
    pub name: String,
    /// The LQ grammar version.
    pub lq_version: u16,
    /// The parameter signature text.
    pub params: String,
    /// The shape word.
    pub shape: String,
    /// The budget-class word.
    pub budget: String,
    /// The portable query text.
    pub text: String,
}

/// A policy row ([F08 §8.5.6]): a [CFG §10.13] row instance and its value in canonical form. A view without the item
/// takes the row's default, and an item never holds the default (absence is its one form).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyItem {
    /// The row instance name ([F14 §7.1] `pname`).
    pub name: String,
    /// The value in [CFG §4.1]'s canonical form; a stored item always has one. `None` only in a `Schema` command's
    /// item, where it (like the row's default) removes the row ([API §9.8]).
    pub value: Option<String>,
}

/// A schema item with its class ([F08 §8.5]).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// Class 1.
    Kind(KindItem),
    /// Class 2.
    Field(FieldItem),
    /// Class 3.
    Enum(EnumItem),
    /// Class 4.
    Edge(EdgeItem),
    /// Class 5.
    Query(QueryItem),
    /// Class 6 ([F08 §8.5.6]).
    Policy(PolicyItem),
}

/// The key of a schema item ([F08 §8.5] "Item key order"): class, then its name components; `*` for "every kind".
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum ItemKey {
    /// A kind by name.
    Kind(String),
    /// (kind or `*`, field).
    Field(String, String),
    /// (kind or `*`, field, value).
    Enum(String, String, String),
    /// An edge kind by stored name.
    Edge(String),
    /// A named query by name.
    Query(String),
    /// A policy row by its instance name ([F08 §8.5.6]).
    Policy(String),
}

impl ItemKey {
    /// The key text of [API §5.3]: `schema:kind:<k>`, `schema:field:<k or *>.<f>`,
    /// `schema:enum:<k or *>.<f>.<v>`, `schema:edge:<e>`, `query:<name>`, `schema:policy:<name>` ([F12 §6.6]).
    pub fn text(&self) -> String {
        match self {
            ItemKey::Kind(k) => format!("schema:kind:{k}"),
            ItemKey::Field(k, f) => format!("schema:field:{k}.{f}"),
            ItemKey::Enum(k, f, v) => format!("schema:enum:{k}.{f}.{v}"),
            ItemKey::Edge(e) => format!("schema:edge:{e}"),
            ItemKey::Query(q) => format!("query:{q}"),
            ItemKey::Policy(p) => format!("schema:policy:{p}"),
        }
    }
}

impl Item {
    /// The item's key.
    pub fn key(&self) -> ItemKey {
        let star = |k: &Option<String>| k.clone().unwrap_or_else(|| "*".into());
        match self {
            Item::Kind(k) => ItemKey::Kind(k.name.clone()),
            Item::Field(f) => ItemKey::Field(star(&f.kind), f.name.clone()),
            Item::Enum(e) => ItemKey::Enum(star(&e.kind), e.field.clone(), e.name.clone()),
            Item::Edge(e) => ItemKey::Edge(e.name.clone()),
            Item::Query(q) => ItemKey::Query(q.name.clone()),
            Item::Policy(p) => ItemKey::Policy(p.name.clone()),
        }
    }
}

/// The core schema of version 1 ([F08 §9]), with its lookup indexes.
#[derive(Debug)]
pub struct Core {
    /// Kinds in id order.
    pub kinds: Vec<KindItem>,
    /// Common fields (`kind` = `None`) then kind fields.
    pub fields: Vec<FieldItem>,
    /// Header and field enumeration values, and every kind's statuses (field `status`).
    pub values: Vec<EnumItem>,
    /// Edge kinds in id order.
    pub edges: Vec<EdgeItem>,
    /// The initial status (default) of each core kind ([F08 §9.1]).
    pub default_status: BTreeMap<String, String>,
    /// Field index: kind name (`""` for the common rows) → field name → position in `fields`.
    field_ix: BTreeMap<String, BTreeMap<String, usize>>,
    /// Value index: kind name (`*` for the common rows) → field → the positions in `values`, in rank order.
    value_ix: BTreeMap<String, BTreeMap<String, Vec<usize>>>,
}

type FieldRow = (
    &'static str,
    Ty,
    &'static str,
    Storage,
    Option<DefaultV>,
    Option<(i64, i64)>,
    u8,
    Shape,
);

/// A default written in the transcription: resolved to a [`Value`] when the core is built.
#[derive(Clone, Copy, Debug)]
enum DefaultV {
    Enum(&'static str),
    Bool(bool),
    Counter,
    InitialStatus,
    Required,
}

const OL: u8 = 1; // one line
const AS: u8 = 2; // ascii
const OPT: u8 = 4; // optional

use DefaultV as D;
use Storage as S;
use Ty as T;

/// The common fields of [F08 §9.2], decl 1–18.
const COMMON: [FieldRow; 18] = [
    (
        "title",
        T::Text,
        "scalar",
        S::Title,
        Some(D::Required),
        None,
        OL,
        Shape::Plain,
    ),
    (
        "abstract",
        T::Text,
        "scalar",
        S::Field,
        None,
        None,
        OL | OPT,
        Shape::Plain,
    ),
    (
        "status",
        T::Enum,
        "status",
        S::Header,
        Some(D::InitialStatus),
        None,
        0,
        Shape::Plain,
    ),
    (
        "resolution",
        T::Enum,
        "status",
        S::Header,
        Some(D::Enum("none")),
        None,
        0,
        Shape::Plain,
    ),
    (
        "priority",
        T::Enum,
        "scalar",
        S::Header,
        Some(D::Enum("P2")),
        None,
        0,
        Shape::Plain,
    ),
    (
        "criticality",
        T::Enum,
        "scalar",
        S::Header,
        Some(D::Enum("normal")),
        None,
        0,
        Shape::Plain,
    ),
    (
        "confidence",
        T::Enum,
        "scalar",
        S::Header,
        Some(D::Enum("unset")),
        None,
        0,
        Shape::Plain,
    ),
    (
        "authority",
        T::Enum,
        "authority",
        S::Header,
        Some(D::Enum("agent")),
        None,
        0,
        Shape::Plain,
    ),
    (
        "parent",
        T::Ref,
        "hierarchy",
        S::Header,
        None,
        None,
        OPT,
        Shape::Plain,
    ),
    (
        "order",
        T::Text,
        "hierarchy",
        S::Field,
        None,
        None,
        OL | AS | OPT,
        Shape::OrderKey,
    ),
    (
        "labels",
        T::Set(Elem::Sym),
        "set",
        S::Field,
        None,
        None,
        OL | OPT,
        Shape::Plain,
    ),
    (
        "pinned",
        T::Bool,
        "scalar",
        S::Flag,
        Some(D::Bool(false)),
        None,
        0,
        Shape::Plain,
    ),
    (
        "archived",
        T::Bool,
        "scalar",
        S::Flag,
        Some(D::Bool(false)),
        None,
        0,
        Shape::Plain,
    ),
    (
        "frozen",
        T::Bool,
        "scalar",
        S::Flag,
        Some(D::Bool(false)),
        None,
        0,
        Shape::Plain,
    ),
    (
        "defer_until",
        T::Int,
        "scalar",
        S::Cold,
        None,
        Some((1, 4_294_967_295)),
        OPT,
        Shape::Plain,
    ),
    (
        "due",
        T::Int,
        "scalar",
        S::Cold,
        None,
        Some((1, 4_294_967_295)),
        OPT,
        Shape::Plain,
    ),
    (
        "reason",
        T::Text,
        "scalar",
        S::Field,
        None,
        None,
        OPT,
        Shape::Plain,
    ),
    (
        "body",
        T::Body,
        "text",
        S::Body,
        None,
        None,
        OPT,
        Shape::Plain,
    ),
];

const U16: Option<(i64, i64)> = Some((0, 65_535));
const NONNEG: Option<(i64, i64)> = Some((0, i64::MAX));
const TARGETS: &[(&str, bool)] = &[
    ("metric", true),
    ("value", true),
    ("unit", false),
    ("op", false),
];
const READINESS: &[(&str, bool)] = &[("item", true), ("state", false), ("reason", false)];
const ALTERNATIVES: &[(&str, bool)] = &[
    ("text", true),
    ("rejected_why", false),
    ("measurement_ref", false),
    ("git_tag", false),
    ("revive_condition", false),
];
const OPTIONS: &[(&str, bool)] = &[("option", true)];

const fn opt(name: &'static str, ty: Ty, class: &'static str) -> FieldRow {
    (name, ty, class, S::Field, None, None, OPT, Shape::Plain)
}

const fn optf(
    name: &'static str,
    ty: Ty,
    class: &'static str,
    flags: u8,
    range: Option<(i64, i64)>,
    shape: Shape,
) -> FieldRow {
    (name, ty, class, S::Field, None, range, flags | OPT, shape)
}

/// The kind fields of [F08 §9.3], per core kind in id order; `decl` starts at 20.
const KIND_FIELDS: [&[FieldRow]; 13] = [
    // task
    &[
        opt("work_kind", T::Enum, "scalar"),
        opt("phase_state", T::Enum, "scalar"),
        optf("assignee", T::Sym, "scalar", OL, None, Shape::Plain),
        opt("acceptance", T::Text, "text"),
        optf(
            "files_owned",
            T::Set(Elem::Sym),
            "glob-set",
            OL,
            None,
            Shape::Globs,
        ),
        optf("estimate", T::Int, "scalar", 0, U16, Shape::Plain),
        (
            "reopen_count",
            T::Counter,
            "counter",
            S::Field,
            Some(D::Counter),
            NONNEG,
            0,
            Shape::Plain,
        ),
        opt("reopen_if", T::Text, "text"),
        (
            "pre_registered",
            T::Bool,
            "scalar",
            S::Field,
            Some(D::Bool(false)),
            None,
            0,
            Shape::Plain,
        ),
    ],
    // doc
    &[
        opt("doc_kind", T::Enum, "scalar"),
        optf("heading", T::Text, "text", OL, None, Shape::Plain),
        optf("revision", T::Int, "scalar", 0, U16, Shape::Plain),
        optf("changed_in_round", T::Int, "scalar", 0, U16, Shape::Plain),
        optf(
            "targets",
            T::Text,
            "scalar",
            0,
            None,
            Shape::Records(TARGETS),
        ),
        optf(
            "readiness",
            T::Text,
            "scalar",
            0,
            None,
            Shape::Records(READINESS),
        ),
    ],
    // note
    &[
        opt("note_kind", T::Enum, "scalar"),
        opt("symptom", T::Text, "text"),
        opt("mechanism", T::Text, "text"),
        opt("defence", T::Text, "text"),
        (
            "incidents",
            T::Counter,
            "counter",
            S::Field,
            Some(D::Counter),
            NONNEG,
            0,
            Shape::Plain,
        ),
        optf(
            "applies_to",
            T::Set(Elem::Sym),
            "glob-set",
            OL,
            None,
            Shape::Tagged,
        ),
        opt("observed_git_sha", T::Oid, "scalar"),
        optf("review_after", T::Int, "scalar", 0, NONNEG, Shape::Plain),
    ],
    // rule
    &[
        opt("text", T::Text, "text"),
        opt("enforcement", T::Enum, "scalar"),
        optf(
            "applies_to",
            T::Set(Elem::Sym),
            "glob-set",
            OL,
            None,
            Shape::Tagged,
        ),
        optf("since", T::Int, "scalar", 0, NONNEG, Shape::Plain),
        opt("rationale", T::Text, "text"),
        opt("owner_quote", T::Text, "owner"),
    ],
    // decision
    &[
        opt("context", T::Text, "text"),
        opt("what", T::Text, "text"),
        opt("why", T::Text, "text"),
        opt("tradeoff", T::Text, "text"),
        optf(
            "alternatives",
            T::Text,
            "scalar",
            0,
            None,
            Shape::Records(ALTERNATIVES),
        ),
        opt("revive_condition", T::Text, "text"),
        opt("owner_quote", T::Text, "owner"),
    ],
    // question
    &[
        opt("q_kind", T::Enum, "scalar"),
        opt("asked_of", T::Enum, "scalar"),
        optf(
            "options",
            T::Text,
            "scalar",
            0,
            None,
            Shape::Records(OPTIONS),
        ),
        opt("answer", T::Text, "text"),
    ],
    // finding
    &[
        optf("local_id", T::Sym, "scalar", OL, None, Shape::Plain),
        opt("severity", T::Enum, "scalar"),
        opt("f_kind", T::Enum, "scalar"),
        (
            "failure_scenario",
            T::Text,
            "text",
            S::Field,
            Some(D::Required),
            None,
            0,
            Shape::Plain,
        ),
        opt("what_needed", T::Text, "text"),
        optf("round", T::Int, "scalar", 0, U16, Shape::Plain),
        opt("evidence", T::Text, "text"),
    ],
    // verdict
    &[
        optf("role", T::Sym, "scalar", OL, None, Shape::Plain),
        optf("round", T::Int, "scalar", 0, U16, Shape::Plain),
        opt("raw_label", T::Text, "text"),
        opt("outcome", T::Enum, "scalar"),
        opt("return_to", T::Enum, "scalar"),
        opt("criteria", T::Text, "text"),
        opt("conditions", T::Text, "text"),
    ],
    // measurement
    &[
        optf("metric", T::Sym, "scalar", OL, None, Shape::Plain),
        opt("value", T::F64, "scalar"),
        optf("unit", T::Sym, "scalar", OL, None, Shape::Plain),
        opt("target", T::F64, "scalar"),
        opt("command", T::Text, "text"),
        opt("measured_on", T::Oid, "scalar"),
        optf("env_host", T::Sym, "scalar", OL, None, Shape::Plain),
        optf("env_profile", T::Sym, "scalar", OL, None, Shape::Plain),
        opt("env_load", T::Enum, "scalar"),
        optf("env_scale", T::Sym, "scalar", OL, None, Shape::Plain),
        opt("baseline", T::Ref, "scalar"),
    ],
    // artifact
    &[
        (
            "root",
            T::Sym,
            "scalar",
            S::Field,
            Some(D::Required),
            None,
            OL,
            Shape::Plain,
        ),
        (
            "origin_path",
            T::Path,
            "identity",
            S::Field,
            Some(D::Required),
            None,
            0,
            Shape::Plain,
        ),
        opt("origin_pred", T::Ref, "identity"),
        (
            "path",
            T::Path,
            "observation",
            S::Field,
            Some(D::Required),
            None,
            0,
            Shape::Plain,
        ),
        opt("oid", T::Oid, "observation"),
        optf("bytes", T::Int, "observation", 0, NONNEG, Shape::Plain),
        opt("observed_git", T::Oid, "observation"),
        opt("observed_blob", T::Oid, "observation"),
        optf("relink", T::Sym, "observation", OL | AS, None, Shape::Plain),
        opt("aliases", T::Set(Elem::Path), "alias-set"),
        opt("artifact_kind", T::Enum, "scalar"),
        opt("replaced_by", T::Ref, "scalar"),
        opt("excerpt", T::Text, "scalar"),
    ],
    // run
    &[
        optf("wf_id", T::Text, "scalar", OL, None, Shape::Plain),
        optf("bg_task_id", T::Text, "scalar", OL, None, Shape::Plain),
        optf("session_id", T::Text, "scalar", OL, None, Shape::Plain),
        opt("script_path", T::Path, "scalar"),
        optf("args_hash", T::Text, "scalar", OL, None, Shape::Plain),
        opt("journal_path", T::Path, "scalar"),
        optf("started", T::Int, "scalar", 0, NONNEG, Shape::Plain),
        optf("ended", T::Int, "scalar", 0, NONNEG, Shape::Plain),
        optf(
            "expected_artifacts",
            T::Set(Elem::Sym),
            "set",
            OL,
            None,
            Shape::Plain,
        ),
        // decl 29 and 30 ([F08 §9.3] `run`; spec sync 2b): the harness and the model family CX-6 reads.
        optf("harness", T::Sym, "scalar", OL, None, Shape::Plain),
        optf("model", T::Sym, "scalar", OL, None, Shape::Plain),
    ],
    // lane
    &[
        opt("worktree_path", T::Path, "scalar"),
        optf("git_branch", T::Sym, "scalar", OL, None, Shape::Plain),
        opt("base_sha", T::Oid, "scalar"),
        opt("tip_sha", T::Oid, "scalar"),
        opt("target_dir", T::Path, "scalar"),
        optf("moirai_branch", T::Sym, "scalar", OL, None, Shape::Plain),
    ],
    // area
    &[
        optf(
            "path_globs",
            T::Set(Elem::Sym),
            "glob-set",
            OL,
            None,
            Shape::Globs,
        ),
        optf("root", T::Sym, "identity", OL, None, Shape::Plain),
        opt("path_moves", T::Set(Elem::PathMove), "pathmove-set"),
    ],
];

/// The core kinds of [F08 §9.1]: (name, uid derivation, root_variant, existence policy, title_derived,
/// immutable_fields, has_done, done_derived).
#[allow(clippy::type_complexity)]
const KINDS: [(&str, UidDerivation, bool, &str, bool, bool, bool, bool); 13] = [
    (
        "task",
        UidDerivation::Random,
        false,
        "delete-wins",
        false,
        false,
        true,
        false,
    ),
    (
        "doc",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "note",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "rule",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "decision",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "question",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        true,
        true,
    ),
    (
        "finding",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "verdict",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        true,
        true,
        false,
    ),
    (
        "measurement",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "artifact",
        UidDerivation::FileKey,
        false,
        "none",
        true,
        false,
        false,
        false,
    ),
    (
        "run",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "lane",
        UidDerivation::Random,
        false,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
    (
        "area",
        UidDerivation::Random,
        true,
        "resurrect",
        false,
        false,
        false,
        false,
    ),
];

/// The "initial status (default)" column of [F08 §9.1]: the status a node of the kind has when it is created without
/// one, which its canonical form holds as absent ([F07 §6.3]). `artifact` may also start `planned` (through
/// `--planned`); every other kind has one initial status.
const DEFAULT_STATUS: [(&str, &str); 13] = [
    ("task", "open"),
    ("doc", "draft"),
    ("note", "active"),
    ("rule", "proposed"),
    ("decision", "proposed"),
    ("question", "open"),
    ("finding", "open"),
    ("verdict", "open"),
    ("measurement", "current"),
    ("artifact", "present"),
    ("run", "running"),
    ("lane", "active"),
    ("area", "active"),
];

/// The `index` column of [F08 §9.2]–§9.3 for the rows that are not `none`: (kind, or `None` for a common row, field,
/// index).
const INDEX: [(Option<&str>, &str, &str); 11] = [
    (None, "labels", "bitmap"),
    (Some("task"), "work_kind", "bitmap"),
    (Some("task"), "phase_state", "bitmap"),
    (Some("task"), "assignee", "bitmap"),
    (Some("finding"), "local_id", "bitmap"),
    (Some("finding"), "severity", "bitmap"),
    (Some("finding"), "f_kind", "bitmap"),
    (Some("finding"), "round", "column"),
    (Some("verdict"), "round", "column"),
    (Some("verdict"), "outcome", "bitmap"),
    (Some("measurement"), "metric", "bitmap"),
];

/// The `coerce` column of [F08 §9.2]–§9.3 for the rows that are not `none`.
const COERCE: [(Option<&str>, &str, &str); 7] = [
    (None, "priority", "priority"),
    (None, "defer_until", "timestamp"),
    (None, "due", "timestamp"),
    (Some("note"), "review_after", "timestamp"),
    (Some("rule"), "since", "timestamp"),
    (Some("run"), "started", "timestamp"),
    (Some("run"), "ended", "timestamp"),
];

/// The header enumerations of [F08 §9.4].
const HEADER_ENUMS: [(&str, &[&str]); 5] = [
    (
        "resolution",
        &[
            "none",
            "completed",
            "wontdo",
            "duplicate",
            "superseded",
            "obsolete",
            "rework",
        ],
    ),
    ("priority", &["P0", "P1", "P2", "P3", "P4"]),
    ("criticality", &["critical", "high", "normal", "low"]),
    (
        "confidence",
        &[
            "unset",
            "verified",
            "observed",
            "inferred",
            "speculative",
            "confirmed",
            "plausible",
        ],
    ),
    (
        "authority",
        &["owner", "orchestrator", "measured", "research", "agent"],
    ),
];

/// The field enumerations of [F08 §9.4]; a leading `!` marks a side value.
const FIELD_ENUMS: [(&str, &str, &[&str]); 13] = [
    (
        "task",
        "work_kind",
        &[
            "design", "impl", "fix", "test", "measure", "merge", "doc", "research", "review",
            "debt", "mutex",
        ],
    ),
    (
        "task",
        "phase_state",
        &[
            "proposed",
            "researching",
            "designing",
            "design_review",
            "refuting",
            "design_approved",
            "implementing",
            "code_review",
            "testing",
            "analysis",
            "accepted",
            "committed",
            "merged",
            "documented",
            "!blocked",
            "!frozen",
            "!deferred",
        ],
    ),
    ("doc", "doc_kind", &["plan", "section", "report", "patch"]),
    (
        "note",
        "note_kind",
        &["note", "hazard", "lesson", "checkpoint", "summary"],
    ),
    ("rule", "enforcement", &["must", "should"]),
    ("question", "q_kind", &["values", "scope", "unclear"]),
    (
        "question",
        "asked_of",
        &["owner", "orchestrator", "architect"],
    ),
    ("finding", "severity", &["blocker", "important", "optional"]),
    (
        "finding",
        "f_kind",
        &[
            "correctness",
            "perf",
            "complexity",
            "security",
            "plan",
            "style",
            "debt",
            "test",
            "deviation",
        ],
    ),
    (
        "verdict",
        "outcome",
        &[
            "pass",
            "pass_with_conditions",
            "fail_fixable",
            "fail_fundamental",
            "unknown",
            "na",
        ],
    ),
    (
        "verdict",
        "return_to",
        &["architect", "developer", "tester", "none"],
    ),
    ("measurement", "env_load", &["quiet", "loaded"]),
    (
        "artifact",
        "artifact_kind",
        &[
            "design",
            "critique",
            "research",
            "impl",
            "test",
            "review",
            "triage",
            "fix",
            "merge",
            "verify",
            "patch",
            "manifest",
            "message",
            "page",
            "plan_file",
            "source",
            "doc",
            "asset",
            "generated",
            "dir",
        ],
    ),
];

const KNOWLEDGE: &[&str] = &["doc", "note", "rule", "decision"];

/// A core edge-kind row of [F08 §9.6]: (stored, class, src, dst, symmetric, same_kind, acyclic, max_depth, card,
/// on_dst, on_src, props, lq_name, reverse names, reading). `None` ends are `any`.
#[allow(clippy::type_complexity)]
type EdgeRow = (
    &'static str,
    EdgeClass,
    Option<&'static [&'static str]>,
    Option<&'static [&'static str]>,
    bool,
    bool,
    Acyclic,
    u8,
    Card,
    &'static str,
    &'static str,
    Props,
    &'static str,
    &'static [&'static str],
    &'static str,
);

use Acyclic as A;
use Card as C;
use EdgeClass::{Historical as H, Structural as St};

/// The 25 core edge kinds of [F08 §9.6], ids 1–25.
const EDGES: [EdgeRow; 25] = [
    (
        "parent",
        St,
        Some(&["task", "doc", "area"]),
        Some(&["task", "doc", "area"]),
        false,
        true,
        A::Forest,
        12,
        C::Max1PerSrc,
        "restrict-cascade-reparent",
        "drop-rollups",
        Props::None,
        "CHILD_OF",
        &["PARENT_OF", "HAS_CHILD", "HAS_SUBTASK"],
        "{a} is a child of {b}",
    ),
    (
        "blocks",
        St,
        Some(&["task", "question"]),
        Some(&["task"]),
        false,
        false,
        A::Precedence,
        0,
        C::Many,
        "drop",
        "repoint-or-flag",
        Props::Flagged,
        "BLOCKS",
        &["BLOCKED_BY"],
        "{a} must finish before {b} starts",
    ),
    (
        "gates",
        St,
        Some(&["verdict"]),
        Some(&["task"]),
        false,
        false,
        A::Precedence,
        0,
        C::Many,
        "drop",
        "repoint-or-flag",
        Props::Flagged,
        "GATES",
        &["GATED_BY"],
        "verdict {a} gates the completion of {b}",
    ),
    (
        "merge_after",
        St,
        Some(&["lane"]),
        Some(&["lane"]),
        false,
        false,
        A::Dag,
        0,
        C::Many,
        "drop-notify",
        "drop",
        Props::None,
        "MERGE_AFTER",
        &[],
        "lane {a} merges after lane {b}",
    ),
    (
        "runs_in",
        St,
        Some(&["run"]),
        Some(&["lane"]),
        false,
        false,
        A::None,
        0,
        C::Max1PerSrc,
        "restrict",
        "drop",
        Props::None,
        "RUNS_IN",
        &[],
        "run {a} runs in lane {b}",
    ),
    (
        "answers",
        St,
        Some(&["decision", "note"]),
        Some(&["question"]),
        false,
        false,
        A::None,
        0,
        C::Max1ActivePerDst,
        "restrict",
        "drop-reopen",
        Props::None,
        "ANSWERS",
        &["ANSWERED_BY"],
        "{a} answers question {b}",
    ),
    (
        "scoped_to",
        St,
        Some(&["note", "rule", "decision", "finding", "measurement"]),
        Some(&["area"]),
        false,
        false,
        A::None,
        0,
        C::Many,
        "restrict-reassign",
        "drop",
        Props::None,
        "SCOPED_TO",
        &[],
        "{a} is scoped to area {b}",
    ),
    (
        "duplicate_of",
        St,
        None,
        None,
        false,
        true,
        A::None,
        0,
        C::Chain1,
        "restrict-repoint",
        "drop",
        Props::None,
        "DUPLICATE_OF",
        &[],
        "{a} duplicates canonical {b}",
    ),
    (
        "depends_on",
        St,
        Some(&["doc"]),
        Some(&["doc"]),
        false,
        false,
        A::Dag,
        0,
        C::Many,
        "drop-src-suspect",
        "drop",
        Props::None,
        "DEPENDS_ON",
        &[],
        "section {a} depends on section {b}",
    ),
    (
        "supersedes",
        H,
        Some(KNOWLEDGE),
        Some(KNOWLEDGE),
        false,
        true,
        A::Dag,
        0,
        C::Max1ActivePerDst,
        "tombstone",
        "retain-warn",
        Props::None,
        "SUPERSEDES",
        &["SUPERSEDED_BY"],
        "{a} supersedes {b}",
    ),
    (
        "derived_from",
        H,
        Some(&["note", "doc", "verdict", "artifact"]),
        None,
        false,
        false,
        A::ByConstruction,
        0,
        C::Many,
        "tombstone-src-suspect",
        "retain",
        Props::Pinned,
        "DERIVED_FROM",
        &[],
        "{a} is derived from {b}",
    ),
    (
        "cites",
        H,
        None,
        Some(KNOWLEDGE),
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone-src-suspect",
        "retain",
        Props::Pinned,
        "CITES",
        &["CITED_BY"],
        "{a} cites {b}",
    ),
    (
        "implements",
        H,
        Some(&["task", "artifact"]),
        Some(&["decision", "doc"]),
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone-src-suspect",
        "retain",
        Props::Pinned,
        "IMPLEMENTS",
        &["IMPLEMENTED_BY"],
        "{a} implements {b}",
    ),
    (
        "refutes",
        H,
        Some(&["finding", "measurement"]),
        Some(&["finding", "decision", "rule"]),
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "REFUTES",
        &["REFUTED_BY"],
        "{a} refutes {b}",
    ),
    (
        "confirms",
        H,
        Some(&["finding", "measurement"]),
        Some(&["finding", "decision", "rule"]),
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "CONFIRMS",
        &["CONFIRMED_BY"],
        "{a} confirms {b}",
    ),
    (
        "verifies",
        H,
        Some(&["measurement", "verdict"]),
        Some(&["finding", "task", "decision"]),
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "VERIFIES",
        &["VERIFIED_BY"],
        "{a} verifies {b}",
    ),
    (
        "addresses",
        H,
        Some(&["task", "artifact"]),
        Some(&["finding"]),
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "ADDRESSES",
        &["ADDRESSED_BY"],
        "{a} addresses finding {b}",
    ),
    (
        "about",
        H,
        Some(&[
            "finding",
            "verdict",
            "measurement",
            "question",
            "rule",
            "decision",
            "note",
        ]),
        None,
        false,
        false,
        A::None,
        0,
        C::Typical1,
        "tombstone",
        "retain",
        Props::None,
        "ABOUT",
        &[],
        "{a} is about {b}",
    ),
    (
        "discovered_from",
        H,
        None,
        Some(&["task"]),
        false,
        false,
        A::ByConstruction,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "DISCOVERED_FROM",
        &[],
        "{a} was discovered from task {b}",
    ),
    (
        "produced",
        H,
        Some(&["run"]),
        None,
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "PRODUCED",
        &[],
        "run {a} produced {b}",
    ),
    (
        "consumed",
        H,
        Some(&["run"]),
        None,
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::Pinned,
        "CONSUMED",
        &[],
        "run {a} consumed {b}",
    ),
    (
        "contradicts",
        H,
        Some(&["rule"]),
        Some(&["rule"]),
        true,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "CONTRADICTS",
        &[],
        "{a} and {b} contradict",
    ),
    (
        "mentions",
        H,
        None,
        None,
        false,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "recompute",
        Props::None,
        "MENTIONS",
        &["MENTIONED_BY"],
        "{a} mentions {b}",
    ),
    (
        "relates",
        H,
        None,
        None,
        true,
        false,
        A::None,
        0,
        C::Many,
        "tombstone",
        "retain",
        Props::None,
        "RELATES",
        &[],
        "{a} and {b} relate",
    ),
    (
        "at",
        H,
        None,
        Some(&["artifact"]),
        false,
        false,
        A::None,
        0,
        C::AnchorsMin1,
        "tombstone-src-suspect",
        "retain-anchors",
        Props::Anchor,
        "AT",
        &[],
        "{a} is anchored in file {b}",
    ),
];

fn build_field(kind: Option<&str>, decl: u16, row: &FieldRow) -> FieldItem {
    let (name, ty, class, storage, default, range, flags, shape) = *row;
    let default = match default {
        None | Some(D::Required) | Some(D::InitialStatus) => None,
        Some(D::Enum(v)) => Some(Value::Enum(v.to_string())),
        Some(D::Bool(b)) => Some(Value::Bool(b)),
        Some(D::Counter) => Some(Value::Counter(0)),
    };
    let optional = flags & OPT != 0;
    let look = |t: &[(Option<&str>, &str, &'static str)]| {
        t.iter()
            .find(|(k, f, _)| *k == kind && *f == name)
            .map_or("none", |(_, _, v)| *v)
    };
    FieldItem {
        kind: kind.map(str::to_string),
        name: name.to_string(),
        ty,
        class,
        storage,
        decl,
        optional,
        default,
        range,
        one_line: flags & OL != 0,
        ascii: flags & AS != 0,
        shape,
        index: look(&INDEX),
        coerce: look(&COERCE),
        retired: false,
    }
}

/// The core schema of version 1, built once: kinds, fields and edges from the transcription of [F08 §9], statuses
/// from the rule tables `statuses` and `status-lattice`.
pub fn core() -> &'static Core {
    static CORE: OnceLock<Core> = OnceLock::new();
    CORE.get_or_init(|| {
        let mut kinds = Vec::new();
        for (i, k) in KINDS.iter().enumerate() {
            kinds.push(KindItem {
                name: k.0.to_string(),
                id: i as u8 + 1,
                uid: k.1,
                root_variant: k.2,
                existence_policy: k.3,
                title_derived: k.4,
                immutable_fields: k.5,
                has_done: k.6,
                done_derived: k.7,
                retired: false,
            });
        }
        let mut fields = Vec::new();
        for (i, row) in COMMON.iter().enumerate() {
            fields.push(build_field(None, i as u16 + 1, row));
        }
        for (k, rows) in KIND_FIELDS.iter().enumerate() {
            for (i, row) in rows.iter().enumerate() {
                fields.push(build_field(Some(KINDS[k].0), i as u16 + 20, row));
            }
        }
        let mut values = Vec::new();
        for (field, names) in HEADER_ENUMS {
            for (rank, n) in names.iter().enumerate() {
                values.push(EnumItem {
                    kind: None,
                    field: field.to_string(),
                    name: n.to_string(),
                    rank: rank as u16,
                    side: false,
                    done: false,
                    covers: Vec::new(),
                    retired: false,
                });
            }
        }
        for (kind, field, names) in FIELD_ENUMS {
            for (rank, n) in names.iter().enumerate() {
                let (name, side) = n.strip_prefix('!').map_or((*n, false), |x| (x, true));
                values.push(EnumItem {
                    kind: Some(kind.to_string()),
                    field: field.to_string(),
                    name: name.to_string(),
                    rank: rank as u16,
                    side,
                    done: false,
                    covers: Vec::new(),
                    retired: false,
                });
            }
        }
        // Statuses: the order of `statuses` rows per kind is the value order of [F08 §9.5]; `side` and `covers` come
        // from `status-lattice`, `done` from `statuses`.
        let r = rules();
        let sl = r.table("status-lattice");
        let mut rank: BTreeMap<String, u16> = BTreeMap::new();
        for st in &r.table("statuses").rows {
            let kind = st.tok("kind").to_string();
            let lat = sl
                .row(st.tok("lattice"))
                .expect("statuses rows name their lattice rows (checked at load)");
            let n = rank.entry(kind.clone()).or_insert(0);
            let covers = lat
                .toks("covers")
                .into_iter()
                .filter(|c| *c != "-")
                .map(str::to_string)
                .collect();
            values.push(EnumItem {
                kind: Some(kind),
                field: "status".into(),
                name: st.tok("status").to_string(),
                rank: *n,
                side: lat.tok("side") == "yes",
                done: st.tok("done") == "yes",
                covers,
                retired: false,
            });
            *n += 1;
        }
        let ends = |e: Option<&[&str]>| {
            e.map_or(Ends::Any, |ks| {
                Ends::Kinds(ks.iter().map(|k| k.to_string()).collect())
            })
        };
        let mut edges = Vec::new();
        for (i, e) in EDGES.iter().enumerate() {
            edges.push(EdgeItem {
                name: e.0.to_string(),
                id: i as u8 + 1,
                class: e.1,
                src: ends(e.2),
                dst: ends(e.3),
                symmetric: e.4,
                same_kind: e.5,
                acyclic: e.6,
                max_depth: e.7,
                card: e.8,
                on_dst: e.9,
                on_src: e.10,
                props: e.11,
                lq_name: e.12.to_string(),
                reverse: e.13.iter().map(|s| s.to_string()).collect(),
                reading: e.14.to_string(),
                retired: false,
            });
        }
        let mut field_ix: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for (i, f) in fields.iter().enumerate() {
            field_ix
                .entry(f.kind.clone().unwrap_or_default())
                .or_default()
                .insert(f.name.clone(), i);
        }
        let mut value_ix: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
        for (i, v) in values.iter().enumerate() {
            value_ix
                .entry(v.kind.clone().unwrap_or_else(|| "*".into()))
                .or_default()
                .entry(v.field.clone())
                .or_default()
                .push(i);
        }
        for per_kind in value_ix.values_mut() {
            for ix in per_kind.values_mut() {
                ix.sort_by(|a, b| {
                    (values[*a].rank, &values[*a].name).cmp(&(values[*b].rank, &values[*b].name))
                });
            }
        }
        Core {
            kinds,
            fields,
            values,
            edges,
            default_status: DEFAULT_STATUS
                .iter()
                .map(|(k, s)| (k.to_string(), s.to_string()))
                .collect(),
            field_ix,
            value_ix,
        }
    })
}

impl Core {
    /// The core field `name` of `kind`: a kind row first, then a common row.
    pub fn field(&self, kind: &str, name: &str) -> Option<&FieldItem> {
        self.field_ix
            .get(kind)
            .and_then(|m| m.get(name))
            .or_else(|| self.field_ix.get("").and_then(|m| m.get(name)))
            .map(|i| &self.fields[*i])
    }

    /// The core values of `field` for `kind`: the kind's rows, then the common rows, each in rank order.
    fn values_of(&self, kind: &str, field: &str) -> impl Iterator<Item = &EnumItem> {
        let own = self.value_ix.get(kind).and_then(|m| m.get(field));
        let common = self.value_ix.get("*").and_then(|m| m.get(field));
        own.into_iter()
            .chain(common)
            .flatten()
            .map(|i| &self.values[*i])
    }
}

/// Whether a field item's `default` matches its type ([API §9.8] E103).
fn default_fits(ty: Ty, v: &Value) -> bool {
    matches!(
        (ty, v),
        (Ty::Bool, Value::Bool(_))
            | (Ty::Int, Value::Int(_))
            | (Ty::Counter, Value::Counter(_))
            | (Ty::F64, Value::F64(_))
            | (Ty::Enum, Value::Enum(_))
            | (Ty::Text | Ty::Sym, Value::Text(_))
            | (Ty::Set(_), Value::Set(_))
            | (Ty::Ref, Value::Ref(_))
            | (Ty::Commit, Value::Commit(_))
            | (Ty::Path, Value::Path(_))
            | (Ty::Oid, Value::Oid(_))
            | (Ty::PathMove, Value::PathMove(_))
    )
}

/// The effective schema of a view: the core and the view's items ([F08 §8.1]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schema {
    /// The view's project items by key.
    pub items: BTreeMap<ItemKey, Item>,
}

impl Schema {
    /// The kind with this name (core first, then project), if not retired.
    pub fn kind(&self, name: &str) -> Option<&KindItem> {
        core().kinds.iter().find(|k| k.name == name).or_else(|| {
            match self.items.get(&ItemKey::Kind(name.to_string())) {
                Some(Item::Kind(k)) if !k.retired => Some(k),
                _ => None,
            }
        })
    }

    /// Every kind name.
    pub fn kind_names(&self) -> Vec<String> {
        let mut v: Vec<String> = core().kinds.iter().map(|k| k.name.clone()).collect();
        for i in self.items.values() {
            if let Item::Kind(k) = i
                && !k.retired
            {
                v.push(k.name.clone());
            }
        }
        v
    }

    /// The field `name` of kind `kind`: a kind row first, then a common row ([F08 §9.2]–§9.3), core before project.
    pub fn field(&self, kind: &str, name: &str) -> Option<&FieldItem> {
        core().field(kind, name).or_else(|| {
            if self.items.is_empty() {
                return None;
            }
            match self
                .items
                .get(&ItemKey::Field(kind.to_string(), name.to_string()))
                .or_else(|| {
                    self.items
                        .get(&ItemKey::Field("*".into(), name.to_string()))
                }) {
                Some(Item::Field(f)) if !f.retired => Some(f),
                _ => None,
            }
        })
    }

    /// The fields of a kind in declaration order (common rows first, ties by name).
    pub fn fields_of(&self, kind: &str) -> Vec<&FieldItem> {
        let mut v: Vec<&FieldItem> = core()
            .fields
            .iter()
            .filter(|f| f.kind.is_none() || f.kind.as_deref() == Some(kind))
            .collect();
        for i in self.items.values() {
            if let Item::Field(f) = i
                && !f.retired
                && (f.kind.is_none() || f.kind.as_deref() == Some(kind))
            {
                v.push(f);
            }
        }
        v.sort_by(|a, b| (a.decl, &a.name).cmp(&(b.decl, &b.name)));
        v
    }

    /// The enumeration values of `field` for `kind`, in rank order: kind rows and common rows, core then project.
    pub fn values(&self, kind: &str, field: &str) -> Vec<&EnumItem> {
        let mut v: Vec<&EnumItem> = core().values_of(kind, field).collect();
        for i in self.items.values() {
            if let Item::Enum(e) = i
                && !e.retired
                && e.field == field
                && (e.kind.is_none() || e.kind.as_deref() == Some(kind))
            {
                v.push(e);
            }
        }
        v.sort_by(|a, b| (a.rank, &a.name).cmp(&(b.rank, &b.name)));
        v
    }

    /// The enumeration value `value` of `field` for `kind`.
    pub fn value(&self, kind: &str, field: &str, value: &str) -> Option<&EnumItem> {
        core()
            .values_of(kind, field)
            .find(|e| e.name == value)
            .or_else(|| {
                if self.items.is_empty() {
                    return None;
                }
                [kind, "*"].into_iter().find_map(|k| {
                    match self.items.get(&ItemKey::Enum(
                        k.to_string(),
                        field.to_string(),
                        value.to_string(),
                    )) {
                        Some(Item::Enum(e)) if !e.retired => Some(e),
                        _ => None,
                    }
                })
            })
    }

    /// The initial status (default) of a kind: [F08 §9.1]'s column for a core kind; for a project kind its first
    /// status, which [F08 §8.5.1] makes the default of its `status` field. A `Create` without a status starts in it,
    /// and the canonical form holds it as absent ([F07 §6.3]).
    // spec: [F08 §9.1]
    // spec: [F08 §8.5.1]
    pub fn initial_status(&self, kind: &str) -> Option<String> {
        if let Some(s) = core().default_status.get(kind) {
            return Some(s.clone());
        }
        self.values(kind, "status").first().map(|e| e.name.clone())
    }

    /// The default of a field for a kind, where it has one; `status` defaults to the kind's initial status.
    pub fn default_of(&self, kind: &str, field: &str) -> Option<Value> {
        if field == "status" {
            return self.initial_status(kind).map(Value::Enum);
        }
        self.field(kind, field)?.default.clone()
    }

    /// The edge kind with this stored name.
    pub fn edge(&self, name: &str) -> Option<&EdgeItem> {
        core().edges.iter().find(|e| e.name == name).or_else(|| {
            match self.items.get(&ItemKey::Edge(name.to_string())) {
                Some(Item::Edge(e)) if !e.retired => Some(e),
                _ => None,
            }
        })
    }

    /// Every edge kind.
    pub fn edges(&self) -> Vec<&EdgeItem> {
        let mut v: Vec<&EdgeItem> = core().edges.iter().collect();
        for i in self.items.values() {
            if let Item::Edge(e) = i
                && !e.retired
            {
                v.push(e);
            }
        }
        v
    }

    /// The value of a policy row the view holds as an item ([F08 §8.5.6]); `None` for the row's default.
    pub fn policy(&self, name: &str) -> Option<&str> {
        match self.items.get(&ItemKey::Policy(name.to_string())) {
            Some(Item::Policy(p)) => p.value.as_deref(),
            _ => None,
        }
    }

    /// The policy items of the view, by name.
    pub fn policies(&self) -> impl Iterator<Item = &PolicyItem> {
        self.items
            .range(ItemKey::Policy(String::new())..)
            .filter_map(|(_, i)| match i {
                Item::Policy(p) => Some(p),
                _ => None,
            })
    }

    /// A named query of the view.
    pub fn query(&self, name: &str) -> Option<&QueryItem> {
        match self.items.get(&ItemKey::Query(name.to_string())) {
            Some(Item::Query(q)) => Some(q),
            _ => None,
        }
    }

    /// Checks a weakening item against [F08 §8.2]'s names and §8.5's rules for project items, on this effective
    /// schema ([API §9.8]): a name outside §8.2's grammar, and a policy row that names no row of [CFG §10.13] or whose
    /// value does not parse as the row's type, are `bad_value` (exit 2); a target the view lacks is E105 (an unknown
    /// kind, also in a `KindSet`), E101 (an enumeration value of a field the kind does not have) or E103 (an
    /// enumeration value of a field that is not an enumeration, a `default` that does not match the field's type), exit
    /// 2 (spec sync 2b); a name that is not unique by §8.2's uniqueness column and a member that §8.5 fixes for project
    /// items are E405 with the rule `schema weakening` (exit 6). Whether the view holds the key already, or it is a
    /// core key, is the caller's check.
    // spec: [F08 §8.2]
    // spec: [F08 §8.5]
    pub fn check_item(&self, it: &Item) -> Result<(), Refusal> {
        let bad = |what: String| Refusal::bad_value("shape", what);
        let weak = |what: String| Refusal::lq("E405", format!("schema weakening: {what}"));
        let kind_exists = |k: &str| k == "*" || self.kind(k).is_some();
        match it {
            Item::Kind(k) => {
                if !names::lower_ok(&k.name) {
                    return Err(bad(format!(
                        "kind name {} is outside [a-z][a-z0-9_]*, 1-64 bytes",
                        k.name
                    )));
                }
                if k.name == "deleted" || core().kinds.iter().any(|c| c.name == k.name) {
                    return Err(weak(format!("the kind name {} is taken", k.name)));
                }
                if k.uid != UidDerivation::Random || k.root_variant || k.title_derived {
                    return Err(weak(format!(
                        "project kind {} must derive random uids, with no root variant or derived title",
                        k.name
                    )));
                }
                if !["delete-wins", "resurrect", "none"].contains(&k.existence_policy) {
                    return Err(weak(format!(
                        "{} is not an existence policy",
                        k.existence_policy
                    )));
                }
            }
            Item::Field(f) => {
                if !names::lower_ok(&f.name) {
                    return Err(bad(format!(
                        "field name {} is outside [a-z][a-z0-9_]*, 1-64 bytes",
                        f.name
                    )));
                }
                let k = f.kind.as_deref().unwrap_or("*");
                if !kind_exists(k) {
                    return Err(Refusal::lq(
                        "E105",
                        format!("the field {}.{} names no kind {k}", k, f.name),
                    ));
                }
                if let Some(v) = &f.default
                    && !default_fits(f.ty, v)
                {
                    return Err(Refusal::lq(
                        "E103",
                        format!("the default of {}.{} does not match its type", k, f.name),
                    ));
                }
                if names::LQ_BUILTINS.contains(&f.name.as_str()) {
                    return Err(weak(format!(
                        "{} is a built-in node property of LQ",
                        f.name
                    )));
                }
                let clash = if k == "*" {
                    core().fields.iter().any(|c| c.name == f.name)
                        || self
                            .items
                            .values()
                            .any(|i| matches!(i, Item::Field(x) if x.name == f.name && !x.retired))
                } else {
                    core().field(k, &f.name).is_some()
                        || self
                            .items
                            .contains_key(&ItemKey::Field("*".into(), f.name.clone()))
                };
                if clash {
                    return Err(weak(format!("the field name {}.{} is taken", k, f.name)));
                }
                if f.storage != Storage::Field || f.ty == Ty::Body {
                    return Err(weak(format!(
                        "project field {} must be stored in the field block with a value type",
                        f.name
                    )));
                }
                if let Some((lo, hi)) = f.range
                    && (lo > hi || f.ty != Ty::Int)
                {
                    return Err(weak(format!("the range of {} is not an int range", f.name)));
                }
            }
            Item::Enum(e) => {
                if !names::enum_ok(&e.name) {
                    return Err(bad(format!(
                        "value {} is outside [A-Za-z0-9_][A-Za-z0-9_-]*, 1-64 bytes",
                        e.name
                    )));
                }
                let k = e.kind.as_deref().unwrap_or("*");
                if !kind_exists(k) {
                    return Err(Refusal::lq(
                        "E105",
                        format!("the value {k}.{}.{} names no kind {k}", e.field, e.name),
                    ));
                }
                let target = if k == "*" {
                    core().field("", &e.field)
                } else {
                    self.field(k, &e.field)
                };
                match target {
                    None => {
                        return Err(Refusal::lq("E101", format!("{k} has no field {}", e.field)));
                    }
                    Some(t) if t.ty != Ty::Enum => {
                        return Err(Refusal::lq(
                            "E103",
                            format!("{k}.{} is not an enumeration field", e.field),
                        ));
                    }
                    Some(_) => {}
                }
                let taken = if k == "*" {
                    core()
                        .values
                        .iter()
                        .any(|v| v.field == e.field && v.name == e.name)
                } else {
                    self.value(k, &e.field, &e.name).is_some()
                };
                if taken {
                    return Err(weak(format!(
                        "the value {k}.{}.{} is taken",
                        e.field, e.name
                    )));
                }
                for c in &e.covers {
                    if c == &e.name || self.value(k, &e.field, c).is_none() {
                        return Err(weak(format!(
                            "{} covers {c}, which is not a value of {k}.{}",
                            e.name, e.field
                        )));
                    }
                }
            }
            Item::Edge(e) => {
                if !names::lower_ok(&e.name) {
                    return Err(bad(format!(
                        "edge name {} is outside [a-z][a-z0-9_]*, 1-64 bytes",
                        e.name
                    )));
                }
                if self.edge(&e.name).is_some() {
                    return Err(weak(format!("the edge kind {} is taken", e.name)));
                }
                let mut upper: Vec<String> = vec![e.lq_name.clone()];
                upper.extend(e.reverse.iter().cloned());
                for n in &upper {
                    if !names::lq_ok(n) {
                        return Err(bad(format!(
                            "LQ name {n} is outside [A-Z][A-Z0-9_]*, 1-64 bytes"
                        )));
                    }
                }
                let mut taken: Vec<String> = Vec::new();
                for x in self.edges() {
                    taken.push(x.lq_name.to_ascii_uppercase());
                    taken.extend(x.reverse.iter().map(|r| r.to_ascii_uppercase()));
                    taken.push(x.name.to_ascii_uppercase());
                }
                taken.push(e.name.to_ascii_uppercase());
                for (i, n) in upper.iter().enumerate() {
                    let u = n.to_ascii_uppercase();
                    let dup_self = upper[..i].iter().any(|p| p.eq_ignore_ascii_case(n));
                    let own_stored = i == 0 && u == e.name.to_ascii_uppercase();
                    if dup_self || (taken.contains(&u) && !own_stored) {
                        return Err(weak(format!("the LQ name {n} is taken")));
                    }
                }
                let reading_ok = !e.reading.is_empty()
                    && e.reading.len() <= 200
                    && e.reading.bytes().all(|b| (0x20..=0x7E).contains(&b))
                    && e.reading.matches("{a}").count() == 1
                    && e.reading.matches("{b}").count() == 1;
                if !reading_ok {
                    return Err(bad(format!(
                        "the reading of {} is not an ASCII line with {{a}} and {{b}} once each",
                        e.name
                    )));
                }
                if e.class != EdgeClass::Historical
                    || e.on_dst != "tombstone"
                    || e.on_src != "retain"
                    || e.props != Props::None
                {
                    return Err(weak(format!(
                        "project edge kind {} must be historical, tombstone/retain, without properties",
                        e.name
                    )));
                }
                for ends in [&e.src, &e.dst] {
                    if let Ends::Kinds(ks) = ends
                        && let Some(k) = ks.iter().find(|k| self.kind(k).is_none())
                    {
                        return Err(Refusal::lq("E105", format!("{} names no kind {k}", e.name)));
                    }
                }
            }
            Item::Query(q) => {
                return Err(bad(format!(
                    "the named query {} is defined through Tx define_query",
                    q.name
                )));
            }
            // A policy row names a row instance of [CFG §10.13] and a value of the row's type ([F08 §8.5.6]).
            Item::Policy(p) => {
                let ok = match &p.value {
                    Some(v) => crate::policy::canonical_policy(&p.name, v).is_some(),
                    None => crate::policy::default_policy(&p.name).is_some(),
                };
                if !ok {
                    return Err(Refusal::bad_value(
                        "shape",
                        format!(
                            "{} = {} is no policy row of [CFG §10.13] with a value of its type",
                            p.name,
                            p.value.as_deref().unwrap_or("null")
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Whether a key names a core item ([F08 §8.1] "Core items are fixed").
    pub fn is_core(key: &ItemKey) -> bool {
        let c = core();
        let kind_ok = |k: &str| k == "*" || c.kinds.iter().any(|x| x.name == k);
        match key {
            ItemKey::Kind(k) => c.kinds.iter().any(|x| &x.name == k),
            ItemKey::Field(k, f) => {
                kind_ok(k)
                    && c.fields.iter().any(|x| {
                        &x.name == f && (x.kind.is_none() || x.kind.as_deref() == Some(k.as_str()))
                    })
            }
            ItemKey::Enum(k, f, v) => {
                kind_ok(k)
                    && c.values.iter().any(|x| {
                        &x.field == f
                            && &x.name == v
                            && (x.kind.is_none() || x.kind.as_deref() == Some(k.as_str()))
                    })
            }
            ItemKey::Edge(e) => c.edges.iter().any(|x| &x.name == e),
            ItemKey::Query(_) | ItemKey::Policy(_) => false,
        }
    }
}

/// The name grammars of [F08 §8.2].
pub mod names {
    /// LQ's built-in node properties ([50 §2.5]), which no field name may take ([F08 §8.2] uniqueness column).
    pub const LQ_BUILTINS: [&str; 33] = [
        "id",
        "uid",
        "kind",
        "rev",
        "created",
        "updated",
        "created_at",
        "updated_at",
        "created_by",
        "created_role",
        "updated_by",
        "done",
        "unfinished",
        "container",
        "unblocked",
        "blocked",
        "open_blockers",
        "is_blocker",
        "children_total",
        "children_done",
        "ready_to_close",
        "suspect",
        "conflicted",
        "answered",
        "has_dangling",
        "depth",
        "topo",
        "ready",
        "claimed",
        "lease",
        "settled_elsewhere",
        "deleted_elsewhere",
        "state",
    ];

    fn grammar(s: &str, first: fn(u8) -> bool, rest: fn(u8) -> bool) -> bool {
        let b = s.as_bytes();
        !b.is_empty() && b.len() <= 64 && first(b[0]) && b[1..].iter().all(|c| rest(*c))
    }

    /// A kind, field or stored edge name: `[a-z][a-z0-9_]*`, 1–64 bytes.
    pub fn lower_ok(s: &str) -> bool {
        grammar(
            s,
            |c| c.is_ascii_lowercase(),
            |c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_',
        )
    }

    /// An enumeration value: `[A-Za-z0-9_][A-Za-z0-9_-]*`, 1–64 bytes.
    pub fn enum_ok(s: &str) -> bool {
        grammar(
            s,
            |c| c.is_ascii_alphanumeric() || c == b'_',
            |c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-',
        )
    }

    /// An `lq_name` or reverse name: `[A-Z][A-Z0-9_]*`, 1–64 bytes.
    pub fn lq_ok(s: &str) -> bool {
        grammar(
            s,
            |c| c.is_ascii_uppercase(),
            |c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_',
        )
    }
}

/// The type name of [F08 §5.1] of a field type, and of its element type for a set.
pub fn type_names(t: Ty) -> (&'static str, Option<&'static str>) {
    let elem = |e: Elem| match e {
        Elem::Sym => "sym",
        Elem::Path => "path",
        Elem::PathMove => "pathmove",
        Elem::Int => "int",
    };
    match t {
        Ty::Bool => ("bool", None),
        Ty::Int => ("int", None),
        Ty::Counter => ("counter", None),
        Ty::F64 => ("f64", None),
        Ty::Enum => ("enum", None),
        Ty::Text => ("text", None),
        Ty::Sym => ("sym", None),
        Ty::Set(e) => ("set", Some(elem(e))),
        Ty::Ref => ("ref", None),
        Ty::Commit => ("commitref", None),
        Ty::Path => ("path", None),
        Ty::Oid => ("oid", None),
        Ty::PathMove => ("pathmove", None),
        Ty::Body => ("body", None),
    }
}

/// A value in the argument form of [API §5.2], node ids as uids ([API §7.3] `args′`).
pub fn value_cj(v: &Value, uid: &dyn Fn(Nid) -> String) -> Cj {
    match v {
        Value::Bool(b) => Cj::Bool(*b),
        Value::Int(i) | Value::Counter(i) => Cj::Int(*i),
        Value::F64(x) => Cj::F64(x.get()),
        Value::Enum(s) | Value::Text(s) => Cj::Str(s.clone()),
        Value::Set(e) => Cj::Arr(e.iter().map(|x| value_cj(x, uid)).collect()),
        Value::Ref(n) => Cj::Str(uid(*n)),
        Value::Commit(c) => Cj::Str(format!("c{}", crate::value::hex(c))),
        Value::Path(p) => Cj::Str(format!("{}:{}", p.root, p.text)),
        Value::Oid(o) => Cj::Str(format!(
            "{}:{}",
            o.algo.name(),
            crate::value::hex(&o.digest)
        )),
        Value::PathMove(m) => Cj::Obj(vec![
            ("hlc".into(), Cj::Str(m.hlc.to_string())),
            (
                "class".into(),
                Cj::Str(
                    match m.class {
                        crate::value::MoveClass::Explicit => "explicit",
                        crate::value::MoveClass::Confirmed => "confirmed",
                        crate::value::MoveClass::Committed => "committed",
                        crate::value::MoveClass::Observed => "observed",
                    }
                    .into(),
                ),
            ),
            (
                "from".into(),
                Cj::Str(format!("{}:{}", m.from.root, m.from.text)),
            ),
            ("to".into(), Cj::Str(format!("{}:{}", m.to.root, m.to.text))),
            (
                "git".into(),
                m.git.as_ref().map_or(Cj::Null, |o| {
                    Cj::Str(format!(
                        "{}:{}",
                        o.algo.name(),
                        crate::value::hex(&o.digest)
                    ))
                }),
            ),
        ]),
    }
}

/// An item object as [API §9.8] writes it: `{"item":…}` followed by the members of [F08 §8.5] by name, in that
/// table's order — symbols as strings, enumeration bytes by name, flag bytes as the names of their set bits without a
/// bit that announces a member (`has_default`, `has_range`), `KindSet`s as `{"any":…,"kinds":[…]}` — and without the
/// store-allocated members (`kind_id`, `edge_id`, a value's `value`, a field's `decl`), the counts that the arrays
/// carry (`n_covers`, `n_reverse`) and the item flags of a weakening item.
// spec: [API §9.8]
pub fn item_cj(it: &Item, uid: &dyn Fn(Nid) -> String) -> Cj {
    let s = |x: &str| Cj::Str(x.to_string());
    let flags =
        |f: &[(&str, bool)]| Cj::Arr(f.iter().filter(|(_, on)| *on).map(|(n, _)| s(n)).collect());
    let ends = |e: &Ends| match e {
        Ends::Any => Cj::Obj(vec![
            ("any".into(), Cj::Bool(true)),
            ("kinds".into(), Cj::Arr(vec![])),
        ]),
        Ends::Kinds(k) => {
            let mut k = k.clone();
            k.sort();
            Cj::Obj(vec![
                ("any".into(), Cj::Bool(false)),
                ("kinds".into(), Cj::Arr(k.iter().map(|x| s(x)).collect())),
            ])
        }
    };
    let star = |k: &Option<String>| s(k.as_deref().unwrap_or("*"));
    let mut m: Vec<(String, Cj)> = Vec::new();
    match it {
        // [API §9.8]: `{"item":"policy","name":<row>,"value":<string or null>}`.
        Item::Policy(p) => {
            m.push(("item".into(), s("policy")));
            m.push(("name".into(), s(&p.name)));
            m.push(("value".into(), p.value.as_deref().map_or(Cj::Null, s)));
        }
        Item::Kind(k) => {
            m.push(("item".into(), s("kind")));
            m.push(("name".into(), s(&k.name)));
            m.push((
                "uid_derivation".into(),
                s(match k.uid {
                    UidDerivation::Random => "random",
                    UidDerivation::FileKey => "file-key",
                }),
            ));
            m.push((
                "root_variant".into(),
                s(if k.root_variant { "root-key" } else { "none" }),
            ));
            m.push(("existence_policy".into(), s(k.existence_policy)));
            m.push((
                "kflags".into(),
                flags(&[
                    ("title_derived", k.title_derived),
                    ("immutable_fields", k.immutable_fields),
                    ("has_done", k.has_done),
                    ("done_derived", k.done_derived),
                ]),
            ));
        }
        Item::Field(f) => {
            let (ty, elem) = type_names(f.ty);
            m.push(("item".into(), s("field")));
            m.push(("kind".into(), star(&f.kind)));
            m.push(("name".into(), s(&f.name)));
            m.push(("type".into(), s(ty)));
            m.push(("elem".into(), elem.map_or(Cj::Null, s)));
            m.push(("class".into(), s(f.class)));
            m.push((
                "storage".into(),
                s(match f.storage {
                    Storage::Header => "header",
                    Storage::Flag => "flag",
                    Storage::Cold => "cold",
                    Storage::Field => "field",
                    Storage::Title => "title",
                    Storage::Body => "body",
                }),
            ));
            m.push(("optional".into(), Cj::Bool(f.optional)));
            m.push(("index".into(), s(f.index)));
            m.push(("coerce".into(), s(f.coerce)));
            m.push((
                "cflags".into(),
                flags(&[("one_line", f.one_line), ("ascii", f.ascii)]),
            ));
            if let Some(d) = &f.default {
                m.push(("default".into(), value_cj(d, uid)));
            }
            if let Some((lo, hi)) = f.range {
                m.push(("range_min".into(), Cj::Int(lo)));
                m.push(("range_max".into(), Cj::Int(hi)));
            }
        }
        Item::Enum(e) => {
            m.push(("item".into(), s("enum")));
            m.push(("kind".into(), star(&e.kind)));
            m.push(("field".into(), s(&e.field)));
            m.push(("name".into(), s(&e.name)));
            m.push(("sort_rank".into(), Cj::Int(i64::from(e.rank))));
            m.push((
                "eflags".into(),
                flags(&[("side", e.side), ("done", e.done)]),
            ));
            m.push((
                "covers".into(),
                Cj::Arr(e.covers.iter().map(|c| s(c)).collect()),
            ));
        }
        Item::Edge(e) => {
            m.push(("item".into(), s("edge")));
            m.push(("name".into(), s(&e.name)));
            m.push((
                "eclass".into(),
                s(match e.class {
                    EdgeClass::Structural => "structural",
                    EdgeClass::Historical => "historical",
                }),
            ));
            m.push(("on_dst".into(), s(e.on_dst)));
            m.push(("on_src".into(), s(e.on_src)));
            m.push((
                "acyclic".into(),
                s(match e.acyclic {
                    Acyclic::None => "none",
                    Acyclic::Forest => "forest",
                    Acyclic::Precedence => "precedence",
                    Acyclic::Dag => "dag",
                    Acyclic::ByConstruction => "by-construction",
                }),
            ));
            m.push((
                "card".into(),
                s(match e.card {
                    Card::Many => "many",
                    Card::Max1PerSrc => "max-1-per-src",
                    Card::Max1ActivePerDst => "max-1-active-per-dst",
                    Card::Chain1 => "chain-1",
                    Card::Typical1 => "typical-1",
                    Card::AnchorsMin1 => "anchors-min-1",
                }),
            ));
            m.push(("max_depth".into(), Cj::Int(i64::from(e.max_depth))));
            m.push((
                "uid_derivation".into(),
                s(if e.name == "at" { "anchor-key" } else { "none" }),
            ));
            m.push((
                "props".into(),
                s(match e.props {
                    Props::None => "none",
                    Props::Pinned => "pinned",
                    Props::Flagged => "flagged",
                    Props::Anchor => "anchor",
                }),
            ));
            m.push((
                "eflags".into(),
                flags(&[("symmetric", e.symmetric), ("same_kind", e.same_kind)]),
            ));
            m.push(("lq_name".into(), s(&e.lq_name)));
            m.push(("src_kinds".into(), ends(&e.src)));
            m.push(("dst_kinds".into(), ends(&e.dst)));
            m.push((
                "reverse_names".into(),
                Cj::Arr(e.reverse.iter().map(|r| s(r)).collect()),
            ));
            m.push(("reading".into(), s(&e.reading)));
        }
        Item::Query(q) => {
            m.push(("item".into(), s("query")));
            m.push(("name".into(), s(&q.name)));
            m.push(("lq_version".into(), Cj::Int(i64::from(q.lq_version))));
            m.push(("params".into(), s(&q.params)));
            m.push(("shape".into(), s(&q.shape)));
            m.push(("budget".into(), s(&q.budget)));
            m.push(("text".into(), s(&q.text)));
        }
    }
    Cj::Obj(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default status of every core kind is one of its `initial = yes` statuses, and it is the only one for
    /// every kind but `artifact` ([F08 §9.1]; [RULES/status-machines] `statuses`).
    #[test]
    fn default_statuses_are_initial_statuses() {
        let s = Schema::default();
        for k in &core().kinds {
            let st = crate::status::statuses(&k.name);
            let initial: Vec<&str> = st.iter().filter(|x| x.1).map(|x| x.0).collect();
            let d = s.initial_status(&k.name).unwrap();
            assert!(initial.contains(&d.as_str()), "{}: {d}", k.name);
            if k.name != "artifact" {
                assert_eq!(initial, vec![d.as_str()], "{}", k.name);
            }
        }
        assert_eq!(s.initial_status("artifact").as_deref(), Some("present"));
        assert!(s.value("artifact", "status", "planned").is_some());
    }

    /// [API §19] example 19's item object is the canonical JSON of the model's item.
    #[test]
    fn the_schema_example_item_is_its_canonical_json() {
        let it = Item::Field(FieldItem {
            kind: Some("task".into()),
            name: "story_points".into(),
            ty: Ty::Int,
            class: "scalar",
            storage: Storage::Field,
            decl: 0,
            optional: true,
            default: None,
            range: Some((0, 100)),
            one_line: false,
            ascii: false,
            shape: Shape::Plain,
            index: "none",
            coerce: "none",
            retired: false,
        });
        assert_eq!(
            item_cj(&it, &|n| n.to_string()).text(),
            "{\"item\":\"field\",\"kind\":\"task\",\"name\":\"story_points\",\"type\":\"int\",\"elem\":null,\
             \"class\":\"scalar\",\"storage\":\"field\",\"optional\":true,\"index\":\"none\",\"coerce\":\"none\",\
             \"cflags\":[],\"range_min\":0,\"range_max\":100}"
        );
    }

    #[test]
    fn project_names_follow_the_grammars_and_never_shadow() {
        let s = Schema::default();
        let field = |kind: Option<&str>, name: &str, ty: Ty| {
            Item::Field(FieldItem {
                kind: kind.map(str::to_string),
                name: name.into(),
                ty,
                class: "scalar",
                storage: Storage::Field,
                decl: 0,
                optional: true,
                default: None,
                range: None,
                one_line: false,
                ascii: false,
                shape: Shape::Plain,
                index: "none",
                coerce: "none",
                retired: false,
            })
        };
        let value = |kind: &str, f: &str, name: &str| {
            Item::Enum(EnumItem {
                kind: Some(kind.into()),
                field: f.into(),
                name: name.into(),
                rank: 9,
                side: false,
                done: false,
                covers: vec![],
                retired: false,
            })
        };
        let code = |it: &Item| s.check_item(it).err().map(|e| e.code);
        assert_eq!(code(&field(Some("task"), "effort", Ty::Int)), None);
        assert_eq!(
            code(&field(Some("task"), "Effort", Ty::Int)).as_deref(),
            Some("bad_value")
        );
        assert_eq!(
            code(&field(Some("task"), "ready", Ty::Bool)).as_deref(),
            Some("E405"),
            "LQ built-in"
        );
        assert_eq!(
            code(&field(None, "severity", Ty::Text)).as_deref(),
            Some("E405"),
            "a core kind's field"
        );
        assert_eq!(
            code(&field(Some("task"), "title", Ty::Text)).as_deref(),
            Some("E405"),
            "a common field"
        );
        assert_eq!(
            code(&value("task", "priority", "-x")).as_deref(),
            Some("bad_value")
        );
        // Targets the view lacks ([API §9.8], spec sync 2b): E103, E101, E105, exit 2.
        assert_eq!(
            code(&value("task", "title", "x")).as_deref(),
            Some("E103"),
            "title is text"
        );
        assert_eq!(
            code(&value("task", "nosuch", "x")).as_deref(),
            Some("E101"),
            "no such field"
        );
        assert_eq!(
            code(&value("nokind", "priority", "x")).as_deref(),
            Some("E105"),
            "no such kind"
        );
        assert_eq!(
            code(&field(Some("nokind"), "effort", Ty::Int)).as_deref(),
            Some("E105")
        );
        let mut mistyped = field(Some("task"), "effort", Ty::Int);
        if let Item::Field(f) = &mut mistyped {
            f.default = Some(Value::Text("x".into()));
        }
        assert_eq!(
            code(&mistyped).as_deref(),
            Some("E103"),
            "a mistyped default"
        );
        // Policy rows ([F08 §8.5.6]): a row instance of [CFG §10.13] with a value of its type.
        let policy = |n: &str, v: &str| {
            Item::Policy(PolicyItem {
                name: n.into(),
                value: Some(v.into()),
            })
        };
        assert_eq!(code(&policy("merge.policy.task", "ours")), None);
        assert_eq!(code(&policy("policy.role.tester.mcp-write", "yes")), None);
        assert_eq!(
            code(&policy("merge.policy.task", "maybe")).as_deref(),
            Some("bad_value")
        );
        assert_eq!(
            code(&policy("policy.nosuch", "yes")).as_deref(),
            Some("bad_value")
        );
        assert_eq!(code(&value("task", "work_kind", "spike")), None);
        assert_eq!(
            code(&value("task", "work_kind", "fix")).as_deref(),
            Some("E405"),
            "a core value"
        );
        assert!(names::lq_ok("DEPENDS_ON") && !names::lq_ok("Depends"));
    }

    #[test]
    fn the_core_has_its_rows() {
        let c = core();
        assert_eq!(c.kinds.len(), 13);
        assert_eq!(c.edges.len(), 25);
        assert_eq!(c.fields.iter().filter(|f| f.kind.is_none()).count(), 18);
        let s = Schema::default();
        assert_eq!(s.initial_status("task").as_deref(), Some("open"));
        assert_eq!(s.initial_status("artifact").as_deref(), Some("present"));
        assert_eq!(s.values("task", "status").len(), 6);
        assert!(s.value("task", "status", "cancelled").unwrap().done);
        assert!(s.value("task", "status", "cancelled").unwrap().side);
        assert_eq!(
            s.value("lane", "status", "merged").unwrap().covers,
            vec!["merge_pending".to_string()]
        );
        assert_eq!(
            s.field("task", "reopen_count").unwrap().default,
            Some(Value::Counter(0))
        );
        assert_eq!(
            s.field("task", "priority").unwrap().default,
            Some(Value::Enum("P2".into()))
        );
        assert!(
            s.field("finding", "failure_scenario")
                .unwrap()
                .default
                .is_none()
        );
        assert!(!s.field("finding", "failure_scenario").unwrap().optional);
    }

    /// Every core field's merge class equals its `field-class` row ([RULES/merge-table] §8: "a model test checks that
    /// every core field's class there equals its row here").
    #[test]
    fn core_field_classes_equal_the_field_class_rows() {
        let s = Schema::default();
        let fc = rules().table("field-class");
        let mut checked = 0;
        for row in &fc.rows {
            let kind = row.tok("kind");
            let field = row.tok("field");
            let class = row.tok("class");
            if matches!(field, "uid" | "kind" | "CREATOR") || class == "derived" || class == "none"
            {
                continue;
            }
            let base = kind.split('/').next().unwrap_or(kind);
            let item = if base == "*" {
                core()
                    .fields
                    .iter()
                    .find(|f| f.kind.is_none() && f.name == field)
            } else {
                s.field(base, field)
            };
            let Some(item) = item else { continue };
            let want = if item.name == "body" && kind == "doc/section" {
                "section-text"
            } else {
                item.class
            };
            if kind == "doc/section" || kind == "*" || kind == base {
                assert_eq!(want, class, "{} {kind}.{field}", row.id);
                checked += 1;
            }
        }
        assert!(checked > 60, "{checked} field rows checked");
    }

    /// Every core edge kind's class equals its `edge-class` row.
    #[test]
    fn core_edge_classes_equal_the_edge_class_rows() {
        let s = Schema::default();
        let ec = rules().table("edge-class");
        for row in &ec.rows {
            let e = s
                .edge(row.tok("edge"))
                .unwrap_or_else(|| panic!("{} names no edge kind", row.id));
            let want = match e.class {
                EdgeClass::Structural => "structural",
                EdgeClass::Historical => "historical",
            };
            assert_eq!(row.tok("edge_class"), want, "{}", row.id);
        }
        assert_eq!(ec.rows.len(), 25);
    }

    /// The `statuses` rows and the SL rows agree kind by kind ([RULES/status-machines] GR-017, first part).
    #[test]
    fn statuses_equal_the_lattice_rows() {
        let r = rules();
        let st: Vec<(String, String)> = r
            .table("statuses")
            .rows
            .iter()
            .map(|x| (x.tok("kind").to_string(), x.tok("status").to_string()))
            .collect();
        let sl: Vec<(String, String)> = r
            .table("status-lattice")
            .rows
            .iter()
            .map(|x| (x.tok("kind").to_string(), x.tok("status").to_string()))
            .collect();
        assert_eq!(st, sl);
    }
}
