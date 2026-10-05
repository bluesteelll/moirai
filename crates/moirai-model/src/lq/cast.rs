//! The canonical AST (C-AST) of [LQ/canonical-ast §5]–§6: its node types, its binary encoding (§6) and its
//! S-expression form for fixtures (§4.3).
//!
//! Variables are held as binder-assigned [`BindingId`]s. Their indexes are assigned while the tree is written, exactly
//! as §5.7 states: the first time the writer meets a field that refers to a binding, the binding takes the root's
//! counter and the counter increases. The encoder and the S-expression renderer are one traversal over a [`Sink`], so
//! the two forms always number variables alike. The query hash `H` is BLAKE3-128 of [`encode`]'s bytes ([§7.1]); the
//! hash itself is computed by the model's evaluator (WP-93b), whose dependencies include `blake3`.

use crate::lq::lexer::json_string;

/// A binding as the binder numbers it; the encoding renumbers bindings by first appearance ([LQ/canonical-ast §5.7]).
pub type BindingId = u32;

/// A node uid ([F01 §5.6] `b16`).
pub type Uid = [u8; 16];

/// A full commit id ([F01 §5.6] `b32`).
pub type CommitId = [u8; 32];

/// `QUERY` (0x01): the first part and the rest with their set operations.
#[derive(Clone, Debug, PartialEq)]
pub struct CQuery {
    /// The first part.
    pub first: CPart,
    /// (op, part): op 1 `union`, 2 `union_all`, 3 `except`, 4 `intersect` ([§6.4]).
    pub rest: Vec<(u8, CPart)>,
}

/// `PART` (0x02).
#[derive(Clone, Debug, PartialEq)]
pub struct CPart {
    /// The view (`USE`, or `--at`): a revision node or `PARAM`.
    pub use_: Option<CExpr>,
    /// `CLAUSES` or `SCALL`.
    pub body: CBody,
}

/// The body of a part.
#[derive(Clone, Debug, PartialEq)]
pub enum CBody {
    /// `CLAUSES` (0x03).
    Clauses(Vec<CClause>, CReturn),
    /// `SCALL` (0x04).
    Call(CSCall),
}

/// `SCALL` (0x04).
#[derive(Clone, Debug, PartialEq)]
pub struct CSCall {
    /// The resolved callee.
    pub proc: String,
    /// Arguments, named and ordered by the callee's signature (N2).
    pub args: Vec<CArg>,
    /// 0 `none`, 1 `star`, 2 `items`.
    pub ymode: u8,
    /// Yield items.
    pub items: Vec<CYield>,
    /// `WHERE`.
    pub where_: Option<CExpr>,
    /// `ORDER BY`.
    pub order: Vec<CSort>,
    /// `LIMIT`.
    pub limit: Option<CExpr>,
}

/// A reading clause.
#[derive(Clone, Debug, PartialEq)]
pub enum CClause {
    /// `MATCH` (0x10).
    Match {
        /// `OPTIONAL MATCH`.
        optional: bool,
        /// Patterns.
        patterns: Vec<CPath>,
        /// `WHERE`.
        where_: Option<CExpr>,
    },
    /// `CALL` (0x11).
    Call {
        /// The resolved callee.
        proc: String,
        /// Arguments.
        args: Vec<CArg>,
        /// Yield items.
        items: Vec<CYield>,
        /// `WHERE`.
        where_: Option<CExpr>,
    },
    /// `UNWIND` (0x12).
    Unwind {
        /// The list.
        expr: CExpr,
        /// The new binding.
        var: BindingId,
    },
    /// `WITH` (0x13).
    With {
        /// `DISTINCT`.
        distinct: bool,
        /// `*`.
        star: bool,
        /// `WITEM`s: (expression, binding).
        items: Vec<(CExpr, BindingId)>,
        /// `WHERE`.
        where_: Option<CExpr>,
        /// `ORDER BY`.
        order: Vec<CSort>,
        /// `LIMIT`.
        limit: Option<CExpr>,
    },
}

/// `RETURN` (0x14).
#[derive(Clone, Debug, PartialEq)]
pub struct CReturn {
    /// `DISTINCT`.
    pub distinct: bool,
    /// `*`.
    pub star: bool,
    /// `RITEM`s: (expression, alias).
    pub items: Vec<(CExpr, Option<String>)>,
    /// `ORDER BY`.
    pub order: Vec<CSort>,
    /// `LIMIT`.
    pub limit: Option<CExpr>,
}

/// `YIELD` (0x17).
#[derive(Clone, Debug, PartialEq)]
pub struct CYield {
    /// The column.
    pub field: String,
    /// The new binding.
    pub var: BindingId,
}

/// `SORT` (0x18).
#[derive(Clone, Debug, PartialEq)]
pub struct CSort {
    /// The key.
    pub expr: CExpr,
    /// Descending.
    pub desc: bool,
}

/// `ARG` (0x19).
#[derive(Clone, Debug, PartialEq)]
pub struct CArg {
    /// The parameter name.
    pub name: Option<String>,
    /// The value.
    pub value: CExpr,
}

/// `PATH` (0x20).
#[derive(Clone, Debug, PartialEq)]
pub struct CPath {
    /// The first node.
    pub start: CNode,
    /// The steps.
    pub steps: Vec<CStep>,
}

/// `NODEP` (0x21).
#[derive(Clone, Debug, PartialEq)]
pub struct CNode {
    /// The binding, absent for an anonymous node.
    pub var: Option<BindingId>,
    /// Kind names.
    pub labels: Vec<String>,
    /// Property map.
    pub props: Vec<(String, CExpr)>,
    /// Inline `WHERE`.
    pub where_: Option<CExpr>,
}

/// A step.
#[derive(Clone, Debug, PartialEq)]
pub enum CStep {
    /// `ESTEP` (0x22).
    Edge(CEdgeP, CNode),
    /// `GSTEP` (0x23).
    Group(CGroup, CNode),
}

/// `EDGEP` (0x24).
#[derive(Clone, Debug, PartialEq)]
pub struct CEdgeP {
    /// The binding.
    pub var: Option<BindingId>,
    /// 0 `typed`, else the direction of an any-kind pattern: 1 `right`, 2 `left`, 3 `both`.
    pub dir: u8,
    /// (`lq_name`, effective direction 1 `right`, 2 `left`, 3 `both`).
    pub types: Vec<(String, u8)>,
    /// (min, max).
    pub quant: Option<(u32, Option<u32>)>,
    /// Property map.
    pub props: Vec<(String, CExpr)>,
    /// Inline `WHERE`.
    pub where_: Option<CExpr>,
}

/// `GROUP` (0x25).
#[derive(Clone, Debug, PartialEq)]
pub struct CGroup {
    /// The group's path.
    pub path: CPath,
    /// Its `WHERE`.
    pub where_: Option<CExpr>,
    /// (min, max).
    pub quant: (u32, Option<u32>),
}

/// A subquery.
#[derive(Clone, Debug, PartialEq)]
pub enum CSub {
    /// `SUBC` (0x40).
    Clauses(Vec<CClause>, Option<CReturn>),
    /// `SUBP` (0x41).
    Patterns(Vec<CPath>, Option<CExpr>),
}

