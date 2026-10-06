//! Property tests of the evaluator ([50 §8.3]): hop bounds equal an enumeration of every walk on random graphs with
//! deleted nodes ([50 §3.7] item 2, §3.6), `p` and `NOT p` partition every input (the metamorphic test, two-valued),
//! the bag and set laws of `DISTINCT` and the set operations on random graphs, a reverse alias equals the swapped
//! pattern, the comparison laws over ints, floats and enumerations, and the aggregate laws of `WITH`.

use super::*;
use crate::lq::bind::bind_read;
use crate::lq::ctx::{BindCtx, Caller, MapIds};
use crate::lq::eval::query::Frame;
use crate::lq::eval::val::{self, cmp_total};
use crate::lq::eval::view::{View, ViewKind};
use crate::lq::parser::{ParseOptions, parse_read};
use crate::state::{Creator, EdgeKey, EdgeProps, Node, State, Tomb};
use crate::value::{Nid, Uid};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngAlgorithm, TestRng, TestRunner};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::rc::Rc;

/// A runner for the tier `MOIRAI_TEST_TIER` names (PLAN §2.1), from a fixed seed per tier; nothing persisted.
fn runner(cases: u32) -> TestRunner {
    let (tier, scale) = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => (2u8, 10),
        Ok("exit") => (3u8, 100),
        _ => (1u8, 1),
    };
    let mut seed = *b"moirai-model/lq-eval/proptest-v1";
    seed[31] ^= tier;
    TestRunner::new_with_rng(
        ProptestConfig {
            cases: cases.saturating_mul(scale),
            max_shrink_iters: 1024,
            failure_persistence: None,
            ..ProptestConfig::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &seed),
    )
}

/// A state of `k` tasks `#1`–`#k`, those in `deleted` tombstones, with the given `blocks` edges (cycles allowed: walks
/// may repeat edges; a tombstone keeps its out-edges).
fn graph(k: u32, edges: &[(u32, u32)], deleted: &BTreeSet<u32>) -> State {
    let base = store();
    let mut st = State {
        schema: base
            .dag
            .state_at(base.dag.live("main").and_then(|r| r.tip), &base.alloc)
            .schema
            .clone(),
        ..State::default()
    };
    for i in 1..=k {
        let mut u = [0u8; 16];
        u[..4].copy_from_slice(&i.to_be_bytes());
        u[15] = 1;
        let mut x = Node::new(Uid(u), "task", &st.schema, Creator::default());
        if deleted.contains(&i) {
            x.tomb = Some(Tomb::default());
        }
        st.nodes.insert(Nid(i), x);
    }
    for (a, b) in edges {
        st.nodes.get_mut(&Nid(*a)).expect("a node").out.insert(
            EdgeKey {
                kind: "blocks".into(),
                dst: Nid(*b),
                disc: None,
            },
            EdgeProps::default(),
        );
    }
    st
}

/// The rows a query returns over a state.
fn eval_rows(st: State, text: &str, bfs: bool) -> Vec<Vec<V>> {
    let base = store();
    let caller = base.resolve(&ctx(), false).expect("the caller");
    let schema = crate::lqh::lq_schema(&st.schema);
    let ids = MapIds::new();
    let params = Params::new();
    let lq_caller = Caller::default();
    let bctx = BindCtx {
        schema: &schema,
        ids: &ids,
        params: &params,
        caller: &lq_caller,
    };
    let p = parse_read(text, ParseOptions::default()).expect("parses");
    let b = bind_read(&bctx, text, &p.tree).expect("binds");
    let w = base.world(
        &caller,
        &ctx(),
        Ablations {
            bfs_hops: bfs,
            ..Ablations::default()
        },
        None,
    );
    let view = View {
        st: Rc::new(st),
        commit: None,
        ref_name: "main".into(),
        branch: None,
        kind: ViewKind::AsOf,
    };
    let ev = crate::lq::eval::view::Ev::new(&w, &view);
    let t = w
        .eval_query(&b.ast, &Frame::default(), Some(&ev))
        .expect("runs");
    t.rows
}

/// The (a, b) pairs a pattern query returns over a state.
fn pairs(st: State, text: &str, bfs: bool) -> BTreeSet<(u32, u32)> {
    eval_rows(st, text, bfs)
        .iter()
        .map(|r| (r[0].node().expect("a").0, r[1].node().expect("b").0))
        .collect()
}

