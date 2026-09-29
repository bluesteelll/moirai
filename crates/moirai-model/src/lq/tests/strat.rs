//! Proptest strategies for S-ASTs that the parser can build ([LQ/canonical-ast §3.4]: the printer property quantifies
//! over parse results). The generators respect the forms the parser normalises away or refuses: node patterns never
//! carry a `NULL` property value (E118), a subquery is never empty, a standalone call's `WHERE` needs a `YIELD`, and ref
//! names never have the shape of a commit or sequence literal. They reach the names the printer must back-quote to
//! round-trip: reserved and contextual words, the refused function names (`nodes`, `single`, `timestamp`, `cast`,
//! `shortestPath`, ...), procedure names whose first segment is a refused prefix (`apoc`, `gds`, `db`, `dbms`, `tx`), and
//! the six revision relations with revisions, ranges, lists and quoted strings at their revision positions
//! ([LQ/lexical §4.2]).

use crate::lq::ast::*;
use crate::lq::diag::Span;
use proptest::prelude::*;
use proptest::strategy::Union;

fn sp() -> Span {
    Span::default()
}

fn name(s: String) -> Name {
    Name::new(s, sp())
}

/// Names that exercise quoting: plain words, reserved and contextual words, non-words.
pub fn any_name() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "[a-z][a-z0-9_]{0,5}",
        1 => prop::sample::select(vec![
            "all", "ALL", "count", "exists", "size", "any", "none", "match", "Where", "order", "key", "node", "starts",
            "in", "not", "true", "null", "under", "policy", "reason", "first", "release", "add", "explain", "DRY",
            "x1", "_y", "Task", "BLOCKS", "t",
        ])
        .prop_map(str::to_string),
        1 => "[a-zA-Z ]{1,4}".prop_filter("non-empty", |s| !s.is_empty()),
        1 => prop::sample::select(vec!["my var", "a`b", "é", "1a", "a.b", "a-b", "tx", "apoc", "diff"]).prop_map(str::to_string),
    ]
}

/// Names valid where a `.` joins segments (procedure and query names): no `.` inside a segment.
fn seg_name() -> impl Strategy<Value = String> {
    any_name().prop_filter("no dot", |s| !s.contains('.'))
}

fn word() -> impl Strategy<Value = String> {
    "[a-zA-Z_][a-zA-Z0-9_]{0,5}"
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => "[a-zA-Z0-9 ]{0,8}",
        1 => prop::collection::vec(
            prop_oneof![
                Just('\''), Just('"'), Just('\\'), Just('\n'), Just('\r'), Just('\t'), Just('\u{0}'), Just('\u{1f}'),
                Just('\u{7f}'), Just('é'), Just('😀'), Just('a'), Just('/'), Just('*'), Just('`')
            ],
            0..6
        )
        .prop_map(|v| v.into_iter().collect()),
    ]
}

fn e(kind: ExprKind) -> Expr {
    Expr::new(kind, sp())
}

fn literal() -> impl Strategy<Value = Expr> {
    prop_oneof![
        (0..=i64::MAX).prop_map(|n| e(ExprKind::Int(n))),
        prop::sample::select(vec![
            "1.5", "0.0", "10.25", "2e10", "3E-2", "1.0e+3", "007.5"
        ])
        .prop_map(|t| e(ExprKind::Float(t.into()))),
        text().prop_map(|t| e(ExprKind::Str(t))),
        prop::sample::select(vec!["3d", "15m", "0s", "2w", "10h"])
            .prop_map(|t| e(ExprKind::Dur(t.into()))),
        any::<bool>().prop_map(|b| e(ExprKind::Bool(b))),
        Just(e(ExprKind::Null)),
        (1..=u32::MAX).prop_map(|n| e(ExprKind::Nid(n))),
        "[0-9a-f]{32}".prop_map(|h| e(ExprKind::Uid(h))),
    ]
}

fn leaf() -> impl Strategy<Value = Expr> {
    prop_oneof![
        3 => literal(),
        3 => any_name().prop_map(|n| e(ExprKind::Ident(n))),
        1 => word().prop_map(|n| e(ExprKind::Param(n))),
        1 => Just(e(ExprKind::CountStar)),
    ]
}

