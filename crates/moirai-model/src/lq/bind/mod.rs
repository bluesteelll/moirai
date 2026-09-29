//! The binder of LQ-3 ([LQ/canonical-ast §5], [50 §3.2], [50 §5.3]): it resolves every name of an S-AST against the
//! binding context ([`BindCtx`], §5.1), types expressions, coerces literals and parameters by their use site (§5.5),
//! resolves revisions (§5.6), renames variables by the scope rules V1–V11 (§5.7), applies the structural
//! normalisations N1–N7 (§5.8) and produces the C-AST of one root (§5.9) with the diagnostics of [LQ/errors] that the
//! binder raises, the warnings and notices it decides (W01, W02, W07, W10, N08), the reading echo of
//! [LQ/envelope §4] and the portable form of definitions ([LQ/canonical-ast §8]).
//!
//! Entry points: [`bind_read`] (R1), [`bind_named`] (R2), [`bind_write`] (R3), [`bind_mutation`] (R4) and
//! [`bind_define`] (R6). A verb (R5) is its expansion's `TX` text, bound with [`bind_write`] after the verb's flags
//! joined the block's options. A bind runs in place or on the front end's stack (`run_bind`).

mod call;
mod expr;
mod pattern;
mod tx;
mod value;

use crate::lq::ast::*;
use crate::lq::cast::*;
use crate::lq::catalog::Ty;
use crate::lq::ctx::{BindCtx, Surface};
use crate::lq::diag::{Code, Diag, Span, q};
use crate::lq::parser::{INLINE_NESTING, nesting_bound, on_front_end_stack};
use crate::lq::printer;
use crate::lq::schema::KindSet;
use call::NamedMemo;
use std::collections::BTreeSet;

pub use tx::portable_text;

/// A successful bind: the C-AST root and what the binder decided about it.
#[derive(Clone, Debug)]
pub struct Bound<T> {
    /// The C-AST root.
    pub ast: T,
    /// Warnings and notices, in order of detection.
    pub lints: Vec<Lint>,
    /// The reading-echo lines, without `reads: ` ([LQ/envelope §4.2]), each once, in order of first appearance.
    pub reads: Vec<String>,
    /// The cursor class: `live` when the root reads runtime or tree-derived state ([50 §3.5], [LQ/std §2.5]).
    pub live: bool,
    /// The result columns of a query root: (name, type); the first part's for a composite query.
    pub columns: Vec<(String, Ty)>,
    /// The portable texts of the definitions bound ([LQ/canonical-ast §8.1]): (query name, stored text).
    pub portable: Vec<(String, String)>,
}

/// A warning or notice the binder decides ([LQ/errors §5.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lint {
    /// `W01`, `W02`, `W07`, `W10` or `N08`.
    pub code: Code,
    /// The span it concerns.
    pub span: Span,
    /// The values its text interpolates.
    pub kind: LintKind,
}

/// The values of a lint's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LintKind {
    /// W01: an ordered comparison of a property that is optional on some kind of its variable.
    W01 {
        /// The field.
        field: String,
        /// The property expression as written.
        expr: String,
    },
    /// W02: a match mode that changes nothing.
    W02 {
        /// The mode as the text prints it (`TRAIL`).
        mode: String,
    },
    /// W07: hand-derived readiness.
    W07 {
        /// The task variable.
        var: String,
    },
    /// W10: `<>` or `NOT IN` over `link_state()` of a node that may have no `AT` edge.
    W10 {
        /// The node variable.
        var: String,
        /// `<>` or `NOT IN`.
        op: String,
    },
    /// N08: an aggregate over bindings of a quantified part.
    N08 {
        /// The aggregate as written.
        agg: String,
        /// The part's left endpoint.
        x: String,
        /// The part's right endpoint.
        y: String,
    },
}

impl Lint {
    /// The first line of the text without its `Wnn: ` prefix ([LQ/errors §5.6]); `n` is the executor's count where
    /// the text carries one (W01, W10).
    pub fn message(&self, n: Option<u64>) -> String {
        let n = n.map_or_else(|| "<n>".to_string(), |n| n.to_string());
        match &self.kind {
            LintKind::W01 { field, expr } => {
                format!(
                    "{n} rows excluded because {field} is absent; use coalesce({expr}, 0) or {expr} IS NULL"
                )
            }
            LintKind::W02 { mode } => {
                format!(
                    "{mode} changes nothing: fixed parts bind distinct edges and quantified parts bind endpoint pairs"
                )
            }
            LintKind::W07 { var } => format!(
                "hand-derived readiness misses inherited and flagged blockers, markers, leases, defer_until and containers; use {var}.unblocked (structural) or {var}.ready (dispatchable now)"
            ),
            LintKind::W10 { var, op } => format!(
                "{n} rows passed {op} because link_state({var}) is 'none' (no AT edge); to keep linked nodes only, add EXISTS {{ ({var})-[:AT]->() }}"
            ),
            LintKind::N08 { agg, x, y } => format!(
                "{agg} over a quantified pattern counts ({x}, {y}) endpoint pairs, not paths"
            ),
        }
    }
}

