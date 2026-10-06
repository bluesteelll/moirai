//! LQ-3's evaluator ([50 §8.2] LQ-3; PLAN WP-93b): nested loops over the model's materialised states with the
//! binding and counting semantics of [50 §3.4] (bags, distinct edges, endpoint pairs), walk-bounded reachability by
//! levels ([50 §3.7]), the absent-value logic of [50 §3.3], the total order of [50 §3.5], derived predicates by
//! definition, runtime state at branch tips, link states from the model of [40]'s resolver, history relations by
//! replay over the commit DAG, aggregates by grouping, and `search()` with BM25 and with the statistics-free scorer.
//! `TX` blocks run through the model's write kernel ([`txrun`]), with `EXPECT`, `ASSERT`, `IF TIP`, `IF TARGETS`, the
//! `DRY` diff and the target-set digest of [LQ/envelope §9.4].
//!
//! The ablation switches of [50 §7.4] item 7 that a query decides are [`Ablations`], the display spelling of
//! quantifiers included (the binder caller's [`crate::lq::ctx::Caller::display`]); the card and its examples are the
//! runner's, and the Cypher-spelling tolerance is the parser's strict-GQL mode ([`QueryReq::strict_gql`]).
//!
//! Written from the specification only (S2): no engine code is read or shared.

pub mod expr;
pub mod func;
pub mod hist;
pub mod pat;
pub mod query;
pub mod rel;
pub mod search;
pub mod txrun;
pub mod val;
pub mod view;

#[cfg(test)]
pub(crate) mod tests;

pub use val::V;

use crate::api::{Ctx, Store};
use crate::err::{Refusal, Res};
use crate::lq::bind::{self, Bound, LintKind};
use crate::lq::cast::{CDefine, CExpr, CQuery, Root, encode};
use crate::lq::catalog::Ty;
use crate::lq::ctx::{BindCtx, Params};
use crate::lq::diag::{Code, Diag};
use crate::lq::parser::{ParseOptions, parse_read};
use crate::state::State;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use view::{View, World};

/// The ablation switches of [50 §7.4] item 7 that the model implements; every switch off is the specified language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ablations {
    /// D8 off: SQL/GQL three-valued logic instead of the absent-value rule ([50 §3.3]).
    pub three_valued: bool,
    /// D11: set semantics instead of bags ([50 §3.4]): each assignment of the named bindings once, `RETURN`
    /// deduplicating (with its count before), and `count(*)` over anonymous elements refused.
    pub set_counting: bool,
    /// BFS-distance hop bounds instead of walk lengths ([50 §3.7] item 2).
    pub bfs_hops: bool,
    /// Reverse aliases off: a reverse name is an unknown edge type ([50 §3.2]).
    pub no_reverse_aliases: bool,
    /// The reading echo off ([LQ/envelope §4]).
    pub no_echo: bool,
    /// Error texts without suggestions: no inline rewrite, help or did-you-mean ([LQ/errors §2.4]).
    pub no_suggestions: bool,
    /// W07 off ([50 §3.2]).
    pub no_w07: bool,
    /// The statistics-free scorer instead of BM25 ([50 §5.5]).
    pub stat_free: bool,
    /// The GQL display spelling of quantifiers instead of the Cypher one in the reading echo and the error rewrites
    /// (`HOLE(LQ-display-spelling)`, [LQ/gql-spelling §4.2]).
    pub gql_display: bool,
}

/// A warning or notice of a result ([LQ/errors §4.2], §5.6): its code, its first line without the code prefix, its
/// continuation lines, and the count a counted code sums.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The code.
    pub code: Code,
    /// The first line's text after `<code>: `.
    pub message: String,
    /// Continuation lines.
    pub detail: Vec<String>,
    /// The count of a counted code (W01, W10, N10).
    pub count: Option<u64>,
    /// What identifies the note among others of its code.
    pub(crate) key: String,
}

