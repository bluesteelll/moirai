//! The LQ syntax tree, the S-AST of [LQ/canonical-ast §3].
//!
//! The parser builds these nodes with the spelling normalisations of [LQ/canonical-ast §3.1] already applied. The
//! catalogue of §3.2 is followed field for field; where a Rust enum states one of §3.4's invariants (a `part` is either
//! a standalone call or clauses with a `RETURN`; `sresolve` is either a key or a query with `EXPECT`), the S-expression
//! form of [`crate::lq::sexpr`] maps it back to §3.2's fields. Every node carries a [`Span`]; spans compare equal, so
//! `==` on these types is the "`==` ignoring spans" of the printer property ([LQ/canonical-ast §3.4]).

use crate::lq::diag::Span;

/// A written name: an identifier, a label, a type, a property or field name, an alias. The text is decoded (a
/// back-quoted name without its quotes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    /// The decoded text.
    pub text: String,
    /// Where it was written.
    pub span: Span,
}

impl Name {
    /// A name with a span.
    pub fn new(text: impl Into<String>, span: Span) -> Name {
        Name {
            text: text.into(),
            span,
        }
    }
}

/// `read.mode` ([LQ/canonical-ast §3.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No prefix.
    Run,
    /// `EXPLAIN`.
    Explain,
    /// `PROFILE`.
    Profile,
}

/// The read root, from `read_input`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    /// `EXPLAIN`, `PROFILE` or neither.
    pub mode: Mode,
    /// The query.
    pub query: Query,
}

/// `query`: parts combined left to right by set operations; `ops` has one entry fewer than `parts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// The parts, at least one.
    pub parts: Vec<Part>,
    /// The set operations between consecutive parts.
    pub ops: Vec<SetOp>,
    /// The whole query.
    pub span: Span,
}

/// A set operation (`set_op`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOp {
    /// `UNION`.
    Union,
    /// `UNION ALL`.
    UnionAll,
    /// `EXCEPT`.
    Except,
    /// `INTERSECT`.
    Intersect,
}

/// `single_query`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The `USE` revision.
    pub use_: Option<Rev>,
    /// A standalone call, or clauses with a `RETURN`.
    pub body: PartBody,
    /// The part.
    pub span: Span,
}

/// The two forms of a `part` ([LQ/canonical-ast §3.2] notes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartBody {
    /// `standalone_call`: `clauses` empty, `return` absent.
    Call(SCall),
    /// `{ clause } return_clause`.
    Clauses {
        /// The reading clauses.
        clauses: Vec<Clause>,
        /// The `RETURN`.
        ret: Return,
    },
}

/// `standalone_call` with its `order_limit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SCall {
    /// The procedure name, segments joined with `.`.
    pub proc: Name,
    /// Arguments in written order.
    pub args: Vec<Arg>,
    /// No `YIELD`, `YIELD *` or `YIELD` items.
    pub yield_: YieldMode,
    /// `WHERE` (needs a `YIELD`).
    pub where_: Option<Expr>,
    /// `ORDER BY` keys.
    pub order: Vec<Sort>,
    /// `LIMIT`.
    pub limit: Option<Expr>,
    /// The call.
    pub span: Span,
}

/// `scall.yield` with its items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YieldMode {
    /// No `YIELD`.
    None,
    /// `YIELD *`.
    Star,
    /// `YIELD` items (non-empty).
    Items(Vec<YItem>),
}

/// A reading clause (`clause`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clause {
    /// `MATCH` or `OPTIONAL MATCH`.
    Match(Match),
    /// `CALL … YIELD …` as a clause.
    Call(Call),
    /// `UNWIND expr AS name`.
    Unwind(Unwind),
    /// `WITH`.
    With(With),
}

/// `match_clause`, `optional_clause`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// `OPTIONAL MATCH`.
    pub optional: bool,
    /// The match mode as written (absent for `OPTIONAL MATCH`).
    pub mode: Option<MatchMode>,
    /// The patterns.
    pub patterns: Vec<Path>,
    /// `WHERE`.
    pub where_: Option<Expr>,
    /// The clause.
    pub span: Span,
}

