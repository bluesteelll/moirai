//! The S-expression form of syntax trees ([LQ/canonical-ast §4]): the AST streams that `fixtures/lq` asserts.
//!
//! [`read`], [`tx`] and [`define`] render an S-AST by §4.2 (lower-case tags, the fields of §3.2 in order, `_` for an
//! absent optional field, strings by [LQ/lexical §11.1]). [`tokens`] splits an S-expression into its tokens so that two
//! S-expressions compare equal when their token sequences do (§4.1: whitespace between tokens is ignored).

use crate::lq::ast::*;
use crate::lq::lexer::json_string;

/// Renders a read root: `(read <mode> <query>)`.
pub fn read(r: &Read) -> String {
    let mode = match r.mode {
        Mode::Run => "run",
        Mode::Explain => "explain",
        Mode::Profile => "profile",
    };
    format!("(read {mode} {})", query(&r.query))
}

/// Renders a write root: `(tx on if_tip if_targets key lease message [stmts] dry)`.
pub fn tx(t: &Tx) -> String {
    format!(
        "(tx {} {} {} {} {} {} [{}] {})",
        opt_rev(&t.on),
        opt_rev(&t.if_tip),
        opt_str(&t.if_targets),
        opt_str(&t.key),
        opt_str(&t.lease),
        opt_str(&t.message),
        join(t.stmts.iter().map(stmt)),
        t.dry
    )
}

/// Renders a definition: `(define name [params] shape budget body)`.
pub fn define(d: &Define) -> String {
    format!(
        "(define {} [{}] {} {} {})",
        json_string(&d.name.text),
        join(d.params.iter().map(pdecl)),
        opt_name(&d.shape),
        opt_name(&d.budget),
        query(&d.body)
    )
}

fn join<I: Iterator<Item = String>>(it: I) -> String {
    it.collect::<Vec<_>>().join(" ")
}

fn opt_str(s: &Option<String>) -> String {
    s.as_deref().map_or("_".to_string(), json_string)
}

fn opt_name(n: &Option<Name>) -> String {
    n.as_ref().map_or("_".to_string(), |n| json_string(&n.text))
}

fn opt_expr(e: &Option<Expr>) -> String {
    e.as_ref().map_or("_".to_string(), expr)
}

fn opt_rev(r: &Option<Rev>) -> String {
    r.as_ref().map_or("_".to_string(), rev)
}

/// `(query [parts] [ops])`.
pub fn query(q: &Query) -> String {
    let ops = q.ops.iter().map(|o| {
        match o {
            SetOp::Union => "union",
            SetOp::UnionAll => "union_all",
            SetOp::Except => "except",
            SetOp::Intersect => "intersect",
        }
        .to_string()
    });
    format!(
        "(query [{}] [{}])",
        join(q.parts.iter().map(part)),
        join(ops)
    )
}

fn part(p: &Part) -> String {
    match &p.body {
        PartBody::Clauses { clauses, ret } => {
            format!(
                "(part {} [{}] {} _)",
                opt_rev(&p.use_),
                join(clauses.iter().map(clause)),
                return_(ret)
            )
        }
        PartBody::Call(c) => {
            let (ymode, items) = match &c.yield_ {
                YieldMode::None => ("none", String::new()),
                YieldMode::Star => ("star", String::new()),
                YieldMode::Items(items) => ("items", join(items.iter().map(yitem))),
            };
            format!(
                "(part {} [] _ (scall {} [{}] {ymode} [{items}] {} [{}] {}))",
                opt_rev(&p.use_),
                json_string(&c.proc.text),
                join(c.args.iter().map(arg)),
                opt_expr(&c.where_),
                join(c.order.iter().map(sort)),
                opt_expr(&c.limit)
            )
        }
    }
}