/// What kind of value a binding holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BKind {
    Node,
    Edge,
    Value,
}

/// One binding (a variable of §5.7, or an anonymous pattern element used for kind inference).
#[derive(Clone, Debug)]
struct BInfo {
    name: String,
    ty: Ty,
    kind: BKind,
    /// An edge variable of a quantified edge (E113).
    quant_edge: bool,
    /// The endpoint's display in the reading echo ([LQ/envelope §4.3]).
    display: String,
}

/// The view a part reads, for E302 ([50 §3.8]) and the E304 count.
#[derive(Clone, Debug, PartialEq, Eq)]
enum View {
    /// A branch tip (`main`, `lane/*`, `plan/*`, `HEAD`).
    Tip,
    /// A past or read-only view; the revspec as written.
    Past(String),
    /// A view decided at run time (a definition's `USE $param`).
    Open,
}

/// Where an aggregate may stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AggPos {
    /// A `WITH` or `RETURN` item or sort key.
    Allowed,
    /// A `WHERE` (E112, first form).
    Where,
    /// Inside another aggregate (E112, second form).
    Nested,
    /// Anywhere else (E112, [LQ/errors] has no form for it; see the spec findings).
    Other,
}

/// The definition being bound: its parameters in declaration order.
#[derive(Clone, Debug)]
struct DefCtx {
    name: String,
    params: Vec<(String, Ty)>,
}

/// The binder of one root.
struct Binder<'a> {
    ctx: &'a BindCtx<'a>,
    src: &'a str,
    errors: Vec<Diag>,
    lints: Vec<Lint>,
    reads: Vec<String>,
    live: bool,
    b: Vec<BInfo>,
    scope: Vec<(String, BindingId)>,
    steps: Vec<String>,
    def: Option<DefCtx>,
    view: View,
    view_names: BTreeSet<String>,
    agg: AggPos,
    saw_agg: bool,
    quant_part: Option<(String, String)>,
    /// Inside a quantified group whose own echo line stands for its edges ([LQ/envelope §4.3]): edge patterns and
    /// nested groups print no line of their own.
    mute_echo: bool,
    n08_done: bool,
    readiness_used: bool,
    hand_derived: Option<String>,
    blocks_sources: Vec<(BindingId, String)>,
    rewrites: Vec<(Span, String)>,
    portable: Vec<(String, String)>,
    named: &'a NamedMemo,
    aliases: Option<ItemAliases>,
}

/// The aliased items of the `RETURN` whose sort keys are binding (V5): (alias, item index, item type), and the scope's
/// length when the keys started.
struct ItemAliases {
    items: Vec<(String, u32, Ty)>,
    mark: usize,
}