/// `match_mode`; `DIFFERENT RELATIONSHIPS` and `DIFFERENT EDGES` both give `Different`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    /// `WALK`.
    Walk,
    /// `TRAIL`.
    Trail,
    /// `ACYCLIC`.
    Acyclic,
    /// `SIMPLE`.
    Simple,
    /// `DIFFERENT RELATIONSHIPS` / `DIFFERENT EDGES`.
    Different,
}

/// `call_clause`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// The procedure name, segments joined with `.`.
    pub proc: Name,
    /// Arguments in written order.
    pub args: Vec<Arg>,
    /// The yield items (non-empty).
    pub yield_: Vec<YItem>,
    /// `WHERE` over the yielded rows.
    pub where_: Option<Expr>,
    /// The clause.
    pub span: Span,
}

/// `unwind_clause`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unwind {
    /// The list.
    pub expr: Expr,
    /// The new variable.
    pub as_: Name,
    /// The clause.
    pub span: Span,
}

/// `with_clause`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct With {
    /// `WITH DISTINCT`.
    pub distinct: bool,
    /// The leading `*`.
    pub star: bool,
    /// Projection items.
    pub items: Vec<Item>,
    /// `WHERE`.
    pub where_: Option<Expr>,
    /// `ORDER BY` keys.
    pub order: Vec<Sort>,
    /// `LIMIT`.
    pub limit: Option<Expr>,
    /// The clause.
    pub span: Span,
}

/// `return_clause`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Return {
    /// `RETURN DISTINCT` (`RETURN ALL` gives false).
    pub distinct: bool,
    /// The leading `*`.
    pub star: bool,
    /// Projection items.
    pub items: Vec<Item>,
    /// The `GROUP BY` list as written.
    pub group: Vec<Expr>,
    /// `ORDER BY` keys.
    pub order: Vec<Sort>,
    /// `LIMIT`.
    pub limit: Option<Expr>,
    /// The clause.
    pub span: Span,
}

/// `proj_item`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The expression.
    pub expr: Expr,
    /// `AS` alias.
    pub as_: Option<Name>,
}

/// One of `yield_items`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YItem {
    /// The field name (a plain name).
    pub name: Name,
    /// `AS` alias.
    pub as_: Option<Name>,
}

/// `sort_item`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sort {
    /// The key.
    pub expr: Expr,
    /// `DESC`/`DESCENDING`.
    pub desc: bool,
}

/// `arg`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arg {
    /// The argument's name (a named argument), case-sensitive.
    pub name: Option<Name>,
    /// The value.
    pub value: ArgVal,
}

/// `argval`: an expression, or a revision value at a revision position ([LQ/lexical §4.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgVal {
    /// An ordinary expression.
    Expr(Expr),
    /// A revspec read in revision mode.
    Rev(Rev),
    /// `a..b` or `a...b`.
    Range {
        /// The left revision.
        from: Rev,
        /// `..` or `...`.
        op: RangeOp,
        /// The right revision.
        to: Rev,
        /// The whole range.
        span: Span,
    },
    /// `[r, ...]`.
    List(Vec<Rev>, Span),
}

/// A range operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeOp {
    /// `..`.
    Two,
    /// `...`.
    Three,
}

/// A revision (`revspec`): the `rev` union of [LQ/canonical-ast §3.3].
///
/// A revision may carry any number of suffixes (`main~~~…`), each nesting the revision before it, so dropping,
/// cloning, comparing and printing walk the suffix chain in a loop ([LQ/grammar-v1.ebnf §P.13]); `Debug` prints the
/// S-expression of [LQ/canonical-ast §4.2].
pub struct Rev {
    /// Which revision.
    pub kind: RevKind,
    /// Where it was written.
    pub span: Span,
}