fn cmp_op() -> impl Strategy<Value = CmpOp> {
    prop::sample::select(vec![
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::Lt,
        CmpOp::Le,
        CmpOp::Gt,
        CmpOp::Ge,
    ])
}

fn arith_op() -> impl Strategy<Value = ArithOp> {
    prop::sample::select(vec![ArithOp::Add, ArithOp::Sub, ArithOp::Mul, ArithOp::Div])
}

fn quant() -> impl Strategy<Value = Quant> {
    (0u32..5, prop::option::of(0u32..5)).prop_map(|(a, b)| match b {
        Some(b) => Quant {
            min: a.min(b),
            max: Some(a.max(b)),
        },
        None => Quant { min: a, max: None },
    })
}

fn no_null(v: &Expr) -> bool {
    v.kind != ExprKind::Null
}

fn kvs(inner: BoxedStrategy<Expr>, pattern: bool) -> impl Strategy<Value = Vec<Kv>> {
    let value = if pattern {
        inner.prop_filter("E118", no_null).boxed()
    } else {
        inner
    };
    prop::collection::vec(
        (any_name(), value).prop_map(|(k, v)| Kv {
            key: name(k),
            value: v,
        }),
        0..3,
    )
}

fn npat(inner: BoxedStrategy<Expr>) -> impl Strategy<Value = NPat> {
    (
        prop::option::of(any_name()),
        prop::collection::vec(any_name(), 0..3),
        kvs(inner.clone(), true),
        prop::option::of(inner),
        prop::option::of(prop_oneof![
            (1..=u32::MAX).prop_map(|n| e(ExprKind::Nid(n))),
            "[0-9a-f]{32}".prop_map(|h| e(ExprKind::Uid(h)))
        ]),
    )
        .prop_map(|(var, labels, props, where_, lit)| match lit {
            Some(l)
                if var.is_none() && labels.is_empty() && props.is_empty() && where_.is_none() =>
            {
                NPat {
                    var: None,
                    labels: vec![],
                    props: vec![Kv {
                        key: name("id".into()),
                        value: l,
                    }],
                    where_: None,
                    span: sp(),
                }
            }
            _ => NPat {
                var: var.map(name),
                labels: labels.into_iter().map(name).collect(),
                props,
                where_,
                span: sp(),
            },
        })
}

fn epat(inner: BoxedStrategy<Expr>) -> impl Strategy<Value = EPat> {
    (
        prop::option::of(any_name()),
        prop::sample::select(vec![Dir::Right, Dir::Left, Dir::Both]),
        prop::collection::vec(any_name(), 0..3),
        prop::option::of(quant()),
        kvs(inner.clone(), true),
        prop::option::of(inner),
    )
        .prop_map(|(var, dir, types, quant, props, where_)| EPat {
            var: var.map(name),
            dir,
            types: types.into_iter().map(name).collect(),
            quant,
            props,
            where_,
            span: sp(),
        })
}

fn path_with(inner: BoxedStrategy<Expr>, groups: bool) -> BoxedStrategy<Path> {
    let step = if groups {
        prop_oneof![
            3 => (epat(inner.clone()), npat(inner.clone())).prop_map(|(e, n)| Step::Edge(e, n)),
            1 => (path_with(inner.clone(), false), prop::option::of(inner.clone()), quant(), npat(inner.clone()))
                .prop_map(|(p, w, q, n)| Step::Group(Group { path: p, where_: w, quant: q, span: sp() }, n)),
        ]
        .boxed()
    } else {
        (epat(inner.clone()), npat(inner.clone()))
            .prop_map(|(e, n)| Step::Edge(e, n))
            .boxed()
    };
    (npat(inner), prop::collection::vec(step, 0..3))
        .prop_map(|(start, steps)| Path {
            start,
            steps,
            span: sp(),
        })
        .boxed()
}

/// A path of at least one step (a pattern predicate needs an edge).
fn edge_path(inner: BoxedStrategy<Expr>) -> BoxedStrategy<Path> {
    path_with(inner, true)
        .prop_filter("one step", |p| !p.steps.is_empty())
        .boxed()
}