fn clause(c: &Clause) -> String {
    match c {
        Clause::Match(m) => {
            let mode = m.mode.map_or("_", |m| match m {
                MatchMode::Walk => "walk",
                MatchMode::Trail => "trail",
                MatchMode::Acyclic => "acyclic",
                MatchMode::Simple => "simple",
                MatchMode::Different => "different",
            });
            format!(
                "(match {} {mode} [{}] {})",
                m.optional,
                join(m.patterns.iter().map(path)),
                opt_expr(&m.where_)
            )
        }
        Clause::Call(c) => format!(
            "(call {} [{}] [{}] {})",
            json_string(&c.proc.text),
            join(c.args.iter().map(arg)),
            join(c.yield_.iter().map(yitem)),
            opt_expr(&c.where_)
        ),
        Clause::Unwind(u) => format!("(unwind {} {})", expr(&u.expr), json_string(&u.as_.text)),
        Clause::With(w) => format!(
            "(with {} {} [{}] {} [{}] {})",
            w.distinct,
            w.star,
            join(w.items.iter().map(item)),
            opt_expr(&w.where_),
            join(w.order.iter().map(sort)),
            opt_expr(&w.limit)
        ),
    }
}

fn return_(r: &Return) -> String {
    format!(
        "(return {} {} [{}] [{}] [{}] {})",
        r.distinct,
        r.star,
        join(r.items.iter().map(item)),
        join(r.group.iter().map(expr)),
        join(r.order.iter().map(sort)),
        opt_expr(&r.limit)
    )
}

fn item(i: &Item) -> String {
    format!("(item {} {})", expr(&i.expr), opt_name(&i.as_))
}

fn yitem(y: &YItem) -> String {
    format!("(yitem {} {})", json_string(&y.name.text), opt_name(&y.as_))
}

fn sort(s: &Sort) -> String {
    format!(
        "(sort {} {})",
        expr(&s.expr),
        if s.desc { "desc" } else { "asc" }
    )
}

fn arg(a: &Arg) -> String {
    let v = match &a.value {
        ArgVal::Expr(e) => expr(e),
        ArgVal::Rev(r) => rev(r),
        ArgVal::Range { from, op, to, .. } => {
            format!(
                "(rrange {} {} {})",
                rev(from),
                if *op == RangeOp::Two { "two" } else { "three" },
                rev(to)
            )
        }
        ArgVal::List(elems, _) => format!("(rlist [{}])", join(elems.iter().map(rev))),
    };
    format!("(arg {} {v})", opt_name(&a.name))
}

/// A revision node; its suffix chain is walked in a loop.
pub fn rev(r: &Rev) -> String {
    let mut sufs = Vec::new();
    let mut base = r;
    while let RevKind::Suf(b, s) = &base.kind {
        sufs.push(s);
        base = b;
    }
    let mut out = "(rsuf ".repeat(sufs.len());
    match &base.kind {
        RevKind::Head => out.push_str("(rhead)"),
        RevKind::Ref(n) => out.push_str(&format!("(rref {})", json_string(n))),
        RevKind::Commit(h) => out.push_str(&format!("(rcommit {})", json_string(h))),
        RevKind::Seq(n) => out.push_str(&format!("(rseq {n})")),
        RevKind::Param(p) => out.push_str(&format!("(param {})", json_string(p))),
        RevKind::Suf(..) => unreachable!("the loop above walked every suffix"),
    }
    for s in sufs.into_iter().rev() {
        let t = match s {
            Suffix::Tilde(n) => format!(" tilde {n} _)"),
            Suffix::Caret(n) => format!(" caret {n} _)"),
            Suffix::At(n) => format!(" at {n} _)"),
            Suffix::AtTime(t) => format!(" attime _ {})", json_string(t)),
        };
        out.push_str(&t);
    }
    out
}

fn kv(k: &Kv) -> String {
    format!("(kv {} {})", json_string(&k.key.text), expr(&k.value))
}

fn kvs(v: &[Kv]) -> String {
    format!("[{}]", join(v.iter().map(kv)))
}

fn names(v: &[Name]) -> String {
    format!("[{}]", join(v.iter().map(|n| json_string(&n.text))))
}

/// A pattern path.
pub fn path(p: &Path) -> String {
    let steps = p.steps.iter().map(|s| match s {
        Step::Edge(e, n) => format!("(estep {} {})", epat(e), npat(n)),
        Step::Group(g, n) => format!(
            "(gstep (group {} {} {}) {})",
            path(&g.path),
            opt_expr(&g.where_),
            quant(&g.quant),
            npat(n)
        ),
    });
    format!("(path {} [{}])", npat(&p.start), join(steps))
}

fn npat(n: &NPat) -> String {
    format!(
        "(npat {} {} {} {})",
        opt_name(&n.var),
        names(&n.labels),
        kvs(&n.props),
        opt_expr(&n.where_)
    )
}

