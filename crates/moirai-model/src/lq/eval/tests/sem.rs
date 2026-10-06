//! Semantic unit tests of the evaluator on the fixture store: counting, absent values, ordering, quantified parts,
//! reverse aliases and symmetric matching, optional matches, aggregation, composition, named queries, runtime and
//! history relations, search, notices, and `TX` blocks with `EXPECT`, `ASSERT`, `DRY`, `IF TARGETS` and the digest.

use super::*;
use crate::api::Outcome;
use crate::lq::diag::Code;
use crate::lq::eval::txrun;
use crate::value::Nid;

fn n(i: u32) -> V {
    V::Node(Nid(i))
}

fn codes(o: &QueryOut) -> Vec<Code> {
    o.warnings
        .iter()
        .chain(&o.notices)
        .map(|x| x.code)
        .collect()
}

fn named(st: &mut Store, name: &str, params: Params) -> QueryOut {
    let r = st.run(
        &Cmd::Query {
            input: QueryInput::Named(name.into()),
            params,
            at: None,
            mode: Mode::Run,
            strict_gql: false,
            ablations: Ablations::default(),
        },
        &ctx(),
    );
    out(r, name)
}

/// [50 §3.4] rule 1: one binding per matched assignment, anonymous elements included (mistake 3).
#[test]
fn bags_count_bindings_of_anonymous_elements() {
    let mut st = store();
    let o = rows(
        &mut st,
        "MATCH (t:task)<-[:BLOCKS]-() RETURN t.id, count(*) AS blockers",
    );
    assert_eq!(
        o.rows,
        vec![
            vec![n(17), V::Int(1)],
            vec![n(51), V::Int(2)],
            vec![n(93), V::Int(1)]
        ]
    );
}

/// [50 §3.4] rule 2: two edge patterns of one clause never bind the same edge; nodes may repeat.
#[test]
fn edge_patterns_of_a_clause_bind_distinct_edges() {
    let mut st = store();
    let o = rows(
        &mut st,
        "MATCH (a)-[:BLOCKS]->(b), (c)-[:BLOCKS]->(d) RETURN count(*) AS n",
    );
    assert_eq!(o.rows, vec![vec![V::Int(4 * 3)]]);
}

/// [50 §3.4] rule 3: an undirected pattern binds each edge once per orientation.
#[test]
fn undirected_patterns_bind_both_orientations() {
    let mut st = store();
    let o = rows(&mut st, "MATCH (a:task)-[:BLOCKS]-(b) RETURN count(*) AS n");
    assert_eq!(o.rows, vec![vec![V::Int(8)]]);
}

/// [50 §3.3]: `<>` with an absent operand is true, `=` false; under ablation D8 both are unknown.
#[test]
fn absent_values_follow_two_valued_logic_and_d8_switches_it() {
    let mut st = store();
    let all = rows(&mut st, "MATCH (t:task) RETURN count(*) AS n").rows[0][0].clone();
    let ne = rows(
        &mut st,
        "MATCH (t:task) WHERE t.assignee <> 'dev#1' RETURN count(*) AS n",
    );
    assert_eq!(ne.rows[0][0], all);
    let eq = rows(
        &mut st,
        "MATCH (t:task) WHERE t.assignee = 'dev#1' RETURN count(*) AS n",
    );
    assert_eq!(eq.rows[0][0], V::Int(0));
    let tri = out(
        q_with(
            &mut st,
            "MATCH (t:task) WHERE t.assignee <> 'dev#1' RETURN count(*) AS n",
            Params::new(),
            Ablations {
                three_valued: true,
                ..Ablations::default()
            },
            &ctx(),
        ),
        "D8",
    );
    assert_eq!(tri.rows[0][0], V::Int(0));
    // p and NOT p partition the rows under the absent-value rule.
    let p = rows(
        &mut st,
        "MATCH (t:task) WHERE t.defer_until < now() RETURN count(*) AS n",
    );
    let np = rows(
        &mut st,
        "MATCH (t:task) WHERE NOT (t.defer_until < now()) RETURN count(*) AS n",
    );
    match (&p.rows[0][0], &np.rows[0][0], &all) {
        (V::Int(a), V::Int(b), V::Int(c)) => assert_eq!(a + b, *c),
        x => panic!("{x:?}"),
    }
    // W01 counts the rows the absence excluded.
    let w = p
        .warnings
        .iter()
        .find(|w| w.code == Code::W01)
        .expect("W01");
    assert_eq!(
        w.count,
        Some(match &all {
            V::Int(c) => *c as u64,
            _ => 0,
        })
    );
}

/// [50 §3.5]: `ORDER BY` keys first, absent last in both directions, then the binding identity.
#[test]
fn results_are_totally_ordered() {
    let mut st = store();
    let o = rows(
        &mut st,
        "MATCH (t:task) WHERE t IN children(#88) RETURN t.id, t.priority ORDER BY t.priority DESC",
    );
    // #93 is P3 on main, every other child the default P2; ties by the binding identity (id).
    assert_eq!(o.rows[0], vec![n(93), V::Int(3)]);
    let ids: Vec<V> = o.rows[1..].iter().map(|r| r[0].clone()).collect();
    assert_eq!(ids, vec![n(89), n(90), n(92), n(95), n(97)]);
    let d = rows(
        &mut st,
        "MATCH (f:finding) RETURN DISTINCT f.round ORDER BY f.round DESC",
    );
    assert_eq!(d.rows, vec![vec![V::Int(2)], vec![V::Int(1)]]);
}

