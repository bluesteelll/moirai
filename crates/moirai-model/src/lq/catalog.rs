//! The binder's registry ([LQ/canonical-ast §5.1] item 1): value types, the scalar and aggregate built-ins of
//! [LQ/canonical-ast] Table 5.3 with the signatures of [LQ/std §2.10], the built-in relations of [LQ/std §2.9], the
//! `tx.*` named mutations of [LQ/std §7] and the standard read catalog of [LQ/std §4]–§6 as LQ text.
//!
//! The standard library is data: its definitions are embedded byte for byte from [LQ/std] and parsed with start symbol
//! `define_stmt` ([LQ/grammar-v1.ebnf §P.1]) when first needed; their signatures come from the `param_decl` lists and
//! their columns from binding them against the core schema.

use crate::lq::ast::Define;
use crate::lq::bind::{self, Bound};
use crate::lq::cast::CDefine;
use crate::lq::ctx::{BindCtx, Caller, MapIds, Params};
use crate::lq::diag::Diag;
use crate::lq::parser::{ParseOptions, parse_define};
use crate::lq::schema::{KindSet, Schema};
use std::sync::OnceLock;

/// An enumeration type: the field and the kinds whose values it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumTy {
    /// The field name.
    pub field: String,
    /// The kinds of the variable it was read from.
    pub kinds: KindSet,
    /// F2 `coerce` = `priority`: `'P1'`, `P1` and `1` denote one value, encoded as `INT`.
    pub priority: bool,
}

/// A value type of the binder ([50 §3.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    /// Unknown: an untyped column, or an operand after an error.
    Any,
    /// The `NULL` literal.
    Null,
    /// `bool`.
    Bool,
    /// `int`.
    Int,
    /// `float`.
    Float,
    /// `text`.
    Text,
    /// A kind name (the elements of `labels()`).
    KindName,
    /// `duration`.
    Dur,
    /// `timestamp`.
    Time,
    /// A revision.
    Rev,
    /// An enumeration value.
    Enum(Box<EnumTy>),
    /// A node of these kinds.
    Node(KindSet),
    /// An edge of these edge kinds (edge-kind indexes in a [`KindSet`]).
    Edge(KindSet),
    /// A list or set.
    List(Box<Ty>),
    /// A map (the `lease` property, a map literal).
    Map,
    /// `range<int>`.
    Range,
}

impl Ty {
    /// The type's name in texts (`<left type>` of [LQ/errors] E103).
    pub fn name(&self) -> String {
        match self {
            Ty::Any => "any".into(),
            Ty::Null => "null".into(),
            Ty::Bool => "bool".into(),
            Ty::Int => "int".into(),
            Ty::Float => "float".into(),
            Ty::Text => "text".into(),
            Ty::KindName => "kind".into(),
            Ty::Dur => "duration".into(),
            Ty::Time => "timestamp".into(),
            Ty::Rev => "rev".into(),
            Ty::Enum(e) => {
                if e.priority {
                    "priority".into()
                } else {
                    format!("enum {}", e.field)
                }
            }
            Ty::Node(_) => "node".into(),
            Ty::Edge(_) => "edge".into(),
            Ty::List(t) => format!("list<{}>", t.name()),
            Ty::Map => "map".into(),
            Ty::Range => "range<int>".into(),
        }
    }
}

/// A parameter or column type of a signature ([LQ/std §2.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PT {
    /// `node`.
    Node,
    /// A node or an edge (`link_state`).
    NodeOrEdge,
    /// An edge.
    Edge,
    /// `int`.
    Int,
    /// `float`.
    Float,
    /// `bool`.
    Bool,
    /// `text`.
    Text,
    /// `rev`.
    Rev,
    /// `timestamp`.
    Time,
    /// `duration`.
    Dur,
    /// `range<int>`.
    Range,
    /// `list<node>`.
    ListNode,
    /// `list<int>`.
    ListInt,
    /// `list<text>`.
    ListText,
    /// `list<rev>`.
    ListRev,
    /// Any value.
    Any,
}

impl PT {
    /// The binder type.
    pub fn ty(self, all: KindSet) -> Ty {
        match self {
            PT::Node => Ty::Node(all),
            PT::NodeOrEdge | PT::Any => Ty::Any,
            PT::Edge => Ty::Edge(KindSet::first(255)),
            PT::Int => Ty::Int,
            PT::Float => Ty::Float,
            PT::Bool => Ty::Bool,
            PT::Text => Ty::Text,
            PT::Rev => Ty::Rev,
            PT::Time => Ty::Time,
            PT::Dur => Ty::Dur,
            PT::Range => Ty::Range,
            PT::ListNode => Ty::List(Box::new(Ty::Node(all))),
            PT::ListInt => Ty::List(Box::new(Ty::Int)),
            PT::ListText => Ty::List(Box::new(Ty::Text)),
            PT::ListRev => Ty::List(Box::new(Ty::Rev)),
        }
    }

    /// The v1 parameter type named by a `param_decl` type ([LQ/std §2.2]), ASCII case-insensitive ([LQ/lexical §9]).
    pub fn from_decl(name: &str, arg: Option<&str>) -> Option<PT> {
        let n = name.to_ascii_lowercase();
        let a = arg.map(str::to_ascii_lowercase);
        Some(match (n.as_str(), a.as_deref()) {
            ("node", None) => PT::Node,
            ("int", None) => PT::Int,
            ("float", None) => PT::Float,
            ("bool", None) => PT::Bool,
            ("text", None) => PT::Text,
            ("rev", None) => PT::Rev,
            ("timestamp", None) => PT::Time,
            ("duration", None) => PT::Dur,
            ("range", Some("int")) => PT::Range,
            ("list", Some("node")) => PT::ListNode,
            ("list", Some("int")) => PT::ListInt,
            ("list", Some("text")) => PT::ListText,
            ("list", Some("rev")) => PT::ListRev,
            _ => return None,
        })
    }
}

/// One parameter of a signature.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    /// The name (case-sensitive, [LQ/grammar-v1.ebnf §P.10]).
    pub name: &'static str,
    /// The type.
    pub ty: PT,
    /// No default and not optional.
    pub required: bool,
}

