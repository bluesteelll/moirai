//! The schema of a view as the binder reads it ([LQ/canonical-ast §5.1] item 1): kinds, fields with their types and F2
//! rows, enumeration values, edge kinds with their F1 rows, and the project's named queries. [`Schema::core`] is the
//! genesis schema of [F08 §9], transcribed row for row; project items ([F08 §8.1]) are added with the `add_*` methods.
//!
//! Names follow [LQ/lexical §9]: kind names, the pseudo-label `DELETED` and edge type names match ASCII
//! case-insensitively; field names and named-query names match exactly.

use std::fmt;

/// The bit of [`KindSet`] that stands for the pseudo-label `DELETED` ([50 §3.6]).
pub const DELETED_BIT: usize = 255;

/// A set of kinds by schema index, with bit 255 for `DELETED`. At most 255 kinds exist: 13 core kinds and the
/// project kinds of ids 64–254 ([F08 §8.3]).
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct KindSet([u64; 4]);

impl KindSet {
    /// The empty set.
    pub const EMPTY: KindSet = KindSet([0; 4]);

    /// The set of the first `n` kinds (every live kind of a schema with `n` kinds).
    pub fn first(n: usize) -> KindSet {
        let mut s = KindSet::EMPTY;
        for i in 0..n.min(DELETED_BIT) {
            s.insert(i);
        }
        s
    }

    /// A set of one kind.
    pub fn one(i: usize) -> KindSet {
        let mut s = KindSet::EMPTY;
        s.insert(i);
        s
    }

    /// Adds a kind.
    pub fn insert(&mut self, i: usize) {
        self.0[i / 64] |= 1 << (i % 64);
    }

    /// Whether the set holds a kind.
    pub fn contains(&self, i: usize) -> bool {
        self.0[i / 64] & (1 << (i % 64)) != 0
    }

    /// The intersection.
    pub fn and(self, o: KindSet) -> KindSet {
        KindSet([
            self.0[0] & o.0[0],
            self.0[1] & o.0[1],
            self.0[2] & o.0[2],
            self.0[3] & o.0[3],
        ])
    }

    /// The union.
    pub fn or(self, o: KindSet) -> KindSet {
        KindSet([
            self.0[0] | o.0[0],
            self.0[1] | o.0[1],
            self.0[2] | o.0[2],
            self.0[3] | o.0[3],
        ])
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }

    /// Whether every member of `self` is in `o`.
    pub fn subset_of(&self, o: KindSet) -> bool {
        self.and(o) == *self
    }

    /// The members in index order.
    pub fn iter(self) -> impl Iterator<Item = usize> {
        (0..256).filter(move |&i| self.contains(i))
    }

    /// The number of members.
    pub fn len(&self) -> usize {
        self.0.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// The set without `DELETED`.
    pub fn live(self) -> KindSet {
        let mut s = self;
        s.0[3] &= !(1 << 63);
        s
    }
}

impl fmt::Debug for KindSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// The stored type of a field ([F08 §5.1]) as LQ sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldTy {
    /// `bool`.
    Bool,
    /// `int`.
    Int,
    /// `counter`: an `int` changed only by increments.
    Counter,
    /// `f64`.
    Float,
    /// `enum`.
    Enum,
    /// `text` or `sym`.
    Text,
    /// `ref`: a node.
    Ref,
    /// `commitref`.
    Commit,
    /// `path`.
    Path,
    /// `oid`.
    Oid,
    /// A set of `sym` values.
    TextSet,
    /// A set of `path` values.
    PathSet,
    /// A set of `pathmove` values.
    PathMoveSet,
    /// The body.
    Body,
}

/// The merge class of a field ([F08 §8.4.1]); the binder uses it for [LQ/errors] E115.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldClass {
    /// `scalar`.
    Scalar,
    /// `owner`.
    Owner,
    /// `authority`.
    Authority,
    /// `status`.
    Status,
    /// `counter`.
    Counter,
    /// `set`.
    Set,
    /// `text`.
    Text,
    /// `section-text`.
    SectionText,
    /// `hierarchy`.
    Hierarchy,
    /// `identity`.
    Identity,
    /// `observation`.
    Observation,
    /// `alias-set`.
    AliasSet,
    /// `glob-set`.
    GlobSet,
    /// `pathmove-set`.
    PathMoveSet,
}

/// `coerce` of a field ([F08 §8.4.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coerce {
    /// No coercion.
    None,
    /// `'P1'`, bare `P1` and `1` denote one value.
    Priority,
    /// A bare integer is a store sequence number; a revision-shaped word or string is a revspec.
    RevisionInteger,
    /// The field holds Unix seconds; an ISO 8601 string compares as a timestamp.
    Timestamp,
}