/// [50 §3.7] item 2: hop bounds are walk lengths; the BFS-distance ablation keeps only shortest distances.
#[test]
fn hop_bounds_are_walk_lengths_and_bfs_is_an_ablation() {
    let mut st = store();
    // A shortcut: #14 blocks #51 directly as well as through #17.
    let r = tx_lq(
        &mut st,
        "TX { CREATE (#14)-[:BLOCKS]->(#51) }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let text = "MATCH (x:task)-[:BLOCKS*2..2]->(#51) RETURN x";
    assert_eq!(col_nodes(&rows(&mut st, text), 0), vec![14]);
    let bfs = out(
        q_with(
            &mut st,
            text,
            Params::new(),
            Ablations {
                bfs_hops: true,
                ..Ablations::default()
            },
            &ctx(),
        ),
        "bfs",
    );
    assert!(bfs.rows.is_empty(), "#14 is at distance 1");
    // A quantified group with a per-step predicate.
    let g = rows(
        &mut st,
        "MATCH (a:task)((x)-[:BLOCKS]->(y) WHERE y.unfinished){1,2}(#51) RETURN a",
    );
    assert_eq!(col_nodes(&g, 0), vec![12, 14, 17]);
}

/// [50 §3.2] reverse aliases: `BLOCKED_BY` is `BLOCKS` reversed; off, it is an unknown edge type.
#[test]
fn reverse_aliases_and_their_ablation() {
    let mut st = store();
    let o = rows(&mut st, "MATCH (#51)-[:BLOCKED_BY]->(b) RETURN b");
    assert_eq!(col_nodes(&o, 0), vec![12, 17]);
    let r = q_with(
        &mut st,
        "MATCH (#51)-[:BLOCKED_BY]->(b) RETURN b",
        Params::new(),
        Ablations {
            no_reverse_aliases: true,
            ..Ablations::default()
        },
        &ctx(),
    );
    assert_eq!(r.error.map(|e| e.code), Some("E104".into()));
}

/// N07 ([LQ/errors §5.6]; mistake 4): an empty result while edges point the other way.
#[test]
fn n07_names_edges_that_point_the_other_way() {
    let mut st = store();
    let o = rows(&mut st, "MATCH (#51)-[:BLOCKS]->(b) RETURN b");
    assert!(o.rows.is_empty());
    let n07 = o.notices.iter().find(|x| x.code == Code::N07).expect("N07");
    assert!(
        n07.message
            .starts_with("nothing matched, but 2 BLOCKS edges point the other way"),
        "{}",
        n07.message
    );
}

/// [50 §3.1] item 2: `OPTIONAL MATCH` keeps the row with absent variables.
#[test]
fn optional_match_keeps_rows() {
    let mut st = store();
    let o = rows(
        &mut st,
        "MATCH (t:task {id: #52}) OPTIONAL MATCH (b)-[:BLOCKS]->(t) RETURN t, b",
    );
    assert_eq!(o.rows, vec![vec![n(52), V::Absent]]);
}

/// Aggregates ([50 §3.4] rule 6, §3.3): absent inputs skipped, `collect` in natural order, `WITH` filtering groups.
#[test]
fn aggregates_group_and_filter() {
    let mut st = store();
    let o = rows(
        &mut st,
        "MATCH (f:finding) WITH f.round AS r, count(*) AS c, collect(f.id) AS ids WHERE c >= 2 RETURN r, c, ids ORDER BY r",
    );
    assert_eq!(
        o.rows,
        vec![
            vec![V::Int(1), V::Int(2), V::List(vec![n(136), n(137)])],
            vec![V::Int(2), V::Int(2), V::List(vec![n(138), n(139)])]
        ]
    );
    let s = rows(
        &mut st,
        "MATCH (f:finding) RETURN sum(f.round) AS s, min(f.round) AS lo, max(f.round) AS hi, avg(f.round) AS a, count(f.local_id) AS none",
    );
    assert_eq!(
        s.rows,
        vec![vec![
            V::Int(6),
            V::Int(1),
            V::Int(2),
            V::Float(1.5),
            V::Int(0)
        ]]
    );
    // N10: a division by zero gives absent and is counted.
    let z = rows(&mut st, "MATCH (f:finding) RETURN f.id, f.round / 0 AS x");
    assert!(z.rows.iter().all(|r| r[1] == V::Absent));
    let n10 = z.notices.iter().find(|x| x.code == Code::N10).expect("N10");
    assert_eq!(n10.count, Some(4));
}

/// `UNWIND`, set operations and composite parts ([50 §3.4] rule 7).
#[test]
fn unwind_and_set_operations() {
    let mut st = store();
    let u = rows(&mut st, "UNWIND [3, 1, 2] AS x RETURN x");
    assert_eq!(
        u.rows,
        vec![vec![V::Int(3)], vec![V::Int(1)], vec![V::Int(2)]]
    );
    let un = rows(
        &mut st,
        "MATCH (t {id: #12}) RETURN t UNION MATCH (t {id: #12}) RETURN t UNION MATCH (t {id: #9}) RETURN t",
    );
    assert_eq!(col_nodes(&un, 0), vec![9, 12]);
    let all = rows(
        &mut st,
        "MATCH (t {id: #12}) RETURN t UNION ALL MATCH (t {id: #12}) RETURN t",
    );
    assert_eq!(col_nodes(&all, 0), vec![12, 12]);
    let ex = rows(
        &mut st,
        "MATCH (t) WHERE t IN children(#9) RETURN t EXCEPT MATCH (t {id: #14}) RETURN t",
    );
    assert_eq!(col_nodes(&ex, 0), vec![12, 17]);
    let co = rows(
        &mut st,
        "USE main MATCH (t {id: #93}) RETURN t.priority UNION ALL USE lane/l5np MATCH (t {id: #93}) RETURN t.priority",
    );
    assert_eq!(co.rows, vec![vec![V::Int(3)], vec![V::Int(1)]]);
    assert!(codes(&co).contains(&Code::N03));
}

/// D11 ([50 §3.4]): set semantics deduplicate and refuse `count(*)` over anonymous elements.
#[test]
fn set_counting_ablation() {
    let mut st = store();
    let ab = Ablations {
        set_counting: true,
        ..Ablations::default()
    };
    let r = q_with(
        &mut st,
        "MATCH (t:task)<-[:BLOCKS]-() RETURN t.id, count(*) AS blockers",
        Params::new(),
        ab,
        &ctx(),
    );
    assert_eq!(r.error.map(|e| e.code), Some("E112".into()));
    let o = out(
        q_with(
            &mut st,
            "MATCH (t:task)<-[:BLOCKS]-() RETURN t",
            Params::new(),
            ab,
            &ctx(),
        ),
        "d11",
    );
    assert_eq!(col_nodes(&o, 0), vec![17, 51, 93]);
}

/// The standard library through `CALL` and by name ([LQ/std §4]).
#[test]
fn standard_queries_run() {
    let mut st = store();
    let b = named(&mut st, "blockers", Params::new().with("id", P::Int(51)));
    assert_eq!(col_nodes(&b, 0), vec![12, 17]);
    let t = named(
        &mut st,
        "blockers",
        Params::new()
            .with("id", P::Int(51))
            .with("transitive", P::Bool(true)),
    );
    assert_eq!(t.rows.len(), 3);
    assert_eq!(t.rows[2][0..3], [n(14), V::Int(2), n(17)]);
    let r = named(&mut st, "ready", Params::new().with("scope", P::Int(88)));
    assert_eq!(col_nodes(&r, 0), vec![92, 95, 97, 98]);
    let tr = named(
        &mut st,
        "tree",
        Params::new()
            .with("id", P::Int(88))
            .with("depth", P::Int(1)),
    );
    assert_eq!(col_nodes(&tr, 0), vec![88, 89, 90, 92, 93, 95, 97]);
    let l = named(&mut st, "loop", Params::new().with("plan", P::Int(130)));
    assert_eq!(l.rows.len(), 2);
    assert_eq!(
        l.rows[0][0..4],
        [V::Int(1), V::Int(2), V::Int(1), V::Int(1)]
    );
    let s = rows(&mut st, "CALL std.show(ids: [#51, #12]) YIELD n RETURN n");
    assert_eq!(col_nodes(&s, 0), vec![12, 51]);
}

/// Runtime state at a tip ([50 §3.8]): leases and `claimed`; a past view refuses them (E302).
#[test]
fn runtime_state_exists_at_tips() {
    let mut st = store();
    let o = rows(
        &mut st,
        "MATCH (t:task) WHERE t.claimed RETURN t.id, t.lease.holder ORDER BY t.id LIMIT 2",
    );
    assert_eq!(
        o.rows,
        vec![vec![n(20), V::text("dev#9")], vec![n(21), V::text("dev#9")]]
    );
    let r = q(&mut st, "USE main~1 MATCH (t:task) WHERE t.ready RETURN t");
    assert_eq!(r.error.map(|e| e.code), Some("E302".into()));
    let l = rows(
        &mut st,
        "CALL leases() YIELD n, holder WHERE holder = 'dev#1' RETURN n",
    );
    assert_eq!(col_nodes(&l, 0), vec![89]);
}

/// History relations by replay ([50 §3.9] item 4): `history`, `log` over a range, `diff` with N05, `across`, `refs`.
#[test]
fn history_relations_replay_the_dag() {
    let mut st = store();
    let h = rows(
        &mut st,
        "CALL history(#93, field: 'priority') YIELD seq, after RETURN seq, after",
    );
    assert_eq!(h.rows.len(), 1, "{:?}", h.rows);
    let lg = rows(&mut st, "CALL log(main..lane/l5np) YIELD seq RETURN seq");
    assert_eq!(
        lg.rows.len(),
        2,
        "the lane's priority commit and #89's start"
    );
    let d = rows(
        &mut st,
        "CALL diff(lane/l5np..main) YIELD node, name RETURN node, name",
    );
    assert!(codes(&d).contains(&Code::N05));
    assert!(
        d.rows
            .iter()
            .any(|r| r[0] == n(93) && r[1] == V::text("priority"))
    );
    let a = rows(
        &mut st,
        "CALL across(refs: [main, lane/l5np], ids: [#93]) YIELD name, ref, diverged WHERE name = 'priority' RETURN ref, diverged",
    );
    assert_eq!(
        a.rows,
        vec![
            vec![V::text("lane/l5np"), V::Bool(true)],
            vec![V::text("main"), V::Bool(true)]
        ]
    );
    let refs = rows(
        &mut st,
        "CALL refs() YIELD name, ahead, behind RETURN name, ahead, behind",
    );
    assert!(
        refs.rows
            .contains(&vec![V::text("lane/l5np"), V::Int(2), V::Int(1)])
    );
}

/// `search()` ranks by BM25 or by the statistics-free scorer; ranks compare scores only with each other.
#[test]
fn search_ranks_with_both_scorers() {
    let mut st = store();
    let text =
        "CALL search('lease reclaim') YIELD node, score RETURN node, score ORDER BY score DESC";
    let b = rows(&mut st, text);
    assert_eq!(col_nodes(&b, 0)[..2], [51, 142]);
    assert!(col_nodes(&b, 0).contains(&143));
    let scores: Vec<f64> = b
        .rows
        .iter()
        .map(|r| match r[1] {
            V::Float(f) => f,
            _ => f64::NAN,
        })
        .collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]));
    let f = out(
        q_with(
            &mut st,
            text,
            Params::new(),
            Ablations {
                stat_free: true,
                ..Ablations::default()
            },
            &ctx(),
        ),
        "stat-free",
    );
    assert_eq!(f.rows[0][1], V::Float(6.0), "two title terms weigh 3 each");
    let m = rows(
        &mut st,
        "MATCH (n) WHERE text_match(n, 'reclaim -maintenance') RETURN n",
    );
    assert_eq!(col_nodes(&m, 0), vec![51]);
}

