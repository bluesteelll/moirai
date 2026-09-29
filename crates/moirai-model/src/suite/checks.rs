//! The refusals and records of the write path against their tables: the code-specific keys of [LQ/errors §5.7] and
//! [F19 §10.3], `IF TIP` ([API §9.1]), message normalisation ([F07 §5]), `Init`'s configuration ([API §8.1];
//! [CFG §7.6]), the lease records' `claimed_hlc` and `root_session` ([F11 §6]; [API §10.1]), the order of the role and
//! invariant checks (WR-009 to WR-011) and the context resolution's warnings (CX-7).

use super::*;
use crate::api::{Data, Door, Meta, normalize_message};
use crate::dag::{Ref, RefKind};
use crate::err::Kv;
use crate::lq::ctx::Value as P;
use crate::value::blake3_128;

fn code(r: &Reply) -> Option<&str> {
    r.error.as_ref().map(|e| e.code.as_str())
}

fn keys(r: &Reply) -> Vec<&str> {
    r.error.as_ref().map(|e| e.key_names()).unwrap_or_default()
}

/// #1 `A` with its child #2 `B`, and #3 `C`, in commit 1.
fn base_block(s: &mut S) {
    s.ok(
        tx(vec![
            task("a", "A"),
            child("b", "B", Target::Var("a".into())),
            task("c", "C"),
        ]),
        orch(),
    );
}

#[test]
fn if_tip_refuses_a_moved_tip() {
    let mut s = S::base();
    base_block(&mut s);
    let tip = s.st.dag.live("main").unwrap().tip;
    s.ok(tx(vec![set(3, &[("title", t("C2"))])]), orch());
    let stale = Ctx {
        if_tip: tip,
        ..orch()
    };
    let r = s.refused(tx(vec![set(3, &[("title", t("C3"))])]), stale, "E402");
    assert_eq!(r.exit, 4);
    let e = r.error.unwrap();
    assert_eq!(
        e.key_names(),
        vec!["statement", "tip", "expected_tip", "targets", "written"]
    );
    assert_eq!(e.get("statement"), Some(&Kv::Null));
    assert_eq!(e.get("expected_tip"), Some(&Kv::Commit(tip.unwrap())));
    assert_eq!(e.get("tip"), Some(&Kv::Commit(2)));
    let fresh = Ctx {
        if_tip: Some(2),
        ..orch()
    };
    assert_eq!(
        s.ok(tx(vec![set(3, &[("title", t("C3"))])]), fresh).rev_new,
        Some(3)
    );
}

/// [F07 §5.4]'s examples.
#[test]
fn messages_normalise_by_f07() {
    for (m, want) in [
        (
            "fix lock\r\n\r\nsee #12  \r\n\r\n\r\n",
            Some("fix lock\n\nsee #12"),
        ),
        ("a\rb\t\n", Some("a\nb")),
        ("\n\nsubject", Some("\n\nsubject")),
        (" \t\n", Some("")),
        ("done\n\nMoirai-Ref: main", None),
        (
            "done\n\nnote\nMoirai-Ref: main",
            Some("done\n\nnote\nMoirai-Ref: main"),
        ),
        ("Moirai-Ref: main", None),
        ("a\0b", None),
    ] {
        match (normalize_message(m), want) {
            (Ok(n), Some(w)) => assert_eq!(n, w, "{m:?}"),
            (Err(e), None) => assert_eq!((e.code.as_str(), e.exit), ("bad_value", 2), "{m:?}"),
            (got, want) => panic!("{m:?}: {got:?}, want {want:?}"),
        }
    }
    assert!(normalize_message(&"x".repeat(65_536)).is_err());
    let mut s = S::base();
    let r = s.ok(
        Cmd::Tx {
            stmts: vec![task("a", "A")],
            message: "fix lock\r\n\r\nsee #12  \r\n\r\n".into(),
        },
        orch(),
    );
    assert_eq!(
        s.st.dag.commits[&r.rev_new.unwrap()].message,
        "fix lock\n\nsee #12"
    );
    let seq = s.st.commit_seq;
    s.refused(
        Cmd::Tx {
            stmts: vec![task("b", "B")],
            message: "done\n\nMoirai-Ref: main".into(),
        },
        orch(),
        "bad_value",
    );
    assert_eq!(s.st.commit_seq, seq, "nothing written");
}