impl<'a> Binder<'a> {
    fn new(ctx: &'a BindCtx<'a>, src: &'a str, named: &'a NamedMemo) -> Binder<'a> {
        Binder {
            ctx,
            src,
            errors: Vec::new(),
            lints: Vec::new(),
            reads: Vec::new(),
            live: false,
            b: Vec::new(),
            scope: Vec::new(),
            steps: Vec::new(),
            def: None,
            view: View::Tip,
            view_names: BTreeSet::new(),
            agg: AggPos::Other,
            saw_agg: false,
            quant_part: None,
            mute_echo: false,
            n08_done: false,
            readiness_used: false,
            hand_derived: None,
            blocks_sources: Vec::new(),
            rewrites: Vec::new(),
            portable: Vec::new(),
            named,
            aliases: None,
        }
    }

    // ----- diagnostics ---------------------------------------------------------------------------------------------

    fn err(&mut self, d: Diag) {
        self.errors.push(d);
    }

    fn lint(&mut self, code: Code, span: Span, kind: LintKind) {
        if !self.lints.iter().any(|l| l.code == code && l.kind == kind) {
            self.lints.push(Lint { code, span, kind });
        }
    }

    /// The source text of a span.
    fn text(&self, s: Span) -> &'a str {
        self.src.get(s.start as usize..s.end as usize).unwrap_or("")
    }

    /// Ends a bind: at most three errors ([LQ/errors §3.1]), else the root. A policy refusal (E406, E411) comes first,
    /// since its exit code (6) must decide the call whatever else the text holds; then the errors by position, ties
    /// by the smaller code ([F19 §12.5.3]'s first diagnostic); unlocated errors last.
    fn finish<T>(mut self, ast: T, columns: Vec<(String, Ty)>) -> Result<Bound<T>, Vec<Diag>> {
        if let Some(v) = self.hand_derived.take()
            && !self.readiness_used
        {
            self.lint(Code::W07, Span::default(), LintKind::W07 { var: v });
        }
        if !self.errors.is_empty() {
            let mut errors = std::mem::take(&mut self.errors);
            if self.src.is_empty() {
                // R2 and R4 carry no LQ text: nothing to point into ([LQ/errors §3.3]).
                for d in &mut errors {
                    d.span = None;
                }
            }
            errors.sort_by_key(|d| {
                (
                    !matches!(d.code, Code::E406 | Code::E411),
                    d.start(),
                    d.code,
                )
            });
            let mut out: Vec<Diag> = Vec::new();
            for d in errors {
                let dup = out.iter().any(|e| {
                    e.code == d.code
                        && e.span.map(|s| (s.start, s.end)) == d.span.map(|s| (s.start, s.end))
                });
                if !dup && out.len() < 3 {
                    out.push(d);
                }
            }
            return Err(out);
        }
        Ok(Bound {
            ast,
            lints: self.lints,
            reads: self.reads,
            live: self.live,
            columns,
            portable: self.portable,
        })
    }

    // ----- bindings and scopes -------------------------------------------------------------------------------------

    fn new_binding(&mut self, name: &str, ty: Ty, kind: BKind) -> BindingId {
        let id = self.b.len() as BindingId;
        self.b.push(BInfo {
            name: name.to_string(),
            ty,
            kind,
            quant_edge: false,
            display: name.to_string(),
        });
        id
    }

    fn declare(&mut self, name: &str, ty: Ty, kind: BKind) -> BindingId {
        let id = self.new_binding(name, ty, kind);
        self.scope.push((name.to_string(), id));
        id
    }

    fn lookup(&self, name: &str) -> Option<BindingId> {
        self.scope
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|&(_, id)| id)
    }

    /// V5: while a `RETURN`'s sort keys bind, a name that is an alias of one of its items is that item (`ITEMREF(i)`
    /// and the item's type), unless a binding made inside the key (a list predicate's variable) shadows it.
    fn item_alias(&self, name: &str) -> Option<(u32, Ty)> {
        let a = self.aliases.as_ref()?;
        if self.scope.get(a.mark..)?.iter().any(|(n, _)| n == name) {
            return None;
        }
        a.items
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, i, t)| (*i, t.clone()))
    }

    fn all_kinds(&self) -> KindSet {
        self.ctx.schema.all_kinds()
    }

    // ----- views ---------------------------------------------------------------------------------------------------

    /// Classifies a revision as a view ([F12 §2.2]: the namespace fixes the kind of a ref; [50 §3.9] item 6).
    fn view_of(&self, r: &Rev) -> View {
        match &r.kind {
            RevKind::Head => View::Tip,
            RevKind::Ref(name) => {
                if ref_is_branch(name) {
                    View::Tip
                } else {
                    View::Past(name.clone())
                }
            }
            // Inside a definition the view is decided at run time; elsewhere by the bound value.
            RevKind::Param(p) if self.def.is_none() => match self.ctx.params.get(p) {
                Some(crate::lq::ctx::Value::Text(t)) => {
                    match crate::lq::lexer::Lexer::new(t).revspec(0, &mut Vec::new()) {
                        Ok((rev, end))
                            if end == t.len() && !matches!(rev.kind, RevKind::Param(_)) =>
                        {
                            match self.view_of(&rev) {
                                View::Past(_) => View::Past(t.clone()),
                                v => v,
                            }
                        }
                        _ => View::Open,
                    }
                }
                Some(crate::lq::ctx::Value::Int(n)) => View::Past(format!("s{n}")),
                _ => View::Open,
            },
            RevKind::Param(_) => View::Open,
            _ => View::Past(self.text(r.span).to_string()),
        }
    }

    /// E302 when the current part's view is not a branch tip, or (for tree-derived state) no tree resolves.
    fn tip_only(
        &mut self,
        what: &str,
        prop: &str,
        span: Span,
        tree: bool,
        ready_hint: Option<&str>,
    ) {
        self.live = true;
        if let View::Past(revspec) = &self.view {
            let mut d = Diag::new(
                Code::E302,
                span,
                format!(
                    "{} uses {what}, which exists only at a branch tip; the view is {} (as-of)",
                    q(prop),
                    q(revspec)
                ),
            );
            if let Some(v) = ready_hint {
                d = d.inline(format!(
                    "use {v}.unblocked (structural, valid at any version)"
                ));
            }
            self.err(d);
        } else if tree && !self.ctx.caller.tree {
            self.err(
                Diag::new(
                    Code::E302,
                    span,
                    format!("{} needs a resolved tree", q(prop)),
                )
                .help("pass --tree DIR (the query tool: tree)"),
            );
        }
    }

    // ----- queries -------------------------------------------------------------------------------------------------

    fn query(&mut self, qy: &Query) -> (CQuery, Vec<(String, Ty)>) {
        let outer = std::mem::take(&mut self.scope);
        let outer_steps = std::mem::take(&mut self.steps);
        let (first, columns) = self.part(&qy.parts[0]);
        let mut rest = Vec::new();
        for (op, part) in qy.ops.iter().zip(&qy.parts[1..]) {
            let (p, _) = self.part(part);
            let code = match op {
                SetOp::Union => 1,
                SetOp::UnionAll => 2,
                SetOp::Except => 3,
                SetOp::Intersect => 4,
            };
            rest.push((code, p));
        }
        let views = self.view_names.len() as u32;
        if qy.parts.len() > 1 && views > self.ctx.caller.refs {
            self.too_many_refs(qy.span, views);
        }
        self.scope = outer;
        self.steps = outer_steps;
        (CQuery { first, rest }, columns)
    }

    fn too_many_refs(&mut self, span: Span, n: u32) {
        let refs = self.ctx.caller.refs;
        let ceiling = if matches!(self.ctx.caller.role.as_str(), "orchestrator" | "owner") {
            80
        } else {
            8
        };
        self.err(
            Diag::new(
                Code::E304,
                span,
                format!("{n} views requested; the budget is refs={refs}"),
            )
            .help(format!("--budget refs={n} (at most {ceiling})")),
        );
    }

    fn part(&mut self, p: &Part) -> (CPart, Vec<(String, Ty)>) {
        self.scope.clear();
        self.steps.clear();
        self.quant_part = None;
        let saved_view = self.view.clone();
        let use_ = match (&p.use_, &self.ctx.caller.at) {
            (Some(r), _) => {
                self.view = self.view_of(r);
                self.view_names.insert(self.text(r.span).to_string());
                Some(self.rev(r))
            }
            (None, Some(at)) => {
                let at = at.clone();
                match self.rev_text(&at, p.span, false) {
                    Some(lit) => {
                        self.view = self.view_of(&lit.first);
                        self.view_names.insert(at);
                        Some(lit.c)
                    }
                    None => None,
                }
            }
            (None, None) => {
                self.view_names.insert(self.ctx.caller.branch.clone());
                if !ref_is_branch(&self.ctx.caller.branch) {
                    self.view = View::Past(self.ctx.caller.branch.clone());
                }
                None
            }
        };
        let (body, columns) = match &p.body {
            PartBody::Call(c) => {
                let (c, cols) = self.scall(c);
                (CBody::Call(c), cols)
            }
            PartBody::Clauses { clauses, ret } => {
                let mut cs = Vec::with_capacity(clauses.len());
                for c in clauses {
                    if let Some(c) = self.clause(c) {
                        cs.push(c);
                    }
                }
                let (r, cols) = self.ret(ret);
                (CBody::Clauses(cs, r), cols)
            }
        };
        self.view = saved_view;
        (CPart { use_, body }, columns)
    }

    fn clause(&mut self, c: &Clause) -> Option<CClause> {
        match c {
            Clause::Match(m) => {
                if let Some(mode) = m.mode {
                    let name = match mode {
                        MatchMode::Walk => Some("WALK"),
                        MatchMode::Trail => Some("TRAIL"),
                        MatchMode::Acyclic => Some("ACYCLIC"),
                        MatchMode::Simple => Some("SIMPLE"),
                        MatchMode::Different => None,
                    };
                    if let Some(name) = name {
                        self.lint(
                            Code::W02,
                            m.span,
                            LintKind::W02 {
                                mode: name.to_string(),
                            },
                        );
                    }
                }
                // An OPTIONAL MATCH keeps its input rows whatever it matches, so the kinds it infers for variables bound
                // before it hold inside the clause only.
                let kept = m.optional.then(|| self.kinds_snapshot());
                let (patterns, where_) = self.match_patterns(&m.patterns, m.where_.as_ref(), true);
                if let Some(k) = kept {
                    self.restore_kinds(k);
                }
                Some(CClause::Match {
                    optional: m.optional,
                    patterns,
                    where_,
                })
            }
            Clause::Call(c) => Some(self.call_clause(c)),
            Clause::Unwind(u) => {
                let (e, t) = self.expr_at(&u.expr, None, AggPos::Other);
                let elem = match t {
                    Ty::List(e) => *e,
                    Ty::Any | Ty::Null => Ty::Any,
                    other => {
                        self.err(Diag::new(
                            Code::E103,
                            u.expr.span,
                            format!("UNWIND {}: the types do not match", q(&other.name())),
                        ));
                        Ty::Any
                    }
                };
                let kind = if matches!(elem, Ty::Node(_)) {
                    BKind::Node
                } else {
                    BKind::Value
                };
                let var = self.declare(&u.as_.text, elem, kind);
                Some(CClause::Unwind { expr: e, var })
            }
            Clause::With(w) => self.with(w),
        }
    }

    fn with(&mut self, w: &With) -> Option<CClause> {
        let before = self.scope.clone();
        self.saw_agg = false;
        let mut items = Vec::with_capacity(w.items.len());
        let mut new_scope: Vec<(String, BindingId)> =
            if w.star { before.clone() } else { Vec::new() };
        let mut ok = true;
        for it in &w.items {
            let (e, t) = self.expr_at(&it.expr, None, AggPos::Allowed);
            let var = match (&it.as_, &it.expr.kind) {
                (Some(alias), _) => {
                    let kind = match t {
                        Ty::Node(_) => BKind::Node,
                        Ty::Edge(_) => BKind::Edge,
                        _ => BKind::Value,
                    };
                    let id = self.new_binding(&alias.text, t, kind);
                    new_scope.push((alias.text.clone(), id));
                    id
                }
                (None, ExprKind::Ident(name)) if self.lookup(name).is_some() => {
                    let id = self.lookup(name).unwrap_or_default();
                    new_scope.push((name.clone(), id));
                    id
                }
                (None, _) => {
                    let text = printer::expr_text(&it.expr);
                    self.err(
                        Diag::new(
                            Code::E001,
                            it.expr.span,
                            format!("WITH {} needs a name", q(&text)),
                        )
                        .inline(format!("write WITH {text} AS <name>")),
                    );
                    ok = false;
                    continue;
                }
            };
            items.push((e, var));
        }
        let aggregated = self.saw_agg;
        self.scan_n08(aggregated, &w.items);
        // WHERE and ORDER BY resolve among the items first, then (no DISTINCT, no aggregate) the bindings before.
        self.scope = if !w.distinct && !aggregated {
            let mut s = before.clone();
            s.extend(new_scope.iter().cloned());
            s
        } else {
            new_scope.clone()
        };
        let where_ = w.where_.as_ref().map(|e| self.bool_expr(e, AggPos::Where));
        let order = self.sorts(&w.order, AggPos::Allowed);
        let limit = w.limit.as_ref().map(|l| self.limit(l));
        self.scope = new_scope;
        ok.then_some(CClause::With {
            distinct: w.distinct,
            star: w.star,
            items,
            where_,
            order,
            limit,
        })
    }

    /// N08 when an aggregate consumes bindings of a quantified part ([50 §3.4] item 4).
    fn scan_n08(&mut self, aggregated: bool, items: &[Item]) {
        if !aggregated || self.n08_done {
            return;
        }
        if let Some((x, y)) = self.quant_part.clone() {
            let agg = items
                .iter()
                .find_map(|it| first_aggregate(&it.expr))
                .unwrap_or_else(|| "count(*)".to_string());
            self.n08_done = true;
            self.lint(Code::N08, Span::default(), LintKind::N08 { agg, x, y });
        }
    }

    fn ret(&mut self, r: &Return) -> (CReturn, Vec<(String, Ty)>) {
        self.saw_agg = false;
        let mut items = Vec::with_capacity(r.items.len());
        let mut columns = Vec::with_capacity(r.items.len());
        if r.star {
            for (name, id) in self.scope.clone() {
                columns.push((name, self.b[id as usize].ty.clone()));
            }
        }
        for it in &r.items {
            let (e, t) = self.expr_at(&it.expr, None, AggPos::Allowed);
            let name = it
                .as_
                .as_ref()
                .map_or_else(|| column_name(&it.expr), |a| a.text.clone());
            columns.push((name, t));
            items.push((e, it.as_.as_ref().map(|a| a.text.clone())));
        }
        let aggregated = self.saw_agg;
        self.scan_n08(aggregated, &r.items);
        if !r.group.is_empty() {
            self.group_by(r);
        }
        // ORDER BY (V5): a name resolves first to an aliased item of this RETURN (ITEMREF), wherever it stands in the
        // key, then among the bindings visible before it.
        let first_item = columns.len() - r.items.len();
        let items_by_alias = r
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                let a = it.as_.as_ref()?;
                Some((a.text.clone(), i as u32, columns[first_item + i].1.clone()))
            })
            .collect();
        let outer = self.aliases.replace(ItemAliases {
            items: items_by_alias,
            mark: self.scope.len(),
        });
        let mut order = Vec::with_capacity(r.order.len());
        for s in &r.order {
            order.push(CSort {
                expr: self.expr_at(&s.expr, None, AggPos::Allowed).0,
                desc: s.desc,
            });
        }
        self.aliases = outer;
        let limit = r.limit.as_ref().map(|l| self.limit(l));
        (
            CReturn {
                distinct: r.distinct,
                star: r.star,
                items,
                order,
                limit,
            },
            columns,
        )
    }

    /// The `GROUP BY` check ([50 §2.7]): the list is exactly the non-aggregate items (E112), then dropped (§5.2).
    fn group_by(&mut self, r: &Return) {
        let keys: Vec<&Expr> = r
            .items
            .iter()
            .map(|it| &it.expr)
            .filter(|e| first_aggregate(e).is_none())
            .collect();
        let mut matched = vec![false; keys.len()];
        let mut ok = r.group.len() == keys.len();
        for g in &r.group {
            match keys
                .iter()
                .enumerate()
                .position(|(i, k)| !matched[i] && **k == *g)
            {
                Some(i) => matched[i] = true,
                None => ok = false,
            }
        }
        if !ok {
            let list: Vec<String> = keys.iter().map(|k| printer::expr_text(k)).collect();
            let span = r.group[0].span.to(r.group[r.group.len() - 1].span);
            self.err(Diag::new(
                Code::E112,
                span,
                format!(
                    "GROUP BY must list exactly the non-aggregate items: {}",
                    crate::lq::diag::list(&list)
                ),
            ));
        }
    }

    fn sorts(&mut self, v: &[Sort], agg: AggPos) -> Vec<CSort> {
        v.iter()
            .map(|s| CSort {
                expr: self.expr_at(&s.expr, None, agg).0,
                desc: s.desc,
            })
            .collect()
    }

    fn limit(&mut self, e: &Expr) -> CExpr {
        let (c, t) = self.expr_at(e, Some(&Ty::Int), AggPos::Other);
        if !matches!(t, Ty::Int | Ty::Any) {
            self.err(Diag::new(
                Code::E103,
                e.span,
                format!("LIMIT {}: the types do not match", q(&t.name())),
            ));
        }
        c
    }

    /// An expression that must be a boolean.
    fn bool_expr(&mut self, e: &Expr, agg: AggPos) -> CExpr {
        let (c, t) = self.expr_at(e, Some(&Ty::Bool), agg);
        if !matches!(t, Ty::Bool | Ty::Any | Ty::Null) {
            self.err(Diag::new(
                Code::E103,
                e.span,
                format!("WHERE {}: the types do not match", q(&t.name())),
            ));
        }
        c
    }

    /// Binds an expression in an aggregate position.
    fn expr_at(&mut self, e: &Expr, want: Option<&Ty>, agg: AggPos) -> (CExpr, Ty) {
        let saved = self.agg;
        self.agg = agg;
        let r = self.expr(e, want);
        self.agg = saved;
        r
    }

    /// A subquery ([LQ/canonical-ast §5.7] V8, §5.8 N1): it sees every visible binding; its own are local.
    fn sub(&mut self, s: &Sub) -> CSub {
        let outer = self.scope.clone();
        let outer_steps = self.steps.clone();
        let saved_agg = self.agg;
        let saved_saw = self.saw_agg;
        let saved_quant = self.quant_part.clone();
        // Its patterns echo like top-level ones ([LQ/envelope §4.1]), also inside a quantified group's `WHERE`.
        let saved_mute = std::mem::replace(&mut self.mute_echo, false);
        // An existence test narrows nothing outside it: the kinds it infers for outer variables are its own.
        let kinds = self.kinds_snapshot();
        let r = match s {
            Sub::Patterns { patterns, where_ } => {
                let (p, w) = self.match_patterns(patterns, where_.as_ref(), false);
                CSub::Patterns(p, w)
            }
            Sub::Clauses { clauses, ret } => {
                if let ([Clause::Match(m)], None) = (clauses.as_slice(), ret)
                    && !m.optional
                {
                    let (p, w) = self.match_patterns(&m.patterns, m.where_.as_ref(), false);
                    self.scope = outer;
                    self.steps = outer_steps;
                    self.agg = saved_agg;
                    self.saw_agg = saved_saw;
                    self.quant_part = saved_quant;
                    self.mute_echo = saved_mute;
                    self.restore_kinds(kinds);
                    return CSub::Patterns(p, w);
                }
                let mut cs = Vec::with_capacity(clauses.len());
                for c in clauses {
                    if let Some(c) = self.clause(c) {
                        cs.push(c);
                    }
                }
                let r = ret.as_ref().map(|r| self.ret(r).0);
                CSub::Clauses(cs, r)
            }
        };
        self.scope = outer;
        self.steps = outer_steps;
        self.agg = saved_agg;
        self.saw_agg = saved_saw;
        self.quant_part = saved_quant;
        self.mute_echo = saved_mute;
        self.restore_kinds(kinds);
        r
    }

    /// The types of the bindings made so far.
    fn kinds_snapshot(&self) -> Vec<Ty> {
        self.b.iter().map(|b| b.ty.clone()).collect()
    }

    /// Restores the types of the bindings a snapshot holds.
    fn restore_kinds(&mut self, kinds: Vec<Ty>) {
        for (b, t) in self.b.iter_mut().zip(kinds) {
            b.ty = t;
        }
    }
}