/// One field row ([F08 §8.5.2]).
#[derive(Clone, Debug)]
pub struct FieldDef {
    /// The kind, or `None` for `*` (every kind).
    pub kind: Option<usize>,
    /// The field name.
    pub name: String,
    /// Its type.
    pub ty: FieldTy,
    /// Its merge class.
    pub class: FieldClass,
    /// F2 `optional`: the field may be absent.
    pub optional: bool,
    /// F2 `coerce`.
    pub coerce: Coerce,
    /// Declaration order.
    pub decl: u16,
}

/// One enumeration value ([F08 §8.5.3]).
#[derive(Clone, Debug)]
pub struct EnumValue {
    /// The kind, or `None` for `*`.
    pub kind: Option<usize>,
    /// The field.
    pub field: String,
    /// The value's declared name.
    pub name: String,
    /// Its rank.
    pub rank: u16,
    /// A side state.
    pub side: bool,
}

/// One kind row ([F08 §8.5.1]).
#[derive(Clone, Debug)]
pub struct KindDef {
    /// The kind name.
    pub name: String,
    /// The kind has the virtual `done` field.
    pub has_done: bool,
    /// Fields are read-only after `Create`.
    pub immutable_fields: bool,
    /// Instances with a `root` field are root nodes (`area`).
    pub root_variant: bool,
}

/// Endpoint kinds of an edge kind: every kind, or a list of kind names ([F08 §8.4.8]).
#[derive(Clone, Debug)]
pub enum Ends {
    /// `any`: every kind, including project kinds added later.
    Any,
    /// The named kinds.
    Kinds(Vec<String>),
}

/// One edge-kind row with its F1 columns ([F08 §8.5.4], §9.6).
#[derive(Clone, Debug)]
pub struct EdgeDef {
    /// The stored snake-case name.
    pub stored: String,
    /// F1 `lq_name`.
    pub lq: String,
    /// F1 `reverse_names`.
    pub reverse: Vec<String>,
    /// F1 `src_kinds`.
    pub src: Ends,
    /// F1 `dst_kinds`.
    pub dst: Ends,
    /// F1 `symmetric`.
    pub symmetric: bool,
    /// Source and destination must have the same kind.
    pub same_kind: bool,
    /// F1 `reading`, with `{a}` and `{b}`.
    pub reading: String,
    /// `props` = `pinned`.
    pub pinned: bool,
    /// `props` = `flagged`.
    pub flagged: bool,
    /// `props` = `anchor` (the `at` kind).
    pub anchor: bool,
}

/// A project named query (F3): its name and its definition text ([F08 §8.5.5]).
#[derive(Clone, Debug)]
pub struct QueryDef {
    /// The query name.
    pub name: String,
    /// The `define_stmt` text.
    pub text: String,
}

/// How a written edge type name resolved ([LQ/canonical-ast §5.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeName {
    /// The edge kind's schema index.
    pub edge: usize,
    /// The name reads in the stored direction (an `lq_name`, a forward synonym or the stored name) rather than the
    /// reverse (a reverse alias).
    pub forward: bool,
}

/// The forward synonyms of grammar version 1 ([LQ/canonical-ast §5.4]): (synonym, `lq_name`).
pub const FORWARD_SYNONYMS: [(&str, &str); 1] = [("SUBTASK_OF", "CHILD_OF")];

/// The effective schema of a view.
#[derive(Clone, Debug)]
pub struct Schema {
    /// Kinds by index; the core kinds are indexes 0–12 (ids 1–13).
    pub kinds: Vec<KindDef>,
    /// Fields: common rows (`kind` = `None`) and kind rows.
    pub fields: Vec<FieldDef>,
    /// Enumeration values.
    pub values: Vec<EnumValue>,
    /// The per-kind `status` values: `status[k]` lists kind k's statuses.
    pub statuses: Vec<Vec<EnumValue>>,
    /// Edge kinds by index; the core kinds are indexes 0–24 (ids 1–25).
    pub edges: Vec<EdgeDef>,
    /// The project named queries.
    pub queries: Vec<QueryDef>,
}