/// Function names: any name, and the refused names and keyword forms the printer back-quotes ([LQ/grammar-v1.ebnf
/// §P.9], §R).
fn fn_name() -> impl Strategy<Value = String> {
    prop_oneof![
        6 => any_name(),
        1 => prop::sample::select(vec![
            "nodes", "relationships", "single", "timestamp", "cast", "shortestPath", "allShortestPaths", "NODES",
            "Timestamp", "CAST", "exists", "size", "all", "any", "none",
        ])
        .prop_map(str::to_string),
    ]
}

fn args(inner: BoxedStrategy<Expr>) -> impl Strategy<Value = Vec<Arg>> {
    prop::collection::vec(
        (prop::option::of(any_name()), inner).prop_map(|(n, v)| Arg {
            name: n.map(name),
            value: ArgVal::Expr(v),
        }),
        0..3,
    )
}

/// Expressions up to a small depth; subqueries hold whole clauses.
pub fn expr() -> BoxedStrategy<Expr> {
    leaf()
        .boxed()
        .prop_recursive(4, 40, 4, |inner| {
            let inner = inner.boxed();
            let b = |x: Expr| Box::new(x);
            Union::new_weighted(vec![
                (
                    2,
                    (inner.clone(), inner.clone())
                        .prop_map(move |(l, r)| e(ExprKind::Or(b(l), b(r))))
                        .boxed(),
                ),
                (
                    2,
                    (inner.clone(), inner.clone())
                        .prop_map(move |(l, r)| e(ExprKind::And(b(l), b(r))))
                        .boxed(),
                ),
                (
                    1,
                    inner
                        .clone()
                        .prop_map(move |x| e(ExprKind::Not(b(x))))
                        .boxed(),
                ),
                (
                    2,
                    (cmp_op(), inner.clone(), inner.clone())
                        .prop_map(move |(o, l, r)| {
                            // `= NULL` and `<> NULL` are refused by the parser (E118, P18).
                            let null = l.kind == ExprKind::Null || r.kind == ExprKind::Null;
                            let o = if null && matches!(o, CmpOp::Eq | CmpOp::Ne) {
                                CmpOp::Lt
                            } else {
                                o
                            };
                            e(ExprKind::Cmp(o, b(l), b(r)))
                        })
                        .boxed(),
                ),
                (
                    1,
                    (any::<bool>(), inner.clone())
                        .prop_map(move |(n, x)| e(ExprKind::IsNull(n, b(x))))
                        .boxed(),
                ),
                (
                    1,
                    (inner.clone(), inner.clone())
                        .prop_map(move |(l, r)| e(ExprKind::In(b(l), b(r))))
                        .boxed(),
                ),
                (
                    1,
                    (
                        prop::sample::select(vec![StrOp::Starts, StrOp::Ends, StrOp::Contains]),
                        inner.clone(),
                        inner.clone(),
                    )
                        .prop_map(move |(o, l, r)| e(ExprKind::StrPred(o, b(l), b(r))))
                        .boxed(),
                ),
                (
                    1,
                    (inner.clone(), prop::collection::vec(any_name(), 1..3))
                        .prop_map(move |(x, ls)| {
                            e(ExprKind::LabelTest(
                                b(x),
                                ls.into_iter().map(name).collect(),
                            ))
                        })
                        .boxed(),
                ),
                (
                    2,
                    (arith_op(), inner.clone(), inner.clone())
                        .prop_map(move |(o, l, r)| e(ExprKind::Arith(o, b(l), b(r))))
                        .boxed(),
                ),
                (
                    1,
                    inner
                        .clone()
                        .prop_map(move |x| e(ExprKind::Neg(b(x))))
                        .boxed(),
                ),
                (
                    2,
                    (inner.clone(), any_name())
                        .prop_map(move |(x, n)| e(ExprKind::Prop(b(x), name(n))))
                        .boxed(),
                ),
                (
                    2,
                    (fn_name(), any::<bool>(), args(inner.clone()))
                        .prop_map(|(n, d, a)| {
                            e(ExprKind::Fn {
                                name: name(n),
                                distinct: d,
                                args: a,
                            })
                        })
                        .boxed(),
                ),
                (
                    1,
                    (
                        prop::sample::select(vec![
                            ListPredKind::All,
                            ListPredKind::Any,
                            ListPredKind::None,
                        ]),
                        any_name(),
                        inner.clone(),
                        inner.clone(),
                    )
                        .prop_map(move |(k, v, l, p)| {
                            e(ExprKind::ListPred {
                                kind: k,
                                var: name(v),
                                list: b(l),
                                pred: b(p),
                            })
                        })
                        .boxed(),
                ),
                (
                    1,
                    prop::collection::vec(inner.clone(), 0..3)
                        .prop_map(|v| e(ExprKind::List(v)))
                        .boxed(),
                ),
                (
                    1,
                    kvs(inner.clone(), false)
                        .prop_map(|v| e(ExprKind::Map(v)))
                        .boxed(),
                ),
                (
                    1,
                    (
                        prop::option::of(inner.clone()),
                        prop::collection::vec(
                            (inner.clone(), inner.clone())
                                .prop_map(|(c, t)| When { cond: c, then: t }),
                            1..3,
                        ),
                        prop::option::of(inner.clone()),
                    )
                        .prop_map(move |(s, w, el)| {
                            e(ExprKind::Case {
                                subject: s.map(b),
                                whens: w,
                                else_: el.map(b),
                            })
                        })
                        .boxed(),
                ),
                (
                    1,
                    sub(inner.clone())
                        .prop_map(|s| e(ExprKind::Exists(Box::new(s))))
                        .boxed(),
                ),
                (
                    1,
                    sub(inner.clone())
                        .prop_map(|s| e(ExprKind::CountSub(Box::new(s))))
                        .boxed(),
                ),
                (
                    1,
                    edge_path(inner)
                        .prop_map(|p| {
                            e(ExprKind::Exists(Box::new(Sub::Patterns {
                                patterns: vec![p],
                                where_: None,
                            })))
                        })
                        .boxed(),
                ),
            ])
        })
        .boxed()
}

