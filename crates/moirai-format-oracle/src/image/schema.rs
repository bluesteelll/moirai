//! The effective schema an importer parses node files with ([F14 §6.1], §11.1 step 1): the core schema of schema
//! version 1 ([F08 §9]) — its 13 kinds with their statuses (§9.1, §9.5), the common fields (§9.2), each kind's fields
//! (§9.3), the enumerations (§9.4) and the 25 edge kinds with their `props` (§9.6) — plus the project items a tree's
//! `schema/*.moi` files add ([F14 §7.1]).

use std::collections::BTreeMap;

/// A value type of the closed set ([F08 §5.1]) as the image writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    /// `bool`.
    Bool,
    /// `int`.
    Int,
    /// `counter` (a ledger, [F14 §6.5]).
    Counter,
    /// `f64`.
    F64,
    /// `enum`.
    Enum,
    /// `text` or `sym` (one text in the image).
    Text,
    /// `set` of the element type.
    Set(Elem),
    /// `ref`.
    Ref,
    /// `commitref`.
    CommitRef,
    /// `path`.
    Path,
    /// `oid`.
    Oid,
    /// `pathmove`.
    PathMove,
}

/// A set element type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elem {
    /// `int`.
    Int,
    /// `enum`.
    Enum,
    /// `text`, `sym`.
    Text,
    /// `ref`.
    Ref,
    /// `commitref`.
    CommitRef,
    /// `path`.
    Path,
    /// `oid`.
    Oid,
    /// `pathmove`.
    PathMove,
}

/// Where a field is written ([F08 §8.4.2], [F14 §6.2], §6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    /// A header column (`status:`, `priority:`, `parent:` …).
    Header,
    /// A source-truth flag (`flags:`).
    Flag,
    /// A cold column (`field` line).
    Cold,
    /// The field block (`field` line; `labels` as `label` lines; `order` as `order:`).
    Field,
    /// `title:`.
    Title,
    /// The body.
    Body,
}

/// A field-specific constraint the importer checks ([F14 §9.2], [F08 §5.4], §9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
    /// None.
    None,
    /// A record list ([F08 §5.4.5]) of the named field.
    RecordList(&'static str),
    /// Every element a path glob ([F08 §5.4.3]).
    Globs,
    /// Tagged scope elements ([F08 §5.4.6]).
    Tagged,
    /// A root name ([F08 §5.4.1]).
    RootName,
    /// A path whose root must be `abs`.
    AbsPath,
    /// ASCII only.
    Ascii,
    /// The `relink` grammar of R-17 ([F18 §5.1]).
    Relink,
}

/// A field item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDef {
    /// Field name.
    pub name: String,
    /// Type.
    pub ty: Ty,
    /// Storage.
    pub storage: Storage,
    /// Enumeration values by name (header and field enumerations).
    pub values: Vec<String>,
    /// The default's canonical single-line text, when the field has one.
    pub default: Option<String>,
    /// `one_line`.
    pub one_line: bool,
    /// `int` range.
    pub range: Option<(i64, i64)>,
    /// Extra constraint.
    pub sub: Sub,
    /// Paths written with the node's root implied ([F14 §5.2]).
    pub implied_root: bool,
    /// `retired`.
    pub retired: bool,
}

/// A kind item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindDef {
    /// Kind name.
    pub name: String,
    /// Status names in value order.
    pub statuses: Vec<String>,
    /// The initial status ([F08 §9.1]), the status an absent `status` key stands for.
    pub initial: String,
    /// `title_derived` (the title is not written while live).
    pub title_derived: bool,
    /// Kind fields by name.
    pub fields: BTreeMap<String, FieldDef>,
    /// `retired`.
    pub retired: bool,
}

/// An edge kind's `props` ([F08 §8.4.6]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Props {
    /// `none`.
    None,
    /// `pinned`.
    Pinned,
    /// `flagged`.
    Flagged,
    /// `anchor`.
    Anchor,
}

/// The effective schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    /// Kinds by name.
    pub kinds: BTreeMap<String, KindDef>,
    /// Common fields (kind `*`) by name.
    pub common: BTreeMap<String, FieldDef>,
    /// Edge kinds by stored name with their props and `symmetric` flag.
    pub edges: BTreeMap<String, (Props, bool)>,
}

fn f(name: &str, ty: Ty) -> FieldDef {
    FieldDef {
        name: name.into(),
        ty,
        storage: Storage::Field,
        values: Vec::new(),
        default: None,
        one_line: matches!(ty, Ty::Set(Elem::Text)),
        range: None,
        sub: Sub::None,
        implied_root: false,
        retired: false,
    }
}

