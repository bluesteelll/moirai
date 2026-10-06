//! The views a query reads ([50 §3.1], §3.9) and what a node or an edge reads as in one of them ([LQ/std §2.11]):
//! stored fields typed by the view's schema (with the `coerce = timestamp` scale of §2.13), identity and derived
//! properties by definition over the view's state ([`crate::derived`]), runtime properties from the store's lease
//! table and the I26′ oracle at a branch tip ([`crate::coord`]), and tree-derived states from the model of [40]'s
//! resolver ([`crate::r4`]) at a tip with a resolved tree.

use super::Note;
use super::val::{EdgeV, EnumV, V};
use crate::api::{Caller as ApiCaller, Store};
use crate::coord::{self, Oracle};
use crate::dag::RefKind;
use crate::derived::{self, Index, Row};
use crate::err::{Refusal, Res};
use crate::lease::{self, LeaseKind};
use crate::links::TreeCtx;
use crate::lq::cast::CExpr;
use crate::lq::diag::Code;
use crate::state::{EdgeProps, Node, State};
use crate::value::{Nid, Uid, Value, hex};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// What kind of view a part reads ([LQ/envelope §3.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// A branch tip: runtime state exists ([50 §3.8]).
    Tip,
    /// A past or fixed revision (a commit, `~n`, `@n`, `@time`, a tag, an import ref).
    AsOf,
    /// A staging ref `merge/<dst>/from/<src>` (read-only except `RESOLVE`).
    Staged,
}

/// One view: the state of a revision with what the part needs to know about it.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// The state.
    pub st: Rc<State>,
    /// The commit, or `None` for the empty state.
    pub commit: Option<u64>,
    /// `view_ref()`: the ref the view is of, or the revision text of a commit view.
    pub ref_name: String,
    /// The branch whose tip this is, for a tip view.
    pub branch: Option<String>,
    /// The kind.
    pub kind: ViewKind,
}

/// The store-wide inputs of one evaluation: the store, the resolved caller and its tree, the clock reading of the
/// query (read once at its start, [50 §3.9] item 7) and the warnings and notices it collects.
pub struct World<'a> {
    /// The store.
    pub store: &'a Store,
    /// The resolved caller.
    pub caller: &'a ApiCaller,
    /// The caller's tree for tree-derived state.
    pub tree: Option<TreeCtx>,
    /// The ablation switches.
    pub ab: super::Ablations,
    /// Warnings and notices.
    pub notes: RefCell<Vec<Note>>,
    /// The executor's counts of W01, W10 and N10 ([LQ/errors §5.6]).
    pub counts: RefCell<super::Counts>,
    /// The binding names of the root and of each named query being evaluated, innermost last.
    pub names: RefCell<Vec<Rc<Vec<String>>>>,
    /// The view of a part without `USE` when no calling part gives one: `--at` ([50 §3.9] item 1).
    pub at: Option<CExpr>,
    /// The state whose schema holds the project named queries the binder resolved: the default view's (`--at`, else
    /// the caller's branch tip). A call takes its definition from here and binds it against the part's view ([50 §3.9]
    /// item 5); `None` takes it from the part's view (a `TX` block, whose catalog is its candidate).
    pub catalog: Option<Rc<State>>,
    /// The lease table runtime properties read: a `TX` block's candidate table, so that a lease the block took earlier
    /// is visible to its later statements ([50 §3.10] item 2); `None` reads the store's.
    pub leases: Option<Rc<BTreeMap<u64, lease::Lease>>>,
}