fn sub(inner: BoxedStrategy<Expr>) -> BoxedStrategy<Sub> {
    prop_oneof![
        (
            prop::collection::vec(path_with(inner.clone(), true), 1..3),
            prop::option::of(inner.clone())
        )
            .prop_map(|(p, w)| Sub::Patterns {
                patterns: p,
                where_: w
            }),
        (
            prop::collection::vec(clause(inner.clone()), 0..2),
            prop::option::of(ret(inner))
        )
            .prop_filter("a subquery is not empty", |(c, r)| !c.is_empty()
                || r.is_some())
            .prop_map(|(c, r)| Sub::Clauses { clauses: c, ret: r }),
    ]
    .boxed()
}

/// The six revision relations ([LQ/lexical §4.2]): name, whether positional argument 0 is a revision position, and the
/// named revision arguments (as the parser's table has them).
const REV_RELATIONS: [(&str, bool, &[&str]); 6] = [
    ("diff", true, &["range"]),
    ("log", true, &["range"]),
    ("changes", false, &["since", "ref"]),
    ("history", false, &["in"]),
    ("across", false, &["refs"]),
    ("violations", true, &["ref"]),
];

/// A procedure name other than a revision relation's (those take [`rev_call`]'s arguments): dotted names, and names
/// whose first segment is a refused prefix, in any case.
fn proc_name() -> impl Strategy<Value = String> {
    let prefixed = (
        prop::sample::select(vec!["apoc", "APOC", "gds", "db", "Db", "dbms", "tx", "TX"]),
        prop::collection::vec(seg_name(), 0..2),
    )
        .prop_map(|(first, rest)| {
            std::iter::once(first.to_string())
                .chain(rest)
                .collect::<Vec<_>>()
                .join(".")
        });
    prop_oneof![
        4 => prop::collection::vec(seg_name(), 1..3).prop_map(|v| v.join(".")),
        1 => prefixed,
    ]
    .prop_filter(
        "the revision relations read their arguments in revision mode",
        |n| {
            !REV_RELATIONS
                .iter()
                .any(|(r, ..)| r.eq_ignore_ascii_case(n))
        },
    )
}

