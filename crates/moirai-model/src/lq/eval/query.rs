//! Query parts, clauses and projections ([50 §3.1], §3.4, §3.5, §3.9; [LQ/canonical-ast §5.7]): clauses run left to
//! right over bags of bindings; `RETURN` and `WITH` project, aggregate (one group per equal key, absent keys forming one
//! group) and filter; every result is totally ordered — explicit keys first, then the binding identity, the group keys
//! of an aggregated result, or the columns of a `DISTINCT` one; set operations combine parts each read in its own view.

use super::expr::{Env, Row};
use super::val::{self, V};
use super::view::{Ev, View, World};
use crate::err::{Refusal, Res};
use crate::lq::cast::{
    BindingId, CArg, CBody, CClause, CExpr, CPart, CPath, CQuery, CReturn, CSCall, CSort, CStep,
};
use crate::lq::diag::Code;
use std::cmp::Ordering;

/// The values a definition's parameters take: by position, with the revision node of each revision-typed argument,
/// which a definition's `USE $p` opens as a view ([50 §3.9] item 5).
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// `PARAM(i)` values.
    pub vals: Vec<V>,
    /// The revision node an argument was written as, if any.
    pub revs: Vec<Option<CExpr>>,
}

/// A projected result: its rows, each with the total-order identity it sorts by after its explicit keys.
#[derive(Clone, Debug, Default)]
pub struct Proj {
    /// The rows.
    pub rows: Vec<Vec<V>>,
    /// The number of rows before the set-counting ablation's deduplication, when it removed some (D11).
    pub dedup_from: Option<usize>,
}

/// The rows of one evaluated query: its rows, the view of each part, and D11's count.
#[derive(Clone, Debug, Default)]
pub struct Table {
    /// The rows.
    pub rows: Vec<Vec<V>>,
    /// The views the parts read, in part order.
    pub views: Vec<View>,
    /// D11's count before deduplication.
    pub dedup_from: Option<usize>,
}

/// Whether an expression holds an aggregate outside any subquery.
pub fn has_agg(e: &CExpr) -> bool {
    match e {
        CExpr::CountStar => true,
        CExpr::Func(n, _, _)
            if matches!(
                n.as_str(),
                "count" | "sum" | "min" | "max" | "avg" | "collect"
            ) =>
        {
            true
        }
        e => {
            let mut found = false;
            e.each_operand(&mut |x| found |= has_agg(x));
            found
        }
    }
}

/// The named bindings a path declares in the binder's order ([LQ/canonical-ast §5.7] V2), its group-local variables
/// left out.
pub fn path_vars(p: &CPath, out: &mut Vec<BindingId>) {
    let mut push = |v: Option<BindingId>| {
        if let Some(b) = v
            && !out.contains(&b)
        {
            out.push(b);
        }
    };
    push(p.start.var);
    for s in &p.steps {
        match s {
            CStep::Edge(e, n) => {
                push(e.var);
                push(n.var);
            }
            CStep::Group(_, n) => push(n.var),
        }
    }
}

/// The number of pattern elements of a clause (for an `OPTIONAL MATCH` that matched nothing).
fn elements(paths: &[CPath]) -> usize {
    paths
        .iter()
        .map(|p| {
            1 + p
                .steps
                .iter()
                .map(|s| match s {
                    CStep::Edge(e, _) if e.quant.is_none() => 2,
                    _ => 1,
                })
                .sum::<usize>()
        })
        .sum()
}

/// Whether a clause's patterns hold an anonymous element (D11's loud form).
fn anonymous(paths: &[CPath]) -> bool {
    paths.iter().any(|p| {
        p.start.var.is_none()
            || p.steps.iter().any(|s| match s {
                CStep::Edge(e, n) => (e.quant.is_none() && e.var.is_none()) || n.var.is_none(),
                CStep::Group(_, n) => n.var.is_none(),
            })
    })
}

/// One row of a `RETURN`: its sort keys, its total-order identity, its values, and the input rows it was projected
/// from (one row, or a group's members), for W03's check of the emitted rows.
struct Out {
    keys: Vec<V>,
    ident: Vec<V>,
    vals: Vec<V>,
    src: Vec<usize>,
}