impl Rev {
    /// The revision under the outermost suffix, if any.
    fn take_base(&mut self) -> Option<Rev> {
        match &mut self.kind {
            RevKind::Suf(b, _) => Some(std::mem::replace(
                &mut **b,
                Rev {
                    kind: RevKind::Head,
                    span: Span::default(),
                },
            )),
            _ => None,
        }
    }
}

impl Drop for Rev {
    fn drop(&mut self) {
        let mut next = self.take_base();
        while let Some(mut r) = next {
            next = r.take_base();
        }
    }
}

impl Clone for Rev {
    fn clone(&self) -> Rev {
        let mut sufs = Vec::new();
        let mut cur = self;
        while let RevKind::Suf(b, s) = &cur.kind {
            sufs.push((s, cur.span));
            cur = b;
        }
        // `cur` has no suffix: its derived clone does not recurse.
        let mut acc = Rev {
            kind: cur.kind.clone(),
            span: cur.span,
        };
        for (s, span) in sufs.into_iter().rev() {
            acc = Rev {
                kind: RevKind::Suf(Box::new(acc), s.clone()),
                span,
            };
        }
        acc
    }
}

impl PartialEq for Rev {
    fn eq(&self, other: &Rev) -> bool {
        let (mut a, mut b) = (self, other);
        loop {
            match (&a.kind, &b.kind) {
                (RevKind::Suf(x, s), RevKind::Suf(y, t)) => {
                    if s != t {
                        return false;
                    }
                    a = x;
                    b = y;
                }
                // At most one side has a suffix here, so the derived comparison does not recurse.
                (x, y) => return x == y,
            }
        }
    }
}

impl Eq for Rev {}

impl std::fmt::Debug for Rev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::lq::sexpr::rev(self))
    }
}

/// The bases and suffixes of a revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevKind {
    /// `HEAD`.
    Head,
    /// A ref name.
    Ref(String),
    /// A commit literal: the hex digits without `c` (7 to 64).
    Commit(String),
    /// A sequence literal.
    Seq(u64),
    /// A `$param` revision.
    Param(String),
    /// A suffix applied to a base; suffixes nest left to right.
    Suf(Box<Rev>, Suffix),
}

/// A revision suffix ([LQ/lexical §7.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Suffix {
    /// `~n` (an omitted count is 1).
    Tilde(u32),
    /// `^n` (an omitted count is 1).
    Caret(u32),
    /// `@n` or `@{n}`.
    At(u32),
    /// `@<datetime>`, normalised to `YYYY-MM-DDTHH:MM:SSZ`.
    AtTime(String),
}

/// A pattern `path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    /// The first node pattern.
    pub start: NPat,
    /// The steps.
    pub steps: Vec<Step>,
    /// The path.
    pub span: Span,
}

/// A step of a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// An edge pattern and the node after it (`estep`).
    Edge(EPat, NPat),
    /// A quantified group and the node after it (`gstep`).
    Group(Group, NPat),
}

/// `node_pat`. A node literal `(#N)` gives `props` = [`id`: the literal].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NPat {
    /// The variable.
    pub var: Option<Name>,
    /// The label disjunction.
    pub labels: Vec<Name>,
    /// The property map (empty for `{}` and for none).
    pub props: Vec<Kv>,
    /// The inline `WHERE`.
    pub where_: Option<Expr>,
    /// The node pattern.
    pub span: Span,
}

/// Direction of an edge pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// `->`.
    Right,
    /// `<-`.
    Left,
    /// No arrowhead.
    Both,
}

/// `edge_pat` with its `edge_body`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EPat {
    /// The edge variable.
    pub var: Option<Name>,
    /// The written direction.
    pub dir: Dir,
    /// The type names as written.
    pub types: Vec<Name>,
    /// The quantifier (Cypher form inside the brackets or GQL form after the pattern).
    pub quant: Option<Quant>,
    /// The property map.
    pub props: Vec<Kv>,
    /// The inline `WHERE`.
    pub where_: Option<Expr>,
    /// The edge pattern.
    pub span: Span,
}

