//! Calls ([LQ/canonical-ast §5.3] "a procedure", §5.8 N2; [LQ/std §2.1], §2.9): name resolution (relations, then
//! `std`, then the project; `moirai q NAME` looks up `std` first), arguments named and ordered by the callee's
//! signature, yields as new bindings (V6), the relation classes of [50 §3.9] item 4 (E302), and E304.

use super::{AggPos, BKind, Binder};
use crate::lq::ast::*;
use crate::lq::cast::*;
use crate::lq::catalog::{self, PT, RelClass, Ty};
use crate::lq::ctx::{BindCtx, Caller, MapIds, Params};
use crate::lq::diag::{Code, Diag, Span, near, q};
use crate::lq::parser::{ParseOptions, parse_define};
use std::cell::RefCell;

/// A resolved callee.
#[derive(Clone)]
pub(super) struct Sig {
    /// The C-AST name: the relation's registry name, `std.<name>` or the project qname.
    pub proc: String,
    /// Parameters: (name, type, required).
    pub params: Vec<(String, PT, bool)>,
    /// Columns: (name, type).
    pub yields: Vec<(String, Ty)>,
    /// A named query (E110 texts) rather than a relation (E109 texts).
    pub named: bool,
    /// A relation's class.
    pub class: Option<RelClass>,
    /// A named query that reads runtime or tree-derived state.
    pub live: bool,
}

impl Sig {
    fn relation(r: &catalog::Relation, all: crate::lq::schema::KindSet) -> Sig {
        Sig {
            proc: r.name.to_string(),
            params: r
                .params
                .iter()
                .map(|p| (p.name.to_string(), p.ty, p.required))
                .collect(),
            yields: r
                .yields
                .iter()
                .map(|(n, t)| (n.to_string(), t.ty(all)))
                .collect(),
            named: false,
            class: Some(r.class),
            live: false,
        }
    }

    fn named(q: &catalog::NamedQuery) -> Sig {
        Sig {
            proc: q.qname.clone(),
            params: q.params.clone(),
            yields: q.columns.clone(),
            named: true,
            class: None,
            live: q.live,
        }
    }

    /// The signature as E110's help prints it.
    fn text(&self) -> String {
        let ps: Vec<String> = self
            .params
            .iter()
            .map(|(n, t, req)| format!("{n}: {}{}", pt_name(*t), if *req { "" } else { "?" }))
            .collect();
        format!("{}({})", self.proc, ps.join(", "))
    }
}

/// What a bind has learnt of a project named query ([`Binder::project_query`]).
pub(super) enum Known {
    /// Its definition is being bound: meeting it again is a call cycle.
    Binding,
    /// Its signature.
    Sig(Sig),
    /// Why it is no callee: its definition does not parse or bind, or it is on a call cycle (the E109 text).
    Refused(String),
}

/// The project named queries of one top-level bind, shared by the binds it nests, so each definition is bound once.
pub(super) type NamedMemo = RefCell<Vec<(String, Known)>>;

/// Why a name is no callee.
pub(super) enum Callee {
    /// No relation, standard query or project query has the name.
    Unknown,
    /// A project query that cannot be called: the E109 text.
    Refused(String),
}

/// The name after an ASCII-case-insensitive `std.` prefix ([LQ/lexical §9]), if the name has one and something after
/// it. The name is user text: its first four bytes are compared as bytes, so a multi-byte character there is simply
/// no prefix, and the rest is sliced only after four ASCII bytes matched.
fn strip_std(name: &str) -> Option<&str> {
    let b = name.as_bytes();
    (b.len() > 4 && b[..4].eq_ignore_ascii_case(b"std.")).then(|| &name[4..])
}

fn pt_name(t: PT) -> &'static str {
    match t {
        PT::Node => "node",
        PT::NodeOrEdge => "node or edge",
        PT::Edge => "edge",
        PT::Int => "int",
        PT::Float => "float",
        PT::Bool => "bool",
        PT::Text => "text",
        PT::Rev => "rev",
        PT::Time => "timestamp",
        PT::Dur => "duration",
        PT::Range => "range<int>",
        PT::ListNode => "list<node>",
        PT::ListInt => "list<int>",
        PT::ListText => "list<text>",
        PT::ListRev => "list<rev>",
        PT::Any => "any",
    }
}