/// Brute force by enumerating every walk: each sequence of qualifying edges from a live a, extended edge by edge, that
/// never leaves a deleted node ([50 §3.6]: a deleted node ends a walk); its live end is an endpoint when the walk's
/// length lies in m..=n. An unbounded maximum is m + k: a node reached by a walk of length at least m on k nodes is
/// reached by one shorter than m + k.
fn walks(
    k: u32,
    edges: &[(u32, u32)],
    deleted: &BTreeSet<u32>,
    m: u32,
    n: Option<u32>,
) -> BTreeSet<(u32, u32)> {
    fn walk(
        x: u32,
        len: u32,
        (m, max): (u32, u32),
        edges: &[(u32, u32)],
        deleted: &BTreeSet<u32>,
        out: &mut BTreeSet<u32>,
    ) {
        if len >= m && !deleted.contains(&x) {
            out.insert(x);
        }
        if len == max || deleted.contains(&x) {
            return;
        }
        for (_, d) in edges.iter().filter(|(s, _)| *s == x) {
            walk(*d, len + 1, (m, max), edges, deleted, out);
        }
    }
    let max = n.unwrap_or(m + k);
    let mut out = BTreeSet::new();
    for a in (1..=k).filter(|a| !deleted.contains(a)) {
        let mut ends = BTreeSet::new();
        walk(a, 0, (m, max), edges, deleted, &mut ends);
        out.extend(ends.into_iter().map(|b| (a, b)));
    }
    out
}

/// Brute force by shortest distances (the BFS ablation), through live nodes only.
fn distances(
    k: u32,
    edges: &[(u32, u32)],
    deleted: &BTreeSet<u32>,
    m: u32,
    n: Option<u32>,
) -> BTreeSet<(u32, u32)> {
    let mut out = BTreeSet::new();
    for a in (1..=k).filter(|a| !deleted.contains(a)) {
        let mut dist = std::collections::BTreeMap::from([(a, 0u32)]);
        let mut frontier = vec![a];
        let mut d = 0;
        while !frontier.is_empty() {
            d += 1;
            let mut next = Vec::new();
            for x in frontier.iter().filter(|x| !deleted.contains(x)) {
                for (_, b) in edges.iter().filter(|(s, _)| s == x) {
                    if !dist.contains_key(b) {
                        dist.insert(*b, d);
                        next.push(*b);
                    }
                }
            }
            frontier = next;
        }
        for (b, d) in dist {
            if d >= m && n.is_none_or(|n| d <= n) && !deleted.contains(&b) {
                out.insert((a, b));
            }
        }
    }
    out
}

/// [50 §3.7] item 2, §3.6 and [50 §8.3]: the endpoint pairs of `*m..n` equal the enumeration of every walk, on random
/// graphs with cycles and deleted nodes; under the ablation, brute-force shortest distances.
#[test]
fn hop_bounds_equal_brute_force_walks() {
    let strat = (2u32..6).prop_flat_map(|k| {
        (
            Just(k),
            proptest::collection::vec((1..=k, 1..=k), 0..10),
            proptest::collection::btree_set(1..=k, 0..2),
            0u32..3,
            proptest::option::of(0u32..4),
        )
    });
    runner(48)
        .run(&strat, |(k, edges, deleted, m, n)| {
            let n = n.map(|x| x.max(m));
            let q = match n {
                Some(n) => format!("MATCH (a:task)-[:BLOCKS*{m}..{n}]->(b:task) RETURN a, b"),
                None => format!("MATCH (a:task)-[:BLOCKS*{m}..]->(b:task) RETURN a, b"),
            };
            let mut e = edges.clone();
            e.sort();
            e.dedup();
            prop_assert_eq!(
                pairs(graph(k, &e, &deleted), &q, false),
                walks(k, &e, &deleted, m, n),
                "{} deleted {:?}",
                q,
                deleted
            );
            prop_assert_eq!(
                pairs(graph(k, &e, &deleted), &q, true),
                distances(k, &e, &deleted, m, n),
                "bfs {} deleted {:?}",
                q,
                deleted
            );
            Ok(())
        })
        .unwrap();
}

/// Rows in the total order ([50 §3.5]).
fn sorted(mut v: Vec<Vec<V>>) -> Vec<Vec<V>> {
    v.sort_by(|a, b| cmp_total(&V::List(a.clone()), &V::List(b.clone())));
    v
}

/// The distinct rows of a bag, by grouping equality ([50 §3.3]), in the total order.
fn set_of(v: Vec<Vec<V>>) -> Vec<Vec<V>> {
    let mut out: Vec<Vec<V>> = Vec::new();
    for r in sorted(v) {
        if !out
            .iter()
            .any(|x| x.iter().zip(&r).all(|(a, b)| val::same(a, b)))
        {
            out.push(r);
        }
    }
    out
}