/// The executor's counts ([LQ/errors §5.6]): rows W01's absence excluded by field, rows W10's `'none'` admitted, rows
/// with a division by zero.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    /// W01, by field.
    pub w01: BTreeMap<String, u64>,
    /// W10.
    pub w10: u64,
    /// N10.
    pub n10: u64,
}

/// What a `Query` evaluates ([API §14.1]).
#[derive(Clone, Copy, Debug)]
pub enum Input<'a> {
    /// `lq`: LQ text (`read_input`).
    Lq(&'a str),
    /// `ir`: a JSON IR document as the converter gives it, its S-AST ([LQ/json-ir]).
    Ast(&'a crate::lq::ast::Read),
    /// `name`: a named query run by name with `params` (R2, [LQ/canonical-ast §5.9]).
    Named(&'a str),
}

/// `mode` ([API §14.1]): `run`, or `check` (bind only, [LQ/envelope §10.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Evaluate.
    #[default]
    Run,
    /// Bind and report the columns, warnings and notices of the binder.
    Check,
}

/// One `Query` ([API §14.1]).
#[derive(Clone, Debug)]
pub struct QueryReq<'a> {
    /// The query.
    pub input: Input<'a>,
    /// `params`.
    pub params: Params,
    /// `use` (`--at`).
    pub at: Option<String>,
    /// `mode`.
    pub mode: Mode,
    /// The strict-GQL spelling mode ([LQ/grammar-v1.ebnf §G]; the Cypher-spelling ablation).
    pub strict_gql: bool,
    /// The ablations.
    pub ablations: Ablations,
}

