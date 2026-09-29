//! Expressions ([50 §3.2]–§3.3; [LQ/canonical-ast §5.3], §5.5): types, properties with their classes, the built-in
//! functions of Table 5.3, aggregates (E112), comparisons with type-directed coercion, and the lints W01, W07 and W10.

use super::{AggPos, BKind, Binder, LintKind};
use crate::lq::ast::*;
use crate::lq::cast::*;
use crate::lq::catalog::{self, EnumTy, FnClass, Ret, Ty};
use crate::lq::diag::{Code, Diag, Span, list, near, q};
use crate::lq::printer;
use crate::lq::schema::{Coerce, DELETED_BIT, FieldClass, FieldTy, KindSet};

/// What reading or writing a property touches ([50 §2.5] groups; [LQ/errors] E115 classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PClass {
    /// A schema field of this merge class.
    Field(FieldClass),
    /// `id`, `uid`, `kind`, `created`, `created_at`, `created_by`, `created_role`.
    Identity,
    /// Derived, versioned state (and the bookkeeping `updated`, `updated_at`, `updated_by`, `rev`).
    Derived,
    /// Runtime state: branch tip only.
    Runtime,
    /// Tree-derived state: branch tip with a resolved tree.
    Tree,
    /// A tombstone field of a `DELETED` node.
    Tombstone,
    /// An edge property.
    EdgeProp,
    /// A field of a map, or a property of an untyped value.
    Other,
}

/// A property as the binder resolved it.
#[derive(Clone, Copy, Debug)]
pub(super) struct PropInfo {
    /// Its class.
    pub class: PClass,
    /// Absent on some kind of the variable ([50 §3.3], W01).
    pub optional: bool,
    /// The field is a counter ([F08 §5.1]).
    pub counter: bool,
}

const NONE_INFO: PropInfo = PropInfo {
    class: PClass::Other,
    optional: false,
    counter: false,
};

/// The built-in node properties ([50 §2.5], [F08 §8.2]): (name, type code, class). Type codes: `n` node, `t` text,
/// `k` kind name, `r` revision, `m` timestamp, `b` bool, `i` int, `p` map.
const NODE_PROPS: [(&str, char, PClass); 33] = [
    ("id", 'n', PClass::Identity),
    ("uid", 't', PClass::Identity),
    ("kind", 'k', PClass::Identity),
    ("created", 'r', PClass::Identity),
    ("updated", 'r', PClass::Derived),
    ("rev", 'r', PClass::Derived),
    ("created_at", 'm', PClass::Identity),
    ("updated_at", 'm', PClass::Derived),
    ("created_by", 't', PClass::Identity),
    ("created_role", 't', PClass::Identity),
    ("updated_by", 't', PClass::Derived),
    ("done", 'b', PClass::Derived),
    ("unfinished", 'b', PClass::Derived),
    ("container", 'b', PClass::Derived),
    ("unblocked", 'b', PClass::Derived),
    ("blocked", 'b', PClass::Derived),
    ("open_blockers", 'i', PClass::Derived),
    ("is_blocker", 'b', PClass::Derived),
    ("children_total", 'i', PClass::Derived),
    ("children_done", 'i', PClass::Derived),
    ("ready_to_close", 'b', PClass::Derived),
    ("suspect", 'b', PClass::Derived),
    ("conflicted", 'b', PClass::Derived),
    ("answered", 'b', PClass::Derived),
    ("has_dangling", 'b', PClass::Derived),
    ("depth", 'i', PClass::Derived),
    ("topo", 'i', PClass::Derived),
    ("ready", 'b', PClass::Runtime),
    ("claimed", 'b', PClass::Runtime),
    ("lease", 'p', PClass::Runtime),
    ("settled_elsewhere", 'b', PClass::Runtime),
    ("deleted_elsewhere", 'b', PClass::Runtime),
    ("state", 't', PClass::Tree),
];

/// The tombstone fields of a `DELETED` node ([50 §3.6]).
const TOMBSTONE_PROPS: [(&str, char); 6] = [
    ("kind", 'k'),
    ("title", 't'),
    ("deleted_by", 't'),
    ("deleted_at", 'm'),
    ("deleted_reason", 't'),
    ("replaced_by", 'n'),
];

/// The fields of the `lease` map ([50 §2.5]).
const LEASE_FIELDS: [(&str, char); 5] = [
    ("holder", 't'),
    ("token", 'i'),
    ("expires", 'm'),
    ("run", 't'),
    ("branch", 't'),
];

/// The anchor fields of an `AT` edge variable ([50 §2.5], [40 §2.7]).
const ANCHOR_FIELDS: [&str; 7] = ["kind", "mode", "watch", "scope", "quote", "hint", "anchor"];

/// The readiness properties whose use silences W07.
const READINESS: [&str; 4] = ["ready", "unblocked", "blocked", "open_blockers"];

fn code_ty(c: char, kinds: KindSet) -> Ty {
    match c {
        'n' => Ty::Node(kinds),
        'k' => Ty::KindName,
        'r' => Ty::Rev,
        'm' => Ty::Time,
        'b' => Ty::Bool,
        'i' => Ty::Int,
        'p' => Ty::Map,
        _ => Ty::Text,
    }
}

/// Whether two operand types compare ([50 §3.2]); ordered comparisons with `NULL` are a type error ([LQ/grammar-v1.ebnf]
/// O-7).
pub(super) fn compatible(ordered: bool, a: &Ty, b: &Ty) -> bool {
    use Ty::*;
    match (a, b) {
        (Any, _) | (_, Any) => true,
        (Null, _) | (_, Null) => !ordered,
        (Int | Float, Int | Float) => true,
        (Enum(x), Enum(y)) => x.field == y.field,
        (Enum(x), Int) | (Int, Enum(x)) => x.priority,
        (Enum(_), Text | KindName) | (Text | KindName, Enum(_)) => true,
        (Text | KindName, Text | KindName) => true,
        (Time, Time | Int) | (Int, Time) => true,
        (Dur, Dur) => true,
        (Rev, Rev | Int) | (Int, Rev) => true,
        (Node(_), Node(_)) | (Edge(_), Edge(_)) | (Bool, Bool) => true,
        (List(_), List(_)) | (Map, Map) | (Range, Range) => !ordered,
        _ => false,
    }
}

/// The use-site type of a node on an operator chain ([`Binder::expr`]): the chain's own, boolean, or none.
#[derive(Clone, Copy)]
enum Want {
    Outer,
    Bool,
    Free,
}