type FieldRow = (&'static str, FieldTy, FieldClass, bool, Coerce);

const fn f(name: &'static str, ty: FieldTy, class: FieldClass, optional: bool) -> FieldRow {
    (name, ty, class, optional, Coerce::None)
}

const fn fc(
    name: &'static str,
    ty: FieldTy,
    class: FieldClass,
    optional: bool,
    coerce: Coerce,
) -> FieldRow {
    (name, ty, class, optional, coerce)
}

use FieldClass as C;
use FieldTy as T;

/// The common fields of [F08 §9.2], in declaration order 1–18.
const COMMON: [FieldRow; 18] = [
    f("title", T::Text, C::Scalar, false),
    f("abstract", T::Text, C::Scalar, true),
    f("status", T::Enum, C::Status, false),
    f("resolution", T::Enum, C::Status, false),
    fc("priority", T::Enum, C::Scalar, false, Coerce::Priority),
    f("criticality", T::Enum, C::Scalar, false),
    f("confidence", T::Enum, C::Scalar, false),
    f("authority", T::Enum, C::Authority, false),
    f("parent", T::Ref, C::Hierarchy, true),
    f("order", T::Text, C::Hierarchy, true),
    f("labels", T::TextSet, C::Set, true),
    f("pinned", T::Bool, C::Scalar, false),
    f("archived", T::Bool, C::Scalar, false),
    f("frozen", T::Bool, C::Scalar, false),
    fc("defer_until", T::Int, C::Scalar, true, Coerce::Timestamp),
    fc("due", T::Int, C::Scalar, true, Coerce::Timestamp),
    f("reason", T::Text, C::Scalar, true),
    f("body", T::Body, C::Text, true),
];

/// The kind fields of [F08 §9.3], per core kind in id order; `decl` starts at 20.
const KIND_FIELDS: [&[FieldRow]; 13] = [
    // task
    &[
        f("work_kind", T::Enum, C::Scalar, true),
        f("phase_state", T::Enum, C::Scalar, true),
        f("assignee", T::Text, C::Scalar, true),
        f("acceptance", T::Text, C::Text, true),
        f("files_owned", T::TextSet, C::GlobSet, true),
        f("estimate", T::Int, C::Scalar, true),
        f("reopen_count", T::Counter, C::Counter, false),
        f("reopen_if", T::Text, C::Text, true),
        f("pre_registered", T::Bool, C::Scalar, false),
    ],
    // doc
    &[
        f("doc_kind", T::Enum, C::Scalar, true),
        f("heading", T::Text, C::Text, true),
        f("revision", T::Int, C::Scalar, true),
        f("changed_in_round", T::Int, C::Scalar, true),
        f("targets", T::Text, C::Scalar, true),
        f("readiness", T::Text, C::Scalar, true),
    ],
    // note
    &[
        f("note_kind", T::Enum, C::Scalar, true),
        f("symptom", T::Text, C::Text, true),
        f("mechanism", T::Text, C::Text, true),
        f("defence", T::Text, C::Text, true),
        f("incidents", T::Counter, C::Counter, false),
        f("applies_to", T::TextSet, C::GlobSet, true),
        f("observed_git_sha", T::Oid, C::Scalar, true),
        fc("review_after", T::Int, C::Scalar, true, Coerce::Timestamp),
    ],
    // rule
    &[
        f("text", T::Text, C::Text, true),
        f("enforcement", T::Enum, C::Scalar, true),
        f("applies_to", T::TextSet, C::GlobSet, true),
        fc("since", T::Int, C::Scalar, true, Coerce::Timestamp),
        f("rationale", T::Text, C::Text, true),
        f("owner_quote", T::Text, C::Owner, true),
    ],
    // decision
    &[
        f("context", T::Text, C::Text, true),
        f("what", T::Text, C::Text, true),
        f("why", T::Text, C::Text, true),
        f("tradeoff", T::Text, C::Text, true),
        f("alternatives", T::Text, C::Scalar, true),
        f("revive_condition", T::Text, C::Text, true),
        f("owner_quote", T::Text, C::Owner, true),
    ],
    // question
    &[
        f("q_kind", T::Enum, C::Scalar, true),
        f("asked_of", T::Enum, C::Scalar, true),
        f("options", T::Text, C::Scalar, true),
        f("answer", T::Text, C::Text, true),
    ],
    // finding
    &[
        f("local_id", T::Text, C::Scalar, true),
        f("severity", T::Enum, C::Scalar, true),
        f("f_kind", T::Enum, C::Scalar, true),
        f("failure_scenario", T::Text, C::Text, false),
        f("what_needed", T::Text, C::Text, true),
        f("round", T::Int, C::Scalar, true),
        f("evidence", T::Text, C::Text, true),
    ],
    // verdict
    &[
        f("role", T::Text, C::Scalar, true),
        f("round", T::Int, C::Scalar, true),
        f("raw_label", T::Text, C::Text, true),
        f("outcome", T::Enum, C::Scalar, true),
        f("return_to", T::Enum, C::Scalar, true),
        f("criteria", T::Text, C::Text, true),
        f("conditions", T::Text, C::Text, true),
    ],
    // measurement
    &[
        f("metric", T::Text, C::Scalar, true),
        f("value", T::Float, C::Scalar, true),
        f("unit", T::Text, C::Scalar, true),
        f("target", T::Float, C::Scalar, true),
        f("command", T::Text, C::Text, true),
        f("measured_on", T::Oid, C::Scalar, true),
        f("env_host", T::Text, C::Scalar, true),
        f("env_profile", T::Text, C::Scalar, true),
        f("env_load", T::Enum, C::Scalar, true),
        f("env_scale", T::Text, C::Scalar, true),
        f("baseline", T::Ref, C::Scalar, true),
    ],
    // artifact
    &[
        f("root", T::Text, C::Identity, false),
        f("origin_path", T::Path, C::Identity, false),
        f("origin_pred", T::Ref, C::Identity, true),
        f("path", T::Path, C::Observation, false),
        f("oid", T::Oid, C::Observation, true),
        f("bytes", T::Int, C::Observation, true),
        f("observed_git", T::Oid, C::Observation, true),
        f("observed_blob", T::Oid, C::Observation, true),
        f("relink", T::Text, C::Observation, true),
        f("aliases", T::PathSet, C::AliasSet, true),
        f("artifact_kind", T::Enum, C::Scalar, true),
        f("replaced_by", T::Ref, C::Scalar, true),
        f("excerpt", T::Text, C::Scalar, true),
    ],
    // run
    &[
        f("wf_id", T::Text, C::Scalar, true),
        f("bg_task_id", T::Text, C::Scalar, true),
        f("session_id", T::Text, C::Scalar, true),
        f("script_path", T::Path, C::Scalar, true),
        f("args_hash", T::Text, C::Scalar, true),
        f("journal_path", T::Path, C::Scalar, true),
        fc("started", T::Int, C::Scalar, true, Coerce::Timestamp),
        fc("ended", T::Int, C::Scalar, true, Coerce::Timestamp),
        f("expected_artifacts", T::TextSet, C::Set, true),
    ],
    // lane
    &[
        f("worktree_path", T::Path, C::Scalar, true),
        f("git_branch", T::Text, C::Scalar, true),
        f("base_sha", T::Oid, C::Scalar, true),
        f("tip_sha", T::Oid, C::Scalar, true),
        f("target_dir", T::Path, C::Scalar, true),
        f("moirai_branch", T::Text, C::Scalar, true),
    ],
    // area
    &[
        f("path_globs", T::TextSet, C::GlobSet, true),
        f("root", T::Text, C::Identity, true),
        f("path_moves", T::PathMoveSet, C::PathMoveSet, true),
    ],
];

/// The core kinds of [F08 §9.1]: (name, `has_done`, `immutable_fields`, `root_variant`).
const KINDS: [(&str, bool, bool, bool); 13] = [
    ("task", true, false, false),
    ("doc", false, false, false),
    ("note", false, false, false),
    ("rule", false, false, false),
    ("decision", false, false, false),
    ("question", true, false, false),
    ("finding", false, false, false),
    ("verdict", true, true, false),
    ("measurement", false, false, false),
    ("artifact", false, false, false),
    ("run", false, false, false),
    ("lane", false, false, false),
    ("area", false, false, true),
];

/// The header enumerations of [F08 §9.4] (kind `*`).
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

/// The field enumerations of [F08 §9.4]: (kind, field, values); a value spelled with a leading `!` is a side value.
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

/// The statuses of [F08 §9.5] per core kind; a leading `!` marks a side state.
const STATUSES: [&[&str]; 13] = [
    &[
        "open",
        "in_progress",
        "done",
        "!deferred",
        "!cancelled",
        "!frozen",
    ],
    &["draft", "current", "!superseded", "!archived"],
    &["active", "!superseded", "!retracted", "!archived"],
    &[
        "proposed",
        "active",
        "!superseded",
        "!retracted",
        "!archived",
    ],
    &["proposed", "accepted", "!rejected", "!superseded"],
    &["open", "answered", "!dropped"],
    &[
        "open",
        "confirmed",
        "refuted",
        "fixed",
        "deferred",
        "withdrawn",
    ],
    &["open", "accepted", "!superseded"],
    &["current", "!moved_declared", "!retracted"],
    &["planned", "present", "!removed"],
    &["running", "green", "red", "stopped", "died"],
    &[
        "active",
        "ready_to_merge",
        "merge_pending",
        "merged",
        "!frozen",
        "!abandoned",
        "!measuring",
    ],
    &["active", "!archived"],
];

const KNOWLEDGE: [&str; 4] = ["doc", "note", "rule", "decision"];

/// A core edge kind row: (stored, lq_name, reverse names, src, dst, symmetric, same_kind, props, reading). `src`/`dst`
/// `None` is `any`; `props` is `p` (pinned), `f` (flagged), `a` (anchor) or `-`.
type EdgeRow = (
    &'static str,
    &'static str,
    &'static [&'static str],
    Option<&'static [&'static str]>,
    Option<&'static [&'static str]>,
    bool,
    bool,
    char,
    &'static str,
);