impl World<'_> {
    /// The lease table runtime properties read ([`World::leases`]).
    pub fn lease_table(&self) -> &BTreeMap<u64, lease::Lease> {
        self.leases.as_deref().unwrap_or(&self.store.leases)
    }

    /// The written name of a binding of the innermost root being evaluated.
    pub fn name(&self, b: crate::lq::cast::BindingId) -> String {
        self.names
            .borrow()
            .last()
            .and_then(|n| n.get(b as usize).cloned())
            .unwrap_or_else(|| "n".to_string())
    }

    /// Records a warning or notice; a counted one adds to an earlier one of the same text ([LQ/errors §4.2]).
    pub fn note(&self, code: Code, message: String, detail: Vec<String>, count: Option<u64>) {
        let mut v = self.notes.borrow_mut();
        if let Some(n) = v.iter_mut().find(|n| n.code == code && n.key == message) {
            if let (Some(a), Some(b)) = (n.count.as_mut(), count) {
                *a += b;
            }
            return;
        }
        v.push(Note {
            code,
            key: message.clone(),
            message,
            detail,
            count,
        });
    }

    /// The revision-resolution context of the caller ([F12 §3.3]).
    fn rev_ctx(&self) -> crate::vcs::RevCtx {
        self.store.rev_ctx(self.caller)
    }

    /// The text of a C-AST revision for the store's resolver ([F12 §3]): `HEAD`, a ref, `c<64 hex>`, and suffixes.
    fn rev_text(e: &CExpr) -> Option<String> {
        Some(match e {
            CExpr::RHead => "HEAD".into(),
            CExpr::RRef(r) => r.clone(),
            CExpr::RCommit(c) => format!("c{}", hex(c)),
            CExpr::RSuf(b, k, n) => {
                let base = Self::rev_text(b)?;
                match k {
                    1 => format!("{base}~{n}"),
                    2 => format!("{base}^{n}"),
                    3 => format!("{base}@{n}"),
                    _ => format!("{base}@{}", super::func::iso(*n)),
                }
            }
            _ => return None,
        })
    }

    /// Resolves a C-AST revision to a commit ([LQ/canonical-ast §5.6]; E301 from view resolution, [LQ/errors §5.4]);
    /// N04 for a reflog time ([LQ/errors §5.6]).
    // spec: [LQ/canonical-ast §5.6]
    pub fn commit_of(&self, e: &CExpr) -> Res<Option<u64>> {
        let text = Self::rev_text(e).ok_or_else(|| Refusal::usage("not a revision"))?;
        let c = self.store.dag.rev_commit(&text, &self.rev_ctx())?;
        if let CExpr::RSuf(_, 4, _) = e
            && let Some(s) = c
        {
            let id = self.store.dag.commits[&s].id;
            self.note(
                Code::N04,
                format!("{text} resolved to rev {s} c{}", &hex(&id)[..8]),
                Vec::new(),
                None,
            );
        }
        Ok(c)
    }

    /// The view of a part ([50 §3.9] item 1): its revision, else the caller's resolved branch (or detached head).
    // spec: [50 §3.9]
    pub fn open(&self, rev: Option<&CExpr>) -> Res<View> {
        let dag = &self.store.dag;
        let alloc = &self.store.alloc;
        let tip_of = |name: &str, kind: RefKind| -> Res<View> {
            let r = dag
                .live(name)
                .ok_or_else(|| Refusal::lq("E301", format!("unknown revision {name}")))?;
            let vk = match kind {
                RefKind::Merge => ViewKind::Staged,
                RefKind::Work | RefKind::Plan => ViewKind::Tip,
                _ => ViewKind::AsOf,
            };
            Ok(View {
                st: dag.state_at(r.tip, alloc),
                commit: r.tip,
                ref_name: name.to_string(),
                branch: (vk != ViewKind::AsOf).then(|| name.to_string()),
                kind: vk,
            })
        };
        if let (None, Some(at)) = (rev, &self.at) {
            return self.open(Some(at));
        }
        let v = match rev {
            None => {
                if self.caller.branch.is_empty() {
                    let c = self.caller.detached;
                    View {
                        st: dag.state_at(c, alloc),
                        commit: c,
                        ref_name: c.map_or_else(String::new, |s| format!("s{s}")),
                        branch: None,
                        kind: ViewKind::AsOf,
                    }
                } else {
                    let b = self.caller.branch.clone();
                    tip_of(&b, RefKind::of(&b))?
                }
            }
            Some(CExpr::RHead) if !self.caller.branch.is_empty() => {
                let b = self.caller.branch.clone();
                tip_of(&b, RefKind::of(&b))?
            }
            Some(CExpr::RRef(name)) => tip_of(name, RefKind::of(name))?,
            Some(e) => {
                let c = self.commit_of(e)?;
                View {
                    st: dag.state_at(c, alloc),
                    commit: c,
                    ref_name: Self::rev_text(e).unwrap_or_default(),
                    branch: None,
                    kind: ViewKind::AsOf,
                }
            }
        };
        if v.kind == ViewKind::Staged {
            self.note(
                Code::N02,
                format!("{} is a staging ref: read-only except RESOLVE", v.ref_name),
                vec![format!(
                    "resolve: TX ON {} {{ RESOLVE '<key>' TAKE ... }}, then moirai merge --continue <src> --into <dst>",
                    v.ref_name
                )],
                None,
            );
        }
        Ok(v)
    }
}

