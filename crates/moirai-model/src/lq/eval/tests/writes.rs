//! Unit tests of `TX` blocks on the fixture store ([50 §3.10]; [LQ/envelope §9]): every statement form compiled to the
//! kernel's operations — counter increments in both spellings, the pinned commits of created edges, `DELETE` of
//! every edge a variable holds, joins on a variable bound to several values, each repeated binding written once, the
//! leases earlier statements took —
//! the named-query validators V10 and V11 on `DEFINE QUERY` and `DROP QUERY`, GR-019 on a knowledge `CREATE`, and
//! the refusal payloads of [LQ/errors §5.5] and §5.7.

use super::*;
use crate::err::Kv;
use crate::value::Nid;

fn code(r: &Reply) -> Option<&str> {
    r.error.as_ref().map(|e| e.code.as_str())
}

fn ok(st: &mut Store, text: &str) -> Reply {
    let r = tx_lq(st, text, Params::new(), &ctx());
    assert_eq!(r.outcome, Outcome::Ok, "{text}: {:?}", r.error);
    r
}

fn one(st: &mut Store, text: &str) -> V {
    rows(st, text).rows[0][0].clone()
}

/// [50 §3.2], §3.10 item 6: `SET n.c = n.c + k` and `SET n.c = k + n.c` are the counter's `Incr`; k is an integer of
/// the same node's counter; any other assignment to a counter is refused (E103).
#[test]
fn counter_increments_in_both_spellings() {
    let mut st = store();
    ok(&mut st, "TX { CREATE (n:note {title: 'inc'}) }");
    let r = ok(
        &mut st,
        "TX { MATCH (n:note) WHERE n.title = 'inc' EXPECT 1 SET n.incidents = 1 + n.incidents }",
    );
    let c = &st.dag.commits[&r.rev_new.expect("a commit")];
    assert!(
        c.changeset
            .values()
            .any(|(_, after)| format!("{after:?}").contains("Counter")),
        "{:?}",
        c.changeset
    );
    ok(
        &mut st,
        "TX { MATCH (n:note) WHERE n.title = 'inc' EXPECT 1 SET n.incidents = n.incidents + 2 }",
    );
    assert_eq!(
        one(
            &mut st,
            "MATCH (n:note) WHERE n.title = 'inc' RETURN n.incidents"
        ),
        V::Int(3)
    );
    let other = tx_lq(
        &mut st,
        "TX { MATCH (n:note), (m:note) WHERE n.title = 'inc' AND m.id = #7 EXPECT 1 SET n.incidents = 1 + m.incidents }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(code(&other), Some("E103"), "{:?}", other.error);
    // An increment that evaluates absent is refused, never taken as 0.
    let absent = tx_lq(
        &mut st,
        "TX { MATCH (n:note) WHERE n.title = 'inc' EXPECT 1 SET n.incidents = n.incidents + toInteger('x') }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(code(&absent), Some("E103"), "{:?}", absent.error);
}

/// [50 §3.10] item 6; [F08 §10]: the pinned commit of an edge that a node's `CREATE` or an edge's `CREATE` writes is
/// kept; another property of a created edge, and a pin on a kind without one, are refused.
#[test]
fn created_edges_keep_their_pinned_commit() {
    let mut st = store();
    let past = st
        .dag
        .rev_commit(
            "main~1",
            &st.rev_ctx(&st.resolve(&ctx(), false).expect("caller")),
        )
        .expect("resolves")
        .expect("a commit");
    ok(
        &mut st,
        "TX { CREATE (n:note {title: 'p1'})-[:CITES {pinned: 'main~1'}]->(#7) }",
    );
    assert_eq!(
        one(
            &mut st,
            "MATCH (n:note {title: 'p1'})-[e:CITES]->() RETURN e.pinned"
        ),
        V::Rev(past)
    );
    let tip = st.dag.live("main").and_then(|r| r.tip).expect("a tip");
    ok(
        &mut st,
        "TX { CREATE (#1)-[:CITES {pinned: 'main'}]->(#7) }",
    );
    assert_eq!(
        one(
            &mut st,
            "MATCH (a {id: #1})-[e:CITES]->(b {id: #7}) RETURN e.pinned"
        ),
        V::Rev(tip)
    );
    for bad in [
        "TX { CREATE (#14)-[:BLOCKS {pinned: 'main'}]->(#93) }",
        "TX { CREATE (n:note {title: 'p2'})-[:CITES {flagged: true}]->(#7) }",
        "TX { CREATE (#2)-[:CITES {pinned: 'no-such-ref'}]->(#7) }",
    ] {
        let r = tx_lq(&mut st, bad, Params::new(), &ctx());
        assert_ne!(r.outcome, Outcome::Ok, "{bad} is refused");
    }
}

/// [LQ/canonical-ast §5.7] V10: a variable a `MATCH … EXPECT` bound to several edges names all of them in a later
/// `DELETE`; a later `MATCH` that joins on a variable bound to several nodes binds each in turn; a `DELETE` target that
/// is neither a node nor an edge is refused.
#[test]
fn later_statements_use_every_value_of_a_variable() {
    let mut st = store();
    ok(
        &mut st,
        "TX { MATCH (a:task)-[e:BLOCKS]->(b:task) WHERE b.id = #51 EXPECT 2 SET a.priority = 1; DELETE e }",
    );
    assert_eq!(
        one(
            &mut st,
            "MATCH (a)-[:BLOCKS]->(b {id: #51}) RETURN count(*) AS n"
        ),
        V::Int(0)
    );
    let mut st = store();
    ok(
        &mut st,
        "TX { MATCH (t:task) WHERE t IN children(#9) EXPECT 3 SET t.estimate = 1; MATCH (t)-[:BLOCKS]->(u) EXPECT 3 SET u.estimate = 2 }",
    );
    let est = rows(
        &mut st,
        "MATCH (t:task) WHERE t.id IN [#12, #14, #17, #51] RETURN t.id, t.estimate ORDER BY t.id",
    );
    assert_eq!(
        est.rows,
        vec![
            vec![V::Node(Nid(12)), V::Int(1)],
            vec![V::Node(Nid(14)), V::Int(1)],
            vec![V::Node(Nid(17)), V::Int(2)],
            vec![V::Node(Nid(51)), V::Int(2)],
        ]
    );
    let r = tx_lq(
        &mut st,
        "TX { MATCH (t:task {id: #93}) EXPECT 1 SET t.estimate = 3; DELETE t.assignee }",
        Params::new(),
        &ctx(),
    );
    assert_ne!(r.outcome, Outcome::Ok);
}

/// [LQ/canonical-ast §5.7] V10, [50 §3.10] item 6: a variable whose rows repeat a value holds each value once, in
/// order of first binding, so a later statement writes or joins on it once; a statement deletes each node and edge
/// once however many of its rows name it; a `DELETE` of an edge an earlier statement removed is `not_found` (exit 3),
/// as one of a node is, never E401.
#[test]
fn repeated_bindings_are_written_once() {
    let mut st = store();
    ok(
        &mut st,
        "TX { CREATE (n:note {title: 'n1'}); CREATE (a:task {title: 't1'}); CREATE (b:task {title: 't2'}); CREATE (a)-[:BLOCKS]->(b) }",
    );
    let count = |st: &mut Store, q: &str| one(st, q);
    let notes = "MATCH (n:note) WHERE n.title = 'n1' RETURN count(n)";
    let edges = "MATCH (a:task)-[:BLOCKS]->(b:task) WHERE a.title = 't1' RETURN count(*)";
    for (block, query) in [
        // A later statement deletes a node the rows of an earlier one repeat once.
        (
            "TX { MATCH (n:note), (c:task) WHERE n.title = 'n1' AND c.title IN ['t1', 't2'] EXPECT 2 SET c.estimate = 1; DELETE n }",
            notes,
        ),
        // One statement deletes a node or an edge its rows repeat once.
        (
            "TX { MATCH (n:note), (c:task) WHERE n.title = 'n1' AND c.title IN ['t1', 't2'] EXPECT 2 DELETE n }",
            notes,
        ),
        (
            "TX { MATCH (a:task)-[e:BLOCKS]->(b:task), (c:task) WHERE a.title = 't1' AND c.title IN ['t1', 't2'] EXPECT 2 DELETE e }",
            edges,
        ),
        (
            "TX { MATCH (a:task)-[e:BLOCKS]->(b:task), (c:task) WHERE a.title = 't1' AND c.title IN ['t1', 't2'] EXPECT 2 SET c.estimate = 1; DELETE e, e }",
            edges,
        ),
        // A later `MATCH` joins on each distinct value once: one edge, not one per repeated row.
        (
            "TX { MATCH (a:task), (c:task) WHERE a.title IN ['t1', 't2'] AND c.title IN ['t1', 't2'] EXPECT 4 SET a.estimate = 2; MATCH (a)-[e:BLOCKS]->(b) EXPECT 1 DELETE e }",
            edges,
        ),
    ] {
        let mut s = st.clone();
        ok(&mut s, block);
        assert_eq!(count(&mut s, query), V::Int(0), "{block}");
    }
    let gone = tx_lq(
        &mut st,
        "TX { MATCH (a:task)-[e:BLOCKS]->(b:task) WHERE a.title = 't1' EXPECT 1 DELETE e; DELETE e }",
        Params::new(),
        &ctx(),
    )
    .error
    .expect("not_found");
    assert_eq!((gone.code.as_str(), gone.exit), ("not_found", 3));
    assert_eq!(gone.get("what"), Some(&Kv::Str("edge".into())));
    assert!(gone.detail.starts_with("edge #"), "{}", gone.detail);
    assert!(
        gone.detail.ends_with("is not live on main"),
        "{}",
        gone.detail
    );
    let twice = tx_lq(
        &mut st,
        "TX { MATCH (n:note) WHERE n.title = 'n1' EXPECT 1 DELETE n; DELETE n }",
        Params::new(),
        &ctx(),
    )
    .error
    .expect("not_found");
    assert_eq!((twice.code.as_str(), twice.exit), ("not_found", 3));
    assert_eq!(twice.get("what"), Some(&Kv::Str("node".into())));
}

/// [50 §3.10] item 2: a lease an earlier statement of the block took is visible to a later statement's runtime
/// predicates.
#[test]
fn leases_taken_earlier_in_a_block_are_visible() {
    let mut st = store();
    let r = tx_lq(
        &mut st,
        "TX { CALL tx.claim(ids: [#92]); MATCH (t:task {id: #92}) WHERE t.claimed EXPECT 1 SET t.estimate = 3 }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
}

/// [F19 §12.5.6]; [50 §3.10] item 5: a write that leaves a named query unbound or on a call cycle is refused with
/// E405, the checks reading the parsed texts (keyword case and comments do not matter).
#[test]
fn writes_keep_named_queries_bound_and_acyclic() {
    let mut st = store();
    ok(
        &mut st,
        "TX { DEFINE QUERY qb() SHAPE node AS { MATCH (t:task) WHERE t IN children(#9) RETURN t } }",
    );
    ok(
        &mut st,
        "TX { DEFINE QUERY qa() SHAPE node AS { CALL qb() YIELD t RETURN t } }",
    );
    let drop = tx_lq(&mut st, "TX { DROP QUERY qb }", Params::new(), &ctx());
    assert_eq!(code(&drop), Some("E405"), "{:?}", drop.error);
    assert!(
        drop.error.as_ref().unwrap().detail.contains("QueryInvalid"),
        "{:?}",
        drop.error
    );
    let breaks = tx_lq(
        &mut st,
        "TX { DEFINE QUERY qb() AS { MATCH (n:note) RETURN n.title AS title } }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(code(&breaks), Some("E405"), "{:?}", breaks.error);
    ok(
        &mut st,
        "TX { DEFINE QUERY qd() SHAPE node AS { MATCH (t:task) WHERE t IN children(#9) RETURN t } }",
    );
    ok(
        &mut st,
        "TX { DEFINE QUERY qc() SHAPE node AS { call qd() YIELD t RETURN t } }",
    );
    let cycle = tx_lq(
        &mut st,
        "TX { DEFINE QUERY qd() SHAPE node AS { call qc() YIELD t RETURN t } }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(code(&cycle), Some("E405"), "{:?}", cycle.error);
    assert!(
        cycle.error.as_ref().unwrap().detail.contains("QueryCycle"),
        "{:?}",
        cycle.error
    );
    ok(
        &mut st,
        "TX { DEFINE QUERY qe() SHAPE node AS { MATCH (t:task) // CALL qe() later\n RETURN t } }",
    );
    // The definitions still run.
    let o = rows(&mut st, "CALL qa() YIELD t RETURN t");
    assert_eq!(col_nodes(&o, 0), vec![12, 14, 17]);
}

/// [RULES/status-machines] GR-019: a rule or a decision that a block creates without the owner attestation starts
/// `proposed`, and a `CREATE` that names another status is refused (E404).
#[test]
fn knowledge_creates_start_proposed() {
    let mut st = store();
    ok(&mut st, "TX { CREATE (r:rule {title: 'gr19', text: 'y'}) }");
    assert_eq!(
        one(
            &mut st,
            "MATCH (r:rule {title: 'gr19'}) RETURN toString(r.status)"
        ),
        V::text("proposed")
    );
    let named = tx_lq(
        &mut st,
        "TX { CREATE (r:rule {title: 'gr19b', text: 'y', status: 'active'}) }",
        Params::new(),
        &ctx(),
    );
    assert_eq!(code(&named), Some("E404"), "{:?}", named.error);
}

/// [LQ/errors §5.5], §5.7: the message, detail lines and help of E401, E402 and E403 are separate; E401 names each id
/// once; E402 carries the tip.
#[test]
fn refusal_payloads_follow_the_error_tables() {
    let mut st = store();
    let e401 = tx_lq(
        &mut st,
        "TX { MATCH (t:task {id: #51}) EXPECT 2 SET t.priority = 1 }",
        Params::new(),
        &ctx(),
    )
    .error
    .expect("E401");
    assert_eq!(e401.code, "E401");
    assert_eq!(e401.detail, "statement 1 matched 1 bindings, expected 2");
    assert_eq!(e401.help_text(), Some("re-read with moirai q show ids=51"));
    let e403 = tx_lq(
        &mut st,
        "TX { SET #52.priority = 1; ASSERT #52.priority = 0 ELSE 'not P0' }",
        Params::new(),
        &ctx(),
    )
    .error
    .expect("E403");
    assert_eq!(e403.detail, "statement 2: ASSERT is false");
    assert_eq!(e403.lines(), ["\"not P0\"", "nothing was written"]);
    let e402 = tx_lq(
        &mut st,
        "TX IF TARGETS '00000000000000000000000000000000' { MATCH (t:task {id: #51}) EXPECT 1 SET t.priority = 1 }",
        Params::new(),
        &ctx(),
    )
    .error
    .expect("E402");
    let tip = st.dag.live("main").and_then(|r| r.tip).expect("a tip");
    assert_eq!(e402.get("tip"), Some(&Kv::Commit(tip)));
    let moved = tx_lq(
        &mut st,
        "TX IF TIP main~1 { SET #52.priority = 2 }",
        Params::new(),
        &ctx(),
    )
    .error
    .expect("E402");
    let past = st
        .dag
        .rev_commit(
            "main~1",
            &st.rev_ctx(&st.resolve(&ctx(), false).expect("caller")),
        )
        .expect("resolves")
        .expect("a commit");
    let c8 = |s: u64| crate::value::hex(&st.dag.commits[&s].id)[..8].to_string();
    assert_eq!(
        moved.detail,
        format!(
            "main moved: IF TIP {}, the tip is {} (rev {tip})",
            c8(past),
            c8(tip)
        )
    );
}

/// [50 §3.10] item 6: `MOVE … UNDER … FIRST`, `REMOVE x.f`, `SET x.body`, `REOPEN … REASON` and an edge `DELETE`
/// compile to their operations.
#[test]
fn statement_forms_compile_to_their_operations() {
    let mut st = store();
    ok(&mut st, "TX { MOVE #95 UNDER #90 }");
    assert_eq!(
        one(&mut st, "MATCH (t {id: #95}) RETURN t.parent"),
        V::Node(Nid(90))
    );
    ok(&mut st, "TX { SET #93.estimate = 5; SET #93.body = 'b' }");
    ok(&mut st, "TX { REMOVE #93.estimate; REMOVE #93.body }");
    let o = rows(&mut st, "MATCH (t {id: #93}) RETURN t.estimate, t.body");
    assert_eq!(o.rows, vec![vec![V::Absent, V::Absent]]);
    ok(&mut st, "TX { SET #92.status = 'done' }");
    ok(&mut st, "TX { REOPEN #92 REASON 'flaky' }");
    let o = rows(
        &mut st,
        "MATCH (t {id: #92}) RETURN toString(t.status), t.reopen_count",
    );
    assert_eq!(o.rows, vec![vec![V::text("open"), V::Int(1)]]);
    ok(
        &mut st,
        "TX { MATCH (a {id: #14})-[e:BLOCKS]->(b {id: #17}) EXPECT 1 DELETE e }",
    );
    assert_eq!(
        one(
            &mut st,
            "MATCH (a {id: #14})-[:BLOCKS]->(b) RETURN count(*) AS n"
        ),
        V::Int(0)
    );
}