/// Whether a ref name is a branch, whose tip is a writable view ([F12 §2.2]: `main`, `lane/*`, `plan/*`).
fn ref_is_branch(name: &str) -> bool {
    name == "main" || name.starts_with("lane/") || name.starts_with("plan/")
}

/// The display name of an unaliased column: the variable, else the expression as printed.
fn column_name(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Ident(n) => n.clone(),
        _ => printer::expr_text(e),
    }
}

/// The aggregate names (Table 5.3).
fn is_aggregate_name(name: &str) -> bool {
    crate::lq::catalog::function(name).is_some_and(|f| f.agg)
}

/// The first aggregate call inside an expression, as printed (for N08 and the `GROUP BY` check).
fn first_aggregate(e: &Expr) -> Option<String> {
    let mut stack = vec![e];
    while let Some(e) = stack.pop() {
        match &e.kind {
            ExprKind::CountStar => return Some("count(*)".to_string()),
            ExprKind::Fn { name, args, .. } => {
                if is_aggregate_name(&name.text) {
                    return Some(printer::expr_text(e));
                }
                for a in args {
                    if let ArgVal::Expr(x) = &a.value {
                        stack.push(x);
                    }
                }
            }
            ExprKind::Or(l, r)
            | ExprKind::And(l, r)
            | ExprKind::Cmp(_, l, r)
            | ExprKind::In(l, r)
            | ExprKind::StrPred(_, l, r)
            | ExprKind::Arith(_, l, r) => {
                stack.push(r);
                stack.push(l);
            }
            ExprKind::Not(x)
            | ExprKind::IsNull(_, x)
            | ExprKind::LabelTest(x, _)
            | ExprKind::Neg(x)
            | ExprKind::Prop(x, _) => stack.push(x),
            ExprKind::List(v) => stack.extend(v.iter().rev()),
            ExprKind::Map(v) => stack.extend(v.iter().rev().map(|kv| &kv.value)),
            ExprKind::Case {
                subject,
                whens,
                else_,
            } => {
                if let Some(x) = else_ {
                    stack.push(x);
                }
                for w in whens.iter().rev() {
                    stack.push(&w.then);
                    stack.push(&w.cond);
                }
                if let Some(s) = subject {
                    stack.push(s);
                }
            }
            ExprKind::ListPred { list, pred, .. } => {
                stack.push(pred);
                stack.push(list);
            }
            _ => {}
        }
    }
    None
}