fn epat(e: &EPat) -> String {
    let dir = match e.dir {
        Dir::Right => "right",
        Dir::Left => "left",
        Dir::Both => "both",
    };
    format!(
        "(epat {} {dir} {} {} {} {})",
        opt_name(&e.var),
        names(&e.types),
        e.quant.as_ref().map_or("_".to_string(), quant),
        kvs(&e.props),
        opt_expr(&e.where_)
    )
}

fn quant(q: &Quant) -> String {
    format!(
        "(quant {} {})",
        q.min,
        q.max.map_or("_".to_string(), |m| m.to_string())
    )
}

fn sub(s: &Sub) -> String {
    match s {
        Sub::Clauses { clauses, ret } => {
            format!(
                "(subq [{}] {})",
                join(clauses.iter().map(clause)),
                ret.as_ref().map_or("_".to_string(), return_)
            )
        }
        Sub::Patterns { patterns, where_ } => format!(
            "(subp [{}] {})",
            join(patterns.iter().map(path)),
            opt_expr(where_)
        ),
    }
}

/// An expression node.
pub fn expr(e: &Expr) -> String {
    let mut out = String::new();
    expr_into(e, &mut out);
    out
}

/// [`expr`] into `out`. The first operand of each operator ([`Expr::first_operand`]), in which chains nest, is walked
/// in a loop ([LQ/grammar-v1.ebnf §P.13]): the heads of the chain's nodes are written top-down, then the innermost
/// node, then the rest of each node bottom-up.
fn expr_into(e: &Expr, out: &mut String) {
    let mut spine = Vec::new();
    let mut cur = e;
    while let Some(first) = cur.first_operand() {
        match &cur.kind {
            ExprKind::Or(..) => out.push_str("(or "),
            ExprKind::And(..) => out.push_str("(and "),
            ExprKind::Not(_) => out.push_str("(not "),
            ExprKind::Cmp(op, ..) => {
                out.push_str("(cmp ");
                out.push_str(op.as_str());
                out.push(' ');
            }
            ExprKind::IsNull(neg, _) => out.push_str(if *neg {
                "(isnull true "
            } else {
                "(isnull false "
            }),
            ExprKind::In(..) => out.push_str("(in "),
            ExprKind::StrPred(op, ..) => out.push_str(match op {
                StrOp::Starts => "(strpred starts ",
                StrOp::Ends => "(strpred ends ",
                StrOp::Contains => "(strpred contains ",
            }),
            ExprKind::LabelTest(..) => out.push_str("(labeltest "),
            ExprKind::Arith(op, ..) => {
                out.push_str("(arith ");
                out.push_str(op.as_str());
                out.push(' ');
            }
            ExprKind::Neg(_) => out.push_str("(neg "),
            ExprKind::Prop(..) => out.push_str("(prop "),
            _ => {}
        }
        spine.push(cur);
        cur = first;
    }
    out.push_str(&primary(cur));
    while let Some(node) = spine.pop() {
        match &node.kind {
            ExprKind::Or(_, r)
            | ExprKind::And(_, r)
            | ExprKind::Cmp(_, _, r)
            | ExprKind::In(_, r)
            | ExprKind::StrPred(_, _, r)
            | ExprKind::Arith(_, _, r) => {
                out.push(' ');
                expr_into(r, out);
            }
            ExprKind::LabelTest(_, labels) => {
                out.push(' ');
                out.push_str(&names(labels));
            }
            ExprKind::Prop(_, n) => {
                out.push(' ');
                out.push_str(&json_string(&n.text));
            }
            _ => {}
        }
        out.push(')');
    }
}

