//! `TX` blocks in LQ ([50 §3.10]; [API §9.1]; [LQ/envelope §9]): the statements of a bound block run in order on the
//! write kernel's candidate ([`crate::tx::Cand`]) — `MATCH … EXPECT` targets evaluated on the candidate as it stands
//! (later statements see earlier effects), each mutation compiled to the kernel's data-level operations, `CREATE`
//! with `UNLESS EXISTS` as create-or-bind, `ASSERT` on the candidate, `RESOLVE` by key or by a query's keys,
//! `DEFINE QUERY` and `DROP QUERY` as schema items — and the target-set digest of [LQ/envelope §9.4] over the bindings
//! of the `MATCH … EXPECT` statements, which `IF TARGETS` compares.

use super::expr::{Env, Row, display, truth};
use super::query::Frame;
use super::val::V;
use super::view::{Ev, View, ViewKind, World};
use crate::api::{Caller as ApiCaller, Ctx, Store};
use crate::err::{Kv, Refusal, Res};
use crate::lq::cast::{BindingId, CExpr, CMut, CResolveKey, CStmt, CTx};
use crate::lq::ctx::Value as P;
use crate::state::EdgeKey;
use crate::tx::{Cand, Position, Stmt, Take, Target};
use crate::value::{Nid, hex, lp};
use std::collections::BTreeSet;
use std::rc::Rc;

/// An LQ block of a `Tx` command ([API §9.1] `lq`): its text (with the command's `message` as its `MESSAGE` when the
/// text has none), its parameters, each statement's source text, and `IF TARGETS`.
#[derive(Clone, Debug)]
pub struct LqBlock {
    /// The text the block binds from.
    pub text: String,
    /// `params`.
    pub params: crate::lq::ctx::Params,
    /// Each statement's text, in order.
    pub stmts: Vec<String>,
    /// `IF TARGETS`: the 32 hex digits of the expected digest.
    pub if_targets: Option<String>,
    /// The block is one `CALL tx.<name>(…)` (`stmt_origin` `named-mutation`, [API §9.1]).
    pub one_call: Option<String>,
    /// Each statement's byte range in `text`, for the statement a binder refusal names.
    pub spans: Vec<(usize, usize)>,
    /// Every statement is a `RESOLVE` (a staging ref accepts only these, [50 §3.9] item 6).
    pub resolve_only: bool,
}

/// A `TX` block's text prepared for the write path ([API §9.1]): the block, the context with the options the text
/// spells (`ON`, `LEASE`, `KEY`, `DRY`, `IF TIP`; a text value differing from the command's is `usage`), and the
/// commit message.
// spec: [API §9.1] lq form
pub fn prepare(
    store: &Store,
    text: &str,
    params: crate::lq::ctx::Params,
    message: &str,
    if_targets: Option<&str>,
    ctx: &Ctx,
) -> Res<(LqBlock, Ctx, String)> {
    use crate::lq::ast::{RevKind, Stmt as S};
    let parsed = crate::lq::parser::parse_write(text, crate::lq::parser::ParseOptions::default())
        .map_err(|d| crate::lqh::refusal(&d[0]).0)?;
    let t = &parsed.tree;
    let mut c = ctx.clone();
    let differ =
        |what: &str| Refusal::usage(format!("{what} differs between the block and the command"));
    if let Some(r) = &t.on {
        let b = match &r.kind {
            RevKind::Ref(n) => n.clone(),
            RevKind::Head => store.resolve(ctx, false)?.branch,
            _ => {
                let span = &text[r.span.start as usize..r.span.end as usize];
                return Err(Refusal::lq(
                    "E305",
                    format!("{span} is read-only: a commit"),
                ));
            }
        };
        if c.branch.as_ref().is_some_and(|x| *x != b) {
            return Err(differ("ON"));
        }
        c.branch = Some(b);
    }
    for (v, slot, what) in [
        (&t.lease, &mut c.lease, "LEASE"),
        (&t.key, &mut c.key, "KEY"),
    ] {
        if let Some(v) = v {
            if slot.as_ref().is_some_and(|x| x != v) {
                return Err(differ(what));
            }
            *slot = Some(v.clone());
        }
    }
    c.dry |= t.dry;
    if let Some(r) = &t.if_tip {
        let span = &text[r.span.start as usize..r.span.end as usize];
        let caller = store.resolve(&c, false)?;
        let seq = store
            .dag
            .rev_commit(span, &store.rev_ctx(&caller))?
            .unwrap_or(0);
        if c.if_tip.is_some_and(|x| x != seq) {
            return Err(differ("IF TIP"));
        }
        c.if_tip = Some(seq);
    }
    let targets = match (&t.if_targets, if_targets) {
        (Some(a), Some(b)) if a != b => return Err(differ("IF TARGETS")),
        (Some(a), _) => Some(a.clone()),
        (None, b) => b.map(str::to_string),
    };
    let msg = match (&t.message, message) {
        (Some(a), m) if !m.is_empty() && m != a => return Err(differ("MESSAGE")),
        (Some(a), _) => a.clone(),
        (None, m) => m.to_string(),
    };
    // A command `message` the text does not spell is the block's `MESSAGE`, inside `H` ([API §7.3], §9.3).
    let text = if t.message.is_none() && !message.is_empty() {
        let at = t.span.start as usize + 2;
        format!(
            "{} MESSAGE {}{}",
            &text[..at],
            crate::lq::printer::string_lit(message),
            &text[at..]
        )
    } else {
        text.to_string()
    };
    let reparsed =
        crate::lq::parser::parse_write(&text, crate::lq::parser::ParseOptions::default())
            .map_err(|d| crate::lqh::refusal(&d[0]).0)?;
    let stmts = reparsed
        .tree
        .stmts
        .iter()
        .map(|s| stmt_text(&text, s).0)
        .collect();
    let spans = reparsed
        .tree
        .stmts
        .iter()
        .map(|s| stmt_text(&text, s).1)
        .collect();
    let resolve_only = reparsed
        .tree
        .stmts
        .iter()
        .all(|s| matches!(s, S::Resolve(_)));
    let one_call = match reparsed.tree.stmts.as_slice() {
        [S::TxCall { name, .. }] => Some(format!("tx.{}", name.text)),
        _ => None,
    };
    Ok((
        LqBlock {
            text,
            params,
            stmts,
            if_targets: targets,
            one_call,
            spans,
            resolve_only,
        },
        c,
        msg,
    ))
}