impl<'a> QueryReq<'a> {
    /// A request to run LQ text with no parameters.
    pub fn lq(text: &'a str) -> QueryReq<'a> {
        QueryReq {
            input: Input::Lq(text),
            params: Params::new(),
            at: None,
            mode: Mode::Run,
            strict_gql: false,
            ablations: Ablations::default(),
        }
    }
}

/// A read's result (family R, [LQ/envelope §7]): columns and typed rows, the view of each part, warnings, notices, the
/// reading echo, the cursor class and the query hash `H` ([LQ/canonical-ast §7.1]).
#[derive(Clone, Debug, PartialEq)]
pub struct QueryOut {
    /// The columns' names.
    pub cols: Vec<String>,
    /// The columns' types.
    pub types: Vec<Ty>,
    /// The rows, totally ordered ([50 §3.5]).
    pub rows: Vec<Vec<V>>,
    /// The view of each part.
    pub views: Vec<View>,
    /// Warnings, by code.
    pub warnings: Vec<Note>,
    /// Notices, by code.
    pub notices: Vec<Note>,
    /// The reading-echo lines, without `reads: ` ([LQ/envelope §4.2]).
    pub reads: Vec<String>,
    /// The query reads runtime or tree-derived state ([50 §3.5]).
    pub live: bool,
    /// `H`.
    pub hash: [u8; 16],
    /// D11: the row count before deduplication, when the ablation removed rows ([50 §3.4]).
    pub dedup_from: Option<usize>,
    /// The exit code: 3 when a `SHAPE detail` run misses a requested id ([LQ/envelope §5.6]), else 0.
    pub exit: u8,
}

/// Why a `Query` did not run: the located diagnostics of its text (lexer, parser, binder; [LQ/errors §3]), which the
/// reference renderer prints, or a refusal of its evaluation (view resolution, a tip-only read, a budget).
#[derive(Clone, Debug)]
pub enum QueryError {
    /// The text's diagnostics, at most three ([LQ/grammar-v1.ebnf §P.14]).
    Text(Vec<Diag>),
    /// A refusal of the evaluation.
    Run(Refusal),
}

impl From<Refusal> for QueryError {
    fn from(r: Refusal) -> QueryError {
        QueryError::Run(r)
    }
}

impl QueryError {
    /// The refusal of a command ([API §14.1]: refusals are LQ codes): the first diagnostic's code and exit.
    pub fn refusal(self) -> Refusal {
        match self {
            QueryError::Text(d) => crate::lqh::refusal(&d[0]).0,
            QueryError::Run(r) => r,
        }
    }
}

/// Diagnostics with the ablation that strips suggestions applied ([50 §7.4] item 7).
fn texts(mut d: Vec<Diag>, ab: &Ablations) -> QueryError {
    if ab.no_suggestions {
        for x in &mut d {
            x.inline = None;
            x.help = None;
            x.suggest = Box::default();
        }
    }
    QueryError::Text(d)
}

/// The LQ schema of a view, with the reverse-alias ablation applied.
fn schema_of(st: &State, ab: &Ablations) -> crate::lq::schema::Schema {
    let mut s = crate::lqh::lq_schema(&st.schema);
    if ab.no_reverse_aliases {
        for e in &mut s.edges {
            e.reverse.clear();
        }
    }
    s
}

/// A project named query bound against a view's schema ([50 §3.9] item 5).
pub(crate) struct ProjectQuery {
    /// Its C-AST.
    pub cast: CDefine,
    /// Its columns.
    pub columns: Vec<(String, Ty)>,
    /// Its binding names.
    pub names: Vec<String>,
}

/// Binds a project named query's stored text against the view's schema ([50 §3.9] item 5); a text that does not bind
/// there is E109 with the binder's text for a callee that does not bind ([F19 §12.5.3] `QueryInvalid`).
pub(crate) fn bind_project_query(
    store: &Store,
    st: &State,
    name: &str,
    text: &str,
) -> Res<ProjectQuery> {
    let schema = schema_of(st, &Ablations::default());
    let commits = crate::lqh::commit_table(&store.dag);
    let ids = crate::lqh::ViewIds {
        uids: &store.alloc.uids,
        uidx: &store.alloc.uidx,
        st,
        next_id: store.next_id,
        commits: &commits,
    };
    let q = crate::lq::catalog::named_query_in(name, text, &schema, &ids).map_err(|e| {
        let first = e
            .first()
            .map_or_else(String::new, |d| format!(": {} {}", d.code, d.message));
        Refusal::lq(
            "E109",
            format!(
                "named query {} does not bind{first} (QueryInvalid)",
                crate::lq::diag::q(name)
            ),
        )
    })?;
    Ok(ProjectQuery {
        cast: q.cast,
        columns: q.columns,
        names: q.names,
    })
}

/// Parses `--at REV` into the revision node of its part's view ([LQ/canonical-ast §5.6]).
pub(crate) fn at_rev(store: &Store, text: &str) -> Res<CExpr> {
    use crate::lq::ast::{RevKind, Suffix};
    let lx = crate::lq::lexer::Lexer::new(text);
    let mut toks = Vec::new();
    let (rev, end) = lx
        .revspec(0, &mut toks)
        .map_err(|_| Refusal::usage(format!("{text} is not a revision")))?;
    if end != text.len() {
        return Err(Refusal::usage(format!("{text} is not a revision")));
    }
    fn conv(store: &Store, r: &crate::lq::ast::Rev, text: &str) -> Res<CExpr> {
        Ok(match &r.kind {
            RevKind::Head => CExpr::RHead,
            RevKind::Ref(n) => CExpr::RRef(n.clone()),
            RevKind::Commit(h) => match store.dag.by_prefix(h).as_slice() {
                [c] => CExpr::RCommit(store.dag.commits[c].id),
                _ => return Err(Refusal::lq("E301", format!("unknown revision {text}"))),
            },
            RevKind::Seq(n) => match store.dag.commits.get(n) {
                Some(c) => CExpr::RCommit(c.id),
                None => return Err(Refusal::lq("E301", format!("unknown revision {text}"))),
            },
            RevKind::Param(_) => return Err(Refusal::usage(format!("{text} is not a revision"))),
            RevKind::Suf(b, s) => {
                let base = Box::new(conv(store, b, text)?);
                match s {
                    Suffix::Tilde(n) => CExpr::RSuf(base, 1, i64::from(*n)),
                    Suffix::Caret(n) => CExpr::RSuf(base, 2, i64::from(*n)),
                    Suffix::At(n) => CExpr::RSuf(base, 3, i64::from(*n)),
                    Suffix::AtTime(t) => CExpr::RSuf(base, 4, crate::lq::lexer::datetime_ms(t)),
                }
            }
        })
    }
    conv(store, &rev, text)
}

impl Store {
    /// `Query` ([API §14.1]): parses (or takes the converter's tree), binds against the default view's schema with the
    /// caller of [LQ/canonical-ast §5.9], evaluates part by part, and returns the rows with the binder's and the
    /// executor's warnings and notices. A read appends nothing ([40] I-F5).
    // spec: [API §14.1]
    pub fn query(&self, req: &QueryReq<'_>, ctx: &Ctx) -> Res<QueryOut> {
        self.query_full(req, ctx).map_err(QueryError::refusal)
    }