/// A value at a revision position: a revspec, a range, a list, or a quoted string, which ends revision mode.
fn rev_val() -> BoxedStrategy<ArgVal> {
    prop_oneof![
        3 => rev().prop_map(ArgVal::Rev),
        2 => (rev(), prop::sample::select(vec![RangeOp::Two, RangeOp::Three]), rev())
            .prop_map(|(from, op, to)| ArgVal::Range { from, op, to, span: sp() }),
        1 => prop::collection::vec(rev(), 1..3).prop_map(|v| ArgVal::List(v, sp())),
        1 => text().prop_map(|t| ArgVal::Expr(e(ExprKind::Str(t)))),
    ]
    .boxed()
}

/// A call of a revision relation, in either case: its revision positions hold [`rev_val`]s; an ordinary named argument
/// and a further positional one hold expressions.
fn rev_call(inner: BoxedStrategy<Expr>) -> BoxedStrategy<(String, Vec<Arg>)> {
    (
        prop::sample::select(REV_RELATIONS.to_vec()),
        any::<bool>(),
        rev_val(),
        prop::collection::vec(prop::option::of(rev_val()), 2),
        prop::option::of((
            prop::sample::select(vec!["kind", "limit", "x"]),
            inner.clone(),
        )),
        prop::option::of(inner),
    )
        .prop_map(|((rel, pos0, named), upper, first, revs, ordinary, last)| {
            let mut args = Vec::new();
            if pos0 {
                args.push(Arg {
                    name: None,
                    value: first,
                });
            }
            for (n, v) in named.iter().zip(revs) {
                if let Some(v) = v {
                    args.push(Arg {
                        name: Some(name((*n).to_string())),
                        value: v,
                    });
                }
            }
            if let Some((n, v)) = ordinary {
                args.push(Arg {
                    name: Some(name(n.to_string())),
                    value: ArgVal::Expr(v),
                });
            }
            if let Some(v) = last {
                args.push(Arg {
                    name: None,
                    value: ArgVal::Expr(v),
                });
            }
            let rel = if upper {
                rel.to_ascii_uppercase()
            } else {
                rel.to_string()
            };
            (rel, args)
        })
        .boxed()
}

/// The callee and arguments of a `CALL`.
fn call_head(inner: BoxedStrategy<Expr>) -> BoxedStrategy<(String, Vec<Arg>)> {
    prop_oneof![
        4 => (proc_name(), args(inner.clone())),
        1 => rev_call(inner),
    ]
    .boxed()
}

fn yitems() -> impl Strategy<Value = Vec<YItem>> {
    prop::collection::vec(
        (any_name(), prop::option::of(any_name())).prop_map(|(n, a)| YItem {
            name: name(n),
            as_: a.map(name),
        }),
        1..3,
    )
}

fn sorts(inner: BoxedStrategy<Expr>) -> impl Strategy<Value = Vec<Sort>> {
    prop::collection::vec(
        (inner, any::<bool>()).prop_map(|(e, d)| Sort { expr: e, desc: d }),
        0..2,
    )
}

fn limit() -> impl Strategy<Value = Option<Expr>> {
    prop::option::of(prop_oneof![
        (0..=i64::MAX).prop_map(|n| e(ExprKind::Int(n))),
        word().prop_map(|n| e(ExprKind::Param(n)))
    ])
}

fn items(inner: BoxedStrategy<Expr>) -> impl Strategy<Value = Vec<Item>> {
    prop::collection::vec(
        (inner, prop::option::of(any_name())).prop_map(|(e, a)| Item {
            expr: e,
            as_: a.map(name),
        }),
        0..3,
    )
}

