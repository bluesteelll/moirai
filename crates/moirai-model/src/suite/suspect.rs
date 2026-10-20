//! `suspect` after the merge-family histories, the target-state clause by citing kind, and a commit's `affected`
//! ([F13 §6.2], §6.3; [F17 §8.2]). `suspect` reads only the state of a citing edge's target: a `derived_from`, `cites`,
//! `implements` or `depends_on` target retracted, superseded or deleted. `pinned_commit` is data and no predicate of
//! `P_F15` reads it, so a pinned citer whose target stays live and active is never suspect, whatever its pin names.

use super::*;
use crate::api::Data;
use crate::history::MergeData;
use crate::state::EdgeKey;

/// The orchestrator on a branch, without the default idempotency key: each write is a new command.
fn fresh(b: &str) -> Ctx {
    Ctx {
        no_dedupe: true,
        ..orch_on(b)
    }
}

fn tip(s: &S, r: &str) -> Option<u64> {
    s.st.dag.live(r).and_then(|x| x.tip)
}

/// The id of a commit, from its seq.
fn id(s: &S, seq: u64) -> [u8; 32] {
    s.st.dag.commits[&seq].id
}

/// The kind of a ref's tip commit.
fn tip_kind(s: &S, r: &str) -> &'static str {
    s.st.dag.commits[&tip(s, r).expect("a tip")].kind
}

/// `suspect` of a live node on a ref, read from the view's `State` snapshot ([API §15.5]).
fn suspect(s: &S, r: &str, n: Nid) -> bool {
    s.st.snapshot(r, tip(s, r))
        .nodes
        .iter()
        .find(|x| x.id == n)
        .and_then(|x| x.derived.as_ref())
        .unwrap_or_else(|| panic!("{n} is not live on {r}"))
        .suspect
}

/// The edge `a -kind-> b` on a ref, if the view holds it, with its pin.
fn edge(s: &S, r: &str, a: Nid, kind: &str, b: Nid) -> Option<Option<[u8; 32]>> {
    let st = s.st.dag.state_at(tip(s, r), &s.st.alloc);
    st.nodes.get(&a).and_then(|x| {
        x.out
            .get(&EdgeKey {
                kind: kind.into(),
                dst: b,
                disc: None,
            })
            .map(|p| p.pinned)
    })
}

/// Whether a node is live on a ref.
fn live(s: &S, r: &str, n: Nid) -> bool {
    s.st.dag.state_at(tip(s, r), &s.st.alloc).live(n).is_some()
}

/// The status of a node on a ref.
fn status(s: &S, r: &str, n: Nid) -> String {
    s.st.dag.state_at(tip(s, r), &s.st.alloc).nodes[&n]
        .status
        .clone()
}

/// The node with a title on `main`.
fn titled(s: &S, title: &str) -> Nid {
    let st = s.st.dag.state_at(tip(s, "main"), &s.st.alloc);
    st.nodes
        .iter()
        .find(|(_, x)| x.text("title") == Some(title))
        .map(|(n, _)| *n)
        .unwrap_or_else(|| panic!("no node titled {title}"))
}

/// A `link`, pinned when `pin` names a commit.
fn pin_link(a: Nid, kind: &str, b: Nid, pin: Option<&str>) -> Stmt {
    Stmt::Link {
        src: Target::Id(a),
        kind: kind.into(),
        dst: Target::Id(b),
        pinned: pin.map(str::to_string),
    }
}

/// A true edit of a node: a new body, which mentions no node.
fn edit(n: Nid, text: &str) -> Stmt {
    Stmt::Set {
        target: Target::Id(n),
        fields: vec![],
        incr: vec![],
        body: Some(Some(text.into())),
        guard: None,
    }
}

fn delete(n: Nid) -> Stmt {
    Stmt::Delete {
        target: Target::Id(n),
        policy: None,
        replaced_by: None,
        release: false,
        reason: Some("gone".into()),
    }
}

/// A `supersedes` edge from a new node of `kind` to `old` ([AR §3.4] I6: the target is superseded in the same commit).
fn supersede(kind: &str, title: &str, old: Nid) -> Vec<Stmt> {
    vec![
        node("new", kind, &[("title", t(title))]),
        Stmt::Link {
            src: Target::Var("new".into()),
            kind: "supersedes".into(),
            dst: Target::Id(old),
            pinned: None,
        },
    ]
}