const fn p(name: &'static str, ty: PT) -> Param {
    Param {
        name,
        ty,
        required: true,
    }
}

const fn o(name: &'static str, ty: PT) -> Param {
    Param {
        name,
        ty,
        required: false,
    }
}

/// What a relation reads ([50 §3.9] item 4, §3.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelClass {
    /// Graph state at the view.
    Graph,
    /// Graph state with a runtime column (`blockers`'s `elsewhere`): any view, live.
    GraphLive,
    /// History, scoped by its range or the view's reachability.
    History,
    /// Runtime tables: branch tip only, live.
    Runtime,
    /// Tree-derived: branch tip with a resolved tree, live.
    Tree,
    /// The schema and the named-query catalog.
    Catalog,
}

/// A built-in relation ([LQ/std §2.9]).
#[derive(Clone, Copy, Debug)]
pub struct Relation {
    /// The registry name.
    pub name: &'static str,
    /// Parameters in declaration order.
    pub params: &'static [Param],
    /// Yield columns.
    pub yields: &'static [(&'static str, PT)],
    /// What it reads.
    pub class: RelClass,
}

/// The built-in relations of [LQ/std §2.9] (with [50 §2.6]'s yields as corrected by §2.8).
pub const RELATIONS: [Relation; 20] = [
    Relation {
        name: "blockers",
        params: &[p("n", PT::Node), o("transitive", PT::Bool)],
        yields: &[
            ("blocker", PT::Node),
            ("depth", PT::Int),
            ("via", PT::Node),
            ("reason", PT::Text),
            ("flagged", PT::Bool),
            ("elsewhere", PT::Bool),
        ],
        class: RelClass::GraphLive,
    },
    Relation {
        name: "subtree",
        params: &[p("n", PT::Node), o("depth", PT::Int)],
        yields: &[
            ("node", PT::Node),
            ("depth", PT::Int),
            ("parent", PT::Node),
            ("position", PT::Int),
        ],
        class: RelClass::Graph,
    },
    Relation {
        name: "neighbors",
        params: &[
            p("n", PT::Node),
            o("depth", PT::Int),
            o("types", PT::ListText),
        ],
        yields: &[
            ("node", PT::Node),
            ("edge", PT::Any),
            ("dir", PT::Text),
            ("depth", PT::Int),
        ],
        class: RelClass::Graph,
    },
    Relation {
        name: "search",
        params: &[
            p("terms", PT::Text),
            o("kinds", PT::ListText),
            o("fields", PT::ListText),
        ],
        yields: &[
            ("node", PT::Node),
            ("score", PT::Float),
            ("field", PT::Text),
            ("snippet", PT::Text),
        ],
        class: RelClass::Graph,
    },
    Relation {
        name: "history",
        params: &[p("n", PT::Node), o("field", PT::Text), o("in", PT::Rev)],
        yields: &[
            ("seq", PT::Int),
            ("commit", PT::Rev),
            ("ref", PT::Text),
            ("actor", PT::Text),
            ("role", PT::Text),
            ("at", PT::Time),
            ("op", PT::Text),
            ("aspect", PT::Text),
            ("name", PT::Text),
            ("before", PT::Any),
            ("after", PT::Any),
            ("message", PT::Text),
            ("via", PT::Text),
        ],
        class: RelClass::History,
    },
    Relation {
        name: "blame",
        params: &[p("n", PT::Node)],
        yields: &[
            ("aspect", PT::Text),
            ("name", PT::Text),
            ("value", PT::Any),
            ("seq", PT::Int),
            ("commit", PT::Rev),
            ("actor", PT::Text),
            ("at", PT::Time),
        ],
        class: RelClass::History,
    },
    Relation {
        name: "log",
        params: &[
            o("range", PT::Rev),
            o("actor", PT::Text),
            o("touching", PT::Node),
        ],
        yields: &[
            ("commit", PT::Rev),
            ("seq", PT::Int),
            ("ref", PT::Text),
            ("kind", PT::Text),
            ("actor", PT::Text),
            ("role", PT::Text),
            ("at", PT::Time),
            ("message", PT::Text),
            ("ops", PT::Int),
        ],
        class: RelClass::History,
    },
    Relation {
        name: "diff",
        params: &[p("range", PT::Rev), o("scope", PT::Node)],
        yields: &[
            ("change", PT::Text),
            ("node", PT::Node),
            ("kind", PT::Text),
            ("aspect", PT::Text),
            ("name", PT::Text),
            ("before", PT::Any),
            ("after", PT::Any),
            ("side", PT::Text),
            ("last_commit", PT::Rev),
            ("actor", PT::Text),
        ],
        class: RelClass::History,
    },
    Relation {
        name: "changes",
        params: &[p("since", PT::Rev), o("ref", PT::Rev)],
        yields: &[
            ("seq", PT::Int),
            ("ref", PT::Text),
            ("commit", PT::Rev),
            ("node", PT::Node),
            ("op", PT::Text),
            ("aspect", PT::Text),
            ("name", PT::Text),
            ("actor", PT::Text),
            ("affected", PT::Any),
        ],
        class: RelClass::History,
    },
    Relation {
        name: "conflicts",
        params: &[],
        yields: &[
            ("key", PT::Text),
            ("node", PT::Node),
            ("class", PT::Text),
            ("base", PT::Any),
            ("ours", PT::Any),
            ("theirs", PT::Any),
            ("commit", PT::Rev),
            ("hint", PT::Text),
        ],
        class: RelClass::Graph,
    },
    Relation {
        name: "violations",
        params: &[o("ref", PT::Rev)],
        yields: &[
            ("key", PT::Text),
            ("class", PT::Text),
            ("detail", PT::Text),
            ("suggested", PT::Text),
        ],
        class: RelClass::History,
    },
    Relation {
        name: "across",
        params: &[
            p("refs", PT::ListRev),
            p("ids", PT::ListNode),
            o("aspects", PT::ListText),
        ],
        yields: &[
            ("node", PT::Node),
            ("aspect", PT::Text),
            ("name", PT::Text),
            ("ref", PT::Text),
            ("value", PT::Any),
            ("diverged", PT::Bool),
        ],
        class: RelClass::Graph,
    },
    Relation {
        name: "refs",
        params: &[],
        yields: &[
            ("name", PT::Text),
            ("kind", PT::Text),
            ("tip", PT::Rev),
            ("seq", PT::Int),
            ("ahead", PT::Int),
            ("behind", PT::Int),
            ("fork", PT::Rev),
            ("staged", PT::Bool),
        ],
        class: RelClass::History,
    },
    Relation {
        name: "leases",
        params: &[],
        yields: &[
            ("n", PT::Node),
            ("lease_id", PT::Int),
            ("kind", PT::Text),
            ("flags", PT::Int),
            ("role", PT::Text),
            ("holder", PT::Text),
            ("branch", PT::Text),
            ("run", PT::Node),
            ("token", PT::Int),
            ("claimed_hlc", PT::Int),
            ("ttl_ms", PT::Int),
            ("expires", PT::Time),
            ("anchor", PT::Any),
            ("bound", PT::Text),
            ("root_session", PT::Text),
            ("proc", PT::Any),
            ("files_owned", PT::ListText),
        ],
        class: RelClass::Runtime,
    },
    Relation {
        name: "markers",
        params: &[],
        yields: &[
            ("n", PT::Node),
            ("ref_id", PT::Text),
            ("commit", PT::Rev),
            ("kind", PT::Text),
            ("status", PT::Text),
            ("cause", PT::Text),
            ("flags", PT::Int),
            ("ref_seq", PT::Int),
            ("actor", PT::Text),
            ("outcome", PT::Text),
            ("hlc", PT::Int),
            ("seq", PT::Int),
            ("emit_lsn", PT::Int),
            ("holders", PT::ListText),
        ],
        class: RelClass::Runtime,
    },
    Relation {
        name: "links",
        params: &[o("scope", PT::Node)],
        yields: &[
            ("node", PT::Node),
            ("anchor", PT::Text),
            ("file", PT::Node),
            ("path", PT::Text),
            ("kind", PT::Text),
            ("scope", PT::Text),
            ("state", PT::Text),
            ("evidence", PT::Text),
            ("next", PT::Text),
        ],
        class: RelClass::Tree,
    },
    Relation {
        name: "root_moves",
        params: &[o("root", PT::Text)],
        yields: &[
            ("hlc", PT::Int),
            ("class", PT::Text),
            ("from", PT::Text),
            ("to", PT::Text),
            ("git", PT::Text),
        ],
        class: RelClass::Graph,
    },
    Relation {
        name: "schema",
        params: &[o("kind", PT::Text)],
        yields: &[
            ("kind", PT::Text),
            ("field", PT::Text),
            ("type", PT::Text),
            ("optional", PT::Bool),
            ("index", PT::Text),
        ],
        class: RelClass::Catalog,
    },
    Relation {
        name: "schema_edges",
        params: &[],
        yields: &[
            ("name", PT::Text),
            ("stored", PT::Text),
            ("src_kinds", PT::ListText),
            ("dst_kinds", PT::ListText),
            ("symmetric", PT::Bool),
            ("reverse_names", PT::ListText),
            ("reading", PT::Text),
            ("class", PT::Text),
            ("acyclic", PT::Text),
        ],
        class: RelClass::Catalog,
    },
    Relation {
        name: "queries",
        params: &[],
        yields: &[
            ("name", PT::Text),
            ("signature", PT::Text),
            ("shape", PT::Text),
            ("budget", PT::Text),
            ("lq", PT::Int),
            ("text", PT::Text),
        ],
        class: RelClass::Catalog,
    },
];

