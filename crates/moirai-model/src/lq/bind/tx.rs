//! `TX` blocks ([50 §3.10]; [LQ/canonical-ast §5.8] N3–N6, §5.9 R3–R4): statements, mutations and targets, created
//! nodes and edges in their stored direction, named mutations, `RESOLVE`, definitions (R6, with the portable form of
//! §8.1), and the tx binder's refusals: E115 (not writable), E305 (read-only view, the `plan/*` mask), E406 (the
//! statement rows of [RULES/role-write-policy] `role-statements`) and E411 (the unknown profile, `role-reads` WQ-004).

use super::call::Sig;
use super::expr::PClass;
use super::pattern::effective;
use super::{AggPos, BKind, Binder, DefCtx, View};
use crate::lq::ast::*;
use crate::lq::cast::*;
use crate::lq::catalog::{self, MutClass, PT, Ty};
use crate::lq::ctx::{Profile, Surface, Value};
use crate::lq::diag::{Code, Diag, Span, near, q};
use crate::lq::lexer::parse_decimal;
use crate::lq::printer;
use crate::lq::schema::{FieldClass, KindSet};

/// The branch a `TX` writes, as the tx binder classifies it ([F12 §2.2], [50 §3.9] item 6).
#[derive(Clone, Debug)]
enum Branch {
    /// `main`, `lane/*`.
    Work,
    /// `plan/*`: the I33′ mask ([F13] I33′).
    Plan(String),
    /// `merge/*` or `import/*`: `RESOLVE` only; the `why` of E305.
    Staging(String, &'static str),
    /// A commit, a past view or a tag: the revspec and the `why` of E305.
    ReadOnly(String, &'static str),
}

/// The fields the I33′ mask covers on `plan/*` ([F13] I33′: status, resolution, assignee and claims).
const PLAN_MASK: [&str; 4] = ["status", "resolution", "assignee", "done"];

/// The procedures that claim or settle (the I33′ "claims").
const CLAIMING: [&str; 5] = ["complete", "claim", "heartbeat", "release", "reclaim"];

fn classify(name: &str) -> Branch {
    if name == "main" || name.starts_with("lane/") {
        Branch::Work
    } else if name.starts_with("plan/") {
        Branch::Plan(name.to_string())
    } else if name.starts_with("merge/") {
        Branch::Staging(name.to_string(), "a staging ref accepts only RESOLVE")
    } else if name.starts_with("import/") {
        Branch::Staging(name.to_string(), "an import ref")
    } else if name.starts_with("tags/") {
        Branch::ReadOnly(name.to_string(), "a tag")
    } else {
        Branch::ReadOnly(name.to_string(), "a past view")
    }
}

/// The portable text of a definition ([LQ/canonical-ast §8.1]): the rewrites of step 4 applied over their exact spans
/// (relative to `src`), then steps 2–3 (CR LF and CR to LF; no SP or HT before an LF or at the end). `src` has no
/// byte-order mark (the lexer removed it, step 1).
pub fn portable_text(src: &str, rewrites: &[(Span, String)]) -> String {
    let mut rw: Vec<&(Span, String)> = rewrites.iter().collect();
    rw.sort_by_key(|(s, _)| s.start);
    let mut out = String::with_capacity(src.len() + 64 * rw.len());
    let mut at = 0usize;
    for (s, text) in rw {
        let (a, b) = (s.start as usize, s.end as usize);
        if a < at || b > src.len() {
            continue;
        }
        out.push_str(&src[at..a]);
        out.push_str(text);
        at = b;
    }
    out.push_str(&src[at..]);
    let lf = out.replace("\r\n", "\n").replace('\r', "\n");
    let mut result = String::with_capacity(lf.len());
    for (i, line) in lf.split('\n').enumerate() {
        if i > 0 {
            result.push('\n');
        }
        result.push_str(line.trim_end_matches([' ', '\t']));
    }
    result
}

impl Binder<'_> {
    /// Classifies the branch of `TX ON` (else the caller's branch).
    fn tx_branch(&mut self, on: Option<&Rev>) -> Branch {
        let Some(r) = on else {
            return classify(&self.ctx.caller.branch.clone());
        };
        match &r.kind {
            RevKind::Head => classify(&self.ctx.caller.branch.clone()),
            RevKind::Ref(n) => classify(n),
            RevKind::Param(p) => match self.ctx.params.get(p) {
                Some(Value::Text(t)) if ref_like(t) => classify(t),
                Some(Value::Text(t)) => Branch::ReadOnly(t.clone(), "a commit"),
                _ => classify(&self.ctx.caller.branch.clone()),
            },
            RevKind::Commit(_) | RevKind::Seq(_) => {
                Branch::ReadOnly(self.text(r.span).to_string(), "a commit")
            }
            RevKind::Suf(..) => Branch::ReadOnly(self.text(r.span).to_string(), "a past view"),
        }
    }

    fn read_only(&mut self, span: Span, revspec: &str, why: &str) {
        self.err(
            Diag::new(
                Code::E305,
                span,
                format!("{} is read-only: {why}", q(revspec)),
            )
            .help("write on a branch tip: TX ON <ref> { ... }"),
        );
    }

    /// E305 for a field the `plan/*` mask covers.
    fn masked(&mut self, br: &Branch, field: &str, span: Span) {
        if let Branch::Plan(name) = br {
            let name = name.clone();
            self.read_only(
                span,
                &name,
                &format!("field {} is masked on plan branches", q(field)),
            );
        }
    }

    /// A `TX` block (R3 when `free_form`).
    pub(super) fn tx(&mut self, t: &Tx, free_form: bool) -> CTx {
        let br = self.tx_branch(t.on.as_ref());
        if let (Branch::ReadOnly(spec, why), Some(on)) = (&br, &t.on) {
            let (spec, why, span) = (spec.clone(), *why, on.span);
            self.read_only(span, &spec, why);
        } else if let Branch::ReadOnly(spec, why) = &br {
            let (spec, why) = (spec.clone(), *why);
            self.read_only(t.span, &spec, why);
        }
        if free_form {
            self.unknown_profile(t);
        }
        let if_tip = t.if_tip.as_ref().map(|r| self.rev(r));
        let mut stmts = Vec::with_capacity(t.stmts.len());
        for (i, s) in t.stmts.iter().enumerate() {
            if let Branch::Staging(name, why) = &br
                && !matches!(s, Stmt::Resolve(_))
            {
                let (name, why) = (name.clone(), *why);
                self.read_only(stmt_span(s), &name, why);
            }
            let c = self.stmt(s, &br);
            self.statement_policy(i + 1, s);
            stmts.push(c);
        }
        CTx {
            if_tip,
            if_targets: t.if_targets.clone(),
            message: t.message.clone(),
            stmts,
        }
    }

    /// R4: `TX { CALL tx.<name>(k: v, ...) }` with `ctx.params`, joined with the tool's options.
    pub(super) fn named_mutation(&mut self, name: &str, options: &Tx) -> CTx {
        let br = self.tx_branch(options.on.as_ref());
        if let Branch::ReadOnly(spec, why) = &br {
            let (spec, why) = (spec.clone(), *why);
            self.read_only(Span::default(), &spec, why);
        }
        let if_tip = options.if_tip.as_ref().map(|r| self.rev(r));
        let Some(m) = catalog::mutation(name) else {
            self.unknown_mutation(name, Span::default());
            return CTx {
                if_tip,
                if_targets: options.if_targets.clone(),
                message: options.message.clone(),
                stmts: Vec::new(),
            };
        };
        if CLAIMING.contains(&m.name) {
            self.masked(&br, "claims", Span::default());
        }
        let sig = mutation_sig(m, self.all_kinds());
        let mut slots: Vec<Option<CArg>> = vec![None; sig.params.len()];
        for (k, v) in &self.ctx.params.0 {
            match sig.params.iter().position(|p| p.0 == *k) {
                Some(i) => {
                    let want = sig.params[i].1.ty(self.all_kinds());
                    let (c, _) = self.convert(v, Some(&want), k, Span::default());
                    slots[i] = Some(CArg {
                        name: Some(k.clone()),
                        value: c,
                    });
                }
                None => {
                    let d = Diag::new(
                        Code::E110,
                        Span::default(),
                        format!("{} has no parameter {}", q(&sig.proc), q(&format!("${k}"))),
                    );
                    self.err(d);
                }
            }
        }
        for (i, (pname, _, req)) in sig.params.iter().enumerate() {
            if *req && slots[i].is_none() {
                self.err(Diag::new(
                    Code::E110,
                    Span::default(),
                    format!("{} needs {}", q(&sig.proc), q(&format!("${pname}"))),
                ));
            }
        }
        let stmt = CStmt::TxCall {
            name: sig.proc.clone(),
            args: slots.into_iter().flatten().collect(),
            items: Vec::new(),
        };
        self.call_policy(1, m.name);
        CTx {
            if_tip,
            if_targets: options.if_targets.clone(),
            message: options.message.clone(),
            stmts: vec![stmt],
        }
    }

    // ----- refusals of the tx binder --------------------------------------------------------------------------------

    /// E411 ([RULES/role-write-policy] WQ-004, WQ-005): a free-form `TX` under a profile whose write rule refuses it
    /// (the binder's `unknown`); the text names the session's own profile ([LQ/errors §5.5], spec sync 2b).
    fn unknown_profile(&mut self, t: &Tx) {
        let c = self.ctx.caller;
        if c.profile != Profile::Unknown
            || (c.unknown_dry_targets && (t.dry || t.if_targets.is_some()))
        {
            return;
        }
        let name = match c.write_profile {
            Profile::Gated => "gated",
            Profile::Compatible => "compatible",
            Profile::Unknown => "unknown",
        };
        let mut d = Diag::unlocated(
            Code::E411,
            format!("free-form TX is refused for a model with the {name} profile"),
        );
        d = d.detail(match matching_mutation(t) {
            Some(n) => format!("use the named mutation tx.{n} (the write tool: name and params)"),
            None => "no named mutation matches; ask the orchestrator".to_string(),
        });
        if c.unknown_dry_targets {
            d = d.detail("or run the block with DRY and apply it with IF TARGETS");
        }
        self.err(d);
    }

    /// E406 for the statement classes of [RULES/role-write-policy] `role-statements` (WX-001–WX-016).
    fn statement_policy(&mut self, i: usize, s: &Stmt) {
        let mut classes: Vec<(&str, &str, &str)> = Vec::new();
        match s {
            Stmt::Define(_) => {
                classes.push(("define-query", "define a named query", "DEFINE QUERY"))
            }
            Stmt::Drop(_) => classes.push(("drop-query", "drop a named query", "DROP QUERY")),
            Stmt::Resolve(_) => classes.push(("resolve", "resolve a conflict", "RESOLVE")),
            Stmt::TxCall { name, .. } => {
                self.call_policy(i, &name.text);
                return;
            }
            _ => {}
        }
        let muts: &[Mut] = match s {
            Stmt::Match(m) => &m.muts,
            Stmt::Muts(v, _) => v,
            _ => &[],
        };
        for m in muts {
            match m {
                Mut::Delete { targets, .. } if targets.iter().any(|t| self.is_node_target(t)) => {
                    classes.push(("node-delete", "delete a node", "node DELETE"));
                }
                Mut::Reopen { .. } => classes.push(("reopen", "reopen", "REOPEN")),
                _ => {}
            }
        }
        if let Stmt::Match(m) = s
            && self.bulk(&m.expect)
        {
            classes.push(("bulk-target", "write a bulk target", "bulk MATCH"));
        }
        for (class, action, stmt) in classes {
            self.refuse_unless(i, class, action, stmt);
        }
    }

    fn call_policy(&mut self, i: usize, name: &str) {
        match name {
            "complete" => self.refuse_unless(
                i,
                "call-tx-complete",
                "call tx.complete",
                "CALL tx.complete",
            ),
            "reclaim" => {
                self.refuse_unless(i, "call-tx-reclaim", "call tx.reclaim", "CALL tx.reclaim")
            }
            _ => {}
        }
    }

    fn refuse_unless(&mut self, i: usize, class: &str, action: &str, stmt: &str) {
        let role = self.ctx.caller.role.clone();
        let cli_only = matches!(
            class,
            "node-delete" | "resolve" | "define-query" | "drop-query"
        );
        if cli_only && self.surface() == Surface::Mcp {
            self.err(Diag::unlocated(
                Code::E406,
                format!(
                    "{} runs only through moirai tx, for the orchestrator and the owner",
                    q(stmt)
                ),
            ));
            return;
        }
        let allowed: &[&str] = if class == "call-tx-complete" {
            &["developer", "tester", "orchestrator", "owner"]
        } else {
            &["orchestrator", "owner"]
        };
        if !allowed.contains(&role.as_str()) {
            let key = if class.ends_with("-query") {
                "define-query"
            } else {
                "tx"
            };
            self.err(
                Diag::unlocated(
                    Code::E406,
                    format!("statement {i}: role {} may not {action}", q(&role)),
                )
                .detail("nothing was written")
                .help(format!(
                    "{stmt} is for the roles {} (policy.role.<role>.{key})",
                    allowed.join(", ")
                )),
            );
        }
    }

    fn is_node_target(&self, t: &Target) -> bool {
        match &t.kind {
            TargetKind::Nid(_) | TargetKind::Uid(_) | TargetKind::Param(_) => true,
            TargetKind::Ident(v) => self
                .lookup(v)
                .is_some_and(|id| self.b[id as usize].kind != BKind::Edge),
        }
    }

    /// A bulk target set ([50 §3.10] item 3): `EXPECT` allows more than 10 bindings or has no upper bound.
    fn bulk(&self, e: &Expect) -> bool {
        match e {
            Expect::Exact(n) | Expect::Le(n) => *n > 10,
            Expect::Range(_, b) => *b > 10,
            Expect::Ge(_) => true,
            Expect::Param(p) => match self.ctx.params.get(&p.text) {
                Some(Value::Int(n)) => *n > 10,
                Some(Value::Text(t)) => t
                    .split_once("..")
                    .is_none_or(|(_, hi)| hi.is_empty() || hi.parse::<i64>().is_ok_and(|h| h > 10)),
                _ => false,
            },
        }
    }

    // ----- statements -----------------------------------------------------------------------------------------------

    fn stmt(&mut self, s: &Stmt, br: &Branch) -> CStmt {
        match s {
            Stmt::Match(m) => {
                let (patterns, where_) = self.match_patterns(&m.patterns, m.where_.as_ref(), false);
                let expect = self.expect(&m.expect, m.span);
                let muts = m.muts.iter().map(|x| self.mutation(x, br)).collect();
                CStmt::Match {
                    patterns,
                    where_,
                    expect,
                    muts,
                }
            }
            Stmt::Muts(v, _) => CStmt::Muts(v.iter().map(|x| self.mutation(x, br)).collect()),
            Stmt::Create(c) => self.create(c),
            Stmt::TxCall {
                name,
                args,
                yield_,
                span,
            } => self.tx_call(name, args, yield_, *span, br),
            Stmt::Assert { expr, else_, .. } => {
                let saved = self.agg;
                self.agg = AggPos::Other;
                let e = self.operand_bool(expr, "ASSERT");
                self.agg = saved;
                CStmt::Assert {
                    expr: e,
                    else_: else_.clone(),
                }
            }
            Stmt::Resolve(r) => self.resolve(r),
            Stmt::Define(d) => CStmt::Define(self.define(d, d.span).0),
            Stmt::Drop(n) => CStmt::Drop(n.text.clone()),
        }
    }

    /// `EXPECT` as (min, max) (N5).
    fn expect(&mut self, e: &Expect, span: Span) -> (u64, Option<u64>) {
        let u = |n: i64| n.max(0) as u64;
        match e {
            Expect::Exact(n) => (u(*n), Some(u(*n))),
            Expect::Range(a, b) => (u(*a), Some(u(*b))),
            Expect::Le(n) => (0, Some(u(*n))),
            Expect::Ge(n) => (u(*n), None),
            Expect::Param(p) => {
                let bad = |b: &mut Self, v: &Value| {
                    let shown = match v {
                        Value::Text(t) => t.clone(),
                        Value::Int(n) => n.to_string(),
                        _ => "a value".to_string(),
                    };
                    b.err(Diag::new(
                        Code::E110,
                        p.span,
                        format!(
                            "{} must be int or range<int>; got {}",
                            q(&format!("${}", p.text)),
                            q(&shown)
                        ),
                    ));
                    (0, None)
                };
                match self.ctx.params.get(&p.text).cloned() {
                    Some(Value::Int(n)) if n >= 0 => (u(n), Some(u(n))),
                    Some(Value::Text(t)) => {
                        let part = |s: &str| {
                            if s.is_empty() {
                                Some(None)
                            } else {
                                parse_decimal(s).map(Some)
                            }
                        };
                        match t
                            .split_once("..")
                            .map_or_else(|| part(&t).map(|a| (a, a)), |(a, b)| part(a).zip(part(b)))
                        {
                            // N5: a bound with min > max, through `$p`, is E001 at binding (spec sync 2b).
                            Some((Some(lo), Some(hi))) if lo > hi => {
                                self.err(Diag::new(
                                    Code::E001,
                                    p.span,
                                    format!("EXPECT {lo}..{hi} has m > n"),
                                ));
                                (0, None)
                            }
                            Some((lo, hi)) if lo.is_some() || hi.is_some() => (lo.unwrap_or(0), hi),
                            _ => bad(self, &Value::Text(t)),
                        }
                    }
                    Some(v) => bad(self, &v),
                    None => {
                        self.err(Diag::new(
                            Code::E110,
                            span,
                            format!("{} needs {}", q("the query"), q(&format!("${}", p.text))),
                        ));
                        (0, None)
                    }
                }
            }
        }
    }

    /// A target: a variable, a node literal or a parameter (a `VAR` or a `NODE`).
    fn target(&mut self, t: &Target) -> (CExpr, Ty, String) {
        match &t.kind {
            TargetKind::Ident(v) => match self.lookup(v) {
                Some(id) => (CExpr::Var(id), self.b[id as usize].ty.clone(), v.clone()),
                None => {
                    let (c, ty) = self.unbound_target(v, t.span);
                    (c, ty, v.clone())
                }
            },
            TargetKind::Nid(n) => {
                let (c, ty) = self.node_num(*n, t.span, true);
                (c, ty, format!("#{n}"))
            }
            TargetKind::Uid(h) => {
                let (c, ty) = self.node_uid(h, t.span);
                (c, ty, format!("#u:{h}"))
            }
            TargetKind::Param(p) => {
                let (c, ty) = self.param(p, t.span, Some(&Ty::Node(self.all_kinds())));
                (c, ty, format!("${p}"))
            }
        }
    }

    fn unbound_target(&mut self, v: &str, span: Span) -> (CExpr, Ty) {
        self.coerce_word(v, span, None)
    }

    fn node_target(&mut self, t: &Target) -> CExpr {
        let (c, ty, shown) = self.target(t);
        if !matches!(ty, Ty::Node(_) | Ty::Any | Ty::Null) {
            self.err(Diag::new(
                Code::E103,
                t.span,
                format!("{} is {}, not a node", q(&shown), q(&ty.name())),
            ));
        }
        c
    }

    fn mutation(&mut self, m: &Mut, br: &Branch) -> CMut {
        match m {
            Mut::Set(assigns, _) => {
                let mut out = Vec::with_capacity(assigns.len());
                for a in assigns {
                    let (tc, tty, owner) = self.target(&a.target);
                    self.masked_field(br, &a.prop);
                    let v = self.write_field(&tty, &owner, &a.prop, Some(&a.value));
                    out.push((tc, a.prop.text.clone(), v.unwrap_or(CExpr::Null)));
                }
                CMut::Set(out)
            }
            Mut::Remove(items, _) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    let (tc, tty, owner) = self.target(&it.target);
                    self.masked_field(br, &it.prop);
                    self.write_field(&tty, &owner, &it.prop, None);
                    out.push((tc, it.prop.text.clone()));
                }
                CMut::Remove(out)
            }
            Mut::Delete { targets, opts, .. } => {
                let ts = targets.iter().map(|t| self.target(t).0).collect();
                let (mut policy, mut replaced_by, mut release, mut reason) = (0, None, false, None);
                for o in opts {
                    match o {
                        DOpt::Policy(p) => {
                            policy = match p {
                                Policy::Restrict => 1,
                                Policy::Cascade => 2,
                                Policy::Reparent => 3,
                                Policy::Reassign => 4,
                            }
                        }
                        DOpt::Replaced(t) => replaced_by = Some(self.node_target(t)),
                        DOpt::Release => release = true,
                        DOpt::Reason(e) => {
                            reason = Some(self.expr_at(e, Some(&Ty::Text), AggPos::Other).0)
                        }
                    }
                }
                CMut::Delete {
                    targets: ts,
                    policy,
                    replaced_by,
                    release,
                    reason,
                }
            }
            Mut::Move {
                target, under, pos, ..
            } => {
                let t = self.node_target(target);
                let u = self.node_target(under);
                let (pos, rel) = match pos {
                    None => (0, None),
                    Some(MovePos::Before(r)) => (1, Some(self.node_target(r))),
                    Some(MovePos::After(r)) => (2, Some(self.node_target(r))),
                    Some(MovePos::First) => (3, None),
                    Some(MovePos::Last) => (4, None),
                };
                CMut::Move {
                    target: t,
                    under: u,
                    pos,
                    rel,
                }
            }
            Mut::Edge(me) => self.medge(me),
            Mut::Reopen {
                target,
                reason,
                span,
            } => {
                self.masked(br, "status", *span);
                let t = self.node_target(target);
                let r = self.expr_at(reason, Some(&Ty::Text), AggPos::Other).0;
                CMut::Reopen(t, r)
            }
            Mut::Patch {
                target,
                field,
                remove,
                add,
                ..
            } => {
                let (tc, tty, owner) = self.target(target);
                self.write_field(&tty, &owner, field, None);
                if let Ty::Node(ks) = tty
                    && let Some((t, _)) = self.node_prop_info(ks, &field.text)
                    && !matches!(t, Ty::Text | Ty::Any)
                {
                    self.err(Diag::new(
                        Code::E103,
                        field.span,
                        format!("PATCH {}: the types do not match", q(&t.name())),
                    ));
                }
                let r = self.expr_at(remove, Some(&Ty::Text), AggPos::Other).0;
                let a = self.expr_at(add, Some(&Ty::Text), AggPos::Other).0;
                CMut::Patch(tc, field.text.clone(), r, a)
            }
        }
    }

    fn masked_field(&mut self, br: &Branch, prop: &Name) {
        if PLAN_MASK.contains(&prop.text.as_str()) {
            self.masked(br, &prop.text, prop.span);
        }
    }

    /// Checks a `SET` (with `value`), `REMOVE` or `PATCH` of `prop` on a target of type `tty` ([50 §3.2] "Writes"):
    /// E101, E115 by class, E103 for a value of the wrong type or a counter written other than by an increment.
    fn write_field(
        &mut self,
        tty: &Ty,
        owner: &str,
        prop: &Name,
        value: Option<&Expr>,
    ) -> Option<CExpr> {
        let span = prop.span;
        let target = format!("{owner}.{}", prop.text);
        match tty {
            Ty::Edge(_) => {
                self.not_writable(
                    span,
                    &target,
                    "an edge property",
                    Some("moirai links fix ID --repin or --pin"),
                );
                value.map(|v| self.expr_at(v, None, AggPos::Other).0)
            }
            Ty::Node(ks) => {
                let (pty, info) = self.prop(tty, &prop.text, span, owner, false);
                if let Some((class, help)) = self.unwritable(*ks, &prop.text, info.class) {
                    self.not_writable(span, &target, class, help);
                }
                let v = value?;
                let (c, t) = self.expr_at(v, Some(&pty), AggPos::Other);
                self.check_assignable(&pty, &t, &target, v.span);
                if info.counter && !is_increment(v, owner, &prop.text) {
                    self.err(
                        Diag::new(
                            Code::E103,
                            v.span,
                            format!("counter {} changes only by an increment", q(&prop.text)),
                        )
                        .inline(format!(
                            "write SET {o}.{f} = {o}.{f} + <k>",
                            o = printer::var(owner),
                            f = printer::plain(&prop.text)
                        )),
                    );
                }
                let artifact = self
                    .ctx
                    .schema
                    .kind("artifact")
                    .map_or(KindSet::EMPTY, KindSet::one);
                if prop.text == "status"
                    && ks.live().subset_of(artifact)
                    && matches!(&c, CExpr::Enum(name, _) if name == "removed")
                {
                    self.not_writable(
                        v.span,
                        "status `removed`",
                        "a file operation",
                        Some("moirai file mv, moirai file rm or moirai links fix"),
                    );
                }
                Some(c)
            }
            Ty::Any | Ty::Null => value.map(|v| self.expr_at(v, None, AggPos::Other).0),
            other => {
                self.err(Diag::new(
                    Code::E103,
                    span,
                    format!("{} is {}, not a node", q(owner), q(&other.name())),
                ));
                value.map(|v| self.expr_at(v, None, AggPos::Other).0)
            }
        }
    }

    /// The E115 class of a property a write may not touch, with the help that names the verb ([LQ/errors §5.3]).
    fn unwritable(
        &self,
        ks: KindSet,
        name: &str,
        class: PClass,
    ) -> Option<(&'static str, Option<&'static str>)> {
        const DERIVED: &str = "it follows from the graph; write the fields it is derived from";
        const FILES: &str = "moirai file mv, moirai file rm or moirai links fix";
        let s = self.ctx.schema;
        let only = |k: &str| s.kind(k).is_some_and(|k| ks.live() == KindSet::one(k));
        match class {
            PClass::Derived if name != "done" => Some(("derived", Some(DERIVED))),
            PClass::Runtime => Some(("runtime", Some(DERIVED))),
            PClass::Tree => Some(("tree-derived", Some(DERIVED))),
            PClass::Identity | PClass::Tombstone => Some(("an identity field", None)),
            PClass::Field(FieldClass::Observation) => Some(("an observation field", Some(FILES))),
            PClass::Field(FieldClass::Identity) if only("area") => {
                Some(("a root-node field", Some(FILES)))
            }
            PClass::Field(FieldClass::Identity) => Some(("an identity field", Some(FILES))),
            PClass::Field(FieldClass::PathMoveSet) => Some(("a root-node field", Some(FILES))),
            PClass::Field(_) if name == "title" && only("artifact") => {
                Some(("derived", Some(FILES)))
            }
            _ => None,
        }
    }

    fn not_writable(&mut self, span: Span, target: &str, class: &str, help: Option<&str>) {
        let mut d = Diag::new(
            Code::E115,
            span,
            format!("{} is {class} and cannot be written here", q(target)),
        );
        if let Some(h) = help {
            d = d.help(h);
        }
        self.err(d);
    }

    /// `CREATE (a)-[:T]->(b)`: `MEDGE` in the stored direction (N6, §5.4).
    fn medge(&mut self, me: &MEdge) -> CMut {
        let types = self.edge_types(std::slice::from_ref(&me.ty));
        let (src, sty, sname) = self.target(&me.src);
        let (dst, dty, dname) = self.target(&me.dst);
        let Some(&[(k, fwd)]) = types.as_deref() else {
            return CMut::Edge(src, me.ty.text.clone(), Vec::new(), dst);
        };
        let d = &self.ctx.schema.edges[k];
        let lq = d.lq.clone();
        if d.anchor {
            self.not_writable(
                me.ty.span,
                "an AT edge",
                "created by capture",
                Some("moirai link ID --at SPEC (the write tool: tx.link_file)"),
            );
        }
        let dir = if me.dir == EdgeDir::Right {
            Dir::Right
        } else {
            Dir::Left
        };
        let forward = effective(dir, fwd, false) == 1;
        self.created_endpoints(k, forward, (&sty, &sname), (&dty, &dname), me.span);
        let props = self.edge_props(k, &me.props);
        if forward {
            CMut::Edge(src, lq, props, dst)
        } else {
            CMut::Edge(dst, lq, props, src)
        }
    }

    /// E106 for a created edge whose endpoint kinds the kind does not admit.
    fn created_endpoints(
        &mut self,
        k: usize,
        forward: bool,
        (lt, ln): (&Ty, &str),
        (rt, rn): (&Ty, &str),
        span: Span,
    ) {
        let (Ty::Node(lk), Ty::Node(rk)) = (lt, rt) else {
            return;
        };
        let s = self.ctx.schema;
        let d = &s.edges[k];
        let (src, dst) = (s.ends(&d.src), s.ends(&d.dst));
        let (sk, dk, sn, dn) = if forward {
            (*lk, *rk, ln, rn)
        } else {
            (*rk, *lk, rn, ln)
        };
        if sk.and(src).is_empty() || dk.and(dst).is_empty() {
            let msg = format!(
                "{} links {} -> {}; {} and {} are {} and {}",
                q(&d.lq),
                s.kinds_text(src),
                s.kinds_text(dst),
                q(sn),
                q(dn),
                s.kinds_text(sk),
                s.kinds_text(dk)
            );
            self.err(Diag::new(Code::E106, span, msg));
        }
    }

    fn edge_props(&mut self, k: usize, props: &[Kv]) -> Vec<(String, CExpr)> {
        let base = Ty::Edge(KindSet::one(k));
        props
            .iter()
            .map(|kv| {
                let (pty, _) = self.prop(&base, &kv.key.text, kv.key.span, "", false);
                let (c, t) = self.expr_at(&kv.value, Some(&pty), AggPos::Other);
                self.check_assignable(&pty, &t, &kv.key.text, kv.value.span);
                (kv.key.text.clone(), c)
            })
            .collect()
    }

    /// `CREATE (x:kind {…}) [-[:T]->(y)…] [UNDER p] [UNLESS EXISTS {…}]` (V8, V10, N6).
    fn create(&mut self, c: &Create) -> CStmt {
        let (kinds, names) = self.labels(std::slice::from_ref(&c.label));
        let deleted = c.label.text.eq_ignore_ascii_case("deleted");
        if deleted {
            self.err(Diag::new(
                Code::E105,
                c.label.span,
                format!("unknown kind {}", q(&c.label.text)),
            ));
        }
        let ks = kinds.filter(|_| !deleted).unwrap_or(self.all_kinds());
        let kind = names
            .first()
            .cloned()
            .unwrap_or_else(|| c.label.text.clone());
        if kind == "artifact" {
            self.not_writable(
                c.label.span,
                "CREATE (x:artifact ...)",
                "created by capture",
                Some("moirai file add PATH"),
            );
        }
        let var = self.declare(&c.var.text, Ty::Node(ks), BKind::Node);
        let base = Ty::Node(ks);
        let mut props = Vec::with_capacity(c.props.len());
        for kv in &c.props {
            let (pty, info) = self.prop(&base, &kv.key.text, kv.key.span, &c.var.text, false);
            if let Some((class, help)) = self.unwritable(ks, &kv.key.text, info.class) {
                self.not_writable(
                    kv.key.span,
                    &format!("{}.{}", c.var.text, kv.key.text),
                    class,
                    help,
                );
            }
            let (v, t) = self.expr_at(&kv.value, Some(&pty), AggPos::Other);
            self.check_assignable(&pty, &t, &kv.key.text, kv.value.span);
            props.push((kv.key.text.clone(), v));
        }
        let mut edges = Vec::with_capacity(c.edges.len());
        for e in &c.edges {
            let types = self.edge_types(std::slice::from_ref(&e.ty));
            let (tc, tty, tname) = self.target(&e.target);
            match types.as_deref() {
                Some(&[(k, fwd)]) => {
                    let d = &self.ctx.schema.edges[k];
                    let lq = d.lq.clone();
                    if d.anchor {
                        self.not_writable(
                            e.ty.span,
                            "an AT edge",
                            "created by capture",
                            Some("moirai link ID --at SPEC (the write tool: tx.link_file)"),
                        );
                    }
                    let dir = if e.dir == EdgeDir::Right {
                        Dir::Right
                    } else {
                        Dir::Left
                    };
                    let out = effective(dir, fwd, false) == 1;
                    self.created_endpoints(
                        k,
                        out,
                        (&Ty::Node(ks), &c.var.text),
                        (&tty, &tname),
                        e.ty.span,
                    );
                    let p = self.edge_props(k, &e.props);
                    edges.push((if out { 1 } else { 2 }, lq, p, tc));
                }
                _ => edges.push((1, e.ty.text.clone(), Vec::new(), tc)),
            }
        }
        let under = c.under.as_ref().map(|t| self.node_target(t));
        let unless = c.unless.as_ref().map(|s| self.sub(s));
        CStmt::Create {
            var,
            kind,
            props,
            edges,
            under,
            unless,
        }
    }

    fn unknown_mutation(&mut self, name: &str, span: Span) {
        let names: Vec<&str> = catalog::MUTATIONS.iter().map(|m| m.name).collect();
        let s = near(name, names.iter().copied());
        let mut d = Diag::new(
            Code::E109,
            span,
            format!("unknown function {}", q(&format!("tx.{name}"))),
        )
        .help("CALL queries() lists the named queries; the built-ins are in reference-ql.md");
        if let Some(first) = s.first() {
            d = d.inline(format!("did you mean {}?", q(&format!("tx.{first}"))));
        }
        d.suggest = s.into();
        self.err(d);
    }

    /// `CALL tx.<name>(…) [YIELD …]`.
    fn tx_call(
        &mut self,
        name: &Name,
        args: &[Arg],
        yield_: &[YItem],
        span: Span,
        br: &Branch,
    ) -> CStmt {
        let Some(m) = catalog::mutation(&name.text) else {
            self.unknown_mutation(&name.text, name.span);
            return CStmt::TxCall {
                name: format!("tx.{}", name.text),
                args: Vec::new(),
                items: Vec::new(),
            };
        };
        if m.class == MutClass::File {
            self.not_writable(
                name.span,
                &format!("tx.{}", m.name),
                "a file operation",
                Some("send it alone: the write tool with name and params, or its CLI verb"),
            );
        }
        if CLAIMING.contains(&m.name) {
            self.masked(br, "claims", name.span);
        }
        let sig = mutation_sig(m, self.all_kinds());
        let cargs = self.call_args(&sig, args, span);
        let items = self.yields(Some(&sig), yield_);
        CStmt::TxCall {
            name: sig.proc,
            args: cargs,
            items,
        }
    }

    /// `RESOLVE`: a key or a query with its `EXPECT` (the query is a scope of its own, V1).
    fn resolve(&mut self, r: &Resolve) -> CStmt {
        let key = match &r.what {
            ResolveWhat::Key(k) => CResolveKey::Text(k.clone()),
            ResolveWhat::Query(qy, e) => {
                let (cq, _) = self.query(qy);
                let e = self.expect(e, r.span);
                CResolveKey::Query(Box::new(cq), e)
            }
        };
        let (take, operand) = match &r.take {
            Take::Ours => (1, None),
            Take::Theirs => (2, None),
            Take::Base => (3, None),
            Take::Value(e) => (4, Some(self.expr_at(e, None, AggPos::Other).0)),
            Take::Repoint(t) => (5, Some(self.node_target(t))),
            Take::Drop => (6, None),
        };
        CStmt::Resolve { key, take, operand }
    }

    // ----- definitions ----------------------------------------------------------------------------------------------

    /// A definition (R6; also a statement of a `TX`): parameters with their declared types and constant defaults, the
    /// body with `PARAM(i)` for parameters, and the portable text of §8.1 over `span`.
    pub(super) fn define(&mut self, d: &Define, span: Span) -> (CDefine, Vec<(String, Ty)>) {
        let saved_def = self.def.take();
        let saved_view = std::mem::replace(&mut self.view, View::Tip);
        let mark = self.rewrites.len();
        let mut decls: Vec<(String, Ty)> = Vec::with_capacity(d.params.len());
        let mut pts = Vec::with_capacity(d.params.len());
        for p in &d.params {
            let pt = PT::from_decl(&p.ty.name.text, p.ty.arg.as_ref().map(|a| a.text.as_str()));
            if pt.is_none() {
                let written = match &p.ty.arg {
                    Some(a) => format!("{}<{}>", p.ty.name.text, a.text),
                    None => p.ty.name.text.clone(),
                };
                self.err(Diag::new(
                    Code::E110,
                    p.ty.name.span,
                    format!(
                        "{} must be node, int, float, bool, text, rev, timestamp, duration, range<int>, list<node>, list<int>, list<text> or list<rev>; got {}",
                        q(&format!("${}", p.name.text)),
                        q(&written)
                    ),
                ));
            }
            pts.push(pt);
            decls.push((
                p.name.text.clone(),
                pt.map_or(Ty::Any, |t| t.ty(self.all_kinds())),
            ));
        }
        self.def = Some(DefCtx {
            name: d.name.text.clone(),
            params: decls.clone(),
        });
        let mut params = Vec::with_capacity(d.params.len());
        for (p, (_, ty)) in d.params.iter().zip(&decls) {
            let default = p.default.as_ref().map(|e| {
                let (c, t) = self.expr_at(e, Some(ty), AggPos::Other);
                let fits = match (ty, &t) {
                    (Ty::List(a), Ty::List(b)) => super::expr::compatible(false, a, b),
                    _ => super::expr::compatible(false, ty, &t),
                };
                if !fits {
                    self.err(Diag::new(
                        Code::E110,
                        e.span,
                        format!(
                            "{} must be {}; got {}",
                            q(&format!("${}", p.name.text)),
                            ty.name(),
                            q(&printer::expr_text(e))
                        ),
                    ));
                }
                c
            });
            params.push(CPDecl {
                name: p.name.text.clone(),
                ty: (
                    p.ty.name.text.to_ascii_lowercase(),
                    p.ty.arg.as_ref().map(|a| a.text.to_ascii_lowercase()),
                ),
                optional: p.optional,
                default,
            });
        }
        let (body, columns) = self.query(&d.body);
        let rel: Vec<(Span, String)> = self.rewrites[mark..]
            .iter()
            .filter(|(s, _)| s.start >= span.start && s.end <= span.end)
            .map(|(s, t)| {
                (
                    Span::new(s.start - span.start, s.end - span.start),
                    t.clone(),
                )
            })
            .collect();
        let text = self
            .src
            .get(span.start as usize..span.end as usize)
            .unwrap_or("");
        self.portable
            .push((d.name.text.clone(), portable_text(text, &rel)));
        self.rewrites.truncate(mark);
        self.def = saved_def;
        self.view = saved_view;
        let cdef = CDefine {
            name: d.name.text.clone(),
            params,
            shape: d.shape.as_ref().map(|s| s.text.to_ascii_lowercase()),
            budget: d.budget.as_ref().map(|b| b.text.to_ascii_lowercase()),
            body,
        };
        (cdef, columns)
    }
}