/// Runs a bind of `src` (see [`crate::lq::parser::FRONT_END_STACK`]): in place when [`bind_nesting_bound`] is at most
/// [`INLINE_NESTING`], else on the front end's own stack.
fn run_bind<T: Send>(ctx: &BindCtx<'_>, src: &str, f: impl FnOnce() -> T + Send) -> T {
    if bind_nesting_bound(ctx, src) <= INLINE_NESTING {
        f()
    } else {
        on_front_end_stack(f)
    }
}

/// An upper bound of the nesting a bind of `src` stacks up: the text's own ([`nesting_bound`]) plus that of every
/// project named query it may reach. A bind parses and binds a callee's definition nested inside the call site
/// ([`Binder::project_query`]), and a definition appears at most once on a call chain (a second entry is a cycle), so
/// the sum over the reachable definitions bounds every chain. The parser takes more stack per level than the binder, so
/// the peak of a chain stays within what a parse of that many levels takes. A definition is reachable when its name
/// occurs in `src` or in a reachable definition's text (a superset of the calls; a back-quoted name that holds a
/// back-quote is counted always).
pub(crate) fn bind_nesting_bound(ctx: &BindCtx<'_>, src: &str) -> u32 {
    let queries = &ctx.schema.queries;
    let mut total = nesting_bound(src);
    if queries.is_empty() {
        return total;
    }
    let mut seen = vec![false; queries.len()];
    let mut texts = vec![src];
    while let Some(t) = texts.pop() {
        for (i, q) in queries.iter().enumerate() {
            if !seen[i] && (q.name.contains('`') || t.contains(q.name.as_str())) {
                seen[i] = true;
                total = total.saturating_add(nesting_bound(&q.text));
                if total > INLINE_NESTING {
                    return total;
                }
                texts.push(&q.text);
            }
        }
    }
    total
}