/// The relations with revision positions ([LQ/lexical §4.2]); written as scalar functions they are E109.
pub const REVISION_RELATIONS: [&str; 6] =
    ["diff", "log", "changes", "history", "across", "violations"];

/// The relation with this registry name, ASCII case-insensitive ([LQ/lexical §9]).
pub fn relation(name: &str) -> Option<&'static Relation> {
    RELATIONS.iter().find(|r| r.name.eq_ignore_ascii_case(name))
}

/// What a scalar function returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    /// `bool`.
    Bool,
    /// `int`.
    Int,
    /// `float`.
    Float,
    /// `text`.
    Text,
    /// `timestamp`.
    Time,
    /// `duration`.
    Dur,
    /// A node of the argument's kinds (`id()`).
    NodeOfArg0,
    /// An artifact node or absent (`file()`).
    Artifact,
    /// A set of nodes.
    NodeList,
    /// The kind names of a node (`labels()`).
    KindNames,
    /// The type of the first argument (`coalesce`, `min`, `max`, `abs`).
    Arg0,
    /// A list of the first argument's type (`collect`).
    ListOfArg0,
}

/// What reading a function's value needs ([50 §3.8]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnClass {
    /// Any view.
    Plain,
    /// Tree-derived: branch tip with a resolved tree, live.
    Tree,
    /// Git ancestry: branch tip, live.
    Git,
}

/// A scalar or aggregate built-in ([LQ/canonical-ast] Table 5.3, [LQ/std §2.10]).
#[derive(Clone, Copy, Debug)]
pub struct Func {
    /// The canonical name, then the other accepted spellings.
    pub names: &'static [&'static str],
    /// Parameters.
    pub params: &'static [Param],
    /// The last parameter repeats (`coalesce`).
    pub variadic: bool,
    /// The result type.
    pub ret: Ret,
    /// An aggregate.
    pub agg: bool,
    /// What it reads.
    pub class: FnClass,
}

const fn func(names: &'static [&'static str], params: &'static [Param], ret: Ret) -> Func {
    Func {
        names,
        params,
        variadic: false,
        ret,
        agg: false,
        class: FnClass::Plain,
    }
}

const AGG_PARAMS: &[Param] = &[p("x", PT::Any)];

const fn agg(names: &'static [&'static str], ret: Ret) -> Func {
    Func {
        names,
        params: AGG_PARAMS,
        variadic: false,
        ret,
        agg: true,
        class: FnClass::Plain,
    }
}