fn clause(inner: BoxedStrategy<Expr>) -> BoxedStrategy<Clause> {
    let modes = prop::option::of(prop::sample::select(vec![
        MatchMode::Walk,
        MatchMode::Trail,
        MatchMode::Acyclic,
        MatchMode::Simple,
        MatchMode::Different,
    ]));
    prop_oneof![
        3 => (any::<bool>(), modes, prop::collection::vec(path_with(inner.clone(), true), 1..3), prop::option::of(inner.clone()))
            .prop_map(|(optional, mode, patterns, where_)| Clause::Match(Match {
                optional,
                mode: if optional { None } else { mode },
                patterns,
                where_,
                span: sp()
            })),
        1 => (call_head(inner.clone()), yitems(), prop::option::of(inner.clone()))
            .prop_map(|((p, a), y, w)| Clause::Call(Call { proc: name(p), args: a, yield_: y, where_: w, span: sp() })),
        1 => (inner.clone(), any_name()).prop_map(|(x, a)| Clause::Unwind(Unwind { expr: x, as_: name(a), span: sp() })),
        1 => (any::<bool>(), any::<bool>(), items(inner.clone()), prop::option::of(inner.clone()), sorts(inner.clone()), limit())
            .prop_filter("WITH has items", |(_, star, it, ..)| *star || !it.is_empty())
            .prop_map(|(distinct, star, items, where_, order, limit)| Clause::With(With {
                distinct,
                star,
                items,
                where_,
                order,
                limit,
                span: sp()
            })),
    ]
    .boxed()
}

fn ret(inner: BoxedStrategy<Expr>) -> BoxedStrategy<Return> {
    (
        any::<bool>(),
        any::<bool>(),
        items(inner.clone()),
        prop::collection::vec(inner.clone(), 0..2),
        sorts(inner),
        limit(),
    )
        .prop_filter("RETURN has items", |(_, star, it, ..)| {
            *star || !it.is_empty()
        })
        .prop_map(|(distinct, star, items, group, order, limit)| Return {
            distinct,
            star,
            items,
            group,
            order,
            limit,
            span: sp(),
        })
        .boxed()
}

fn ref_name() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "main",
        "lane/x",
        "tags/v1.2",
        "merge/main/from/lane/l10",
        "plan/a-b",
        "c123456",
        "s1x",
        "h",
    ])
    .prop_map(str::to_string)
}

/// A revision (`revspec`).
pub fn rev() -> BoxedStrategy<Rev> {
    let base = prop_oneof![
        Just(RevKind::Head),
        ref_name().prop_map(RevKind::Ref),
        "[0-9a-f]{7,12}".prop_map(RevKind::Commit),
        any::<u64>().prop_map(RevKind::Seq),
    ];
    let suffix = prop_oneof![
        any::<u32>().prop_map(Suffix::Tilde),
        any::<u32>().prop_map(Suffix::Caret),
        any::<u32>().prop_map(Suffix::At),
        (
            2000u32..2100,
            1u32..13,
            1u32..29,
            0u32..24,
            0u32..60,
            0u32..60
        )
            .prop_map(|(y, mo, d, h, mi, s)| Suffix::AtTime(format!(
                "{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"
            ))),
    ];
    prop_oneof![
        5 => (base, prop::collection::vec(suffix, 0..3)).prop_map(|(b, sufs)| {
            let mut r = Rev { kind: b, span: sp() };
            for s in sufs {
                r = Rev { kind: RevKind::Suf(Box::new(r), s), span: sp() };
            }
            r
        }),
        1 => word().prop_map(|p| Rev { kind: RevKind::Param(p), span: sp() }),
    ]
    .boxed()
}

fn part(inner: BoxedStrategy<Expr>) -> BoxedStrategy<Part> {
    let scall = (
        call_head(inner.clone()),
        prop_oneof![
            Just(YieldMode::None),
            Just(YieldMode::Star),
            yitems().prop_map(YieldMode::Items)
        ],
        prop::option::of(inner.clone()),
        sorts(inner.clone()),
        limit(),
    )
        .prop_map(|((p, a), y, w, order, limit)| {
            let where_ = if y == YieldMode::None { None } else { w };
            PartBody::Call(SCall {
                proc: name(p),
                args: a,
                yield_: y,
                where_,
                order,
                limit,
                span: sp(),
            })
        });
    let clauses = (
        prop::collection::vec(clause(inner.clone()), 0..3),
        ret(inner),
    )
        .prop_map(|(clauses, ret)| PartBody::Clauses { clauses, ret });
    (
        prop::option::of(rev()),
        prop_oneof![1 => scall, 3 => clauses],
    )
        .prop_map(|(u, body)| Part {
            use_: u,
            body,
            span: sp(),
        })
        .boxed()
}