/// The evaluation context of one view: the view, its derived adjacency and the per-view caches.
pub struct Ev<'a> {
    /// The store-wide inputs.
    pub w: &'a World<'a>,
    /// The view.
    pub v: &'a View,
    /// The derived adjacency, built on first use: an evaluation that reads only stored fields never builds it.
    ix: OnceCell<Index<'a>>,
    rows: RefCell<BTreeMap<Nid, Rc<Row>>>,
    local: OnceCell<BTreeMap<Nid, (u64, u64, u64)>>,
    oracle: RefCell<Option<Oracle<'a>>>,
    links: RefCell<BTreeMap<Nid, Rc<super::rel::NodeLinks>>>,
    topo: OnceCell<BTreeMap<Nid, i64>>,
    now: i64,
    /// The counted facts of the row being evaluated.
    pub flags: RefCell<super::expr::Flags>,
    /// How deep the evaluation is inside nested evaluations (a `WHERE` being filtered, a subquery, a quantified group's
    /// step): only the outermost row's facts are counted ([LQ/errors §5.6]).
    pub depth: Cell<u32>,
    /// The members of the group being aggregated that a division by zero hit (N10, counted per input row).
    pub hits: RefCell<Option<Vec<bool>>>,
    /// The literal ids whose notice the view has raised ([50 §3.6]: once per id).
    pub noticed: RefCell<BTreeSet<Nid>>,
    /// The revision node of each parameter of the definition being evaluated, by position ([LQ/std §2.8] item 1).
    pub revs: RefCell<Vec<Option<CExpr>>>,
}

/// The `coerce = timestamp` fields' scale: stored Unix seconds, LQ milliseconds ([LQ/std §2.13]).
const MS: i64 = 1000;

impl<'a> Ev<'a> {
    /// The context of a view.
    pub fn new(w: &'a World<'a>, v: &'a View) -> Ev<'a> {
        let now = match v.kind {
            // `now()` is the wall clock at a tip, the view commit's HLC at a past view ([50 §3.9] item 7).
            ViewKind::AsOf => v
                .commit
                .and_then(|c| w.store.dag.commits.get(&c))
                .map_or(w.store.env.wall_ms, |c| (c.hlc >> 16) as i64),
            _ => w.store.env.wall_ms,
        };
        Ev {
            w,
            v,
            ix: OnceCell::new(),
            rows: RefCell::new(BTreeMap::new()),
            local: OnceCell::new(),
            oracle: RefCell::new(None),
            links: RefCell::new(BTreeMap::new()),
            topo: OnceCell::new(),
            now,
            flags: RefCell::new(Default::default()),
            depth: Cell::new(0),
            hits: RefCell::new(None),
            noticed: RefCell::new(BTreeSet::new()),
            revs: RefCell::new(Vec::new()),
        }
    }

    /// The derived adjacency of the view ([F13 §6.2]).
    pub fn ix(&self) -> &Index<'a> {
        self.ix.get_or_init(|| Index::new(&self.v.st))
    }