/// Binds `read_input` (R1: `moirai q`, MCP `query` with `q`). `src` is the text the read was parsed from; for a tree
/// from elsewhere (a JSON IR document), its source text, which also sizes the stack the bind runs on.
pub fn bind_read(ctx: &BindCtx<'_>, src: &str, r: &Read) -> Result<Bound<CQuery>, Vec<Diag>> {
    run_bind(ctx, src, || {
        let named = NamedMemo::default();
        let mut b = Binder::new(ctx, src, &named);
        if ctx.caller.named_only {
            b.err(
                Diag::unlocated(
                    Code::E406,
                    format!(
                        "role {} may run only named queries (query.safelist.{} = named-only)",
                        q(&ctx.caller.role),
                        ctx.caller.role
                    ),
                )
                .help("moirai q --list lists them"),
            );
        }
        let (qy, cols) = b.query(&r.query);
        b.finish(qy, cols)
    })
}

/// Binds a named query run by name (R2: `moirai q NAME k=v`, MCP `query` with `name` and `params`): the `QUERY` of
/// `CALL <name>(k: v, ...)` with no `YIELD`, the values being `ctx.params` coerced by the signature.
pub fn bind_named(ctx: &BindCtx<'_>, name: &str) -> Result<Bound<CQuery>, Vec<Diag>> {
    run_bind(ctx, "", || {
        let named = NamedMemo::default();
        let mut b = Binder::new(ctx, "", &named);
        let (call, cols) = b.named_run(name);
        let qy = CQuery {
            first: CPart {
                use_: None,
                body: CBody::Call(call),
            },
            rest: Vec::new(),
        };
        b.finish(qy, cols)
    })
}