/// The target-set digest of [LQ/envelope §9.4] reproduces `envelope.cases` `target-set-digest`.
#[test]
fn the_target_set_digest_golden() {
    let uid = |last: u8| {
        let mut u = [
            0x01, 0x8f, 0x3c, 0x2e, 0x7a, 0x11, 0x7b, 0x3c, 0x9d, 0x5e, 0x4c, 0x2f, 0x1a, 0x0b,
            0x9e, 0x00,
        ];
        u[15] = last;
        u.to_vec()
    };
    let d = txrun::digest(&[(1, vec![uid(0x90), uid(0x93), uid(0x95)])]);
    assert_eq!(crate::value::hex(&d), "df505c0603f14d46b66a88d2f767999e");
}

/// `TX` semantics ([50 §3.10]): `EXPECT` (E401), `ASSERT` (E403), `DRY` with its diff and digest, `IF TARGETS`
/// (E402), create-or-bind (E410), and `PATCH`'s exactly-once rule (E404).
#[test]
fn tx_blocks_guard_and_dry_run() {
    let mut st = store();
    let c = ctx();
    let e = tx_lq(
        &mut st,
        "TX { MATCH (t:task) WHERE t.status = 'done' EXPECT 1 SET t.priority = 1 }",
        Params::new(),
        &c,
    );
    assert_eq!(e.error.as_ref().map(|e| e.code.as_str()), Some("E401"));
    let e = tx_lq(
        &mut st,
        "TX { SET #52.priority = 1; ASSERT #52.priority = 0 ELSE 'not P0' }",
        Params::new(),
        &c,
    );
    assert_eq!(e.error.as_ref().map(|e| e.code.as_str()), Some("E403"));
    let block = "TX { MATCH (t:task) WHERE t IN children(#9) EXPECT 3 SET t.priority = 1 } DRY";
    let dry = tx_lq(&mut st, block, Params::new(), &c);
    assert_eq!(dry.outcome, Outcome::Dry, "{:?}", dry.error);
    let digest = dry.targets.expect("a digest");
    assert_eq!(dry.statements[0].targets, vec![Nid(12), Nid(14), Nid(17)]);
    let mut stale = st.clone();
    let e = tx_lq(
        &mut stale,
        &block
            .replace(" DRY", "")
            .replace("TX {", "TX IF TARGETS '00000000000000000000000000000000' {"),
        Params::new(),
        &c,
    );
    assert_eq!(e.error.as_ref().map(|e| e.code.as_str()), Some("E402"));
    let applied = tx_lq(
        &mut st,
        &block.replace(" DRY", "").replace(
            "TX {",
            &format!("TX IF TARGETS '{}' {{", crate::value::hex(&digest)),
        ),
        Params::new(),
        &c,
    );
    assert_eq!(applied.outcome, Outcome::Ok, "{:?}", applied.error);
    let seq = applied.rev_new.expect("a commit");
    assert_eq!(
        st.dag.commits[&seq].changeset, dry.diff,
        "DRY diff = committed diff"
    );
    assert_eq!(
        (&dry.ready, &dry.other),
        (&applied.ready, &applied.other),
        "DRY affected = committed affected"
    );
    // Create-or-bind: one match binds, two refuse.
    let r = tx_lq(
        &mut st,
        "TX { CREATE (n:note {title: 'Note 5'}) UNLESS EXISTS { (n:note {title: 'Note 5'}) } }",
        Params::new(),
        &c,
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert!(r.rev_new.is_none(), "bound, nothing created");
    tx_lq(
        &mut st,
        "TX { CREATE (n:note {title: 'twin'}); CREATE (m:note {title: 'twin'}) }",
        Params::new(),
        &c,
    );
    let r = tx_lq(
        &mut st,
        "TX { CREATE (n:note {title: 'twin'}) UNLESS EXISTS { (n:note {title: 'twin'}) } }",
        Params::new(),
        &c,
    );
    assert_eq!(r.error.as_ref().map(|e| e.code.as_str()), Some("E410"));
    // PATCH: the removed text must occur exactly once.
    tx_lq(&mut st, "TX { SET #133.body = 'aaa b' }", Params::new(), &c);
    for (old, n) in [("aa", Some(2)), ("", None), ("zz", Some(0))] {
        let r = tx_lq(
            &mut st,
            "TX { PATCH #133.body REMOVE $old ADD 'x' }",
            Params::new().with("old", P::Text(old.into())),
            &c,
        );
        let e = r.error.expect("refused");
        assert_eq!(e.code, "E404");
        let occ = e.get("occurrences").cloned();
        assert_eq!(
            occ,
            Some(n.map_or(crate::err::Kv::Null, crate::err::Kv::Int))
        );
    }
    let ok = tx_lq(
        &mut st,
        "TX { PATCH #133.body REMOVE ' b' ADD ' c' }",
        Params::new(),
        &c,
    );
    assert_eq!(ok.outcome, Outcome::Ok, "{:?}", ok.error);
}

/// `DEFINE QUERY` stores a project query that a later read calls ([50 §4.4]).
#[test]
fn project_queries_define_and_run() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { DEFINE QUERY open_under($p: node) SHAPE node AS { MATCH (t:task) WHERE t IN children($p) AND t.status = 'open' RETURN t } }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let o = rows(&mut st, "CALL open_under(#9) YIELD t RETURN t");
    assert_eq!(col_nodes(&o, 0), vec![12, 14, 17]);
    let d = tx_lq(
        &mut st,
        "TX { DROP QUERY open_under }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(d.outcome, Outcome::Ok, "{:?}", d.error);
}

/// Every standard named query runs on the fixture store by name with typical arguments ([LQ/std §3]–§6): the
/// tree-derived ones refuse with E302 without a resolved tree ([LQ/std §2.8] item 4), every other returns its rows.
#[test]
fn every_standard_query_runs() {
    let arg = |name: &str| -> Option<P> {
        Some(match name {
            "scope" | "target" | "area" => P::Int(88),
            "plan" => P::Int(130),
            "id" => P::Int(51),
            "ids" => P::List(vec![P::Int(51), P::Int(12)]),
            "role" => P::Text("developer".into()),
            "path" => P::Text("crates/ecs/world.rs".into()),
            "since" => P::Text("s1".into()),
            "ref" => P::Text("lane/l5np".into()),
            "range" => P::Text("main...lane/l5np".into()),
            "refs" => P::List(vec![P::Text("main".into()), P::Text("lane/l5np".into())]),
            "agent" => P::Text("dev#1".into()),
            "lane" | "a" | "b" => P::Int(9),
            _ => return None,
        })
    };
    let tree = [
        "links_broken",
        "links_pending",
        "links_proposals",
        "files_replaced",
        "pack_header",
    ];
    for q in crate::lq::catalog::std_catalog() {
        let name = q.qname.strip_prefix("std.").expect("std");
        let mut params = Params::new();
        for (p, _, required) in &q.params {
            if *required {
                params = params.with(
                    p,
                    arg(p).unwrap_or_else(|| panic!("{name}: no argument for {p}")),
                );
            }
        }
        let mut st = store();
        let r = st.run(
            &Cmd::Query {
                input: QueryInput::Named(name.into()),
                params,
                at: None,
                mode: Mode::Run,
                strict_gql: false,
                ablations: Ablations::default(),
            },
            &ctx(),
        );
        if tree.contains(&name) {
            assert_eq!(code_of(&r), Some("E302"), "{name}: {:?}", r.error);
        } else if name == "violations" {
            // `lane/l5np` is not a staging ref.
            assert_eq!(code_of(&r), Some("E302"), "{name}: {:?}", r.error);
        } else {
            assert_eq!(r.outcome, Outcome::Ok, "{name}: {:?}", r.error);
        }
    }
}

fn code_of(r: &Reply) -> Option<&str> {
    r.error.as_ref().map(|e| e.code.as_str())
}

/// Expressions ([50 §3.3]; [LQ/canonical-ast §5.10]): string predicates, `IS NULL`, label tests, `CASE`, list
/// predicates, `COUNT {}`, durations and timestamps, conversions and `coalesce`.
#[test]
fn expressions_evaluate_by_their_rules() {
    let mut st = store();
    let one = |st: &mut Store, e: &str| -> V {
        let o = rows(st, &format!("MATCH (t {{id: #51}}) RETURN {e} AS x"));
        o.rows[0][0].clone()
    };
    assert_eq!(one(&mut st, "t.title STARTS WITH 'Wire'"), V::Bool(true));
    assert_eq!(one(&mut st, "t.title ENDS WITH 'claim'"), V::Bool(true));
    assert_eq!(one(&mut st, "t.title CONTAINS 'lease'"), V::Bool(true));
    assert_eq!(one(&mut st, "t.assignee IS NULL"), V::Bool(true));
    assert_eq!(one(&mut st, "t:task"), V::Bool(true));
    assert_eq!(one(&mut st, "t:note"), V::Bool(false));
    assert_eq!(
        one(
            &mut st,
            "CASE t.status WHEN 'done' THEN 1 WHEN 'open' THEN 2 ELSE 3 END"
        ),
        V::Int(2)
    );
    assert_eq!(
        one(&mut st, "CASE WHEN t.priority > 3 THEN 'hi' END"),
        V::Absent
    );
    assert_eq!(
        one(&mut st, "all(x IN [1, 2, 3] WHERE x > 0)"),
        V::Bool(true)
    );
    assert_eq!(
        one(&mut st, "any(x IN [1, 2, 3] WHERE x > 2)"),
        V::Bool(true)
    );
    assert_eq!(
        one(&mut st, "none(x IN [1, 2, 3] WHERE x > 2)"),
        V::Bool(false)
    );
    assert_eq!(one(&mut st, "COUNT { (b)-[:BLOCKS]->(t) }"), V::Int(2));
    assert_eq!(
        one(
            &mut st,
            "EXISTS { MATCH (b)-[:BLOCKS]->(t) WHERE b.id = #12 }"
        ),
        V::Bool(true)
    );
    assert_eq!(
        one(&mut st, "duration('2h') + duration('30m')"),
        V::Dur(9_000_000)
    );
    assert_eq!(
        one(&mut st, "datetime('2026-09-25T12:00:00Z') + 1d"),
        V::Time(crate::lq::lexer::datetime_ms("2026-09-26T12:00:00Z"))
    );
    assert_eq!(
        one(&mut st, "datetime('2026-09-26') - datetime('2026-09-25')"),
        V::Dur(86_400_000)
    );
    assert_eq!(one(&mut st, "3 / 2"), V::Float(1.5));
    assert_eq!(one(&mut st, "toInteger(7 / 2)"), V::Int(3));
    assert_eq!(one(&mut st, "toString(2.0)"), V::text("2.0"));
    assert_eq!(
        one(&mut st, "coalesce(t.assignee, 'nobody')"),
        V::text("nobody")
    );
    assert_eq!(one(&mut st, "size(t.title)"), V::Int(18));
    assert_eq!(one(&mut st, "substring(t.title, 0, 4)"), V::text("Wire"));
    assert_eq!(
        one(&mut st, "lower(t.title)"),
        V::text("wire lease reclaim")
    );
    assert_eq!(one(&mut st, "labels(t)"), V::List(vec![V::text("task")]));
    assert_eq!(one(&mut st, "round(2.675, 2)"), V::Float(2.68));
    let r = q(
        &mut st,
        "MATCH (t {id: #51}) RETURN 9223372036854775807 + 1 AS x",
    );
    assert_eq!(r.error.map(|e| e.code), Some("E103".into()));
    // An enumeration literal compares by rank: critical < high < normal < low.
    let o = rows(
        &mut st,
        "MATCH (r:rule) WHERE r.criticality < 'normal' RETURN r",
    );
    assert_eq!(col_nodes(&o, 0), vec![140, 141]);
    // ORDER BY an alias, LIMIT by a parameter, WITH DISTINCT.
    let o = out(
        q_with(
            &mut st,
            "MATCH (t:task) WHERE t IN children(#88) RETURN t.id AS i ORDER BY i DESC LIMIT $n",
            Params::new().with("n", P::Int(2)),
            Ablations::default(),
            &ctx(),
        ),
        "limit",
    );
    assert_eq!(col_nodes(&o, 0), vec![97, 95]);
    let o = rows(
        &mut st,
        "MATCH (f:finding) WITH DISTINCT f.round AS r RETURN r ORDER BY r",
    );
    assert_eq!(o.rows, vec![vec![V::Int(1)], vec![V::Int(2)]]);
}

/// The `show` family ([LQ/envelope §5.6]; [LQ/errors] open point 6): requested ids that yield no row are reported by
/// N01 (deleted here), N06 (created on another branch) or N12 (never allocated, not E111), the other rows print, and
/// the result exits 3; a request whose ids all print exits 0.
#[test]
fn show_reports_ids_that_yield_no_row() {
    let mut st = store();
    let d = tx_lq(
        &mut st,
        "TX { DELETE #142 REASON 'merged into #143' }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(d.outcome, Outcome::Ok, "{:?}", d.error);
    let c = tx_lq(
        &mut st,
        "TX ON lane/l5np { CREATE (n:note {title: 'Lane only'}) }",
        Params::new(),
        &crate::suite::orch_on("lane/l5np"),
    );
    assert_eq!(c.outcome, Outcome::Ok, "{:?}", c.error);
    let lane_only = st.next_id - 1;
    let show = |st: &mut Store, ids: Vec<P>| {
        st.run(
            &Cmd::Query {
                input: QueryInput::Named("show".into()),
                params: Params::new().with("ids", P::List(ids)),
                at: None,
                mode: Mode::Run,
                strict_gql: false,
                ablations: Ablations::default(),
            },
            &ctx(),
        )
    };
    let r = show(
        &mut st,
        vec![
            P::Int(999),
            P::Int(51),
            P::Int(142),
            P::Int(i64::from(lane_only)),
        ],
    );
    assert_eq!(r.exit, 3, "{:?}", r.error);
    let o = out(r, "show");
    assert_eq!(col_nodes(&o, 0), vec![51]);
    let texts: Vec<(Code, String)> = o
        .notices
        .iter()
        .map(|x| (x.code, x.message.clone()))
        .collect();
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert_eq!(texts[0].0, Code::N01);
    assert!(
        texts[0].1.starts_with("#142 is deleted in this view (rev ")
            && texts[0].1.ends_with("\"merged into #143\")"),
        "{texts:?}"
    );
    assert_eq!(texts[1].0, Code::N06);
    assert!(
        texts[1].1.starts_with(&format!(
            "#{lane_only} is not in this view: created on lane/l5np at rev "
        )),
        "{texts:?}"
    );
    assert_eq!(
        texts[2],
        (
            Code::N12,
            "#999 was never allocated in this store".to_string()
        )
    );
    let r = show(&mut st, vec![P::Int(51), P::Int(12)]);
    assert_eq!(r.exit, 0);
    assert_eq!(col_nodes(&out(r, "show"), 0), vec![12, 51]);
    // Everywhere else a never-allocated literal id stays E111.
    let r = q(&mut st, "MATCH (n) WHERE n IN [#999] RETURN n");
    assert_eq!(code_of(&r), Some("E111"));
}

/// [LQ/envelope §9.4] row 3a: a binding holds every variable the statement's mutations use, wherever in the mutation it
/// stands (here `u` only inside a function argument), in variable-index order.
#[test]
fn the_digest_binds_every_variable_a_mutation_uses() {
    let mut st = store();
    let dry = tx_lq(
        &mut st,
        "TX { MATCH (t:task), (u:task) WHERE t.id = #12 AND u.id = #14 EXPECT 1 SET t.title = coalesce(u.title, 'x') } DRY",
        Params::new(),
        &ctx(),
    );
    assert_eq!(dry.outcome, Outcome::Dry, "{:?}", dry.error);
    let uid = |n: u32| st.alloc.uids[&Nid(n)].0.to_vec();
    let mut binding = uid(12);
    binding.extend(uid(14));
    assert_eq!(dry.targets, Some(txrun::digest(&[(1, vec![binding])])));
}

/// [50 §3.3]: an int and a float of the same value are equal under every comparison, `DISTINCT` groups them, and a
/// division gives `0.0`, never `-0.0`.
#[test]
fn numbers_compare_and_group_by_value() {
    let mut st = store();
    let o = rows(
        &mut st,
        "UNWIND [2.0] AS v RETURN v >= 2, v <= 2, v = 2, v > 2, v < 2, 2 <= v, 2 > v",
    );
    let b = V::Bool;
    assert_eq!(
        o.rows,
        vec![vec![
            b(true),
            b(true),
            b(true),
            b(false),
            b(false),
            b(true),
            b(false)
        ]]
    );
    let d = rows(&mut st, "UNWIND [2, 2.0, 3] AS x RETURN DISTINCT x");
    assert_eq!(d.rows, vec![vec![V::Int(2)], vec![V::Int(3)]]);
    let z = rows(
        &mut st,
        "UNWIND [1] AS x RETURN 0.0 / -1.0 = 0.0, toString(0.0 / -1.0)",
    );
    assert_eq!(z.rows, vec![vec![V::Bool(true), V::text("0.0")]]);
    let g = rows(
        &mut st,
        "UNWIND [1, 1.0, 2] AS x RETURN x, count(*) AS c ORDER BY c DESC",
    );
    assert_eq!(g.rows[0][1], V::Int(2), "{:?}", g.rows);
}

/// [LQ/canonical-ast §5.7] V4: an aggregating `WITH` over no rows yields one row, as `RETURN` does; its `ORDER BY`
/// aggregates the group; `WITH *, count(*)` groups by every binding in scope.
#[test]
fn aggregating_with_clauses() {
    let mut st = store();
    let e = rows(
        &mut st,
        "MATCH (t:task) WHERE t.title = 'zzz' WITH count(t) AS n, sum(t.estimate) AS s, avg(t.estimate) AS a RETURN n, s, a",
    );
    assert_eq!(e.rows, vec![vec![V::Int(0), V::Int(0), V::Absent]]);
    let r = rows(
        &mut st,
        "MATCH (t:task) WHERE t.title = 'zzz' RETURN count(t) AS n, sum(t.estimate) AS s, avg(t.estimate) AS a",
    );
    assert_eq!(e.rows, r.rows);
    let o = rows(
        &mut st,
        "UNWIND [1, 2, 2, 2] AS x WITH x, count(*) AS c ORDER BY count(*) DESC LIMIT 1 RETURN x, c",
    );
    assert_eq!(o.rows, vec![vec![V::Int(2), V::Int(3)]]);
    let s = rows(
        &mut st,
        "UNWIND [1, 2] AS x WITH *, count(*) AS c RETURN x, c",
    );
    assert_eq!(
        s.rows,
        vec![vec![V::Int(1), V::Int(1)], vec![V::Int(2), V::Int(1)]]
    );
    let s0 = rows(&mut st, "UNWIND [] AS x WITH *, count(*) AS c RETURN c");
    assert!(s0.rows.is_empty(), "{:?}", s0.rows);
}

/// [LQ/canonical-ast §5.7] V4, [50 §3.4] rule 6: an aggregate in an aggregating `WITH`'s `ORDER BY` reads the bindings
/// the `WITH` creates — an aliased grouping key, an aggregate item — with the values a `RETURN` gives its items.
#[test]
fn aggregating_with_order_keys_read_the_items() {
    let mut st = store();
    for (key, want) in [
        ("sum(y) DESC", (2, 2)),
        ("sum(y)", (1, 1)),
        ("size(collect(y)) DESC", (2, 2)),
        ("collect(y) DESC", (3, 1)),
        ("max(c) DESC", (2, 2)),
        ("min(c * y) DESC", (2, 2)),
        ("count(y) DESC, y DESC", (2, 2)),
    ] {
        let with = rows(
            &mut st,
            &format!(
                "UNWIND [1, 2, 2, 3] AS x WITH x AS y, count(*) AS c ORDER BY {key} LIMIT 1 RETURN y, c"
            ),
        );
        assert_eq!(
            with.rows,
            vec![vec![V::Int(want.0), V::Int(want.1)]],
            "{key}"
        );
        let ret = rows(
            &mut st,
            &format!(
                "UNWIND [1, 2, 2, 3] AS x RETURN x AS y, count(*) AS c ORDER BY {key} LIMIT 1"
            ),
        );
        assert_eq!(with.rows, ret.rows, "{key}");
    }
    // `WITH *` keeps the bindings in scope beside the new ones.
    let star = rows(
        &mut st,
        "UNWIND [1, 2, 2, 3] AS x WITH *, x * 10 AS y, count(*) AS c ORDER BY sum(y) DESC LIMIT 1 RETURN x, y, c",
    );
    assert_eq!(star.rows, vec![vec![V::Int(2), V::Int(20), V::Int(2)]]);
}

/// [50 §3.5]: an ordered comparison of an enumeration with a text ranks the text in the enumeration's field, however
/// either operand was produced; a text that is no value of the field compares false.
#[test]
fn enumerations_compare_by_rank_through_any_expression() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { CREATE (a:rule {title: 'low one', text: 'x', criticality: 'low'}); CREATE (b:rule {title: 'high one', text: 'y', criticality: 'high'}) }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let titles = |o: &QueryOut| -> Vec<V> { o.rows.iter().map(|x| x[0].clone()).collect() };
    let direct = rows(
        &mut st,
        "MATCH (r:rule) WHERE r.criticality < 'normal' AND r.title ENDS WITH ' one' RETURN r.title",
    );
    assert_eq!(titles(&direct), vec![V::text("high one")]);
    for q in [
        "MATCH (r:rule) WHERE r.title ENDS WITH ' one' WITH r.title AS t, r.criticality AS c WHERE c < 'normal' RETURN t",
        "MATCH (r:rule) WHERE r.title ENDS WITH ' one' AND coalesce(r.criticality, 'normal') < 'normal' RETURN r.title",
        "MATCH (r:rule) WHERE r.title ENDS WITH ' one' AND 'normal' > CASE WHEN true THEN r.criticality END RETURN r.title",
    ] {
        assert_eq!(titles(&rows(&mut st, q)), titles(&direct), "{q}");
    }
    // A computed text that is no value of the field: the binder cannot refuse it (E102), and it compares false.
    let none = rows(
        &mut st,
        "MATCH (r:rule) WHERE r.title ENDS WITH ' one' WITH r.criticality AS c WHERE c < trim('zzz') OR c >= trim('zzz') RETURN c",
    );
    assert!(none.rows.is_empty(), "{:?}", none.rows);
}

/// [50 §3.5]: an enum constant is a value of its field and ranks there, so `ORDER BY`, `min`, `max` and `collect` over
/// a column that mixes stored values and constants — through `CASE` in either arm order, `coalesce` or a parameter —
/// order every value by declared rank (criticality: critical < high < normal < low).
#[test]
fn enum_constants_order_with_stored_values_by_rank() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { CREATE (a:rule {title: 'ra', text: 'x', criticality: 'low'}); CREATE (b:rule {title: 'rb', text: 'y', criticality: 'critical'}); CREATE (c:rule {title: 'rc', text: 'z', criticality: 'low'}) }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let shown = |o: &QueryOut| -> Vec<Vec<String>> {
        o.rows
            .iter()
            .map(|r| r.iter().map(crate::lq::eval::expr::display).collect())
            .collect()
    };
    let params = Params::new().with("v", P::Text("normal".into()));
    let run = |st: &mut Store, text: &str| {
        out(
            q_with(st, text, params.clone(), Ablations::default(), &ctx()),
            text,
        )
    };
    for k in [
        "CASE WHEN r.title = 'rc' THEN 'normal' ELSE r.criticality END",
        "CASE WHEN r.title <> 'rc' THEN r.criticality ELSE 'normal' END",
        "CASE WHEN r.title = 'rc' THEN normal ELSE r.criticality END",
        "coalesce(CASE WHEN r.title <> 'rc' THEN r.criticality END, 'normal')",
        "CASE WHEN r.title = 'rc' THEN $v ELSE r.criticality END",
    ] {
        let from = format!("MATCH (r:rule) WHERE r.title IN ['ra', 'rb', 'rc'] WITH r, {k} AS k");
        let asc = run(&mut st, &format!("{from} RETURN r.title ORDER BY k"));
        assert_eq!(shown(&asc), [["rb"], ["rc"], ["ra"]], "{k}");
        let desc = run(&mut st, &format!("{from} RETURN r.title ORDER BY k DESC"));
        assert_eq!(shown(&desc), [["ra"], ["rc"], ["rb"]], "{k}");
        let agg = run(
            &mut st,
            &format!("{from} RETURN min(k), max(k), collect(k)"),
        );
        assert_eq!(
            shown(&agg),
            [["critical", "low", "[critical, normal, low]"]],
            "{k}"
        );
        assert!(
            matches!(&agg.rows[0][2], V::List(l) if l.iter().all(|x| matches!(x, V::Enum(_)))),
            "{k}: {:?}",
            agg.rows
        );
    }
    // A constant is the stored value of its name: it groups with it.
    let grouped = run(
        &mut st,
        "MATCH (r:rule) WHERE r.title IN ['ra', 'rb', 'rc'] WITH CASE WHEN r.title = 'rb' THEN 'low' ELSE r.criticality END AS k, count(*) AS n RETURN k, n",
    );
    assert_eq!(shown(&grouped), [["low", "3"]]);
}

