//! The built-in relations ([LQ/std §2.9], §2.12; [50 §2.6]) and calls of named queries ([LQ/std §2.1]): graph
//! relations over the view, runtime relations over the store's tables at a tip, tree-derived link rows from the model
//! of [40]'s resolver, and the catalog relations. The history relations are in [`super::hist`].

use super::expr::Env;
use super::query::Frame;
use super::val::{EdgeV, V};
use super::view::Ev;
use crate::err::{Refusal, Res};
use crate::lease::{self, LeaseKind};
use crate::lq::cast::{CArg, CExpr};
use crate::lq::catalog::{self, RELATIONS};
use crate::r4::strings::State as LState;
use crate::state::{Alloc, Aspect, KVal, Key};
use crate::value::{Nid, Uid, Value, hex};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// A relation's rows: its column names and its rows in the relation's declared order.
#[derive(Clone, Debug, Default)]
pub struct RelOut {
    /// The columns.
    pub cols: Vec<String>,
    /// The rows.
    pub rows: Vec<Vec<V>>,
}

/// One link of a node ([40 §6.1]): its `AT` edge, file, path, anchor and the states the resolver gives it.
#[derive(Clone, Debug)]
pub struct LinkRow {
    /// The `AT` edge.
    pub edge: EdgeV,
    /// The anchor handle `a<n>`.
    pub handle: Option<String>,
    /// The file node.
    pub file: Nid,
    /// The file's recorded path.
    pub path: String,
    /// The anchor kind, or `file`.
    pub kind: String,
    /// The anchor's scope text.
    pub scope: String,
    /// The link state ([F18 §4.4]).
    pub state: String,
    /// The same as a value.
    pub lstate: LState,
    /// The anchor-level state, or `unresolved` ([50 §2.6]).
    pub anchor_state: String,
    /// The link state in its qualified form ([F18 §4.7] rule 3).
    pub evidence: String,
    /// The next command: the evidence command of a proposal, the settle command of an exact state ([LQ/std §2.8]
    /// item 2); absent for `ok`.
    pub next: Option<String>,
}

/// The links of one node.
#[derive(Clone, Debug, Default)]
pub struct NodeLinks {
    /// One row per anchor of every `AT` edge, in edge order.
    pub rows: Vec<LinkRow>,
}

fn cols(c: &[&str]) -> Vec<String> {
    c.iter().map(|s| s.to_string()).collect()
}

fn opt_node(n: Option<Nid>) -> V {
    n.map_or(V::Absent, V::Node)
}

fn text_or_absent(s: &str) -> V {
    if s.is_empty() { V::Absent } else { V::text(s) }
}