/// [50 §3.4] and [50 §8.3] on random graphs with deleted nodes: `RETURN DISTINCT` is the set of `RETURN`'s rows,
/// `count(*)` is the bag's size, `UNION ALL` the bags' sum, and `UNION`, `INTERSECT` and `EXCEPT` the set union,
/// intersection and difference of the parts' rows.
#[test]
fn bag_and_set_laws_on_random_graphs() {
    let strat = (2u32..6).prop_flat_map(|k| {
        (
            Just(k),
            proptest::collection::vec((1..=k, 1..=k), 0..10),
            proptest::collection::btree_set(1..=k, 0..2),
            0u32..3,
        )
    });
    runner(32)
        .run(&strat, |(k, edges, deleted, h)| {
            let mut e = edges.clone();
            e.sort();
            e.dedup();
            let g = || graph(k, &e, &deleted);
            let p = format!(
                "MATCH (a:task)-[:BLOCKS*1..{}]->(b:task) RETURN b AS x",
                h + 1
            );
            let q = "MATCH (a:task)<-[:BLOCKS]-(b) RETURN a AS x";
            let bag = eval_rows(g(), &p, false);
            let other = eval_rows(g(), q, false);
            prop_assert_eq!(
                eval_rows(g(), &p.replace("RETURN b", "RETURN DISTINCT b"), false),
                set_of(bag.clone())
            );
            prop_assert_eq!(
                eval_rows(
                    g(),
                    &p.replace("RETURN b AS x", "RETURN count(*) AS n"),
                    false
                ),
                vec![vec![V::Int(bag.len() as i64)]]
            );
            let all = eval_rows(g(), &format!("{p} UNION ALL {q}"), false);
            let mut sum = bag.clone();
            sum.extend(other.clone());
            prop_assert_eq!(sorted(all), sorted(sum.clone()));
            prop_assert_eq!(
                eval_rows(g(), &format!("{p} UNION {q}"), false),
                set_of(sum)
            );
            let (bs, os) = (set_of(bag), set_of(other));
            prop_assert_eq!(
                eval_rows(g(), &format!("{p} INTERSECT {q}"), false),
                bs.iter()
                    .filter(|r| os.contains(r))
                    .cloned()
                    .collect::<Vec<_>>()
            );
            prop_assert_eq!(
                eval_rows(g(), &format!("{p} EXCEPT {q}"), false),
                bs.iter()
                    .filter(|r| !os.contains(r))
                    .cloned()
                    .collect::<Vec<_>>()
            );
            Ok(())
        })
        .unwrap();
}

/// [50 §3.3]: the ordered operators agree with `=` and with each other over ints and floats — `<=` is `<` or `=`,
/// `>=` is not `<`, exactly one of `<`, `=` and `>` holds, the order is antisymmetric — and grouping equality is `=`.
#[test]
fn mixed_numeric_comparisons_are_consistent() {
    let finite = any::<f64>().prop_filter("not NaN", |f| !f.is_nan());
    let strat = (any::<i64>(), -1_000i64..1_000, finite, 0u8..4);
    runner(256)
        .run(&strat, |(i, j, f, pick)| {
            let (x, y) = match pick {
                0 => (V::Int(i), V::Float(f)),
                1 => (V::Float(f), V::Int(i)),
                2 => (V::Int(j), V::Float(j as f64)),
                _ => (V::Float(j as f64 / 2.0), V::Int(j / 2)),
            };
            let o = val::ord(&x, &y).expect("numbers compare");
            let (lt, gt) = (o == Ordering::Less, o == Ordering::Greater);
            let e = val::eq(&x, &y) == Some(true);
            prop_assert_eq!(o != Ordering::Greater, lt || e);
            prop_assert_eq!(o != Ordering::Less, !lt);
            prop_assert_eq!(u8::from(lt) + u8::from(e) + u8::from(gt), 1);
            prop_assert_eq!(val::ord(&y, &x), Some(o.reverse()));
            prop_assert_eq!(val::same(&x, &y), e);
            Ok(())
        })
        .unwrap();
}