/// The 25 core edge kinds of [F08 §9.6] with their F1 columns.
const EDGES: [EdgeRow; 25] = [
    (
        "parent",
        "CHILD_OF",
        &["PARENT_OF", "HAS_CHILD", "HAS_SUBTASK"],
        Some(&["task", "doc", "area"]),
        Some(&["task", "doc", "area"]),
        false,
        true,
        '-',
        "{a} is a child of {b}",
    ),
    (
        "blocks",
        "BLOCKS",
        &["BLOCKED_BY"],
        Some(&["task", "question"]),
        Some(&["task"]),
        false,
        false,
        'f',
        "{a} must finish before {b} starts",
    ),
    (
        "gates",
        "GATES",
        &["GATED_BY"],
        Some(&["verdict"]),
        Some(&["task"]),
        false,
        false,
        'f',
        "verdict {a} gates the completion of {b}",
    ),
    (
        "merge_after",
        "MERGE_AFTER",
        &[],
        Some(&["lane"]),
        Some(&["lane"]),
        false,
        false,
        '-',
        "lane {a} merges after lane {b}",
    ),
    (
        "runs_in",
        "RUNS_IN",
        &[],
        Some(&["run"]),
        Some(&["lane"]),
        false,
        false,
        '-',
        "run {a} runs in lane {b}",
    ),
    (
        "answers",
        "ANSWERS",
        &["ANSWERED_BY"],
        Some(&["decision", "note"]),
        Some(&["question"]),
        false,
        false,
        '-',
        "{a} answers question {b}",
    ),
    (
        "scoped_to",
        "SCOPED_TO",
        &[],
        Some(&["note", "rule", "decision", "finding", "measurement"]),
        Some(&["area"]),
        false,
        false,
        '-',
        "{a} is scoped to area {b}",
    ),
    (
        "duplicate_of",
        "DUPLICATE_OF",
        &[],
        None,
        None,
        false,
        true,
        '-',
        "{a} duplicates canonical {b}",
    ),
    (
        "depends_on",
        "DEPENDS_ON",
        &[],
        Some(&["doc"]),
        Some(&["doc"]),
        false,
        false,
        '-',
        "section {a} depends on section {b}",
    ),
    (
        "supersedes",
        "SUPERSEDES",
        &["SUPERSEDED_BY"],
        Some(&KNOWLEDGE),
        Some(&KNOWLEDGE),
        false,
        true,
        '-',
        "{a} supersedes {b}",
    ),
    (
        "derived_from",
        "DERIVED_FROM",
        &[],
        Some(&["note", "doc", "verdict", "artifact"]),
        None,
        false,
        false,
        'p',
        "{a} is derived from {b}",
    ),
    (
        "cites",
        "CITES",
        &["CITED_BY"],
        None,
        Some(&KNOWLEDGE),
        false,
        false,
        'p',
        "{a} cites {b}",
    ),
    (
        "implements",
        "IMPLEMENTS",
        &["IMPLEMENTED_BY"],
        Some(&["task", "artifact"]),
        Some(&["decision", "doc"]),
        false,
        false,
        'p',
        "{a} implements {b}",
    ),
    (
        "refutes",
        "REFUTES",
        &["REFUTED_BY"],
        Some(&["finding", "measurement"]),
        Some(&["finding", "decision", "rule"]),
        false,
        false,
        '-',
        "{a} refutes {b}",
    ),
    (
        "confirms",
        "CONFIRMS",
        &["CONFIRMED_BY"],
        Some(&["finding", "measurement"]),
        Some(&["finding", "decision", "rule"]),
        false,
        false,
        '-',
        "{a} confirms {b}",
    ),
    (
        "verifies",
        "VERIFIES",
        &["VERIFIED_BY"],
        Some(&["measurement", "verdict"]),
        Some(&["finding", "task", "decision"]),
        false,
        false,
        '-',
        "{a} verifies {b}",
    ),
    (
        "addresses",
        "ADDRESSES",
        &["ADDRESSED_BY"],
        Some(&["task", "artifact"]),
        Some(&["finding"]),
        false,
        false,
        '-',
        "{a} addresses finding {b}",
    ),
    (
        "about",
        "ABOUT",
        &[],
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
        '-',
        "{a} is about {b}",
    ),
    (
        "discovered_from",
        "DISCOVERED_FROM",
        &[],
        None,
        Some(&["task"]),
        false,
        false,
        '-',
        "{a} was discovered from task {b}",
    ),
    (
        "produced",
        "PRODUCED",
        &[],
        Some(&["run"]),
        None,
        false,
        false,
        '-',
        "run {a} produced {b}",
    ),
    (
        "consumed",
        "CONSUMED",
        &[],
        Some(&["run"]),
        None,
        false,
        false,
        'p',
        "run {a} consumed {b}",
    ),
    (
        "contradicts",
        "CONTRADICTS",
        &[],
        Some(&["rule"]),
        Some(&["rule"]),
        true,
        false,
        '-',
        "{a} and {b} contradict",
    ),
    (
        "mentions",
        "MENTIONS",
        &["MENTIONED_BY"],
        None,
        None,
        false,
        false,
        '-',
        "{a} mentions {b}",
    ),
    (
        "relates",
        "RELATES",
        &[],
        None,
        None,
        true,
        false,
        '-',
        "{a} and {b} relate",
    ),
    (
        "at",
        "AT",
        &[],
        None,
        Some(&["artifact"]),
        false,
        false,
        'a',
        "{a} is anchored in file {b}",
    ),
];

