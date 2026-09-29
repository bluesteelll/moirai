//! The change feed ([AR §6.3]; [60 §4.2]): the recorded entries against their definition from the DAG, and the
//! relevance filters of `std.changes` and `std.delta` ([LQ/std §4.8], §4.20) by definition.

use super::*;
use crate::feed::{self, commit_rows, relevant_to};

fn claim_ctx(agent: &str, r: &str) -> Ctx {
    Ctx {
        agent: Some(agent.into()),
        no_dedupe: true,
        ..orch_on(r)
    }
}

/// #1 A, #2 B, #3 C, by the orchestrator.
fn world() -> S {
    let mut s = S::base();
    s.ok(
        tx(vec![task("a", "A"), task("b", "B"), task("c", "C")]),
        orch(),
    );
    s
}

/// The orchestrator claims `#n` for `holder`; the new lease.
fn claim_for(s: &mut S, n: u32, holder: &str) -> String {
    let c = s.run(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(n))],
            next: false,
            scope: None,
            role: None,
            agent: Some(holder.into()),
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        claim_ctx("orch", "main"),
    );
    assert_eq!(c.outcome, Outcome::Ok, "{:?}", c.error);
    c.yields[0].rows[0]
        .iter()
        .find(|(k, _)| k == "lease")
        .unwrap()
        .1
        .clone()
}

/// The key rows the feed recorded for commit `c`: (node, op, aspect, name).
fn key_rows(s: &S, c: u64) -> Vec<(u32, String, String, String)> {
    s.st.feed
        .rows
        .iter()
        .filter(|r| r.commit == Some(c) && !["lease", "marker", "ref"].contains(&r.aspect.as_str()))
        .map(|r| {
            (
                r.node.expect("a key row names its node").0,
                r.op.clone(),
                r.aspect.clone(),
                r.name.clone(),
            )
        })
        .collect()
}

fn row(n: u32, op: &str, aspect: &str, name: &str) -> (u32, String, String, String) {
    (n, op.into(), aspect.into(), name.into())
}

/// The feed's key rows, written out by hand for the doors that decide them ([API §5.7] `change`, [LQ/std §2.12]): a
/// create adds existence (named by the kind) and its non-default fields; an edge is named by its LQ name; a field set
/// to its default leaves the canonical state, so it is a `-`; a changed value is a `~`.
#[test]
fn the_feed_records_each_changed_key() {
    let mut s = world();
    let c1 = s.st.commit_seq;
    let mut want = Vec::new();
    for n in 1..=3 {
        want.push(row(n, "+", "existence", "task"));
        want.push(row(n, "+", "field", "title"));
    }
    let mut got = key_rows(&s, c1);
    got.sort();
    want.sort();
    assert_eq!(got, want, "the creates");
    let c2 = s.ok(tx(vec![link(1, "blocks", 2)]), orch()).commit.unwrap();
    assert_eq!(key_rows(&s, c2), [row(1, "+", "edge", "BLOCKS")]);
    let c3 = s
        .ok(tx(vec![set(3, &[("priority", t("P1"))])]), orch())
        .commit
        .unwrap();
    assert_eq!(key_rows(&s, c3), [row(3, "+", "field", "priority")]);
    let c4 = s
        .ok(tx(vec![set(3, &[("priority", t("P2"))])]), orch())
        .commit
        .unwrap();
    assert_eq!(
        key_rows(&s, c4),
        [row(3, "-", "field", "priority")],
        "P2 is the default"
    );
    let c5 = s
        .ok(tx(vec![set(3, &[("title", t("C2"))])]), orch())
        .commit
        .unwrap();
    assert_eq!(key_rows(&s, c5), [row(3, "~", "field", "title")]);
    // Every row carries its commit's seq, ref, actor and `affected`, and the feed is ordered by seq.
    for r in s.st.feed.rows.iter().filter(|r| r.commit.is_some()) {
        let c = &s.st.dag.commits[&r.commit.unwrap()];
        assert_eq!((r.seq, r.ref_.as_str()), (c.seq, "main"));
        if !["lease", "marker"].contains(&r.aspect.as_str()) {
            assert_eq!((&r.actor, &r.affected), (&c.actor, &c.affected));
        }
    }
    assert!(s.st.feed.rows.windows(2).all(|w| w[0].seq <= w[1].seq));
    // And `commit_rows`, which WP-93b's evaluator reads, gives the same rows in changeset order.
    for c in [c1, c2, c3, c4, c5] {
        let st = s.st.dag.state_at(Some(c), &s.st.alloc);
        let from_fn: Vec<_> = commit_rows(&s.st.dag, &st, c)
            .into_iter()
            .map(|r| (r.node.unwrap().0, r.op, r.aspect, r.name))
            .collect();
        assert_eq!(from_fn, key_rows(&s, c), "s{c}");
    }
}