/// [50 §3.5]: an ordered comparison of `criticality` with a text is the comparison of their declared ranks, whether the
/// enumeration is read directly, through a `WITH` variable or through `coalesce`; a text that is no value of the field
/// compares false; an enum constant in a `CASE` column sorts among the stored values by its rank.
#[test]
fn enumeration_comparisons_follow_the_declared_ranks() {
    const VALUES: [&str; 4] = ["critical", "high", "normal", "low"];
    let strat = (
        proptest::collection::vec(0usize..4, 1..4),
        0usize..5,
        0usize..4,
    );
    runner(16)
        .run(&strat, |(crit, lit, op)| {
            let mut st = store();
            let creates: Vec<String> = crit
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    format!(
                        "CREATE (r{i}:rule {{title: 'pr {i}', text: 't', criticality: '{}'}})",
                        VALUES[*c]
                    )
                })
                .collect();
            let r = tx_lq(
                &mut st,
                &format!("TX {{ {} }}", creates.join("; ")),
                Params::new(),
                &ctx(),
            );
            prop_assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
            // A value as a literal; a text that is no value as a computed text, which the binder cannot refuse (E102).
            let text = VALUES
                .get(lit)
                .map_or_else(|| "trim('zzz')".to_string(), |v| format!("'{v}'"));
            let sym = ["<", "<=", ">", ">="][op];
            let want: Vec<Vec<V>> = crit
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    lit < 4
                        && match op {
                            0 => **c < lit,
                            1 => **c <= lit,
                            2 => **c > lit,
                            _ => **c >= lit,
                        }
                })
                .map(|(i, _)| vec![V::text(format!("pr {i}"))])
                .collect();
            for q in [
                format!("MATCH (r:rule) WHERE r.title STARTS WITH 'pr ' AND r.criticality {sym} {text} RETURN r.title ORDER BY r.title"),
                format!("MATCH (r:rule) WHERE r.title STARTS WITH 'pr ' WITH r.title AS t, r.criticality AS c WHERE c {sym} {text} RETURN t ORDER BY t"),
                format!("MATCH (r:rule) WHERE r.title STARTS WITH 'pr ' AND coalesce(r.criticality, 'normal') {sym} {text} RETURN r.title ORDER BY r.title"),
            ] {
                prop_assert_eq!(&rows(&mut st, &q).rows, &want, "{}", q);
            }
            // A constant in the column ranks with the stored values ([50 §3.5]): `pr 0` takes value `lit`.
            if let Some(v) = VALUES.get(lit) {
                let mut order: Vec<(usize, usize)> = crit
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (if i == 0 { lit } else { *c }, i))
                    .collect();
                order.sort();
                let want: Vec<Vec<V>> = order
                    .iter()
                    .map(|(_, i)| vec![V::text(format!("pr {i}"))])
                    .collect();
                for k in [
                    format!("CASE WHEN r.title = 'pr 0' THEN '{v}' ELSE r.criticality END"),
                    format!("CASE WHEN r.title <> 'pr 0' THEN r.criticality ELSE '{v}' END"),
                ] {
                    let q = format!(
                        "MATCH (r:rule) WHERE r.title STARTS WITH 'pr ' WITH r, {k} AS k RETURN r.title ORDER BY k, r.title"
                    );
                    prop_assert_eq!(&rows(&mut st, &q).rows, &want, "{}", q);
                }
            }
            Ok(())
        })
        .unwrap();
}

/// [LQ/canonical-ast §5.7] V4 and [50 §3.4] rule 6: an aggregating `WITH` yields the rows of the same aggregating
/// `RETURN`, ints and floats of one value forming one group, and its `ORDER BY` aggregates over its items order them
/// as the `RETURN`'s do; over no rows, one row (count 0, sum 0, avg absent).
#[test]
fn aggregating_with_equals_return() {
    let num = prop_oneof![
        (-3i64..4).prop_map(P::Int),
        (-3i64..4).prop_map(|i| P::Float(i as f64)),
        (-6i64..8).prop_map(|i| P::Float(i as f64 / 2.0)),
    ];
    runner(32)
        .run(&proptest::collection::vec(num, 0..8), |xs| {
            let mut st = store();
            let params = Params::new().with("xs", P::List(xs.clone()));
            let run = |st: &mut Store, q: &str| {
                out(
                    q_with(st, q, params.clone(), Ablations::default(), &ctx()),
                    q,
                )
                .rows
            };
            let with = run(
                &mut st,
                "UNWIND $xs AS x WITH x, count(*) AS c, sum(x) AS s, collect(x) AS l RETURN x, c, s, l",
            );
            let ret = run(
                &mut st,
                "UNWIND $xs AS x RETURN x, count(*) AS c, sum(x) AS s, collect(x) AS l",
            );
            prop_assert_eq!(sorted(with), sorted(ret));
            // `ORDER BY` aggregates over the items — an aliased key, an aggregate item — order the groups alike, so
            // `LIMIT` keeps the same groups (the `RETURN` after the `WITH` orders its rows by their identity again).
            for key in [
                "sum(y) DESC",
                "size(collect(y))",
                "max(c) DESC, y",
                "collect(y) DESC",
            ] {
                let with = run(
                    &mut st,
                    &format!(
                        "UNWIND $xs AS x WITH x AS y, count(*) AS c ORDER BY {key} LIMIT 3 RETURN y, c"
                    ),
                );
                let ret = run(
                    &mut st,
                    &format!("UNWIND $xs AS x RETURN x AS y, count(*) AS c ORDER BY {key} LIMIT 3"),
                );
                prop_assert_eq!(sorted(with), sorted(ret), "{}", key);
            }
            let total = run(
                &mut st,
                "UNWIND $xs AS x WITH count(*) AS c, sum(x) AS s, avg(x) AS a RETURN c, s, a",
            );
            prop_assert_eq!(total.len(), 1);
            prop_assert_eq!(&total[0][0], &V::Int(xs.len() as i64));
            if xs.is_empty() {
                prop_assert_eq!(&total[0], &vec![V::Int(0), V::Int(0), V::Absent]);
            }
            Ok(())
        })
        .unwrap();
}