impl Schema {
    /// The core schema, version 1 ([F08 §9]).
    pub fn core() -> Schema {
        let mut s = Schema {
            kinds: Vec::new(),
            fields: Vec::new(),
            values: Vec::new(),
            statuses: Vec::new(),
            edges: Vec::new(),
            queries: Vec::new(),
        };
        for (name, has_done, immutable_fields, root_variant) in KINDS {
            s.kinds.push(KindDef {
                name: name.to_string(),
                has_done,
                immutable_fields,
                root_variant,
            });
        }
        for (i, (name, ty, class, optional, coerce)) in COMMON.iter().enumerate() {
            s.fields.push(FieldDef {
                kind: None,
                name: name.to_string(),
                ty: *ty,
                class: *class,
                optional: *optional,
                coerce: *coerce,
                decl: i as u16 + 1,
            });
        }
        for (k, rows) in KIND_FIELDS.iter().enumerate() {
            for (i, (name, ty, class, optional, coerce)) in rows.iter().enumerate() {
                s.fields.push(FieldDef {
                    kind: Some(k),
                    name: name.to_string(),
                    ty: *ty,
                    class: *class,
                    optional: *optional,
                    coerce: *coerce,
                    decl: i as u16 + 20,
                });
            }
        }
        for (field, values) in HEADER_ENUMS {
            for (rank, v) in values.iter().enumerate() {
                s.values.push(EnumValue {
                    kind: None,
                    field: field.to_string(),
                    name: v.to_string(),
                    rank: rank as u16,
                    side: false,
                });
            }
        }
        for (kind, field, values) in FIELD_ENUMS {
            let k = s.kind(kind);
            for (rank, v) in values.iter().enumerate() {
                let (name, side) = v.strip_prefix('!').map_or((*v, false), |n| (n, true));
                s.values.push(EnumValue {
                    kind: k,
                    field: field.to_string(),
                    name: name.to_string(),
                    rank: rank as u16,
                    side,
                });
            }
        }
        for (k, values) in STATUSES.iter().enumerate() {
            let list = values
                .iter()
                .enumerate()
                .map(|(rank, v)| {
                    let (name, side) = v.strip_prefix('!').map_or((*v, false), |n| (n, true));
                    EnumValue {
                        kind: Some(k),
                        field: "status".to_string(),
                        name: name.to_string(),
                        rank: rank as u16,
                        side,
                    }
                })
                .collect();
            s.statuses.push(list);
        }
        for (stored, lq, reverse, src, dst, symmetric, same_kind, props, reading) in EDGES {
            let ends = |e: Option<&[&str]>| {
                e.map_or(Ends::Any, |ks| {
                    Ends::Kinds(ks.iter().map(|k| k.to_string()).collect())
                })
            };
            s.edges.push(EdgeDef {
                stored: stored.to_string(),
                lq: lq.to_string(),
                reverse: reverse.iter().map(|r| r.to_string()).collect(),
                src: ends(src),
                dst: ends(dst),
                symmetric,
                same_kind,
                reading: reading.to_string(),
                pinned: props == 'p',
                flagged: props == 'f',
                anchor: props == 'a',
            });
        }
        s
    }