/// `group_pat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The group's path.
    pub path: Path,
    /// The group's `WHERE`.
    pub where_: Option<Expr>,
    /// The quantifier.
    pub quant: Quant,
    /// The group.
    pub span: Span,
}

/// A quantifier `quant(min, max)`; `max` absent is unbounded ([LQ/canonical-ast §3.1] item 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quant {
    /// Lower bound.
    pub min: u32,
    /// Upper bound; `None` is unbounded.
    pub max: Option<u32>,
}

/// One entry of a property map or map literal (`kv`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kv {
    /// The key (a plain name).
    pub key: Name,
    /// The value.
    pub value: Expr,
}

/// An expression node with its span.
///
/// Operator chains are loops in the grammar but trees as deep as the chain is long ([LQ/grammar-v1.ebnf §P.13]):
/// `1 + 1 + … + 1` nests in its left operand once per `+`. Every traversal therefore walks the operand an operator's
/// text starts with ([`Expr::first_operand`]) in a loop with a heap stack and recurses only into the other operands and
/// sub-expressions, whose depth the nesting limit of 64 bounds. `Drop`, `Clone` and `PartialEq` are written that way;
/// `Debug` prints the S-expression of [LQ/canonical-ast §4.2].
pub struct Expr {
    /// The node.
    pub kind: ExprKind,
    /// Where it was written.
    pub span: Span,
}

impl Expr {
    /// An expression node.
    pub fn new(kind: ExprKind, span: Span) -> Expr {
        Expr { kind, span }
    }

    /// The operand an operator's text starts with, in which left-associative chains nest: the left operand of a
    /// binary operator, the operand of a unary or postfix one. `None` for every other node.
    pub fn first_operand(&self) -> Option<&Expr> {
        match &self.kind {
            ExprKind::Or(l, _)
            | ExprKind::And(l, _)
            | ExprKind::Cmp(_, l, _)
            | ExprKind::In(l, _)
            | ExprKind::StrPred(_, l, _)
            | ExprKind::Arith(_, l, _) => Some(l),
            ExprKind::Not(x)
            | ExprKind::IsNull(_, x)
            | ExprKind::LabelTest(x, _)
            | ExprKind::Neg(x)
            | ExprKind::Prop(x, _) => Some(x),
            _ => None,
        }
    }

    /// The operator node `self` with its first operand replaced by `first` (see [`Expr::first_operand`]); the other
    /// operands are cloned. `None` when `self` is no operator.
    fn with_first_operand(&self, first: Expr) -> Option<Expr> {
        let a = Box::new(first);
        let kind = match &self.kind {
            ExprKind::Or(_, r) => ExprKind::Or(a, r.clone()),
            ExprKind::And(_, r) => ExprKind::And(a, r.clone()),
            ExprKind::Cmp(op, _, r) => ExprKind::Cmp(*op, a, r.clone()),
            ExprKind::In(_, r) => ExprKind::In(a, r.clone()),
            ExprKind::StrPred(op, _, r) => ExprKind::StrPred(*op, a, r.clone()),
            ExprKind::Arith(op, _, r) => ExprKind::Arith(*op, a, r.clone()),
            ExprKind::Not(_) => ExprKind::Not(a),
            ExprKind::IsNull(neg, _) => ExprKind::IsNull(*neg, a),
            ExprKind::LabelTest(_, labels) => ExprKind::LabelTest(a, labels.clone()),
            ExprKind::Neg(_) => ExprKind::Neg(a),
            ExprKind::Prop(_, name) => ExprKind::Prop(a, name.clone()),
            _ => return None,
        };
        Some(Expr::new(kind, self.span))
    }