fn merge_data(r: &Reply) -> &MergeData {
    match &r.data {
        Data::Merge(d) => d,
        other => panic!("not a merge result: {other:?}"),
    }
}

/// A store with the notes `n` (the citer) and `m` (its target) on `main`, and `lane/x` forked from it.
fn citer_and_target() -> (S, Nid, Nid) {
    let mut s = S::base();
    s.ok(
        tx(vec![
            node("n", "note", &[("title", t("the citer"))]),
            node("m", "note", &[("title", t("the target"))]),
        ]),
        fresh("main"),
    );
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        fresh("main"),
    );
    let (n, m) = (titled(&s, "the citer"), titled(&s, "the target"));
    (s, n, m)
}

/// A pin taken on a lane at the target's own edit, merged into `main`: the citer is not suspect on `main`.
#[test]
fn a_pin_taken_on_a_lane_is_not_suspect_after_the_merge_into_main() {
    let (mut s, n, m) = citer_and_target();
    let l1 = s
        .ok(tx(vec![edit(m, "edited on the lane")]), fresh("lane/x"))
        .commit
        .expect("the edit commits");
    s.ok(
        tx(vec![pin_link(n, "cites", m, Some(&format!("s{l1}")))]),
        fresh("lane/x"),
    );
    let r = s.ok(
        Cmd::Merge {
            src: "lane/x".into(),
            into: Some("main".into()),
            policy: None,
            strict: None,
            base: None,
            message: String::new(),
        },
        fresh("main"),
    );
    assert_eq!(merge_data(&r).outcome, "landed");
    assert_eq!(tip_kind(&s, "main"), "merge");
    assert_eq!(edge(&s, "main", n, "cites", m), Some(Some(id(&s, l1))));
    assert_eq!(status(&s, "main", m), "active");
    assert!(
        !suspect(&s, "main", n),
        "the target is live and active ([F13 §6.2])"
    );
}

/// A pin taken on `main` at the target's own edit, synced into a lane: the citer is not suspect on the lane.
#[test]
fn a_pin_taken_on_main_is_not_suspect_on_a_lane_after_its_sync() {
    let (mut s, n, m) = citer_and_target();
    let m1 = s
        .ok(tx(vec![edit(m, "edited on main")]), fresh("main"))
        .commit
        .expect("the edit commits");
    s.ok(
        tx(vec![pin_link(n, "cites", m, Some(&format!("s{m1}")))]),
        fresh("main"),
    );
    let r = s.ok(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        fresh("lane/x"),
    );
    assert_eq!(merge_data(&r).outcome, "landed");
    assert_eq!(tip_kind(&s, "lane/x"), "sync");
    assert_eq!(edge(&s, "lane/x", n, "cites", m), Some(Some(id(&s, m1))));
    assert_eq!(status(&s, "lane/x", m), "active");
    assert!(
        !suspect(&s, "lane/x", n),
        "the target is live and active ([F13 §6.2])"
    );
}

/// NX-080's form: the target of a pinned `derived_from` is deleted, which makes its citer suspect (EG-035), and a
/// revert of the delete restores it (UD-001); the citer is then not suspect.
#[test]
fn a_revert_of_the_targets_delete_leaves_the_pinned_citer_not_suspect() {
    let (mut s, n, m) = citer_and_target();
    let r = s.ok(
        tx(vec![pin_link(n, "derived_from", m, Some("main"))]),
        fresh("main"),
    );
    let pin = Some(id(&s, r.rev.expect("main's tip before the link")));
    let d = s
        .ok(tx(vec![delete(m)]), fresh("main"))
        .commit
        .expect("the delete commits");
    assert!(!live(&s, "main", m), "the target is a tombstone");
    assert_eq!(
        edge(&s, "main", n, "derived_from", m),
        Some(pin),
        "the edge is kept to the tombstone (EG-035)"
    );
    assert!(
        suspect(&s, "main", n),
        "a deleted target ([F13 §6.2]; EG-035)"
    );
    s.ok(
        Cmd::Revert {
            commit: format!("s{d}"),
            onto: Some("main".into()),
            mainline: None,
            message: String::new(),
        },
        fresh("main"),
    );
    assert_eq!(tip_kind(&s, "main"), "revert");
    assert!(
        live(&s, "main", m),
        "the revert restores the target (UD-001)"
    );
    assert_eq!(status(&s, "main", m), "active");
    assert_eq!(edge(&s, "main", n, "derived_from", m), Some(pin));
    assert!(
        !suspect(&s, "main", n),
        "the target is live and active again ([F13 §6.2]; NX-080)"
    );
}