/// A query of one to three parts.
pub fn query() -> BoxedStrategy<Query> {
    let inner = expr();
    (
        prop::collection::vec(part(inner), 1..3),
        prop::collection::vec(
            prop::sample::select(vec![
                SetOp::Union,
                SetOp::UnionAll,
                SetOp::Except,
                SetOp::Intersect,
            ]),
            2,
        ),
    )
        .prop_map(|(parts, ops)| {
            let n = parts.len() - 1;
            Query {
                parts,
                ops: ops[..n].to_vec(),
                span: sp(),
            }
        })
        .boxed()
}

/// A read root.
pub fn read() -> BoxedStrategy<Read> {
    (
        prop::sample::select(vec![Mode::Run, Mode::Explain, Mode::Profile]),
        query(),
    )
        .prop_map(|(mode, query)| Read { mode, query })
        .boxed()
}

fn target() -> impl Strategy<Value = Target> {
    prop_oneof![
        any_name().prop_map(TargetKind::Ident),
        (1..=u32::MAX).prop_map(TargetKind::Nid),
        "[0-9a-f]{32}".prop_map(TargetKind::Uid),
        word().prop_map(TargetKind::Param),
    ]
    .prop_map(|kind| Target { kind, span: sp() })
}

fn expect() -> impl Strategy<Value = Expect> {
    prop_oneof![
        (0..=i64::MAX).prop_map(Expect::Exact),
        (0..=i64::MAX, 0..=i64::MAX).prop_map(|(a, b)| Expect::Range(a, b)),
        (0..=i64::MAX).prop_map(Expect::Le),
        (0..=i64::MAX).prop_map(Expect::Ge),
        word().prop_map(|p| Expect::Param(name(p))),
    ]
}

fn mutation(inner: BoxedStrategy<Expr>) -> BoxedStrategy<Mut> {
    let dopt = prop_oneof![
        prop::sample::select(vec![Policy::Restrict, Policy::Cascade, Policy::Reparent])
            .prop_map(DOpt::Policy),
        target().prop_map(DOpt::Replaced),
        Just(DOpt::Release),
        inner.clone().prop_map(DOpt::Reason),
    ];
    prop_oneof![
        prop::collection::vec(
            (target(), any_name(), inner.clone()).prop_map(|(t, p, v)| Assign {
                target: t,
                prop: name(p),
                value: v
            }),
            1..3
        )
        .prop_map(|a| Mut::Set(a, sp())),
        prop::collection::vec(
            (target(), any_name()).prop_map(|(t, p)| TProp {
                target: t,
                prop: name(p)
            }),
            1..3
        )
        .prop_map(|a| Mut::Remove(a, sp())),
        (
            prop::collection::vec(target(), 1..3),
            prop::collection::vec(dopt, 0..4)
        )
            .prop_map(|(targets, opts)| {
                let mut seen: Vec<std::mem::Discriminant<DOpt>> = Vec::new();
                let opts = opts
                    .into_iter()
                    .filter(|o| {
                        let d = std::mem::discriminant(o);
                        let fresh = !seen.contains(&d);
                        seen.push(d);
                        fresh
                    })
                    .collect();
                Mut::Delete {
                    targets,
                    opts,
                    span: sp(),
                }
            }),
        (
            target(),
            target(),
            prop::option::of(prop_oneof![
                target().prop_map(MovePos::Before),
                target().prop_map(MovePos::After),
                Just(MovePos::First),
                Just(MovePos::Last)
            ])
        )
            .prop_map(|(t, u, pos)| Mut::Move {
                target: t,
                under: u,
                pos,
                span: sp()
            }),
        (
            target(),
            prop::sample::select(vec![EdgeDir::Right, EdgeDir::Left]),
            any_name(),
            kvs(inner.clone(), false),
            target()
        )
            .prop_map(|(src, dir, ty, props, dst)| Mut::Edge(MEdge {
                src,
                dir,
                ty: name(ty),
                props,
                dst,
                span: sp()
            })),
        (target(), inner.clone()).prop_map(|(t, r)| Mut::Reopen {
            target: t,
            reason: r,
            span: sp()
        }),
        (target(), any_name(), inner.clone(), inner).prop_map(|(t, f, r, a)| Mut::Patch {
            target: t,
            field: name(f),
            remove: r,
            add: a,
            span: sp()
        }),
    ]
    .boxed()
}