    /// Moves the operands of a binary or unary node onto `out`, leaving `NULL` placeholders.
    fn take_operands(&mut self, out: &mut Vec<Expr>) {
        let mut take = |b: &mut Box<Expr>| {
            out.push(std::mem::replace(
                &mut **b,
                Expr::new(ExprKind::Null, Span::default()),
            ))
        };
        match &mut self.kind {
            ExprKind::Or(l, r)
            | ExprKind::And(l, r)
            | ExprKind::Cmp(_, l, r)
            | ExprKind::In(l, r)
            | ExprKind::StrPred(_, l, r)
            | ExprKind::Arith(_, l, r) => {
                take(l);
                take(r);
            }
            ExprKind::Not(x)
            | ExprKind::IsNull(_, x)
            | ExprKind::LabelTest(x, _)
            | ExprKind::Neg(x)
            | ExprKind::Prop(x, _) => take(x),
            _ => {}
        }
    }
}

/// Operator chains are loops in the grammar ([LQ/grammar-v1.ebnf §P.13]), so `1 + 1 + ... + 1` builds a tree as deep
/// as the chain is long; dropping it walks the operands with a heap stack instead of recursing.
impl Drop for Expr {
    fn drop(&mut self) {
        let mut stack = Vec::new();
        self.take_operands(&mut stack);
        while let Some(mut e) = stack.pop() {
            e.take_operands(&mut stack);
        }
    }
}

impl Clone for Expr {
    fn clone(&self) -> Expr {
        let mut spine = Vec::new();
        let mut cur = self;
        while let Some(x) = cur.first_operand() {
            spine.push(cur);
            cur = x;
        }
        // `cur` is no operator: its derived clone reaches sub-expressions only through nesting, which is bounded.
        let mut acc = Expr::new(cur.kind.clone(), cur.span);
        for node in spine.into_iter().rev() {
            acc = match node.with_first_operand(acc) {
                Some(e) => e,
                None => unreachable!("the spine holds operators only"),
            };
        }
        acc
    }
}

/// `==` ignoring spans ([LQ/canonical-ast §3.4]).
impl PartialEq for Expr {
    fn eq(&self, other: &Expr) -> bool {
        let mut stack: Vec<(&Expr, &Expr)> = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            use ExprKind::*;
            match (&a.kind, &b.kind) {
                (Or(l1, r1), Or(l2, r2))
                | (And(l1, r1), And(l2, r2))
                | (In(l1, r1), In(l2, r2)) => {
                    stack.push((r1, r2));
                    stack.push((l1, l2));
                }
                (Cmp(o1, l1, r1), Cmp(o2, l2, r2)) if o1 == o2 => {
                    stack.push((r1, r2));
                    stack.push((l1, l2));
                }
                (StrPred(o1, l1, r1), StrPred(o2, l2, r2)) if o1 == o2 => {
                    stack.push((r1, r2));
                    stack.push((l1, l2));
                }
                (Arith(o1, l1, r1), Arith(o2, l2, r2)) if o1 == o2 => {
                    stack.push((r1, r2));
                    stack.push((l1, l2));
                }
                (Not(x1), Not(x2)) | (Neg(x1), Neg(x2)) => stack.push((x1, x2)),
                (IsNull(n1, x1), IsNull(n2, x2)) if n1 == n2 => stack.push((x1, x2)),
                (LabelTest(x1, l1), LabelTest(x2, l2)) if l1 == l2 => stack.push((x1, x2)),
                (Prop(x1, n1), Prop(x2, n2)) if n1 == n2 => stack.push((x1, x2)),
                // Operators that differ in kind or in a field; the derived comparison below is for other nodes only.
                _ if a.first_operand().is_some() || b.first_operand().is_some() => return false,
                (k1, k2) => {
                    if k1 != k2 {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl Eq for Expr {}

impl std::fmt::Debug for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::lq::sexpr::expr(self))
    }
}

/// Comparison operators; `!=` gives `Ne`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    /// `=`.
    Eq,
    /// `<>` (and `!=`).
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
}

impl CmpOp {
    /// The operator's spelling (`<>` for inequality).
    pub fn as_str(self) -> &'static str {
        match self {
            CmpOp::Eq => "=",
            CmpOp::Ne => "<>",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }
}

/// `STARTS WITH`, `ENDS WITH`, `CONTAINS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrOp {
    /// `STARTS WITH`.
    Starts,
    /// `ENDS WITH`.
    Ends,
    /// `CONTAINS`.
    Contains,
}