impl Ev<'_> {
    /// A `CALL` ([LQ/std §2.1]): a built-in relation first, then `std.<name>`, then the project catalog.
    // spec: [LQ/std §2.1]
    pub fn call(&self, proc: &str, args: &[CArg], env: &Env<'_>) -> Res<RelOut> {
        if catalog::relation(proc).is_some() && !proc.contains('.') {
            return self.relation(proc, args, env);
        }
        self.named(proc, args, env)
    }

    /// A named query called with its arguments by name ([LQ/canonical-ast §5.8] N2): its parameters take the arguments,
    /// else their defaults, else absent; its parts without `USE` read the calling part's view, sharing its evaluation
    /// context ([50 §3.9] item 5). A project query's definition is the one the binder resolved, at the default view
    /// ([`super::view::World::catalog`]), bound against the part's view; a definition that does not bind there is the
    /// binder's E109 (`QueryInvalid`).
    // spec: [50 §3.9] item 5
    fn named(&self, qname: &str, args: &[CArg], env: &Env<'_>) -> Res<RelOut> {
        let (def, columns, names) = match qname.strip_prefix("std.") {
            Some(n) => {
                let q = catalog::std_query(n)
                    .ok_or_else(|| Refusal::lq("E109", format!("no standard query {n}")))?;
                (
                    q.cast.clone(),
                    q.columns.iter().map(|c| c.0.clone()).collect(),
                    q.names.clone(),
                )
            }
            None => {
                let defs = self.w.catalog.as_deref().unwrap_or(self.st());
                let item = defs.schema.query(qname).cloned().ok_or_else(|| {
                    Refusal::lq(
                        "E109",
                        format!("unknown function {}", crate::lq::diag::q(qname)),
                    )
                })?;
                let q = super::bind_project_query(self.w.store, self.st(), qname, &item.text)?;
                (
                    q.cast,
                    q.columns.iter().map(|c| c.0.clone()).collect(),
                    q.names,
                )
            }
        };
        let given = self.args(args, env)?;
        let mut frame = Frame::default();
        for pd in &def.params {
            match given.iter().find(|(n, _, _)| *n == pd.name) {
                Some((_, v, rev)) => {
                    frame.vals.push(v.clone());
                    frame.revs.push(rev.clone());
                }
                None => {
                    let v = match &pd.default {
                        Some(d) => self.eval(d, &Env::of(&Default::default(), &[]))?,
                        None => V::Absent,
                    };
                    frame.vals.push(v);
                    frame.revs.push(None);
                }
            }
        }
        self.w.names.borrow_mut().push(Rc::new(names));
        let t = self.w.eval_query(&def.body, &frame, Some(self));
        self.w.names.borrow_mut().pop();
        Ok(RelOut {
            cols: columns,
            rows: t?.rows,
        })
    }

    /// A built-in relation by its registry name.
    fn relation(&self, proc: &str, args: &[CArg], env: &Env<'_>) -> Res<RelOut> {
        let r = RELATIONS
            .iter()
            .find(|r| r.name == proc)
            .expect("a registered relation");
        let given = self.args(args, env)?;
        // Positional arguments bind the parameters in declaration order (N2 names them; a missing name is the first).
        let get = |name: &str| -> (V, Option<CExpr>) {
            let pos = r.params.iter().position(|p| p.name == name);
            given
                .iter()
                .enumerate()
                .find(|(i, (n, _, _))| n == name || (n.is_empty() && Some(*i) == pos))
                .map_or((V::Absent, None), |(_, (_, v, rev))| {
                    (v.clone(), rev.clone())
                })
        };
        let v = |name: &str| get(name).0;
        let names: Vec<&str> = r.yields.iter().map(|(n, _)| *n).collect();
        let rows = match proc {
            "blockers" => match v("n").node() {
                Some(n) => self.blockers(n, v("transitive").as_bool().unwrap_or(false)),
                None => Vec::new(),
            },
            "subtree" => match v("n").node() {
                Some(n) => self.subtree_rows(n, v("depth").as_int().unwrap_or(3)),
                None => Vec::new(),
            },
            "neighbors" => match v("n").node() {
                Some(n) => {
                    let types: Option<Vec<String>> = match v("types") {
                        V::List(l) => Some(
                            l.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect(),
                        ),
                        _ => None,
                    };
                    self.neighbors(n, v("depth").as_int().unwrap_or(1), types.as_deref())
                }
                None => Vec::new(),
            },
            "search" => {
                let kinds: Option<Vec<String>> = match v("kinds") {
                    V::List(l) => Some(
                        l.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect(),
                    ),
                    _ => None,
                };
                let fields: Vec<String> = match v("fields") {
                    V::List(l) => l
                        .iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect(),
                    _ => vec!["title".into(), "abstract".into()],
                };
                match v("terms") {
                    V::Text(t) => super::search::search(self, &t, kinds.as_deref(), &fields),
                    _ => Vec::new(),
                }
            }
            "history" => match v("n").node() {
                Some(n) => {
                    let field = v("field");
                    self.history_rows(n, field.as_str(), get("in").1.as_ref())?
                }
                None => Vec::new(),
            },
            "blame" => match v("n").node() {
                Some(n) => self.blame_rows(n),
                None => Vec::new(),
            },
            "log" => {
                let actor = v("actor");
                self.log_rows(
                    get("range").1.as_ref(),
                    actor.as_str(),
                    v("touching").node(),
                )?
            }
            "diff" => match get("range").1 {
                Some(range) => self.diff_rows(&range, v("scope").node())?,
                None => return Err(Refusal::lq("E110", "diff() needs a revision range")),
            },
            "changes" => self.changes_rows(&get("since"), get("ref").1.as_ref())?,
            "conflicts" => self.conflict_rows(),
            "violations" => self.violation_rows(get("ref").1.as_ref())?,
            "across" => {
                let ids: Vec<Nid> = v("ids").elems().iter().filter_map(V::node).collect();
                let aspects: Option<Vec<String>> = match v("aspects") {
                    V::List(l) => Some(
                        l.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect(),
                    ),
                    _ => None,
                };
                self.across_rows(get("refs").1.as_ref(), &ids, aspects.as_deref())?
            }
            "refs" => self.refs_rows(),
            "leases" => self.runtime_rows(true)?,
            "markers" => self.runtime_rows(false)?,
            "links" => self.links_rows(v("scope").node())?,
            "root_moves" => {
                let root = match v("root") {
                    V::Text(t) => t,
                    _ => "project".into(),
                };
                self.root_moves(&root)
            }
            "schema" => self.schema_rows(v("kind").as_str()),
            "schema_edges" => self.schema_edge_rows(),
            _ => self.query_rows(),
        };
        Ok(RelOut {
            cols: cols(&names),
            rows,
        })
    }

    /// The blockers of one task that count in `open_blockers` ([RULES/state-definition] `blocker-terms`): direct
    /// in-edges of `blocks` and `gates`, then the exogenous blockers of its ancestors, nearest first (inherited).
    fn direct_and_inherited(&self, t: Nid) -> Vec<(Nid, Option<Nid>, &'static str, bool)> {
        let mut out = Vec::new();
        let mut push = |src: Nid, via: Option<Nid>, reason: &'static str, flagged: bool| {
            if !out
                .iter()
                .any(|(s, _, _, _): &(Nid, Option<Nid>, &str, bool)| *s == src)
            {
                out.push((src, via, reason, flagged));
            }
        };
        let mut direct: Vec<(Nid, bool)> = Vec::new();
        for kind in ["blocks", "gates"] {
            for (src, _, p) in self.ix().in_edges(t, kind) {
                if crate::derived::term_weight(self.ix(), kind, src, p, "open_blockers") > 0 {
                    direct.push((src, p.flagged));
                }
            }
        }
        direct.sort();
        for (s, f) in direct {
            push(s, None, "direct", f);
        }
        for a in self.ix().ancestors(t) {
            if crate::derived::open_blockers_exo(self.ix(), a) == 0 {
                continue;
            }
            let mut inh: Vec<(Nid, bool)> = Vec::new();
            for kind in ["blocks", "gates"] {
                for (src, _, p) in self.ix().in_edges(a, kind) {
                    if (p.flagged || !self.ix().in_subtree(src, a))
                        && crate::derived::term_weight(self.ix(), kind, src, p, "open_blockers") > 0
                    {
                        inh.push((src, p.flagged));
                    }
                }
            }
            inh.sort();
            for (s, f) in inh {
                push(s, Some(a), "inherited", f);
            }
        }
        out
    }

    /// `blockers(n, transitive)` ([50 §2.6]; [AR §3.5]): one row per blocker at its smallest depth, `via` the ancestor
    /// an inherited blocker blocks, or the blocked node of a deeper blocker; `elsewhere` the I26′ runtime flag at a tip.
    // spec: [50 §2.6] blockers
    fn blockers(&self, n: Nid, transitive: bool) -> Vec<Vec<V>> {
        let mut rows = Vec::new();
        let mut seen = BTreeSet::from([n]);
        let mut frontier = vec![n];
        let mut depth = 1;
        loop {
            let mut next = Vec::new();
            for t in &frontier {
                for (b, via, reason, flagged) in self.direct_and_inherited(*t) {
                    if !seen.insert(b) {
                        continue;
                    }
                    let via = match (via, depth) {
                        (Some(a), _) => Some(a),
                        (None, 1) => None,
                        (None, _) => Some(*t),
                    };
                    rows.push(vec![
                        V::Node(b),
                        V::Int(depth),
                        opt_node(via),
                        V::text(reason),
                        V::Bool(flagged),
                        V::Bool(self.settled_elsewhere(b)),
                    ]);
                    if self.st().live(b).is_some() {
                        next.push(b);
                    }
                }
            }
            if !transitive || next.is_empty() {
                break;
            }
            frontier = next;
            depth += 1;
        }
        rows
    }

    /// The live children of a node in sibling order: by the `order` key, then by id.
    fn ordered_children(&self, p: Nid) -> Vec<Nid> {
        let mut v = self.ix().children.get(&p).cloned().unwrap_or_default();
        v.sort_by(|a, b| {
            let ka = self.node(*a).and_then(|x| x.order.clone());
            let kb = self.node(*b).and_then(|x| x.order.clone());
            (ka.is_none(), ka, *a).cmp(&(kb.is_none(), kb, *b))
        });
        v
    }

    /// `subtree(n, depth)` ([50 §2.6]): the node at depth 0 and its descendants within `depth` levels in preorder,
    /// siblings in order; `position` is the preorder index. The hierarchy is a forest (I4), so each node is visited
    /// once; budgets are not modelled ([60 §4.2]: GT9 checks them).
    // spec: [50 §2.6] subtree
    fn subtree_rows(&self, n: Nid, depth: i64) -> Vec<Vec<V>> {
        let mut rows = Vec::new();
        if self.st().live(n).is_none() {
            return rows;
        }
        let mut seen = BTreeSet::from([n]);
        let mut stack = vec![(n, 0i64, None::<Nid>)];
        while let Some((x, d, p)) = stack.pop() {
            rows.push(vec![
                V::Node(x),
                V::Int(d),
                opt_node(p),
                V::Int(rows.len() as i64),
            ]);
            if d < depth {
                for c in self.ordered_children(x).into_iter().rev() {
                    if seen.insert(c) {
                        stack.push((c, d + 1, Some(x)));
                    }
                }
            }
        }
        rows
    }

    /// `neighbors(n, depth, types)` ([50 §2.6]): the nodes within `depth` edges in either direction over the given
    /// kinds (LQ names; every kind when absent), each once at its smallest depth with the edge that reached it first.
    // spec: [50 §2.6] neighbors
    fn neighbors(&self, n: Nid, depth: i64, types: Option<&[String]>) -> Vec<Vec<V>> {
        let ep = crate::lq::cast::CEdgeP {
            var: None,
            dir: 3,
            types: Vec::new(),
            quant: None,
            props: Vec::new(),
            where_: None,
        };
        let mut seen = BTreeSet::from([n]);
        let mut frontier = vec![n];
        let mut rows = Vec::new();
        let mut d = 0;
        while d < depth && !frontier.is_empty() {
            d += 1;
            let mut next = Vec::new();
            for x in &frontier {
                for (e, other) in self.edges_from(*x, &ep) {
                    let lq = self.lq_name(&e.kind);
                    if types.is_some_and(|t| !t.iter().any(|k| k.eq_ignore_ascii_case(&lq))) {
                        continue;
                    }
                    if self.st().live(other).is_none() || !seen.insert(other) {
                        continue;
                    }
                    let dir = if e.src == *x { "out" } else { "in" };
                    rows.push(vec![V::Node(other), V::text(lq), V::text(dir), V::Int(d)]);
                    next.push(other);
                }
            }
            frontier = next;
        }
        rows
    }

    /// The value of a key value as a `diff` row shows it ([API §5.7]).
    // spec: [API §5.7]
    pub fn kval_v(&self, kind: &str, aspect: &Aspect, v: &KVal) -> V {
        match (aspect, v) {
            (_, KVal::Live(_)) => V::text("live"),
            (_, KVal::Deleted { .. }) => V::text("deleted"),
            (_, KVal::Status { status, resolution }) => V::Map(vec![
                ("status".into(), self.enum_v(kind, "status", status)),
                (
                    "resolution".into(),
                    self.enum_v(kind, "resolution", resolution),
                ),
            ]),
            (_, KVal::Hierarchy { parent, order }) => V::Map(vec![
                ("parent".into(), opt_node(*parent)),
                ("order".into(), order.clone().map_or(V::Absent, V::Text)),
            ]),
            (Aspect::Field(f) | Aspect::Counter(f), KVal::Value(x)) => self.value(kind, f, x),
            (_, KVal::Value(x)) => self.value(kind, "", x),
            (_, KVal::Observation(vs)) => V::Map(
                crate::state::OBSERVATION
                    .iter()
                    .zip(vs)
                    .map(|(f, x)| {
                        (
                            f.to_string(),
                            x.as_ref().map_or(V::Absent, |x| self.value(kind, f, x)),
                        )
                    })
                    .collect(),
            ),
            (_, KVal::Body(b)) => V::text(hex(&crate::canon::b3_128(b.as_bytes()))),
            (Aspect::Edge(k), KVal::Edge(p)) => {
                let mut props = Vec::new();
                if p.flagged {
                    props.push(("flagged".to_string(), V::Bool(true)));
                }
                if let Some(c) = p.pinned {
                    props.push(("pinned".to_string(), V::text(format!("c{}", hex(&c)))));
                }
                if p.anchor.is_some() {
                    props.push((
                        "anchor".to_string(),
                        k.disc
                            .and_then(|u| self.store().files.anchors.get(&u))
                            .map_or(V::Absent, |h| V::text(format!("a{h}"))),
                    ));
                }
                V::Map(vec![
                    ("dst".into(), V::Node(k.dst)),
                    (
                        "disc".into(),
                        k.disc.map_or(V::Absent, |u| V::text(u.hex())),
                    ),
                    ("props".into(), V::Map(props)),
                ])
            }
            (_, KVal::Item(it)) => V::text(it.key().text()),
            (_, KVal::Edge(_)) => V::Absent,
        }
    }

    /// The value of a key state ([API §5.7]): a conflict as `{conflict, base, ours, theirs}`.
    pub fn kstate_v(&self, kind: &str, aspect: &Aspect, k: &crate::state::KState) -> V {
        match k {
            crate::state::KState::Plain(None) => V::Absent,
            crate::state::KState::Plain(Some(v)) => self.kval_v(kind, aspect, v),
            crate::state::KState::Conflict(c) => {
                let side = |s: &Option<KVal>| {
                    s.as_ref()
                        .map_or(V::Absent, |v| self.kval_v(kind, aspect, v))
                };
                V::Map(vec![
                    ("conflict".into(), V::text(c.class.clone())),
                    ("base".into(), side(&c.base)),
                    ("ours".into(), side(&c.ours)),
                    ("theirs".into(), side(&c.theirs)),
                ])
            }
        }
    }

    /// The text of a key in a result ([API §5.3]; an `at` edge with its anchor handle).
    pub fn key_text(&self, k: &Key) -> String {
        let t = crate::merge::key_text(k, &|n| self.store().alloc.uid(n));
        match k {
            Key::Node(_, Aspect::Edge(e)) => {
                match e.disc.and_then(|u| self.store().files.anchors.get(&u)) {
                    Some(h) => format!("{t}:a{h}"),
                    None => t,
                }
            }
            _ => t,
        }
    }

    /// `conflicts()` ([50 §2.6]; [F12 §6]): the unresolved conflict values of the view, by node then key.
    // spec: [50 §2.6] conflicts
    fn conflict_rows(&self) -> Vec<Vec<V>> {
        let mut rows = Vec::new();
        let dag = &self.store().dag;
        let introduced = |k: &Key| -> V {
            dag.chain(self.v.commit)
                .into_iter()
                .find(|c| {
                    dag.commits[c]
                        .changeset
                        .get(k)
                        .is_some_and(|(_, a)| matches!(a, crate::state::KState::Conflict(_)))
                })
                .map_or(V::Absent, V::Rev)
        };
        for (n, x) in &self.st().nodes {
            for (a, c) in &x.conflicts {
                let k = Key::Node(*n, a.clone());
                let side =
                    |s: &Option<KVal>| s.as_ref().map_or(V::Absent, |v| self.kval_v(&x.kind, a, v));
                let text = self.key_text(&k);
                rows.push(vec![
                    V::text(text.clone()),
                    V::Node(*n),
                    V::text(c.class.clone()),
                    side(&c.base),
                    side(&c.ours),
                    side(&c.theirs),
                    introduced(&k),
                    V::text(format!("RESOLVE '{text}' TAKE OURS|THEIRS|BASE")),
                ]);
            }
        }
        for (ik, c) in &self.st().schema_conflicts {
            let k = Key::Schema(ik.clone());
            let side = |s: &Option<KVal>| {
                s.as_ref()
                    .map_or(V::Absent, |v| self.kval_v("", &Aspect::Existence, v))
            };
            let text = ik.text();
            rows.push(vec![
                V::text(text.clone()),
                V::Absent,
                V::text(c.class.clone()),
                side(&c.base),
                side(&c.ours),
                side(&c.theirs),
                introduced(&k),
                V::text(format!("RESOLVE '{text}' TAKE OURS|THEIRS|BASE")),
            ]);
        }
        // The order of `std.conflicts` ([LQ/std §4.10]): node (schema keys last), then key.
        rows.sort_by(|a, b| {
            super::val::cmp_total(&a[1], &b[1]).then_with(|| super::val::cmp_total(&a[0], &b[0]))
        });
        rows
    }

    /// `root_moves(root)` ([40 §2.4]): the root node's `path_moves` entries, by (`hlc`, `from`, `to`).
    // spec: [LQ/std §2.12] root_moves
    fn root_moves(&self, root: &str) -> Vec<Vec<V>> {
        let mut rows = Vec::new();
        for x in self.st().nodes.values() {
            if !x.live() || x.text("root") != Some(root) {
                continue;
            }
            if let Some(ms) = x.fields.get("path_moves") {
                for m in ms.elems() {
                    if let Value::PathMove(m) = m {
                        rows.push(vec![
                            V::Int(m.hlc as i64),
                            V::text(crate::canon::move_class_name(m.class)),
                            V::text(crate::links::path_text(&m.from)),
                            V::text(crate::links::path_text(&m.to)),
                            m.git.as_ref().map_or(V::Absent, |g| {
                                V::text(format!("{}:{}", g.algo.name(), hex(&g.digest)))
                            }),
                        ]);
                    }
                }
            }
        }
        // By (`hlc`, `from`, `to`), the order of `std.root_moves` ([LQ/std §4.23]).
        rows.sort_by(|a, b| {
            super::val::cmp_total(&a[0], &b[0])
                .then_with(|| super::val::cmp_total(&a[2], &b[2]))
                .then_with(|| super::val::cmp_total(&a[3], &b[3]))
        });
        rows
    }

    /// `schema(kind)` ([50 §2.9] Q22): one row per field of each kind.
    // spec: [LQ/std §2.12] schema
    fn schema_rows(&self, kind: Option<&str>) -> Vec<Vec<V>> {
        let s = &self.st().schema;
        let mut rows = Vec::new();
        for k in s.kind_names() {
            if kind.is_some_and(|w| w != k) {
                continue;
            }
            for f in s.fields_of(&k) {
                let (t, arg) = crate::schema::type_names(f.ty);
                let ty = match arg {
                    Some(a) => format!("{t}<{a}>"),
                    None => t.to_string(),
                };
                rows.push(vec![
                    V::text(k.clone()),
                    V::text(f.name.clone()),
                    V::text(ty),
                    V::Bool(f.optional),
                    V::text(f.index),
                ]);
            }
        }
        rows
    }

    /// `schema_edges()`: the F1 row of each edge kind ([F08 §9.6]).
    // spec: [LQ/std §2.12] schema_edges
    fn schema_edge_rows(&self) -> Vec<Vec<V>> {
        let ends = |e: &crate::schema::Ends| match e {
            crate::schema::Ends::Any => V::List(vec![V::text("any")]),
            crate::schema::Ends::Kinds(k) => {
                V::List(k.iter().map(|x| V::text(x.clone())).collect())
            }
        };
        self.st()
            .schema
            .edges()
            .into_iter()
            .map(|e| {
                vec![
                    V::text(e.lq_name.clone()),
                    V::text(e.name.clone()),
                    ends(&e.src),
                    ends(&e.dst),
                    V::Bool(e.symmetric),
                    V::List(e.reverse.iter().map(|r| V::text(r.clone())).collect()),
                    V::text(e.reading.clone()),
                    V::text(match e.class {
                        crate::schema::EdgeClass::Structural => "structural",
                        crate::schema::EdgeClass::Historical => "historical",
                    }),
                    V::text(match e.acyclic {
                        crate::schema::Acyclic::None => "none",
                        crate::schema::Acyclic::Forest => "forest",
                        crate::schema::Acyclic::Precedence => "precedence",
                        crate::schema::Acyclic::Dag => "dag",
                        crate::schema::Acyclic::ByConstruction => "by-construction",
                    }),
                ]
            })
            .collect()
    }

    /// `queries()`: the project named queries (F3), by name.
    // spec: [LQ/std §2.12] queries
    fn query_rows(&self) -> Vec<Vec<V>> {
        let s = &self.st().schema;
        let mut v: Vec<&crate::schema::QueryItem> = s
            .items
            .values()
            .filter_map(|i| match i {
                crate::schema::Item::Query(q) => Some(q),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v.into_iter()
            .map(|q| {
                vec![
                    V::text(q.name.clone()),
                    V::text(q.params.clone()),
                    V::text(q.shape.clone()),
                    V::text(q.budget.clone()),
                    V::Int(i64::from(q.lq_version)),
                    V::text(q.text.clone()),
                ]
            })
            .collect()
    }

    /// `leases()` and `markers()` ([F11 §6], §7): the rows of the runtime tables, tip only ([50 §3.8]).
    // spec: [F11 §6]
    fn runtime_rows(&self, leases: bool) -> Res<Vec<Vec<V>>> {
        if !self.at_tip() {
            return Err(self.not_here(
                if leases { "leases()" } else { "markers()" },
                "leases and markers",
            ));
        }
        let store = self.store();
        let mut rows = Vec::new();
        if leases {
            let mut ls: Vec<&lease::Lease> = self
                .w
                .lease_table()
                .values()
                .filter(|l| l.ended.is_none())
                .collect();
            ls.sort_by_key(|l| (l.task.map_or(0, |t| t.0), l.id));
            for l in ls {
                let flags = i64::from(l.run_scoped) | (i64::from(l.session_role) << 1);
                rows.push(vec![
                    opt_node(l.task),
                    V::Int(l.id as i64),
                    V::text(if l.kind == LeaseKind::Task {
                        "task"
                    } else {
                        "role"
                    }),
                    V::Int(flags),
                    V::text(l.role.clone()),
                    V::text(l.holder.clone()),
                    V::text(l.branch.clone()),
                    opt_node(l.run),
                    V::Int(l.token as i64),
                    V::Int(l.claimed_hlc as i64),
                    V::Int(l.ttl_ms as i64),
                    if l.expires.wall == u64::MAX {
                        V::Absent
                    } else {
                        V::Time(l.expires.wall as i64)
                    },
                    V::text(l.anchor.name()),
                    l.bound.map_or(V::Absent, |b| V::text(hex(&b))),
                    l.root_session.map_or(V::Absent, |b| V::text(hex(&b))),
                    V::Absent,
                    V::List(l.files_owned.iter().map(|f| V::text(f.clone())).collect()),
                ]);
            }
        } else {
            let refname = |id: u32| {
                store
                    .dag
                    .refs
                    .get(&id)
                    .map_or(String::new(), |r| r.name.clone())
            };
            for m in store.markers.rows() {
                let (n, r, c) = m.key;
                rows.push(vec![
                    V::Node(n),
                    V::text(refname(r)),
                    V::Rev(c),
                    V::text(m.kind.name()),
                    m.status.map_or(V::Absent, V::text),
                    V::text(m.cause.name()),
                    V::Int(i64::from(m.nonlinear)),
                    V::Int(m.ref_seq as i64),
                    m.actor.clone().map_or(V::Absent, V::Text),
                    m.outcome.clone().map_or(V::Absent, V::Text),
                    V::Int(m.hlc as i64),
                    V::Int(c as i64),
                    V::Absent,
                    V::List(m.holders.iter().map(|h| V::text(refname(*h))).collect()),
                ]);
            }
        }
        Ok(rows)
    }

    /// `links(scope)` ([40 §6.1]; [LQ/std §2.8] item 2): the link rows of every live node, or of the nodes in
    /// `subtree(scope)`, by node then anchor; tree-derived, tip only.
    // spec: [LQ/std §2.8]
    fn links_rows(&self, scope: Option<Nid>) -> Res<Vec<Vec<V>>> {
        if !self.at_tip() {
            return Err(self.not_here("links()", "the file tree"));
        }
        self.tree("links()")?;
        let nodes: Vec<Nid> = match scope {
            Some(s) => super::func::subtree(self, s, None, true),
            None => self
                .st()
                .nodes
                .iter()
                .filter(|(_, x)| x.live())
                .map(|(n, _)| *n)
                .collect(),
        };
        let mut rows = Vec::new();
        for n in nodes {
            for l in &self.node_links(n)?.rows {
                rows.push(vec![
                    V::Node(n),
                    l.handle.clone().map_or(V::Absent, V::Text),
                    V::Node(l.file),
                    V::text(l.path.clone()),
                    V::text(l.kind.clone()),
                    text_or_absent(&l.scope),
                    V::text(l.state.clone()),
                    V::text(l.evidence.clone()),
                    l.next.clone().map_or(V::Absent, V::Text),
                ]);
            }
        }
        Ok(rows)
    }
}

/// The resolution of a file node in the caller's tree.
fn resolve_file(ev: &Ev<'_>, f: Nid) -> Res<crate::r4::cascade::FileResult> {
    let tc = ev.tree("link_state()")?;
    let branch = ev.v.branch.clone().unwrap_or_default();
    Ok(ev
        .store()
        .resolve_node(ev.st(), ev.v.commit, tc, &branch, f))
}

/// The links of a node ([F18 §4.4]): for each anchor of each `AT` edge, its file's resolution refined by the anchor's
/// resolution on the content at the file's path.
// spec: [F18 §4.4]
pub fn resolve_links(ev: &Ev<'_>, n: Nid) -> Res<NodeLinks> {
    let Some(x) = ev.node(n) else {
        return Ok(NodeLinks::default());
    };
    let mut out = NodeLinks::default();
    let store = ev.store();
    for (k, p) in x.out.iter().filter(|(k, _)| k.kind == "at") {
        let tc = ev.tree("link_state()")?.clone();
        let fr = resolve_file(ev, k.dst)?;
        let fnode = ev.node(k.dst).map(|y| crate::links::file_node(k.dst, y, 0));
        let path = fnode.as_ref().map_or(String::new(), |f| f.path.clone());
        let anchor = p
            .anchor
            .as_deref()
            .map(|a| crate::r4::anchor::Anchor::from_canon(k.disc.unwrap_or(Uid::ZERO), a));
        let (state, details, astate) = match &anchor {
            None => (fr.state, fr.details.clone(), "unresolved".to_string()),
            Some(a) => {
                if crate::r4::link::anchor_runs(&fr, a) {
                    let at = fr.at.clone().unwrap_or_else(|| path.clone());
                    let content = store
                        .files
                        .fs
                        .trees
                        .get(&tc.root)
                        .and_then(|t| t.read(&at).ok())
                        .map(<[u8]>::to_vec);
                    let root = fnode
                        .as_ref()
                        .map_or("project".to_string(), |f| f.root.clone());
                    let algo = store.root_algo(&root, &tc.root);
                    let consts = crate::links::anchor_consts(&store.conf);
                    let r = match &content {
                        Some(c) => crate::r4::anchor::resolve(
                            a,
                            crate::r4::anchor::Content::Bytes(c),
                            algo,
                            &consts,
                        ),
                        // The file did not read: `unverified (unreadable)` ([F18 §4.6] 59; OQ-F-4).
                        None => crate::r4::anchor::AResult {
                            state: crate::r4::anchor::AState::Unverified(59),
                            span: None,
                            score: None,
                            details: Vec::new(),
                        },
                    };
                    let (s, d) = crate::r4::link::link_state(&fr, a, &r);
                    (s, d, r.state.name().to_string())
                } else {
                    (fr.state, fr.details.clone(), "unresolved".to_string())
                }
            }
        };
        let codes: Vec<u8> = details.iter().map(|d| d.code).collect();
        let os = crate::links::os_of(&tc.root);
        let handle = k
            .disc
            .and_then(|u| store.files.anchors.get(&u))
            .map(|h| format!("a{h}"));
        let target = handle.clone().unwrap_or_else(|| k.dst.0.to_string());
        let next = match state {
            LState::Ok => None,
            LState::MovedNeedsConfirm | LState::Ambiguous | LState::StaleAnchor => {
                Some(format!("moirai file where {target} --evidence"))
            }
            _ => Some(format!("moirai links sync --scope {}", n.0)),
        };
        out.rows.push(LinkRow {
            edge: EdgeV {
                src: n,
                kind: "at".into(),
                dst: k.dst,
                disc: k.disc,
            },
            handle,
            file: k.dst,
            path,
            kind: anchor.as_ref().map_or("file".to_string(), |_| {
                p.anchor.as_ref().map_or("file".into(), |a| a.kind.clone())
            }),
            scope: anchor
                .as_ref()
                .map_or(String::new(), |a| super::view::scope_text(&a.scope)),
            state: state.name().to_string(),
            lstate: state,
            anchor_state: astate,
            evidence: crate::r4::strings::qualified(state, &codes, os),
            next,
        });
    }
    Ok(out)
}

/// `link_state(f)` of an artifact: its file-level state ([50 §2.6]).
pub fn file_state(ev: &Ev<'_>, f: Nid) -> Res<String> {
    Ok(resolve_file(ev, f)?.state.name().to_string())
}

/// `link_state(n)` ([50 §2.6]; [LQ/std §2.8] item 3): an artifact's file-level state; any other node's most severe
/// state over its anchors, or the frozen string `none` without an `AT` edge.
// spec: [LQ/std §2.8] item 3
pub fn link_state_node(ev: &Ev<'_>, n: Nid) -> Res<String> {
    if ev.node(n).is_some_and(|x| x.kind == "artifact") {
        return file_state(ev, n);
    }
    let l = ev.node_links(n)?;
    if l.rows.is_empty() {
        return Ok("none".into());
    }
    let states: Vec<LState> = l.rows.iter().map(|r| r.lstate).collect();
    Ok(crate::r4::strings::most_severe(states).name().to_string())
}

/// `link_state(a)` of an `AT` edge variable: that anchor's link state ([50 §2.6]).
pub fn link_state_edge(ev: &Ev<'_>, e: &EdgeV) -> Res<Option<String>> {
    let l = ev.node_links(e.src)?;
    Ok(l.rows
        .iter()
        .find(|r| r.edge == *e)
        .map(|r| r.state.clone()))
}

/// `file(path, root)` ([50 §2.6]): the live artifact whose current path is `path` under `root`, else the one with the
/// path among its aliases (notice N11), else absent.
// spec: [50 §2.6] file
pub fn file_of(ev: &Ev<'_>, root: &str, path: &str) -> V {
    let files: Vec<crate::r4::cascade::FileNode> = ev
        .st()
        .nodes
        .iter()
        .filter(|(_, x)| x.live() && x.kind == "artifact")
        .map(|(n, x)| crate::links::file_node(*n, x, 0))
        .collect();
    if let Some(f) = files.iter().find(|f| f.root == root && f.path == path) {
        return V::Node(Nid(f.n));
    }
    if let Some(f) = files
        .iter()
        .find(|f| f.root == root && f.aliases.iter().any(|a| a == path))
    {
        ev.w.note(
            crate::lq::diag::Code::N11,
            format!(
                "'{path}' is an old path of #{}; it is now '{}'",
                f.n, f.path
            ),
            Vec::new(),
            None,
        );
        return V::Node(Nid(f.n));
    }
    V::Absent
}

/// `staleness(n)` ([50 §2.6]; [AR §3.5]): the node's pinned git commit (`measured_on` or `observed_git_sha`) against
/// the caller's tree's HEAD: `fresh` when an ancestor, `stale` when not, `unknown` when not determinable.
// spec: [50 §2.6] staleness
pub fn staleness(ev: &Ev<'_>, n: Nid) -> String {
    let store = ev.store();
    let Some(x) = ev.node(n) else {
        return "unknown".into();
    };
    let pinned = ["measured_on", "observed_git_sha"]
        .iter()
        .find_map(|f| match x.fields.get(*f) {
            Some(Value::Oid(o)) => Some(o.clone()),
            _ => None,
        });
    let tree = ev.w.tree.as_ref().map(|t| t.root.clone()).or_else(|| {
        store
            .designation()
            .into_iter()
            .find(|p| Some(&p.branch) == ev.v.branch.as_ref())
            .map(|p| p.tree)
    });
    let (Some(o), Some(t)) = (pinned, tree) else {
        return "unknown".into();
    };
    let Some((repo, head)) = store.files.git.of_tree(&t) else {
        return "unknown".into();
    };
    let Some(h) = repo.head_commit(head) else {
        return "unknown".into();
    };
    let c = hex(&o.digest);
    if o.algo != repo.algo || !repo.has(&c) {
        "unknown".into()
    } else if repo.is_ancestor(&c, h) {
        "fresh".into()
    } else {
        "stale".into()
    }
}

/// The node ids of a list value.
pub fn nodes_of(v: &V) -> Vec<Nid> {
    v.elems().iter().filter_map(V::node).collect()
}

/// A map from ids to their rows' first appearance, for stable orders.
pub type Seen = BTreeMap<Nid, usize>;