fn pdecl() -> impl Strategy<Value = PDecl> {
    (
        word(),
        any_name(),
        prop::option::of(any_name()),
        any::<bool>(),
        prop::option::of(literal()),
    )
        .prop_map(|(n, t, a, optional, default)| PDecl {
            name: name(n),
            ty: PType {
                name: name(t),
                arg: a.map(name),
            },
            optional,
            default,
        })
}

/// A definition.
pub fn define() -> BoxedStrategy<Define> {
    (
        prop::collection::vec(seg_name(), 1..3),
        prop::collection::vec(pdecl(), 0..3),
        prop::option::of(any_name()),
        prop::option::of(any_name()),
        query(),
    )
        .prop_map(|(n, params, shape, budget, body)| Define {
            name: name(n.join(".")),
            params,
            shape: shape.map(name),
            budget: budget.map(name),
            body,
            span: sp(),
        })
        .boxed()
}

fn stmt() -> BoxedStrategy<Stmt> {
    let inner = expr();
    let muts = prop::collection::vec(mutation(inner.clone()), 1..3);
    let cedge = (
        prop::sample::select(vec![EdgeDir::Right, EdgeDir::Left]),
        any_name(),
        kvs(inner.clone(), false),
        target(),
    )
        .prop_map(|(dir, ty, props, target)| CEdge {
            dir,
            ty: name(ty),
            props,
            target,
        });
    prop_oneof![
        (
            prop::collection::vec(path_with(inner.clone(), true), 1..3),
            prop::option::of(inner.clone()),
            expect(),
            muts.clone()
        )
            .prop_map(|(patterns, where_, expect, muts)| Stmt::Match(SMatch {
                patterns,
                where_,
                expect,
                muts,
                span: sp()
            })),
        muts.prop_map(|m| Stmt::Muts(m, sp())),
        (
            any_name(),
            any_name(),
            kvs(inner.clone(), false),
            prop::collection::vec(cedge, 0..3),
            prop::option::of(target()),
            prop::option::of(sub(inner.clone()))
        )
            .prop_map(|(v, l, props, edges, under, unless)| Stmt::Create(Create {
                var: name(v),
                label: name(l),
                props,
                edges,
                under,
                unless,
                span: sp()
            })),
        (
            seg_name(),
            args(inner.clone()),
            prop::collection::vec(
                (any_name(), prop::option::of(any_name())).prop_map(|(n, a)| YItem {
                    name: name(n),
                    as_: a.map(name)
                }),
                0..3
            )
        )
            .prop_map(|(n, a, y)| Stmt::TxCall {
                name: name(n),
                args: a,
                yield_: y,
                span: sp()
            }),
        (inner.clone(), prop::option::of(text())).prop_map(|(x, el)| Stmt::Assert {
            expr: x,
            else_: el,
            span: sp()
        }),
        (
            prop_oneof![
                text().prop_map(ResolveWhat::Key),
                (query(), expect()).prop_map(|(q, e)| ResolveWhat::Query(Box::new(q), e))
            ],
            prop_oneof![
                Just(Take::Ours),
                Just(Take::Theirs),
                Just(Take::Base),
                inner.prop_map(Take::Value),
                target().prop_map(Take::Repoint)
            ]
        )
            .prop_map(|(what, take)| Stmt::Resolve(Resolve {
                what,
                take,
                span: sp()
            })),
        define().prop_map(Stmt::Define),
        prop::collection::vec(seg_name(), 1..3).prop_map(|v| Stmt::Drop(name(v.join(".")))),
    ]
    .boxed()
}

/// A write root.
pub fn tx() -> BoxedStrategy<Tx> {
    (
        (prop::option::of(rev()), prop::option::of(rev())),
        (
            prop::option::of(text()),
            prop::option::of(text()),
            prop::option::of(text()),
            prop::option::of(text()),
        ),
        prop::collection::vec(stmt(), 1..4),
        any::<bool>(),
    )
        .prop_map(
            |((on, if_tip), (if_targets, key, lease, message), stmts, dry)| Tx {
                on,
                if_tip,
                if_targets,
                key,
                lease,
                message,
                stmts,
                dry,
                span: sp(),
            },
        )
        .boxed()
}