/// The built-in functions. `datetime` with no argument is `now` ([LQ/canonical-ast] Table 5.3); the binder maps it.
pub const FUNCS: [Func; 40] = [
    func(
        &["subtree"],
        &[p("n", PT::Node), o("depth", PT::Int)],
        Ret::NodeList,
    ),
    func(
        &["descendants"],
        &[p("n", PT::Node), o("depth", PT::Int)],
        Ret::NodeList,
    ),
    func(&["ancestors"], &[p("n", PT::Node)], Ret::NodeList),
    func(&["children"], &[p("n", PT::Node)], Ret::NodeList),
    func(
        &["applies"],
        &[p("k", PT::Node), p("glob", PT::Text)],
        Ret::Bool,
    ),
    func(
        &["applies_role"],
        &[p("k", PT::Node), p("role", PT::Text)],
        Ret::Bool,
    ),
    func(
        &["applies_phase"],
        &[p("k", PT::Node), p("phase", PT::Text)],
        Ret::Bool,
    ),
    func(
        &["fits_role"],
        &[p("t", PT::Node), p("role", PT::Text)],
        Ret::Bool,
    ),
    func(
        &["glob_match"],
        &[p("path", PT::Text), p("glob", PT::Text)],
        Ret::Bool,
    ),
    func(
        &["text_match"],
        &[p("n", PT::Node), p("terms", PT::Text)],
        Ret::Bool,
    ),
    func(
        &["file"],
        &[p("path", PT::Text), o("root", PT::Text)],
        Ret::Artifact,
    ),
    Func {
        names: &["link_state"],
        params: &[p("x", PT::NodeOrEdge)],
        variadic: false,
        ret: Ret::Text,
        agg: false,
        class: FnClass::Tree,
    },
    Func {
        names: &["staleness"],
        params: &[p("n", PT::Node)],
        variadic: false,
        ret: Ret::Text,
        agg: false,
        class: FnClass::Git,
    },
    func(
        &["relevant_to"],
        &[p("n", PT::Node), p("agent", PT::Text)],
        Ret::Bool,
    ),
    func(&["me"], &[], Ret::Text),
    func(&["view_ref"], &[], Ret::Text),
    func(&["now"], &[], Ret::Time),
    func(&["datetime"], &[p("s", PT::Text)], Ret::Time),
    func(&["date"], &[p("s", PT::Text)], Ret::Time),
    func(&["duration"], &[p("s", PT::Text)], Ret::Dur),
    func(
        &["size", "cardinality", "length"],
        &[p("x", PT::Any)],
        Ret::Int,
    ),
    func(&["lower", "toLower"], &[p("s", PT::Text)], Ret::Text),
    func(&["upper", "toUpper"], &[p("s", PT::Text)], Ret::Text),
    func(&["trim"], &[p("s", PT::Text)], Ret::Text),
    func(
        &["substring"],
        &[p("s", PT::Text), p("start", PT::Int), o("length", PT::Int)],
        Ret::Text,
    ),
    Func {
        names: &["coalesce"],
        params: &[p("x", PT::Any)],
        variadic: true,
        ret: Ret::Arg0,
        agg: false,
        class: FnClass::Plain,
    },
    func(
        &["round"],
        &[p("x", PT::Any), o("digits", PT::Int)],
        Ret::Float,
    ),
    func(&["abs"], &[p("x", PT::Any)], Ret::Arg0),
    func(&["toString"], &[p("x", PT::Any)], Ret::Text),
    func(&["toInteger"], &[p("x", PT::Any)], Ret::Int),
    func(&["toFloat"], &[p("x", PT::Any)], Ret::Float),
    func(&["id"], &[p("n", PT::Node)], Ret::NodeOfArg0),
    func(&["labels"], &[p("n", PT::Node)], Ret::KindNames),
    func(&["type"], &[p("e", PT::Edge)], Ret::Text),
    agg(&["count"], Ret::Int),
    agg(&["sum"], Ret::Arg0),
    agg(&["min"], Ret::Arg0),
    agg(&["max"], Ret::Arg0),
    agg(&["avg"], Ret::Float),
    agg(&["collect", "collect_list"], Ret::ListOfArg0),
];

/// The function with this name (any accepted spelling, ASCII case-insensitive). `exists` exists only in the keyword
/// forms the parser normalises ([LQ/grammar-v1.ebnf §P.9]), so any other `exists(...)` is unknown.
pub fn function(name: &str) -> Option<&'static Func> {
    FUNCS
        .iter()
        .find(|f| f.names.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

/// Every function spelling, for did-you-mean.
pub fn function_names() -> Vec<&'static str> {
    FUNCS.iter().flat_map(|f| f.names.iter().copied()).collect()
}

/// The class of a named mutation ([LQ/std §7]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutClass {
    /// A verb mutation (§7.2).
    Verb,
    /// A procedure (§7.3).
    Procedure,
    /// A file named mutation (§7.4): never a `TX` statement (E115).
    File,
}

/// A named mutation.
#[derive(Clone, Copy, Debug)]
pub struct Mutation {
    /// The name without `tx.`.
    pub name: &'static str,
    /// Parameters.
    pub params: &'static [Param],
    /// Yield columns.
    pub yields: &'static [(&'static str, PT)],
    /// The class.
    pub class: MutClass,
}

const fn verb(name: &'static str, params: &'static [Param]) -> Mutation {
    Mutation {
        name,
        params,
        yields: &[],
        class: MutClass::Verb,
    }
}

const fn file(name: &'static str, params: &'static [Param]) -> Mutation {
    Mutation {
        name,
        params,
        yields: &[],
        class: MutClass::File,
    }
}