    /// Adds a project kind with its statuses (the first is the initial status; a leading `!` marks a side state).
    pub fn add_kind(&mut self, name: &str, statuses: &[&str]) -> usize {
        let k = self.kinds.len();
        self.kinds.push(KindDef {
            name: name.to_string(),
            has_done: false,
            immutable_fields: false,
            root_variant: false,
        });
        self.statuses.push(
            statuses
                .iter()
                .enumerate()
                .map(|(rank, v)| {
                    let (n, side) = v.strip_prefix('!').map_or((*v, false), |n| (n, true));
                    EnumValue {
                        kind: Some(k),
                        field: "status".to_string(),
                        name: n.to_string(),
                        rank: rank as u16,
                        side,
                    }
                })
                .collect(),
        );
        k
    }

    /// Adds a project field on a kind; project fields are optional scalars with storage `field` ([F08 §8.4.2]).
    pub fn add_field(&mut self, kind: &str, name: &str, ty: FieldTy) {
        let k = self.kind(kind);
        let decl = self
            .fields
            .iter()
            .filter(|f| f.kind == k || f.kind.is_none())
            .map(|f| f.decl)
            .max()
            .unwrap_or(0)
            + 1;
        let class = match ty {
            FieldTy::TextSet => FieldClass::Set,
            FieldTy::Counter => FieldClass::Counter,
            _ => FieldClass::Scalar,
        };
        self.fields.push(FieldDef {
            kind: k,
            name: name.to_string(),
            ty,
            class,
            optional: true,
            coerce: Coerce::None,
            decl,
        });
    }

