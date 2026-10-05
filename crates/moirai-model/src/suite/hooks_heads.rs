//! Hooks ([RULES/role-write-policy] `role-hooks`; [API §18]), client heads and bindings in the branch order CX-2
//! ([API §4.2], §11.3, §11.4), and `--move-lease` ([AR §5a.4]; [API §4.3] row 3).

use super::*;
use crate::api::{Data, Door};
use crate::hooks::{self, Write};
use crate::lease::EndReason;

fn claude_sub(agent: &str) -> Ctx {
    Ctx {
        client: Some("claude".into()),
        stamp: Some(crate::api::StampCtx {
            session_id: Some("s1".into()),
            agent_id: Some(agent.into()),
            agent_type: None,
            cwd: None,
        }),
        ..Default::default()
    }
}

/// `SubagentStop` releases the stopping agent's leases through the hook and keeps its last message as a
/// `needs-triage` note naming the task (WH-004, LE-010).
#[test]
fn subagent_stop_releases_and_leaves_a_triage_note() {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "A")]), orch());
    let c = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(1))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        claude_sub("a7"),
    );
    let lease: u64 = crate::tx::parse_lease(&c.yields[0].rows[0][0].1).unwrap();
    assert_eq!(s.st.leases[&lease].holder, "claude:a7");
    let w = hooks::subagent_stop(&s.st, "s1", "a7", "stopped half way");
    assert!(
        matches!(&w[0], Write::Command(c, ctx) if matches!(**c, Cmd::Release { .. }) && ctx.door == Door::Hook)
    );
    let replies = hooks::run(&mut s.st, &w);
    assert!(
        replies.iter().all(|r| r.outcome == Outcome::Ok),
        "{:?}",
        replies.iter().map(|r| &r.error).collect::<Vec<_>>()
    );
    assert_eq!(s.st.leases[&lease].ended, Some(EndReason::Release));
    let tip = s.st.dag.live("main").unwrap().tip;
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    let note = st
        .nodes
        .values()
        .find(|x| x.kind == "note")
        .expect("the triage note");
    assert!(
        note.text("title")
            .unwrap()
            .starts_with("needs-triage: claude:a7")
    );
    assert!(
        note.out
            .keys()
            .any(|e| e.kind == "mentions" && e.dst == Nid(1))
    );
    // Nothing left to release: the hook writes nothing.
    assert!(hooks::subagent_stop(&s.st, "s1", "a7", "again").is_empty());
}

/// `SessionStart` of a main session mints the orchestrator's session lease through the hook door (WH-001); a worker's
/// mints none and renders its role pack.
#[test]
fn session_start_mints_only_in_a_main_session() {
    let mut s = S::base();
    s.ok(
        Cmd::EnvSlots(crate::clock::EnvSlots {
            hold: vec!["claude:s2".into()],
            ..Default::default()
        }),
        Ctx::default(),
    );
    let mut ctx = Ctx {
        agent: Some("orch2".into()),
        ..Default::default()
    };
    ctx.env.insert("CLAUDECODE".into(), "1".into());
    ctx.env.insert("CLAUDE_CODE_SESSION_ID".into(), "s2".into());
    let w = hooks::session_start(&s.st.conf, hooks::Source::Startup, false, true, &ctx);
    assert!(w.contains(&Write::Later("image-export-checkpoint", "M5")));
    assert!(w.contains(&Write::Read("brief")));
    // The orchestrator lease, then the link settle (`LinksSync`, [API §18]), which finds no tree and writes nothing.
    let r = hooks::run(&mut s.st, &w);
    assert_eq!(r.len(), 2);
    for x in &r {
        assert_eq!(x.outcome, Outcome::Ok, "{:?}", x.error);
    }
    assert_eq!(r[1].commit, s.st.dag.live("main").and_then(|m| m.tip));
    let worker = hooks::session_start(&s.st.conf, hooks::Source::Startup, true, false, &ctx);
    assert!(
        !worker
            .iter()
            .any(|x| matches!(x, Write::Command(c, _) if matches!(**c, Cmd::Claim { .. })))
    );
    assert!(worker.contains(&Write::Read("role-pack")));
}