/// A node that is no operator.
fn primary(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Ident(n) => format!("(ident {})", json_string(n)),
        ExprKind::Param(n) => format!("(param {})", json_string(n)),
        ExprKind::Nid(n) => format!("(nid {n})"),
        ExprKind::Uid(h) => format!("(uid {})", json_string(h)),
        ExprKind::Int(n) => format!("(int {n})"),
        ExprKind::Float(t) => format!("(float {})", json_string(t)),
        ExprKind::Str(s) => format!("(str {})", json_string(s)),
        ExprKind::Dur(t) => format!("(dur {})", json_string(t)),
        ExprKind::Bool(b) => format!("(bool {b})"),
        ExprKind::Null => "(null)".into(),
        ExprKind::Exists(s) => format!("(exists {})", sub(s)),
        ExprKind::CountSub(s) => format!("(countsub {})", sub(s)),
        ExprKind::Fn {
            name,
            distinct,
            args,
        } => {
            format!(
                "(fn {} {distinct} [{}])",
                json_string(&name.text),
                join(args.iter().map(arg))
            )
        }
        ExprKind::CountStar => "(countstar)".into(),
        ExprKind::ListPred {
            kind,
            var,
            list,
            pred,
        } => {
            let k = match kind {
                ListPredKind::All => "all",
                ListPredKind::Any => "any",
                ListPredKind::None => "none",
            };
            format!(
                "(listpred {k} {} {} {})",
                json_string(&var.text),
                expr(list),
                expr(pred)
            )
        }
        ExprKind::List(elems) => format!("(list [{}])", join(elems.iter().map(expr))),
        ExprKind::Map(entries) => format!("(map {})", kvs(entries)),
        ExprKind::Case {
            subject,
            whens,
            else_,
        } => format!(
            "(case {} [{}] {})",
            subject.as_deref().map_or("_".to_string(), expr),
            join(
                whens
                    .iter()
                    .map(|w| format!("(when {} {})", expr(&w.cond), expr(&w.then)))
            ),
            else_.as_deref().map_or("_".to_string(), expr)
        ),
        // Operators are rendered by `expr_into` around their first operand.
        ExprKind::Or(..)
        | ExprKind::And(..)
        | ExprKind::Not(_)
        | ExprKind::Cmp(..)
        | ExprKind::IsNull(..)
        | ExprKind::In(..)
        | ExprKind::StrPred(..)
        | ExprKind::LabelTest(..)
        | ExprKind::Arith(..)
        | ExprKind::Neg(_)
        | ExprKind::Prop(..) => expr(e),
    }
}

fn target(t: &Target) -> String {
    match &t.kind {
        TargetKind::Ident(n) => format!("(ident {})", json_string(n)),
        TargetKind::Nid(n) => format!("(nid {n})"),
        TargetKind::Uid(h) => format!("(uid {})", json_string(h)),
        TargetKind::Param(p) => format!("(param {})", json_string(p)),
    }
}

fn opt_target(t: &Option<Target>) -> String {
    t.as_ref().map_or("_".to_string(), target)
}

fn expect(e: &Expect) -> String {
    match e {
        Expect::Exact(n) => format!("(expect exact {n} _ _)"),
        Expect::Range(a, b) => format!("(expect range {a} {b} _)"),
        Expect::Le(n) => format!("(expect le {n} _ _)"),
        Expect::Ge(n) => format!("(expect ge {n} _ _)"),
        Expect::Param(p) => format!("(expect param _ _ {})", json_string(&p.text)),
    }
}

fn edge_dir(d: EdgeDir) -> &'static str {
    match d {
        EdgeDir::Right => "right",
        EdgeDir::Left => "left",
    }
}

fn mutation(m: &Mut) -> String {
    match m {
        Mut::Set(assigns, _) => format!(
            "(mset [{}])",
            join(assigns.iter().map(|a| format!(
                "(assign {} {} {})",
                target(&a.target),
                json_string(&a.prop.text),
                expr(&a.value)
            )))
        ),
        Mut::Remove(items, _) => {
            format!(
                "(mremove [{}])",
                join(items.iter().map(|t| format!(
                    "(tprop {} {})",
                    target(&t.target),
                    json_string(&t.prop.text)
                )))
            )
        }
        Mut::Delete { targets, opts, .. } => {
            let opts = opts.iter().map(|o| match o {
                DOpt::Policy(p) => format!(
                    "(dpolicy {})",
                    match p {
                        Policy::Restrict => "restrict",
                        Policy::Cascade => "cascade",
                        Policy::Reparent => "reparent",
                    }
                ),
                DOpt::Replaced(t) => format!("(dreplaced {})", target(t)),
                DOpt::Release => "(drelease)".to_string(),
                DOpt::Reason(e) => format!("(dreason {})", expr(e)),
            });
            format!(
                "(mdelete [{}] [{}])",
                join(targets.iter().map(target)),
                join(opts)
            )
        }
        Mut::Move {
            target: t,
            under,
            pos,
            ..
        } => {
            let (p, rel) = match pos {
                None => ("_", "_".to_string()),
                Some(MovePos::Before(r)) => ("before", target(r)),
                Some(MovePos::After(r)) => ("after", target(r)),
                Some(MovePos::First) => ("first", "_".to_string()),
                Some(MovePos::Last) => ("last", "_".to_string()),
            };
            format!("(mmove {} {} {p} {rel})", target(t), target(under))
        }
        Mut::Edge(e) => format!(
            "(medge {} {} {} {} {})",
            target(&e.src),
            edge_dir(e.dir),
            json_string(&e.ty.text),
            kvs(&e.props),
            target(&e.dst)
        ),
        Mut::Reopen {
            target: t, reason, ..
        } => format!("(mreopen {} {})", target(t), expr(reason)),
        Mut::Patch {
            target: t,
            field,
            remove,
            add,
            ..
        } => {
            format!(
                "(mpatch {} {} {} {})",
                target(t),
                json_string(&field.text),
                expr(remove),
                expr(add)
            )
        }
    }
}