/// The signature of a named mutation, for [`Binder::call_args`].
fn mutation_sig(m: &catalog::Mutation, all: KindSet) -> Sig {
    Sig {
        proc: format!("tx.{}", m.name),
        params: m
            .params
            .iter()
            .map(|p| (p.name.to_string(), p.ty, p.required))
            .collect(),
        yields: m
            .yields
            .iter()
            .map(|(n, t)| (n.to_string(), t.ty(all)))
            .collect(),
        named: true,
        class: None,
        live: false,
        tree: false,
        detail: false,
    }
}

/// Whether a `SET` value is an increment of the same counter: `t.c + k` or `k + t.c` ([50 §3.2]).
fn is_increment(v: &Expr, owner: &str, field: &str) -> bool {
    let same = |e: &Expr| match &e.kind {
        ExprKind::Prop(x, n) => {
            n.text == field && matches!(&x.kind, ExprKind::Ident(o) if o == owner)
                || n.text == field
                    && owner.starts_with('#')
                    && matches!(&x.kind, ExprKind::Nid(k) if format!("#{k}") == owner)
        }
        _ => false,
    };
    match &v.kind {
        ExprKind::Arith(ArithOp::Add, l, r) => same(l) || same(r),
        _ => false,
    }
}

/// The span of a statement, for located refusals.
fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Match(m) => m.span,
        Stmt::Muts(_, s) => *s,
        Stmt::Create(c) => c.span,
        Stmt::TxCall { span, .. } | Stmt::Assert { span, .. } => *span,
        Stmt::Resolve(r) => r.span,
        Stmt::Define(d) => d.span,
        Stmt::Drop(n) => n.span,
    }
}