/// Binds `write_input` (R3: `moirai tx`, MCP `write` with `tx`). The CLI flags and MCP fields (`--if-tip`, `--dry-run`,
/// `--idempotency-key`, `--lease`, `--branch`) join the block's options before the call ([LQ/canonical-ast §5.9]).
pub fn bind_write(ctx: &BindCtx<'_>, src: &str, t: &Tx) -> Result<Bound<CTx>, Vec<Diag>> {
    run_bind(ctx, src, || {
        let named = NamedMemo::default();
        let mut b = Binder::new(ctx, src, &named);
        let c = b.tx(t, true);
        b.finish(c, Vec::new())
    })
}

/// Binds a named mutation run by name (R4: MCP `write` with `name` and `params`): the `TX` of
/// `TX { CALL tx.<name>(k: v, ...) }`, joined with the tool's fields in `options` (whose `stmts` are ignored).
pub fn bind_mutation(ctx: &BindCtx<'_>, name: &str, options: &Tx) -> Result<Bound<CTx>, Vec<Diag>> {
    run_bind(ctx, "", || {
        let named = NamedMemo::default();
        let mut b = Binder::new(ctx, "", &named);
        let c = b.named_mutation(name, options);
        b.finish(c, Vec::new())
    })
}

/// Binds a definition (R6: the F3 hash of a `QUERIES` item; a standard-library source). `src` is the definition's text;
/// the result's `portable` holds its portable form ([LQ/canonical-ast §8.1]).
pub fn bind_define(ctx: &BindCtx<'_>, src: &str, d: &Define) -> Result<Bound<CDefine>, Vec<Diag>> {
    run_bind(ctx, src, || {
        let named = NamedMemo::default();
        let mut b = Binder::new(ctx, src, &named);
        let (c, cols) = b.define(d, Span::new(0, src.len() as u32));
        b.finish(c, cols)
    })
}

impl Binder<'_> {
    fn surface(&self) -> Surface {
        self.ctx.caller.surface
    }
}