/// Arithmetic operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArithOp {
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
}

impl ArithOp {
    /// The operator's spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ArithOp::Add => "+",
            ArithOp::Sub => "-",
            ArithOp::Mul => "*",
            ArithOp::Div => "/",
        }
    }
}

/// `all`, `any`, `none`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListPredKind {
    /// `all(…)`.
    All,
    /// `any(…)`.
    Any,
    /// `none(…)`.
    None,
}

/// The `expr` union of [LQ/canonical-ast §3.3].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprKind {
    /// `l OR r`.
    Or(Box<Expr>, Box<Expr>),
    /// `l AND r`.
    And(Box<Expr>, Box<Expr>),
    /// `NOT e`; `x NOT IN y` gives `Not(In(x, y))`.
    Not(Box<Expr>),
    /// A comparison.
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    /// `e IS NULL` (`neg` false) or `e IS NOT NULL` (`neg` true).
    IsNull(bool, Box<Expr>),
    /// `l IN r`.
    In(Box<Expr>, Box<Expr>),
    /// `STARTS WITH`, `ENDS WITH`, `CONTAINS`.
    StrPred(StrOp, Box<Expr>, Box<Expr>),
    /// `e:a|b`.
    LabelTest(Box<Expr>, Vec<Name>),
    /// Arithmetic, left-associative.
    Arith(ArithOp, Box<Expr>, Box<Expr>),
    /// Unary `-`.
    Neg(Box<Expr>),
    /// `e.name`.
    Prop(Box<Expr>, Name),
    /// An identifier in expression position.
    Ident(String),
    /// `$name` (the name without `$`).
    Param(String),
    /// `#N`.
    Nid(u32),
    /// `#u:` and 32 lower-case hex digits (the digits).
    Uid(String),
    /// An integer literal.
    Int(i64),
    /// A float literal as written.
    Float(String),
    /// A string literal, decoded.
    Str(String),
    /// A duration literal as written.
    Dur(String),
    /// `TRUE` / `FALSE`.
    Bool(bool),
    /// `NULL`.
    Null,
    /// `EXISTS { … }`, a pattern predicate, `exists(path)`.
    Exists(Box<Sub>),
    /// `COUNT { … }`, `size(path)`.
    CountSub(Box<Sub>),
    /// A generic function call, name as written.
    Fn {
        /// The name.
        name: Name,
        /// `DISTINCT`.
        distinct: bool,
        /// Arguments.
        args: Vec<Arg>,
    },
    /// `count( * )`.
    CountStar,
    /// `all|any|none(x IN list WHERE pred)`.
    ListPred {
        /// Which predicate.
        kind: ListPredKind,
        /// The variable.
        var: Name,
        /// The list.
        list: Box<Expr>,
        /// The predicate.
        pred: Box<Expr>,
    },
    /// `[e, ...]`.
    List(Vec<Expr>),
    /// `{k: v, ...}`.
    Map(Vec<Kv>),
    /// `CASE`.
    Case {
        /// The subject of a simple `CASE`.
        subject: Option<Box<Expr>>,
        /// The arms.
        whens: Vec<When>,
        /// `ELSE`.
        else_: Option<Box<Expr>>,
    },
}

/// One `WHEN … THEN …` arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct When {
    /// The condition (or the value compared with the subject).
    pub cond: Expr,
    /// The result.
    pub then: Expr,
}

/// `subquery`: the `sub` union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sub {
    /// Clause form (`subq`).
    Clauses {
        /// The clauses.
        clauses: Vec<Clause>,
        /// The optional `RETURN`.
        ret: Option<Return>,
    },
    /// Pattern form (`subp`).
    Patterns {
        /// The patterns.
        patterns: Vec<Path>,
        /// `WHERE`.
        where_: Option<Expr>,
    },
}