static BOOL: Ty = Ty::Bool;

impl Want {
    fn of(self, outer: Option<&Ty>) -> Option<&Ty> {
        match self {
            Want::Outer => outer,
            Want::Bool => Some(&BOOL),
            Want::Free => None,
        }
    }
}

/// Who owns a property, for the texts that name it: a written name, or an expression printed only when a text needs
/// it (printing every owner of a chain `a.b.c…` would take time quadratic in its length).
pub(super) enum Owner<'e> {
    /// A name as written (a variable, a target).
    Text(&'e str),
    /// The expression the property is read from.
    Expr(&'e Expr),
}

impl Owner<'_> {
    fn text(&self) -> String {
        match self {
            Owner::Text(s) => (*s).to_string(),
            Owner::Expr(e) => match &e.kind {
                ExprKind::Ident(v) => v.clone(),
                _ => printer::expr_text(e),
            },
        }
    }
}

impl Binder<'_> {
    /// Whether an expression takes its type from its use site: a literal the binder coerces, an unbound word, a
    /// parameter, or a list of them ([LQ/canonical-ast §5.5]).
    pub(super) fn coercible(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Str(_)
            | ExprKind::Int(_)
            | ExprKind::Nid(_)
            | ExprKind::Uid(_)
            | ExprKind::Null
            | ExprKind::Param(_) => true,
            ExprKind::Ident(n) => self.lookup(n).is_none() && self.item_alias(n).is_none(),
            ExprKind::List(v) => v.iter().all(|x| self.coercible(x)),
            _ => false,
        }
    }

    /// Binds an expression; `want` is its use site's type, which coerces literals, bare words and parameters.
    ///
    /// Operator chains nest in their first operand once per operator ([`Expr::first_operand`]), so that operand is
    /// walked in a loop with a heap stack ([LQ/grammar-v1.ebnf §P.13]): the binder descends while a node binds its first
    /// operand first ([`Self::first_bound`]), binds the innermost node, then finishes each node bottom-up — its other
    /// operand, its checks, its C-AST node — in the order the recursive definition gives. Only the other operands
    /// recurse, and nesting bounds their depth.
    pub(super) fn expr(&mut self, e: &Expr, want: Option<&Ty>) -> (CExpr, Ty) {
        let mut spine: Vec<&Expr> = Vec::new();
        let mut cur = e;
        let mut w = Want::Outer;
        while let Some((first, fw)) = self.first_bound(cur, w) {
            if let ExprKind::Not(x) = &cur.kind {
                self.lint_not(x);
            }
            spine.push(cur);
            cur = first;
            w = fw;
        }
        let mut acc = self.node(cur, w.of(want));
        while let Some(op) = spine.pop() {
            acc = self.finish_operator(op, acc);
        }
        acc
    }

    /// The first operand of `e` and its use-site type, when `e` binds that operand before anything else. A comparison or
    /// arithmetic whose left side alone is coercible binds its right side first (the coercible side takes the other's
    /// type, [LQ/canonical-ast §5.5]), and so does an `IN` unless its right side alone is coercible; such a node is bound
    /// whole by [`Self::node`].
    fn first_bound<'e>(&self, e: &'e Expr, w: Want) -> Option<(&'e Expr, Want)> {
        match &e.kind {
            ExprKind::Or(l, _) | ExprKind::And(l, _) => Some((l, Want::Bool)),
            ExprKind::Not(x) => Some((x, Want::Bool)),
            ExprKind::Cmp(_, l, r) | ExprKind::Arith(_, l, r) => {
                (!(self.coercible(l) && !self.coercible(r))).then_some((&**l, Want::Free))
            }
            ExprKind::StrPred(_, l, _) => Some((l, Want::Free)),
            ExprKind::IsNull(_, x) | ExprKind::LabelTest(x, _) | ExprKind::Prop(x, _) => {
                Some((x, Want::Free))
            }
            ExprKind::Neg(x) => Some((x, w)),
            _ => None,
        }
    }

    /// Finishes an operator whose first operand is bound (`first`): see [`Self::expr`].
    fn finish_operator(&mut self, e: &Expr, first: (CExpr, Ty)) -> (CExpr, Ty) {
        let span = e.span;
        let (fc, ft) = first;
        match &e.kind {
            ExprKind::Or(l, r) => {
                self.check_bool(&ft, l.span, "OR");
                let rc = self.operand_bool(r, "OR");
                (CExpr::Or(Box::new(fc), Box::new(rc)), Ty::Bool)
            }
            ExprKind::And(l, r) => {
                self.check_bool(&ft, l.span, "AND");
                let rc = self.operand_bool(r, "AND");
                (CExpr::And(Box::new(fc), Box::new(rc)), Ty::Bool)
            }
            ExprKind::Not(x) => {
                self.check_bool(&ft, x.span, "NOT");
                (CExpr::Not(Box::new(fc)), Ty::Bool)
            }
            ExprKind::Cmp(op, l, r) => {
                let (rc, rt) = self.expr(r, Some(&ft));
                self.cmp_done(*op, l, r, span, (fc, ft), (rc, rt))
            }
            ExprKind::IsNull(neg, _) => (CExpr::IsNull(*neg, Box::new(fc)), Ty::Bool),
            ExprKind::StrPred(op, l, r) => self.strpred_done(*op, l, r, span, fc, ft),
            ExprKind::LabelTest(x, labels) => {
                if !matches!(ft, Ty::Node(_) | Ty::Any | Ty::Null) {
                    self.err(Diag::new(
                        Code::E103,
                        x.span,
                        format!("{} :label: the types do not match", q(&ft.name())),
                    ));
                }
                let (_, names) = self.labels(labels);
                (CExpr::LabelTest(Box::new(fc), names), Ty::Bool)
            }
            ExprKind::Arith(op, _, r) => {
                let (rc, rt) = self.expr(r, Some(&ft));
                self.arith_done(*op, span, (fc, ft), (rc, rt))
            }
            ExprKind::Neg(_) => {
                // A priority is an integer 0–4 ([50 §2.5] header row, F2 `coerce` = priority), so it negates like one;
                // a literal operand takes that type from its use site (`a.priority - -1`, [LQ/grammar-v1.ebnf §P.5]
                // fixture `undirected-minus`).
                let ft = match ft {
                    Ty::Enum(e) if e.priority => Ty::Int,
                    t => t,
                };
                if !matches!(ft, Ty::Int | Ty::Float | Ty::Dur | Ty::Any) {
                    self.err(Diag::new(
                        Code::E103,
                        span,
                        format!("- {}: the types do not match", q(&ft.name())),
                    ));
                }
                (CExpr::Neg(Box::new(fc)), ft)
            }
            ExprKind::Prop(x, name) => {
                let (pt, _) = self.prop_of(&ft, &name.text, name.span, &Owner::Expr(x), true);
                (CExpr::Prop(Box::new(fc), name.text.clone()), pt)
            }
            _ => unreachable!("only operators that bind their first operand first are finished"),
        }
    }

    /// Binds a node that [`Self::expr`] does not walk: no operator, or an operator that binds its right side first.
    fn node(&mut self, e: &Expr, want: Option<&Ty>) -> (CExpr, Ty) {
        let span = e.span;
        match &e.kind {
            ExprKind::Cmp(op, l, r) => {
                let (rc, rt) = self.expr(r, None);
                let (lc, lt) = self.expr(l, Some(&rt));
                self.cmp_done(*op, l, r, span, (lc, lt), (rc, rt))
            }
            ExprKind::Arith(op, l, r) => {
                let (rc, rt) = self.expr(r, None);
                let (lc, lt) = self.expr(l, Some(&rt));
                self.arith_done(*op, span, (lc, lt), (rc, rt))
            }
            ExprKind::In(l, r) => self.in_(l, r, span),
            ExprKind::Ident(n) => {
                if let Some((i, t)) = self.item_alias(n) {
                    return (CExpr::ItemRef(i), t);
                }
                match self.lookup(n) {
                    Some(id) => (CExpr::Var(id), self.b[id as usize].ty.clone()),
                    None => self.coerce_word(n, span, want),
                }
            }
            ExprKind::Param(n) => self.param(n, span, want),
            ExprKind::Nid(n) => self.node_num(*n, span, true),
            ExprKind::Uid(h) => self.node_uid(h, span),
            ExprKind::Int(n) => self.coerce_int(*n, span, want),
            ExprKind::Float(t) => (CExpr::Float(float_value(t)), Ty::Float),
            ExprKind::Str(s) => self.coerce_str(s, span, want),
            // A duration token always has a value; a JSON IR `dur` text might not ([LQ/json-ir §2] item 3: E003).
            ExprKind::Dur(t) => match crate::lq::lexer::duration_ms(t) {
                Some(ms) => (CExpr::Duration(ms), Ty::Dur),
                None => {
                    self.err(Diag::new(
                        Code::E003,
                        span,
                        format!("{} is out of range", q(t)),
                    ));
                    (CExpr::Null, Ty::Dur)
                }
            },
            ExprKind::Bool(b) => (CExpr::Bool(*b), Ty::Bool),
            ExprKind::Null => (CExpr::Null, Ty::Null),
            ExprKind::Exists(s) => {
                let s = self.sub(s);
                (CExpr::Exists(Box::new(s)), Ty::Bool)
            }
            ExprKind::CountSub(s) => {
                let s = self.sub(s);
                (CExpr::CountSub(Box::new(s)), Ty::Int)
            }
            ExprKind::Fn {
                name,
                distinct,
                args,
            } => self.func(name, *distinct, args, span),
            ExprKind::CountStar => {
                self.aggregate_here("count", span);
                (CExpr::CountStar, Ty::Int)
            }
            ExprKind::ListPred {
                kind,
                var,
                list,
                pred,
            } => {
                let (lc, lt) = self.expr(list, None);
                let elem = match lt {
                    Ty::List(t) => *t,
                    Ty::Any | Ty::Null => Ty::Any,
                    other => {
                        self.err(Diag::new(
                            Code::E103,
                            list.span,
                            format!(
                                "{} IN {}: the types do not match",
                                q(&var.text),
                                q(&other.name())
                            ),
                        ));
                        Ty::Any
                    }
                };
                let mark = self.scope.len();
                let bk = if matches!(elem, Ty::Node(_)) {
                    BKind::Node
                } else {
                    BKind::Value
                };
                let id = self.declare(&var.text, elem, bk);
                let pc = self.operand_bool(pred, "WHERE");
                self.scope.truncate(mark);
                let k = match kind {
                    ListPredKind::All => 1,
                    ListPredKind::Any => 2,
                    ListPredKind::None => 3,
                };
                (CExpr::ListPred(k, id, Box::new(lc), Box::new(pc)), Ty::Bool)
            }
            ExprKind::List(v) => {
                let elem_want = match want {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                let mut out = Vec::with_capacity(v.len());
                let mut elem = elem_want.clone().unwrap_or(Ty::Any);
                for x in v {
                    let (c, t) = self.expr(x, elem_want.as_ref());
                    if elem == Ty::Any && t != Ty::Null {
                        elem = t;
                    }
                    out.push(c);
                }
                (CExpr::List(out), Ty::List(Box::new(elem)))
            }
            ExprKind::Map(v) => {
                let entries = v
                    .iter()
                    .map(|kv| (kv.key.text.clone(), self.expr(&kv.value, None).0))
                    .collect();
                (CExpr::Map(entries), Ty::Map)
            }
            ExprKind::Case {
                subject,
                whens,
                else_,
            } => self.case(subject.as_deref(), whens, else_.as_deref(), want),
            ExprKind::Or(..)
            | ExprKind::And(..)
            | ExprKind::Not(_)
            | ExprKind::IsNull(..)
            | ExprKind::StrPred(..)
            | ExprKind::LabelTest(..)
            | ExprKind::Neg(_)
            | ExprKind::Prop(..) => {
                unreachable!("expr walks operators that bind their first operand first")
            }
        }
    }

    /// E103 when an operand of a boolean operator is not boolean.
    fn check_bool(&mut self, t: &Ty, span: Span, op: &str) {
        if !matches!(t, Ty::Bool | Ty::Any | Ty::Null) {
            self.err(Diag::new(
                Code::E103,
                span,
                format!("{op} {}: the types do not match", q(&t.name())),
            ));
        }
    }

    pub(super) fn operand_bool(&mut self, e: &Expr, op: &str) -> CExpr {
        let (c, t) = self.expr(e, Some(&Ty::Bool));
        self.check_bool(&t, e.span, op);
        c
    }

    /// The type checks and lints of a comparison whose operands are bound.
    fn cmp_done(
        &mut self,
        op: CmpOp,
        l: &Expr,
        r: &Expr,
        span: Span,
        (lc, lt): (CExpr, Ty),
        (rc, rt): (CExpr, Ty),
    ) -> (CExpr, Ty) {
        let ordered = matches!(op, CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge);
        if !compatible(ordered, &lt, &rt) {
            self.err(Diag::new(
                Code::E103,
                span,
                format!(
                    "{} {} {}: the types do not match",
                    q(&lt.name()),
                    op.as_str(),
                    q(&rt.name())
                ),
            ));
        }
        for (side, other) in [(l, r), (r, l)] {
            if ordered {
                self.lint_w01(side);
            }
            self.lint_status_of_blocker(side);
            if op == CmpOp::Ne {
                self.lint_w10(side, "<>");
            }
            if self.def.is_some()
                && self.is_anchor_prop(side)
                && matches!(other.kind, ExprKind::Str(_))
            {
                self.anchor_handle(other.span);
            }
        }
        if op == CmpOp::Eq {
            for (side, other) in [(l, r), (r, l)] {
                if matches!(other.kind, ExprKind::Int(0))
                    && let ExprKind::CountSub(s) = &side.kind
                {
                    self.lint_blocks_into(s);
                }
            }
        }
        let code = match op {
            CmpOp::Eq => 1,
            CmpOp::Ne => 2,
            CmpOp::Lt => 3,
            CmpOp::Le => 4,
            CmpOp::Gt => 5,
            CmpOp::Ge => 6,
        };
        (CExpr::Cmp(code, Box::new(lc), Box::new(rc)), Ty::Bool)
    }

    fn in_(&mut self, l: &Expr, r: &Expr, span: Span) -> (CExpr, Ty) {
        let (lc, lt, rc, elem) = if self.coercible(r) && !self.coercible(l) {
            let (lc, lt) = self.expr(l, None);
            let (rc, rt) = self.expr(r, Some(&Ty::List(Box::new(lt.clone()))));
            let elem = self.elem_of(&rt, r.span);
            (lc, lt, rc, elem)
        } else {
            let (rc, rt) = self.expr(r, None);
            let elem = self.elem_of(&rt, r.span);
            let (lc, lt) = self.expr(l, Some(&elem));
            (lc, lt, rc, elem)
        };
        if !compatible(false, &lt, &elem) {
            self.err(Diag::new(
                Code::E103,
                span,
                format!(
                    "{} IN {}: the types do not match",
                    q(&lt.name()),
                    q(&format!("list<{}>", elem.name()))
                ),
            ));
        }
        self.labels_membership(l, r);
        (CExpr::In(Box::new(lc), Box::new(rc)), Ty::Bool)
    }

    /// The element type of an `IN` right side.
    fn elem_of(&mut self, t: &Ty, span: Span) -> Ty {
        match t {
            Ty::List(e) => (**e).clone(),
            Ty::Range => Ty::Int,
            Ty::Any | Ty::Null => Ty::Any,
            other => {
                self.err(Diag::new(
                    Code::E103,
                    span,
                    format!("IN {}: the types do not match", q(&other.name())),
                ));
                Ty::Any
            }
        }
    }

    /// E102's `labels()` row ([50 §2.5]): a literal that is not a kind tested against `labels(n)`.
    fn labels_membership(&mut self, value: &Expr, list: &Expr) {
        let ExprKind::Fn { name, args, .. } = &list.kind else {
            return;
        };
        if !name.text.eq_ignore_ascii_case("labels") {
            return;
        }
        let ExprKind::Str(s) = &value.kind else {
            return;
        };
        if s.eq_ignore_ascii_case("deleted") || self.ctx.schema.kind(s).is_some() {
            return;
        }
        let v = match args.first().map(|a| &a.value) {
            Some(ArgVal::Expr(x)) => printer::expr_text(x),
            _ => "n".to_string(),
        };
        let lit = printer::string_lit(s);
        self.err(
            Diag::new(
                Code::E102,
                value.span,
                format!("{} is not a kind; task labels are the field", q(&lit)),
            )
            .inline(format!("write {lit} IN {v}.labels")),
        );
    }

    /// A string predicate whose left side is bound: its right side, checks and node.
    fn strpred_done(
        &mut self,
        op: StrOp,
        l: &Expr,
        r: &Expr,
        span: Span,
        lc: CExpr,
        lt: Ty,
    ) -> (CExpr, Ty) {
        let code = match op {
            StrOp::Starts => 1,
            StrOp::Ends => 2,
            StrOp::Contains => 3,
        };
        let want = match (&lt, op) {
            (Ty::List(e), StrOp::Contains) => (**e).clone(),
            _ => Ty::Text,
        };
        let (rc, rt) = self.expr(r, Some(&want));
        let textual = |t: &Ty| {
            matches!(
                t,
                Ty::Text | Ty::KindName | Ty::Enum(_) | Ty::Any | Ty::Null
            )
        };
        let ok = match (&lt, op) {
            (Ty::List(e), StrOp::Contains) => compatible(false, e, &rt),
            _ => textual(&lt) && textual(&rt),
        };
        if !ok {
            let word = match op {
                StrOp::Starts => "STARTS WITH",
                StrOp::Ends => "ENDS WITH",
                StrOp::Contains => "CONTAINS",
            };
            self.err(Diag::new(
                Code::E103,
                span,
                format!(
                    "{} {word} {}: the types do not match",
                    q(&lt.name()),
                    q(&rt.name())
                ),
            ));
        }
        if op == StrOp::Contains {
            self.labels_membership(r, l);
        }
        (CExpr::StrPred(code, Box::new(lc), Box::new(rc)), Ty::Bool)
    }

    /// The type of an arithmetic whose operands are bound.
    fn arith_done(
        &mut self,
        op: ArithOp,
        span: Span,
        (lc, lt): (CExpr, Ty),
        (rc, rt): (CExpr, Ty),
    ) -> (CExpr, Ty) {
        let num = |t: &Ty| match t {
            Ty::Enum(e) if e.priority => Some(Ty::Int),
            Ty::Int => Some(Ty::Int),
            Ty::Float => Some(Ty::Float),
            _ => None,
        };
        use ArithOp::*;
        let t = match (&lt, &rt, op) {
            (Ty::Any, _, _) | (_, Ty::Any, _) | (Ty::Null, _, _) | (_, Ty::Null, _) => {
                Some(if op == Div { Ty::Float } else { Ty::Any })
            }
            (Ty::Time, Ty::Dur, Add | Sub) | (Ty::Dur, Ty::Time, Add) => Some(Ty::Time),
            (Ty::Time, Ty::Time, Sub) => Some(Ty::Dur),
            (Ty::Dur, Ty::Dur, Add | Sub) => Some(Ty::Dur),
            // A duration scaled by an integer ([50 §2.9] Q21 writes `$days * 1d`).
            (Ty::Dur, Ty::Int, Mul) | (Ty::Int, Ty::Dur, Mul) => Some(Ty::Dur),
            (Ty::Text, Ty::Text, Add) => Some(Ty::Text),
            (Ty::List(a), Ty::List(_), Add) => Some(Ty::List(a.clone())),
            _ => match (num(&lt), num(&rt)) {
                (Some(_), Some(_)) if op == Div => Some(Ty::Float),
                (Some(Ty::Int), Some(Ty::Int)) => Some(Ty::Int),
                (Some(_), Some(_)) => Some(Ty::Float),
                _ => None,
            },
        };
        let t = t.unwrap_or_else(|| {
            self.err(Diag::new(
                Code::E103,
                span,
                format!(
                    "{} {} {}: the types do not match",
                    q(&lt.name()),
                    op.as_str(),
                    q(&rt.name())
                ),
            ));
            Ty::Any
        });
        let code = match op {
            Add => 1,
            Sub => 2,
            Mul => 3,
            Div => 4,
        };
        (CExpr::Arith(code, Box::new(lc), Box::new(rc)), t)
    }

    fn case(
        &mut self,
        subject: Option<&Expr>,
        whens: &[When],
        else_: Option<&Expr>,
        want: Option<&Ty>,
    ) -> (CExpr, Ty) {
        let subj = subject.map(|s| self.expr(s, None));
        let mut arms = Vec::with_capacity(whens.len());
        let mut ty = Ty::Null;
        for w in whens {
            let c = match &subj {
                Some((_, st)) => {
                    let st = st.clone();
                    let (c, t) = self.expr(&w.cond, Some(&st));
                    if !compatible(false, &st, &t) {
                        self.err(Diag::new(
                            Code::E103,
                            w.cond.span,
                            format!(
                                "{} = {}: the types do not match",
                                q(&st.name()),
                                q(&t.name())
                            ),
                        ));
                    }
                    c
                }
                None => self.operand_bool(&w.cond, "WHEN"),
            };
            let hint = if ty == Ty::Null {
                want.cloned()
            } else {
                Some(ty.clone())
            };
            let (t, tt) = self.expr(&w.then, hint.as_ref());
            if ty == Ty::Null {
                ty = tt;
            }
            arms.push((c, t));
        }
        let e = else_.map(|x| {
            let hint = if ty == Ty::Null {
                want.cloned()
            } else {
                Some(ty.clone())
            };
            let (c, t) = self.expr(x, hint.as_ref());
            if ty == Ty::Null {
                ty = t;
            }
            Box::new(c)
        });
        (
            CExpr::Case(subj.map(|(c, _)| Box::new(c)), arms, e),
            if ty == Ty::Null { Ty::Any } else { ty },
        )
    }

    // ----- properties ----------------------------------------------------------------------------------------------

    /// The type of property `name` on a value of type `base` ([50 §3.2] "Properties"): E101 when no kind of the
    /// variable has it; when `read`, E302 for runtime and tree-derived state at a past view.
    pub(super) fn prop(
        &mut self,
        base: &Ty,
        name: &str,
        span: Span,
        owner: &str,
        read: bool,
    ) -> (Ty, PropInfo) {
        self.prop_of(base, name, span, &Owner::Text(owner), read)
    }

    /// [`Self::prop`] with the owner printed only when a text names it.
    fn prop_of(
        &mut self,
        base: &Ty,
        name: &str,
        span: Span,
        owner: &Owner<'_>,
        read: bool,
    ) -> (Ty, PropInfo) {
        match base {
            Ty::Any | Ty::Null => (Ty::Any, NONE_INFO),
            Ty::Node(ks) => self.node_prop(*ks, name, span, owner, read),
            Ty::Edge(es) => self.edge_prop(*es, name, span, owner, read),
            Ty::Map => match LEASE_FIELDS.iter().find(|(n, _)| *n == name) {
                Some((_, c)) => (
                    code_ty(*c, self.all_kinds()),
                    PropInfo {
                        optional: true,
                        ..NONE_INFO
                    },
                ),
                None => {
                    self.unknown_field(
                        "lease",
                        name,
                        span,
                        LEASE_FIELDS.iter().map(|(n, _)| n.to_string()).collect(),
                    );
                    (Ty::Any, NONE_INFO)
                }
            },
            other => {
                self.err(Diag::new(
                    Code::E103,
                    span,
                    format!(
                        "{} is {}; only nodes, edges and maps have properties",
                        q(&owner.text()),
                        q(&other.name())
                    ),
                ));
                (Ty::Any, NONE_INFO)
            }
        }
    }

    /// The property type the binder uses for a pattern map entry or a `SET`; see [`Self::prop`].
    pub(super) fn prop_type(
        &mut self,
        base: &Ty,
        name: &str,
        span: Span,
        owner: String,
    ) -> (Ty, PropInfo) {
        self.prop(base, name, span, &owner, true)
    }

    /// Looks a node property up without diagnostics: (type, info), or `None`.
    pub(super) fn node_prop_info(&self, ks: KindSet, name: &str) -> Option<(Ty, PropInfo)> {
        let s = self.ctx.schema;
        let live = ks.live();
        if let Some((_, c, class)) = NODE_PROPS.iter().find(|(n, _, _)| *n == name) {
            let admits = match name {
                "id" | "uid" | "kind" => true,
                "done" => live
                    .iter()
                    .any(|k| k < s.kinds.len() && s.kinds[k].has_done),
                "state" => s.kind("artifact").is_some_and(|a| live.contains(a)),
                _ => !live.is_empty(),
            };
            if admits {
                let optional = name == "lease"
                    || (name == "done"
                        && live
                            .iter()
                            .any(|k| k < s.kinds.len() && !s.kinds[k].has_done));
                return Some((
                    code_ty(*c, ks),
                    PropInfo {
                        class: *class,
                        optional,
                        counter: false,
                    },
                ));
            }
        }
        if ks.contains(DELETED_BIT)
            && let Some((_, c)) = TOMBSTONE_PROPS.iter().find(|(n, _)| *n == name)
        {
            return Some((
                code_ty(*c, self.all_kinds()),
                PropInfo {
                    class: PClass::Tombstone,
                    optional: true,
                    counter: false,
                },
            ));
        }
        let mut found = KindSet::EMPTY;
        let mut first = None;
        let mut optional = false;
        for k in live.iter() {
            if k >= s.kinds.len() {
                continue;
            }
            match s.field(k, name) {
                Some(f) => {
                    found.insert(k);
                    optional |= f.optional;
                    if first.is_none() {
                        first = Some(f);
                    }
                }
                None => optional = true,
            }
        }
        let f = first?;
        let ty = match f.ty {
            FieldTy::Bool => Ty::Bool,
            FieldTy::Int | FieldTy::Counter => match f.coerce {
                Coerce::Timestamp => Ty::Time,
                Coerce::RevisionInteger => Ty::Rev,
                _ => Ty::Int,
            },
            FieldTy::Float => Ty::Float,
            FieldTy::Enum => Ty::Enum(Box::new(EnumTy {
                field: name.to_string(),
                kinds: found,
                priority: f.coerce == Coerce::Priority,
            })),
            FieldTy::Text | FieldTy::Path | FieldTy::Oid | FieldTy::Body => Ty::Text,
            FieldTy::Ref => Ty::Node(self.all_kinds()),
            FieldTy::Commit => Ty::Rev,
            FieldTy::TextSet | FieldTy::PathSet | FieldTy::PathMoveSet => {
                Ty::List(Box::new(Ty::Text))
            }
        };
        Some((
            ty,
            PropInfo {
                class: PClass::Field(f.class),
                optional,
                counter: f.ty == FieldTy::Counter,
            },
        ))
    }

    fn node_prop(
        &mut self,
        ks: KindSet,
        name: &str,
        span: Span,
        owner: &Owner<'_>,
        read: bool,
    ) -> (Ty, PropInfo) {
        match self.node_prop_info(ks, name) {
            Some((ty, info)) => {
                if read {
                    if READINESS.contains(&name) {
                        self.readiness_used = true;
                    }
                    match info.class {
                        PClass::Runtime => {
                            let owner = owner.text();
                            let prop = format!("{owner}.{name}");
                            let hint = (name == "ready").then_some(owner.as_str());
                            self.tip_only("leases and markers", &prop, span, false, hint);
                        }
                        PClass::Tree => {
                            let prop = format!("{}.{name}", owner.text());
                            self.tip_only("the file tree", &prop, span, true, None);
                        }
                        _ => {}
                    }
                }
                (ty, info)
            }
            // No kind is left when a pattern's labels contradict its literal id's kind (`(x:task {id: #212})` with a
            // rule #212, or `(x:DELETED {id: #51})` with a live #51): the pattern binds nothing, which is no error, as a
            // literal id naming a deleted node is none ([50 §3.6]); nothing is known of its properties.
            None if ks.is_empty() => (Ty::Any, NONE_INFO),
            None => {
                let s = self.ctx.schema;
                let live: Vec<usize> = ks.live().iter().filter(|&k| k < s.kinds.len()).collect();
                let mut names: Vec<String> = Vec::new();
                for &k in &live {
                    for f in s.fields_of(k) {
                        if !names.contains(&f.name) {
                            names.push(f.name.clone());
                        }
                    }
                }
                names.extend(NODE_PROPS.iter().map(|(n, _, _)| n.to_string()));
                let kind = if live.len() == 1 {
                    s.kinds[live[0]].name.clone()
                } else {
                    s.kinds_text(ks)
                };
                if name == "open" {
                    let msg = if live.len() == 1 {
                        format!("kind {} has no field {}", q(&kind), q(name))
                    } else {
                        format!("kinds {} have no field {}", q(&kind), q(name))
                    };
                    let owner = owner.text();
                    self.err(
                        Diag::new(Code::E101, span, msg)
                            .inline(format!("write {owner}.status = 'open', or {owner}.unfinished for any unfinished status")),
                    );
                } else {
                    self.unknown_field(&kind, name, span, names);
                    if live.len() == 1 {
                        let fields: Vec<String> = s
                            .fields_of(live[0])
                            .iter()
                            .map(|f| f.name.clone())
                            .collect();
                        if let Some(d) = self.errors.last_mut() {
                            d.help = Some(
                                format!(
                                    "{kind} fields: {} (CALL schema(kind: '{kind}'))",
                                    list(&fields)
                                )
                                .into(),
                            );
                        }
                    } else if let Some(d) = self.errors.last_mut() {
                        d.message = format!("kinds {} have no field {}", q(&kind), q(name));
                    }
                }
                (Ty::Any, NONE_INFO)
            }
        }
    }

    fn unknown_field(&mut self, kind: &str, name: &str, span: Span, candidates: Vec<String>) {
        let s = near(name, candidates.iter().map(String::as_str));
        let mut d = Diag::new(
            Code::E101,
            span,
            format!("kind {} has no field {}", q(kind), q(name)),
        );
        if let Some(first) = s.first() {
            d = d.inline(format!("did you mean {}?", q(first)));
        }
        d.suggest = s.into();
        self.err(d);
    }

    fn edge_prop(
        &mut self,
        es: KindSet,
        name: &str,
        span: Span,
        owner: &Owner<'_>,
        read: bool,
    ) -> (Ty, PropInfo) {
        let s = self.ctx.schema;
        let kinds: Vec<usize> = es.iter().filter(|&k| k < s.edges.len()).collect();
        let edge_info = PropInfo {
            class: PClass::EdgeProp,
            optional: true,
            counter: false,
        };
        let found = match name {
            "type" => Some(Ty::Text),
            "pinned" if kinds.iter().any(|&k| s.edges[k].pinned) => Some(Ty::Rev),
            "flagged" if kinds.iter().any(|&k| s.edges[k].flagged) => Some(Ty::Bool),
            "state" if kinds.iter().any(|&k| s.edges[k].anchor) => {
                if read {
                    self.tip_only(
                        "the file tree",
                        &format!("{}.{name}", owner.text()),
                        span,
                        true,
                        None,
                    );
                }
                Some(Ty::Text)
            }
            n if ANCHOR_FIELDS.contains(&n) && kinds.iter().any(|&k| s.edges[k].anchor) => {
                Some(Ty::Text)
            }
            _ => None,
        };
        match found {
            Some(t) => (t, edge_info),
            None => {
                let mut cands = vec!["type".to_string()];
                if kinds.iter().any(|&k| s.edges[k].pinned) {
                    cands.push("pinned".into());
                }
                if kinds.iter().any(|&k| s.edges[k].flagged) {
                    cands.push("flagged".into());
                }
                if kinds.iter().any(|&k| s.edges[k].anchor) {
                    cands.extend(ANCHOR_FIELDS.iter().map(|a| a.to_string()));
                    cands.push("state".into());
                }
                let names: Vec<&str> = kinds.iter().map(|&k| s.edges[k].lq.as_str()).collect();
                let kind = if names.len() == s.edges.len() {
                    "edge".to_string()
                } else {
                    names.join("|")
                };
                self.unknown_field(&kind, name, span, cands);
                (Ty::Any, NONE_INFO)
            }
        }
    }

    /// E103 when a value's type does not fit a property or parameter type.
    pub(super) fn check_assignable(&mut self, want: &Ty, got: &Ty, what: &str, span: Span) {
        let ok = match (want, got) {
            (Ty::List(a), Ty::List(b)) => compatible(false, a, b),
            (Ty::Map, _) => true,
            _ => compatible(false, want, got),
        };
        if !ok {
            self.err(Diag::new(
                Code::E103,
                span,
                format!(
                    "{} {} = {}: the types do not match",
                    q(what),
                    q(&want.name()),
                    q(&got.name())
                ),
            ));
        }
    }

    // ----- functions -----------------------------------------------------------------------------------------------

    /// Records an aggregate at the current position (E112).
    fn aggregate_here(&mut self, name: &str, span: Span) -> bool {
        match self.agg {
            AggPos::Allowed => {
                self.saw_agg = true;
                true
            }
            AggPos::Where => {
                self.err(
                    Diag::new(Code::E112, span, format!("aggregate {name}() inside WHERE"))
                        .help("filter aggregates with WITH ... WHERE"),
                );
                false
            }
            AggPos::Nested => {
                self.err(Diag::new(
                    Code::E112,
                    span,
                    format!("aggregate {name}() inside another aggregate"),
                ));
                false
            }
            AggPos::Other => {
                self.err(Diag::new(
                    Code::E112,
                    span,
                    format!("aggregate {name}() outside RETURN and WITH"),
                ));
                false
            }
        }
    }

    fn func(&mut self, name: &Name, distinct: bool, args: &[Arg], span: Span) -> (CExpr, Ty) {
        let written = name.text.as_str();
        let rel = catalog::relation(written);
        let f = if written.eq_ignore_ascii_case("datetime") && args.is_empty() {
            catalog::function("now")
        } else {
            catalog::function(written)
        };
        let Some(f) = f else {
            let mut d = if let Some(r) = rel {
                Diag::new(
                    Code::E109,
                    name.span,
                    format!(
                        "{} is a relation; call it with CALL {}(...) YIELD ...",
                        q(r.name),
                        r.name
                    ),
                )
            } else {
                Diag::new(
                    Code::E109,
                    name.span,
                    format!("unknown function {}", q(written)),
                )
            };
            if rel.is_none() {
                let s = near(written, catalog::function_names());
                if let Some(first) = s.first() {
                    d = d.inline(format!("did you mean {}?", q(first)));
                }
                d.suggest = s.into();
            }
            d = d.help(
                "CALL queries() lists the named queries; the built-ins are in reference-ql.md",
            );
            self.err(d);
            for a in args {
                if let ArgVal::Expr(x) = &a.value {
                    self.expr(x, None);
                }
            }
            return (CExpr::Null, Ty::Any);
        };
        let canonical = f.names[0];
        if f.agg {
            self.aggregate_here(canonical, span);
        }
        // Arity and named arguments.
        let required = f.params.iter().filter(|p| p.required).count();
        let most = if f.variadic {
            usize::MAX
        } else {
            f.params.len()
        };
        if args.len() < required || args.len() > most {
            let n = if f.variadic {
                format!("at least {required}")
            } else if required == f.params.len() {
                required.to_string()
            } else {
                format!("{required} to {}", f.params.len())
            };
            self.err(Diag::new(
                Code::E109,
                name.span,
                format!("{} takes {n} arguments, got {}", q(canonical), args.len()),
            ));
        }
        let saved = self.agg;
        if f.agg {
            self.agg = AggPos::Nested;
        }
        let mut out = Vec::with_capacity(args.len());
        let mut types = Vec::with_capacity(args.len());
        let mut pos = 0;
        for a in args {
            let param = match &a.name {
                Some(n) => match f.params.iter().find(|p| p.name == n.text) {
                    Some(p) => Some(*p),
                    None => {
                        self.err(Diag::new(
                            Code::E109,
                            n.span,
                            format!("{} has no argument {}", q(canonical), q(&n.text)),
                        ));
                        None
                    }
                },
                None => {
                    let i = if f.variadic {
                        pos.min(f.params.len().saturating_sub(1))
                    } else {
                        pos
                    };
                    pos += 1;
                    f.params.get(i).copied()
                }
            };
            let want = param.map(|p| p.ty.ty(self.all_kinds()));
            let want = match (canonical, types.first()) {
                ("coalesce", Some(t)) => Some(Ty::clone(t)),
                _ => want.filter(|w| *w != Ty::Any),
            };
            let (c, t) = match &a.value {
                ArgVal::Expr(x) => self.expr(x, want.as_ref()),
                other => (self.rev_arg(other), Ty::Rev),
            };
            if let (Some(w), ArgVal::Expr(x)) = (&want, &a.value) {
                let fits = match (w, &t) {
                    (Ty::Node(_), Ty::Node(_) | Ty::Any | Ty::Null) => true,
                    (Ty::Text, Ty::Enum(_) | Ty::KindName) => true,
                    _ => compatible(false, w, &t),
                };
                if !fits {
                    self.err(Diag::new(
                        Code::E103,
                        x.span,
                        format!("{}({}): the types do not match", canonical, q(&t.name())),
                    ));
                }
            }
            types.push(t);
            out.push(CArg {
                name: a.name.as_ref().map(|n| n.text.clone()),
                value: c,
            });
        }
        self.agg = saved;
        match f.class {
            FnClass::Tree => {
                let text = format!("{canonical}(...)");
                self.tip_only("the file tree", &text, span, true, None);
            }
            FnClass::Git => {
                let text = format!("{canonical}(...)");
                self.tip_only("git ancestry", &text, span, false, None);
            }
            FnClass::Plain => {}
        }
        let first = types.first().cloned().unwrap_or(Ty::Any);
        let ret = match f.ret {
            Ret::Bool => Ty::Bool,
            Ret::Int => Ty::Int,
            Ret::Float => Ty::Float,
            Ret::Text => Ty::Text,
            Ret::Time => Ty::Time,
            Ret::Dur => Ty::Dur,
            Ret::NodeOfArg0 => match first {
                Ty::Node(k) => Ty::Node(k),
                _ => Ty::Node(self.all_kinds()),
            },
            Ret::Artifact => Ty::Node(
                self.ctx
                    .schema
                    .kind("artifact")
                    .map_or(self.all_kinds(), KindSet::one),
            ),
            Ret::NodeList => Ty::List(Box::new(Ty::Node(self.all_kinds()))),
            Ret::KindNames => Ty::List(Box::new(Ty::KindName)),
            Ret::Arg0 => match first {
                Ty::Enum(e) if e.priority => Ty::Int,
                t => t,
            },
            Ret::ListOfArg0 => Ty::List(Box::new(first)),
        };
        (CExpr::Func(canonical.to_string(), distinct, out), ret)
    }

    // ----- lints ---------------------------------------------------------------------------------------------------

    /// The kind set of a node variable named by an identifier.
    fn var_kinds(&self, e: &Expr) -> Option<(String, KindSet)> {
        let ExprKind::Ident(v) = &e.kind else {
            return None;
        };
        let id = self.lookup(v)?;
        match &self.b[id as usize].ty {
            Ty::Node(k) if self.b[id as usize].kind != BKind::Edge => Some((v.clone(), *k)),
            _ => None,
        }
    }

    /// W01: an ordered comparison of a property absent on some kind of its variable ([50 §3.3]).
    fn lint_w01(&mut self, side: &Expr) {
        let ExprKind::Prop(x, name) = &side.kind else {
            return;
        };
        let Some((_, ks)) = self.var_kinds(x) else {
            return;
        };
        if let Some((_, info)) = self.node_prop_info(ks, &name.text)
            && info.optional
        {
            let expr = printer::expr_text(side);
            self.lint(
                Code::W01,
                side.span,
                LintKind::W01 {
                    field: name.text.clone(),
                    expr,
                },
            );
        }
    }

    /// W10: `<>` or `NOT IN` over `link_state()` of a node variable that is neither an `AT` edge nor an artifact.
    fn lint_w10(&mut self, side: &Expr, op: &str) {
        let ExprKind::Fn { name, args, .. } = &side.kind else {
            return;
        };
        if !name.text.eq_ignore_ascii_case("link_state") {
            return;
        }
        let Some(ArgVal::Expr(x)) = args.first().map(|a| &a.value) else {
            return;
        };
        let Some((v, ks)) = self.var_kinds(x) else {
            return;
        };
        let artifact = self
            .ctx
            .schema
            .kind("artifact")
            .map_or(KindSet::EMPTY, KindSet::one);
        if !ks.live().subset_of(artifact) {
            self.lint(
                Code::W10,
                side.span,
                LintKind::W10 {
                    var: v,
                    op: op.to_string(),
                },
            );
        }
    }

    /// `NOT` over `EXISTS {…}` of an in-edge of a blocking kind (W07), and `NOT IN` over `link_state()` (W10).
    fn lint_not(&mut self, x: &Expr) {
        match &x.kind {
            ExprKind::Exists(s) => self.lint_blocks_into(s),
            ExprKind::In(l, _) => self.lint_w10(l, "NOT IN"),
            _ => {}
        }
    }

    /// W07 (a): a subquery whose pattern has a `BLOCKS` or `GATES` edge into a variable visible outside it.
    fn lint_blocks_into(&mut self, s: &Sub) {
        let paths: Vec<&Path> = match s {
            Sub::Patterns { patterns, .. } => patterns.iter().collect(),
            Sub::Clauses { clauses, .. } => clauses
                .iter()
                .filter_map(|c| match c {
                    Clause::Match(m) => Some(m.patterns.iter()),
                    _ => None,
                })
                .flatten()
                .collect(),
        };
        for p in paths {
            let mut left = &p.start;
            for st in &p.steps {
                let Step::Edge(e, right) = st else {
                    if let Step::Group(_, n) = st {
                        left = n;
                    }
                    continue;
                };
                for t in &e.types {
                    let Some(r) = self.ctx.schema.edge_name(&t.text) else {
                        continue;
                    };
                    let d = &self.ctx.schema.edges[r.edge];
                    if d.stored != "blocks" && d.stored != "gates" {
                        continue;
                    }
                    let dst = match (e.dir, r.forward) {
                        (Dir::Right, true) | (Dir::Left, false) => right,
                        (Dir::Left, true) | (Dir::Right, false) => left,
                        (Dir::Both, _) => continue,
                    };
                    if let Some(v) = &dst.var
                        && self.lookup(&v.text).is_some()
                    {
                        self.hand_derived.get_or_insert_with(|| v.text.clone());
                    }
                }
                left = right;
            }
        }
    }

    /// W07 (b): the `status` of a variable bound as the source of a `BLOCKS` or `GATES` edge.
    fn lint_status_of_blocker(&mut self, side: &Expr) {
        let ExprKind::Prop(x, name) = &side.kind else {
            return;
        };
        if name.text != "status" {
            return;
        }
        let ExprKind::Ident(v) = &x.kind else { return };
        let Some(id) = self.lookup(v) else { return };
        if let Some((_, task)) = self.blocks_sources.iter().find(|(s, _)| *s == id) {
            let task = task.clone();
            self.hand_derived.get_or_insert(task);
        }
    }

    /// Whether an expression reads the `anchor` handle of an `AT` edge variable.
    fn is_anchor_prop(&self, e: &Expr) -> bool {
        let ExprKind::Prop(x, name) = &e.kind else {
            return false;
        };
        if name.text != "anchor" {
            return false;
        }
        let ExprKind::Ident(v) = &x.kind else {
            return false;
        };
        self.lookup(v)
            .is_some_and(|id| self.b[id as usize].kind == BKind::Edge)
    }
}

/// The correctly rounded binary64 value of a float literal ([LQ/lexical §5.6]); −0.0 cannot be written.
fn float_value(text: &str) -> f64 {
    text.parse::<f64>().unwrap_or(0.0)
}