fn stmt(s: &Stmt) -> String {
    match s {
        Stmt::Match(m) => format!(
            "(smatch [{}] {} {} [{}])",
            join(m.patterns.iter().map(path)),
            opt_expr(&m.where_),
            expect(&m.expect),
            join(m.muts.iter().map(mutation))
        ),
        Stmt::Muts(muts, _) => format!("(smuts [{}])", join(muts.iter().map(mutation))),
        Stmt::Create(c) => {
            let edges = c.edges.iter().map(|e| {
                format!(
                    "(cedge {} {} {} {})",
                    edge_dir(e.dir),
                    json_string(&e.ty.text),
                    kvs(&e.props),
                    target(&e.target)
                )
            });
            format!(
                "(screate {} {} {} [{}] {} {})",
                json_string(&c.var.text),
                json_string(&c.label.text),
                kvs(&c.props),
                join(edges),
                opt_target(&c.under),
                c.unless.as_ref().map_or("_".to_string(), sub)
            )
        }
        Stmt::TxCall {
            name, args, yield_, ..
        } => format!(
            "(stxcall {} [{}] [{}])",
            json_string(&name.text),
            join(args.iter().map(arg)),
            join(yield_.iter().map(yitem))
        ),
        Stmt::Assert { expr: e, else_, .. } => format!("(sassert {} {})", expr(e), opt_str(else_)),
        Stmt::Resolve(r) => {
            let (key, q, e) = match &r.what {
                ResolveWhat::Key(k) => (json_string(k), "_".to_string(), "_".to_string()),
                ResolveWhat::Query(q, e) => ("_".to_string(), query(q), expect(e)),
            };
            let (take, value, tgt) = match &r.take {
                Take::Ours => ("ours", "_".to_string(), "_".to_string()),
                Take::Theirs => ("theirs", "_".to_string(), "_".to_string()),
                Take::Base => ("base", "_".to_string(), "_".to_string()),
                Take::Value(v) => ("value", expr(v), "_".to_string()),
                Take::Repoint(t) => ("repoint", "_".to_string(), target(t)),
            };
            format!("(sresolve {key} {q} {e} {take} {value} {tgt})")
        }
        Stmt::Define(d) => define(d),
        Stmt::Drop(n) => format!("(sdrop {})", json_string(&n.text)),
    }
}

fn pdecl(p: &PDecl) -> String {
    format!(
        "(pdecl {} (ptype {} {}) {} {})",
        json_string(&p.name.text),
        json_string(&p.ty.name.text),
        opt_name(&p.ty.arg),
        p.optional,
        opt_expr(&p.default)
    )
}

/// Splits an S-expression into tokens (`(`, `)`, `[`, `]`, JSON strings, atoms), ignoring whitespace.
pub fn tokens(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'(' | b')' | b'[' | b']' => {
                out.push((b[i] as char).to_string());
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                out.push(s[start..i].to_string());
            }
            _ => {
                let start = i;
                while i < b.len()
                    && !matches!(
                        b[i],
                        b' ' | b'\t' | b'\n' | b'\r' | b'(' | b')' | b'[' | b']' | b'"'
                    )
                {
                    i += 1;
                }
                out.push(s[start..i].to_string());
            }
        }
    }
    out
}

/// Whether two S-expressions are equal by [LQ/canonical-ast §4.1].
pub fn same(a: &str, b: &str) -> bool {
    tokens(a) == tokens(b)
}