/// The named mutation whose expansion the block's statements have ([LQ/std §7.2]), for E411's detail line.
fn matching_mutation(t: &Tx) -> Option<&'static str> {
    let mut found: Option<&'static str> = None;
    let mut agree = |n: Option<&'static str>| -> bool {
        match (found, n) {
            (_, None) => false,
            (None, Some(n)) => {
                found = Some(n);
                true
            }
            (Some(f), Some(n)) => f == n,
        }
    };
    for s in &t.stmts {
        let ok = match s {
            Stmt::TxCall { name, .. } => agree(catalog::mutation(&name.text).map(|m| m.name)),
            Stmt::Create(_) => agree(Some("add")),
            Stmt::Resolve(_) => agree(Some("resolve")),
            Stmt::Muts(v, _) => v.iter().all(|m| agree(mut_name(m))),
            Stmt::Match(m) => m.muts.iter().all(|x| {
                agree(match x {
                    Mut::Delete { .. } => Some("unlink"),
                    other => mut_name(other),
                })
            }),
            _ => false,
        };
        if !ok {
            return None;
        }
    }
    found
}

fn mut_name(m: &Mut) -> Option<&'static str> {
    Some(match m {
        Mut::Set(..) | Mut::Remove(..) => "set",
        Mut::Edge(e) if e.ty.text.eq_ignore_ascii_case("SUPERSEDES") => "supersede",
        Mut::Edge(_) => "link",
        Mut::Move { .. } => "move",
        Mut::Reopen { .. } => "reopen",
        Mut::Patch { .. } => "doc_patch",
        Mut::Delete { .. } => "rm",
    })
}

/// Whether a `TX ON $param` value is a ref name.
fn ref_like(t: &str) -> bool {
    let lx = crate::lq::lexer::Lexer::new(t);
    matches!(lx.revspec(0, &mut Vec::new()), Ok((r, end)) if end == t.len() && matches!(r.kind, RevKind::Ref(_)))
}