    /// Adds a project enumeration value to a field of a kind.
    pub fn add_value(&mut self, kind: &str, field: &str, name: &str) {
        let k = self.kind(kind);
        let rank = self.enum_values(k, field).len() as u16;
        self.values.push(EnumValue {
            kind: k,
            field: field.to_string(),
            name: name.to_string(),
            rank,
            side: false,
        });
    }

    /// Adds a project edge kind ([F08 §8.5.4]: `historical`, `tombstone`/`retain`, no properties).
    pub fn add_edge(
        &mut self,
        stored: &str,
        lq: &str,
        reverse: &[&str],
        src: Ends,
        dst: Ends,
        reading: &str,
    ) {
        self.edges.push(EdgeDef {
            stored: stored.to_string(),
            lq: lq.to_string(),
            reverse: reverse.iter().map(|r| r.to_string()).collect(),
            src,
            dst,
            symmetric: false,
            same_kind: false,
            reading: reading.to_string(),
            pinned: false,
            flagged: false,
            anchor: false,
        });
    }

    /// Adds a project named query.
    pub fn add_query(&mut self, name: &str, text: &str) {
        self.queries.push(QueryDef {
            name: name.to_string(),
            text: text.to_string(),
        });
    }

    /// The kind with this name, ASCII case-insensitive ([LQ/lexical §9]).
    pub fn kind(&self, name: &str) -> Option<usize> {
        self.kinds
            .iter()
            .position(|k| k.name.eq_ignore_ascii_case(name))
    }

    /// Every live kind.
    pub fn all_kinds(&self) -> KindSet {
        KindSet::first(self.kinds.len())
    }

    /// The kind set of edge endpoints.
    pub fn ends(&self, e: &Ends) -> KindSet {
        match e {
            Ends::Any => self.all_kinds(),
            Ends::Kinds(ks) => {
                let mut s = KindSet::EMPTY;
                for k in ks {
                    if let Some(i) = self.kind(k) {
                        s.insert(i);
                    }
                }
                s
            }
        }
    }

    /// The field `name` of kind `k`: a kind row first, then a common row ([F08 §9.2]–§9.3).
    pub fn field(&self, k: usize, name: &str) -> Option<&FieldDef> {
        self.fields
            .iter()
            .find(|f| f.kind == Some(k) && f.name == name)
            .or_else(|| {
                self.fields
                    .iter()
                    .find(|f| f.kind.is_none() && f.name == name)
            })
    }

    /// The fields of kind `k` in declaration order (common rows first).
    pub fn fields_of(&self, k: usize) -> Vec<&FieldDef> {
        let mut v: Vec<&FieldDef> = self
            .fields
            .iter()
            .filter(|f| f.kind.is_none() || f.kind == Some(k))
            .collect();
        v.sort_by(|a, b| (a.decl, &a.name).cmp(&(b.decl, &b.name)));
        v
    }