    /// [`Store::query`] with the located diagnostics of a text that does not parse or bind.
    pub fn query_full(&self, req: &QueryReq<'_>, ctx: &Ctx) -> Result<QueryOut, QueryError> {
        let caller = self.resolve(ctx, false)?;
        let ab = req.ablations;
        let at = req.at.as_deref().map(|t| at_rev(self, t)).transpose()?;
        // The binder reads the schema of the default view: `--at`, else the caller's branch tip.
        let st = match &at {
            Some(e) => {
                let w = self.world(&caller, ctx, ab, None);
                w.open(Some(e))?.st
            }
            None => {
                let tip = if caller.branch.is_empty() {
                    caller.detached
                } else {
                    self.dag.live(&caller.branch).and_then(|r| r.tip)
                };
                self.dag.state_at(tip, &self.alloc)
            }
        };
        let schema = schema_of(&st, &ab);
        let commits = crate::lqh::commit_table(&self.dag);
        let ids = crate::lqh::ViewIds {
            uids: &self.alloc.uids,
            uidx: &self.alloc.uidx,
            st: &st,
            next_id: self.next_id,
            commits: &commits,
        };
        let rights = self.rights(&caller, ctx);
        let mut lq_caller = self.lq_caller(&caller, ctx, &rights, false);
        lq_caller.at = req.at.clone();
        lq_caller.display = if ab.gql_display {
            crate::lq::printer::Spelling::Gql
        } else {
            crate::lq::printer::Spelling::Cypher
        };
        lq_caller.tree = self.tree_ctx(&caller, ctx).is_some_and(|t| t.eligible);
        let bctx = BindCtx {
            schema: &schema,
            ids: &ids,
            params: &req.params,
            caller: &lq_caller,
        };
        let opts = ParseOptions {
            strict_gql: req.strict_gql,
        };
        let bound: Bound<CQuery> = match req.input {
            Input::Lq(text) => {
                let p = parse_read(text, opts).map_err(|e| texts(e, &ab))?;
                bind::bind_read(&bctx, text, &p.tree).map_err(|e| texts(e, &ab))?
            }
            Input::Ast(tree) => bind::bind_read(&bctx, "", tree).map_err(|e| texts(e, &ab))?,
            Input::Named(name) => bind::bind_named(&bctx, name).map_err(|e| texts(e, &ab))?,
        };
        let h = blake3::hash(&encode(Root::Query(&bound.ast)));
        let mut hash = [0u8; 16];
        hash.copy_from_slice(&h.as_bytes()[..16]);
        let mut w = self.world(&caller, ctx, ab, at);
        w.catalog = Some(st.clone());
        w.names.borrow_mut().push(Rc::new(bound.names.clone()));
        let table = match req.mode {
            Mode::Check => query::Table::default(),
            Mode::Run => w.eval_query(&bound.ast, &query::Frame::default(), None)?,
        };
        let counts = w.counts.borrow().clone();
        let mut warnings = Vec::new();
        let mut notices = Vec::new();
        for l in &bound.lints {
            if ab.no_w07 && l.code == Code::W07 {
                continue;
            }
            let count = match (&l.kind, req.mode) {
                (_, Mode::Check) => None,
                (LintKind::W01 { field, .. }, _) => {
                    Some(counts.w01.get(field).copied().unwrap_or(0))
                }
                (LintKind::W10 { .. }, _) => Some(counts.w10),
                _ => None,
            };
            let note = Note {
                code: l.code,
                message: l.message(count),
                detail: Vec::new(),
                count,
                key: format!("{:?}", l.kind),
            };
            if l.code.as_str().starts_with('W') {
                warnings.push(note);
            } else {
                notices.push(note);
            }
        }
        if counts.n10 > 0 {
            w.note(
                Code::N10,
                format!("division by zero gave absent in {} rows", counts.n10),
                Vec::new(),
                Some(counts.n10),
            );
        }
        if req.mode == Mode::Run && table.rows.is_empty() {
            self.n07(&w, &bound.ast);
        }
        let missing =
            req.mode == Mode::Run && bound.detail && self.detail_misses(&w, &bound, &table);
        for n in w.notes.borrow().iter() {
            let mut n = n.clone();
            if n.code == Code::N10 {
                n.count = Some(counts.n10);
            }
            if n.code.as_str().starts_with('W') {
                warnings.push(n);
            } else {
                notices.push(n);
            }
        }
        warnings.sort_by_key(|n| n.code);
        notices.sort_by_key(|n| n.code);
        Ok(QueryOut {
            cols: bound.columns.iter().map(|c| c.0.clone()).collect(),
            types: bound.columns.iter().map(|c| c.1.clone()).collect(),
            rows: table.rows,
            views: table.views,
            warnings,
            notices,
            reads: if ab.no_echo { Vec::new() } else { bound.reads },
            live: bound.live,
            hash,
            dedup_from: table.dedup_from,
            exit: if missing { 3 } else { 0 },
        })
    }