/// A pin taken on a lane at the target's own edit, both commits cherry-picked onto `main`: the citer is not suspect.
#[test]
fn a_pin_cherry_picked_with_its_targets_edit_is_not_suspect() {
    let (mut s, n, m) = citer_and_target();
    let l1 = s
        .ok(tx(vec![edit(m, "edited on the lane")]), fresh("lane/x"))
        .commit
        .expect("the edit commits");
    let l2 = s
        .ok(
            tx(vec![pin_link(n, "cites", m, Some(&format!("s{l1}")))]),
            fresh("lane/x"),
        )
        .commit
        .expect("the link commits");
    for c in [l1, l2] {
        s.ok(
            Cmd::CherryPick {
                commit: format!("s{c}"),
                onto: Some("main".into()),
                message: String::new(),
            },
            fresh("main"),
        );
        assert_eq!(tip_kind(&s, "main"), "cherry-pick");
    }
    assert_eq!(edge(&s, "main", n, "cites", m), Some(Some(id(&s, l1))));
    assert_eq!(status(&s, "main", m), "active");
    assert!(
        !suspect(&s, "main", n),
        "the target is live and active ([F13 §6.2])"
    );
}

/// A pin by revision name taken after an unrelated commit names that commit, not the target's: the citer is not
/// suspect.
#[test]
fn a_pin_by_revision_name_after_an_unrelated_commit_is_not_suspect() {
    let (mut s, n, m) = citer_and_target();
    let u = s
        .ok(tx(vec![task("u", "an unrelated task")]), fresh("main"))
        .commit
        .expect("the unrelated commit");
    let r = s.ok(
        tx(vec![pin_link(n, "cites", m, Some("main"))]),
        fresh("main"),
    );
    assert_eq!(r.rev, Some(u), "`main` names the unrelated commit");
    assert_eq!(edge(&s, "main", n, "cites", m), Some(Some(id(&s, u))));
    assert!(
        !suspect(&s, "main", n),
        "the target is live and active ([F13 §6.2])"
    );
}

/// A true edit of the target after the pin leaves the citer not suspect: no predicate reads the pin ([F13 §6.2]).
#[test]
fn an_edit_of_the_target_after_the_pin_leaves_the_citer_not_suspect() {
    let (mut s, n, m) = citer_and_target();
    let r = s.ok(
        tx(vec![pin_link(n, "cites", m, Some("main"))]),
        fresh("main"),
    );
    let pin = Some(id(&s, r.rev.expect("main's tip before the link")));
    s.ok(tx(vec![edit(m, "a new text")]), fresh("main"));
    assert_eq!(edge(&s, "main", n, "cites", m), Some(pin));
    assert_eq!(status(&s, "main", m), "active");
    assert!(
        !suspect(&s, "main", n),
        "`pinned_commit` is data; no predicate of `P_F15` reads it ([F13 §6.2])"
    );
}

/// A pin written in the TX that edits its target names the tip before the TX: the citer is not suspect.
#[test]
fn a_pin_written_in_the_tx_that_edits_its_target_is_not_suspect() {
    let (mut s, n, m) = citer_and_target();
    let r = s.ok(
        tx(vec![
            edit(m, "edited with the pin"),
            pin_link(n, "cites", m, Some("main")),
        ]),
        fresh("main"),
    );
    let pin = Some(id(&s, r.rev.expect("main's tip before the TX")));
    assert_eq!(edge(&s, "main", n, "cites", m), Some(pin));
    assert!(
        !suspect(&s, "main", n),
        "the target is live and active ([F13 §6.2])"
    );
}

/// A commit that adds a pinned edge to a live, active target changes no `P_F15` value, so its `affected` is empty and
/// complete ([F13 §6.3] `D(c)`; [F17 §8.2]).
#[test]
fn a_commit_that_adds_a_pinned_edge_leaves_its_source_out_of_affected() {
    let (mut s, n, m) = citer_and_target();
    let c = s
        .ok(
            tx(vec![pin_link(n, "cites", m, Some("main"))]),
            fresh("main"),
        )
        .commit
        .expect("the link commits");
    assert!(edge(&s, "main", n, "cites", m).is_some_and(|p| p.is_some()));
    let x = &s.st.dag.commits[&c];
    assert_eq!(
        (x.affected.clone(), x.affected_complete),
        (vec![], true),
        "no P_F15 value of the source or the target changes"
    );
}