    /// The store.
    pub fn store(&self) -> &'a Store {
        self.w.store
    }

    /// The state.
    pub fn st(&self) -> &'a State {
        &self.v.st
    }

    /// `now()` of the view in milliseconds.
    pub fn now(&self) -> i64 {
        self.now
    }

    /// Whether runtime state exists: a branch tip ([50 §3.8]).
    pub fn at_tip(&self) -> bool {
        self.v.kind == ViewKind::Tip
    }

    /// E302 for a property that exists only at a branch tip ([LQ/errors §5.4]).
    pub fn not_here(&self, prop: &str, what: &str) -> Refusal {
        Refusal::lq(
            "E302",
            format!(
                "{prop} uses {what}, which exists only at a branch tip; the view is {} (as-of)",
                self.v.ref_name
            ),
        )
    }

    /// The from-scratch derived row of a live node ([F13 §6.2]).
    pub fn row(&self, n: Nid) -> Rc<Row> {
        if let Some(r) = self.rows.borrow().get(&n) {
            return r.clone();
        }
        let r = Rc::new(derived::row(self.ix(), n, &|_| None));
        self.rows.borrow_mut().insert(n, r.clone());
        r
    }

    /// (`rev`, `created`, `updated`) of a node at the view ([API §15.4]).
    pub fn local(&self, n: Nid) -> (u64, u64, u64) {
        self.local
            .get_or_init(|| self.w.store.dag.local_seqs_all(self.v.commit))
            .get(&n)
            .copied()
            .unwrap_or((0, 0, 0))
    }

    /// A commit's `append_hlc` wall time in milliseconds ([50] F14).
    pub fn commit_ms(&self, seq: u64) -> Option<i64> {
        self.w
            .store
            .dag
            .commits
            .get(&seq)
            .map(|c| (c.append_hlc >> 16) as i64)
    }

    /// Runs `f` with the I26′ oracle of the store's DAG.
    fn with_oracle<T>(&self, f: impl FnOnce(&mut Oracle<'a>) -> T) -> T {
        let mut o = self.oracle.borrow_mut();
        let o = o.get_or_insert_with(|| Oracle::new(&self.w.store.dag, &self.w.store.alloc));
        f(o)
    }

    /// The enumeration value of a stored name, with its rank in the view's schema and the field it belongs to
    /// ([50 §3.5]).
    // spec: [50 §3.5] enumerations
    pub fn enum_v(&self, kind: &str, field: &str, name: &str) -> V {
        let rank = self
            .st()
            .schema
            .value(kind, field, name)
            .map_or(u32::MAX, |e| u32::from(e.rank));
        V::Enum(Box::new(EnumV {
            name: name.to_string(),
            rank,
            kind: kind.to_string(),
            field: field.to_string(),
        }))
    }

    /// A text ranked as a value of an enumeration's field, for an ordered comparison with it ([50 §3.5]: enumerations
    /// order by declared rank); `None` when the text is no value of that field (the comparison is then false).
    // spec: [50 §3.5] enumerations
    pub fn rank_text(&self, e: &EnumV, text: &str) -> Option<V> {
        self.st()
            .schema
            .value(&e.kind, &e.field, text)
            .map(|_| self.enum_v(&e.kind, &e.field, text))
    }

    /// The revision a commit id names: its seq when the store holds it, else the id as text ([F08 §5.1]: a cited
    /// commit need not be one this store holds).
    fn commit_v(&self, c: &[u8; 32]) -> V {
        self.w
            .store
            .dag
            .seq_of(c)
            .map_or_else(|| V::text(format!("c{}", hex(c))), V::Rev)
    }

    /// A stored value as an LQ value, typed by its field ([LQ/std §2.11]; [LQ/std §2.13] for `coerce = timestamp`).
    // spec: [LQ/std §2.13]
    pub fn value(&self, kind: &str, field: &str, v: &Value) -> V {
        let timestamp = self
            .st()
            .schema
            .field(kind, field)
            .is_some_and(|f| f.coerce == "timestamp");
        match v {
            Value::Bool(b) => V::Bool(*b),
            Value::Int(i) if timestamp => V::Time(i.saturating_mul(MS)),
            Value::Int(i) | Value::Counter(i) => V::Int(*i),
            Value::F64(f) => V::Float(f.get()),
            Value::Enum(s) if field == "priority" => s
                .strip_prefix('P')
                .and_then(|d| d.parse::<i64>().ok())
                .map_or_else(|| V::text(s.clone()), V::Int),
            Value::Enum(s) => self.enum_v(kind, field, s),
            Value::Text(s) => V::text(s.clone()),
            Value::Set(e) => V::List(e.iter().map(|x| self.value(kind, field, x)).collect()),
            Value::Ref(n) => V::Node(*n),
            Value::Commit(c) => self.commit_v(c),
            Value::Path(p) => V::text(crate::links::path_text(p)),
            Value::Oid(o) => V::text(format!("{}:{}", o.algo.name(), hex(&o.digest))),
            Value::PathMove(m) => V::Map(vec![
                ("hlc".into(), V::Int(m.hlc as i64)),
                ("from".into(), V::text(crate::links::path_text(&m.from))),
                ("to".into(), V::text(crate::links::path_text(&m.to))),
            ]),
        }
    }

    /// The node `n` of the view, live or a tombstone.
    pub fn node(&self, n: Nid) -> Option<&'a Node> {
        self.v.st.nodes.get(&n)
    }

    /// A node property ([LQ/std §2.11]): identity, derived, runtime and tree-derived properties by definition, the
    /// tombstone properties of a node bound through `DELETED`, every other name a stored field (absent where the
    /// node's kind does not have it).
    // spec: [LQ/std §2.11]
    pub fn prop(&self, n: Nid, name: &str) -> Res<V> {
        let Some(x) = self.node(n) else {
            return Ok(V::Absent);
        };
        if let Some(t) = &x.tomb {
            return Ok(self.tomb_prop(n, x, t, name));
        }
        let row = |f: fn(&Row) -> V| -> V { f(&self.row(n)) };
        let rev = |s: u64| if s == 0 { V::Absent } else { V::Rev(s) };
        Ok(match name {
            "id" => V::Node(n),
            "uid" => V::text(x.uid.hex()),
            "kind" => V::text(x.kind.clone()),
            "status" => self.enum_v(&x.kind, "status", &x.status),
            "resolution" => self.enum_v(&x.kind, "resolution", &x.resolution),
            "parent" => x.parent.map_or(V::Absent, V::Node),
            "order" => x.order.clone().map_or(V::Absent, V::Text),
            "body" => x.body.clone().map_or(V::Absent, V::Text),
            "created" => rev(self.local(n).1),
            "created_at" => self.commit_ms(self.local(n).1).map_or(V::Absent, V::Time),
            "created_by" => V::text(x.creator.actor.clone()),
            "created_role" if x.creator.role.is_empty() => V::Absent,
            "created_role" => V::text(x.creator.role.clone()),
            "updated" => rev(self.local(n).2),
            "rev" => rev(self.local(n).0),
            "updated_at" => self.commit_ms(self.local(n).2).map_or(V::Absent, V::Time),
            "updated_by" => self
                .w
                .store
                .dag
                .commits
                .get(&self.local(n).2)
                .map_or(V::Absent, |c| V::text(c.actor.clone())),
            "done" => row(|r| r.done.map_or(V::Absent, V::Bool)),
            "unfinished" => row(|r| r.unfinished.map_or(V::Absent, V::Bool)),
            "container" => row(|r| V::Bool(r.container)),
            "ready_to_close" => row(|r| V::Bool(r.ready_to_close)),
            "is_blocker" => row(|r| V::Bool(r.is_blocker)),
            "suspect" => row(|r| V::Bool(r.suspect)),
            "conflicted" => row(|r| V::Bool(r.conflicted)),
            "has_dangling" => row(|r| V::Bool(r.has_dangling)),
            // `unblocked` includes `defer_until ≤ now()` at the view's `now()` ([50 §3.8]).
            "unblocked" => V::Bool(
                self.row(n).unblocked && coord::defer_ok(self.st(), n, self.now.div_euclid(MS)),
            ),
            "blocked" => row(|r| V::Bool(r.blocked)),
            "children_total" => row(|r| V::Int(i64::from(r.children_total))),
            "children_done" => row(|r| V::Int(i64::from(r.children_done))),
            "open_blockers" => row(|r| V::Int(i64::from(r.open_blockers))),
            "answered" => row(|r| r.answered.map_or(V::Absent, V::Bool)),
            "depth" => row(|r| V::Int(i64::from(r.depth))),
            "topo" => V::Int(self.topo(n)),
            "ready" | "claimed" | "lease" | "settled_elsewhere" | "deleted_elsewhere" => {
                return self.runtime_prop(n, x, name);
            }
            "state" if x.kind == "artifact" => {
                if !self.at_tip() {
                    return Err(self.not_here("f.state", "the file tree"));
                }
                V::text(self.file_state(n)?)
            }
            _ => match x.field(&self.st().schema, name) {
                Some(v) => self.value(&x.kind, name, &v),
                None => V::Absent,
            },
        })
    }

    /// `topo` ([F08 §3.4]): the length of the longest precedence path into the node — a topological rank of the
    /// combined precedence graph that is a function of the view's state, computed once per view by Kahn's order.
    // spec: [F08 §3.4] topo
    fn topo(&self, n: Nid) -> i64 {
        let ranks = self.topo.get_or_init(|| {
            let edges = derived::precedence_edges(self.ix());
            let mut indeg: BTreeMap<Nid, usize> = BTreeMap::new();
            let mut succ: BTreeMap<Nid, Vec<Nid>> = BTreeMap::new();
            for (a, b, _) in &edges {
                *indeg.entry(*b).or_insert(0) += 1;
                indeg.entry(*a).or_insert(0);
                succ.entry(*a).or_default().push(*b);
            }
            let mut rank: BTreeMap<Nid, i64> = BTreeMap::new();
            let mut ready: std::collections::BTreeSet<Nid> = indeg
                .iter()
                .filter(|(_, d)| **d == 0)
                .map(|(n, _)| *n)
                .collect();
            while let Some(a) = ready.pop_first() {
                let ra = *rank.entry(a).or_insert(0);
                for b in succ.get(&a).into_iter().flatten() {
                    let rb = rank.entry(*b).or_insert(0);
                    *rb = (*rb).max(ra + 1);
                    let d = indeg.get_mut(b).expect("an edge end");
                    *d -= 1;
                    if *d == 0 {
                        ready.insert(*b);
                    }
                }
            }
            rank
        });
        ranks.get(&n).copied().unwrap_or(0)
    }

    /// The tombstone properties of a node bound through `DELETED` ([LQ/std §2.11]; [50 §3.6]).
    // spec: [50 §3.6] tombstones
    fn tomb_prop(&self, n: Nid, x: &Node, t: &crate::state::Tomb, name: &str) -> V {
        let deleting = || {
            self.w
                .store
                .dag
                .chain(self.v.commit)
                .into_iter()
                .find(|c| {
                    self.w.store.dag.commits[c].changeset.keys().any(|k| {
                        matches!(k, crate::state::Key::Node(m, crate::state::Aspect::Existence) if *m == n)
                    })
                })
        };
        match name {
            "id" => V::Node(n),
            "uid" => V::text(x.uid.hex()),
            "kind" => V::text(x.kind.clone()),
            "title" => x.text("title").map_or(V::Absent, V::text),
            "deleted_by" => deleting().map_or(V::Absent, |c| {
                V::text(self.w.store.dag.commits[&c].actor.clone())
            }),
            "deleted_at" => deleting()
                .and_then(|c| self.commit_ms(c))
                .map_or(V::Absent, V::Time),
            "deleted_reason" => t.reason.clone().map_or(V::Absent, V::Text),
            "replaced_by" => t.replaced_by.map_or(V::Absent, V::Node),
            _ => V::Absent,
        }
    }

    /// The live task lease on a node, if any, in the lease table runtime properties read ([`World::lease_table`]).
    fn live_lease(&self, n: Nid) -> Option<lease::Lease> {
        let env = &self.w.store.env;
        self.w
            .lease_table()
            .values()
            .find(|l| {
                l.kind == LeaseKind::Task && l.task == Some(n) && lease::is_live(l, env).is_live()
            })
            .cloned()
    }

    /// The runtime properties ([50 §3.8]; tip only): `ready` for the caller, `claimed`, `lease`, and the I26′
    /// `settled_elsewhere` and `deleted_elsewhere` ([F13 §6.2]; PD-009 to PD-015).
    // spec: [50 §3.8] runtime
    fn runtime_prop(&self, n: Nid, x: &Node, name: &str) -> Res<V> {
        let Some(b) = self.v.branch.clone().filter(|_| self.at_tip()) else {
            return Err(self.not_here(&format!("n.{name}"), "leases and markers"));
        };
        let task = x.kind == "task";
        Ok(match name {
            "ready" => {
                let st = &self.w.store;
                let caller = Some(self.w.caller.actor.as_str()).filter(|a| !a.is_empty());
                V::Bool(
                    task && {
                        let env_now = crate::clock::Env {
                            wall_ms: self.now,
                            ..st.env.clone()
                        };
                        let leases = self.w.lease_table();
                        self.with_oracle(|o| {
                            coord::ready(o, self.ix(), &b, n, leases, &env_now, caller)
                        })
                    },
                )
            }
            "claimed" => V::Bool(task && self.live_lease(n).is_some()),
            "lease" => match self.live_lease(n) {
                None => V::Absent,
                Some(l) => V::Map(vec![
                    ("holder".into(), V::text(l.holder.clone())),
                    ("token".into(), V::Int(l.token as i64)),
                    (
                        "expires".into(),
                        if l.expires.wall == u64::MAX {
                            V::Absent
                        } else {
                            V::Time(l.expires.wall as i64)
                        },
                    ),
                    (
                        "run".into(),
                        l.run
                            .and_then(|r| self.node(r))
                            .and_then(|r| r.text("name").or(r.text("title")))
                            .map_or(V::Absent, V::text),
                    ),
                    ("branch".into(), V::text(l.branch.clone())),
                ]),
            },
            "settled_elsewhere" => V::Bool(self.with_oracle(|o| o.settled_elsewhere(&b, n))),
            _ => V::Bool(self.with_oracle(|o| o.deleted_elsewhere(&b, n))),
        })
    }

    /// An edge's property ([LQ/std §2.11]; the anchor fields of an `AT` edge, [F08 §10.3]).
    // spec: [LQ/std §2.11] edges
    pub fn edge_prop(&self, e: &EdgeV, name: &str) -> Res<V> {
        let props = self.edge_props(e);
        Ok(match name {
            "type" => V::text(self.lq_name(&e.kind)),
            "flagged" => V::Bool(props.is_some_and(|p| p.flagged)),
            "pinned" => match props.and_then(|p| p.pinned) {
                None => V::Absent,
                Some(c) => self.commit_v(&c),
            },
            "state" => {
                if !self.at_tip() {
                    return Err(self.not_here("a.state", "the file tree"));
                }
                self.anchor_state(e)?.map_or(V::Absent, V::text)
            }
            "anchor" => e
                .disc
                .and_then(|u| self.w.store.files.anchors.get(&u))
                .map_or(V::Absent, |h| V::text(format!("a{h}"))),
            "kind" | "mode" | "watch" | "scope" | "quote" | "hint" => {
                let Some(a) = props.and_then(|p| p.anchor.as_deref()) else {
                    return Ok(V::Absent);
                };
                let r = crate::r4::anchor::Anchor::from_canon(e.disc.unwrap_or(Uid::ZERO), a);
                match name {
                    "kind" => V::text(a.kind.clone()),
                    "mode" => V::text(a.mode.clone()),
                    "watch" => V::text(a.watch.clone()),
                    "scope" => {
                        let s = scope_text(&r.scope);
                        if s.is_empty() { V::Absent } else { V::text(s) }
                    }
                    "quote" => a
                        .text
                        .as_ref()
                        .filter(|t| !t.quote.is_empty())
                        .map_or(V::Absent, |t| {
                            V::text(String::from_utf8_lossy(&t.quote).into_owned())
                        }),
                    _ => a
                        .hint
                        .map_or(V::Absent, |(f, l)| V::text(format!("{f}-{l}"))),
                }
            }
            _ => V::Absent,
        })
    }

    /// The property block of an edge of the view (`None` for the hierarchy edge and a missing edge).
    pub fn edge_props(&self, e: &EdgeV) -> Option<&'a EdgeProps> {
        let x = self.node(e.src)?;
        x.out.get(&crate::state::EdgeKey {
            kind: e.kind.clone(),
            dst: e.dst,
            disc: e.disc,
        })
    }

    /// The LQ name of a stored edge kind.
    pub fn lq_name(&self, stored: &str) -> String {
        self.st()
            .schema
            .edge(stored)
            .map_or_else(|| stored.to_uppercase(), |x| x.lq_name.clone())
    }

    /// The stored name of an LQ edge name (`lq_name`).
    pub fn stored_name(&self, lq: &str) -> Option<String> {
        self.st()
            .schema
            .edges()
            .into_iter()
            .find(|e| e.lq_name.eq_ignore_ascii_case(lq))
            .map(|e| e.name.clone())
    }

    /// The tree for tree-derived state, or E302 with the `--tree` hint ([LQ/std §2.8] item 4).
    pub fn tree(&self, prop: &str) -> Res<&TreeCtx> {
        self.w
            .tree
            .as_ref()
            .filter(|t| t.eligible)
            .ok_or_else(|| Refusal::lq("E302", format!("{prop} needs a resolved tree")))
    }

    /// The links of a node, resolved once per view.
    pub fn node_links(&self, n: Nid) -> Res<Rc<super::rel::NodeLinks>> {
        if let Some(l) = self.links.borrow().get(&n) {
            return Ok(l.clone());
        }
        let l = Rc::new(super::rel::resolve_links(self, n)?);
        self.links.borrow_mut().insert(n, l.clone());
        Ok(l)
    }

    /// `f.state` = `link_state(f)` of an artifact: its file-level state ([50 §2.6]).
    pub fn file_state(&self, f: Nid) -> Res<String> {
        super::rel::file_state(self, f)
    }

    /// `a.state` of an `AT` edge ([50 §2.6]).
    pub fn anchor_state(&self, e: &EdgeV) -> Res<Option<String>> {
        let l = self.node_links(e.src)?;
        Ok(l.rows
            .iter()
            .find(|r| r.edge == *e)
            .map(|r| r.anchor_state.clone()))
    }

    /// The I26′ exclusion of a node on the view's branch (for `blockers()`'s `elsewhere`).
    pub fn settled_elsewhere(&self, n: Nid) -> bool {
        match &self.v.branch {
            Some(b) if self.at_tip() => self.with_oracle(|o| o.settled_elsewhere(b, n)),
            _ => false,
        }
    }
}