#[test]
fn refusal_keys_follow_lq_errors_5_7() {
    let mut s = S::base();
    base_block(&mut s);
    // E404 in the second statement: the statement index is the block's.
    let r = s.refused(
        tx(vec![
            set(3, &[("title", t("x"))]),
            set(1, &[("status", t("done"))]),
        ]),
        orch(),
        "E404",
    );
    assert_eq!(keys(&r), vec!["statement", "written"]);
    let e = r.error.unwrap();
    assert_eq!(e.get("statement"), Some(&Kv::Int(2)));
    assert_eq!(e.get("written"), Some(&Kv::Bool(false)));
    // E401 from a guard: the bound, the count and the current node with the commit that changed it.
    let r = s.refused(
        tx(vec![crate::tx::Stmt::Set {
            target: Target::Id(Nid(2)),
            fields: vec![("title".into(), t("y"))],
            incr: vec![],
            body: None,
            guard: Some(crate::tx::Guard {
                if_status: Some("done".into()),
                ..Default::default()
            }),
        }]),
        orch(),
        "E401",
    );
    assert_eq!(
        keys(&r),
        vec!["statement", "expect", "matched", "current", "written"]
    );
    let e = r.error.unwrap();
    let Some(Kv::List(cur)) = e.get("current") else {
        panic!("current is a list")
    };
    assert_eq!(cur[0].member("id"), Some(&Kv::Node(Nid(2))));
    assert_eq!(
        cur[0].member("changed_by").and_then(|c| c.member("commit")),
        Some(&Kv::Commit(1))
    );
    // E407 with a lease that does not exist.
    let r = s.refused(
        tx(vec![set(3, &[("title", t("z"))])]),
        Ctx {
            lease: Some("L-9".into()),
            ..Default::default()
        },
        "E407",
    );
    assert_eq!(keys(&r), vec!["lease", "holder", "written"]);
    // E406 carries no key of its own besides the statement: no `rule`, no `row`.
    let r = s.refused(tx(vec![task("x", "x")]), Ctx::default(), "E406");
    assert_eq!(keys(&r), vec!["statement", "written"]);
    // A failure inside a data-level create names the LQ statement of its part: `CREATE … UNDER #1` (1), then the
    // `MOVE` of its position (2), whose sibling #3 is not a child of #1 (E404, [API §9.5]).
    let r = s.refused(
        tx(vec![crate::tx::Stmt::Create {
            name: Some("q".into()),
            kind: "task".into(),
            fields: vec![("title".into(), t("q"))],
            body: None,
            under: Some(Target::Id(Nid(1))),
            position: Some(crate::tx::Position::Before(Target::Id(Nid(3)))),
            edges_out: vec![],
            edges_in: vec![],
        }]),
        orch(),
        "E404",
    );
    assert_eq!(r.error.unwrap().get("statement"), Some(&Kv::Int(2)));
    // A deferred validator names the block's last statement.
    let r = s.refused(
        tx(vec![set(3, &[("title", t("w"))]), link(1, "blocks", 2)]),
        orch(),
        "E405",
    );
    assert_eq!(r.error.unwrap().get("statement"), Some(&Kv::Int(2)));
    // usage names its argument.
    let r = s.refused(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(3))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: Some(t("99999999999999999w")),
            start: false,
            run: None,
            session: false,
        },
        orch(),
        "usage",
    );
    assert_eq!(
        r.error.unwrap().get("argument"),
        Some(&Kv::Str("ttl".into()))
    );
}