/// The source text of a statement of the S-AST.
fn stmt_text(text: &str, s: &crate::lq::ast::Stmt) -> (String, (usize, usize)) {
    use crate::lq::ast::Stmt as S;
    let sp = match s {
        S::Match(m) => m.span,
        S::Muts(_, sp) => *sp,
        S::Create(c) => c.span,
        S::TxCall { span, .. } | S::Assert { span, .. } => *span,
        S::Resolve(r) => r.span,
        S::Define(d) => d.span,
        S::Drop(n) => {
            let end = n.span.end as usize;
            let start = text[..n.span.start as usize]
                .rfind("DROP")
                .or_else(|| {
                    text[..n.span.start as usize]
                        .to_ascii_uppercase()
                        .rfind("DROP")
                })
                .unwrap_or(n.span.start as usize);
            return (format!("DROP QUERY {}", n.text), (start, end));
        }
    };
    let (s, e) = (sp.start as usize, sp.end as usize);
    (text.get(s..e).unwrap_or("").to_string(), (s, e))
}

/// The value of an LQ value as a kernel argument ([API §5.2]): a node as `#N`, a timestamp field in Unix seconds
/// ([LQ/std §2.13]), a duration in milliseconds.
fn to_p(store: &Store, st: &crate::state::State, kind: &str, field: &str, v: &V) -> P {
    match v {
        V::Absent => P::Null,
        V::Bool(b) => P::Bool(*b),
        V::Int(i) => P::Int(*i),
        V::Float(f) => P::Float(*f),
        V::Text(s) => P::Text(s.clone()),
        V::Enum(e) => P::Text(e.name.clone()),
        V::Time(ms) => {
            if st
                .schema
                .field(kind, field)
                .is_some_and(|f| f.coerce == "timestamp")
            {
                P::Int(ms.div_euclid(1000))
            } else {
                P::Text(super::func::iso(*ms))
            }
        }
        V::Dur(ms) => P::Int(*ms),
        V::Node(n) => P::Text(format!("#{}", n.0)),
        V::Rev(c) => P::Text(format!(
            "c{}",
            store
                .dag
                .commits
                .get(c)
                .map_or_else(String::new, |x| hex(&x.id))
        )),
        V::List(l) => P::List(l.iter().map(|x| to_p(store, st, kind, field, x)).collect()),
        other => P::Text(super::expr::display(other)),
    }
}

/// The commit text of a `pinned` value ([F08 §10]): a revision value as its id, a text as a revision to resolve.
fn pin_text(store: &Store, i: usize, v: &V) -> Res<String> {
    match v {
        V::Rev(c) => Ok(store
            .dag
            .commits
            .get(c)
            .map_or_else(String::new, |x| format!("c{}", hex(&x.id)))),
        V::Text(t) => Ok(t.clone()),
        other => Err(Refusal::lq(
            "E103",
            format!(
                "statement {i}: pinned takes a revision; got {}",
                display(other)
            ),
        )
        .finish(Some(i))),
    }
}

/// The `pinned` commit of an edge's property map in a `CREATE`; any other property is not writable ([50 §3.10] item
/// 6: edge properties other than a pin are the verbs').
fn edge_pin(
    ev: &Ev<'_>,
    env: &Env<'_>,
    store: &Store,
    i: usize,
    lq: &str,
    props: &[(String, CExpr)],
) -> Res<Option<String>> {
    let mut pin = None;
    for (k, x) in props {
        if k != "pinned" {
            return Err(Refusal::lq(
                "E115",
                format!(
                    "statement {i}: {lq}.{k} is not writable; an edge's CREATE writes only pinned"
                ),
            )
            .finish(Some(i)));
        }
        pin = Some(pin_text(store, i, &ev.eval(x, env)?)?);
    }
    Ok(pin)
}