/// The write root, from `write_input` (`tx`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tx {
    /// `ON revspec`.
    pub on: Option<Rev>,
    /// `IF TIP revspec`.
    pub if_tip: Option<Rev>,
    /// `IF TARGETS 'digest'`.
    pub if_targets: Option<String>,
    /// `KEY 'k'`.
    pub key: Option<String>,
    /// `LEASE 'L-n'`.
    pub lease: Option<String>,
    /// `MESSAGE 'text'`.
    pub message: Option<String>,
    /// The statements (non-empty).
    pub stmts: Vec<Stmt>,
    /// `DRY`.
    pub dry: bool,
    /// The block.
    pub span: Span,
}

/// `tx_stmt`: the `stmt` union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stmt {
    /// `MATCH … EXPECT … mutation…` (`smatch`).
    Match(SMatch),
    /// A mutation list (`smuts`).
    Muts(Vec<Mut>, Span),
    /// Node creation (`screate`).
    Create(Create),
    /// `CALL tx.name(…) [YIELD …]` (`stxcall`, the name without `tx.`).
    TxCall {
        /// The mutation name without the `tx.` prefix.
        name: Name,
        /// Arguments.
        args: Vec<Arg>,
        /// Yield items.
        yield_: Vec<YItem>,
        /// The statement.
        span: Span,
    },
    /// `ASSERT expr [ELSE 'text']`.
    Assert {
        /// The assertion.
        expr: Expr,
        /// The `ELSE` text.
        else_: Option<String>,
        /// The statement.
        span: Span,
    },
    /// `RESOLVE`.
    Resolve(Resolve),
    /// `DEFINE QUERY`.
    Define(Define),
    /// `DROP QUERY name`.
    Drop(Name),
}

/// `smatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SMatch {
    /// The patterns.
    pub patterns: Vec<Path>,
    /// `WHERE`.
    pub where_: Option<Expr>,
    /// `EXPECT`.
    pub expect: Expect,
    /// The mutations.
    pub muts: Vec<Mut>,
    /// The statement.
    pub span: Span,
}

/// `expect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expect {
    /// `n`.
    Exact(i64),
    /// `m..n`.
    Range(i64, i64),
    /// `<= n`.
    Le(i64),
    /// `>= n`.
    Ge(i64),
    /// `$p`.
    Param(Name),
}

/// A mutation target (`target`: `ident nid uid param`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Which target.
    pub kind: TargetKind,
    /// Where it was written.
    pub span: Span,
}

/// The `target` union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// A variable.
    Ident(String),
    /// `#N`.
    Nid(u32),
    /// `#u:…` (the 32 hex digits).
    Uid(String),
    /// `$p`.
    Param(String),
}

/// `mutation`: the `mut` union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mut {
    /// `SET t.p = v, …` (`mset`).
    Set(Vec<Assign>, Span),
    /// `REMOVE t.p, …` (`mremove`).
    Remove(Vec<TProp>, Span),
    /// `DELETE t, … opts` (`mdelete`); options in written order.
    Delete {
        /// Targets.
        targets: Vec<Target>,
        /// Options.
        opts: Vec<DOpt>,
        /// The mutation.
        span: Span,
    },
    /// `MOVE t UNDER u [BEFORE|AFTER r | FIRST | LAST]` (`mmove`).
    Move {
        /// The moved node.
        target: Target,
        /// The new parent.
        under: Target,
        /// The position.
        pos: Option<MovePos>,
        /// The mutation.
        span: Span,
    },
    /// `CREATE (a)-[:T]->(b)` (`medge`); `INSERT` gives the same node.
    Edge(MEdge),
    /// `REOPEN t REASON e` (`mreopen`).
    Reopen {
        /// The node.
        target: Target,
        /// The reason.
        reason: Expr,
        /// The mutation.
        span: Span,
    },
    /// `PATCH t.f REMOVE e ADD e` (`mpatch`).
    Patch {
        /// The node.
        target: Target,
        /// The field.
        field: Name,
        /// Removed text.
        remove: Expr,
        /// Added text.
        add: Expr,
        /// The mutation.
        span: Span,
    },
}