/// [50 §3.9] item 5: a project query's definition is resolved at the caller's branch tip and bound against the part's
/// view, so a past view runs a query defined after it.
#[test]
fn a_past_view_runs_a_query_defined_later() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { DEFINE QUERY qb() SHAPE node AS { MATCH (t:task) WHERE t IN children(#9) RETURN t } }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let o = rows(&mut st, "USE main~3 CALL qb() YIELD t RETURN t");
    assert_eq!(col_nodes(&o, 0), vec![12, 14, 17]);
}

/// [50 §3.6]: a literal id that names no live node gives its notice at any position of a path, once per id.
#[test]
fn literal_ids_give_notices_at_every_position() {
    let mut st = store();
    let r = tx_lq(&mut st, "TX { DELETE #142 }", Params::new(), &ctx());
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let first = rows(&mut st, "MATCH (x {id: #142})-->(t) RETURN t");
    assert!(codes(&first).contains(&Code::N01), "{:?}", first.notices);
    let step = rows(&mut st, "MATCH (t:task)-->(#142) RETURN t");
    assert!(step.rows.is_empty());
    assert!(codes(&step).contains(&Code::N01), "{:?}", step.notices);
    let n01 = step.notices.iter().filter(|x| x.code == Code::N01).count();
    assert_eq!(n01, 1, "once per id");
}

/// [LQ/errors §5.6]: W01 counts the rows of the outermost `WHERE` only, whatever nested `WHERE`s it holds; N10 counts
/// each input row of an aggregate whose argument divided by zero, and each row of a `WITH`.
#[test]
fn counted_warnings_count_outer_rows() {
    let mut st = store();
    let w01 = |o: &QueryOut| {
        o.warnings
            .iter()
            .find(|x| x.code == Code::W01)
            .and_then(|x| x.count)
    };
    let plain = rows(&mut st, "MATCH (t:task) WHERE t.estimate > 1 RETURN t");
    let nested = rows(
        &mut st,
        "MATCH (t:task) WHERE t.estimate > 1 OR EXISTS { MATCH (b:task)-[:BLOCKS]->(t) WHERE b.title = 'zz' } RETURN t",
    );
    assert!(w01(&plain).is_some_and(|c| c > 0), "{:?}", plain.warnings);
    assert_eq!(w01(&plain), w01(&nested));
    let sum = rows(&mut st, "UNWIND [1, 2] AS x RETURN sum(x / 0) AS s");
    assert_eq!(sum.rows, vec![vec![V::Int(0)]]);
    let n10 = sum
        .notices
        .iter()
        .find(|x| x.code == Code::N10)
        .expect("N10");
    assert_eq!(n10.count, Some(2));
    let with = rows(
        &mut st,
        "UNWIND [1, 2, 3] AS x WITH x / 0 AS y RETURN count(*)",
    );
    let n10 = with
        .notices
        .iter()
        .find(|x| x.code == Code::N10)
        .expect("N10");
    assert_eq!(n10.count, Some(3));
}

/// [50 §3.8] W03: checked on the rows the result emits, with `done` read anywhere in the part.
#[test]
fn w03_reads_the_emitted_rows() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { SET #92.status = 'cancelled' }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let has = |o: &QueryOut| codes(o).contains(&Code::W03);
    assert!(has(&rows(
        &mut st,
        "MATCH (t:task) WHERE t.done RETURN t.id"
    )));
    assert!(!has(&rows(
        &mut st,
        "MATCH (t:task) WHERE t.done RETURN t.id ORDER BY t.id LIMIT 0"
    )));
    assert!(has(&rows(
        &mut st,
        "MATCH (t {id: #92}) RETURN t.id ORDER BY t.done"
    )));
}

/// [50 §3.6] last item: a quantified group whose node patterns admit `DELETED` goes on through a deleted node; one
/// whose do not stops there.
#[test]
fn quantified_groups_go_through_deleted_nodes_only_when_admitted() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { CREATE (a:note {title: 'qa'}); CREATE (b:note {title: 'qb'}); CREATE (c:note {title: 'qc'}); CREATE (a)-[:CITES]->(b); CREATE (b)-[:CITES]->(c) }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let id = |st: &mut Store, t: &str| -> u32 {
        col_nodes(
            &rows(st, &format!("MATCH (n:note {{title: '{t}'}}) RETURN n")),
            0,
        )[0]
    };
    let (a, b, c) = (id(&mut st, "qa"), id(&mut st, "qb"), id(&mut st, "qc"));
    let r = tx_lq(
        &mut st,
        &format!("TX {{ DELETE #{b} }}"),
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let through = rows(
        &mut st,
        &format!("MATCH (s {{id: #{a}}}) ((x:DELETED)-[:CITES]->(y:DELETED))+ (e) RETURN e"),
    );
    assert_eq!(col_nodes(&through, 0), vec![c]);
    let stops = rows(
        &mut st,
        &format!("MATCH (s {{id: #{a}}}) ((x)-[:CITES]->(y))+ (e) RETURN e"),
    );
    assert!(stops.rows.is_empty(), "{:?}", stops.rows);
    let edge = rows(
        &mut st,
        &format!("MATCH (s {{id: #{a}}})-[:CITES]->+(e) RETURN e"),
    );
    assert!(edge.rows.is_empty(), "{:?}", edge.rows);
    let tomb = rows(
        &mut st,
        &format!("MATCH (s {{id: #{a}}})-[:CITES]->+(e:DELETED) RETURN e"),
    );
    assert_eq!(col_nodes(&tomb, 0), vec![b]);
}

/// [LQ/std §2.10]: `toInteger` of a text applies the float path's finiteness and range; `abs` of a duration is
/// checked.
#[test]
fn conversions_check_their_ranges() {
    let mut st = store();
    let o = rows(
        &mut st,
        "UNWIND [1] AS x RETURN toInteger('inf'), toInteger('1e300'), toInteger('NaN'), toInteger('12'), toInteger(' 7.9 '), toInteger('-3.5'), abs(duration('2h') - duration('4h'))",
    );
    assert_eq!(
        o.rows,
        vec![vec![
            V::Absent,
            V::Absent,
            V::Absent,
            V::Int(12),
            V::Int(7),
            V::Int(-3),
            V::Dur(7_200_000)
        ]]
    );
}

/// BM25 golden ([50 §5.5]; [LQ/std §2.9]) on a store of three notes, `alpha beta`, `alpha alpha gamma` and `delta`:
/// N = 3, df(alpha) = 2, the title's mean length 2, k1 = 1.2, b = 0.75, title weight 3, so `alpha` scores
/// 3·ln(1.6)·2·2.2/(2 + 1.2·1.375) = 1.700 for the second and 3·ln(1.6) = 1.410 for the first; a prefix term matches the
/// same, an excluded term drops a document; the statistics-free scorer weighs a title match 3, ties by recency then id.
#[test]
fn bm25_scores_a_small_store() {
    let mut s = crate::suite::S::base();
    s.ok(
        tx(vec![
            crate::suite::node("a", "note", &[("title", P::Text("alpha beta".into()))]),
            crate::suite::node(
                "b",
                "note",
                &[("title", P::Text("alpha alpha gamma".into()))],
            ),
            crate::suite::node("c", "note", &[("title", P::Text("delta".into()))]),
        ]),
        crate::suite::orch(),
    );
    let mut st = s.st;
    let live = st
        .dag
        .state_at(st.dag.live("main").and_then(|r| r.tip), &st.alloc)
        .nodes
        .values()
        .filter(|x| x.live())
        .count();
    assert_eq!(live, 3, "the store holds the three notes only");
    let q = |st: &mut Store, terms: &str, ab: Ablations| {
        out(
            q_with(
                st,
                &format!(
                    "CALL search('{terms}') YIELD node, score, field RETURN node, score, field"
                ),
                Params::new(),
                ab,
                &ctx(),
            ),
            terms,
        )
        .rows
    };
    let row = |n: u32, s: f64| vec![V::Node(Nid(n)), V::Float(s), V::text("title")];
    assert_eq!(
        q(&mut st, "alpha", Ablations::default()),
        vec![row(2, 1.7), row(1, 1.41)]
    );
    assert_eq!(
        q(&mut st, "alph*", Ablations::default()),
        vec![row(2, 1.7), row(1, 1.41)]
    );
    assert_eq!(
        q(&mut st, "alpha -gamma", Ablations::default()),
        vec![row(1, 1.41)]
    );
    assert_eq!(
        q(&mut st, "beta delta", Ablations::default()),
        vec![row(3, 3.699), row(1, 2.942)]
    );
    let free = Ablations {
        stat_free: true,
        ..Ablations::default()
    };
    assert_eq!(q(&mut st, "alpha", free), vec![row(1, 3.0), row(2, 3.0)]);
}

/// [50 §3.6]: a `DELETED` pattern binds a tombstone, whose properties are those of the deletion.
#[test]
fn tombstones_read_their_deletion() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { DELETE #141 REPLACED BY #140 REASON 'dup of #140' }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let o = rows(
        &mut st,
        "MATCH (x:DELETED {id: #141}) RETURN x.kind, x.title, x.deleted_reason, x.replaced_by, x.deleted_by IS NULL",
    );
    assert_eq!(
        o.rows,
        vec![vec![
            V::text("rule"),
            V::text("Never kill processes by image name"),
            V::text("dup of #140"),
            V::Node(Nid(140)),
            V::Bool(false)
        ]]
    );
    let live = rows(&mut st, "MATCH (x {id: #141}) RETURN x");
    assert!(live.rows.is_empty());
}