/// The counter increment `t.c + k` or `k + t.c` of a `SET t.c = …` ([50 §3.2]): the property's base expression and
/// the expression of k, when the value is a sum with the field `f` read on either side.
fn increment<'e>(x: &'e CExpr, f: &str) -> Option<(&'e CExpr, &'e CExpr)> {
    let CExpr::Arith(1, a, b) = x else {
        return None;
    };
    match (&**a, &**b) {
        (CExpr::Prop(base, g), k) if g == f => Some((base, k)),
        (k, CExpr::Prop(base, g)) if g == f => Some((base, k)),
        _ => None,
    }
}

/// The nodes a target value names.
fn nodes(v: &V) -> Vec<Nid> {
    match v {
        V::Node(n) => vec![*n],
        V::List(l) => l.iter().filter_map(V::node).collect(),
        _ => Vec::new(),
    }
}

/// `EXPECT`'s bound as its text ([LQ/errors §5.7] `expect`).
fn expect_text((lo, hi): (u64, Option<u64>)) -> String {
    match hi {
        Some(h) if h == lo => lo.to_string(),
        Some(h) if lo == 0 => format!("<= {h}"),
        Some(h) => format!("{lo}..{h}"),
        None => format!(">= {lo}"),
    }
}

/// The variables a statement's mutations read, ascending ([LQ/envelope §9.4] row 3a: "in variable-index order").
fn mut_vars(muts: &[CMut], out: &mut BTreeSet<BindingId>) {
    fn ex(e: &CExpr, out: &mut BTreeSet<BindingId>) {
        match e {
            CExpr::Var(b) => {
                out.insert(*b);
            }
            e => e.each_operand(&mut |x| ex(x, out)),
        }
    }
    for m in muts {
        match m {
            CMut::Set(v) => v.iter().for_each(|(t, _, x)| {
                ex(t, out);
                ex(x, out);
            }),
            CMut::Remove(v) => v.iter().for_each(|(t, _)| ex(t, out)),
            CMut::Delete {
                targets,
                replaced_by,
                ..
            } => {
                targets.iter().for_each(|t| ex(t, out));
                if let Some(r) = replaced_by {
                    ex(r, out);
                }
            }
            CMut::Move {
                target, under, rel, ..
            } => {
                ex(target, out);
                ex(under, out);
                if let Some(r) = rel {
                    ex(r, out);
                }
            }
            CMut::Edge(a, _, _, b) => {
                ex(a, out);
                ex(b, out);
            }
            CMut::Reopen(t, r) => {
                ex(t, out);
                ex(r, out);
            }
            CMut::Patch(t, _, a, b) => {
                ex(t, out);
                ex(a, out);
                ex(b, out);
            }
        }
    }
}

/// The runner of one block over a candidate.
struct Runner<'s, 'c, 'k> {
    store: &'s Store,
    caller: &'s ApiCaller,
    ctx: &'s Ctx,
    cand: &'c mut Cand<'k>,
    hlc: &'s crate::clock::Hlc,
    vars: Row,
    portable: &'s [(String, String)],
    /// The digest's statement records: (index, bindings).
    targets: Vec<(u32, Vec<Vec<u8>>)>,
    /// The nodes the current statement deleted: its bindings name a node once however many rows repeat it.
    del_nodes: BTreeSet<Nid>,
    /// The edges (source, key) the current statement deleted, likewise.
    del_edges: BTreeSet<(Nid, EdgeKey)>,
}

impl<'s> Runner<'s, '_, '_> {
    /// The candidate as a tip view of the branch written ([50 §3.10] item 2), sharing the candidate's state: a view
    /// must be dropped before the next kernel statement, which then writes the state in place.
    fn view(&self) -> View {
        View {
            st: self.cand.st.shared(),
            commit: self
                .store
                .dag
                .live(&self.cand.cx.branch)
                .and_then(|r| r.tip),
            ref_name: self.cand.cx.branch.clone(),
            branch: Some(self.cand.cx.branch.clone()),
            kind: ViewKind::Tip,
        }
    }

    /// Runs one kernel statement as part of LQ statement `i`.
    fn k(&mut self, i: usize, s: Stmt) -> Res<()> {
        self.cand.run(i, &s, self.hlc)
    }

