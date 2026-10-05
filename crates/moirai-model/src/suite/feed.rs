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

/// The orchestrator claims `#n` for `holder` on branch `r`; the new lease.
fn claim_for(s: &mut S, n: u32, holder: &str, r: &str) -> String {
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
        claim_ctx("orch", r),
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
    claim_for(&mut s, 1, "dev", "main");
    let wl = claim_for(&mut s, 3, "writer", "main");
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

/// The entries that are not key changes, each against its row of [LQ/std §2.15]'s table (spec sync 2b, S2B-F-56;
/// the WP-90b review's R24): (`op`, `aspect`, `name`, `node`, `ref`, `commit`, `actor`, `seq`) for a lease granted in a
/// lease-only group and in a commit group, a lease ended by `complete` and by `release`, a lease ended by a branch
/// deletion, `settled` and `deleted` marker entries, and ref moves (a create's commit is the new tip, a deletion has
/// none), each entry that no commit carries at the seq of the newest commit before it.
#[test]
fn non_key_entries_follow_the_table() {
    let mut s = world();
    let c1 = s.st.commit_seq;
    // A lease-only group: the grant has no commit, at the seq of the newest commit.
    let from = s.st.feed.rows.len();
    let l2 = claim_for(&mut s, 1, "dev", "main");
    assert_eq!(
        entries(&s, from),
        vec![entry(
            "grant",
            "lease",
            &l2,
            Some(1),
            "main",
            None,
            "dev",
            c1
        )]
    );
    // `release` ends it (reason 1) in a lease-only group.
    let from = s.st.feed.rows.len();
    s.ok(
        Cmd::Release { lease: l2.clone() },
        Ctx {
            lease: Some(l2.clone()),
            no_dedupe: true,
            ..Default::default()
        },
    );
    assert_eq!(
        entries(&s, from),
        vec![entry("end", "lease", &l2, Some(1), "main", None, "dev", c1)]
    );
    // A claim with `start` writes a commit: the grant carries the group's commit; `complete` ends the lease and settles
    // the task in its commit's group, the marker's actor the completion's holder (MF-009).
    let from = s.st.feed.rows.len();
    let c = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: Some("dev".into()),
            ttl: None,
            start: true,
            run: None,
            session: false,
        },
        claim_ctx("orch", "main"),
    );
    let c2 = c.commit.unwrap();
    let l3 = c.yields[0].rows[0]
        .iter()
        .find(|(k, _)| k == "lease")
        .unwrap()
        .1
        .clone();
    assert_eq!(
        entries(&s, from),
        vec![entry(
            "grant",
            "lease",
            &l3,
            Some(2),
            "main",
            Some(c2),
            "dev",
            c2
        )]
    );
    let from = s.st.feed.rows.len();
    let c3 = s
        .ok(
            Cmd::Complete {
                id: Target::Id(Nid(2)),
                outcome: "done".into(),
                summary: "ok".into(),
                evidence: vec![],
                move_lease: None,
            },
            Ctx {
                lease: Some(l3.clone()),
                no_dedupe: true,
                ..Default::default()
            },
        )
        .commit
        .unwrap();
    assert_eq!(
        entries(&s, from),
        vec![
            entry("end", "lease", &l3, Some(2), "main", Some(c3), "dev", c3),
            entry(
                "settled",
                "marker",
                &format!("s{c3}"),
                Some(2),
                "main",
                Some(c3),
                "dev",
                c3
            ),
        ],
        "the Lease record, then the Marker record ([F05 §4.7])"
    );
    // A `deleted` marker's actor is its origin commit's.
    let from = s.st.feed.rows.len();
    let c4 = s
        .ok(
            tx(vec![Stmt::Delete {
                target: Target::Id(Nid(3)),
                policy: None,
                replaced_by: None,
                release: false,
                reason: Some("gone".into()),
            }]),
            orch(),
        )
        .commit
        .unwrap();
    let actor = s.st.dag.commits[&c4].actor.clone();
    assert_eq!(
        entries(&s, from),
        vec![entry(
            "deleted",
            "marker",
            &format!("s{c4}"),
            Some(3),
            "main",
            Some(c4),
            &actor,
            c4
        )]
    );
    // Ref moves: a create names its new tip; a lease on the branch ends at its deletion, after the ref move, which names
    // no commit.
    let from = s.st.feed.rows.len();
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: None,
            kind: None,
        },
        orch(),
    );
    let lx = claim_for(&mut s, 1, "dev", "lane/x");
    s.ok(
        Cmd::BranchDelete {
            name: "lane/x".into(),
            force: true,
        },
        orch(),
    );
    let rows = entries(&s, from);
    assert_eq!(
        rows[0],
        entry(
            "create",
            "ref",
            "lane/x",
            None,
            "lane/x",
            Some(c4),
            &actor,
            c4
        )
    );
    assert_eq!(
        rows[1],
        entry("grant", "lease", &lx, Some(1), "lane/x", None, "dev", c4)
    );
    assert_eq!(
        (rows[2].op.as_str(), rows[2].ref_.as_str(), rows[2].commit),
        ("delete", "lane/x", None)
    );
    assert_eq!(
        rows.last().cloned(),
        Some(entry(
            "end",
            "lease",
            &lx,
            Some(1),
            "lane/x",
            None,
            "dev",
            c4
        )),
        "the branch deletion ends the lease after its ref move"
    );
}

/// One entry's columns of [LQ/std §2.15].
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    op: String,
    aspect: String,
    name: String,
    node: Option<u32>,
    ref_: String,
    commit: Option<u64>,
    actor: String,
    seq: u64,
}

#[allow(clippy::too_many_arguments)]
fn entry(
    op: &str,
    aspect: &str,
    name: &str,
    node: Option<u32>,
    r: &str,
    commit: Option<u64>,
    actor: &str,
    seq: u64,
) -> Entry {
    Entry {
        op: op.into(),
        aspect: aspect.into(),
        name: name.into(),
        node,
        ref_: r.into(),
        commit,
        actor: actor.into(),
        seq,
    }
}

/// The feed's non-key entries from row `from` on.
fn entries(s: &S, from: usize) -> Vec<Entry> {
    s.st.feed.rows[from..]
        .iter()
        .filter(|r| ["lease", "marker", "ref"].contains(&r.aspect.as_str()))
        .map(|r| Entry {
            op: r.op.clone(),
            aspect: r.aspect.clone(),
            name: r.name.clone(),
            node: r.node.map(|n| n.0),
            ref_: r.ref_.clone(),
            commit: r.commit,
            actor: r.actor.clone(),
            seq: r.seq,
        })
        .collect()
}

/// [LQ/std §2.15] "The `since(s)` cut": a lease event after a commit shares that commit's `seq` but no commit carries
/// it, so `changes(since: s)` shows it, and hides the commit's own group.
#[test]
fn entries_after_a_commit_are_after_it() {
    let mut s = world();
    let seq = s.st.commit_seq;
    let lease = claim_for(&mut s, 1, "dev", "main");
    let rows: Vec<_> = s.st.feed.since(seq, None).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].op.as_str(), rows[0].name.as_str(), rows[0].seq),
        ("grant", lease.as_str(), seq)
    );
    assert!(s.st.feed.since(0, None).count() > rows.len());
}