/// [F13 §6.2] `suspect`'s target-state clause for each citing kind, pinned (at `main`, as NX-017's #17) and unpinned:
/// a retracted, superseded or deleted target of `derived_from` and `cites`, a superseded or deleted target of
/// `implements`, and a superseded target of `depends_on` make the citer suspect; a deleted target of `depends_on` drops
/// the edge, so the citer is not suspect, and is in that delete's `affected` (EG-031, `src-affected`).
#[test]
fn the_target_state_clause_holds_for_each_citing_kind() {
    let mut s = S::base();
    // The targets: notes to retract, supersede and delete; docs to supersede and delete.
    let targets = [
        ("tr", "note"),
        ("ts", "note"),
        ("td", "note"),
        ("ds", "doc"),
        ("dd", "doc"),
    ];
    // The citers: (kind of the citer, edge kind, target, pinned).
    let mut citers = Vec::new();
    for k in ["derived_from", "cites"] {
        for tg in ["tr", "ts", "td"] {
            for p in [true, false] {
                citers.push(("note", k, tg, p));
            }
        }
    }
    for tg in ["ds", "dd"] {
        for p in [true, false] {
            citers.push(("task", "implements", tg, p));
        }
        citers.push(("doc", "depends_on", tg, false));
    }
    let name = |c: &(&str, &str, &str, bool)| format!("{} {} {} {}", c.0, c.1, c.2, c.3);
    let mut stmts: Vec<Stmt> = targets
        .iter()
        .map(|(v, k)| node(v, k, &[("title", t(v))]))
        .collect();
    stmts.extend(
        citers
            .iter()
            .enumerate()
            .map(|(i, c)| node(&format!("c{i}"), c.0, &[("title", t(&name(c)))])),
    );
    s.ok(tx(stmts), fresh("main"));
    let id: std::collections::BTreeMap<String, Nid> = targets
        .iter()
        .map(|(v, _)| v.to_string())
        .chain(citers.iter().map(name))
        .map(|x| (x.clone(), titled(&s, &x)))
        .collect();
    let ds = id["ds"];
    s.ok(
        tx(vec![set(ds.0, &[("status", t("current"))])]),
        fresh("main"),
    );
    let links = citers
        .iter()
        .map(|c| pin_link(id[&name(c)], c.1, id[c.2], c.3.then_some("main")))
        .collect();
    s.ok(tx(links), fresh("main"));
    for c in citers.iter().filter(|c| !c.3) {
        assert!(!suspect(&s, "main", id[&name(c)]), "{} before", name(c));
    }
    // The changes: TR-024, TR-023, a delete, TR-020, a delete.
    s.ok(
        Cmd::Mutation {
            name: "tx.retract".into(),
            params: vec![
                ("id".into(), t(&id["tr"].to_string())),
                ("reason".into(), t("wrong")),
            ],
            message: String::new(),
            move_lease: None,
        },
        fresh("main"),
    );
    s.ok(tx(supersede("note", "ts2", id["ts"])), fresh("main"));
    let (td, dd) = (id["td"], id["dd"]);
    s.ok(tx(vec![delete(td)]), fresh("main"));
    s.ok(tx(supersede("doc", "ds2", ds)), fresh("main"));
    let d = s
        .ok(tx(vec![delete(dd)]), fresh("main"))
        .commit
        .expect("the delete commits");
    assert_eq!(status(&s, "main", id["tr"]), "retracted");
    assert_eq!(status(&s, "main", id["ts"]), "superseded");
    assert_eq!(status(&s, "main", ds), "superseded");
    assert!(!live(&s, "main", td) && !live(&s, "main", dd));
    for c in &citers {
        let (a, b) = (id[&name(c)], id[c.2]);
        let e = edge(&s, "main", a, c.1, b);
        if c.1 == "depends_on" && c.2 == "dd" {
            assert_eq!(e, None, "{}: the edge is dropped (EG-031)", name(c));
            assert!(!suspect(&s, "main", a), "{}: not suspect", name(c));
            assert!(
                s.st.dag.commits[&d].affected.contains(&a),
                "{}: in the delete's affected (EG-031 `src-affected`)",
                name(c)
            );
        } else {
            assert!(e.is_some(), "{}: the edge stays", name(c));
            assert!(suspect(&s, "main", a), "{}: suspect", name(c));
        }
    }
}