/// `assign`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assign {
    /// The node.
    pub target: Target,
    /// The property.
    pub prop: Name,
    /// The value.
    pub value: Expr,
}

/// `tprop`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TProp {
    /// The node.
    pub target: Target,
    /// The property.
    pub prop: Name,
}

/// `delete_opt`: the `dopt` union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DOpt {
    /// `POLICY …`.
    Policy(Policy),
    /// `REPLACED BY t`.
    Replaced(Target),
    /// `RELEASE`.
    Release,
    /// `REASON e`.
    Reason(Expr),
}

/// `POLICY` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// `RESTRICT`.
    Restrict,
    /// `CASCADE`.
    Cascade,
    /// `REPARENT`.
    Reparent,
}

/// `MOVE` positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MovePos {
    /// `BEFORE t`.
    Before(Target),
    /// `AFTER t`.
    After(Target),
    /// `FIRST`.
    First,
    /// `LAST`.
    Last,
}

/// Direction of a created edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeDir {
    /// `-[…]->`.
    Right,
    /// `<-[…]-`.
    Left,
}

/// `medge`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MEdge {
    /// The left target.
    pub src: Target,
    /// The written direction.
    pub dir: EdgeDir,
    /// The type as written.
    pub ty: Name,
    /// Edge properties.
    pub props: Vec<Kv>,
    /// The right target.
    pub dst: Target,
    /// The mutation.
    pub span: Span,
}

/// `screate`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Create {
    /// The created node's variable.
    pub var: Name,
    /// Its kind as written.
    pub label: Name,
    /// Its property map.
    pub props: Vec<Kv>,
    /// Edges to or from it.
    pub edges: Vec<CEdge>,
    /// `UNDER t`.
    pub under: Option<Target>,
    /// `UNLESS EXISTS { … }`.
    pub unless: Option<Sub>,
    /// The statement.
    pub span: Span,
}

/// `cedge`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CEdge {
    /// The written direction.
    pub dir: EdgeDir,
    /// The type as written.
    pub ty: Name,
    /// Edge properties.
    pub props: Vec<Kv>,
    /// The other end.
    pub target: Target,
}

/// `sresolve`: either `key`, or `query` with `expect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolve {
    /// The conflict key or the query that lists keys.
    pub what: ResolveWhat,
    /// The choice.
    pub take: Take,
    /// The statement.
    pub span: Span,
}

/// What a `RESOLVE` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveWhat {
    /// `RESOLVE 'key'`.
    Key(String),
    /// `RESOLVE (query) EXPECT e`.
    Query(Box<Query>, Expect),
}

/// `TAKE` choices; `value` exactly for `Value`, `target` exactly for `Repoint`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Take {
    /// `OURS`.
    Ours,
    /// `THEIRS`.
    Theirs,
    /// `BASE`.
    Base,
    /// `VALUE e`.
    Value(Expr),
    /// `REPOINT t`.
    Repoint(Target),
}

/// `define_stmt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Define {
    /// The qname joined with `.`.
    pub name: Name,
    /// Parameter declarations.
    pub params: Vec<PDecl>,
    /// `SHAPE word`.
    pub shape: Option<Name>,
    /// `BUDGET word`.
    pub budget: Option<Name>,
    /// The body.
    pub body: Query,
    /// The statement.
    pub span: Span,
}

/// `param_decl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PDecl {
    /// The name without `$`.
    pub name: Name,
    /// The type.
    pub ty: PType,
    /// `?`.
    pub optional: bool,
    /// `= literal` (`= NULL` gives the `Null` literal); absent when no `=` was written.
    pub default: Option<Expr>,
}

/// `type`: `word [< word >]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PType {
    /// The type word.
    pub name: Name,
    /// The argument word.
    pub arg: Option<Name>,
}