/// An expression, constant or revision node ([LQ/canonical-ast §6.3]).
///
/// Like the S-AST's operator chains ([`crate::lq::ast::Expr`]), the C-AST's are as deep as the chain is long, and so is
/// a revision's suffix chain; `Drop`, `Clone`, `PartialEq` and the writer walk the first operand in a loop
/// ([LQ/grammar-v1.ebnf §P.13]). `Debug` prints the S-expression of §4.3 with the binder's binding ids.
pub enum CExpr {
    /// `OR` (0x30).
    Or(Box<CExpr>, Box<CExpr>),
    /// `AND` (0x31).
    And(Box<CExpr>, Box<CExpr>),
    /// `NOT` (0x32).
    Not(Box<CExpr>),
    /// `CMP` (0x33): op 1–6 = `=` `<>` `<` `<=` `>` `>=`.
    Cmp(u8, Box<CExpr>, Box<CExpr>),
    /// `ISNULL` (0x34).
    IsNull(bool, Box<CExpr>),
    /// `IN` (0x35).
    In(Box<CExpr>, Box<CExpr>),
    /// `STRPRED` (0x36): op 1 `starts`, 2 `ends`, 3 `contains`.
    StrPred(u8, Box<CExpr>, Box<CExpr>),
    /// `LABELTEST` (0x37).
    LabelTest(Box<CExpr>, Vec<String>),
    /// `ARITH` (0x38): op 1–4 = `+` `-` `*` `/`.
    Arith(u8, Box<CExpr>, Box<CExpr>),
    /// `NEG` (0x39).
    Neg(Box<CExpr>),
    /// `PROP` (0x3A).
    Prop(Box<CExpr>, String),
    /// `VAR` (0x3B).
    Var(BindingId),
    /// `PARAM` (0x3C): a parameter of the enclosing definition.
    Param(u32),
    /// `ITEMREF` (0x3D).
    ItemRef(u32),
    /// `EXISTS` (0x3E).
    Exists(Box<CSub>),
    /// `COUNTSUB` (0x3F).
    CountSub(Box<CSub>),
    /// `FUNC` (0x42).
    Func(String, bool, Vec<CArg>),
    /// `COUNTSTAR` (0x43).
    CountStar,
    /// `LISTPRED` (0x44): kind 1 `all`, 2 `any`, 3 `none`.
    ListPred(u8, BindingId, Box<CExpr>, Box<CExpr>),
    /// `LIST` (0x45).
    List(Vec<CExpr>),
    /// `MAP` (0x46).
    Map(Vec<(String, CExpr)>),
    /// `CASE` (0x47): subject, (when, then) arms, else.
    Case(Option<Box<CExpr>>, Vec<(CExpr, CExpr)>, Option<Box<CExpr>>),
    /// `NULL` (0x50).
    Null,
    /// `BOOL` (0x51).
    Bool(bool),
    /// `INT` (0x52).
    Int(i64),
    /// `FLOAT` (0x53): never a NaN, never −0.0.
    Float(f64),
    /// `TEXT` (0x54).
    Text(String),
    /// `DURATION` (0x55): milliseconds.
    Duration(i64),
    /// `TIMESTAMP` (0x56): milliseconds since the Unix epoch, UTC.
    Timestamp(i64),
    /// `NODE` (0x57).
    Node(Uid),
    /// `ENUM` (0x58).
    Enum(String),
    /// `RANGEINT` (0x59).
    RangeInt(Option<i64>, Option<i64>),
    /// `RHEAD` (0x60).
    RHead,
    /// `RREF` (0x61).
    RRef(String),
    /// `RCOMMIT` (0x62).
    RCommit(CommitId),
    /// `RSUF` (0x63): kind 1 `tilde`, 2 `caret`, 3 `at`, 4 `attime`; n the count or the milliseconds.
    RSuf(Box<CExpr>, u8, i64),
    /// `RRANGE` (0x64): op 1 `two`, 2 `three`.
    RRange(Box<CExpr>, u8, Box<CExpr>),
    /// `RLIST` (0x65).
    RList(Vec<CExpr>),
}

impl CExpr {
    fn take_operands(&mut self, out: &mut Vec<CExpr>) {
        let mut take = |b: &mut Box<CExpr>| out.push(std::mem::replace(&mut **b, CExpr::Null));
        match self {
            CExpr::Or(l, r)
            | CExpr::And(l, r)
            | CExpr::Cmp(_, l, r)
            | CExpr::In(l, r)
            | CExpr::StrPred(_, l, r)
            | CExpr::Arith(_, l, r)
            | CExpr::RRange(l, _, r) => {
                take(l);
                take(r);
            }
            CExpr::Not(x)
            | CExpr::IsNull(_, x)
            | CExpr::LabelTest(x, _)
            | CExpr::Neg(x)
            | CExpr::Prop(x, _)
            | CExpr::RSuf(x, _, _) => take(x),
            _ => {}
        }
    }

    /// The operand the node's encoding writes first, in which chains nest: the left operand of a binary operator, the
    /// operand of a unary or postfix one, the base of a revision suffix. `None` for every other node.
    pub fn first_operand(&self) -> Option<&CExpr> {
        match self {
            CExpr::Or(l, _)
            | CExpr::And(l, _)
            | CExpr::Cmp(_, l, _)
            | CExpr::In(l, _)
            | CExpr::StrPred(_, l, _)
            | CExpr::Arith(_, l, _) => Some(l),
            CExpr::Not(x)
            | CExpr::IsNull(_, x)
            | CExpr::LabelTest(x, _)
            | CExpr::Neg(x)
            | CExpr::Prop(x, _)
            | CExpr::RSuf(x, _, _) => Some(x),
            _ => None,
        }
    }

    /// The node with its first operand replaced by `first`, the rest cloned; `None` when it has none.
    fn with_first_operand(&self, first: CExpr) -> Option<CExpr> {
        let a = Box::new(first);
        Some(match self {
            CExpr::Or(_, r) => CExpr::Or(a, r.clone()),
            CExpr::And(_, r) => CExpr::And(a, r.clone()),
            CExpr::Cmp(op, _, r) => CExpr::Cmp(*op, a, r.clone()),
            CExpr::In(_, r) => CExpr::In(a, r.clone()),
            CExpr::StrPred(op, _, r) => CExpr::StrPred(*op, a, r.clone()),
            CExpr::Arith(op, _, r) => CExpr::Arith(*op, a, r.clone()),
            CExpr::Not(_) => CExpr::Not(a),
            CExpr::IsNull(neg, _) => CExpr::IsNull(*neg, a),
            CExpr::LabelTest(_, labels) => CExpr::LabelTest(a, labels.clone()),
            CExpr::Neg(_) => CExpr::Neg(a),
            CExpr::Prop(_, name) => CExpr::Prop(a, name.clone()),
            CExpr::RSuf(_, kind, n) => CExpr::RSuf(a, *kind, *n),
            _ => return None,
        })
    }