    /// The evaluation inputs of the block's statements: the caller's, with the candidate's lease table, so that
    /// runtime predicates (`t.claimed`, `t.ready`) see the leases earlier statements took or ended ([50 §3.10] item 2).
    fn world(&self) -> World<'s> {
        let mut w = self
            .store
            .world(self.caller, self.ctx, super::Ablations::default(), None);
        w.leases = Some(Rc::new(self.cand.leases.clone()));
        w
    }

    /// The bytes of one binding of the target-set digest ([LQ/envelope §9.4] row 3a).
    // spec: [LQ/envelope §9.4] row 3a
    fn binding_bytes(&self, row: &Row, vars: &BTreeSet<BindingId>) -> Vec<u8> {
        let mut b = Vec::new();
        let uid = |n: Nid| {
            self.store
                .alloc
                .uids
                .get(&n)
                .copied()
                .or_else(|| self.cand.new_alloc.get(&n).map(|x| x.0))
                .unwrap_or(crate::value::Uid::ZERO)
                .0
        };
        for v in vars {
            match row.get(*v) {
                V::Node(n) => b.extend_from_slice(&uid(*n)),
                V::Edge(e) => {
                    b.extend_from_slice(&uid(e.src));
                    let lq = self
                        .cand
                        .st
                        .schema
                        .edge(&e.kind)
                        .map_or_else(|| e.kind.to_uppercase(), |x| x.lq_name.clone());
                    lp(&mut b, lq.as_bytes());
                    b.extend_from_slice(&uid(e.dst));
                    b.extend_from_slice(&e.disc.map_or([0u8; 16], |u| u.0));
                }
                _ => {}
            }
        }
        b
    }

    /// One statement ([50 §3.10] item 6).
    // spec: [50 §3.10] item 6
    fn stmt(&mut self, i: usize, s: &CStmt) -> Res<()> {
        let w = self.world();
        self.del_nodes.clear();
        self.del_edges.clear();
        match s {
            CStmt::Match {
                patterns,
                where_,
                expect,
                muts,
            } => {
                let rows = {
                    let view = self.view();
                    let ev = Ev::new(&w, &view);
                    let mut out = Vec::new();
                    ev.match_paths(&self.vars, patterns, where_.as_ref(), &[], &mut out, false)?;
                    out
                };
                let n = rows.len() as u64;
                let mut vars = BTreeSet::new();
                mut_vars(muts, &mut vars);
                let mut matched: Vec<Nid> = Vec::new();
                for r in &rows {
                    for v in &vars {
                        matched.extend(nodes(r.get(*v)));
                    }
                }
                if n < expect.0 || expect.1.is_some_and(|h| n > h) {
                    let mut literal: Vec<Nid> = Vec::new();
                    for p in patterns {
                        let mut np = vec![&p.start];
                        for st in &p.steps {
                            match st {
                                crate::lq::cast::CStep::Edge(_, x)
                                | crate::lq::cast::CStep::Group(_, x) => np.push(x),
                            }
                        }
                        for x in np {
                            if let Some((_, CExpr::Node(u))) =
                                x.props.iter().find(|(k, _)| k == "id")
                                && let Some(m) = self.store.alloc.uidx.get(&crate::value::Uid(*u))
                            {
                                literal.push(*m);
                            }
                        }
                    }
                    literal.extend(matched.iter().copied());
                    let mut seen = BTreeSet::new();
                    literal.retain(|m| seen.insert(*m));
                    let ids: Vec<String> =
                        literal.iter().take(10).map(|m| m.0.to_string()).collect();
                    let current = Kv::List(
                        literal
                            .iter()
                            .take(10)
                            .map(|m| self.cand.current(*m))
                            .collect(),
                    );
                    return Err(Refusal::lq(
                        "E401",
                        format!(
                            "statement {i} matched {n} bindings, expected {}",
                            expect_text(*expect)
                        ),
                    )
                    .line("nothing was written")
                    .help(format!("re-read with moirai q show ids={}", ids.join(",")))
                    .key("expect", expect_text(*expect))
                    .key("matched", n as i64)
                    .key("current", current)
                    .finish(Some(i)));
                }
                let mut bindings: Vec<Vec<u8>> =
                    rows.iter().map(|r| self.binding_bytes(r, &vars)).collect();
                bindings.sort();
                bindings.dedup();
                self.targets.push((i as u32, bindings));
                matched.sort();
                matched.dedup();
                self.cand.record_targets(i, &matched);
                for r in &rows {
                    self.muts(i, muts, r)?;
                }
                // V10: the statement's new variables are visible to later statements; a variable bound to several
                // values holds the distinct ones as a list in order of first binding (a value the rows repeat names
                // one node or edge, which a later statement writes once), and one bound to none an empty list. A
                // variable an earlier statement bound keeps its value.
                let mut new_vars = Vec::new();
                for p in patterns {
                    super::query::path_vars(p, &mut new_vars);
                }
                new_vars.retain(|b| !self.vars.is_bound(*b));
                for b in new_vars {
                    let mut firsts: Vec<(usize, &V)> =
                        rows.iter().map(|r| r.get(b)).enumerate().collect();
                    firsts.sort_by(|x, y| super::val::cmp_total(x.1, y.1).then(x.0.cmp(&y.0)));
                    firsts.dedup_by(|x, y| super::val::same(x.1, y.1));
                    firsts.sort_unstable_by_key(|x| x.0);
                    let mut vals: Vec<V> = firsts.into_iter().map(|(_, v)| v.clone()).collect();
                    let v = match vals.len() {
                        1 => vals.remove(0),
                        _ => V::List(vals),
                    };
                    self.vars.set(b, v);
                }
                Ok(())
            }
            CStmt::Muts(muts) => {
                let row = self.vars.clone();
                self.muts(i, muts, &row)
            }
            CStmt::Create {
                var,
                kind,
                props,
                edges,
                under,
                unless,
            } => {
                let view = self.view();
                let ev = Ev::new(&w, &view);
                if let Some(sub) = unless {
                    let mut out = Vec::new();
                    match sub {
                        crate::lq::cast::CSub::Patterns(p, wh) => {
                            ev.match_paths(&self.vars, p, wh.as_ref(), &[], &mut out, false)?
                        }
                        crate::lq::cast::CSub::Clauses(c, _) => {
                            out = ev.run_clauses(vec![self.vars.clone()], c, &[])?
                        }
                    }
                    let mut found: Vec<Nid> =
                        out.iter().filter_map(|r| r.get(*var).node()).collect();
                    found.sort();
                    found.dedup();
                    match found.as_slice() {
                        [] => {}
                        [one] => {
                            self.vars.set(*var, V::Node(*one));
                            self.cand.record_targets(i, &[*one]);
                            return Ok(());
                        }
                        many => {
                            return Err(Refusal::lq(
                                "E410",
                                format!(
                                    "statement {i}: UNLESS EXISTS matched {} nodes; create-or-bind needs at most 1",
                                    many.len()
                                ),
                            )
                            .finish(Some(i)));
                        }
                    }
                }
                let env = Env::of(&self.vars, &[]);
                let mut fields = Vec::new();
                let mut body = None;
                for (k, e) in props {
                    let v = ev.eval(e, &env)?;
                    if k == "body" {
                        body = v.as_str().map(str::to_string);
                    } else {
                        fields.push((k.clone(), to_p(self.store, &view.st, kind, k, &v)));
                    }
                }
                let under = match under {
                    Some(u) => ev.eval(u, &env)?.node().map(Target::Id),
                    None => None,
                };
                let mut out_e = Vec::new();
                let mut in_e = Vec::new();
                // Edges with a pinned commit follow the `Create` as links of their own ([API §9.2]: each edge's
                // `CREATE` is its own statement), in their written order.
                let mut pinned_e: Vec<(bool, String, Nid, String)> = Vec::new();
                for (dir, lq, props, t) in edges {
                    let Some(stored) = ev.stored_name(lq) else {
                        return Err(Refusal::lq("E104", format!("unknown edge type {lq}")));
                    };
                    let pin = edge_pin(&ev, &env, self.store, i, lq, props)?;
                    for n in nodes(&ev.eval(t, &env)?) {
                        match &pin {
                            Some(p) => pinned_e.push((*dir == 1, stored.clone(), n, p.clone())),
                            None if *dir == 1 => out_e.push((stored.clone(), Target::Id(n))),
                            None => in_e.push((stored.clone(), Target::Id(n))),
                        }
                    }
                }
                drop(ev);
                drop(view);
                self.k(
                    i,
                    Stmt::Create {
                        name: None,
                        kind: kind.clone(),
                        fields,
                        body,
                        under,
                        position: None,
                        edges_out: out_e,
                        edges_in: in_e,
                    },
                )?;
                if let Some(n) = self.cand.created.last().copied() {
                    self.vars.set(*var, V::Node(n));
                    for (out, kind, other, pin) in pinned_e {
                        let (src, dst) = if out { (n, other) } else { (other, n) };
                        self.k(
                            i,
                            Stmt::Link {
                                src: Target::Id(src),
                                kind,
                                dst: Target::Id(dst),
                                pinned: Some(pin),
                            },
                        )?;
                    }
                }
                Ok(())
            }
            CStmt::TxCall { name, args, items } => {
                let view = self.view();
                let ev = Ev::new(&w, &view);
                let env = Env::of(&self.vars, &[]);
                let mut ps = Vec::new();
                for a in args {
                    let v = ev.eval(&a.value, &env)?;
                    ps.push((
                        a.name.clone().unwrap_or_default(),
                        to_p(self.store, &view.st, "", "", &v),
                    ));
                }
                drop(ev);
                drop(view);
                let before = self.cand.yields.len();
                self.k(
                    i,
                    Stmt::Call {
                        proc: name.clone(),
                        args: ps,
                    },
                )?;
                if let Some(y) = self.cand.yields.get(before).cloned()
                    && let Some(row) = y.rows.first()
                {
                    for it in items {
                        let v = row
                            .iter()
                            .find(|(k, _)| *k == it.field)
                            .map_or(V::Absent, |(_, t)| yield_value(t));
                        self.vars.set(it.var, v);
                    }
                }
                Ok(())
            }
            CStmt::Assert { expr, else_ } => {
                let view = self.view();
                let ev = Ev::new(&w, &view);
                if truth(&ev.eval(expr, &Env::of(&self.vars, &[]))?) {
                    return Ok(());
                }
                let mut r = Refusal::lq("E403", format!("statement {i}: ASSERT is false"));
                if let Some(t) = else_ {
                    r = r.line(format!("\"{t}\""));
                }
                Err(r.line("nothing was written").finish(Some(i)))
            }
            CStmt::Resolve { key, take, operand } => {
                let view = self.view();
                let (keys, op) = {
                    let ev = Ev::new(&w, &view);
                    let op = match operand {
                        Some(o) => ev.eval(o, &Env::of(&self.vars, &[]))?,
                        None => V::Absent,
                    };
                    let keys: Vec<String> = match key {
                        CResolveKey::Text(k) => vec![k.clone()],
                        CResolveKey::Query(q, expect) => {
                            let t = w.eval_query(q, &Frame::default(), Some(&ev))?;
                            let n = t.rows.len() as u64;
                            if n < expect.0 || expect.1.is_some_and(|h| n > h) {
                                return Err(Refusal::lq(
                                    "E401",
                                    format!(
                                        "statement {i} matched {n} bindings, expected {}",
                                        expect_text(*expect)
                                    ),
                                )
                                .key("expect", expect_text(*expect))
                                .key("matched", n as i64)
                                .key("current", Kv::List(Vec::new()))
                                .finish(Some(i)));
                            }
                            t.rows
                                .iter()
                                .filter_map(|r| {
                                    r.first().and_then(|v| v.as_str().map(str::to_string))
                                })
                                .collect()
                        }
                    };
                    (keys, op)
                };
                let pin = match take {
                    4 => Some(to_p(self.store, &view.st, "", "", &op)),
                    _ => None,
                };
                drop(view);
                let take = match take {
                    1 => Take::Ours,
                    2 => Take::Theirs,
                    3 => Take::Base,
                    4 => Take::Value(pin.expect("a VALUE operand")),
                    5 => Take::Repoint(match op.node() {
                        Some(n) => Target::Id(n),
                        None => return Err(Refusal::lq("E103", "REPOINT takes a node")),
                    }),
                    _ => Take::Drop,
                };
                for k in keys {
                    self.k(
                        i,
                        Stmt::Resolve {
                            key: k,
                            take: take.clone(),
                        },
                    )?;
                }
                Ok(())
            }
            CStmt::Define(d) => self.define(i, d),
            CStmt::Drop(name) => {
                let k = crate::schema::ItemKey::Query(name.clone());
                if self.cand.st.schema.items.remove(&k).is_none() {
                    return Err(Refusal::not_found("query", name.clone()));
                }
                Ok(())
            }
        }
    }

    /// `DEFINE QUERY` ([50 §4.4]; [F08 §8.5.5]): the item `query:<name>` with its stored portable text. Whether the
    /// candidate's named queries still bind and call no cycle is V10 and V11, which the block's deferred validators
    /// check on the parsed texts ([F19 §12.5.6]; [`crate::tx::Cand::deferred`]).
    // spec: [50 §4.4]
    fn define(&mut self, i: usize, d: &crate::lq::cast::CDefine) -> Res<()> {
        let text = self
            .portable
            .iter()
            .find(|(n, _)| *n == d.name)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| {
                Refusal::lq(
                    "E405",
                    format!("statement {i}: named queries bind (QueryInvalid)"),
                )
            })?;
        let open = text.find('(').unwrap_or(0);
        let close = text[open..].find(')').map_or(open, |c| open + c);
        let item = crate::schema::QueryItem {
            name: d.name.clone(),
            lq_version: 1,
            params: text.get(open + 1..close).unwrap_or("").to_string(),
            shape: d.shape.clone().unwrap_or_else(|| "table".into()),
            budget: d.budget.clone().unwrap_or_else(|| "medium".into()),
            text,
        };
        self.cand.st.schema.items.insert(
            crate::schema::ItemKey::Query(d.name.clone()),
            crate::schema::Item::Query(item),
        );
        Ok(())
    }

    /// The mutations of a statement for one binding row ([50 §3.10] item 6): `SET` and `REMOVE` as `set` (a counter
    /// `c = c + k` as `incr`, `status`, `done` and `resolution` as the guarded transition, `body`, `parent` as a move),
    /// `DELETE` of a node or of an edge (each node and edge once per statement, however many of its rows name it),
    /// `MOVE`, a created edge, `REOPEN` and `PATCH`.
    // spec: [50 §3.10] item 6
    fn muts(&mut self, i: usize, muts: &[CMut], row: &Row) -> Res<()> {
        for m in muts {
            let w = self.world();
            let view = self.view();
            let ev = Ev::new(&w, &view);
            let env = Env::of(row, &[]);
            let mut ks: Vec<Stmt> = Vec::new();
            let mut unlink: Vec<(Nid, EdgeKey)> = Vec::new();
            match m {
                CMut::Set(assigns) => {
                    let mut per: Vec<(Nid, Stmt)> = Vec::new();
                    let mut completes: Vec<Stmt> = Vec::new();
                    let leased = self.cand.cx.rights.lease.as_ref().and_then(|l| l.task);
                    for (t, f, x) in assigns {
                        for n in nodes(&ev.eval(t, &env)?) {
                            let kind = view
                                .st
                                .nodes
                                .get(&n)
                                .map_or_else(String::new, |y| y.kind.clone());
                            // `SET t.done = true` on the presented lease's own task is its completion ([50 §2.9] Q18;
                            // [LQ/envelope §9.1]: resolution `completed`, the lease released into a settled marker).
                            if leased == Some(n) && kind == "task" {
                                let v = ev.eval(x, &env)?;
                                let done = (f == "done" && v == V::Bool(true))
                                    || (f == "status" && v.as_str() == Some("done"));
                                if done {
                                    completes.push(Stmt::Call {
                                        proc: "tx.complete".into(),
                                        args: vec![
                                            ("id".into(), P::Text(format!("#{}", n.0))),
                                            ("outcome".into(), P::Text("done".into())),
                                            ("summary".into(), P::Text(String::new())),
                                        ],
                                    });
                                    continue;
                                }
                            }
                            let idx = match per.iter().position(|(m, _)| *m == n) {
                                Some(p) => p,
                                None => {
                                    per.push((
                                        n,
                                        Stmt::Set {
                                            target: Target::Id(n),
                                            fields: Vec::new(),
                                            incr: Vec::new(),
                                            body: None,
                                            guard: None,
                                        },
                                    ));
                                    per.len() - 1
                                }
                            };
                            let counter = view
                                .st
                                .schema
                                .field(&kind, f)
                                .is_some_and(|d| d.ty == crate::schema::Ty::Counter);
                            if f == "parent" {
                                let to = ev.eval(x, &env)?;
                                ks.push(Stmt::Move {
                                    target: Target::Id(n),
                                    under: to.node().map(Target::Id),
                                    position: None,
                                });
                                continue;
                            }
                            let Stmt::Set {
                                fields, incr, body, ..
                            } = &mut per[idx].1
                            else {
                                continue;
                            };
                            // `SET t.c = t.c + k` (or `k + t.c`) on a counter of the target itself is its `Incr`
                            // ([50 §3.2]); k is an integer. Any other assignment to a counter reaches the kernel,
                            // which refuses it (E103).
                            let inc = match increment(x, f).filter(|_| counter) {
                                Some((base, k))
                                    if ev.eval(base, &env)?.elems().contains(&V::Node(n)) =>
                                {
                                    Some(k)
                                }
                                _ => None,
                            };
                            match inc {
                                Some(k) => {
                                    let d = match ev.eval(k, &env)? {
                                        V::Int(d) => d,
                                        other => {
                                            return Err(Refusal::lq(
                                                "E103",
                                                format!(
                                                    "statement {i}: {kind}.{f} is a counter: an increment takes an integer; got {}",
                                                    if other.is_absent() { "null".to_string() } else { display(&other) }
                                                ),
                                            )
                                            .finish(Some(i)));
                                        }
                                    };
                                    incr.push((f.clone(), d));
                                }
                                None => {
                                    let v = ev.eval(x, &env)?;
                                    if f == "body" {
                                        *body = Some(v.as_str().map(str::to_string));
                                    } else {
                                        fields.push((
                                            f.clone(),
                                            to_p(self.store, &view.st, &kind, f, &v),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    ks.extend(per.into_iter().map(|(_, s)| s));
                    ks.extend(completes);
                }
                CMut::Remove(items) => {
                    for (t, f) in items {
                        for n in nodes(&ev.eval(t, &env)?) {
                            ks.push(Stmt::Set {
                                target: Target::Id(n),
                                fields: if f == "body" {
                                    Vec::new()
                                } else {
                                    vec![(f.clone(), P::Null)]
                                },
                                incr: Vec::new(),
                                body: (f == "body").then_some(None),
                                guard: None,
                            });
                        }
                    }
                }
                CMut::Delete {
                    targets,
                    policy,
                    replaced_by,
                    release,
                    reason,
                } => {
                    let repl = match replaced_by {
                        Some(r) => ev.eval(r, &env)?.node().map(Target::Id),
                        None => None,
                    };
                    let reason = match reason {
                        Some(r) => ev.eval(r, &env)?.as_str().map(str::to_string),
                        None => None,
                    };
                    // Each target names nodes and edges: a value, or a list of them (a variable an earlier statement
                    // bound to several, [LQ/canonical-ast §5.7] V10); anything else is refused, never ignored.
                    for t in targets {
                        let v = ev.eval(t, &env)?;
                        let elems: Vec<V> = match v {
                            V::List(l) => l,
                            other => vec![other],
                        };
                        // A node or edge the statement already deleted — named again by another of its rows or
                        // targets — is deleted once.
                        for x in elems {
                            match x {
                                V::Edge(e) => {
                                    let key = EdgeKey {
                                        kind: e.kind.clone(),
                                        dst: e.dst,
                                        disc: e.disc,
                                    };
                                    if self.del_edges.insert((e.src, key.clone())) {
                                        unlink.push((e.src, key));
                                    }
                                }
                                V::Node(n) => {
                                    if self.del_nodes.insert(n) {
                                        ks.push(Stmt::Delete {
                                            target: Target::Id(n),
                                            policy: match policy {
                                                1 => Some("restrict".into()),
                                                2 => Some("cascade".into()),
                                                3 => Some("reparent".into()),
                                                _ => None,
                                            },
                                            replaced_by: repl.clone(),
                                            release: *release,
                                            reason: reason.clone(),
                                        });
                                    }
                                }
                                other => {
                                    return Err(Refusal::lq(
                                        "E103",
                                        format!(
                                            "statement {i}: DELETE takes a node or an edge; got {}",
                                            if other.is_absent() {
                                                "null".to_string()
                                            } else {
                                                display(&other)
                                            }
                                        ),
                                    )
                                    .finish(Some(i)));
                                }
                            }
                        }
                    }
                }
                CMut::Move {
                    target,
                    under,
                    pos,
                    rel,
                } => {
                    let rel = match rel {
                        Some(r) => ev.eval(r, &env)?.node().map(Target::Id),
                        None => None,
                    };
                    let position = match (pos, rel) {
                        (1, Some(r)) => Some(Position::Before(r)),
                        (2, Some(r)) => Some(Position::After(r)),
                        (3, _) => Some(Position::First),
                        (4, _) => Some(Position::Last),
                        _ => None,
                    };
                    let to = ev.eval(under, &env)?.node().map(Target::Id);
                    for n in nodes(&ev.eval(target, &env)?) {
                        ks.push(Stmt::Move {
                            target: Target::Id(n),
                            under: to.clone(),
                            position: position.clone(),
                        });
                    }
                }
                CMut::Edge(a, lq, props, b) => {
                    let stored = ev
                        .stored_name(lq)
                        .ok_or_else(|| Refusal::lq("E104", format!("unknown edge type {lq}")))?;
                    let pinned = edge_pin(&ev, &env, self.store, i, lq, props)?;
                    for s in nodes(&ev.eval(a, &env)?) {
                        for d in nodes(&ev.eval(b, &env)?) {
                            ks.push(Stmt::Link {
                                src: Target::Id(s),
                                kind: stored.clone(),
                                dst: Target::Id(d),
                                pinned: pinned.clone(),
                            });
                        }
                    }
                }
                CMut::Reopen(t, r) => {
                    let reason = ev.eval(r, &env)?.as_str().unwrap_or("").to_string();
                    for n in nodes(&ev.eval(t, &env)?) {
                        ks.push(Stmt::Reopen {
                            target: Target::Id(n),
                            reason: reason.clone(),
                        });
                    }
                }
                CMut::Patch(t, _, old, new) => {
                    let old = ev.eval(old, &env)?.as_str().unwrap_or("").to_string();
                    let new = ev.eval(new, &env)?.as_str().unwrap_or("").to_string();
                    for n in nodes(&ev.eval(t, &env)?) {
                        ks.push(Stmt::Patch {
                            target: Target::Id(n),
                            remove: old.clone(),
                            add: new.clone(),
                        });
                    }
                }
            }
            drop(ev);
            drop(view);
            for (src, key) in unlink {
                self.cand.stmt = i;
                self.cand.sub = 0;
                self.cand
                    .unlink_key(src, &key)
                    .map_err(|e| e.finish(Some(i)))?;
            }
            for s in ks {
                self.k(i, s)?;
            }
        }
        Ok(())
    }
}

/// A yielded value's text as a value: `#N` a node, an integer, else text.
fn yield_value(t: &str) -> V {
    if let Some(n) = t.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()) {
        return V::Node(Nid(n));
    }
    if let Ok(i) = t.parse::<i64>() {
        return V::Int(i);
    }
    if t == "null" {
        return V::Absent;
    }
    V::text(t)
}

/// The target-set digest ([LQ/envelope §9.4]): BLAKE3-128 over the domain, the grammar version and, per `MATCH …
/// EXPECT` statement in block order, its index, its binding count and its bindings sorted bytewise.
// spec: [LQ/envelope §9.4]
pub fn digest(stmts: &[(u32, Vec<Vec<u8>>)]) -> [u8; 16] {
    let mut b = Vec::new();
    lp(&mut b, b"moirai-lq-targets-v1");
    b.extend_from_slice(&1u16.to_le_bytes());
    for (i, bindings) in stmts {
        b.extend_from_slice(&i.to_le_bytes());
        b.extend_from_slice(&(bindings.len() as u32).to_le_bytes());
        for x in bindings {
            b.extend_from_slice(x);
        }
    }
    crate::canon::b3_128(&b)
}

/// Runs a bound block's statements on the candidate ([50 §3.10] items 1–3) and returns the target-set digest;
/// `IF TARGETS` refuses with E402 when it differs ([LQ/errors §5.5]).
#[allow(clippy::too_many_arguments)]
// spec: [50 §3.10]
pub fn run(
    store: &Store,
    cand: &mut Cand<'_>,
    blk: &LqBlock,
    tx: &CTx,
    portable: &[(String, String)],
    caller: &ApiCaller,
    ctx: &Ctx,
    hlc: &crate::clock::Hlc,
) -> Res<[u8; 16]> {
    let mut r = Runner {
        store,
        caller,
        ctx,
        cand,
        hlc,
        vars: Row::default(),
        portable,
        targets: Vec::new(),
        del_nodes: BTreeSet::new(),
        del_edges: BTreeSet::new(),
    };
    for (i, s) in tx.stmts.iter().enumerate() {
        r.stmt(i + 1, s)?;
    }
    let d = digest(&r.targets);
    if let Some(want) = &blk.if_targets
        && *want != hex(&d)
    {
        let tip = store.dag.live(&cand.cx.branch).and_then(|x| x.tip);
        return Err(Refusal::lq(
            "E402",
            format!("IF TARGETS {want}: the targets now digest to {}", hex(&d)),
        )
        .line("nothing was written")
        .help("run the block with DRY again and apply its digest")
        .key("statement", Kv::Null)
        .key("tip", tip.map_or(Kv::Null, Kv::Commit))
        .key("expected_tip", ctx.if_tip.map_or(Kv::Null, Kv::Commit))
        .key("targets", hex(&d))
        .key("written", false));
    }
    Ok(d)
}
