//! Streams through the Store API: the base stream of [API §19] examples 02–05 and the refusals of [API §9.1], §10.

use super::*;
use crate::api::{Data, Door, Meta, StampCtx};
use crate::clock::EnvClock;
use crate::lease::Live;
use crate::state::Aspect;
use crate::tx::{Guard, Take};

/// Example 02: a data-level `Tx` creating a parent, two ordered children and a `blocks` edge ([API §9.1], §9.5,
/// §5.8), then an E105 refusal.
fn base_tx(s: &mut S) -> Reply {
    let mut ctx = orch();
    ctx.key = Some("plan-api".into());
    let stmts = vec![
        Stmt::Create {
            name: Some("api".into()),
            kind: "task".into(),
            fields: vec![
                ("title".into(), t("Ship the Store API")),
                ("priority".into(), t("P1")),
            ],
            body: None,
            under: None,
            position: None,
            edges_out: vec![],
            edges_in: vec![],
        },
        child("ch", "Write the chapter", Target::Var("api".into())),
        Stmt::Create {
            name: Some("ex".into()),
            kind: "task".into(),
            fields: vec![("title".into(), t("Write the examples"))],
            body: None,
            under: Some(Target::Var("api".into())),
            position: Some(Position::Last),
            edges_out: vec![],
            edges_in: vec![("blocks".into(), Target::Var("ch".into()))],
        },
    ];
    s.ok(
        Cmd::Tx {
            stmts,
            message: "plan the Store API work".into(),
        },
        ctx,
    )
}

#[test]
fn example_02_creates_and_orders() {
    let mut s = S::base();
    let rt = s.st.runtime();
    let l1 = &rt.leases[0];
    assert_eq!(
        (l1.lease.id, l1.lease.token, l1.lease.holder.as_str()),
        (1, 1, "orch")
    );
    assert_eq!(l1.lease.anchor.name(), "session");
    assert!(l1.lease.session_role);
    let r = base_tx(&mut s);
    assert_eq!((r.rev, r.rev_new, r.commit), (Some(0), Some(1), Some(1)));
    assert_eq!(r.key.as_deref(), Some("plan-api"));
    assert_eq!(
        r.statements.len(),
        6,
        "create, create + move, create + move + edge"
    );
    assert_eq!(r.ready, vec![Nid(2)]);
    assert_eq!(r.other, vec![Nid(1), Nid(3)]);
    let snap = s.st.snapshot("main", Some(1));
    let n = |id: u32| snap.nodes.iter().find(|x| x.id == Nid(id)).unwrap();
    assert_eq!(n(2).node.order.as_deref(), Some("V"));
    assert_eq!(n(3).node.order.as_deref(), Some("k"));
    assert_eq!(n(2).node.parent, Some(Nid(1)));
    assert!(n(1).derived.as_ref().unwrap().container);
    assert!(n(2).derived.as_ref().unwrap().unblocked);
    assert!(n(3).derived.as_ref().unwrap().blocked);
    assert_eq!(n(1).node.creator.role, "orchestrator");
    assert_eq!(n(2).local, (1, 1, 1));
    // An unknown kind is E105, exit 2, and nothing is written.
    let e = s.refused(
        tx(vec![node("x", "tsk", &[("title", t("x"))])]),
        orch(),
        "E105",
    );
    assert_eq!(e.exit, 2);
    assert_eq!(s.st.commit_seq, 1);
}

#[test]
fn example_03_a_guard_conflict_then_success() {
    let mut s = S::base();
    base_tx(&mut s);
    let guarded = |status: &str| Stmt::Set {
        target: Target::Id(Nid(2)),
        fields: vec![("title".into(), t("Write chapter 1"))],
        incr: vec![],
        body: None,
        guard: Some(Guard {
            if_status: Some(status.to_string()),
            ..Default::default()
        }),
    };
    let e = s.refused(tx(vec![guarded("in_progress")]), orch(), "E401");
    assert_eq!(e.exit, 4);
    let r = s.ok(tx(vec![guarded("open")]), orch());
    assert_eq!(r.rev_new, Some(2));
}