#[test]
fn relevance_is_claimed_blocked_on_authored_or_cited() {
    let mut s = world();
    claim_for(&mut s, 1, "dev");
    let wl = claim_for(&mut s, 3, "writer");
    // A blocker of dev's task, added after the claim.
    s.ok(tx(vec![link(2, "blocks", 1)]), orch());
    // The writer's note mentions #1 (a `mentions` edge from the `#1` sigil, WE-016).
    s.ok(
        tx(vec![node("n", "note", &[("title", t("see #1"))])]),
        Ctx {
            lease: Some(wl),
            client: Some("claude".into()),
            ..Default::default()
        },
    );
    let tip = s.st.dag.live("main").unwrap().tip;
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    let rel = |n: u32, a: &str| relevant_to(&st, &s.st.leases, Some(Nid(n)), a);
    assert!(rel(1, "dev"), "claimed");
    assert!(rel(2, "dev"), "blocked on: #2 blocks #1");
    assert!(!rel(3, "dev"));
    assert!(rel(3, "writer"), "claimed");
    assert!(rel(4, "writer"), "authored");
    assert!(rel(1, "writer"), "cited: the writer's note mentions #1");
    assert!(!rel(2, "writer"));
    assert!(
        !relevant_to(&st, &s.st.leases, None, "dev"),
        "no node, no relevance"
    );
    // The set `changes` and `delta` build once per evaluation is exactly the nodes the definition finds relevant.
    for a in ["dev", "writer", "orch", "nobody"] {
        let set = feed::relevant_set(&st, &s.st.leases, a);
        for n in (1..=6).map(Nid) {
            assert_eq!(
                set.contains(&n),
                relevant_to(&st, &s.st.leases, Some(n), a),
                "{n} for {a}"
            );
        }
    }
    // std.delta: #2 changes by orch are shown to dev; dev's own change is not.
    let since = s.st.commit_seq;
    s.ok(tx(vec![set(2, &[("title", t("B2"))])]), orch());
    s.ok(tx(vec![set(3, &[("title", t("C2"))])]), orch());
    let tip = s.st.dag.live("main").unwrap().tip;
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    let d = feed::delta(&s.st.feed, since, "dev", &st, &s.st.leases);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].node, Some(Nid(2)));
    let own = feed::delta(&s.st.feed, since, "orch", &st, &s.st.leases);
    assert!(own.is_empty(), "the agent's own changes are not a delta");
    // std.changes: every entry of the view's ref, or of every ref with `all`, restricted by `about` and relevance.
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    s.ok(tx(vec![set(1, &[("title", t("A2"))])]), orch_on("lane/x"));
    let tip = s.st.dag.live("main").unwrap().tip;
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    let on_main = feed::changes(
        &s.st.feed,
        "main",
        since,
        None,
        None,
        false,
        &st,
        &s.st.leases,
    );
    assert!(on_main.iter().all(|c| c.ref_ == "main"));
    let all = feed::changes(
        &s.st.feed,
        "main",
        since,
        None,
        None,
        true,
        &st,
        &s.st.leases,
    );
    assert!(all.iter().any(|c| c.ref_ == "lane/x" && c.aspect == "ref"));
    let about = feed::changes(
        &s.st.feed,
        "main",
        since,
        Some(&[Nid(1)]),
        Some("dev"),
        true,
        &st,
        &s.st.leases,
    );
    assert!(!about.is_empty() && about.iter().all(|c| c.node == Some(Nid(1))));
}

#[test]
fn lease_and_marker_events_are_entries() {
    let mut s = world();
    let c = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(3))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        orch(),
    );
    let lease = c.yields[0].rows[0]
        .iter()
        .find(|(k, _)| k == "lease")
        .unwrap()
        .1
        .clone();
    s.ok(
        Cmd::Complete {
            id: Target::Id(Nid(3)),
            outcome: "done".into(),
            summary: "ok".into(),
            evidence: vec![],
            move_lease: None,
        },
        Ctx {
            lease: Some(lease.clone()),
            ..Default::default()
        },
    );
    let ops: Vec<(&str, &str)> =
        s.st.feed
            .rows
            .iter()
            .filter(|r| r.node == Some(Nid(3)) && (r.aspect == "lease" || r.aspect == "marker"))
            .map(|r| (r.op.as_str(), r.aspect.as_str()))
            .collect();
    assert_eq!(
        ops,
        [("grant", "lease"), ("end", "lease"), ("settled", "marker")]
    );
}

/// A lease event after a commit shares that commit's `seq` but belongs to a later command: `changes(since: s)` shows
/// it, and hides the commit's own group.
#[test]
fn entries_after_a_commit_are_after_it() {
    let mut s = world();
    let seq = s.st.commit_seq;
    let lease = claim_for(&mut s, 1, "dev");
    let rows: Vec<_> = s.st.feed.since(seq, None).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].op.as_str(), rows[0].name.as_str(), rows[0].seq),
        ("grant", lease.as_str(), seq)
    );
    assert!(s.st.feed.since(0, None).count() > rows.len());
}