    /// The enumeration values of field `field` on kind `k` (`None`: the common rows only).
    pub fn enum_values(&self, k: Option<usize>, field: &str) -> Vec<&EnumValue> {
        if field == "status" {
            return k.map_or_else(Vec::new, |k| self.statuses[k].iter().collect());
        }
        self.values
            .iter()
            .filter(|v| v.field == field && (v.kind.is_none() || v.kind == k))
            .collect()
    }

    /// Resolves a written edge type name ([LQ/canonical-ast §5.4]): the `lq_name`, the forward synonyms, the stored
    /// snake-case name, then the reverse aliases, each ASCII case-insensitive. `parent` is the caller's E107 check.
    pub fn edge_name(&self, written: &str) -> Option<EdgeName> {
        if let Some(e) = self
            .edges
            .iter()
            .position(|e| e.lq.eq_ignore_ascii_case(written))
        {
            return Some(EdgeName {
                edge: e,
                forward: true,
            });
        }
        for (syn, lq) in FORWARD_SYNONYMS {
            if syn.eq_ignore_ascii_case(written) {
                return self
                    .edges
                    .iter()
                    .position(|e| e.lq == lq)
                    .map(|edge| EdgeName {
                        edge,
                        forward: true,
                    });
            }
        }
        if let Some(e) = self
            .edges
            .iter()
            .position(|e| e.stored.eq_ignore_ascii_case(written))
        {
            return Some(EdgeName {
                edge: e,
                forward: true,
            });
        }
        self.edges
            .iter()
            .position(|e| e.reverse.iter().any(|r| r.eq_ignore_ascii_case(written)))
            .map(|edge| EdgeName {
                edge,
                forward: false,
            })
    }

    /// Every name an edge type may be written with: `lq_name`s, synonyms, reverse aliases and stored names.
    pub fn edge_spellings(&self) -> Vec<String> {
        let mut v = Vec::new();
        for e in &self.edges {
            v.push(e.lq.clone());
            v.extend(e.reverse.iter().cloned());
        }
        v.extend(FORWARD_SYNONYMS.iter().map(|(s, _)| s.to_string()));
        for e in &self.edges {
            v.push(e.stored.clone());
        }
        v
    }

    /// The kind names of a set, joined with `|` (`any` for every live kind).
    pub fn kinds_text(&self, s: KindSet) -> String {
        if s.live() == self.all_kinds() && !s.contains(DELETED_BIT) {
            return "any".to_string();
        }
        let mut names: Vec<&str> = s
            .live()
            .iter()
            .filter(|&i| i < self.kinds.len())
            .map(|i| self.kinds[i].name.as_str())
            .collect();
        if s.contains(DELETED_BIT) {
            names.push("DELETED");
        }
        names.join("|")
    }

    /// Whether an edge kind is *same-kind*: its source and destination kind sets intersect ([LQ/envelope §4.1]).
    pub fn same_kind_edge(&self, e: usize) -> bool {
        let d = &self.edges[e];
        !self.ends(&d.src).and(self.ends(&d.dst)).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_schema_has_its_rows() {
        let s = Schema::core();
        assert_eq!(s.kinds.len(), 13);
        assert_eq!(s.edges.len(), 25);
        assert_eq!(s.fields.iter().filter(|f| f.kind.is_none()).count(), 18);
        let task = s.kind("Task").unwrap();
        assert_eq!(s.field(task, "status").unwrap().decl, 3);
        assert_eq!(s.field(task, "reopen_count").unwrap().ty, FieldTy::Counter);
        assert_eq!(s.enum_values(Some(task), "status").len(), 6);
        assert_eq!(s.enum_values(Some(task), "priority").len(), 5);
        let at = s.edge_name("at").unwrap();
        assert!(s.edges[at.edge].anchor);
        assert_eq!(
            s.edge_name("blocked_by"),
            Some(EdgeName {
                edge: 1,
                forward: false
            })
        );
        assert_eq!(
            s.edge_name("subtask_of"),
            Some(EdgeName {
                edge: 0,
                forward: true
            })
        );
        assert_eq!(
            s.edge_name("derived_from"),
            Some(EdgeName {
                edge: 10,
                forward: true
            })
        );
        assert!(s.same_kind_edge(1));
        assert!(!s.same_kind_edge(2));
        assert_eq!(s.kinds_text(s.ends(&s.edges[1].src)), "task|question");
    }

    #[test]
    fn kind_sets_are_bitsets() {
        let mut s = KindSet::first(13);
        assert_eq!(s.len(), 13);
        s.insert(DELETED_BIT);
        assert!(s.contains(DELETED_BIT));
        assert_eq!(s.live().len(), 13);
        assert!(KindSet::one(3).subset_of(s));
        assert!(KindSet::one(3).and(KindSet::one(4)).is_empty());
    }
}