/// The `tx.*` catalog of [LQ/std §7.2]–§7.4.
pub const MUTATIONS: [Mutation; 21] = [
    verb(
        "add",
        &[
            p("kind", PT::Text),
            p("title", PT::Text),
            o("parent", PT::Node),
            o("blocked_by", PT::ListNode),
            o("blocks", PT::ListNode),
            o("fields", PT::ListText),
            o("body", PT::Text),
        ],
    ),
    verb(
        "set",
        &[
            p("id", PT::Node),
            o("fields", PT::ListText),
            o("status", PT::Text),
            o("done", PT::Bool),
            o("resolution", PT::Text),
            o("if_rev", PT::Int),
            o("if_status", PT::Text),
            o("if_holder", PT::Text),
        ],
    ),
    verb(
        "link",
        &[
            p("a", PT::Node),
            p("kind", PT::Text),
            p("b", PT::Node),
            o("pinned", PT::Text),
        ],
    ),
    verb(
        "unlink",
        &[p("a", PT::Node), p("kind", PT::Text), p("b", PT::Node)],
    ),
    verb("move", &[p("id", PT::Node), p("parent", PT::Node)]),
    verb("reopen", &[p("id", PT::Node), p("reason", PT::Text)]),
    verb("supersede", &[p("old", PT::Node), p("new", PT::Node)]),
    verb(
        "doc_patch",
        &[
            p("section", PT::Node),
            p("old", PT::Text),
            p("new", PT::Text),
            o("depends_on", PT::ListNode),
        ],
    ),
    verb(
        "rm",
        &[
            p("id", PT::Node),
            o("reason", PT::Text),
            o("replaced_by", PT::Node),
            o("policy", PT::Text),
            o("release", PT::Bool),
        ],
    ),
    verb(
        "resolve",
        &[
            o("key", PT::Text),
            p("take", PT::Text),
            o("value", PT::Text),
            o("all", PT::Bool),
        ],
    ),
    verb(
        "remember",
        &[
            p("kind", PT::Text),
            p("title", PT::Text),
            p("text", PT::Text),
            o("fields", PT::ListText),
            o("about", PT::ListNode),
            o("applies_to", PT::ListText),
        ],
    ),
    Mutation {
        name: "complete",
        params: &[
            p("id", PT::Node),
            p("outcome", PT::Text),
            p("summary", PT::Text),
            o("evidence", PT::ListText),
            o("digest", PT::Text),
        ],
        yields: &[
            ("task", PT::Node),
            ("status", PT::Text),
            ("ready", PT::ListNode),
        ],
        class: MutClass::Procedure,
    },
    Mutation {
        name: "claim",
        params: &[
            o("ids", PT::ListNode),
            o("next", PT::Bool),
            o("scope", PT::Node),
            o("role", PT::Text),
            o("agent", PT::Text),
            o("ttl", PT::Text),
            o("start", PT::Bool),
            o("run", PT::Text),
            o("session", PT::Bool),
        ],
        yields: &[
            ("lease", PT::Text),
            ("token", PT::Int),
            ("branch", PT::Text),
            ("expires", PT::Time),
        ],
        class: MutClass::Procedure,
    },
    Mutation {
        name: "heartbeat",
        params: &[p("lease", PT::Text)],
        yields: &[("lease", PT::Text), ("expires", PT::Time)],
        class: MutClass::Procedure,
    },
    Mutation {
        name: "release",
        params: &[p("lease", PT::Text)],
        yields: &[("lease", PT::Text)],
        class: MutClass::Procedure,
    },
    Mutation {
        name: "reclaim",
        params: &[o("older_than", PT::Dur), o("run", PT::Text)],
        yields: &[("lease", PT::Text)],
        class: MutClass::Procedure,
    },
    file(
        "link_file",
        &[
            p("node", PT::Node),
            p("spec", PT::Text),
            o("watch", PT::Text),
            o("planned", PT::Bool),
            o("quote", PT::Text),
            o("end", PT::Text),
        ],
    ),
    file(
        "unlink_file",
        &[
            p("node", PT::Node),
            o("anchor", PT::Text),
            o("path", PT::Text),
        ],
    ),
    file("record_move", &[p("from", PT::Text), p("to", PT::Text)]),
    file(
        "links_fix",
        &[
            p("target", PT::Text),
            p("action", PT::Text),
            o("expect", PT::Text),
            o("to", PT::Text),
            o("at", PT::Text),
            o("same_as", PT::Node),
        ],
    ),
    file(
        "links_sync",
        &[o("scope", PT::Node), o("budget_ms", PT::Int)],
    ),
];

/// The named mutation `tx.<name>` (names after the namespace match exactly, [LQ/lexical §9]).
pub fn mutation(name: &str) -> Option<&'static Mutation> {
    MUTATIONS.iter().find(|m| m.name == name)
}