    /// A copy of a node that has no first operand, its children cloned (their own chains in their own loops).
    fn clone_leaf(&self) -> CExpr {
        match self {
            CExpr::Var(b) => CExpr::Var(*b),
            CExpr::Param(i) => CExpr::Param(*i),
            CExpr::ItemRef(i) => CExpr::ItemRef(*i),
            CExpr::Exists(s) => CExpr::Exists(s.clone()),
            CExpr::CountSub(s) => CExpr::CountSub(s.clone()),
            CExpr::Func(n, d, a) => CExpr::Func(n.clone(), *d, a.clone()),
            CExpr::CountStar => CExpr::CountStar,
            CExpr::ListPred(k, v, l, p) => CExpr::ListPred(*k, *v, l.clone(), p.clone()),
            CExpr::List(v) => CExpr::List(v.clone()),
            CExpr::Map(v) => CExpr::Map(v.clone()),
            CExpr::Case(s, w, e) => CExpr::Case(s.clone(), w.clone(), e.clone()),
            CExpr::Null => CExpr::Null,
            CExpr::Bool(b) => CExpr::Bool(*b),
            CExpr::Int(n) => CExpr::Int(*n),
            CExpr::Float(f) => CExpr::Float(*f),
            CExpr::Text(s) => CExpr::Text(s.clone()),
            CExpr::Duration(n) => CExpr::Duration(*n),
            CExpr::Timestamp(n) => CExpr::Timestamp(*n),
            CExpr::Node(u) => CExpr::Node(*u),
            CExpr::Enum(s) => CExpr::Enum(s.clone()),
            CExpr::RangeInt(a, b) => CExpr::RangeInt(*a, *b),
            CExpr::RHead => CExpr::RHead,
            CExpr::RRef(s) => CExpr::RRef(s.clone()),
            CExpr::RCommit(c) => CExpr::RCommit(*c),
            CExpr::RRange(a, op, b) => CExpr::RRange(a.clone(), *op, b.clone()),
            CExpr::RList(v) => CExpr::RList(v.clone()),
            // Nodes with a first operand are rebuilt by `Clone::clone` around their clone.
            CExpr::Or(..)
            | CExpr::And(..)
            | CExpr::Not(_)
            | CExpr::Cmp(..)
            | CExpr::IsNull(..)
            | CExpr::In(..)
            | CExpr::StrPred(..)
            | CExpr::LabelTest(..)
            | CExpr::Arith(..)
            | CExpr::Neg(_)
            | CExpr::Prop(..)
            | CExpr::RSuf(..) => unreachable!("clone_leaf takes nodes without a first operand"),
        }
    }

    /// Whether two nodes without a first operand are equal, their children compared by `==` (their own loops).
    fn leaf_eq(&self, other: &CExpr) -> bool {
        match (self, other) {
            (CExpr::Var(a), CExpr::Var(b)) => a == b,
            (CExpr::Param(a), CExpr::Param(b)) | (CExpr::ItemRef(a), CExpr::ItemRef(b)) => a == b,
            (CExpr::Exists(a), CExpr::Exists(b)) | (CExpr::CountSub(a), CExpr::CountSub(b)) => {
                a == b
            }
            (CExpr::Func(n1, d1, a1), CExpr::Func(n2, d2, a2)) => n1 == n2 && d1 == d2 && a1 == a2,
            (CExpr::CountStar, CExpr::CountStar)
            | (CExpr::Null, CExpr::Null)
            | (CExpr::RHead, CExpr::RHead) => true,
            (CExpr::ListPred(k1, v1, l1, p1), CExpr::ListPred(k2, v2, l2, p2)) => {
                k1 == k2 && v1 == v2 && l1 == l2 && p1 == p2
            }
            (CExpr::List(a), CExpr::List(b)) | (CExpr::RList(a), CExpr::RList(b)) => a == b,
            (CExpr::Map(a), CExpr::Map(b)) => a == b,
            (CExpr::Case(s1, w1, e1), CExpr::Case(s2, w2, e2)) => s1 == s2 && w1 == w2 && e1 == e2,
            (CExpr::Bool(a), CExpr::Bool(b)) => a == b,
            (CExpr::Int(a), CExpr::Int(b))
            | (CExpr::Duration(a), CExpr::Duration(b))
            | (CExpr::Timestamp(a), CExpr::Timestamp(b)) => a == b,
            (CExpr::Float(a), CExpr::Float(b)) => a == b,
            (CExpr::Text(a), CExpr::Text(b))
            | (CExpr::Enum(a), CExpr::Enum(b))
            | (CExpr::RRef(a), CExpr::RRef(b)) => a == b,
            (CExpr::Node(a), CExpr::Node(b)) => a == b,
            (CExpr::RangeInt(a1, b1), CExpr::RangeInt(a2, b2)) => a1 == a2 && b1 == b2,
            (CExpr::RCommit(a), CExpr::RCommit(b)) => a == b,
            (CExpr::RRange(a1, o1, b1), CExpr::RRange(a2, o2, b2)) => {
                o1 == o2 && a1 == a2 && b1 == b2
            }
            _ => false,
        }
    }
}

impl Clone for CExpr {
    fn clone(&self) -> CExpr {
        let mut spine = Vec::new();
        let mut cur = self;
        while let Some(x) = cur.first_operand() {
            spine.push(cur);
            cur = x;
        }
        let mut acc = cur.clone_leaf();
        for node in spine.into_iter().rev() {
            acc = match node.with_first_operand(acc) {
                Some(e) => e,
                None => unreachable!("the spine holds nodes with a first operand only"),
            };
        }
        acc
    }
}