/// The text of a scope value ([F08 §10.3.1]): `<lang>:` and the segments outermost first, joined by `/` — a Rust
/// segment as `<skind> <name>`, a Markdown heading as `#`×level and its text, a TOML segment by its name; the bytes in
/// hex when they do not decode.
pub fn scope_text(b: &[u8]) -> String {
    fn uvar(b: &[u8], i: &mut usize) -> Option<usize> {
        let mut v: usize = 0;
        for k in 0..5 {
            let x = *b.get(*i)?;
            *i += 1;
            v |= usize::from(x & 0x7f) << (7 * k);
            if x & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    fn vstr<'b>(b: &'b [u8], i: &mut usize) -> Option<&'b str> {
        let n = uvar(b, i)?;
        let s = std::str::from_utf8(b.get(*i..*i + n)?).ok()?;
        *i += n;
        Some(s)
    }
    fn decode(b: &[u8]) -> Option<String> {
        let (lang, n) = (*b.first()?, *b.get(1)?);
        let mut i = 2;
        let mut segs = Vec::new();
        for _ in 0..n {
            let skind = *b.get(i)?;
            i += 1;
            let name = vstr(b, &mut i)?;
            let _qual = vstr(b, &mut i)?;
            segs.push(match lang {
                1 => {
                    const K: [&str; 9] = [
                        "mod",
                        "impl",
                        "fn",
                        "struct",
                        "enum",
                        "trait",
                        "const",
                        "static",
                        "macro_rules",
                    ];
                    format!("{} {name}", K.get(usize::from(skind).checked_sub(1)?)?)
                }
                2 => format!("{} {name}", "#".repeat(usize::from(skind))),
                _ => name.to_string(),
            });
        }
        let l = match lang {
            1 => "rust",
            2 => "markdown",
            _ => "toml",
        };
        (i == b.len()).then(|| format!("{l}:{}", segs.join("/")))
    }
    if b.is_empty() {
        return String::new();
    }
    decode(b).unwrap_or_else(|| hex(b))
}