/// The definitions of the standard library, [LQ/std §4]–§6, byte for byte.
pub const STD_TEXTS: [&str; 40] = [
    r#"DEFINE QUERY ready($scope: node? = NULL, $role: text? = NULL, $limit: int = 20) SHAPE node BUDGET light AS {
  MATCH (t:task)
  WHERE t.ready
    AND ($scope IS NULL OR t IN subtree($scope))
    AND ($role IS NULL OR fits_role(t, $role))
  RETURN t ORDER BY t.priority, t.id LIMIT $limit
}"#,
    r#"DEFINE QUERY blocking($scope: node? = NULL) SHAPE node BUDGET light AS {
  MATCH (t:task)
  WHERE t.is_blocker AND NOT t.settled_elsewhere
    AND ($scope IS NULL OR t IN subtree($scope))
  RETURN t ORDER BY t.id
}"#,
    r#"DEFINE QUERY blockers($id: node, $transitive: bool = false) SHAPE blockers BUDGET light AS {
  CALL blockers($id, transitive: $transitive) YIELD blocker, depth, via, reason, flagged, elsewhere
  RETURN blocker, depth, via, reason, flagged, elsewhere
  ORDER BY depth, blocker
}"#,
    r#"DEFINE QUERY tree($id: node, $depth: int = 3) SHAPE tree BUDGET medium AS {
  CALL subtree($id, depth: $depth) YIELD node, depth, parent, position
  RETURN node, depth, parent, position ORDER BY position
}"#,
    r#"DEFINE QUERY show($ids: list<node>, $full: bool = false) SHAPE detail BUDGET light AS {
  MATCH (n) WHERE n IN $ids RETURN n ORDER BY n.id
}"#,
    r#"DEFINE QUERY find($kind: text? = NULL, $status: text? = NULL, $prio: range<int>? = NULL,
                  $label: text? = NULL, $area: node? = NULL, $text: text? = NULL,
                  $done: bool? = NULL, $suspect: bool? = NULL, $conflicted: bool? = NULL,
                  $limit: int = 50) SHAPE node BUDGET medium AS {
  MATCH (n)
  WHERE ($kind IS NULL OR n.kind = $kind)
    AND ($status IS NULL OR n.status = $status)
    AND ($prio IS NULL OR n.priority IN $prio)
    AND ($label IS NULL OR $label IN n.labels)
    AND ($area IS NULL OR EXISTS { (n)-[:SCOPED_TO]->(a) WHERE a IN subtree($area) })
    AND ($text IS NULL OR text_match(n, $text))
    AND ($done IS NULL OR n.done = $done)
    AND ($suspect IS NULL OR n.suspect = $suspect)
    AND ($conflicted IS NULL OR n.conflicted = $conflicted)
  RETURN n ORDER BY n.id LIMIT $limit
}"#,
    r#"DEFINE QUERY notes($path: text, $role: text? = NULL) SHAPE node BUDGET light AS {
  MATCH (k:note|rule|decision)
  WHERE k.status IN ['active', 'accepted']
    AND (applies(k, $path)
         OR EXISTS { (k)-[:SCOPED_TO]->(a:area) WHERE applies(a, $path) }
         OR EXISTS { (k)-[:AT]->(f:artifact) WHERE f = file($path) })
    AND ($role IS NULL OR applies_role(k, $role))
  RETURN k ORDER BY k.criticality, k.authority, k.id
}"#,
    r#"DEFINE QUERY changes($since: rev, $about: list<node>? = NULL, $for_agent: text? = NULL, $all: bool = false) SHAPE changes BUDGET light AS {
  CALL changes(since: $since) YIELD seq, ref, commit, node, op, aspect, name, actor, affected
  WHERE ($all OR ref = view_ref())
    AND ($about IS NULL OR node IN $about)
    AND ($for_agent IS NULL OR relevant_to(node, $for_agent))
  RETURN seq, ref, node, op, aspect, name, actor ORDER BY seq
}"#,
    r#"DEFINE QUERY stale($scope: node? = NULL) SHAPE node BUDGET medium AS {
  MATCH (n:measurement|note)
  WHERE (n.measured_on IS NOT NULL OR n.observed_git_sha IS NOT NULL)
    AND staleness(n) <> 'fresh'
    AND ($scope IS NULL OR EXISTS { (n)-[:ABOUT|SCOPED_TO]->(x) WHERE x IN subtree($scope) })
  RETURN n ORDER BY n.id
}"#,
    r#"DEFINE QUERY conflicts($scope: node? = NULL) SHAPE conflict BUDGET light AS {
  CALL conflicts() YIELD key, node, class, base, ours, theirs, commit, hint
  WHERE $scope IS NULL OR node IN subtree($scope)
  RETURN key, node, class, base, ours, theirs, commit, hint ORDER BY node, key
}"#,
    r#"DEFINE QUERY violations($ref: rev) SHAPE violation BUDGET light AS {
  USE $ref
  CALL violations() YIELD key, class, detail, suggested
  RETURN key, class, detail, suggested ORDER BY key
}"#,
    r#"DEFINE QUERY history($id: node, $field: text? = NULL, $range: rev? = NULL) SHAPE history BUDGET light AS {
  CALL history($id, field: $field, in: $range)
  YIELD seq, commit, ref, actor, role, at, op, aspect, name, before, after, message, via
  RETURN seq, commit, ref, actor, role, at, op, aspect, name, before, after, message, via
  ORDER BY seq DESC
}"#,
    r#"DEFINE QUERY blame($id: node, $field: text? = NULL) SHAPE table BUDGET light AS {
  CALL blame($id) YIELD aspect, name, value, seq, commit, actor, at
  WHERE $field IS NULL OR name = $field
  RETURN aspect, name, value, seq AS rev, commit, actor ORDER BY aspect, name
}"#,
    r#"DEFINE QUERY log($range: rev? = NULL, $actor: text? = NULL, $touching: node? = NULL) SHAPE history BUDGET light AS {
  CALL log($range, actor: $actor, touching: $touching)
  YIELD commit, seq, ref, kind, actor, role, at, message, ops
  RETURN seq, commit, ref, actor, role, at, kind, message, ops ORDER BY seq DESC
}"#,
    r#"DEFINE QUERY diff($range: rev, $scope: node? = NULL, $aspect: text? = NULL, $side: text? = NULL) SHAPE diff BUDGET medium AS {
  CALL diff($range, scope: $scope)
  YIELD change, node, kind, aspect, name, before, after, side, last_commit, actor
  WHERE ($aspect IS NULL OR aspect = $aspect) AND ($side IS NULL OR side = $side)
  RETURN change, node, kind, aspect, name, before, after, side, last_commit, actor
  ORDER BY node, aspect, name
}"#,
    r#"DEFINE QUERY across($refs: list<rev>, $ids: list<node>) SHAPE across BUDGET medium AS {
  CALL across(refs: $refs, ids: $ids) YIELD node, aspect, name, ref, value, diverged
  RETURN node, aspect, name, ref, value, diverged ORDER BY node, name, ref
}"#,
    r#"DEFINE QUERY loop($plan: node, $round: int? = NULL) SHAPE loop BUDGET light AS {
  MATCH (f:finding)
  WHERE EXISTS { (f)-[:ABOUT]->(s) WHERE s IN subtree($plan) }
    AND ($round IS NULL OR f.round = $round)
  RETURN f.round AS round,
         count(*) AS raised,
         count(CASE WHEN f.status = 'confirmed' THEN 1 END) AS confirmed,
         count(CASE WHEN f.status = 'refuted' THEN 1 END) AS refuted,
         count(CASE WHEN f.status = 'confirmed' AND f.severity IN ['blocker', 'important'] THEN 1 END) AS blocking
  ORDER BY round
}"#,
    r#"DEFINE QUERY refuted_share($role: text, $round: int? = NULL) SHAPE table BUDGET light AS {
  MATCH (f:finding) WHERE f.created_role = $role AND ($round IS NULL OR f.round = $round)
  WITH f.round AS round, count(*) AS raised, count(CASE WHEN f.status = 'refuted' THEN 1 END) AS refuted
  RETURN round, raised, refuted, round(100.0 * refuted / raised, 1) AS pct ORDER BY round
}"#,
    r#"DEFINE QUERY lane_conflicts($a: node, $b: node) SHAPE table BUDGET medium AS {
  MATCH (la:lane {id: $a}), (lb:lane {id: $b}), (ta:task), (tb:task)
  WHERE ta.claimed AND tb.claimed AND ta <> tb
    AND ta.lease.branch = la.moirai_branch AND tb.lease.branch = lb.moirai_branch
  UNWIND ta.files_owned AS ga
  UNWIND tb.files_owned AS gb
  WITH ta, tb, ga, gb WHERE glob_match(ga, gb) OR glob_match(gb, ga)
  RETURN ga AS path, ta, gb AS other, tb ORDER BY path, ta, other, tb
}"#,
    r#"DEFINE QUERY delta($since: rev, $agent: text) SHAPE changes BUDGET light AS {
  CALL changes(since: $since) YIELD seq, ref, node, op, aspect, name, actor
  WHERE relevant_to(node, $agent) AND actor <> $agent
  RETURN seq, ref, node, op, aspect, name, actor ORDER BY seq LIMIT 12
}"#,
    r#"DEFINE QUERY links_broken($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (n)-[a:AT]->(f)
  WHERE ($scope IS NULL OR n IN subtree($scope)) AND link_state(a) <> 'ok'
  RETURN f, a, link_state(a)
}"#,
    r#"DEFINE QUERY links_pending($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (n)-[a:AT]->(f)
  WHERE ($scope IS NULL OR n IN subtree($scope)) AND link_state(a) = 'pending'
  RETURN f, a, link_state(a)
}"#,
    r#"DEFINE QUERY links_proposals($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (n)-[a:AT]->(f)
  WHERE ($scope IS NULL OR n IN subtree($scope))
    AND link_state(a) IN ['moved-needs-confirm', 'ambiguous']
  RETURN f, a, link_state(a)
}"#,
    r#"DEFINE QUERY links_guesses($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (f:artifact)
  WHERE (f.relink STARTS WITH 'agent/' OR f.relink STARTS WITH 'policy/')
    AND ($scope IS NULL OR EXISTS { (n)-[:AT]->(f) WHERE n IN subtree($scope) })
  RETURN f ORDER BY f.id
}"#,
    r#"DEFINE QUERY files_removed($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (f:artifact)
  WHERE f.status = 'removed'
    AND ($scope IS NULL OR EXISTS { (n)-[:AT]->(f) WHERE n IN subtree($scope) })
  RETURN f ORDER BY f.id
}"#,
    r#"DEFINE QUERY files_replaced($scope: node? = NULL) SHAPE links BUDGET medium AS {
  MATCH (f:artifact)
  WHERE link_state(f) = 'replaced'
    AND ($scope IS NULL OR EXISTS { (n)-[:AT]->(f) WHERE n IN subtree($scope) })
  RETURN f ORDER BY f.id
}"#,
    r#"DEFINE QUERY root_moves($root: text = 'project') SHAPE table BUDGET light AS {
  CALL root_moves($root) YIELD hlc, class, from, to, git
  RETURN hlc, class, from, to, git ORDER BY hlc, from, to
}"#,
    r#"DEFINE QUERY pack_header($target: node) SHAPE table BUDGET light AS {
  CALL refs() YIELD name, kind, tip, seq, ahead, behind, fork, staged
  WHERE name = view_ref() OR (kind = 'merge' AND name CONTAINS view_ref())
  RETURN 'ref' AS item, name AS key, ahead, behind, staged, NULL AS n
  UNION ALL
  CALL links(scope: $target) YIELD state
  RETURN 'links' AS item, state AS key, NULL AS ahead, NULL AS behind, NULL AS staged, count(*) AS n
}"#,
    r#"DEFINE QUERY pack_rules($role: text, $phase: text? = NULL) SHAPE node BUDGET light AS {
  MATCH (r:rule)
  WHERE r.status = 'active'
    AND (applies_role(r, $role) OR ($phase IS NOT NULL AND applies_phase(r, $phase)))
  RETURN r ORDER BY r.criticality, r.authority, r.id
}"#,
    r#"DEFINE QUERY pack_rules_unmerged($role: text) SHAPE node BUDGET light AS {
  USE main
  CALL diff(HEAD...main) YIELD node, side
  WHERE side IN ['theirs', 'both']
  MATCH (r:rule)
  WHERE r = node AND r.status = 'active' AND r.criticality = 'critical' AND applies_role(r, $role)
  RETURN DISTINCT r ORDER BY r.criticality, r.authority, r.id
}"#,
    r#"DEFINE QUERY pack_target($target: node) SHAPE node BUDGET medium AS {
  MATCH (t) WHERE t = $target
  RETURN t AS node, 'target' AS why
  UNION
  MATCH (a) WHERE a IN ancestors($target)
  RETURN a AS node, 'ancestor' AS why
  UNION
  MATCH (q:question)-[:BLOCKS]->(t) WHERE t = $target AND q.unfinished
  RETURN q AS node, 'question' AS why
  UNION
  MATCH (k)-[:ABOUT]->(x) WHERE x IN subtree($target) AND k.authority = 'owner'
  RETURN k AS node, 'ruling' AS why
  UNION
  MATCH (t)-[:AT]->(f:artifact) WHERE t = $target
  RETURN f AS node, 'link' AS why
}"#,
    r#"DEFINE QUERY pack_spec($target: node, $role: text, $round: int? = NULL) SHAPE node BUDGET medium AS {
  MATCH (d:doc)
  WHERE d = $target OR EXISTS { MATCH (t)-[:IMPLEMENTS|ABOUT]->(d) WHERE t = $target }
  MATCH (s:doc) WHERE s IN subtree(d)
  RETURN DISTINCT s AS node, $round IS NOT NULL AND coalesce(s.changed_in_round, 0) > $round AS changed
  UNION
  MATCH (d:doc)
  WHERE d = $target OR EXISTS { MATCH (t)-[:IMPLEMENTS|ABOUT]->(d) WHERE t = $target }
  MATCH (s:doc) WHERE s IN subtree(d) AND $round IS NOT NULL AND coalesce(s.changed_in_round, 0) > $round
  MATCH (dep:doc)-[:DEPENDS_ON]->(s)
  RETURN DISTINCT dep AS node, true AS changed
}"#,
    r#"DEFINE QUERY pack_findings($target: node, $role: text, $round: int? = NULL) SHAPE node BUDGET light AS {
  MATCH (f:finding)
  WHERE EXISTS { MATCH (f)-[:ABOUT]->(x) WHERE x IN subtree($target) }
    AND CASE $role
          WHEN 'developer' THEN f.status = 'confirmed'
          WHEN 'architecture-critic' THEN f.created_role = $role
                                          AND ($round IS NULL OR coalesce(f.round, 0) <= $round)
          WHEN 'code-reviewer' THEN f.created_role = $role OR f.status = 'open'
          ELSE f.status IN ['open', 'confirmed']
        END
  RETURN f ORDER BY f.severity, f.id
}"#,
    r#"DEFINE QUERY pack_measurements($lane: node) SHAPE node BUDGET light AS {
  MATCH (m:measurement)
  WHERE m.status = 'current'
    AND NOT EXISTS { MATCH (r:run)-[:PRODUCED]->(m), (r)-[:RUNS_IN]->(l:lane) WHERE l <> $lane }
  RETURN m, staleness(m) AS staleness ORDER BY m.id
}"#,
    r#"DEFINE QUERY pack_hazards($target: node) SHAPE node BUDGET medium AS {
  MATCH (t:task) WHERE t = $target
  UNWIND t.files_owned AS g
  MATCH (k:note|rule)
  WHERE k.status = 'active' AND coalesce(size(k.applies_to), 0) > 0 AND applies(k, g)
  RETURN DISTINCT k AS node
  UNION
  MATCH (t:task) WHERE t = $target
  UNWIND t.files_owned AS g
  MATCH (k:note|rule)-[:AT]->(f:artifact)
  WHERE k.status = 'active' AND glob_match(f.path, g)
  RETURN DISTINCT k AS node
}"#,
    r#"DEFINE QUERY brief_lanes() SHAPE node BUDGET light AS {
  MATCH (l:lane) WHERE l.status IN ['active', 'ready_to_merge', 'merge_pending', 'measuring', 'frozen']
  RETURN l AS node, 'lane' AS why
  UNION
  MATCH (r:run) WHERE r.status = 'running'
  RETURN r AS node, 'run' AS why
  UNION
  MATCH (a:lane)-[:MERGE_AFTER]->(b:lane) WHERE a.status = 'merge_pending'
  RETURN a AS node, 'merge queue' AS why
}"#,
    r#"DEFINE QUERY brief_triage() SHAPE node BUDGET light AS {
  MATCH (t:task) WHERE t.settled_elsewhere OR t.deleted_elsewhere OR t.has_dangling RETURN t
}"#,
    r#"DEFINE QUERY brief_questions() SHAPE node BUDGET light AS {
  MATCH (q:question) WHERE q.status = 'open' AND q.asked_of = 'owner'
  RETURN q ORDER BY q.criticality, q.id
}"#,
    r#"DEFINE QUERY brief_critical() SHAPE node BUDGET light AS {
  MATCH (k:rule|note) WHERE k.status = 'active' AND k.criticality = 'critical'
  RETURN k ORDER BY k.authority, k.id
}"#,
    r#"DEFINE QUERY brief_verdicts($since: rev) SHAPE node BUDGET light AS {
  MATCH (v:verdict) WHERE v.created > $since
  RETURN v ORDER BY v.created, v.id
}"#,
];