/// One group of rows ([50 §3.3]): its key, its members in order of appearance with their input positions, and which
/// members a division by zero hit while the key was evaluated (N10).
struct Group {
    key: Vec<V>,
    members: Vec<Row>,
    idx: Vec<usize>,
    hits: Vec<bool>,
}

/// Compares two rows by their sort keys, then their identities ([50 §3.5]).
fn by_keys(a: &(Vec<V>, Vec<V>), b: &(Vec<V>, Vec<V>), desc: &[bool]) -> Ordering {
    for (i, d) in desc.iter().enumerate() {
        let o = val::cmp_key(&a.0[i], &b.0[i], *d);
        if o != Ordering::Equal {
            return o;
        }
    }
    val::cmp_total(&V::List(a.1.clone()), &V::List(b.1.clone()))
}

impl Ev<'_> {
    /// Runs reading clauses left to right ([50 §3.1] item 2), keeping the scope of named bindings for `*`.
    pub fn run_clauses(&self, rows: Vec<Row>, clauses: &[CClause], params: &[V]) -> Res<Vec<Row>> {
        self.run_clauses_scoped(rows, clauses, params, &mut Vec::new())
    }

    /// [`Ev::run_clauses`] with the visible bindings, in the binder's order ([LQ/canonical-ast §5.7]).
    // spec: [50 §3.1]
    pub fn run_clauses_scoped(
        &self,
        mut rows: Vec<Row>,
        clauses: &[CClause],
        params: &[V],
        scope: &mut Vec<BindingId>,
    ) -> Res<Vec<Row>> {
        for c in clauses {
            rows = match c {
                CClause::Match {
                    optional,
                    patterns,
                    where_,
                } => {
                    let mut fresh = Vec::new();
                    for p in patterns {
                        path_vars(p, &mut fresh);
                    }
                    fresh.retain(|b| !scope.contains(b));
                    let mut out = Vec::new();
                    for r in &rows {
                        let before = out.len();
                        self.match_paths(r, patterns, where_.as_ref(), params, &mut out, false)?;
                        if *optional && out.len() == before {
                            // Nothing matched: the new variables are bound to the absent value ([50 §3.1] item 2).
                            let mut n = r.clone();
                            for b in &fresh {
                                n.set(*b, V::Absent);
                            }
                            n.ident
                                .extend(std::iter::repeat_n(V::Absent, elements(patterns)));
                            out.push(n);
                        }
                    }
                    for p in patterns {
                        path_vars(p, scope);
                    }
                    if self.w.ab.set_counting {
                        // D11: each distinct assignment of the named bindings once.
                        out.sort_by(|a, b| {
                            val::cmp_total(&V::List(a.ident.clone()), &V::List(b.ident.clone()))
                        });
                        let mut kept: Vec<Row> = Vec::new();
                        for r in out {
                            let dup = kept
                                .iter()
                                .any(|k| scope.iter().all(|b| val::same(k.get(*b), r.get(*b))));
                            if !dup {
                                kept.push(r);
                            }
                        }
                        kept
                    } else {
                        out
                    }
                }
                CClause::Unwind { expr, var } => {
                    let mut out = Vec::new();
                    for r in &rows {
                        let l = self.eval(expr, &Env::of(r, params))?;
                        for (i, x) in l.elems().iter().enumerate() {
                            let mut n = r.clone();
                            n.set(*var, x.clone());
                            n.ident.push(V::Int(i as i64));
                            out.push(n);
                        }
                    }
                    if !scope.contains(var) {
                        scope.push(*var);
                    }
                    out
                }
                CClause::Call {
                    proc,
                    args,
                    items,
                    where_,
                } => {
                    let mut out = Vec::new();
                    for r in &rows {
                        let rel = self.call(proc, args, &Env::of(r, params))?;
                        for (i, row) in rel.rows.into_iter().enumerate() {
                            let mut n = r.clone();
                            for y in items {
                                let v = rel
                                    .cols
                                    .iter()
                                    .position(|c| *c == y.field)
                                    .map_or(V::Absent, |k| row[k].clone());
                                n.set(y.var, v);
                            }
                            n.ident.push(V::Int(i as i64));
                            if self.filter(where_.as_ref(), &n, params)? {
                                out.push(n);
                            }
                        }
                    }
                    for y in items {
                        if !scope.contains(&y.var) {
                            scope.push(y.var);
                        }
                    }
                    out
                }
                CClause::With {
                    distinct,
                    star,
                    items,
                    where_,
                    order,
                    limit,
                } => self.with(
                    rows,
                    (*distinct, *star),
                    items,
                    where_.as_ref(),
                    order,
                    limit.as_ref(),
                    params,
                    scope,
                )?,
            };
        }
        Ok(rows)
    }

    /// `WITH` ([LQ/canonical-ast §5.7] V4): items evaluated in the scope before it; an aggregating or `DISTINCT` `WITH`
    /// groups — by its non-aggregate items and, for `WITH *`, by every binding in scope, as `RETURN *` does — and every
    /// other keeps its rows and their identities; `WHERE`, `ORDER BY` and `LIMIT` follow. As in `RETURN`, an
    /// aggregating `WITH` over no rows and no grouping key yields one row, and its `ORDER BY` keys read the group, so
    /// an aggregate there aggregates the group's rows ([50 §3.4] rule 6), in each of which the bindings the `WITH`
    /// creates hold the group's values (an aggregate over an item reads it as one over a `RETURN` item does).
    #[allow(clippy::too_many_arguments)]
    // spec: [LQ/canonical-ast §5.7] V4
    fn with(
        &self,
        rows: Vec<Row>,
        (distinct, star): (bool, bool),
        items: &[(CExpr, BindingId)],
        where_: Option<&CExpr>,
        order: &[CSort],
        limit: Option<&CExpr>,
        params: &[V],
        scope: &mut Vec<BindingId>,
    ) -> Res<Vec<Row>> {
        let order_agg = order.iter().any(|s| has_agg(&s.expr));
        let aggregated = order_agg || items.iter().any(|(e, _)| has_agg(e));
        let star_vars: Vec<BindingId> = if star { scope.clone() } else { Vec::new() };
        // Each row with the sort keys an aggregating `WITH` computes with its group.
        let mut out: Vec<(Row, Option<Vec<V>>)> = Vec::new();
        if aggregated || distinct {
            let mut exprs: Vec<CExpr> = star_vars.iter().map(|b| CExpr::Var(*b)).collect();
            exprs.extend(items.iter().map(|(e, _)| e.clone()));
            let empty = Row::default();
            for mut g in self.group(&rows, exprs.iter(), params)? {
                let hits = std::mem::take(&mut g.hits);
                let ident = std::mem::take(&mut g.key);
                let (n, keys) = self.group_hits(&hits, || {
                    let first = g.members.first().unwrap_or(&empty);
                    let mut n = Row::default();
                    let mut svals = Vec::with_capacity(star_vars.len());
                    for b in &star_vars {
                        let v = first.get(*b).clone();
                        svals.push(v.clone());
                        n.set(*b, v);
                    }
                    let env = Env {
                        row: first,
                        params,
                        items: None,
                        group: Some(&g.members),
                    };
                    let mut vals = Vec::with_capacity(items.len());
                    for (e, b) in items {
                        let v = self.eval(e, &env)?;
                        vals.push(v.clone());
                        n.set(*b, v);
                    }
                    let keys = if aggregated && !order.is_empty() {
                        if order_agg {
                            // The keys see the bindings this `WITH` creates: an aggregate over an item reads, in
                            // every row of the group, the item's value in the group's row, as an aggregate over a
                            // `RETURN` item does ([50 §3.4] rule 6).
                            for m in &mut g.members {
                                for (_, b) in items {
                                    m.set(*b, n.get(*b).clone());
                                }
                            }
                        }
                        let kenv = Env {
                            row: &n,
                            params,
                            items: None,
                            group: Some(&g.members),
                        };
                        let mut k = Vec::with_capacity(order.len());
                        for s in order {
                            k.push(self.eval(&s.expr, &kenv)?);
                        }
                        Some(k)
                    } else {
                        None
                    };
                    n.ident = if aggregated {
                        ident
                    } else {
                        svals.iter().cloned().chain(vals).collect()
                    };
                    Ok((n, keys))
                })?;
                out.push((n, keys));
            }
        } else {
            for r in rows {
                let mut n = r.clone();
                self.per_row(|| {
                    for (e, b) in items {
                        let v = self.eval(e, &Env::of(&r, params))?;
                        n.set(*b, v);
                    }
                    Ok(())
                })?;
                out.push((n, None));
            }
        }
        let mut next_scope: Vec<BindingId> = star_vars;
        for (_, b) in items {
            next_scope.push(*b);
        }
        *scope = next_scope;
        let mut kept = Vec::with_capacity(out.len());
        for r in out {
            if self.filter(where_, &r.0, params)? {
                kept.push(r);
            }
        }
        let mut kept: Vec<Row> = if order.is_empty() {
            kept.into_iter().map(|(r, _)| r).collect()
        } else {
            let mut keyed = Vec::with_capacity(kept.len());
            for (r, keys) in kept {
                let k = match keys {
                    Some(k) => k,
                    None => {
                        let mut k = Vec::with_capacity(order.len());
                        for s in order {
                            k.push(self.eval(&s.expr, &Env::of(&r, params))?);
                        }
                        k
                    }
                };
                keyed.push(((k, r.ident.clone()), r));
            }
            let desc: Vec<bool> = order.iter().map(|s| s.desc).collect();
            keyed.sort_by(|a, b| by_keys(&a.0, &b.0, &desc));
            keyed.into_iter().map(|(_, r)| r).collect()
        };
        if let Some(l) = limit {
            let n = self.limit_of(l, params)?;
            kept.truncate(n);
        }
        Ok(kept)
    }

    /// Evaluates one row's projection, counting the row for N10 when a division by zero gave absent in it
    /// ([LQ/errors §5.6]; outermost evaluations only).
    fn per_row<T>(&self, f: impl FnOnce() -> Res<T>) -> Res<T> {
        if !self.counting() {
            return f();
        }
        let outer = std::mem::take(&mut *self.flags.borrow_mut());
        let r = f();
        let fl = std::mem::replace(&mut *self.flags.borrow_mut(), outer);
        if r.is_ok() && fl.div0 {
            self.w.counts.borrow_mut().n10 += 1;
        }
        r
    }

    /// Evaluates one group's projection, counting for N10 each input row of the group in which a division by zero
    /// gave absent, while its key or an aggregate's argument was evaluated ([LQ/errors §5.6]; outermost evaluations
    /// only).
    fn group_hits<T>(&self, hits: &[bool], f: impl FnOnce() -> Res<T>) -> Res<T> {
        if !self.counting() {
            return f();
        }
        let saved = self.hits.replace(Some(hits.to_vec()));
        let r = f();
        let hits = self.hits.replace(saved);
        if r.is_ok() {
            let n = hits.iter().flatten().filter(|h| **h).count() as u64;
            self.w.counts.borrow_mut().n10 += n;
        }
        r
    }

    /// The groups of rows by the values of the non-aggregate expressions, in order of first appearance; all absent
    /// values equal, and numbers by numeric value ([50 §3.3]). With no rows and no grouping key there is one group
    /// with no members, whose aggregates give `count` 0, `sum` 0 and absent for the others.
    // spec: [50 §3.3] grouping
    // spec: [50 §3.4] aggregates
    fn group<'e>(
        &self,
        rows: &[Row],
        exprs: impl Iterator<Item = &'e CExpr>,
        params: &[V],
    ) -> Res<Vec<Group>> {
        let keys: Vec<&CExpr> = exprs.filter(|e| !has_agg(e)).collect();
        let mut groups: Vec<Group> = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            let before = std::mem::take(&mut self.flags.borrow_mut().div0);
            let mut k = Vec::with_capacity(keys.len());
            for e in &keys {
                k.push(self.eval(e, &Env::of(r, params))?);
            }
            let hit = std::mem::replace(&mut self.flags.borrow_mut().div0, before);
            match groups
                .iter_mut()
                .find(|g| g.key.iter().zip(&k).all(|(a, b)| val::same(a, b)))
            {
                Some(g) => {
                    g.members.push(r.clone());
                    g.idx.push(i);
                    g.hits.push(hit);
                }
                None => groups.push(Group {
                    key: k,
                    members: vec![r.clone()],
                    idx: vec![i],
                    hits: vec![hit],
                }),
            }
        }
        if groups.is_empty() && keys.is_empty() {
            groups.push(Group {
                key: Vec::new(),
                members: Vec::new(),
                idx: Vec::new(),
                hits: Vec::new(),
            });
        }
        Ok(groups)
    }

    /// `LIMIT`'s count.
    fn limit_of(&self, l: &CExpr, params: &[V]) -> Res<usize> {
        match self.eval(l, &Env::of(&Row::default(), params))? {
            V::Int(n) if n >= 0 => Ok(n as usize),
            V::Absent => Ok(usize::MAX),
            _ => Err(Refusal::lq("E103", "LIMIT takes a non-negative integer")),
        }
    }

    /// `RETURN` ([50 §3.1] item 3, §3.5): the projection of the rows, grouped when an item aggregates, deduplicated with
    /// `DISTINCT` (or under the set-counting ablation), totally ordered and limited; W03 for the `done` reads of
    /// `w03` on the rows it emits ([50 §3.8]).
    // spec: [50 §3.5]
    pub fn project(
        &self,
        rows: Vec<Row>,
        ret: &CReturn,
        params: &[V],
        scope: Option<&[BindingId]>,
        w03: &[BindingId],
    ) -> Res<Proj> {
        let mut items: Vec<CExpr> = Vec::new();
        if ret.star {
            for b in scope.unwrap_or(&[]) {
                items.push(CExpr::Var(*b));
            }
        }
        items.extend(ret.items.iter().map(|(e, _)| e.clone()));
        let aggregated = items.iter().any(has_agg) || ret.order.iter().any(|s| has_agg(&s.expr));
        let mut out: Vec<Out> = Vec::new();
        let desc: Vec<bool> = ret.order.iter().map(|s| s.desc).collect();
        let by = |a: &Out, b: &Out| {
            for (i, d) in desc.iter().enumerate() {
                let o = val::cmp_key(&a.keys[i], &b.keys[i], *d);
                if o != Ordering::Equal {
                    return o;
                }
            }
            val::cmp_total(&V::List(a.ident.clone()), &V::List(b.ident.clone()))
        };
        let eval_row = |vals: &[V], env: Env<'_>| -> Res<Vec<V>> {
            let mut k = Vec::with_capacity(ret.order.len());
            for s in &ret.order {
                k.push(self.eval(
                    &s.expr,
                    &Env {
                        items: Some(vals),
                        ..env
                    },
                )?);
            }
            Ok(k)
        };
        if aggregated {
            let empty = Row::default();
            for g in self.group(&rows, items.iter(), params)? {
                let first = g.members.first().unwrap_or(&empty);
                let env = Env {
                    row: first,
                    params,
                    items: None,
                    group: Some(&g.members),
                };
                let (vals, k) = self.group_hits(&g.hits, || {
                    let mut vals = Vec::with_capacity(items.len());
                    for e in &items {
                        vals.push(self.eval(e, &env)?);
                    }
                    let k = eval_row(&vals, env)?;
                    Ok((vals, k))
                })?;
                let ident = if ret.distinct {
                    vals.clone()
                } else {
                    g.key.clone()
                };
                out.push(Out {
                    keys: k,
                    ident,
                    vals,
                    src: g.idx,
                });
            }
        } else {
            for (i, r) in rows.iter().enumerate() {
                let env = Env::of(r, params);
                let (vals, k) = self.per_row(|| {
                    let mut vals = Vec::with_capacity(items.len());
                    for e in &items {
                        vals.push(self.eval(e, &env)?);
                    }
                    let k = eval_row(&vals, env)?;
                    Ok((vals, k))
                })?;
                let ident = if ret.distinct {
                    vals.clone()
                } else {
                    r.ident.clone()
                };
                out.push(Out {
                    keys: k,
                    ident,
                    vals,
                    src: vec![i],
                });
            }
        }
        let mut dedup_from = None;
        let set = self.w.ab.set_counting && !aggregated;
        if ret.distinct || set {
            let before = out.len();
            let mut kept: Vec<Out> = Vec::new();
            out.sort_by(|a, b| by(a, b));
            for x in out {
                if !kept
                    .iter()
                    .any(|k| k.ident.iter().zip(&x.ident).all(|(a, b)| val::same(a, b)))
                {
                    kept.push(x);
                }
            }
            if set && !ret.distinct && kept.len() < before {
                dedup_from = Some(before);
            }
            out = kept;
        }
        out.sort_by(|a, b| by(a, b));
        if let Some(l) = &ret.limit {
            out.truncate(self.limit_of(l, params)?);
        }
        if !w03.is_empty() {
            let emitted: Vec<&Row> = out
                .iter()
                .flat_map(|o| o.src.iter().map(|i| &rows[*i]))
                .collect();
            self.w03(&emitted, w03);
        }
        Ok(Proj {
            rows: out.into_iter().map(|o| o.vals).collect(),
            dedup_from,
        })
    }

    /// W03 ([50 §3.8]; [LQ/errors §5.6]): the query reads `v.done` and a row the result emits binds `v` to a cancelled
    /// node (an aggregated row: one of its group's rows).
    // spec: [50 §3.8] W03
    fn w03(&self, rows: &[&Row], vars: &[BindingId]) {
        for &b in vars {
            let cancelled = rows.iter().any(|r| {
                r.get(b)
                    .node()
                    .and_then(|n| self.node(n))
                    .is_some_and(|x| x.status == "cancelled")
            });
            if cancelled {
                let v = self.w.name(b);
                self.w.note(
                    Code::W03,
                    format!(
                        "{v}.done includes cancelled; write {v}.status = 'done' for completed only"
                    ),
                    Vec::new(),
                    None,
                );
            }
        }
    }

    /// A part's rows ([50 §3.9]): its clauses then its `RETURN`, or its standalone `CALL`.
    // spec: [50 §3.9]
    pub fn run_body(&self, body: &CBody, frame: &Frame) -> Res<Proj> {
        match body {
            CBody::Clauses(clauses, ret) => {
                // W03 reads `v.done` wherever the part writes it.
                let mut vars = Vec::new();
                for c in clauses {
                    clause_exprs(c, &mut |e| done_vars(e, &mut vars));
                }
                for (e, _) in &ret.items {
                    done_vars(e, &mut vars);
                }
                for s in &ret.order {
                    done_vars(&s.expr, &mut vars);
                }
                // D11's loud form ([50 §3.4]): `count(*)` over a pattern with anonymous elements is refused.
                if self.w.ab.set_counting {
                    let anon = clauses.iter().any(
                        |c| matches!(c, CClause::Match { patterns, .. } if anonymous(patterns)),
                    );
                    let star = ret.items.iter().any(|(e, _)| counts_star(e))
                        || clauses.iter().any(|c| {
                            matches!(c, CClause::With { items, .. } if items.iter().any(|(e, _)| counts_star(e)))
                        });
                    if anon && star {
                        return Err(Refusal::lq(
                            "E112",
                            "count(*) counts the bindings of anonymous pattern elements, which set counting collapses; count entities with count(DISTINCT <variable>)",
                        ));
                    }
                }
                let mut scope = Vec::new();
                let rows = self.run_clauses_scoped(
                    vec![Row::default()],
                    clauses,
                    &frame.vals,
                    &mut scope,
                )?;
                self.project(rows, ret, &frame.vals, Some(&scope), &vars)
            }
            CBody::Call(sc) => self.scall(sc, frame),
        }
    }

    /// A standalone `CALL` ([50 §3.1] item 3): the callee's rows as declared, or its yielded columns filtered, ordered
    /// (keys, then the callee's order) and limited.
    // spec: [50 §3.1] item 3
    fn scall(&self, sc: &CSCall, frame: &Frame) -> Res<Proj> {
        let params = &frame.vals;
        let rel = self.call(&sc.proc, &sc.args, &Env::of(&Row::default(), params))?;
        if sc.ymode != 2 {
            let mut rows = rel.rows;
            if let Some(l) = &sc.limit {
                rows.truncate(self.limit_of(l, params)?);
            }
            return Ok(Proj {
                rows,
                dedup_from: None,
            });
        }
        let mut keyed = Vec::new();
        for (i, row) in rel.rows.into_iter().enumerate() {
            let mut r = Row::default();
            for y in &sc.items {
                let v = rel
                    .cols
                    .iter()
                    .position(|c| *c == y.field)
                    .map_or(V::Absent, |k| row[k].clone());
                r.set(y.var, v);
            }
            r.ident.push(V::Int(i as i64));
            if !self.filter(sc.where_.as_ref(), &r, params)? {
                continue;
            }
            let mut k = Vec::with_capacity(sc.order.len());
            for s in &sc.order {
                k.push(self.eval(&s.expr, &Env::of(&r, params))?);
            }
            let vals: Vec<V> = sc.items.iter().map(|y| r.get(y.var).clone()).collect();
            keyed.push(((k, r.ident.clone()), vals));
        }
        let desc: Vec<bool> = sc.order.iter().map(|s| s.desc).collect();
        keyed.sort_by(|a, b| by_keys(&a.0, &b.0, &desc));
        let mut rows: Vec<Vec<V>> = keyed.into_iter().map(|(_, v)| v).collect();
        if let Some(l) = &sc.limit {
            rows.truncate(self.limit_of(l, params)?);
        }
        Ok(Proj {
            rows,
            dedup_from: None,
        })
    }

    /// The arguments of a call evaluated by name, revision arguments kept as written ([LQ/std §2.8] item 1).
    pub fn args(&self, args: &[CArg], env: &Env<'_>) -> Res<Vec<(String, V, Option<CExpr>)>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            // A definition's revision parameter passes the revision node it was called with.
            let rev_param = match &a.value {
                CExpr::Param(i) => self.revs.borrow().get(*i as usize).cloned().flatten(),
                _ => None,
            };
            if let Some(r) = &rev_param {
                let v = match r {
                    CExpr::RRange(..) => V::Absent,
                    x => self.eval(x, env)?,
                };
                out.push((a.name.clone().unwrap_or_default(), v, Some(r.clone())));
                continue;
            }
            let rev = matches!(
                a.value,
                CExpr::RHead
                    | CExpr::RRef(_)
                    | CExpr::RCommit(_)
                    | CExpr::RSuf(..)
                    | CExpr::RRange(..)
                    | CExpr::RList(_)
            );
            let v = match &a.value {
                CExpr::RRange(..) => V::Absent,
                x => self.eval(x, env)?,
            };
            out.push((
                a.name.clone().unwrap_or_default(),
                v,
                rev.then(|| a.value.clone()),
            ));
        }
        Ok(out)
    }
}