fn e(name: &str, values: &[&str]) -> FieldDef {
    let mut d = f(name, Ty::Enum);
    d.values = values.iter().map(|s| (*s).to_owned()).collect();
    d
}

fn sym(name: &str) -> FieldDef {
    let mut d = f(name, Ty::Text);
    d.one_line = true;
    d
}

fn ranged(mut d: FieldDef, lo: i64, hi: i64) -> FieldDef {
    d.range = Some((lo, hi));
    d
}

fn with(mut d: FieldDef, sub: Sub) -> FieldDef {
    d.sub = sub;
    d
}

fn one_line(mut d: FieldDef) -> FieldDef {
    d.one_line = true;
    d
}

fn header(mut d: FieldDef, default: Option<&str>) -> FieldDef {
    d.storage = Storage::Header;
    d.default = default.map(str::to_owned);
    d
}

const U16: i64 = 65_535;
const U32: i64 = 4_294_967_295;
const I64MAX: i64 = i64::MAX;

impl Schema {
    /// The core schema of schema version 1 ([F08 §9]).
    pub fn core() -> Schema {
        let mut common = BTreeMap::new();
        let mut title = f("title", Ty::Text);
        title.storage = Storage::Title;
        title.one_line = true;
        let mut body = f("body", Ty::Text);
        body.storage = Storage::Body;
        let mut parent = f("parent", Ty::Ref);
        parent.storage = Storage::Header;
        let mut order = one_line(f("order", Ty::Text));
        order.sub = Sub::Ascii;
        let flag = |n: &str| {
            let mut d = f(n, Ty::Bool);
            d.storage = Storage::Flag;
            d.default = Some("false".into());
            d
        };
        let cold = |n: &str| {
            let mut d = ranged(f(n, Ty::Int), 1, U32);
            d.storage = Storage::Cold;
            d
        };
        for d in [
            title,
            one_line(f("abstract", Ty::Text)),
            header(e("status", &[]), None),
            header(
                e(
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
                Some("none"),
            ),
            header(e("priority", &["P0", "P1", "P2", "P3", "P4"]), Some("P2")),
            header(
                e("criticality", &["critical", "high", "normal", "low"]),
                Some("normal"),
            ),
            header(
                e(
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
                Some("unset"),
            ),
            header(
                e(
                    "authority",
                    &["owner", "orchestrator", "measured", "research", "agent"],
                ),
                Some("agent"),
            ),
            parent,
            order,
            f("labels", Ty::Set(Elem::Text)),
            flag("pinned"),
            flag("archived"),
            flag("frozen"),
            cold("defer_until"),
            cold("due"),
            f("reason", Ty::Text),
            body,
        ] {
            common.insert(d.name.clone(), d);
        }
        let mut kinds = BTreeMap::new();
        let mut kind = |name: &str, statuses: &[&str], fields: Vec<FieldDef>| {
            kinds.insert(
                name.to_owned(),
                KindDef {
                    name: name.into(),
                    statuses: statuses.iter().map(|s| (*s).to_owned()).collect(),
                    initial: if name == "artifact" {
                        "present".into()
                    } else {
                        statuses[0].to_owned()
                    },
                    title_derived: name == "artifact",
                    fields: fields.into_iter().map(|d| (d.name.clone(), d)).collect(),
                    retired: false,
                },
            );
        };
        let counter = |n: &str| {
            let mut d = f(n, Ty::Counter);
            d.default = Some("0".into());
            d
        };
        kind(
            "task",
            &[
                "open",
                "in_progress",
                "done",
                "deferred",
                "cancelled",
                "frozen",
            ],
            vec![
                e(
                    "work_kind",
                    &[
                        "design", "impl", "fix", "test", "measure", "merge", "doc", "research",
                        "review", "debt", "mutex",
                    ],
                ),
                e(
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
                        "blocked",
                        "frozen",
                        "deferred",
                    ],
                ),
                sym("assignee"),
                f("acceptance", Ty::Text),
                with(f("files_owned", Ty::Set(Elem::Text)), Sub::Globs),
                ranged(f("estimate", Ty::Int), 0, U16),
                counter("reopen_count"),
                f("reopen_if", Ty::Text),
                {
                    let mut d = f("pre_registered", Ty::Bool);
                    d.default = Some("false".into());
                    d
                },
            ],
        );
        kind(
            "doc",
            &["draft", "current", "superseded", "archived"],
            vec![
                e("doc_kind", &["plan", "section", "report", "patch"]),
                one_line(f("heading", Ty::Text)),
                ranged(f("revision", Ty::Int), 0, U16),
                ranged(f("changed_in_round", Ty::Int), 0, U16),
                with(f("targets", Ty::Text), Sub::RecordList("targets")),
                with(f("readiness", Ty::Text), Sub::RecordList("readiness")),
            ],
        );
        kind(
            "note",
            &["active", "superseded", "retracted", "archived"],
            vec![
                e(
                    "note_kind",
                    &["note", "hazard", "lesson", "checkpoint", "summary"],
                ),
                f("symptom", Ty::Text),
                f("mechanism", Ty::Text),
                f("defence", Ty::Text),
                counter("incidents"),
                with(f("applies_to", Ty::Set(Elem::Text)), Sub::Tagged),
                f("observed_git_sha", Ty::Oid),
                ranged(f("review_after", Ty::Int), 0, I64MAX),
            ],
        );
        kind(
            "rule",
            &["proposed", "active", "superseded", "retracted", "archived"],
            vec![
                f("text", Ty::Text),
                e("enforcement", &["must", "should"]),
                with(f("applies_to", Ty::Set(Elem::Text)), Sub::Tagged),
                ranged(f("since", Ty::Int), 0, I64MAX),
                f("rationale", Ty::Text),
                f("owner_quote", Ty::Text),
            ],
        );
        kind(
            "decision",
            &["proposed", "accepted", "rejected", "superseded"],
            vec![
                f("context", Ty::Text),
                f("what", Ty::Text),
                f("why", Ty::Text),
                f("tradeoff", Ty::Text),
                with(f("alternatives", Ty::Text), Sub::RecordList("alternatives")),
                f("revive_condition", Ty::Text),
                f("owner_quote", Ty::Text),
            ],
        );
        kind(
            "question",
            &["open", "answered", "dropped"],
            vec![
                e("q_kind", &["values", "scope", "unclear"]),
                e("asked_of", &["owner", "orchestrator", "architect"]),
                with(f("options", Ty::Text), Sub::RecordList("options")),
                f("answer", Ty::Text),
            ],
        );
        kind(
            "finding",
            &[
                "open",
                "confirmed",
                "refuted",
                "fixed",
                "deferred",
                "withdrawn",
            ],
            vec![
                sym("local_id"),
                e("severity", &["blocker", "important", "optional"]),
                e(
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
                f("failure_scenario", Ty::Text),
                f("what_needed", Ty::Text),
                ranged(f("round", Ty::Int), 0, U16),
                f("evidence", Ty::Text),
            ],
        );
        kind(
            "verdict",
            &["open", "accepted", "superseded"],
            vec![
                sym("role"),
                ranged(f("round", Ty::Int), 0, U16),
                f("raw_label", Ty::Text),
                e(
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
                e("return_to", &["architect", "developer", "tester", "none"]),
                f("criteria", Ty::Text),
                f("conditions", Ty::Text),
            ],
        );
        kind(
            "measurement",
            &["current", "moved_declared", "retracted"],
            vec![
                sym("metric"),
                f("value", Ty::F64),
                sym("unit"),
                f("target", Ty::F64),
                f("command", Ty::Text),
                f("measured_on", Ty::Oid),
                sym("env_host"),
                sym("env_profile"),
                e("env_load", &["quiet", "loaded"]),
                sym("env_scale"),
                f("baseline", Ty::Ref),
            ],
        );
        let implied = |mut d: FieldDef| {
            d.implied_root = true;
            d
        };
        kind(
            "artifact",
            &["planned", "present", "removed"],
            vec![
                with(sym("root"), Sub::RootName),
                implied(f("origin_path", Ty::Path)),
                f("origin_pred", Ty::Ref),
                implied(f("path", Ty::Path)),
                f("oid", Ty::Oid),
                ranged(f("bytes", Ty::Int), 0, I64MAX),
                f("observed_git", Ty::Oid),
                f("observed_blob", Ty::Oid),
                with(sym("relink"), Sub::Relink),
                implied(f("aliases", Ty::Set(Elem::Path))),
                e(
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
                f("replaced_by", Ty::Ref),
                f("excerpt", Ty::Text),
            ],
        );
        kind(
            "run",
            &["running", "green", "red", "stopped", "died"],
            vec![
                one_line(f("wf_id", Ty::Text)),
                one_line(f("bg_task_id", Ty::Text)),
                one_line(f("session_id", Ty::Text)),
                with(f("script_path", Ty::Path), Sub::AbsPath),
                one_line(f("args_hash", Ty::Text)),
                with(f("journal_path", Ty::Path), Sub::AbsPath),
                ranged(f("started", Ty::Int), 0, I64MAX),
                ranged(f("ended", Ty::Int), 0, I64MAX),
                f("expected_artifacts", Ty::Set(Elem::Text)),
                // [F08 §9.3] decls 29 and 30 (spec sync 2b): the run's harness and model family, both `sym`.
                sym("harness"),
                sym("model"),
            ],
        );
        kind(
            "lane",
            &[
                "active",
                "ready_to_merge",
                "merge_pending",
                "merged",
                "frozen",
                "abandoned",
                "measuring",
            ],
            vec![
                with(f("worktree_path", Ty::Path), Sub::AbsPath),
                sym("git_branch"),
                f("base_sha", Ty::Oid),
                f("tip_sha", Ty::Oid),
                with(f("target_dir", Ty::Path), Sub::AbsPath),
                sym("moirai_branch"),
            ],
        );
        kind(
            "area",
            &["active", "archived"],
            vec![
                with(f("path_globs", Ty::Set(Elem::Text)), Sub::Globs),
                with(sym("root"), Sub::RootName),
                implied(f("path_moves", Ty::Set(Elem::PathMove))),
            ],
        );
        let edges = [
            ("parent", Props::None, false),
            ("blocks", Props::Flagged, false),
            ("gates", Props::Flagged, false),
            ("merge_after", Props::None, false),
            ("runs_in", Props::None, false),
            ("answers", Props::None, false),
            ("scoped_to", Props::None, false),
            ("duplicate_of", Props::None, false),
            ("depends_on", Props::None, false),
            ("supersedes", Props::None, false),
            ("derived_from", Props::Pinned, false),
            ("cites", Props::Pinned, false),
            ("implements", Props::Pinned, false),
            ("refutes", Props::None, false),
            ("confirms", Props::None, false),
            ("verifies", Props::None, false),
            ("addresses", Props::None, false),
            ("about", Props::None, false),
            ("discovered_from", Props::None, false),
            ("produced", Props::None, false),
            ("consumed", Props::Pinned, false),
            ("contradicts", Props::None, true),
            ("mentions", Props::None, false),
            ("relates", Props::None, true),
            ("at", Props::Anchor, false),
        ]
        .into_iter()
        .map(|(n, p, s)| (n.to_owned(), (p, s)))
        .collect();
        Schema {
            kinds,
            common,
            edges,
        }
    }

    /// The field `name` of kind `kind`: a kind field, else a common field.
    pub fn field(&self, kind: &str, name: &str) -> Option<&FieldDef> {
        self.kinds
            .get(kind)
            .and_then(|k| k.fields.get(name))
            .or_else(|| self.common.get(name))
    }

    /// The enumeration values of a field for a kind (`status` takes the kind's statuses).
    pub fn enum_values<'a>(&'a self, kind: &'a str, field: &'a FieldDef) -> Vec<&'a str> {
        if field.name == "status" {
            self.kinds
                .get(kind)
                .map(|k| k.statuses.iter().map(String::as_str).collect())
                .unwrap_or_default()
        } else {
            field.values.iter().map(String::as_str).collect()
        }
    }
}

/// The type named by a `tname` of [F14 §7.1].
pub fn ty_of_name(t: &str, elem: Option<&str>) -> Option<Ty> {
    Some(match t {
        "bool" => Ty::Bool,
        "int" => Ty::Int,
        "counter" => Ty::Counter,
        "f64" => Ty::F64,
        "enum" => Ty::Enum,
        "text" | "sym" => Ty::Text,
        "ref" => Ty::Ref,
        "commitref" => Ty::CommitRef,
        "path" => Ty::Path,
        "oid" => Ty::Oid,
        "pathmove" => Ty::PathMove,
        "set" => Ty::Set(match elem? {
            "int" => Elem::Int,
            "enum" => Elem::Enum,
            "text" | "sym" => Elem::Text,
            "ref" => Elem::Ref,
            "commitref" => Elem::CommitRef,
            "path" => Elem::Path,
            "oid" => Elem::Oid,
            "pathmove" => Elem::PathMove,
            _ => return None,
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [F08 §9.1]: 13 core kinds, 25 edge kinds, the initial statuses of §9.1; §9.3: a run's `harness` and `model`
    /// are one-line `sym` fields (spec sync 2b).
    #[test]
    fn core_schema_shape() {
        let s = Schema::core();
        assert_eq!(s.kinds.len(), 13);
        assert_eq!(s.edges.len(), 25);
        assert_eq!(s.kinds["artifact"].statuses[1], "present");
        assert!(s.kinds["artifact"].title_derived);
        assert_eq!(
            s.field("task", "estimate").unwrap().range,
            Some((0, 65_535))
        );
        assert_eq!(
            s.field("task", "priority").unwrap().default.as_deref(),
            Some("P2")
        );
        for name in ["harness", "model"] {
            let f = s.field("run", name).unwrap();
            assert_eq!((f.ty, f.one_line), (Ty::Text, true), "run.{name}");
        }
    }
}