#[test]
fn a_delete_under_a_lease_names_the_leases() {
    let mut s = S::base();
    base_block(&mut s);
    s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(3))],
            next: false,
            scope: None,
            role: None,
            agent: Some("dev".into()),
            ttl: Some(t("10m")),
            start: false,
            run: None,
            session: false,
        },
        orch(),
    );
    let r = s.refused(
        tx(vec![crate::tx::Stmt::Delete {
            target: Target::Id(Nid(3)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: None,
        }]),
        orch(),
        "E409",
    );
    assert_eq!(keys(&r), vec!["statement", "leases", "written"]);
    let e = r.error.unwrap();
    let Some(Kv::List(ls)) = e.get("leases") else {
        panic!("leases is a list")
    };
    assert_eq!(ls[0].member("id"), Some(&Kv::Str("L-2".into())));
    assert_eq!(ls[0].member("holder"), Some(&Kv::Str("dev".into())));
    assert_eq!(ls[0].member("node"), Some(&Kv::Node(Nid(3))));
    // A root-node refusal of the same block would come first (DP-010 before DP-005); here the lease case is the
    // first precondition that fails, before the references.
}

#[test]
fn init_refuses_what_it_cannot_validate() {
    for (params, want) in [
        (vec!["no.such.key=1"], "config_key"),
        (vec!["lease.ttl-default=1ms"], "config_value"),
        (vec!["store.log-extent-bytes=64KiB"], "config_value"),
        (vec!["default-branch=dev"], "config_value"),
    ] {
        let mut st = crate::api::Store::new();
        let r = st.run(
            &Cmd::Init {
                seed: 1,
                params: params.iter().map(|p| p.to_string()).collect(),
                default_branch: None,
            },
            &Ctx::default(),
        );
        assert_eq!(code(&r), Some(want), "{params:?}");
        assert_eq!(r.exit, 2);
        assert!(st.inited.is_none(), "nothing created");
    }
    let mut st = crate::api::Store::new();
    let r = st.run(
        &Cmd::Init {
            seed: 1,
            params: vec![
                "store.log-extent-bytes=64KiB".into(),
                "store.commit.inline-max-bytes=4KiB".into(),
                "lease.ttl-default=900s".into(),
            ],
            default_branch: Some("lane/dev".into()),
        },
        &Ctx::default(),
    );
    let Data::Init(_, init, config) = r.data else {
        panic!("{:?}", r.error)
    };
    assert_eq!(init["store.log-extent-bytes"], 1 << 16);
    assert_eq!(config["lease.ttl-default"], "15m");
    assert_eq!(config["default-branch"], "lane/dev");
    assert!(!config.contains_key("store.log-extent-bytes"));
}

/// A bulk claim with `start`: the commit takes the first HLC, each lease's `Lease` record the next, in grant order
/// ([F11 §6] field 36); a Codex holder's lease carries its root session ([API §10.1]).
#[test]
fn leases_record_their_own_hlc_and_root_session() {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "A"), task("b", "B")]), orch());
    let r = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(1)), Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: Some("dev".into()),
            ttl: None,
            start: true,
            run: None,
            session: false,
        },
        orch(),
    );
    let commit_hlc = s.st.dag.commits[&r.rev_new.unwrap()].append_hlc;
    let (h2, h3) = (s.st.leases[&2].claimed_hlc, s.st.leases[&3].claimed_hlc);
    assert!(commit_hlc < h2 && h2 < h3, "{commit_hlc} {h2} {h3}");
    let rt = s.st.runtime();
    assert_eq!(
        rt.leases
            .iter()
            .find(|l| l.lease.id == 3)
            .unwrap()
            .lease
            .claimed_hlc,
        h3
    );
    s.ok(tx(vec![task("c", "C")]), orch());
    let codex = Ctx {
        door: Door::Mcp,
        meta: Some(Meta {
            thread_id: Some("T1".into()),
            session_id: Some("S0".into()),
            sandbox_cwd: None,
        }),
        ..Default::default()
    };
    s.ok(
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
        codex,
    );
    let l =
        s.st.leases
            .values()
            .find(|l| l.task == Some(Nid(3)))
            .unwrap();
    assert_eq!(l.holder, "codex:T1");
    assert_eq!(l.root_session, Some(blake3_128(&[b"codex:S0"])));
    assert_eq!(s.st.leases[&2].root_session, None);
}