impl PartialEq for CExpr {
    fn eq(&self, other: &CExpr) -> bool {
        let mut stack: Vec<(&CExpr, &CExpr)> = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            use CExpr::*;
            match (a, b) {
                (Or(l1, r1), Or(l2, r2))
                | (And(l1, r1), And(l2, r2))
                | (In(l1, r1), In(l2, r2)) => {
                    stack.push((r1, r2));
                    stack.push((l1, l2));
                }
                (Cmp(o1, l1, r1), Cmp(o2, l2, r2))
                | (StrPred(o1, l1, r1), StrPred(o2, l2, r2))
                | (Arith(o1, l1, r1), Arith(o2, l2, r2))
                    if o1 == o2 =>
                {
                    stack.push((r1, r2));
                    stack.push((l1, l2));
                }
                (Not(x1), Not(x2)) | (Neg(x1), Neg(x2)) => stack.push((x1, x2)),
                (IsNull(n1, x1), IsNull(n2, x2)) if n1 == n2 => stack.push((x1, x2)),
                (LabelTest(x1, l1), LabelTest(x2, l2)) if l1 == l2 => stack.push((x1, x2)),
                (Prop(x1, n1), Prop(x2, n2)) if n1 == n2 => stack.push((x1, x2)),
                (RSuf(x1, k1, n1), RSuf(x2, k2, n2)) if k1 == k2 && n1 == n2 => {
                    stack.push((x1, x2))
                }
                _ if a.first_operand().is_some() || b.first_operand().is_some() => return false,
                _ => {
                    if !a.leaf_eq(b) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl std::fmt::Debug for CExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut w = Writer::new(Text(String::new()));
        w.renumber = false;
        w.expr(self);
        f.write_str(&w.sink.0)
    }
}

/// Operator chains give trees as deep as the chain is long; dropping walks them with a heap stack
/// ([LQ/grammar-v1.ebnf §P.13]).
impl Drop for CExpr {
    fn drop(&mut self) {
        let mut stack = Vec::new();
        self.take_operands(&mut stack);
        while let Some(mut e) = stack.pop() {
            e.take_operands(&mut stack);
        }
    }
}

/// `TX` (0x70).
#[derive(Clone, Debug, PartialEq)]
pub struct CTx {
    /// `IF TIP`.
    pub if_tip: Option<CExpr>,
    /// `IF TARGETS`.
    pub if_targets: Option<String>,
    /// `MESSAGE`.
    pub message: Option<String>,
    /// The statements.
    pub stmts: Vec<CStmt>,
}

/// A statement of a `TX` block.
#[derive(Clone, Debug, PartialEq)]
pub enum CStmt {
    /// `SMATCH` (0x71).
    Match {
        /// Patterns.
        patterns: Vec<CPath>,
        /// `WHERE`.
        where_: Option<CExpr>,
        /// `EXPECT` (min, max).
        expect: (u64, Option<u64>),
        /// Mutations.
        muts: Vec<CMut>,
    },
    /// `SMUTS` (0x72).
    Muts(Vec<CMut>),
    /// `SCREATE` (0x73).
    Create {
        /// The created node's binding.
        var: BindingId,
        /// Its kind.
        kind: String,
        /// Its properties.
        props: Vec<(String, CExpr)>,
        /// The created edges.
        edges: Vec<CreatedEdge>,
        /// `UNDER`.
        under: Option<CExpr>,
        /// `UNLESS EXISTS`.
        unless: Option<CSub>,
    },
    /// `STXCALL` (0x74).
    TxCall {
        /// `tx.<name>`.
        name: String,
        /// Arguments.
        args: Vec<CArg>,
        /// Yield items.
        items: Vec<CYield>,
    },
    /// `SASSERT` (0x75).
    Assert {
        /// The assertion.
        expr: CExpr,
        /// The `ELSE` text.
        else_: Option<String>,
    },
    /// `SRESOLVE` (0x76).
    Resolve {
        /// The key.
        key: CResolveKey,
        /// 1 `ours`, 2 `theirs`, 3 `base`, 4 `value`, 5 `repoint`, 6 `drop` ([LQ/canonical-ast §6.4]).
        take: u8,
        /// The value (take 4) or the target (take 5).
        operand: Option<CExpr>,
    },
    /// `DEFINE` (0x90).
    Define(CDefine),
    /// `SDROP` (0x78).
    Drop(String),
}

/// An edge of `SCREATE`: (dir 1 `out` / 2 `in`, `lq_name`, properties, target).
pub type CreatedEdge = (u8, String, Vec<(String, CExpr)>, CExpr);

/// The key of a `RESOLVE`.
#[derive(Clone, Debug, PartialEq)]
pub enum CResolveKey {
    /// A `TEXT` key.
    Text(String),
    /// `RESOLVEQ` (0x77): the query and its `EXPECT`.
    Query(Box<CQuery>, (u64, Option<u64>)),
}

/// A mutation.
#[derive(Clone, Debug, PartialEq)]
pub enum CMut {
    /// `MSET` (0x80): (target, property, value).
    Set(Vec<(CExpr, String, CExpr)>),
    /// `MREMOVE` (0x81): (target, property).
    Remove(Vec<(CExpr, String)>),
    /// `MDELETE` (0x82).
    Delete {
        /// Targets.
        targets: Vec<CExpr>,
        /// 0 `none`, 1 `restrict`, 2 `cascade`, 3 `reparent`.
        policy: u8,
        /// `REPLACED BY`.
        replaced_by: Option<CExpr>,
        /// `RELEASE`.
        release: bool,
        /// `REASON`.
        reason: Option<CExpr>,
    },
    /// `MMOVE` (0x83).
    Move {
        /// The moved node.
        target: CExpr,
        /// The new parent.
        under: CExpr,
        /// 0 `none`, 1 `before`, 2 `after`, 3 `first`, 4 `last`.
        pos: u8,
        /// The relative node.
        rel: Option<CExpr>,
    },
    /// `MEDGE` (0x84): (source, `lq_name`, properties, destination) in the stored direction.
    Edge(CExpr, String, Vec<(String, CExpr)>, CExpr),
    /// `MREOPEN` (0x85).
    Reopen(CExpr, CExpr),
    /// `MPATCH` (0x86).
    Patch(CExpr, String, CExpr, CExpr),
}

/// `DEFINE` (0x90).
#[derive(Clone, Debug, PartialEq)]
pub struct CDefine {
    /// The query name.
    pub name: String,
    /// `PDECL`s.
    pub params: Vec<CPDecl>,
    /// `SHAPE`, lower case.
    pub shape: Option<String>,
    /// `BUDGET`, lower case.
    pub budget: Option<String>,
    /// The body.
    pub body: CQuery,
}

/// `PDECL` (0x91).
#[derive(Clone, Debug, PartialEq)]
pub struct CPDecl {
    /// The name without `$`.
    pub name: String,
    /// `TYPE` (0x92): name and argument, lower case.
    pub ty: (String, Option<String>),
    /// `?`.
    pub optional: bool,
    /// The default constant.
    pub default: Option<CExpr>,
}

/// A root of the C-AST ([LQ/canonical-ast §5.9]).
#[derive(Clone, Copy, Debug)]
pub enum Root<'a> {
    /// R1, R2: a `QUERY`.
    Query(&'a CQuery),
    /// R3–R5: a `TX`.
    Tx(&'a CTx),
    /// R6: a `DEFINE`.
    Define(&'a CDefine),
}

/// The header's domain-separation string ([LQ/canonical-ast §6.1]).
pub const MAGIC: &[u8; 16] = b"moirai-lq-ast-v1";

/// The grammar version of this canonical-AST algorithm.
pub const LQ_VERSION: u16 = 1;

/// The binary encoding of a root: the 22-byte header, then the root node ([LQ/canonical-ast §6]).
pub fn encode(root: Root<'_>) -> Vec<u8> {
    let mut w = Writer::new(Bytes(Vec::with_capacity(256)));
    w.sink
        .0
        .extend_from_slice(&(MAGIC.len() as u32).to_le_bytes());
    w.sink.0.extend_from_slice(MAGIC);
    w.sink.0.extend_from_slice(&LQ_VERSION.to_le_bytes());
    w.root(root);
    w.sink.0
}

/// The S-expression of a root ([LQ/canonical-ast §4.3]).
pub fn sexpr(root: Root<'_>) -> String {
    let mut w = Writer::new(Text(String::with_capacity(256)));
    w.root(root);
    w.sink.0
}

/// The receiver of the one traversal: bytes (§6) or text (§4.3).
trait Sink {
    fn tag(&mut self, tag: u8, name: &str);
    fn end(&mut self);
    fn code(&mut self, v: u8, atom: &str);
    fn bool8(&mut self, v: bool);
    fn u32(&mut self, v: u32);
    fn u64(&mut self, v: u64);
    fn i64(&mut self, v: i64);
    fn f64(&mut self, v: f64);
    fn str(&mut self, s: &str);
    fn b16(&mut self, b: &Uid);
    fn b32(&mut self, b: &CommitId);
    fn none(&mut self);
    fn some(&mut self);
    fn list(&mut self, n: usize);
    fn list_end(&mut self);
    fn tuple(&mut self);
    fn tuple_end(&mut self);
}

struct Bytes(Vec<u8>);

impl Sink for Bytes {
    fn tag(&mut self, tag: u8, _: &str) {
        self.0.push(tag);
    }
    fn end(&mut self) {}
    fn code(&mut self, v: u8, _: &str) {
        self.0.push(v);
    }
    fn bool8(&mut self, v: bool) {
        self.0.push(u8::from(v));
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn b16(&mut self, b: &Uid) {
        self.0.extend_from_slice(b);
    }
    fn b32(&mut self, b: &CommitId) {
        self.0.extend_from_slice(b);
    }
    fn none(&mut self) {
        self.0.push(0);
    }
    fn some(&mut self) {
        self.0.push(1);
    }
    fn list(&mut self, n: usize) {
        self.u32(n as u32);
    }
    fn list_end(&mut self) {}
    fn tuple(&mut self) {}
    fn tuple_end(&mut self) {}
}

struct Text(String);

impl Text {
    fn token(&mut self, t: &str) {
        if !self.0.is_empty() && !self.0.ends_with(['(', '[']) {
            self.0.push(' ');
        }
        self.0.push_str(t);
    }
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

impl Sink for Text {
    fn tag(&mut self, _: u8, name: &str) {
        self.token(&format!("({name}"));
    }
    fn end(&mut self) {
        self.0.push(')');
    }
    fn code(&mut self, _: u8, atom: &str) {
        self.token(atom);
    }
    fn bool8(&mut self, v: bool) {
        self.token(if v { "true" } else { "false" });
    }
    fn u32(&mut self, v: u32) {
        self.token(&v.to_string());
    }
    fn u64(&mut self, v: u64) {
        self.token(&v.to_string());
    }
    fn i64(&mut self, v: i64) {
        self.token(&v.to_string());
    }
    fn f64(&mut self, v: f64) {
        self.token(&format!("\"{:016x}\"", v.to_bits()));
    }
    fn str(&mut self, s: &str) {
        self.token(&json_string(s));
    }
    fn b16(&mut self, b: &Uid) {
        self.token(&format!("\"{}\"", hex(b)));
    }
    fn b32(&mut self, b: &CommitId) {
        self.token(&format!("\"{}\"", hex(b)));
    }
    fn none(&mut self) {
        self.token("_");
    }
    fn some(&mut self) {}
    fn list(&mut self, _: usize) {
        self.token("[");
    }
    fn list_end(&mut self) {
        self.0.push(']');
    }
    fn tuple(&mut self) {
        self.token("[");
    }
    fn tuple_end(&mut self) {
        self.0.push(']');
    }
}

/// The traversal: fields of §6.3 in order, variables numbered by first appearance (§5.7).
struct Writer<S: Sink> {
    sink: S,
    index: Vec<u32>,
    next: u32,
    /// Number bindings by first appearance (§5.7); off, a `VAR` shows the binder's id (`Debug`).
    renumber: bool,
}

const CMP_ATOMS: [&str; 7] = ["", "=", "<>", "<", "<=", ">", ">="];
const ARITH_ATOMS: [&str; 5] = ["", "+", "-", "*", "/"];
const SETOP_ATOMS: [&str; 5] = ["", "union", "union_all", "except", "intersect"];
const STR_ATOMS: [&str; 4] = ["", "starts", "ends", "contains"];
const LISTPRED_ATOMS: [&str; 4] = ["", "all", "any", "none"];
const RSUF_ATOMS: [&str; 5] = ["", "tilde", "caret", "at", "attime"];
const RRANGE_ATOMS: [&str; 3] = ["", "two", "three"];
const YMODE_ATOMS: [&str; 3] = ["none", "star", "items"];
const EDGEDIR_ATOMS: [&str; 4] = ["typed", "right", "left", "both"];
const CREATEDIR_ATOMS: [&str; 3] = ["", "out", "in"];
const TAKE_ATOMS: [&str; 7] = ["", "ours", "theirs", "base", "value", "repoint", "drop"];
const POLICY_ATOMS: [&str; 5] = ["none", "restrict", "cascade", "reparent", "reassign"];
const POS_ATOMS: [&str; 5] = ["none", "before", "after", "first", "last"];

fn atom(table: &'static [&'static str], v: u8) -> &'static str {
    table.get(v as usize).copied().unwrap_or("")
}

impl<S: Sink> Writer<S> {
    fn new(sink: S) -> Writer<S> {
        Writer {
            sink,
            index: Vec::new(),
            next: 0,
            renumber: true,
        }
    }

    fn root(&mut self, root: Root<'_>) {
        match root {
            Root::Query(q) => self.query(q),
            Root::Tx(t) => self.tx(t),
            Root::Define(d) => self.define(d),
        }
    }

    fn var(&mut self, b: BindingId) {
        if !self.renumber {
            self.sink.u32(b);
            return;
        }
        let b = b as usize;
        if self.index.len() <= b {
            self.index.resize(b + 1, u32::MAX);
        }
        if self.index[b] == u32::MAX {
            self.index[b] = self.next;
            self.next += 1;
        }
        let i = self.index[b];
        self.sink.u32(i);
    }

    fn opt_var(&mut self, b: Option<BindingId>) {
        match b {
            Some(b) => {
                self.sink.some();
                self.var(b);
            }
            None => self.sink.none(),
        }
    }

    fn opt_expr(&mut self, e: &Option<CExpr>) {
        match e {
            Some(e) => {
                self.sink.some();
                self.expr(e);
            }
            None => self.sink.none(),
        }
    }

    fn opt_str(&mut self, s: &Option<String>) {
        match s {
            Some(s) => {
                self.sink.some();
                self.sink.str(s);
            }
            None => self.sink.none(),
        }
    }

    fn strs(&mut self, v: &[String]) {
        self.sink.list(v.len());
        for s in v {
            self.sink.str(s);
        }
        self.sink.list_end();
    }

    fn exprs(&mut self, v: &[CExpr]) {
        self.sink.list(v.len());
        for e in v {
            self.expr(e);
        }
        self.sink.list_end();
    }

    fn kvs(&mut self, v: &[(String, CExpr)]) {
        self.sink.list(v.len());
        for (k, e) in v {
            self.sink.tuple();
            self.sink.str(k);
            self.expr(e);
            self.sink.tuple_end();
        }
        self.sink.list_end();
    }

    fn quant(&mut self, q: (u32, Option<u32>)) {
        self.sink.tag(0x26, "QUANT");
        self.sink.u32(q.0);
        match q.1 {
            Some(m) => {
                self.sink.some();
                self.sink.u32(m);
            }
            None => self.sink.none(),
        }
        self.sink.end();
    }

    fn query(&mut self, q: &CQuery) {
        self.sink.tag(0x01, "QUERY");
        self.part(&q.first);
        self.sink.list(q.rest.len());
        for (op, p) in &q.rest {
            self.sink.tuple();
            self.sink.code(*op, atom(&SETOP_ATOMS, *op));
            self.part(p);
            self.sink.tuple_end();
        }
        self.sink.list_end();
        self.sink.end();
    }

    fn part(&mut self, p: &CPart) {
        self.sink.tag(0x02, "PART");
        self.opt_expr(&p.use_);
        match &p.body {
            CBody::Clauses(clauses, ret) => {
                self.sink.tag(0x03, "CLAUSES");
                self.clauses(clauses);
                self.ret(ret);
                self.sink.end();
            }
            CBody::Call(c) => self.scall(c),
        }
        self.sink.end();
    }

    fn clauses(&mut self, v: &[CClause]) {
        self.sink.list(v.len());
        for c in v {
            self.clause(c);
        }
        self.sink.list_end();
    }

    fn scall(&mut self, c: &CSCall) {
        self.sink.tag(0x04, "SCALL");
        self.sink.str(&c.proc);
        self.args(&c.args);
        self.sink.code(c.ymode, atom(&YMODE_ATOMS, c.ymode));
        self.yields(&c.items);
        self.opt_expr(&c.where_);
        self.sorts(&c.order);
        self.opt_expr(&c.limit);
        self.sink.end();
    }

    fn args(&mut self, v: &[CArg]) {
        self.sink.list(v.len());
        for a in v {
            self.sink.tag(0x19, "ARG");
            self.opt_str(&a.name);
            self.expr(&a.value);
            self.sink.end();
        }
        self.sink.list_end();
    }

    fn yields(&mut self, v: &[CYield]) {
        self.sink.list(v.len());
        for y in v {
            self.sink.tag(0x17, "YIELD");
            self.sink.str(&y.field);
            self.var(y.var);
            self.sink.end();
        }
        self.sink.list_end();
    }

    fn sorts(&mut self, v: &[CSort]) {
        self.sink.list(v.len());
        for s in v {
            self.sink.tag(0x18, "SORT");
            self.expr(&s.expr);
            self.sink.bool8(s.desc);
            self.sink.end();
        }
        self.sink.list_end();
    }

    fn clause(&mut self, c: &CClause) {
        match c {
            CClause::Match {
                optional,
                patterns,
                where_,
            } => {
                self.sink.tag(0x10, "MATCH");
                self.sink.bool8(*optional);
                self.paths(patterns);
                self.opt_expr(where_);
            }
            CClause::Call {
                proc,
                args,
                items,
                where_,
            } => {
                self.sink.tag(0x11, "CALL");
                self.sink.str(proc);
                self.args(args);
                self.yields(items);
                self.opt_expr(where_);
            }
            CClause::Unwind { expr, var } => {
                self.sink.tag(0x12, "UNWIND");
                self.expr(expr);
                self.var(*var);
            }
            CClause::With {
                distinct,
                star,
                items,
                where_,
                order,
                limit,
            } => {
                self.sink.tag(0x13, "WITH");
                self.sink.bool8(*distinct);
                self.sink.bool8(*star);
                self.sink.list(items.len());
                for (e, v) in items {
                    self.sink.tag(0x15, "WITEM");
                    self.expr(e);
                    self.var(*v);
                    self.sink.end();
                }
                self.sink.list_end();
                self.opt_expr(where_);
                self.sorts(order);
                self.opt_expr(limit);
            }
        }
        self.sink.end();
    }

    fn ret(&mut self, r: &CReturn) {
        self.sink.tag(0x14, "RETURN");
        self.sink.bool8(r.distinct);
        self.sink.bool8(r.star);
        self.sink.list(r.items.len());
        for (e, alias) in &r.items {
            self.sink.tag(0x16, "RITEM");
            self.expr(e);
            self.opt_str(alias);
            self.sink.end();
        }
        self.sink.list_end();
        self.sorts(&r.order);
        self.opt_expr(&r.limit);
        self.sink.end();
    }

    fn paths(&mut self, v: &[CPath]) {
        self.sink.list(v.len());
        for p in v {
            self.path(p);
        }
        self.sink.list_end();
    }

    fn path(&mut self, p: &CPath) {
        self.sink.tag(0x20, "PATH");
        self.node(&p.start);
        self.sink.list(p.steps.len());
        for s in &p.steps {
            match s {
                CStep::Edge(e, n) => {
                    self.sink.tag(0x22, "ESTEP");
                    self.edge(e);
                    self.node(n);
                }
                CStep::Group(g, n) => {
                    self.sink.tag(0x23, "GSTEP");
                    self.sink.tag(0x25, "GROUP");
                    self.path(&g.path);
                    self.opt_expr(&g.where_);
                    self.quant(g.quant);
                    self.sink.end();
                    self.node(n);
                }
            }
            self.sink.end();
        }
        self.sink.list_end();
        self.sink.end();
    }

    fn node(&mut self, n: &CNode) {
        self.sink.tag(0x21, "NODEP");
        self.opt_var(n.var);
        self.strs(&n.labels);
        self.kvs(&n.props);
        self.opt_expr(&n.where_);
        self.sink.end();
    }

    fn edge(&mut self, e: &CEdgeP) {
        self.sink.tag(0x24, "EDGEP");
        self.opt_var(e.var);
        self.sink.code(e.dir, atom(&EDGEDIR_ATOMS, e.dir));
        self.sink.list(e.types.len());
        for (name, d) in &e.types {
            self.sink.tuple();
            self.sink.str(name);
            self.sink.code(*d, atom(&EDGEDIR_ATOMS, *d));
            self.sink.tuple_end();
        }
        self.sink.list_end();
        match e.quant {
            Some(q) => {
                self.sink.some();
                self.quant(q);
            }
            None => self.sink.none(),
        }
        self.kvs(&e.props);
        self.opt_expr(&e.where_);
        self.sink.end();
    }

    fn sub(&mut self, s: &CSub) {
        match s {
            CSub::Clauses(clauses, ret) => {
                self.sink.tag(0x40, "SUBC");
                self.clauses(clauses);
                match ret {
                    Some(r) => {
                        self.sink.some();
                        self.ret(r);
                    }
                    None => self.sink.none(),
                }
            }
            CSub::Patterns(p, w) => {
                self.sink.tag(0x41, "SUBP");
                self.paths(p);
                self.opt_expr(w);
            }
        }
        self.sink.end();
    }

    /// An expression node, pre-order. The first operand of each node ([`CExpr::first_operand`]) is walked in a loop:
    /// the heads of the chain's nodes are written top-down, then the innermost node, then the rest of each node
    /// bottom-up. Only the other operands recurse, and their depth is bounded by nesting ([LQ/grammar-v1.ebnf §P.13]).
    fn expr(&mut self, e: &CExpr) {
        let mut spine = Vec::new();
        let mut cur = e;
        while let Some(x) = cur.first_operand() {
            self.expr_head(cur);
            spine.push(cur);
            cur = x;
        }
        self.leaf(cur);
        while let Some(node) = spine.pop() {
            self.expr_tail(node);
        }
    }

    /// The tag and the fields a node writes before its first operand.
    fn expr_head(&mut self, e: &CExpr) {
        match e {
            CExpr::Or(..) => self.sink.tag(0x30, "OR"),
            CExpr::And(..) => self.sink.tag(0x31, "AND"),
            CExpr::Not(_) => self.sink.tag(0x32, "NOT"),
            CExpr::Cmp(op, _, _) => {
                self.sink.tag(0x33, "CMP");
                self.sink.code(*op, atom(&CMP_ATOMS, *op));
            }
            CExpr::IsNull(neg, _) => {
                self.sink.tag(0x34, "ISNULL");
                self.sink.bool8(*neg);
            }
            CExpr::In(..) => self.sink.tag(0x35, "IN"),
            CExpr::StrPred(op, _, _) => {
                self.sink.tag(0x36, "STRPRED");
                self.sink.code(*op, atom(&STR_ATOMS, *op));
            }
            CExpr::LabelTest(..) => self.sink.tag(0x37, "LABELTEST"),
            CExpr::Arith(op, _, _) => {
                self.sink.tag(0x38, "ARITH");
                self.sink.code(*op, atom(&ARITH_ATOMS, *op));
            }
            CExpr::Neg(_) => self.sink.tag(0x39, "NEG"),
            CExpr::Prop(..) => self.sink.tag(0x3A, "PROP"),
            CExpr::RSuf(..) => self.sink.tag(0x63, "RSUF"),
            _ => {}
        }
    }

    /// The fields a node writes after its first operand, and its end.
    fn expr_tail(&mut self, e: &CExpr) {
        match e {
            CExpr::Or(_, r)
            | CExpr::And(_, r)
            | CExpr::In(_, r)
            | CExpr::Cmp(_, _, r)
            | CExpr::StrPred(_, _, r)
            | CExpr::Arith(_, _, r) => self.expr(r),
            CExpr::LabelTest(_, labels) => self.strs(labels),
            CExpr::Prop(_, name) => self.sink.str(name),
            CExpr::RSuf(_, kind, n) => {
                self.sink.code(*kind, atom(&RSUF_ATOMS, *kind));
                self.sink.i64(*n);
            }
            _ => {}
        }
        self.sink.end();
    }

    /// A node without a first operand.
    fn leaf(&mut self, e: &CExpr) {
        match e {
            CExpr::Var(b) => {
                self.sink.tag(0x3B, "VAR");
                self.var(*b);
                self.sink.end();
            }
            CExpr::Param(i) => {
                self.sink.tag(0x3C, "PARAM");
                self.sink.u32(*i);
                self.sink.end();
            }
            CExpr::ItemRef(i) => {
                self.sink.tag(0x3D, "ITEMREF");
                self.sink.u32(*i);
                self.sink.end();
            }
            CExpr::Exists(s) => {
                self.sink.tag(0x3E, "EXISTS");
                self.sub(s);
                self.sink.end();
            }
            CExpr::CountSub(s) => {
                self.sink.tag(0x3F, "COUNTSUB");
                self.sub(s);
                self.sink.end();
            }
            CExpr::Func(name, distinct, args) => {
                self.sink.tag(0x42, "FUNC");
                self.sink.str(name);
                self.sink.bool8(*distinct);
                self.args(args);
                self.sink.end();
            }
            CExpr::CountStar => {
                self.sink.tag(0x43, "COUNTSTAR");
                self.sink.end();
            }
            CExpr::ListPred(kind, var, list, pred) => {
                self.sink.tag(0x44, "LISTPRED");
                self.sink.code(*kind, atom(&LISTPRED_ATOMS, *kind));
                self.var(*var);
                self.expr(list);
                self.expr(pred);
                self.sink.end();
            }
            CExpr::List(v) => {
                self.sink.tag(0x45, "LIST");
                self.exprs(v);
                self.sink.end();
            }
            CExpr::Map(v) => {
                self.sink.tag(0x46, "MAP");
                self.kvs(v);
                self.sink.end();
            }
            CExpr::Case(subject, whens, else_) => {
                self.sink.tag(0x47, "CASE");
                match subject {
                    Some(s) => {
                        self.sink.some();
                        self.expr(s);
                    }
                    None => self.sink.none(),
                }
                self.sink.list(whens.len());
                for (w, t) in whens {
                    self.sink.tuple();
                    self.expr(w);
                    self.expr(t);
                    self.sink.tuple_end();
                }
                self.sink.list_end();
                match else_ {
                    Some(s) => {
                        self.sink.some();
                        self.expr(s);
                    }
                    None => self.sink.none(),
                }
                self.sink.end();
            }
            CExpr::Null => {
                self.sink.tag(0x50, "NULL");
                self.sink.end();
            }
            CExpr::Bool(v) => {
                self.sink.tag(0x51, "BOOL");
                self.sink.bool8(*v);
                self.sink.end();
            }
            CExpr::Int(v) => {
                self.sink.tag(0x52, "INT");
                self.sink.i64(*v);
                self.sink.end();
            }
            CExpr::Float(v) => {
                self.sink.tag(0x53, "FLOAT");
                self.sink.f64(*v);
                self.sink.end();
            }
            CExpr::Text(s) => {
                self.sink.tag(0x54, "TEXT");
                self.sink.str(s);
                self.sink.end();
            }
            CExpr::Duration(v) => {
                self.sink.tag(0x55, "DURATION");
                self.sink.i64(*v);
                self.sink.end();
            }
            CExpr::Timestamp(v) => {
                self.sink.tag(0x56, "TIMESTAMP");
                self.sink.i64(*v);
                self.sink.end();
            }
            CExpr::Node(u) => {
                self.sink.tag(0x57, "NODE");
                self.sink.b16(u);
                self.sink.end();
            }
            CExpr::Enum(s) => {
                self.sink.tag(0x58, "ENUM");
                self.sink.str(s);
                self.sink.end();
            }
            CExpr::RangeInt(lo, hi) => {
                self.sink.tag(0x59, "RANGEINT");
                for b in [lo, hi] {
                    match b {
                        Some(v) => {
                            self.sink.some();
                            self.sink.i64(*v);
                        }
                        None => self.sink.none(),
                    }
                }
                self.sink.end();
            }
            CExpr::RHead => {
                self.sink.tag(0x60, "RHEAD");
                self.sink.end();
            }
            CExpr::RRef(s) => {
                self.sink.tag(0x61, "RREF");
                self.sink.str(s);
                self.sink.end();
            }
            CExpr::RCommit(id) => {
                self.sink.tag(0x62, "RCOMMIT");
                self.sink.b32(id);
                self.sink.end();
            }
            CExpr::RRange(a, op, b) => {
                self.sink.tag(0x64, "RRANGE");
                self.expr(a);
                self.sink.code(*op, atom(&RRANGE_ATOMS, *op));
                self.expr(b);
                self.sink.end();
            }
            CExpr::RList(v) => {
                self.sink.tag(0x65, "RLIST");
                self.exprs(v);
                self.sink.end();
            }
            // Written by `expr` around their first operand.
            CExpr::Or(..)
            | CExpr::And(..)
            | CExpr::Not(_)
            | CExpr::Cmp(..)
            | CExpr::IsNull(..)
            | CExpr::In(..)
            | CExpr::StrPred(..)
            | CExpr::LabelTest(..)
            | CExpr::Arith(..)
            | CExpr::Neg(_)
            | CExpr::Prop(..)
            | CExpr::RSuf(..) => unreachable!("leaf takes nodes without a first operand"),
        }
    }

    fn expect(&mut self, e: (u64, Option<u64>)) {
        self.sink.tag(0x87, "EXPECT");
        self.sink.u64(e.0);
        match e.1 {
            Some(m) => {
                self.sink.some();
                self.sink.u64(m);
            }
            None => self.sink.none(),
        }
        self.sink.end();
    }

    fn tx(&mut self, t: &CTx) {
        self.sink.tag(0x70, "TX");
        self.opt_expr(&t.if_tip);
        self.opt_str(&t.if_targets);
        self.opt_str(&t.message);
        self.sink.list(t.stmts.len());
        for s in &t.stmts {
            self.stmt(s);
        }
        self.sink.list_end();
        self.sink.end();
    }

    fn stmt(&mut self, s: &CStmt) {
        match s {
            CStmt::Match {
                patterns,
                where_,
                expect,
                muts,
            } => {
                self.sink.tag(0x71, "SMATCH");
                self.paths(patterns);
                self.opt_expr(where_);
                self.expect(*expect);
                self.muts(muts);
                self.sink.end();
            }
            CStmt::Muts(m) => {
                self.sink.tag(0x72, "SMUTS");
                self.muts(m);
                self.sink.end();
            }
            CStmt::Create {
                var,
                kind,
                props,
                edges,
                under,
                unless,
            } => {
                self.sink.tag(0x73, "SCREATE");
                self.var(*var);
                self.sink.str(kind);
                self.kvs(props);
                self.sink.list(edges.len());
                for (dir, name, props, target) in edges {
                    self.sink.tuple();
                    self.sink.code(*dir, atom(&CREATEDIR_ATOMS, *dir));
                    self.sink.str(name);
                    self.kvs(props);
                    self.expr(target);
                    self.sink.tuple_end();
                }
                self.sink.list_end();
                self.opt_expr(under);
                match unless {
                    Some(s) => {
                        self.sink.some();
                        self.sub(s);
                    }
                    None => self.sink.none(),
                }
                self.sink.end();
            }
            CStmt::TxCall { name, args, items } => {
                self.sink.tag(0x74, "STXCALL");
                self.sink.str(name);
                self.args(args);
                self.yields(items);
                self.sink.end();
            }
            CStmt::Assert { expr, else_ } => {
                self.sink.tag(0x75, "SASSERT");
                self.expr(expr);
                self.opt_str(else_);
                self.sink.end();
            }
            CStmt::Resolve { key, take, operand } => {
                self.sink.tag(0x76, "SRESOLVE");
                match key {
                    CResolveKey::Text(k) => self.expr(&CExpr::Text(k.clone())),
                    CResolveKey::Query(q, e) => {
                        self.sink.tag(0x77, "RESOLVEQ");
                        self.query(q);
                        self.expect(*e);
                        self.sink.end();
                    }
                }
                self.sink.code(*take, atom(&TAKE_ATOMS, *take));
                self.opt_expr(operand);
                self.sink.end();
            }
            CStmt::Define(d) => self.define(d),
            CStmt::Drop(name) => {
                self.sink.tag(0x78, "SDROP");
                self.sink.str(name);
                self.sink.end();
            }
        }
    }

    fn muts(&mut self, v: &[CMut]) {
        self.sink.list(v.len());
        for m in v {
            self.mutation(m);
        }
        self.sink.list_end();
    }

    fn mutation(&mut self, m: &CMut) {
        match m {
            CMut::Set(assigns) => {
                self.sink.tag(0x80, "MSET");
                self.sink.list(assigns.len());
                for (t, p, v) in assigns {
                    self.sink.tuple();
                    self.expr(t);
                    self.sink.str(p);
                    self.expr(v);
                    self.sink.tuple_end();
                }
                self.sink.list_end();
            }
            CMut::Remove(items) => {
                self.sink.tag(0x81, "MREMOVE");
                self.sink.list(items.len());
                for (t, p) in items {
                    self.sink.tuple();
                    self.expr(t);
                    self.sink.str(p);
                    self.sink.tuple_end();
                }
                self.sink.list_end();
            }
            CMut::Delete {
                targets,
                policy,
                replaced_by,
                release,
                reason,
            } => {
                self.sink.tag(0x82, "MDELETE");
                self.exprs(targets);
                self.sink.code(*policy, atom(&POLICY_ATOMS, *policy));
                self.opt_expr(replaced_by);
                self.sink.bool8(*release);
                self.opt_expr(reason);
            }
            CMut::Move {
                target,
                under,
                pos,
                rel,
            } => {
                self.sink.tag(0x83, "MMOVE");
                self.expr(target);
                self.expr(under);
                self.sink.code(*pos, atom(&POS_ATOMS, *pos));
                self.opt_expr(rel);
            }
            CMut::Edge(src, kind, props, dst) => {
                self.sink.tag(0x84, "MEDGE");
                self.expr(src);
                self.sink.str(kind);
                self.kvs(props);
                self.expr(dst);
            }
            CMut::Reopen(t, r) => {
                self.sink.tag(0x85, "MREOPEN");
                self.expr(t);
                self.expr(r);
            }
            CMut::Patch(t, field, remove, add) => {
                self.sink.tag(0x86, "MPATCH");
                self.expr(t);
                self.sink.str(field);
                self.expr(remove);
                self.expr(add);
            }
        }
        self.sink.end();
    }

    fn define(&mut self, d: &CDefine) {
        self.sink.tag(0x90, "DEFINE");
        self.sink.str(&d.name);
        self.sink.list(d.params.len());
        for p in &d.params {
            self.sink.tag(0x91, "PDECL");
            self.sink.str(&p.name);
            self.sink.tag(0x92, "TYPE");
            self.sink.str(&p.ty.0);
            self.opt_str(&p.ty.1);
            self.sink.end();
            self.sink.bool8(p.optional);
            self.opt_expr(&p.default);
            self.sink.end();
        }
        self.sink.list_end();
        self.opt_str(&d.shape);
        self.opt_str(&d.budget);
        self.query(&d.body);
        self.sink.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The C-AST of [LQ/canonical-ast §4.4], built by hand with arbitrary binding ids.
    fn example() -> CQuery {
        let v = || Box::new(CExpr::Var(7));
        CQuery {
            first: CPart {
                use_: None,
                body: CBody::Clauses(
                    vec![CClause::Match {
                        optional: false,
                        patterns: vec![CPath {
                            start: CNode {
                                var: Some(7),
                                labels: vec!["task".into()],
                                props: vec![],
                                where_: None,
                            },
                            steps: vec![],
                        }],
                        where_: Some(CExpr::And(
                            Box::new(CExpr::Cmp(
                                1,
                                Box::new(CExpr::Prop(v(), "status".into())),
                                Box::new(CExpr::Enum("open".into())),
                            )),
                            Box::new(CExpr::Cmp(
                                4,
                                Box::new(CExpr::Prop(v(), "priority".into())),
                                Box::new(CExpr::Int(1)),
                            )),
                        )),
                    }],
                    CReturn {
                        distinct: false,
                        star: false,
                        items: vec![(CExpr::Var(7), None)],
                        order: vec![CSort {
                            expr: CExpr::Prop(v(), "priority".into()),
                            desc: false,
                        }],
                        limit: Some(CExpr::Int(5)),
                    },
                ),
            },
            rest: vec![],
        }
    }

    #[test]
    fn worked_example_of_6_5_encodes_to_its_174_bytes() {
        let want = "10 00 00 00 6d 6f 69 72 61 69 2d 6c 71 2d 61 73 74 2d 76 31 01 00 01 02 00 03 01 00 00 00 10 00
                    01 00 00 00 20 21 01 00 00 00 00 01 00 00 00 04 00 00 00 74 61 73 6b 00 00 00 00 00 00 00 00 00
                    01 31 33 01 3a 3b 00 00 00 00 06 00 00 00 73 74 61 74 75 73 58 04 00 00 00 6f 70 65 6e 33 04 3a
                    3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 52 01 00 00 00 00 00 00 00 14 00 00 01 00 00
                    00 16 3b 00 00 00 00 00 01 00 00 00 18 3a 3b 00 00 00 00 08 00 00 00 70 72 69 6f 72 69 74 79 00
                    01 52 05 00 00 00 00 00 00 00 00 00 00 00";
        let want: Vec<u8> = want
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        let got = encode(Root::Query(&example()));
        assert_eq!(got.len(), 174);
        assert_eq!(got, want);
    }

    #[test]
    fn sexpr_of_4_4() {
        let got = sexpr(Root::Query(&example()));
        let want = r#"(QUERY
 (PART _
  (CLAUSES
   [(MATCH false
     [(PATH (NODEP 0 ["task"] [] _) [])]
     (AND (CMP = (PROP (VAR 0) "status") (ENUM "open"))
          (CMP <= (PROP (VAR 0) "priority") (INT 1))))]
   (RETURN false false [(RITEM (VAR 0) _)] [(SORT (PROP (VAR 0) "priority") false)] (INT 5))))
 [])"#;
        assert!(crate::lq::sexpr::same(&got, want), "{got}");
    }

    #[test]
    fn variables_are_numbered_by_first_write() {
        let mk = |a: BindingId, b: BindingId| CQuery {
            first: CPart {
                use_: None,
                body: CBody::Clauses(
                    vec![],
                    CReturn {
                        distinct: false,
                        star: false,
                        items: vec![
                            (CExpr::Var(a), None),
                            (CExpr::Var(b), None),
                            (CExpr::Var(a), None),
                        ],
                        order: vec![],
                        limit: None,
                    },
                ),
            },
            rest: vec![],
        };
        assert_eq!(
            encode(Root::Query(&mk(9, 3))),
            encode(Root::Query(&mk(0, 1)))
        );
        assert_ne!(
            encode(Root::Query(&mk(9, 3))),
            encode(Root::Query(&mk(0, 0)))
        );
        assert!(
            sexpr(Root::Query(&mk(40, 2)))
                .contains("(RITEM (VAR 0) _) (RITEM (VAR 1) _) (RITEM (VAR 0) _)")
        );
    }

    #[test]
    fn deep_chains_drop_without_recursion() {
        let mut e = CExpr::Int(1);
        for _ in 0..200_000 {
            e = CExpr::Arith(1, Box::new(e), Box::new(CExpr::Int(1)));
        }
        drop(e);
    }
}