/// Whether an expression holds `count(*)` outside any subquery.
fn counts_star(e: &CExpr) -> bool {
    let mut found = matches!(e, CExpr::CountStar);
    if !found {
        e.each_operand(&mut |x| found |= counts_star(x));
    }
    found
}

/// Visits every expression a clause holds outside its subqueries: its patterns' property maps and inline `WHERE`s,
/// its `WHERE`, its `UNWIND` list, its `CALL` arguments, and its `WITH` items, keys and `LIMIT`.
fn clause_exprs(c: &CClause, f: &mut dyn FnMut(&CExpr)) {
    fn path(p: &CPath, f: &mut dyn FnMut(&CExpr)) {
        let node = |n: &crate::lq::cast::CNode, f: &mut dyn FnMut(&CExpr)| {
            n.props.iter().for_each(|(_, e)| f(e));
            if let Some(w) = &n.where_ {
                f(w);
            }
        };
        node(&p.start, f);
        for s in &p.steps {
            match s {
                CStep::Edge(e, n) => {
                    e.props.iter().for_each(|(_, x)| f(x));
                    if let Some(w) = &e.where_ {
                        f(w);
                    }
                    node(n, f);
                }
                CStep::Group(g, n) => {
                    path(&g.path, f);
                    if let Some(w) = &g.where_ {
                        f(w);
                    }
                    node(n, f);
                }
            }
        }
    }
    match c {
        CClause::Match {
            patterns, where_, ..
        } => {
            patterns.iter().for_each(|p| path(p, f));
            if let Some(w) = where_ {
                f(w);
            }
        }
        CClause::Unwind { expr, .. } => f(expr),
        CClause::Call { args, where_, .. } => {
            args.iter().for_each(|a| f(&a.value));
            if let Some(w) = where_ {
                f(w);
            }
        }
        CClause::With {
            items,
            where_,
            order,
            limit,
            ..
        } => {
            items.iter().for_each(|(e, _)| f(e));
            if let Some(w) = where_ {
                f(w);
            }
            order.iter().for_each(|s| f(&s.expr));
            if let Some(l) = limit {
                f(l);
            }
        }
    }
}