/// [API §9.1]: a counter assigned is E103; WR-009's role check on the created node precedes WR-011's invariants.
#[test]
fn counters_and_the_order_of_checks() {
    let mut s = S::base();
    s.refused(
        tx(vec![node(
            "a",
            "task",
            &[("title", t("A")), ("reopen_count", P::Int(3))],
        )]),
        orch(),
        "E103",
    );
    // A task without its title (I11, deferred) created by an unleased caller (no role-create row): E406 first.
    s.refused(tx(vec![node("x", "task", &[])]), Ctx::default(), "E406");
    // The same block by the orchestrator fails only I11.
    s.refused(tx(vec![node("x", "task", &[])]), orch(), "E405");
}

/// GD-005 (I13): an `addresses` edge added in the same block counts as added by the block's actor.
#[test]
fn a_review_guard_reads_edges_the_block_adds() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            node(
                "f",
                "finding",
                &[
                    ("title", t("slow")),
                    ("failure_scenario", t("x")),
                    ("f_kind", t("perf")),
                ],
            ),
            task("t", "fix it"),
        ]),
        orch(),
    );
    s.ok(tx(vec![set(1, &[("status", t("confirmed"))])]), orch());
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
            role: Some("code-reviewer".into()),
            agent: Some("rev".into()),
            ttl: Some(t("run")),
            start: false,
            run: Some("r1".into()),
            session: false,
        },
        orch(),
    );
    s.ok(
        tx(vec![node(
            "v",
            "verdict",
            &[
                ("title", t("review")),
                ("role", t("code-reviewer")),
                ("outcome", t("pass")),
            ],
        )]),
        Ctx {
            lease: Some("L-2".into()),
            client: Some("claude".into()),
            ..Default::default()
        },
    );
    s.ok(tx(vec![link(4, "verifies", 1)]), orch());
    let r = s.ok(
        tx(vec![
            link(2, "addresses", 1),
            set(1, &[("status", t("fixed"))]),
        ]),
        orch(),
    );
    let st = s.st.dag.state_at(r.rev_new, &s.st.alloc);
    assert_eq!(st.nodes[&Nid(1)].status, "fixed");
}

/// [LQ/std §7.2] `tx.remember`: a verdict's `DERIVED_FROM` edges go to the findings it is about.
#[test]
fn remember_writes_a_verdicts_derived_from_edges() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            node(
                "f",
                "finding",
                &[("title", t("f")), ("failure_scenario", t("x"))],
            ),
            task("t", "t"),
        ]),
        orch(),
    );
    let r = s.ok(
        Cmd::Mutation {
            name: "tx.remember".into(),
            params: vec![
                ("kind".into(), t("verdict")),
                ("title".into(), t("review")),
                ("text".into(), t("looks fine")),
                (
                    "fields".into(),
                    P::List(vec![t("outcome=pass"), t("role=code-reviewer")]),
                ),
                ("about".into(), P::List(vec![t("#1"), t("#2")])),
            ],
            message: String::new(),
            move_lease: None,
        },
        orch(),
    );
    let st = s.st.dag.state_at(r.rev_new, &s.st.alloc);
    let v = &st.nodes[&Nid(3)];
    let edges: Vec<(&str, u32)> = v.out.keys().map(|k| (k.kind.as_str(), k.dst.0)).collect();
    assert!(edges.contains(&("about", 1)) && edges.contains(&("about", 2)));
    assert!(edges.contains(&("derived_from", 1)));
    assert!(!edges.contains(&("derived_from", 2)), "#2 is a task");
    assert_eq!(
        r.statements.len(),
        5,
        "CREATE, SET body, two ABOUT, one DERIVED_FROM"
    );
}