#[test]
fn example_04_claim_start_and_complete() {
    let mut s = S::base();
    base_tx(&mut s);
    s.ok(
        Cmd::EnvSlots(EnvSlots {
            alias: vec![("claude:s1".into(), "claude:s1c".into())],
            ..Default::default()
        }),
        Ctx::default(),
    );
    let ctx = Ctx {
        door: Door::Mcp,
        client_info: Some("claude-code".into()),
        stamp: Some(StampCtx {
            session_id: Some("s1".into()),
            agent_id: Some("ag7".into()),
            agent_type: Some("developer".into()),
            cwd: Some("C:/work/moirai".into()),
        }),
        ..Default::default()
    };
    let r = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: None,
            start: true,
            run: None,
            session: false,
        },
        ctx,
    );
    assert_eq!(
        r.rev_new,
        Some(2),
        "claim --start commits open -> in_progress"
    );
    let y = &r.yields[0].rows[0];
    assert!(y.contains(&("lease".into(), "L-2".into())));
    assert!(y.contains(&("holder".into(), "claude:ag7".into())));
    assert!(y.contains(&("anchor".into(), "session".into())));
    assert!(y.contains(&("role".into(), "developer".into())));
    s.ok(
        Cmd::EnvClock(EnvClock {
            advance_ms: Some(600_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    let r = s.ok(
        Cmd::Complete {
            id: Target::Id(Nid(2)),
            outcome: "done".into(),
            summary: "chapter written".into(),
            evidence: vec!["commit:abc1234".into()],
            move_lease: None,
        },
        Ctx {
            lease: Some("L-2".into()),
            ..Default::default()
        },
    );
    assert_eq!(r.rev_new, Some(3));
    assert_eq!(r.ready, vec![Nid(3)], "#3 was blocked by #2 only");
    let c = &s.st.dag.commits[&3];
    assert_eq!(c.message, "chapter written\n\nevidence: commit:abc1234");
    let snap = s.st.snapshot("main", Some(3));
    let two = snap.nodes.iter().find(|x| x.id == Nid(2)).unwrap();
    assert_eq!(
        (two.node.status.as_str(), two.node.resolution.as_str()),
        ("done", "completed")
    );
    assert!(s.st.leases[&2].ended.is_some(), "released into settled");
}

#[test]
fn example_05_idempotency_replays_and_mismatches() {
    let mut s = S::base();
    base_tx(&mut s);
    let r = base_tx(&mut s);
    assert_eq!(r.outcome, Outcome::Replayed);
    assert_eq!(r.commit, Some(1));
    assert_eq!(s.st.commit_seq, 1);
    let mut ctx = orch();
    ctx.key = Some("plan-api".into());
    let e = s.refused(tx(vec![task("z", "another")]), ctx, "E408");
    assert_eq!(e.exit, 9);
    // A refusal records nothing: the same key can be retried after the refusal.
    let mut ctx = orch();
    ctx.key = Some("k2".into());
    s.refused(
        tx(vec![node("x", "tsk", &[("title", t("x"))])]),
        ctx.clone(),
        "E105",
    );
    s.ok(tx(vec![task("y", "y")]), ctx.clone());
    // A default key replays within its window and not after it.
    let dctx = orch();
    let a = s.ok(tx(vec![task("q", "same")]), dctx.clone());
    let b = s.ok(tx(vec![task("q", "same")]), dctx.clone());
    assert_eq!(b.outcome, Outcome::Replayed);
    assert_eq!(a.rev_new, b.commit);
    s.ok(
        Cmd::EnvClock(EnvClock {
            advance_ms: Some(601_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    let c = s.ok(tx(vec![task("q", "same")]), dctx);
    assert_eq!(
        c.outcome,
        Outcome::Ok,
        "the default window of 10 min passed"
    );
}

#[test]
fn the_role_write_policy_refuses_without_a_lease() {
    let mut s = S::base();
    // An unleased caller may remember a note (WC-029) but not create a task; from a Claude Code session, whose model
    // profile writes free-form `TX` ([90 §8.2]).
    s.ok(
        tx(vec![node("n", "note", &[("title", t("a note"))])]),
        Ctx {
            client: Some("claude".into()),
            ..Ctx::default()
        },
    );
    let e = s.refused(tx(vec![task("x", "t")]), Ctx::default(), "E406");
    assert_eq!(e.exit, 6);
    // A node DELETE is the orchestrator's (WX-001, the binder's E406).
    base_tx(&mut s);
    s.refused(
        tx(vec![Stmt::Delete {
            target: Target::Id(Nid(4)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: None,
        }]),
        Ctx::default(),
        "E406",
    );
}

#[test]
fn status_machines_guard_transitions() {
    let mut s = S::base();
    base_tx(&mut s);
    // done -> open only through REOPEN (GR-001).
    s.ok(tx(vec![set(3, &[("status", t("in_progress"))])]), orch());
    s.ok(tx(vec![set(3, &[("done", P::Bool(true))])]), orch());
    s.refused(tx(vec![set(3, &[("status", t("open"))])]), orch(), "E404");
    s.refused(
        tx(vec![set(3, &[("done", P::Bool(false))])]),
        orch(),
        "E404",
    );
    let r = s.ok(
        tx(vec![Stmt::Reopen {
            target: Target::Id(Nid(3)),
            reason: "not done".into(),
        }]),
        orch(),
    );
    let snap = s.st.snapshot("main", r.rev_new);
    let three = snap.nodes.iter().find(|x| x.id == Nid(3)).unwrap();
    assert_eq!(three.node.status, "open");
    assert_eq!(
        three.node.fields.get("reopen_count"),
        Some(&crate::value::Value::Counter(1))
    );
    // A parent with an unfinished child cannot be done (GD-001).
    s.refused(tx(vec![set(1, &[("status", t("done"))])]), orch(), "E404");
    // A verdict gating #3 refuses its completion (GD-002) but not its readiness.
    s.ok(
        tx(vec![
            node(
                "v",
                "verdict",
                &[("title", t("review")), ("outcome", t("fail_fixable"))],
            ),
            Stmt::Link {
                src: Target::Var("v".into()),
                kind: "gates".into(),
                dst: Target::Id(Nid(3)),
                pinned: None,
            },
        ]),
        orch(),
    );
    s.ok(tx(vec![set(3, &[("status", t("in_progress"))])]), orch());
    s.refused(tx(vec![set(3, &[("status", t("done"))])]), orch(), "E404");
}

#[test]
fn invariants_refuse_cycles_and_depth() {
    let mut s = S::base();
    base_tx(&mut s);
    // #3 blocks #2 closes a cycle with #2 blocks #3 (I5′).
    let e = s.refused(tx(vec![link(3, "blocks", 2)]), orch(), "E405");
    assert!(e.error.unwrap().detail.contains("Cycle"));
    // A node never blocks its own descendant.
    s.refused(tx(vec![link(1, "blocks", 2)]), orch(), "E405");
    // A self-edge.
    s.refused(tx(vec![link(2, "relates", 2)]), orch(), "E405");
    // A finding needs failure_scenario (I11).
    s.refused(
        tx(vec![node("f", "finding", &[("title", t("f"))])]),
        orch(),
        "E405",
    );
    // A supersedes edge moves the target to superseded in the same commit (I6).
    s.ok(
        tx(vec![
            node("a", "note", &[("title", t("old"))]),
            node("b", "note", &[("title", t("new"))]),
            Stmt::Link {
                src: Target::Var("b".into()),
                kind: "supersedes".into(),
                dst: Target::Var("a".into()),
                pinned: None,
            },
        ]),
        orch(),
    );
    let snap = s.st.snapshot("main", s.st.dag.live("main").unwrap().tip);
    assert!(
        snap.nodes
            .iter()
            .any(|x| x.node.kind == "note" && x.node.status == "superseded")
    );
}

#[test]
fn leases_expire_renew_and_fence() {
    let mut s = S::base();
    base_tx(&mut s);
    let dev = Ctx {
        agent: Some("dev".into()),
        ..Default::default()
    };
    s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: Some(t("1m")),
            start: false,
            run: None,
            session: false,
        },
        dev.clone(),
    );
    // Another holder cannot claim it (PD-011); the same holder gets the same lease back.
    s.refused(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        Ctx {
            agent: Some("other".into()),
            ..Default::default()
        },
        "E404",
    );
    let again = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: Some(t("1m")),
            start: false,
            run: None,
            session: false,
        },
        Ctx {
            no_dedupe: true,
            ..dev.clone()
        },
    );
    assert!(again.yields[0].rows[0].contains(&("reused".into(), "true".into())));
    assert_eq!(s.st.fence, 2);
    // After the deadline the lease is not live (LL-003) and the task is ready again for others.
    s.ok(
        Cmd::EnvClock(EnvClock {
            advance_ms: Some(61_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    let rt = s.st.runtime();
    assert_eq!(
        rt.leases.iter().find(|l| l.lease.id == 2).unwrap().live,
        Live::Deadline
    );
    // Its holder renews it by a heartbeat (I17′: the same token).
    let hb = s.ok(
        Cmd::Heartbeat {
            lease: "L-2".into(),
        },
        dev.clone(),
    );
    assert!(hb.yields[0].rows[0].contains(&("renewed".into(), "true".into())));
    assert_eq!(s.st.leases[&2].token, 2);
    // Release by another actor is refused; by the holder it ends the lease.
    s.refused(
        Cmd::Release {
            lease: "L-2".into(),
        },
        Ctx {
            agent: Some("x".into()),
            ..Default::default()
        },
        "E407",
    );
    s.ok(
        Cmd::Release {
            lease: "L-2".into(),
        },
        dev.clone(),
    );
    // The same release again replays under its default key; without it, the ended lease is E407.
    assert_eq!(
        s.run(
            Cmd::Release {
                lease: "L-2".into()
            },
            dev.clone()
        )
        .outcome,
        Outcome::Replayed
    );
    s.refused(
        Cmd::Release {
            lease: "L-2".into(),
        },
        Ctx {
            no_dedupe: true,
            ..dev
        },
        "E407",
    );
    // A lost lease presented to a write is E407, exit 5.
    let e = s.refused(
        tx(vec![set(2, &[("title", t("x"))])]),
        Ctx {
            lease: Some("L-2".into()),
            ..Default::default()
        },
        "E407",
    );
    assert_eq!(e.exit, 5);
}

fn claim_one(n: u32, ttl: Option<&str>) -> Cmd {
    Cmd::Claim {
        ids: vec![Target::Id(Nid(n))],
        next: false,
        scope: None,
        role: None,
        agent: None,
        ttl: ttl.map(t),
        start: false,
        run: None,
        session: false,
    }
}

fn slots(hold: &[&str], release: &[&str]) -> Cmd {
    Cmd::EnvSlots(EnvSlots {
        hold: hold.iter().map(|x| x.to_string()).collect(),
        release: release.iter().map(|x| x.to_string()).collect(),
        ..Default::default()
    })
}

/// LE-012 and LE-011: once another holder claims a task whose TTL lease expired, the claim's group ends that lease
/// (reason 4), so its old holder can no longer renew it and the task never has two live leases ([AR §6.2]).
#[test]
fn a_claim_ends_an_expired_lease_so_it_cannot_be_renewed() {
    let mut s = S::base();
    base_tx(&mut s);
    let dev = Ctx {
        agent: Some("dev".into()),
        no_dedupe: true,
        ..Default::default()
    };
    let other = Ctx {
        agent: Some("other".into()),
        no_dedupe: true,
        ..Default::default()
    };
    s.ok(claim_one(2, Some("1m")), dev.clone());
    s.ok(
        Cmd::EnvClock(EnvClock {
            advance_ms: Some(61_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    let c = s.ok(claim_one(2, None), other.clone());
    assert!(c.yields[0].rows[0].contains(&("lease".into(), "L-3".into())));
    assert_eq!(
        s.st.leases[&2].ended,
        Some(crate::lease::EndReason::Dead),
        "LE-012"
    );
    let ends: Vec<_> =
        s.st.feed
            .rows
            .iter()
            .filter(|r| r.aspect == "lease")
            .map(|r| (r.op.as_str(), r.name.as_str()))
            .collect();
    assert!(
        ends.ends_with(&[("end", "L-2"), ("grant", "L-3")]),
        "{ends:?}"
    );
    // The old holder's heartbeat is E407: the lease has ended.
    s.refused(
        Cmd::Heartbeat {
            lease: "L-2".into(),
        },
        dev,
        "E407",
    );
    let rt = s.st.runtime();
    let on1: Vec<(u64, Live)> = rt
        .leases
        .iter()
        .filter(|l| l.lease.task == Some(Nid(2)))
        .map(|l| (l.lease.id, l.live))
        .collect();
    assert_eq!(on1, [(3, Live::Alive)]);
}

/// LE-009 and LE-012 with a session anchor whose slot is released and held again. A read appends nothing (I-F5), so a
/// Dead lease that nothing ended lives again with its slot; once another holder claimed the task, the claim ended it
/// (reason 4) and a re-held slot no longer revives it.
#[test]
fn a_dead_anchor_is_ended_by_the_next_claim_of_its_task() {
    let mut s = S::base();
    base_tx(&mut s);
    s.ok(slots(&["claude:s2"], &[]), Ctx::default());
    let mut dev = Ctx {
        agent: Some("dev".into()),
        no_dedupe: true,
        ..Default::default()
    };
    dev.env.insert("CLAUDECODE".into(), "1".into());
    dev.env.insert("CLAUDE_CODE_SESSION_ID".into(), "s2".into());
    s.ok(claim_one(2, None), dev.clone());
    assert_eq!(
        s.st.leases[&2].anchor,
        crate::lease::AnchorKind::Session,
        "SL-1"
    );
    let live2 = |s: &S| {
        s.st.runtime()
            .leases
            .iter()
            .find(|l| l.lease.id == 2)
            .map(|l| l.live)
    };
    s.ok(slots(&[], &["claude:s2"]), Ctx::default());
    assert_eq!(live2(&s), Some(Live::Dead), "LL-006");
    s.ok(Cmd::Runtime, Ctx::default());
    s.ok(slots(&["claude:s2"], &[]), Ctx::default());
    assert_eq!(
        live2(&s),
        Some(Live::Alive),
        "no write ended it: the slot keeps it again (LL-005)"
    );
    // The slot goes again, another holder claims #1: the claim ends the Dead lease in its group.
    s.ok(slots(&[], &["claude:s2"]), Ctx::default());
    let other = Ctx {
        agent: Some("other".into()),
        no_dedupe: true,
        ..Default::default()
    };
    s.ok(claim_one(2, None), other);
    assert_eq!(s.st.leases[&2].ended, Some(crate::lease::EndReason::Dead));
    s.ok(slots(&["claude:s2"], &[]), Ctx::default());
    assert_eq!(live2(&s), None, "an ended lease is not listed");
    let on1: Vec<u64> =
        s.st.runtime()
            .leases
            .iter()
            .filter(|l| l.lease.task == Some(Nid(2)) && l.live.is_live())
            .map(|l| l.lease.id)
            .collect();
    assert_eq!(on1, [3], "one live lease on #2");
    s.refused(
        Cmd::Heartbeat {
            lease: "L-2".into(),
        },
        dev,
        "E407",
    );
}

/// LE-008: `branch -D` releases every task lease on the branch that has not ended — an expired one included, which
/// could otherwise be renewed on a deleted branch — and records an `end` entry per lease in the feed after the ref's
/// marker entries; role leases stay.
#[test]
fn a_branch_deletion_ends_its_task_leases_in_the_feed() {
    let mut s = S::base();
    base_tx(&mut s);
    s.ok(tx(vec![task("d", "D")]), orch());
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    let on_x = |agent: &str| Ctx {
        agent: Some(agent.into()),
        branch: Some("lane/x".into()),
        no_dedupe: true,
        ..Default::default()
    };
    s.ok(claim_one(2, Some("1m")), on_x("dev"));
    s.ok(claim_one(4, Some("1h")), on_x("dev2"));
    // A run role lease taken on the branch (L-4): it holds no node of the branch and stays.
    s.ok(
        Cmd::RunOpen {
            name: "r1".into(),
            fields: vec![],
        },
        orch_on("lane/x"),
    );
    s.ok(
        Cmd::Claim {
            ids: vec![],
            next: false,
            scope: None,
            role: Some("tester".into()),
            agent: Some("w1".into()),
            ttl: None,
            start: false,
            run: Some("r1".into()),
            session: false,
        },
        orch_on("lane/x"),
    );
    assert_eq!(s.st.leases[&4].branch, "lane/x");
    s.ok(
        Cmd::EnvClock(EnvClock {
            advance_ms: Some(61_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    let d = s.ok(
        Cmd::BranchDelete {
            name: "lane/x".into(),
            force: true,
        },
        orch(),
    );
    assert!(
        matches!(d.data, Data::BranchDelete(_, _, _, ref rel) if rel == &vec![2, 3]),
        "{:?}",
        d.data
    );
    assert!(
        s.st.leases[&1].ended.is_none(),
        "the session role lease stays"
    );
    assert!(
        s.st.leases[&4].ended.is_none(),
        "a role lease on the branch stays"
    );
    let tail: Vec<(String, String, String)> =
        s.st.feed
            .rows
            .iter()
            .rev()
            .take(3)
            .rev()
            .map(|r| (r.op.clone(), r.aspect.clone(), r.name.clone()))
            .collect();
    assert_eq!(
        tail,
        [
            ("delete".into(), "ref".into(), "lane/x".into()),
            ("end".into(), "lease".into(), "L-2".into()),
            ("end".into(), "lease".into(), "L-3".into()),
        ]
    );
    s.refused(
        Cmd::Heartbeat {
            lease: "L-2".into(),
        },
        Ctx {
            agent: Some("dev".into()),
            ..Default::default()
        },
        "E407",
    );
}

#[test]
fn a_reboot_kills_ttl_leases_but_not_run_scoped_ones() {
    let mut s = S::base();
    base_tx(&mut s);
    s.ok(
        Cmd::RunOpen {
            name: "r1".into(),
            fields: vec![],
        },
        orch(),
    );
    s.ok(
        Cmd::Claim {
            ids: vec![],
            next: false,
            scope: None,
            role: Some("developer".into()),
            agent: Some("w1".into()),
            ttl: None,
            start: false,
            run: Some("r1".into()),
            session: false,
        },
        orch(),
    );
    s.ok(
        Cmd::EnvClock(EnvClock {
            reboot: true,
            ..Default::default()
        }),
        Ctx::default(),
    );
    let rt = s.st.runtime();
    let live: Vec<(u64, Live)> = rt.leases.iter().map(|l| (l.lease.id, l.live)).collect();
    assert!(live.contains(&(2, Live::Alive)), "LL-001: {live:?}");
    assert!(
        live.contains(&(1, Live::Dead)),
        "LL-002 (the session role lease's slot is gone): {live:?}"
    );
}

#[test]
fn run_close_releases_the_runs_leases() {
    let mut s = S::base();
    s.ok(
        Cmd::RunOpen {
            name: "r1".into(),
            fields: vec![],
        },
        orch(),
    );
    s.ok(
        Cmd::Claim {
            ids: vec![],
            next: false,
            scope: None,
            role: Some("tester".into()),
            agent: Some("w1".into()),
            ttl: None,
            start: false,
            run: Some("r1".into()),
            session: false,
        },
        orch(),
    );
    let r = s.run(
        Cmd::RunClose {
            name: "r1".into(),
            outcome: "red".into(),
        },
        Ctx::default(),
    );
    assert_eq!(
        r.error.map(|e| e.code),
        Some("E406".into()),
        "run close is the orchestrator's (WV-023)"
    );
    let r = s.ok(
        Cmd::RunClose {
            name: "r1".into(),
            outcome: "red".into(),
        },
        orch(),
    );
    assert!(matches!(r.data, Data::RunClose(_, ref st, ref rel) if st == "red" && rel == &vec![2]));
    assert!(s.st.leases[&2].ended.is_some());
}

#[test]
fn the_caller_context_resolves_actor_session_and_branch() {
    let s = S::base();
    let mut ctx = Ctx {
        meta: Some(Meta {
            thread_id: Some("T1".into()),
            session_id: Some("S0".into()),
            sandbox_cwd: None,
        }),
        ..Default::default()
    };
    ctx.env.insert("MOIRAI_BRANCH".into(), "lane/x".into());
    let c = s.st.resolve(&ctx, false).unwrap();
    assert_eq!((c.actor.as_str(), c.actor_src), ("codex:T1", "meta"));
    assert_eq!(c.session.as_deref(), Some("codex:T1"));
    assert_eq!(c.branch, "lane/x");
    assert_eq!(c.role, "general-purpose");
    let mut two = Ctx::default();
    two.env.insert("CLAUDECODE".into(), "1".into());
    two.env.insert("CODEX_THREAD_ID".into(), "T2".into());
    let c = s.st.resolve(&two, false).unwrap();
    assert_eq!(c.client, "generic");
    assert_eq!(
        c.session, None,
        "CX-7: two harnesses give no session identity"
    );
    assert!(c.warnings.contains(&"two_harnesses".to_string()));
    let e =
        s.st.resolve(
            &Ctx {
                lease: Some("L-9".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap_err();
    assert_eq!((e.code.as_str(), e.exit), ("E407", 5));
}

#[test]
fn branches_follow_the_ref_name_rules() {
    let mut s = S::base();
    base_tx(&mut s);
    let r = s.ok(
        Cmd::BranchCreate {
            name: "a".into(),
            from: None,
            kind: None,
        },
        orch(),
    );
    assert!(matches!(r.data, Data::BranchCreate(ref n, 1, _, Some(1)) if n == "lane/a"));
    // The same command again replays under its default key ([API §7.4], looked up before RN-7); without the key the
    // name is taken.
    let again = s.run(
        Cmd::BranchCreate {
            name: "a".into(),
            from: None,
            kind: None,
        },
        orch(),
    );
    assert_eq!(again.outcome, Outcome::Replayed);
    assert_eq!(again.data, r.data);
    s.refused(
        Cmd::BranchCreate {
            name: "a".into(),
            from: None,
            kind: None,
        },
        Ctx {
            no_dedupe: true,
            ..orch()
        },
        "ref_exists",
    );
    s.refused(
        Cmd::BranchCreate {
            name: "a/b".into(),
            from: None,
            kind: None,
        },
        orch(),
        "ref_prefix",
    );
    s.refused(
        Cmd::BranchCreate {
            name: "merge".into(),
            from: None,
            kind: None,
        },
        orch(),
        "bad_ref_name",
    );
    // A write on the lane does not reach main.
    s.ok(
        tx(vec![set(2, &[("title", t("lane title"))])]),
        orch_on("lane/a"),
    );
    let main = s.st.snapshot("main", s.st.dag.live("main").unwrap().tip);
    assert_eq!(
        main.nodes
            .iter()
            .find(|x| x.id == Nid(2))
            .unwrap()
            .node
            .text("title"),
        Some("Write the chapter")
    );
    // An unmerged lane is not deleted without force.
    s.refused(
        Cmd::BranchDelete {
            name: "lane/a".into(),
            force: false,
        },
        orch(),
        "not_merged",
    );
    let d = s.ok(
        Cmd::BranchDelete {
            name: "lane/a".into(),
            force: true,
        },
        orch(),
    );
    assert!(matches!(
        d.data,
        Data::BranchDelete(_, 1, Some((1, 0, 0)), _)
    ));
}

#[test]
fn schema_items_weaken_and_are_used() {
    let mut s = S::base();
    let item = crate::schema::Item::Field(crate::schema::FieldItem {
        kind: Some("task".into()),
        name: "effort".into(),
        ty: crate::schema::Ty::Int,
        class: "scalar",
        storage: crate::schema::Storage::Field,
        decl: 40,
        optional: true,
        default: None,
        range: None,
        one_line: false,
        ascii: false,
        shape: crate::schema::Shape::Plain,
        index: "none",
        coerce: "none",
        retired: false,
    });
    let r = s.ok(
        Cmd::Schema {
            items: vec![item.clone()],
            message: String::new(),
        },
        orch(),
    );
    assert!(
        matches!(&r.data, Data::Schema(k) if k == &vec!["schema:field:task.effort".to_string()])
    );
    s.refused(
        Cmd::Schema {
            items: vec![item],
            message: String::new(),
        },
        Ctx {
            no_dedupe: true,
            ..orch()
        },
        "E405",
    );
    s.ok(
        tx(vec![node(
            "x",
            "task",
            &[("title", t("x")), ("effort", P::Int(3))],
        )]),
        orch(),
    );
}

#[test]
fn a_crash_in_the_next_command_keeps_both_candidates() {
    let mut s = S::base();
    s.ok(Cmd::EnvCrash { in_next: true }, Ctx::default());
    let r = s.run(tx(vec![task("a", "a")]), orch());
    assert_eq!(r.error.map(|e| e.code), Some("outcome_unknown".into()));
    assert_eq!(s.st.commit_seq, 1, "the applied candidate");
    s.st.adopt_without();
    assert_eq!(s.st.commit_seq, 0, "the candidate without it");
    // A retry with the same key converges.
    let r = s.ok(tx(vec![task("a", "a")]), orch());
    assert_eq!(r.rev_new, Some(1));
}

#[test]
fn resolve_is_a_staging_statement() {
    let mut s = S::base();
    base_tx(&mut s);
    // Plant a conflict value on #2's title as a merge would ([F12 §6.3]): the node's member holds ours.
    let tip = s.st.dag.live("main").unwrap().tip;
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    assert!(st.nodes[&Nid(2)].conflicts.is_empty());
    let r = s.run(
        tx(vec![Stmt::Resolve {
            key: "#2.title".into(),
            take: Take::Theirs,
        }]),
        orch(),
    );
    assert_eq!(
        r.error.map(|e| e.code),
        Some("not_found".into()),
        "#2.title holds no conflict value"
    );
    let _ = Aspect::Status;
}

fn mutation(name: &str, params: &[(&str, P)]) -> Cmd {
    Cmd::Mutation {
        name: name.into(),
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        message: String::new(),
        move_lease: None,
    }
}

#[test]
fn named_mutations_expand_and_run() {
    let mut s = S::base();
    base_tx(&mut s);
    let r = s.ok(
        mutation(
            "tx.add",
            &[
                ("kind", t("task")),
                ("title", t("Review")),
                ("parent", t("#1")),
                ("blocked_by", P::List(vec![t("#3")])),
                ("fields", P::List(vec![t("priority=P0"), t("estimate=3")])),
            ],
        ),
        orch(),
    );
    assert_eq!(r.statements.len(), 2);
    let st = s.st.dag.state_at(r.rev_new, &s.st.alloc);
    let four = &st.nodes[&Nid(4)];
    assert_eq!(four.parent, Some(Nid(1)));
    assert_eq!(
        four.fields.get("estimate"),
        Some(&crate::value::Value::Int(3))
    );
    assert!(
        st.nodes[&Nid(3)]
            .out
            .keys()
            .any(|k| k.kind == "blocks" && k.dst == Nid(4))
    );
    // The same verb again replays under its default key (the payload is H of the expansion).
    let again = s.run(
        mutation(
            "tx.add",
            &[
                ("kind", t("task")),
                ("title", t("Review")),
                ("parent", t("#1")),
                ("blocked_by", P::List(vec![t("#3")])),
                ("fields", P::List(vec![t("priority=P0"), t("estimate=3")])),
            ],
        ),
        orch(),
    );
    assert_eq!(again.outcome, Outcome::Replayed);
    // tx.retract goes through the door `retract` (DR-007): a note to retracted with its reason.
    s.ok(
        tx(vec![node("n", "note", &[("title", t("a claim"))])]),
        orch(),
    );
    let r = s.ok(
        mutation("tx.retract", &[("id", t("#5")), ("reason", t("wrong"))]),
        orch(),
    );
    let st = s.st.dag.state_at(r.rev_new, &s.st.alloc);
    assert_eq!(st.nodes[&Nid(5)].status, "retracted");
    assert_eq!(st.nodes[&Nid(5)].text("reason"), Some("wrong"));
    // A plain set of the same status has no transition (GR-001).
    s.ok(
        tx(vec![node("m", "note", &[("title", t("another"))])]),
        orch(),
    );
    s.refused(
        tx(vec![set(6, &[("status", t("retracted"))])]),
        orch(),
        "E404",
    );
}

#[test]
fn answer_is_the_owners() {
    let mut s = S::base();
    s.ok(
        tx(vec![node("q", "question", &[("title", t("Which codec?"))])]),
        orch(),
    );
    // Presented with the orchestrator's session lease, `--by owner` attests the owner (WT-012, WV-018).
    let r = s.ok(
        mutation(
            "tx.answer",
            &[("q", t("#1")), ("text", t("LZ4.\nBecause it is fast."))],
        ),
        orch(),
    );
    let st = s.st.dag.state_at(r.rev_new, &s.st.alloc);
    assert_eq!(st.nodes[&Nid(1)].status, "answered");
    assert_eq!(st.nodes[&Nid(2)].text("title"), Some("LZ4."));
    assert_eq!(
        st.nodes[&Nid(2)].fields.get("authority"),
        Some(&crate::value::Value::Enum("owner".into()))
    );
    // An unleased caller may not answer.
    s.ok(
        tx(vec![node("q2", "question", &[("title", t("Another?"))])]),
        orch(),
    );
    s.refused(
        mutation("tx.answer", &[("q", t("#3")), ("text", t("x"))]),
        Ctx::default(),
        "E406",
    );
}

#[test]
fn mcp_writes_follow_the_mcp_write_rows() {
    let mut s = S::base();
    base_tx(&mut s);
    let mcp = Ctx {
        door: Door::Mcp,
        lease: Some("L-1".into()),
        ..Default::default()
    };
    // The orchestrator's `mcp_write` is no (WO-001, OP-8): the write tool refuses it.
    s.refused(
        mutation(
            "tx.set",
            &[("id", t("#2")), ("fields", P::List(vec![t("priority=P3")]))],
        ),
        mcp.clone(),
        "E406",
    );
    s.refused(tx(vec![set(2, &[("priority", t("P3"))])]), mcp, "E406");
}