/// A client head and a binding take part in CX-2 in the order of record; a detached head is read-only (§4.3 row 6).
#[test]
fn heads_and_bindings_resolve_the_branch() {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "A")]), orch());
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    // A binding of a directory: a command run below it resolves to its ref, with the warning `not_a_tree`.
    let b = s.ok(
        Cmd::WorktreeBind {
            dir: "/w/x".into(),
            ref_: "lane/x".into(),
            replace: false,
        },
        orch(),
    );
    assert!(b.warnings.contains(&"not_a_tree".to_string()));
    let below = Ctx {
        cwd: Some("/w/x/src".into()),
        ..Default::default()
    };
    assert_eq!(s.st.resolve(&below, false).unwrap().branch, "lane/x");
    // A client head outranks the cwd binding.
    let ci = Ctx {
        client: Some("ci".into()),
        cwd: Some("/w/x/src".into()),
        ..orch()
    };
    let co = s.ok(
        Cmd::Checkout {
            target: "main".into(),
            branch_new: None,
        },
        Ctx {
            branch: None,
            ..ci.clone()
        },
    );
    match &co.data {
        Data::Checkout(d) => assert_eq!((d.key.as_str(), d.ref_.as_deref()), ("ci", Some("main"))),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        s.st.resolve(
            &Ctx {
                lease: None,
                ..ci.clone()
            },
            false
        )
        .unwrap()
        .branch,
        "main"
    );
    // A detached checkout: reads resolve to the commit, writes are E305.
    let tip = s.st.dag.live("main").unwrap().tip.unwrap();
    s.ok(
        Cmd::Checkout {
            target: format!("s{tip}"),
            branch_new: None,
        },
        ci.clone(),
    );
    let c = s.st.resolve(&ci, false).unwrap();
    assert_eq!((c.branch.as_str(), c.detached), ("", Some(tip)));
    let lease_free = Ctx {
        lease: None,
        ..ci.clone()
    };
    s.refused(tx(vec![task("b", "B")]), lease_free, "E305");
    // Unbinding restores the default branch below the directory.
    s.ok(Cmd::WorktreeUnbind { dir: "/w/x".into() }, orch());
    assert_eq!(s.st.resolve(&below, false).unwrap().branch, "main");
    // The runtime snapshot lists the heads.
    match &s.run(Cmd::Runtime, Ctx::default()).data {
        Data::Runtime(r) => assert_eq!(r.heads.len(), 1),
        other => panic!("{other:?}"),
    }
}

/// `--move-lease`: a task lease fixes the branch (§4.3 row 3) unless the command moves it there, with the warning
/// `lease_moved` and a `Lease` record of event 3 (the lease's branch changes).
#[test]
fn move_lease_moves_the_branch_of_a_task_lease() {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "A")]), orch());
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    let c = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(1))],
            next: false,
            scope: None,
            role: None,
            agent: Some("w1".into()),
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        orch(),
    );
    let l = c.yields[0].rows[0][0].1.clone();
    let id = crate::tx::parse_lease(&l).unwrap();
    let on_x = Ctx {
        lease: Some(l.clone()),
        branch: Some("lane/x".into()),
        client: Some("claude".into()),
        ..Default::default()
    };
    let complete = |move_lease: Option<&str>| Cmd::Complete {
        id: Target::Id(Nid(1)),
        outcome: "done".into(),
        summary: "ok".into(),
        evidence: vec![],
        move_lease: move_lease.map(str::to_string),
    };
    s.refused(complete(None), on_x.clone(), "E407");
    let r = s.ok(complete(Some("lane/x")), on_x);
    assert!(r.warnings.contains(&"lease_moved".to_string()));
    assert_eq!(r.branch.as_deref(), Some("lane/x"));
    assert_eq!(s.st.leases[&id].branch, "lane/x");
    assert_eq!(s.st.leases[&id].ended, Some(EndReason::Complete));
    // Only tx.set and tx.complete move a lease.
    s.refused(
        Cmd::Mutation {
            name: "tx.add".into(),
            params: vec![],
            message: String::new(),
            move_lease: Some("lane/x".into()),
        },
        orch(),
        "usage",
    );
}

/// An interrupted bulk-class command keeps a third candidate, its reservation without its commit ([API §6.7]); another
/// command keeps two.
#[test]
fn a_crashed_bulk_command_keeps_its_reservation_candidate() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "A"),
            child("b", "B", Target::Var("a".into())),
        ]),
        orch(),
    );
    s.ok(Cmd::EnvCrash { in_next: true }, Ctx::default());
    let rm = Cmd::Mutation {
        name: "tx.rm".into(),
        params: vec![
            ("id".into(), crate::lq::ctx::Value::Text("#1".into())),
            (
                "policy".into(),
                crate::lq::ctx::Value::Text("cascade".into()),
            ),
        ],
        message: String::new(),
        move_lease: None,
    };
    let r = s.run(rm, orch());
    assert_eq!(
        r.error.as_ref().map(|e| e.code.as_str()),
        Some("outcome_unknown")
    );
    let (without, reserved) = (
        s.st.without.clone().unwrap(),
        s.st.reserved.clone().unwrap(),
    );
    assert_eq!(reserved.next_id, s.st.next_id);
    assert_eq!(reserved.commit_seq, without.commit_seq);
    s.st.adopt_reserved();
    assert_eq!(s.st.commit_seq, without.commit_seq);
    s.ok(Cmd::EnvCrash { in_next: true }, Ctx::default());
    s.run(tx(vec![task("c", "C")]), orch());
    assert!(s.st.reserved.is_none(), "not a bulk-class command");
}