/// The variables whose `done` an expression reads (W03).
fn done_vars(e: &CExpr, out: &mut Vec<BindingId>) {
    if let CExpr::Prop(a, name) = e
        && let (CExpr::Var(b), "done" | "unfinished") = (&**a, name.as_str())
        && !out.contains(b)
    {
        out.push(*b);
    }
    e.each_operand(&mut |x| done_vars(x, out));
}

impl<'a> World<'a> {
    /// A query evaluated part by part ([50 §3.9] item 2): each part in its own view (its `USE`, else `inherit`, else
    /// the caller's branch), the set operations over the parts' rows (`UNION`, `EXCEPT` and `INTERSECT` remove
    /// duplicates and order by the columns; `UNION ALL` concatenates), N03 when the parts read several views.
    // spec: [50 §3.9] composite
    // spec: [50 §3.4] set operations
    pub fn eval_query(&self, q: &CQuery, frame: &Frame, inherit: Option<&Ev<'_>>) -> Res<Table> {
        let mut t = self.eval_part(&q.first, frame, inherit)?;
        for (op, p) in &q.rest {
            let u = self.eval_part(p, frame, inherit)?;
            t.views.extend(u.views);
            let rows = std::mem::take(&mut t.rows);
            let contains = |set: &[Vec<V>], r: &Vec<V>| {
                set.iter()
                    .any(|s| s.iter().zip(r).all(|(a, b)| val::same(a, b)))
            };
            t.rows = match op {
                2 => rows.into_iter().chain(u.rows).collect(),
                1 => rows.into_iter().chain(u.rows).collect(),
                3 => rows.into_iter().filter(|r| !contains(&u.rows, r)).collect(),
                _ => rows.into_iter().filter(|r| contains(&u.rows, r)).collect(),
            };
            if *op != 2 {
                t.rows
                    .sort_by(|a, b| val::cmp_total(&V::List(a.clone()), &V::List(b.clone())));
                t.rows
                    .dedup_by(|a, b| a.iter().zip(b.iter()).all(|(x, y)| val::same(x, y)));
            }
        }
        if t.views.len() > 1 {
            let mut distinct: Vec<(String, Option<u64>)> = Vec::new();
            for v in &t.views {
                let k = (v.ref_name.clone(), v.commit);
                if !distinct.contains(&k) {
                    distinct.push(k);
                }
            }
            if distinct.len() > 1 {
                self.note(
                    Code::N03,
                    format!(
                        "{} parts read {} views; each row comes from the view of the part that produced it",
                        t.views.len(),
                        distinct.len()
                    ),
                    Vec::new(),
                    None,
                );
            }
        }
        Ok(t)
    }