/// The predicates of the metamorphic test, over the fixture store's tasks and findings.
const PREDICATES: [&str; 10] = [
    "t.priority < 2",
    "t.defer_until < now()",
    "t.status = 'open'",
    "t.assignee <> 'dev#1'",
    "t.labels IS NULL",
    "'l5' IN t.labels",
    "t.title STARTS WITH 'Note'",
    "t.unfinished AND t.priority >= 3",
    "t.round = 1 OR t.severity = 'optional'",
    "size(t.title) > 10",
];

/// [50 §3.3] and [50 §8.3]: `p` and `NOT p` partition every input under the two-valued rule, for compound
/// predicates too.
#[test]
fn p_and_not_p_partition_the_rows() {
    let strat = (0..PREDICATES.len(), 0..PREDICATES.len(), any::<bool>());
    runner(24)
        .run(&strat, |(i, j, and)| {
            let mut st = store();
            let p = format!(
                "({}) {} ({})",
                PREDICATES[i],
                if and { "AND" } else { "OR" },
                PREDICATES[j]
            );
            let count = |st: &mut Store, w: &str| -> i64 {
                let o = rows(st, &format!("MATCH (t) WHERE {w} RETURN count(*) AS n"));
                o.rows[0][0].as_int().expect("a count")
            };
            let all = count(&mut st, "true");
            let yes = count(&mut st, &p);
            let no = count(&mut st, &format!("NOT ({p})"));
            prop_assert_eq!(yes + no, all, "{}", p);
            Ok(())
        })
        .unwrap();
}

/// [50 §8.3] bag laws: `RETURN DISTINCT` is the set of `RETURN`'s rows; a reverse alias equals the swapped pattern.
#[test]
fn distinct_is_the_set_of_the_bag_and_aliases_swap() {
    let shapes = [
        (
            "MATCH (a)-[:BLOCKS]->(b) RETURN b",
            "MATCH (a)-[:BLOCKS]->(b) RETURN DISTINCT b",
        ),
        (
            "MATCH (f:finding)-[:ABOUT]->(d) RETURN d",
            "MATCH (f:finding)-[:ABOUT]->(d) RETURN DISTINCT d",
        ),
        (
            "MATCH (t:task) RETURN t.priority",
            "MATCH (t:task) RETURN DISTINCT t.priority",
        ),
    ];
    runner(6)
        .run(&(0..shapes.len()), |i| {
            let mut st = store();
            let bag = rows(&mut st, shapes[i].0).rows;
            let set = rows(&mut st, shapes[i].1).rows;
            let mut want: Vec<Vec<V>> = Vec::new();
            for r in bag {
                if !want.contains(&r) {
                    want.push(r);
                }
            }
            want.sort_by(|a, b| {
                crate::lq::eval::val::cmp_total(&V::List(a.clone()), &V::List(b.clone()))
            });
            prop_assert_eq!(set, want);
            let a = rows(&mut st, "MATCH (x)-[:BLOCKED_BY]->(y) RETURN x, y").rows;
            let mut b = rows(&mut st, "MATCH (y)-[:BLOCKS]->(x) RETURN x, y").rows;
            b.sort_by(|p, q| {
                crate::lq::eval::val::cmp_total(&V::List(p.clone()), &V::List(q.clone()))
            });
            let mut a2 = a.clone();
            a2.sort_by(|p, q| {
                crate::lq::eval::val::cmp_total(&V::List(p.clone()), &V::List(q.clone()))
            });
            prop_assert_eq!(a2, b);
            Ok(())
        })
        .unwrap();
}