    /// The requested ids of a `SHAPE detail` run that yield no row ([LQ/envelope §5.6]; [LQ/errors] open point 6): the
    /// node arguments of the call, ascending, each that is not live in the part's view and heads no row reported by
    /// N01 (deleted) or N06 (another branch), then each id the binder did not know by N12. Whether any is missing.
    // spec: [LQ/envelope §5.6]
    fn detail_misses(&self, w: &World<'_>, bound: &Bound<CQuery>, table: &query::Table) -> bool {
        use crate::lq::cast::CBody;
        let mut requested: Vec<crate::value::Nid> = Vec::new();
        if let CBody::Call(call) = &bound.ast.first.body {
            for a in &call.args {
                let one = |e: &CExpr, out: &mut Vec<crate::value::Nid>| {
                    if let CExpr::Node(u) = e
                        && let Some(n) = self.alloc.uidx.get(&crate::value::Uid(*u))
                    {
                        out.push(*n);
                    }
                };
                match &a.value {
                    CExpr::List(items) => items.iter().for_each(|e| one(e, &mut requested)),
                    e => one(e, &mut requested),
                }
            }
        }
        requested.sort_unstable();
        requested.dedup();
        let mut missing = false;
        if let Some(view) = table.views.first() {
            let ev = view::Ev::new(w, view);
            for n in requested {
                let heads = table
                    .rows
                    .iter()
                    .any(|r| r.first().and_then(V::node) == Some(n));
                if heads || ev.node(n).is_some_and(|x| x.live()) {
                    continue;
                }
                ev.literal_notice(n, &[]);
                missing = true;
            }
        }
        for id in &bound.unknown_ids {
            w.note(
                Code::N12,
                format!("{id} was never allocated in this store"),
                Vec::new(),
                None,
            );
            missing = true;
        }
        missing
    }