impl Binder<'_> {
    /// A project named query as a callee: its signature and columns, from its definition bound against the view's
    /// schema ([LQ/std §2.1]). A top-level bind binds each definition once and shares the result with the binds it
    /// nests. A definition that does not parse or bind, or that is met again while its own bind runs (a call cycle,
    /// [F19 §12.5.4]), is refused with its own text rather than reported as an unknown name.
    fn project_query(&mut self, name: &str) -> Result<Sig, Callee> {
        let Some(def) = self.ctx.schema.queries.iter().find(|q| q.name == name) else {
            return Err(Callee::Unknown);
        };
        {
            let mut memo = self.named.borrow_mut();
            if let Some(i) = memo.iter().position(|(n, _)| n == name) {
                return match &memo[i].1 {
                    Known::Sig(s) => Ok(s.clone()),
                    Known::Refused(why) => Err(Callee::Refused(why.clone())),
                    Known::Binding => {
                        // The definitions still binding from this one on are the cycle, in call order.
                        let mut path: Vec<&str> = memo[i..]
                            .iter()
                            .filter(|(_, k)| matches!(k, Known::Binding))
                            .map(|(n, _)| n.as_str())
                            .collect();
                        path.push(name);
                        let why = format!(
                            "named query {} is on a call cycle: {} (QueryCycle)",
                            q(name),
                            path.join(" -> ")
                        );
                        for (_, k) in memo[i..].iter_mut() {
                            if matches!(k, Known::Binding) {
                                *k = Known::Refused(why.clone());
                            }
                        }
                        Err(Callee::Refused(why))
                    }
                };
            }
            memo.push((name.to_string(), Known::Binding));
        }
        let text = def.text.clone();
        let result = self.bind_project_definition(name, &text);
        let mut memo = self.named.borrow_mut();
        let Some((_, slot)) = memo.iter_mut().find(|(n, _)| n == name) else {
            return result.map_err(Callee::Refused);
        };
        match (&*slot, result) {
            // A cycle through this query refused it while its bind ran; that text stands.
            (Known::Refused(why), _) => Err(Callee::Refused(why.clone())),
            (_, Ok(sig)) => {
                *slot = Known::Sig(sig.clone());
                Ok(sig)
            }
            (_, Err(why)) => {
                *slot = Known::Refused(why.clone());
                Err(Callee::Refused(why))
            }
        }
    }

    /// Parses and binds a project definition (as [LQ/std §2.1] binds a callee: the view's schema, no store ids, no
    /// parameters) and reads its signature, or the E109 text naming its first error ([F19 §12.5.3]).
    fn bind_project_definition(&mut self, name: &str, text: &str) -> Result<Sig, String> {
        let invalid = |e: &[Diag]| {
            let first = e
                .first()
                .map_or_else(String::new, |d| format!(": {} {}", d.code, d.message));
            format!(
                "named query {} does not bind{first} (QueryInvalid)",
                q(name)
            )
        };
        let d = parse_define(text, ParseOptions::default())
            .map_err(|e| invalid(&e))?
            .tree;
        let (ids, params, caller) = (MapIds::new(), Params::new(), Caller::default());
        let ctx = BindCtx {
            schema: self.ctx.schema,
            ids: &ids,
            params: &params,
            caller: &caller,
        };
        let mut b = Binder::new(&ctx, text, self.named);
        let (c, cols) = b.define(&d, Span::new(0, text.len() as u32));
        let bound = b.finish(c, cols).map_err(|e| invalid(&e))?;
        let params = d
            .params
            .iter()
            .map(|pd| {
                let ty = PT::from_decl(
                    &pd.ty.name.text,
                    pd.ty.arg.as_ref().map(|a| a.text.as_str()),
                )
                .unwrap_or(PT::Any);
                (
                    pd.name.text.clone(),
                    ty,
                    !pd.optional && pd.default.is_none(),
                )
            })
            .collect();
        Ok(Sig {
            proc: name.to_string(),
            params,
            yields: bound.columns,
            named: true,
            class: None,
            live: bound.live,
        })
    }

    /// Resolves a `CALL` name ([LQ/std §2.1]): the built-in relations first, then `std`, then the project.
    fn resolve_call(&mut self, name: &str) -> Result<Sig, Callee> {
        let single = !name.contains('.');
        if single {
            if let Some(r) = catalog::relation(name) {
                return Ok(Sig::relation(r, self.all_kinds()));
            }
            if let Some(q) = catalog::std_query(name) {
                return Ok(Sig::named(q));
            }
        }
        if let Some(rest) = strip_std(name) {
            return catalog::std_query(rest)
                .map(Sig::named)
                .ok_or(Callee::Unknown);
        }
        self.project_query(name)
    }

    /// The callee of a call, or its diagnostic (E109) and `None`.
    fn callee(&mut self, name: &str, span: Span) -> Option<Sig> {
        match self.resolve_call(name) {
            Ok(sig) => Some(sig),
            Err(Callee::Unknown) => {
                self.unknown_callee(name, span);
                None
            }
            Err(Callee::Refused(why)) => {
                self.err(Diag::new(Code::E109, span, why));
                None
            }
        }
    }

    /// E109 for a name that is no callee.
    fn unknown_callee(&mut self, name: &str, span: Span) {
        if let Some(m) = catalog::mutation(name) {
            self.err(Diag::new(
                Code::E109,
                span,
                format!(
                    "{} is a named mutation; call it inside TX",
                    q(&format!("tx.{}", m.name))
                ),
            ));
            return;
        }
        let mut names: Vec<String> = catalog::RELATIONS
            .iter()
            .map(|r| r.name.to_string())
            .collect();
        names.extend(
            catalog::std_catalog()
                .iter()
                .map(|q| q.qname.trim_start_matches("std.").to_string()),
        );
        names.extend(self.ctx.schema.queries.iter().map(|q| q.name.clone()));
        let s = near(name, names.iter().map(String::as_str));
        let mut d = Diag::new(Code::E109, span, format!("unknown function {}", q(name)))
            .help("CALL queries() lists the named queries; the built-ins are in reference-ql.md");
        if let Some(first) = s.first() {
            d = d.inline(format!("did you mean {}?", q(first)));
        }
        d.suggest = s.into();
        self.err(d);
    }

    /// The arguments of a call, named and ordered by the signature (N2); omitted arguments stay omitted.
    pub(super) fn call_args(&mut self, sig: &Sig, args: &[Arg], span: Span) -> Vec<CArg> {
        let mut slots: Vec<Option<CArg>> = vec![None; sig.params.len()];
        let mut pos = 0;
        let arity = |sig: &Sig, got: usize| {
            let req = sig.params.iter().filter(|p| p.2).count();
            let n = if req == sig.params.len() {
                req.to_string()
            } else {
                format!("{req} to {}", sig.params.len())
            };
            Diag::new(
                Code::E109,
                span,
                format!("{} takes {n} arguments, got {got}", q(&sig.proc)),
            )
        };
        for a in args {
            let idx = match &a.name {
                Some(n) => match sig.params.iter().position(|p| p.0 == n.text) {
                    Some(i) => i,
                    None => {
                        let d = if sig.named {
                            Diag::new(
                                Code::E110,
                                n.span,
                                format!(
                                    "{} has no parameter {}",
                                    q(&sig.proc),
                                    q(&format!("${}", n.text))
                                ),
                            )
                            .help(sig.text())
                        } else {
                            Diag::new(
                                Code::E109,
                                n.span,
                                format!("{} has no argument {}", q(&sig.proc), q(&n.text)),
                            )
                        };
                        self.err(d);
                        self.arg_value(a, None);
                        continue;
                    }
                },
                None => {
                    let i = pos;
                    pos += 1;
                    if i >= sig.params.len() {
                        let d = arity(sig, args.len());
                        self.err(d);
                        self.arg_value(a, None);
                        continue;
                    }
                    i
                }
            };
            if slots[idx].is_some() {
                let d = arity(sig, args.len());
                self.err(d);
                continue;
            }
            let (pname, pt, _) = &sig.params[idx];
            let want = pt.ty(self.all_kinds());
            let value = self.arg_value(a, Some(&want));
            slots[idx] = Some(CArg {
                name: Some(pname.clone()),
                value,
            });
        }
        for (i, (pname, _, req)) in sig.params.iter().enumerate() {
            if *req && slots[i].is_none() {
                let d = if sig.named {
                    Diag::new(
                        Code::E110,
                        span,
                        format!("{} needs {}", q(&sig.proc), q(&format!("${pname}"))),
                    )
                    .help(sig.text())
                } else {
                    arity(sig, args.len())
                };
                self.err(d);
            }
        }
        slots.into_iter().flatten().collect()
    }

    fn arg_value(&mut self, a: &Arg, want: Option<&Ty>) -> CExpr {
        match &a.value {
            ArgVal::Expr(e) => {
                let (c, t) = self.expr_at(e, want, AggPos::Other);
                if let Some(w) = want {
                    let fits = match (w, &t) {
                        (Ty::Node(_), Ty::Node(_)) | (Ty::Text, Ty::Enum(_) | Ty::KindName) => true,
                        (Ty::List(a), Ty::List(b)) => super::expr::compatible(false, a, b),
                        _ => super::expr::compatible(false, w, &t),
                    };
                    if !fits {
                        self.err(Diag::new(
                            Code::E103,
                            e.span,
                            format!("{} {}: the types do not match", q(&w.name()), q(&t.name())),
                        ));
                    }
                }
                c
            }
            other => self.rev_arg(other),
        }
    }

    /// The class effects of a callee: E302 at a past view for runtime and tree relations, the cursor class, W07's
    /// readiness, and E304 for `across`.
    fn callee_effects(&mut self, sig: &Sig, span: Span, args: &[CArg]) {
        match sig.class {
            Some(RelClass::Runtime) => self.tip_only(
                "leases and markers",
                &format!("{}()", sig.proc),
                span,
                false,
                None,
            ),
            Some(RelClass::Tree) => self.tip_only(
                "the file tree",
                &format!("{}()", sig.proc),
                span,
                true,
                None,
            ),
            Some(RelClass::GraphLive) => self.live = true,
            _ => {}
        }
        if sig.live {
            self.tip_only(
                "runtime or tree-derived state",
                &sig.proc,
                span,
                false,
                None,
            );
        }
        if sig.proc == "blockers" || sig.proc == "std.blockers" {
            self.readiness_used = true;
        }
        if sig.proc == "across" {
            let n = args
                .iter()
                .find(|a| a.name.as_deref() == Some("refs"))
                .map_or(0, |a| match &a.value {
                    CExpr::RList(v) | CExpr::List(v) => v.len() as u32,
                    _ => 0,
                });
            if n > self.ctx.caller.refs {
                self.too_many_refs(span, n);
            }
        }
    }

    /// Yield items as new bindings (V6); a column the callee does not yield is E109.
    pub(super) fn yields(&mut self, sig: Option<&Sig>, items: &[YItem]) -> Vec<CYield> {
        let mut out = Vec::with_capacity(items.len());
        for y in items {
            let ty = match sig {
                Some(sig) => match sig.yields.iter().find(|(n, _)| *n == y.name.text) {
                    Some((_, t)) => t.clone(),
                    None => {
                        let cols: Vec<&str> = sig.yields.iter().map(|(n, _)| n.as_str()).collect();
                        let s = near(&y.name.text, cols.iter().copied());
                        let mut d = Diag::new(
                            Code::E109,
                            y.name.span,
                            format!("{} yields no column {}", q(&sig.proc), q(&y.name.text)),
                        )
                        .help(format!(
                            "{} yields {}",
                            sig.proc,
                            cols.join(", ")
                        ));
                        if let Some(first) = s.first() {
                            d = d.inline(format!("did you mean {}?", q(first)));
                        }
                        d.suggest = s.into();
                        self.err(d);
                        Ty::Any
                    }
                },
                None => Ty::Any,
            };
            let name = y.as_.as_ref().unwrap_or(&y.name).text.clone();
            let kind = if matches!(ty, Ty::Node(_)) {
                BKind::Node
            } else {
                BKind::Value
            };
            let var = self.declare(&name, ty, kind);
            out.push(CYield {
                field: y.name.text.clone(),
                var,
            });
        }
        out
    }

    /// `CALL … YIELD …` as a clause.
    pub(super) fn call_clause(&mut self, c: &Call) -> CClause {
        let sig = self.callee(&c.proc.text, c.proc.span);
        let args = match &sig {
            Some(s) => self.call_args(s, &c.args, c.span),
            None => c
                .args
                .iter()
                .map(|a| CArg {
                    name: a.name.as_ref().map(|n| n.text.clone()),
                    value: self.arg_value(a, None),
                })
                .collect(),
        };
        if let Some(s) = &sig {
            self.callee_effects(s, c.span, &args);
        }
        let items = self.yields(sig.as_ref(), &c.yield_);
        let where_ = c.where_.as_ref().map(|w| self.bool_expr(w, AggPos::Where));
        let proc = sig.map_or_else(|| c.proc.text.clone(), |s| s.proc);
        CClause::Call {
            proc,
            args,
            items,
            where_,
        }
    }

    /// A standalone call with its `YIELD`, `WHERE`, `ORDER BY` and `LIMIT`.
    pub(super) fn scall(&mut self, c: &SCall) -> (CSCall, Vec<(String, Ty)>) {
        let sig = self.callee(&c.proc.text, c.proc.span);
        let args = match &sig {
            Some(s) => self.call_args(s, &c.args, c.span),
            None => c
                .args
                .iter()
                .map(|a| CArg {
                    name: a.name.as_ref().map(|n| n.text.clone()),
                    value: self.arg_value(a, None),
                })
                .collect(),
        };
        if let Some(s) = &sig {
            self.callee_effects(s, c.span, &args);
        }
        let all_cols = sig.as_ref().map_or_else(Vec::new, |s| s.yields.clone());
        let (ymode, items, columns) = match &c.yield_ {
            YieldMode::None => (0, Vec::new(), all_cols),
            YieldMode::Star => {
                for (n, t) in &all_cols {
                    let kind = if matches!(t, Ty::Node(_)) {
                        BKind::Node
                    } else {
                        BKind::Value
                    };
                    self.declare(n, t.clone(), kind);
                }
                (1, Vec::new(), all_cols)
            }
            YieldMode::Items(v) => {
                let items = self.yields(sig.as_ref(), v);
                let cols = items
                    .iter()
                    .map(|y| {
                        (
                            self.b[y.var as usize].name.clone(),
                            self.b[y.var as usize].ty.clone(),
                        )
                    })
                    .collect();
                (2, items, cols)
            }
        };
        let where_ = c.where_.as_ref().map(|w| self.bool_expr(w, AggPos::Where));
        let order = self.sorts(&c.order, AggPos::Other);
        let limit = c.limit.as_ref().map(|l| self.limit(l));
        let proc = sig.map_or_else(|| c.proc.text.clone(), |s| s.proc);
        (
            CSCall {
                proc,
                args,
                ymode,
                items,
                where_,
                order,
                limit,
            },
            columns,
        )
    }

    /// R2: a named query run by name, `std` first, then the project ([LQ/std §2.1]); arguments are `ctx.params`.
    pub(super) fn named_run(&mut self, name: &str) -> (CSCall, Vec<(String, Ty)>) {
        let span = Span::default();
        let sig = match strip_std(name) {
            Some(rest) => catalog::std_query(rest)
                .map(Sig::named)
                .ok_or(Callee::Unknown),
            None => match catalog::std_query(name) {
                Some(q) => Ok(Sig::named(q)),
                None => self.project_query(name),
            },
        };
        let sig = match sig {
            Ok(sig) => sig,
            Err(why) => {
                match why {
                    Callee::Unknown => self.unknown_callee(name, span),
                    Callee::Refused(why) => self.err(Diag::new(Code::E109, span, why)),
                }
                return (
                    CSCall {
                        proc: name.to_string(),
                        args: Vec::new(),
                        ymode: 0,
                        items: Vec::new(),
                        where_: None,
                        order: Vec::new(),
                        limit: None,
                    },
                    Vec::new(),
                );
            }
        };
        let mut slots: Vec<Option<CArg>> = vec![None; sig.params.len()];
        for (k, v) in &self.ctx.params.0 {
            match sig.params.iter().position(|p| p.0 == *k) {
                Some(i) => {
                    let want = sig.params[i].1.ty(self.all_kinds());
                    let (c, _) = self.convert(v, Some(&want), k, span);
                    slots[i] = Some(CArg {
                        name: Some(k.clone()),
                        value: c,
                    });
                }
                None => {
                    let d = Diag::new(
                        Code::E110,
                        span,
                        format!("{} has no parameter {}", q(&sig.proc), q(&format!("${k}"))),
                    )
                    .help(sig.text());
                    self.err(d);
                }
            }
        }
        for (i, (pname, _, req)) in sig.params.iter().enumerate() {
            if *req && slots[i].is_none() {
                let d = Diag::new(
                    Code::E110,
                    span,
                    format!("{} needs {}", q(&sig.proc), q(&format!("${pname}"))),
                )
                .help(sig.text());
                self.err(d);
            }
        }
        let args: Vec<CArg> = slots.into_iter().flatten().collect();
        self.callee_effects(&sig, span, &args);
        let columns = sig.yields.clone();
        (
            CSCall {
                proc: sig.proc,
                args,
                ymode: 0,
                items: Vec::new(),
                where_: None,
                order: Vec::new(),
                limit: None,
            },
            columns,
        )
    }
}