/// A standard or project named query, ready to be called ([LQ/std §2]).
#[derive(Clone, Debug)]
pub struct NamedQuery {
    /// `std.<name>` or the project qname.
    pub qname: String,
    /// The parameters: (name, type, required).
    pub params: Vec<(String, PT, bool)>,
    /// The columns of its result: (name, type).
    pub columns: Vec<(String, Ty)>,
    /// The bound class: `live` when it reads runtime or tree-derived state ([LQ/std §2.5]).
    pub live: bool,
    /// The definition's S-AST.
    pub define: Define,
    /// The definition's C-AST (bound against the core schema).
    pub cast: CDefine,
}

/// Builds a named query from its definition text; `None` when it does not parse or bind.
pub fn named_query(qname: &str, text: &str, schema: &Schema) -> Option<NamedQuery> {
    named_query_checked(qname, text, schema).ok()
}

/// [`named_query`] with the diagnostics of a definition that does not parse or bind. A definition that calls a project
/// query binds the callee's definition to learn its columns; a call cycle is refused there ([F19] `QueryCycle`).
pub fn named_query_checked(
    qname: &str,
    text: &str,
    schema: &Schema,
) -> Result<NamedQuery, Vec<Diag>> {
    let d = parse_define(text, ParseOptions::default())?.tree;
    let ids = MapIds::new();
    let prm = Params::new();
    let caller = Caller::default();
    let ctx = BindCtx {
        schema,
        ids: &ids,
        params: &prm,
        caller: &caller,
    };
    let b: Bound<CDefine> = bind::bind_define(&ctx, text, &d)?;
    let mut params = Vec::new();
    for pd in &d.params {
        let ty = PT::from_decl(
            &pd.ty.name.text,
            pd.ty.arg.as_ref().map(|a| a.text.as_str()),
        )
        .unwrap_or(PT::Any);
        params.push((
            pd.name.text.clone(),
            ty,
            !pd.optional && pd.default.is_none(),
        ));
    }
    Ok(NamedQuery {
        qname: qname.to_string(),
        params,
        columns: b.columns,
        live: b.live,
        define: d,
        cast: b.ast,
    })
}

/// The standard library, parsed and bound against the core schema once.
pub fn std_catalog() -> &'static [NamedQuery] {
    static CATALOG: OnceLock<Vec<NamedQuery>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let schema = Schema::core();
        STD_TEXTS
            .iter()
            .map(|t| {
                let name = t["DEFINE QUERY ".len()..]
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .to_string();
                named_query_checked(&format!("std.{name}"), t, &schema).unwrap_or_else(|e| {
                    panic!(
                        "std.{name} does not bind: {}",
                        e.iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    )
                })
            })
            .collect()
    })
}

/// The standard query `std.<name>` (exact match after the namespace).
pub fn std_query(name: &str) -> Option<&'static NamedQuery> {
    std_catalog()
        .iter()
        .find(|q| q.qname.strip_prefix("std.") == Some(name))
}