    /// The evaluation inputs of one caller.
    pub(crate) fn world<'s>(
        &'s self,
        caller: &'s crate::api::Caller,
        ctx: &Ctx,
        ab: Ablations,
        at: Option<CExpr>,
    ) -> World<'s> {
        World {
            store: self,
            caller,
            tree: self.tree_ctx(caller, ctx),
            ab,
            notes: RefCell::new(Vec::new()),
            counts: RefCell::new(Counts::default()),
            names: RefCell::new(Vec::new()),
            at,
            catalog: None,
            leases: None,
        }
    }

    /// N07 ([LQ/errors §5.6]; [50 §2.9]): an empty result whose first part's first `MATCH` has a typed directed edge
    /// pattern that would match with its direction reversed: the count of such edges, the pattern the other way, and
    /// the reverse alias when the kind has one.
    // spec: [LQ/errors §5.6] N07
    fn n07(&self, w: &World<'_>, q: &CQuery) {
        use crate::lq::cast::{CBody, CClause, CStep};
        let CBody::Clauses(clauses, _) = &q.first.body else {
            return;
        };
        let Some(CClause::Match {
            patterns, where_, ..
        }) = clauses.first()
        else {
            return;
        };
        let Ok(view) = w.open(q.first.use_.as_ref()) else {
            return;
        };
        for (pi, p) in patterns.iter().enumerate() {
            for (si, s) in p.steps.iter().enumerate() {
                let CStep::Edge(e, _) = s else { continue };
                let [(lq, dir)] = e.types.as_slice() else {
                    continue;
                };
                if e.quant.is_some() || !matches!(dir, 1 | 2) {
                    continue;
                }
                let mut flipped = patterns.clone();
                if let CStep::Edge(fe, _) = &mut flipped[pi].steps[si] {
                    fe.types[0].1 = 3 - *dir;
                }
                let ev = view::Ev::new(w, &view);
                let mut out = Vec::new();
                if ev
                    .match_paths(
                        &expr::Row::default(),
                        &flipped,
                        where_.as_ref(),
                        &[],
                        &mut out,
                        false,
                    )
                    .is_err()
                    || out.is_empty()
                {
                    continue;
                }
                let names = w.names.borrow().last().cloned().unwrap_or_default();
                let left = match si.checked_sub(1).map(|k| &p.steps[k]) {
                    None => &p.start,
                    Some(CStep::Edge(_, n) | CStep::Group(_, n)) => n,
                };
                let right = match s {
                    CStep::Edge(_, n) | CStep::Group(_, n) => n,
                };
                let (a, b) = (
                    node_text(left, &names, self),
                    node_text(right, &names, self),
                );
                let (src, dst) = if *dir == 1 {
                    (b.clone(), a.clone())
                } else {
                    (a.clone(), b.clone())
                };
                let alias = view
                    .st
                    .schema
                    .edges()
                    .into_iter()
                    .find(|x| x.lq_name == *lq)
                    .and_then(|x| x.reverse.first().cloned());
                let also = alias.map_or(String::new(), |r| {
                    format!(", also written ({dst})-[:{r}]->({src})")
                });
                w.note(
                    Code::N07,
                    format!(
                        "nothing matched, but {} {lq} edges point the other way: ({src})-[:{lq}]->({dst}){also}",
                        out.len()
                    ),
                    Vec::new(),
                    None,
                );
                return;
            }
        }
    }
}

/// The text of a node pattern in N07: its variable, else its anchored id, else `()`.
fn node_text(n: &crate::lq::cast::CNode, names: &[String], store: &Store) -> String {
    if let Some(b) = n.var {
        return names.get(b as usize).cloned().unwrap_or_else(|| "n".into());
    }
    match n.props.iter().find(|(k, _)| k == "id") {
        Some((_, CExpr::Node(u))) => store
            .alloc
            .uidx
            .get(&crate::value::Uid(*u))
            .map_or_else(String::new, |x| x.to_string()),
        _ => String::new(),
    }
}