    /// One part in its view: its own `USE`, else the calling part's view, whose evaluation context (with its derived
    /// adjacency and caches) it shares, else the default view.
    fn eval_part(&self, p: &CPart, frame: &Frame, inherit: Option<&Ev<'_>>) -> Res<Table> {
        let own = match &p.use_ {
            Some(CExpr::Param(i)) => match frame.revs.get(*i as usize).cloned().flatten() {
                Some(rev) => Some(self.open(Some(&rev))?),
                None => match frame.vals.get(*i as usize) {
                    Some(V::Rev(c)) => {
                        Some(self.open(Some(&CExpr::RCommit(self.store.dag.commits[c].id)))?)
                    }
                    _ => None,
                },
            },
            Some(rev) => Some(self.open(Some(rev))?),
            None => None,
        };
        let view = match (own, inherit) {
            (None, Some(ev)) => {
                let saved = ev.revs.replace(frame.revs.clone());
                let proj = ev.run_body(&p.body, frame);
                ev.revs.replace(saved);
                let proj = proj?;
                return Ok(Table {
                    rows: proj.rows,
                    dedup_from: proj.dedup_from,
                    views: vec![ev.v.clone()],
                });
            }
            (Some(v), _) => v,
            (None, None) => self.open(None)?,
        };
        let ev = Ev::new(self, &view);
        ev.revs.replace(frame.revs.clone());
        let proj = ev.run_body(&p.body, frame)?;
        drop(ev);
        Ok(Table {
            rows: proj.rows,
            dedup_from: proj.dedup_from,
            views: vec![view],
        })
    }
}