/// §4.3 row 7: a staging ref takes only `RESOLVE`.
#[test]
fn a_staging_ref_takes_only_resolve() {
    let mut s = S::base();
    base_block(&mut s);
    let tip = s.st.dag.live("main").unwrap().tip;
    let id = s.st.next_ref_id;
    s.st.next_ref_id += 1;
    s.st.dag.refs.insert(
        id,
        Ref {
            id,
            name: "merge/lane.a/main".into(),
            kind: RefKind::Merge,
            tip,
            ref_seq_next: 1,
            fork: tip,
            deleted: false,
            message: None,
            pinned: false,
            moves: Vec::new(),
        },
    );
    s.refused(
        tx(vec![set(3, &[("title", t("x"))])]),
        orch_on("merge/lane.a/main"),
        "E305",
    );
    s.refused(
        tx(vec![crate::tx::Stmt::Resolve {
            key: "#3.title".into(),
            take: crate::tx::Take::Ours,
        }]),
        orch_on("merge/lane.a/main"),
        "not_found",
    );
}

/// CX-7: two harnesses' variables give no session identity and a warning only when the detection step decides the
/// client; family-W results carry the warning.
#[test]
fn two_harnesses_warn_when_detection_decides() {
    let s = S::base();
    let mut two = Ctx::default();
    two.env.insert("CLAUDECODE".into(), "1".into());
    two.env.insert("CLAUDE_CODE_SESSION_ID".into(), "s7".into());
    two.env.insert("CODEX_THREAD_ID".into(), "T2".into());
    let c = s.st.resolve(&two, false).unwrap();
    assert_eq!((c.client, c.session.clone()), ("generic", None));
    assert!(c.warnings.contains(&"two_harnesses".to_string()));
    let named = Ctx {
        client: Some("claude".into()),
        ..two.clone()
    };
    let c = s.st.resolve(&named, false).unwrap();
    assert_eq!(c.client, "claude");
    assert!(c.session.is_some(), "the detection step did not decide");
    assert!(c.warnings.is_empty());
    let mut s = S::base();
    s.ok(tx(vec![task("a", "A")]), orch());
    let r = s.ok(
        Cmd::BranchCreate {
            name: "w".into(),
            from: None,
            kind: None,
        },
        Ctx {
            lease: Some("L-1".into()),
            ..two
        },
    );
    assert!(r.warnings.contains(&"two_harnesses".to_string()));
}

#[test]
fn observations_show_moves_parts_and_views() {
    let mut s = S::base();
    base_block(&mut s);
    s.ok(
        Cmd::BranchCreate {
            name: "a".into(),
            from: None,
            kind: None,
        },
        orch(),
    );
    let r = s.ok(
        Cmd::History {
            ref_: None,
            since_seq: 0,
        },
        Ctx::default(),
    );
    let Data::History(commits, moves) = r.data else {
        panic!()
    };
    assert_eq!(commits.len(), 1);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].0, "lane/a");
    assert_eq!(s.st.runtime().moves.len(), 1);
    let r = s.ok(
        Cmd::State {
            ref_: None,
            at: Some(1),
            parts: Some(vec!["derived".into()]),
        },
        Ctx::default(),
    );
    let Data::State(snap) = r.data else { panic!() };
    assert_eq!((snap.ref_.clone(), snap.commit), (None, Some(1)));
    assert_eq!(snap.parts, [false, false, true]);
    s.refused(
        Cmd::Maintain {
            op: "promote".into(),
            ref_: None,
        },
        orch(),
        "usage",
    );
    s.ok(
        Cmd::Maintain {
            op: "promote".into(),
            ref_: Some("lane/a".into()),
        },
        orch(),
    );
}
