//! The version-control verbs through the Store API ([API §11.7]–§11.11): merges with the sync-first step, conflicts that
//! land or stage, `merge --continue` and `--abort`, `sync`, `revert` and `cherry-pick` with their refusals and
//! `NotFound`, `undo` and `op restore`, replays; the recursive-virtual-base cases of [F12 §5.7] (E5) built through the
//! API; and after every stream, every commit id recomputed from the states by definition ([`Dag::verify_ids`]).

use super::*;
use crate::api::{Data, Outcome};
use crate::history::MergeData;
use crate::state::{Aspect, KState, KVal, Key};
use crate::tx::Take;
use crate::value::Value;

fn merge(src: &str, into: &str) -> Cmd {
    Cmd::Merge {
        src: src.into(),
        into: Some(into.into()),
        policy: None,
        strict: None,
        base: None,
        message: String::new(),
    }
}

fn branch(name: &str, from: &str) -> Cmd {
    Cmd::BranchCreate {
        name: name.into(),
        from: Some(from.into()),
        kind: None,
    }
}

fn data(r: &crate::api::Reply) -> &MergeData {
    match &r.data {
        Data::Merge(d) => d,
        other => panic!("not a merge result: {other:?}"),
    }
}

fn tip(s: &S, r: &str) -> Option<u64> {
    s.st.dag.live(r).and_then(|x| x.tip)
}

fn state(s: &S, r: &str) -> std::rc::Rc<crate::state::State> {
    s.st.dag.state_at(tip(s, r), &s.st.alloc)
}

fn field(s: &S, r: &str, n: u32, f: &str) -> KState {
    state(s, r).kstate(&Key::Node(Nid(n), Aspect::Field(f.into())))
}

fn prio(p: &str) -> KState {
    KState::Plain(Some(KVal::Value(Value::Enum(p.into()))))
}

fn resolve(key: &str, take: Take) -> Cmd {
    // Each call is a new command: the tests resolve one key on several branches.
    tx(vec![Stmt::Resolve {
        key: key.into(),
        take,
    }])
}

/// The orchestrator on a branch, without the default idempotency key.
fn fresh_on(b: &str) -> Ctx {
    Ctx {
        no_dedupe: true,
        ..orch_on(b)
    }
}

/// The ids of every commit equal their definition over the states ([F07 §15]; [`crate::dag::Dag::verify_ids`]).
fn ids_verify(s: &S) {
    s.st.dag.verify_ids(&s.st.alloc).unwrap();
}

/// A store with tasks #1 and #2 on `main` and `lane/x` forked from it.
fn two_tasks() -> S {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "task a"), task("b", "task b")]), orch());
    s.ok(branch("x", "main"), orch());
    s
}

#[test]
fn a_merge_into_main_syncs_first_and_lands_two_parents() {
    let mut s = two_tasks();
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(1))])]),
        orch_on("lane/x"),
    );
    s.ok(tx(vec![set(2, &[("priority", P::Int(3))])]), orch());
    let main_before = tip(&s, "main");
    let r = s.ok(merge("lane/x", "main"), orch());
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    let sync = d.sync.as_ref().expect("step 0 synced the lane");
    assert_eq!(sync.outcome, "landed");
    let sc = &s.st.dag.commits[&sync.commit.unwrap()];
    assert_eq!(sc.kind, "sync");
    assert_eq!(sc.sync_base, main_before);
    assert_eq!(sc.parents[1], main_before.unwrap());
    let m = &s.st.dag.commits[&tip(&s, "main").unwrap()];
    assert_eq!(m.kind, "merge");
    assert_eq!(m.parents, vec![main_before.unwrap(), sc.seq]);
    assert_eq!(
        d.lca,
        main_before.into_iter().collect::<Vec<_>>(),
        "after the sync the LCA is main's tip"
    );
    assert_eq!(field(&s, "main", 1, "priority"), prio("P1"));
    assert_eq!(field(&s, "main", 2, "priority"), prio("P3"));
    assert_eq!(
        d.absorbed.get("lane/x"),
        Some(&s.st.dag.commits[&sc.seq].ref_seq)
    );
    // Merging again (a new command, not a retry): up to date, nothing appended.
    let n = s.st.commit_seq;
    let fresh = Ctx {
        no_dedupe: true,
        ..orch()
    };
    let r = s.ok(merge("lane/x", "main"), fresh);
    assert_eq!(data(&r).outcome, "up-to-date");
    assert_eq!(s.st.commit_seq, n);
    ids_verify(&s);
}

#[test]
fn a_value_conflict_lands_and_strict_stages_until_continue() {
    let mut s = two_tasks();
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(3))])]),
        orch_on("lane/x"),
    );
    // Landing: a FieldEdit value on the lane; the node is `conflicted`.
    let mut t = S { st: s.st.clone() };
    let r = t.ok(merge("main", "lane/x"), orch());
    assert_eq!(
        data(&r).conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    let st = state(&t, "lane/x");
    assert!(
        st.nodes[&Nid(1)]
            .conflicts
            .contains_key(&Aspect::Field("priority".into()))
    );
    ids_verify(&t);
    // Staging under --strict: the lane does not move; exit 6 `staged`.
    let before = tip(&s, "lane/x");
    let r = s.run(
        Cmd::Merge {
            src: "main".into(),
            into: Some("lane/x".into()),
            policy: None,
            strict: Some(true),
            base: None,
            message: String::new(),
        },
        orch(),
    );
    assert_eq!((r.outcome, r.exit), (Outcome::Staged, 6));
    assert_eq!(r.error.as_ref().unwrap().code, "staged");
    let g = "merge/lane/x/from/main";
    assert_eq!(data(&r).staging_ref.as_deref(), Some(g));
    assert_eq!(tip(&s, "lane/x"), before);
    // A second merge of the pair is refused while the staging ref is open (I41′).
    s.refused(merge("main", "lane/x"), orch(), "staging_exists");
    // Resolve on the staging ref, then continue: the resolution lands.
    s.ok(resolve("#1.priority", Take::Theirs), orch_on(g));
    let r = s.run(
        Cmd::MergeContinue {
            src: Some("main".into()),
            into: Some("lane/x".into()),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(data(&r).outcome, "landed");
    assert!(s.st.dag.live(g).is_none(), "the staging ref is deleted");
    assert_eq!(field(&s, "lane/x", 1, "priority"), prio("P1"));
    assert_eq!(s.st.dag.commits[&tip(&s, "lane/x").unwrap()].kind, "merge");
    ids_verify(&s);
}

#[test]
fn merge_abort_drops_the_staging_ref() {
    let mut s = two_tasks();
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(3))])]),
        orch_on("lane/x"),
    );
    let before = s.st.runtime().refs.len();
    let r = s.run(
        Cmd::Merge {
            src: "main".into(),
            into: Some("lane/x".into()),
            policy: None,
            strict: Some(true),
            base: None,
            message: String::new(),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged);
    let r = s.ok(
        Cmd::MergeAbort {
            src: Some("main".into()),
            into: Some("lane/x".into()),
        },
        orch(),
    );
    assert_eq!(r.data, Data::MergeAbort("merge/lane/x/from/main".into()));
    assert!(s.st.dag.live("merge/lane/x/from/main").is_none());
    assert_eq!(
        s.st.runtime().refs.len(),
        before + 1,
        "the deleted row is kept"
    );
    // The pair can merge again.
    s.ok(merge("main", "lane/x"), orch());
}

#[test]
fn sync_lands_mains_window_as_a_sync_commit() {
    let mut s = two_tasks();
    s.ok(tx(vec![set(2, &[("priority", P::Int(0))])]), orch());
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(4))])]),
        orch_on("lane/x"),
    );
    let preview = s.st.sync_preview("lane/x").unwrap();
    assert_eq!(
        (preview.conflicts, preview.violations, preview.keys),
        (0, 0, 1)
    );
    let r = s.ok(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: true,
        },
        orch(),
    );
    assert_eq!(data(&r).outcome, "landed", "the preview");
    let n = s.st.commit_seq;
    assert_eq!(n, s.st.commit_seq);
    let r = s.ok(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    let c = &s.st.dag.commits[&r.commit.unwrap()];
    assert_eq!(c.kind, "sync");
    assert_eq!(c.sync_base, tip(&s, "main"));
    assert_eq!(field(&s, "lane/x", 2, "priority"), prio("P0"));
    assert_eq!(field(&s, "lane/x", 1, "priority"), prio("P4"));
    s.refused(
        Cmd::Sync {
            lane: Some("main".into()),
            check: false,
        },
        orch(),
        "usage",
    );
    ids_verify(&s);
}

#[test]
fn revert_and_cherry_pick_apply_a_commit_three_ways() {
    let mut s = two_tasks();
    let c = s
        .ok(tx(vec![set(1, &[("priority", P::Int(0))])]), orch())
        .commit
        .unwrap();
    s.ok(tx(vec![set(2, &[("priority", P::Int(4))])]), orch());
    let r = s.ok(
        Cmd::Revert {
            commit: format!("s{c}"),
            onto: Some("main".into()),
            mainline: None,
            message: String::new(),
        },
        orch(),
    );
    let rc = &s.st.dag.commits[&r.commit.unwrap()];
    assert_eq!((rc.kind, rc.origin), ("revert", Some(c)));
    assert_eq!(field(&s, "main", 1, "priority"), KState::ABSENT);
    assert_eq!(field(&s, "main", 2, "priority"), prio("P4"));
    // Cherry-pick a lane commit onto main.
    let l = s
        .ok(
            tx(vec![set(2, &[("title", t("renamed"))])]),
            orch_on("lane/x"),
        )
        .commit
        .unwrap();
    let r = s.ok(
        Cmd::CherryPick {
            commit: format!("s{l}"),
            onto: Some("main".into()),
            message: String::new(),
        },
        orch(),
    );
    let pc = &s.st.dag.commits[&r.commit.unwrap()];
    assert_eq!((pc.kind, pc.origin), ("cherry-pick", Some(l)));
    assert_eq!(
        field(&s, "main", 2, "title"),
        KState::Plain(Some(KVal::Value(Value::Text("renamed".into()))))
    );
    ids_verify(&s);
}

#[test]
fn reverting_a_creation_leaves_a_tombstone_and_dependents_refuse() {
    let mut s = S::base();
    let c1 = s.ok(tx(vec![task("a", "task a")]), orch()).commit.unwrap();
    // DM-017: the revert of the creation deletes the node with the empty reason and no replacement.
    let mut t = S { st: s.st.clone() };
    t.ok(
        Cmd::Revert {
            commit: format!("s{c1}"),
            onto: Some("main".into()),
            mainline: None,
            message: String::new(),
        },
        orch(),
    );
    let st = state(&t, "main");
    let x = &st.nodes[&Nid(1)];
    assert!(!x.live());
    assert_eq!(x.tomb.as_ref().unwrap().reason.as_deref(), Some(""));
    ids_verify(&t);
    // DM-008: a later commit added a child of the node c1 created.
    let c2 = s
        .ok(tx(vec![child("b", "child", Target::Id(Nid(1)))]), orch())
        .commit
        .unwrap();
    let r = s.refused(
        Cmd::Revert {
            commit: format!("s{c1}"),
            onto: Some("main".into()),
            mainline: None,
            message: String::new(),
        },
        orch(),
        "revert_refused",
    );
    let e = r.error.unwrap();
    assert_eq!(e.get_str("case"), Some("dependents"));
    assert_eq!(
        e.get("dependents"),
        Some(&crate::err::Kv::List(vec![crate::err::Kv::Commit(c2)]))
    );
}

#[test]
fn reverts_of_syncs_and_merges_are_refused() {
    let mut s = two_tasks();
    s.ok(tx(vec![set(2, &[("priority", P::Int(0))])]), orch());
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(4))])]),
        orch_on("lane/x"),
    );
    let r = s.ok(merge("lane/x", "main"), orch());
    let sync = data(&r).sync.as_ref().unwrap().commit.unwrap();
    let m = r.commit.unwrap();
    let rev = |c: u64, mainline: Option<u32>| Cmd::Revert {
        commit: format!("s{c}"),
        onto: Some("main".into()),
        mainline,
        message: String::new(),
    };
    let e = s
        .refused(rev(sync, None), orch(), "revert_refused")
        .error
        .unwrap();
    assert_eq!(e.get_str("case"), Some("sync"));
    let e = s
        .refused(rev(m, None), orch(), "revert_refused")
        .error
        .unwrap();
    assert_eq!(e.get_str("case"), Some("mainline"));
    s.refused(rev(m, Some(2)), orch(), "usage");
    // `--mainline 1`: the merge's changes against main's parent are undone.
    s.ok(rev(m, Some(1)), orch());
    assert_eq!(field(&s, "main", 1, "priority"), KState::ABSENT);
    assert_eq!(field(&s, "main", 2, "priority"), prio("P0"));
    ids_verify(&s);
}

#[test]
fn a_revert_whose_node_is_gone_stages_not_found() {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "task a")]), orch());
    let c = s
        .ok(tx(vec![set(1, &[("priority", P::Int(0))])]), orch())
        .commit
        .unwrap();
    s.ok(
        tx(vec![Stmt::Delete {
            target: Target::Id(Nid(1)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("gone".into()),
        }]),
        orch(),
    );
    let before = tip(&s, "main");
    let r = s.run(
        Cmd::Revert {
            commit: format!("s{c}"),
            onto: Some("main".into()),
            mainline: None,
            message: String::new(),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.violations[0].class, "NotFound");
    assert_eq!(d.violations[0].key, "#1.priority");
    let g = d.staging_ref.clone().unwrap();
    assert!(g.starts_with("merge/main/from/c") && g.len() == "merge/main/from/c".len() + 64);
    assert_eq!(tip(&s, "main"), before);
    // [F12 §6.5] "On a violation's key": the `NotFound` key keeps main's value (ours), and the revert lands.
    s.ok(resolve("#1.priority", Take::Ours), fresh_on(&g));
    let src = &g["merge/main/from/".len()..];
    let r = s.run(continue_(src, "main"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.violations.is_empty(), "{:?}", d.violations);
    let m = &s.st.dag.commits[&tip(&s, "main").unwrap()];
    assert_eq!(
        (m.kind, m.parents.clone(), m.origin),
        ("revert", vec![before.unwrap()], Some(c))
    );
    assert!(state(&s, "main").live(Nid(1)).is_none());
    assert!(s.st.dag.live(&g).is_none(), "G is deleted");
    ids_verify(&s);
}

#[test]
fn undo_and_op_restore_move_refs_back() {
    let mut s = S::base();
    let c1 = s.ok(tx(vec![task("a", "task a")]), orch()).commit.unwrap();
    let c2 = s
        .ok(tx(vec![set(1, &[("priority", P::Int(0))])]), orch())
        .commit
        .unwrap();
    s.refused(
        Cmd::Undo {
            ref_: Some("main".into()),
            n: None,
            expect: Some(format!("s{c1}")),
        },
        orch(),
        "E402",
    );
    let r = s.ok(
        Cmd::Undo {
            ref_: Some("main".into()),
            n: None,
            expect: Some(format!("s{c2}")),
        },
        orch(),
    );
    match &r.data {
        Data::Undo(u) => assert_eq!((u.old, u.new, u.moved_back), (Some(c2), Some(c1), 1)),
        other => panic!("{other:?}"),
    }
    assert_eq!(tip(&s, "main"), Some(c1));
    s.refused(
        Cmd::Undo {
            ref_: Some("main".into()),
            n: Some(9),
            expect: None,
        },
        orch(),
        "E301",
    );
    // main@3 is the value before c1, zero ([F12 §3.5]).
    let e = s
        .refused(
            Cmd::Undo {
                ref_: Some("main".into()),
                n: Some(3),
                expect: None,
            },
            orch(),
            "E301",
        )
        .error
        .unwrap();
    assert!(e.detail.contains("zero value"), "{}", e.detail);
    // `restore_seq` names a commit seq; a ref move no commit carries lies after the newest commit appended before it.
    // The undo above lies after c2; `lane/x` is created after c2 at c1, c3 lands on it, and `lane/late` is created
    // after c3.
    s.ok(branch("x", "main"), orch());
    let c3 = s
        .ok(
            tx(vec![set(1, &[("priority", P::Int(4))])]),
            orch_on("lane/x"),
        )
        .commit
        .unwrap();
    s.ok(branch("late", "main"), orch());
    // At c1: `main` at c1 (c2 and the undo lie after it), and neither lane exists.
    let dry = s.ok(
        Cmd::OpRestore { seq: c1 },
        Ctx {
            dry: true,
            ..orch()
        },
    );
    assert_eq!(dry.outcome, Outcome::Dry);
    assert!(
        s.st.dag.live("lane/late").is_some(),
        "a dry run moves nothing"
    );
    let r = s.ok(Cmd::OpRestore { seq: c1 }, orch());
    match (&dry.data, &r.data) {
        (Data::OpRestore(p), Data::OpRestore(d)) => {
            assert_eq!(p.moved, d.moved, "the dry run lists what the run moves");
            assert!(
                d.moved.contains(&("lane/late".into(), Some(c1), None)),
                "{:?}",
                d.moved
            );
            assert!(
                d.moved.contains(&("lane/x".into(), Some(c3), None)),
                "{:?}",
                d.moved
            );
            assert!(
                !d.moved.iter().any(|m| m.0 == "main"),
                "main is at c1 already"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(s.st.dag.live("lane/late").is_none() && s.st.dag.live("lane/x").is_none());
    // Back to c3: `lane/x` (deleted after c3, still held) is restored at its tip c3, `main` stays at c1 (its undo lies
    // before c3), and `lane/late`, created after c3, stays deleted.
    let r = s.ok(Cmd::OpRestore { seq: c3 }, orch());
    match &r.data {
        Data::OpRestore(d) => assert_eq!(d.moved, vec![("lane/x".into(), None, Some(c3))]),
        other => panic!("{other:?}"),
    }
    assert_eq!(tip(&s, "lane/x"), Some(c3));
    assert_eq!(tip(&s, "main"), Some(c1));
    assert!(s.st.dag.live("lane/late").is_none());
    // Restoring the same seq again moves nothing and records nothing.
    let r = s.ok(Cmd::OpRestore { seq: c3 }, fresh_on("main"));
    match &r.data {
        Data::OpRestore(d) => assert!(d.moved.is_empty(), "{:?}", d.moved),
        other => panic!("{other:?}"),
    }
    // A seq beyond the newest commit names nothing.
    s.refused(Cmd::OpRestore { seq: c3 + 1 }, orch(), "E301");
    ids_verify(&s);
}

/// `op restore` of the newest seq takes back the ref moves made since that commit: a name a later ref took is freed for
/// the ref restored under it, and a deleted ref's task leases end (LE-008).
#[test]
fn op_restore_frees_a_retaken_name_and_ends_leases() {
    let mut s = two_tasks();
    let c = s
        .ok(
            tx(vec![set(1, &[("priority", P::Int(1))])]),
            orch_on("lane/x"),
        )
        .commit
        .unwrap();
    let old_id = s.st.dag.live("lane/x").unwrap().id;
    s.ok(
        Cmd::BranchDelete {
            name: "lane/x".into(),
            force: true,
        },
        orch(),
    );
    // A new command (the first `branch x` would replay under the default key).
    s.ok(branch("x", "main"), fresh_on("main"));
    let new_id = s.st.dag.live("lane/x").unwrap().id;
    assert_ne!(old_id, new_id);
    s.ok(branch("y", "main"), orch());
    s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: Some("dev".into()),
            ttl: Some(t("10m")),
            start: false,
            run: None,
            session: false,
        },
        orch_on("lane/y"),
    );
    let lease =
        s.st.leases
            .values()
            .find(|l| l.branch == "lane/y")
            .map(|l| l.id)
            .expect("a lease on lane/y");
    let r = s.ok(Cmd::OpRestore { seq: c }, orch());
    match &r.data {
        Data::OpRestore(d) => {
            let main_tip = tip(&s, "main");
            assert_eq!(
                d.moved,
                vec![
                    ("lane/x".into(), main_tip, None),
                    ("lane/y".into(), main_tip, None),
                    ("lane/x".into(), None, Some(c)),
                ]
            );
        }
        other => panic!("{other:?}"),
    }
    let x = s.st.dag.live("lane/x").unwrap();
    assert_eq!(
        (x.id, x.tip),
        (old_id, Some(c)),
        "the old lane/x is back under its name"
    );
    assert!(s.st.dag.live("lane/y").is_none());
    assert_eq!(
        s.st.leases[&lease].ended,
        Some(crate::lease::EndReason::BranchDeleted),
        "LE-008"
    );
    ids_verify(&s);
}

#[test]
fn a_keyed_merge_replays() {
    let mut s = two_tasks();
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(1))])]),
        orch_on("lane/x"),
    );
    let ctx = Ctx {
        key: Some("m1".into()),
        ..orch()
    };
    let a = s.ok(merge("lane/x", "main"), ctx.clone());
    let n = s.st.commit_seq;
    let b = s.ok(merge("lane/x", "main"), ctx);
    assert_eq!(b.outcome, Outcome::Replayed);
    assert_eq!(s.st.commit_seq, n);
    assert_eq!((b.commit, &b.data), (a.commit, &a.data));
}

/// Two lanes that each set `priority` of task #1 from its base R, then merge each other's first commit and resolve:
/// `lane/a` keeps `ra`, `lane/b` keeps `rb` (either may be `None`, leaving the conflict value). The next merge of
/// `lane/b` into `lane/a` has two LCAs ([F12 §5.8]).
fn criss_cross(ra: Option<Take>, rb: Option<Take>) -> (S, crate::api::Reply) {
    let mut s = criss_crossed(ra, rb);
    let r = s.ok(merge("lane/b", "lane/a"), orch());
    (s, r)
}

/// [`criss_cross`]'s store before its last merge.
fn criss_crossed(ra: Option<Take>, rb: Option<Take>) -> S {
    let mut s = S::base();
    s.ok(tx(vec![task("k", "task k"), task("m", "task m")]), orch());
    for l in ["a", "b"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(0))])]),
        orch_on("lane/a"),
    );
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(4))])]),
        orch_on("lane/b"),
    );
    s.ok(branch("a1", "lane/a"), orch());
    s.ok(branch("b1", "lane/b"), orch());
    s.ok(merge("lane/b1", "lane/a"), orch());
    s.ok(merge("lane/a1", "lane/b"), orch());
    if let Some(t) = ra {
        s.ok(resolve("#1.priority", t), fresh_on("lane/a"));
    }
    if let Some(t) = rb {
        s.ok(resolve("#1.priority", t), fresh_on("lane/b"));
    }
    s
}

/// VBC-2 ([F12 §5.8]'s example): the sides resolved the criss-cross differently: a conflict whose base is the inner
/// base's value.
#[test]
fn vbc_2_resolved_differently_conflicts() {
    let (s, r) = criss_cross(Some(Take::Ours), Some(Take::Ours));
    let d = data(&r);
    assert_eq!(d.lca.len(), 2);
    assert!(d.virtual_base);
    assert_eq!(
        d.conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    match field(&s, "lane/a", 1, "priority") {
        KState::Conflict(c) => {
            assert_eq!(c.base, None, "the inner base R has the default priority");
            assert_eq!(c.ours, Some(KVal::Value(Value::Enum("P0".into()))));
            assert_eq!(c.theirs, Some(KVal::Value(Value::Enum("P4".into()))));
        }
        other => panic!("{other:?}"),
    }
    ids_verify(&s);
}

/// VBC-1: both sides resolved identically: clean.
#[test]
fn vbc_1_resolved_identically_is_clean() {
    // lane/a keeps its own P0 (ours); lane/b takes lane/a's P0 (theirs).
    let (s, r) = criss_cross(Some(Take::Ours), Some(Take::Theirs));
    assert!(data(&r).conflicts.is_empty(), "{:?}", data(&r).conflicts);
    assert_eq!(field(&s, "lane/a", 1, "priority"), prio("P0"));
}

/// VBC-3: one side holds the virtual base's conflict value unchanged, the other resolved: clean, the resolving side's
/// value (RVB-2, RVB-3). The virtual base's conflict value has the first LCA by (gen, id) as `ours`, and a side's own
/// conflict has its own value as `ours`; only the side whose orientation matches holds the base's value unchanged, so
/// VBC-3's clean result holds for one lane and the other lane's untouched conflict meets RVB-4. Which lane that is
/// depends on the LCAs' ids: the WP-91 review's spec finding S2. The test asserts both results as the rules give them.
#[test]
fn vbc_3_one_side_untouched_takes_the_resolution() {
    // lane/a leaves its conflict (ours P0, theirs P4) and lane/b resolves to its P4; then the reverse.
    let (s1, r1) = criss_cross(None, Some(Take::Ours));
    let (s2, r2) = criss_cross(Some(Take::Ours), None);
    let a_first = {
        let d = data(&r1);
        let l1 = s1.st.dag.commits[&d.lca[0]].clone();
        // The first LCA is lane/a's commit exactly when it set P0.
        l1.changeset.values().any(|(_, v)| *v == prio("P0"))
    };
    let (clean, other) = if a_first {
        ((&s1, &r1), &r2)
    } else {
        ((&s2, &r2), &r1)
    };
    assert!(
        data(clean.1).conflicts.is_empty(),
        "{:?}",
        data(clean.1).conflicts
    );
    let want = if a_first { "P4" } else { "P0" };
    assert_eq!(field(clean.0, "lane/a", 1, "priority"), prio(want));
    assert_eq!(
        data(other).conflicts.len(),
        1,
        "the other orientation is not the base's value"
    );
}

/// VBC-4: a key both LCAs agree on and neither side touched never conflicts; VBC-9: a counter incremented on both
/// LCAs and on both sides sums over the virtual base.
#[test]
fn vbc_4_and_9_untouched_keys_and_counters() {
    let (s, _) = criss_cross(Some(Take::Ours), Some(Take::Ours));
    assert_eq!(field(&s, "lane/a", 2, "priority"), KState::ABSENT);
    let mut s = S::base();
    s.ok(tx(vec![task("k", "task k")]), orch());
    for l in ["a", "b"] {
        s.ok(branch(l, "main"), orch());
    }
    let incr = |n: i64| {
        tx(vec![Stmt::Set {
            target: Target::Id(Nid(1)),
            fields: vec![],
            incr: vec![("reopen_count".into(), n)],
            body: None,
            guard: None,
        }])
    };
    s.ok(incr(1), orch_on("lane/a"));
    s.ok(incr(10), orch_on("lane/b"));
    s.ok(branch("a1", "lane/a"), orch());
    s.ok(branch("b1", "lane/b"), orch());
    s.ok(merge("lane/b1", "lane/a"), orch());
    s.ok(merge("lane/a1", "lane/b"), orch());
    s.ok(incr(100), orch_on("lane/a"));
    s.ok(incr(1000), orch_on("lane/b"));
    let r = s.ok(merge("lane/b", "lane/a"), orch());
    assert!(data(&r).virtual_base);
    assert_eq!(
        state(&s, "lane/a").kstate(&Key::Node(Nid(1), Aspect::Counter("reopen_count".into()))),
        KState::Plain(Some(KVal::Value(Value::Counter(1111))))
    );
    ids_verify(&s);
}

/// VBC-5: Base(a, b) = Base(b, a) — the LCAs come sorted by (gen, id) whatever the order they are asked in — and the
/// two merge directions exchange the conflict's sides ([F12 §3.6], §7.7).
#[test]
fn vbc_5_the_base_is_symmetric() {
    let s = criss_crossed(Some(Take::Ours), Some(Take::Ours));
    let (a, b) = (tip(&s, "lane/a"), tip(&s, "lane/b"));
    let dag = &s.st.dag;
    assert_eq!(dag.lcas(a, b), dag.lcas(b, a));
    let uid = |n: Nid| s.st.alloc.uids[&n];
    let nid = |_: crate::value::Uid| None;
    let mut b1 = crate::vcs::Bases::new(dag, &s.st.alloc, &uid, &nid);
    let mut b2 = crate::vcs::Bases::new(dag, &s.st.alloc, &uid, &nid);
    let x = b1.base(a, b, None);
    let y = b2.base(b, a, None);
    assert_eq!(x.lcas, y.lcas);
    assert_eq!(*x.st, *y.st);
    // The two directions over that base: one conflict, its class and base equal, its sides exchanged.
    let (mut s1, mut s2) = (S { st: s.st.clone() }, S { st: s.st.clone() });
    s1.ok(merge("lane/b", "lane/a"), orch());
    s2.ok(merge("lane/a", "lane/b"), orch());
    match (
        field(&s1, "lane/a", 1, "priority"),
        field(&s2, "lane/b", 1, "priority"),
    ) {
        (KState::Conflict(x), KState::Conflict(y)) => {
            assert_eq!((&x.class, &x.base), (&y.class, &y.base));
            assert_eq!((&x.ours, &x.theirs), (&y.theirs, &y.ours));
            assert_eq!(x.ours, Some(KVal::Value(Value::Enum("P0".into()))));
        }
        other => panic!("{other:?}"),
    }
}

/// A merge into `main` from a ref that is not a branch takes no sync step: a tag never moves ([F12 §2.2]), so the merge
/// runs directly over the base of §4.3 and the tag keeps its tip (the WP-91 review's spec finding S4). The model has
/// no `tag` verb before WP-92's `EnvGit`, so the tag's row is written directly.
#[test]
fn a_merge_of_a_tag_into_main_syncs_nothing() {
    use crate::dag::{MoveReason, Ref, RefKind, RefMove};
    let mut s = two_tasks();
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(1))])]),
        orch_on("lane/x"),
    );
    let at = tip(&s, "lane/x");
    let id = s.st.next_ref_id;
    s.st.next_ref_id += 1;
    s.st.dag.refs.insert(
        id,
        Ref {
            id,
            name: "tags/v1".into(),
            kind: RefKind::Tag,
            tip: at,
            ref_seq_next: 1,
            fork: at,
            deleted: false,
            message: None,
            pinned: false,
            moves: vec![RefMove {
                old: None,
                new: at,
                reason: MoveReason::Create,
                actor: "orchestrator".into(),
                hlc: 0,
            }],
        },
    );
    // main moves past the tag's history.
    s.ok(tx(vec![set(2, &[("priority", P::Int(3))])]), orch());
    let main_before = tip(&s, "main");
    let r = s.ok(merge("tags/v1", "main"), orch());
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.sync.is_none(), "no sync step for a tag");
    assert_eq!(tip(&s, "tags/v1"), at, "the tag never moves");
    let m = &s.st.dag.commits[&tip(&s, "main").unwrap()];
    assert_eq!(
        (m.kind, m.parents.clone()),
        ("merge", vec![main_before.unwrap(), at.unwrap()])
    );
    assert_eq!(field(&s, "main", 1, "priority"), prio("P1"));
    assert_eq!(field(&s, "main", 2, "priority"), prio("P3"));
    ids_verify(&s);
}

/// A `move` statement: node `n` under `under`, or to the root.
fn mv(n: u32, under: Option<u32>) -> Cmd {
    tx(vec![Stmt::Move {
        target: Target::Id(Nid(n)),
        under: under.map(|p| Target::Id(Nid(p))),
        position: None,
    }])
}

/// The parents of nodes #1 and #2 on a ref.
fn parents_12(s: &S, r: &str) -> (Option<Nid>, Option<Nid>) {
    let st = state(s, r);
    (st.nodes[&Nid(1)].parent, st.nodes[&Nid(2)].parent)
}

/// VBC-10 ([F12 §5.7]): a move that closes a cycle inside a virtual merge is skipped silently in the base, and the real
/// merge validates its own candidate. L₁ (lane/a) puts #1 under #2 and lane/a moves #1 back; then L₂ (lane/b) puts #2
/// under #1 and lane/b moves #2 back. Each merge replays both sides' commits since its base in (hlc, commit id) order
/// (RS-007 as the WP-91 review's spec finding S1b states it): lane/b1 into lane/a meets #1 back at the root, and
/// lane/a1 into lane/b undoes L₂'s move and then applies lane/b's later move back, so both cross merges are clean.
/// Merging lane/b into lane/a has the LCAs {L₁, L₂}, whose virtual merge meets the cycle.
#[test]
fn vbc_10_a_cycle_in_the_virtual_base_is_skipped_silently() {
    let mut s = S::base();
    s.ok(tx(vec![task("k", "task k"), task("m", "task m")]), orch());
    for l in ["a", "b"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(mv(1, Some(2)), orch_on("lane/a"));
    s.ok(branch("a1", "lane/a"), orch());
    s.ok(mv(1, None), orch_on("lane/a"));
    s.ok(mv(2, Some(1)), orch_on("lane/b"));
    s.ok(branch("b1", "lane/b"), orch());
    s.ok(mv(2, None), orch_on("lane/b"));
    for (src, into) in [("lane/b1", "lane/a"), ("lane/a1", "lane/b")] {
        let r = s.ok(merge(src, into), orch());
        assert_eq!(data(&r).outcome, "landed", "{src} into {into}");
        assert!(data(&r).violations.is_empty());
    }
    assert_eq!(parents_12(&s, "lane/a"), (None, Some(Nid(1))));
    assert_eq!(parents_12(&s, "lane/b"), (Some(Nid(2)), None));
    // The virtual base: L₁'s move (the earlier hlc) applied, L₂'s skipped; a forest, and nothing recorded.
    let (a, b) = (tip(&s, "lane/a"), tip(&s, "lane/b"));
    {
        let uid = |n: Nid| s.st.alloc.uids[&n];
        let nid = |_: crate::value::Uid| None;
        let mut bases = crate::vcs::Bases::new(&s.st.dag, &s.st.alloc, &uid, &nid);
        let v = bases.base(a, b, None);
        assert!(v.virtual_base && v.lcas.len() == 2);
        assert_eq!(
            (v.st.nodes[&Nid(1)].parent, v.st.nodes[&Nid(2)].parent),
            (Some(Nid(2)), None)
        );
    }
    // The real merge validates its own candidate: after lane/a's merge put #2 under #1, lane/b's later merge put #1
    // under #2, which closes a cycle; the move is skipped and the merge stages with `HierarchyCycle` on #1.parent.
    let r = s.run(merge("lane/b", "lane/a"), orch());
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!((d.lca.len(), d.virtual_base), (2, true));
    assert!(d.conflicts.is_empty(), "{:?}", d.conflicts);
    assert_eq!(violations(d), vec![("#1.parent", "HierarchyCycle")]);
    let g = "merge/lane/a/from/lane/b";
    assert_eq!(
        parents_12(&s, g),
        (None, Some(Nid(1))),
        "the staged candidate"
    );
    // [F12 §6.5] "On a violation's key": #1.parent takes lane/a's value, the one the candidate holds, and the merge
    // lands with lane/a's hierarchy.
    s.ok(resolve("#1.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("lane/b", "lane/a"), fresh_on("lane/a"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(
        (d.outcome, d.lca.len(), d.virtual_base),
        ("landed", 2, true)
    );
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    assert_eq!(parents_12(&s, "lane/a"), (None, Some(Nid(1))));
    assert!(s.st.dag.live(g).is_none(), "G is deleted");
    ids_verify(&s);
}

/// A strict merge into lane/x, staged with a `FieldEdit` on #1.priority and a `HierarchyCycle` on #3.parent, the move
/// of main's that Kleppmann skipped: lane/x put #2 under #3 first, main #3 under #2 later.
fn staged_with_a_cycle() -> S {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
        ]),
        orch(),
    );
    s.ok(branch("x", "main"), orch());
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(3))])]),
        orch_on("lane/x"),
    );
    s.ok(mv(2, Some(3)), orch_on("lane/x"));
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    s.ok(mv(3, Some(2)), orch());
    let r = s.run(strict_merge("main", "lane/x"), orch());
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(
        d.conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    assert_eq!(violations(d), vec![("#3.parent", "HierarchyCycle")]);
    s
}

fn strict_merge(src: &str, into: &str) -> Cmd {
    Cmd::Merge {
        src: src.into(),
        into: Some(into.into()),
        policy: None,
        strict: Some(true),
        base: None,
        message: String::new(),
    }
}

fn continue_(src: &str, into: &str) -> Cmd {
    Cmd::MergeContinue {
        src: Some(src.into()),
        into: Some(into.into()),
    }
}

/// (key, class) of a result's violations.
fn violations(d: &MergeData) -> Vec<(&str, &str)> {
    d.violations
        .iter()
        .map(|v| (v.key.as_str(), v.class.as_str()))
        .collect()
}

/// [F12 §9.4] steps 1–3: resolving #1.priority alone and continuing re-stages on G. The recomputed candidate keeps the
/// typed records and the skipped move of every key no resolution set, so the `HierarchyCycle` of #3.parent stays; the
/// resolved key holds its resolution and no conflict. Then [F12 §6.5] "On a violation's key": #3.parent, a key of G's
/// staged violations, takes lane/x's value (ours: #3 at the root, which the candidate already holds, so the resolve
/// commit changes no value and still names the key); main's value would close a cycle and is refused, and a key no
/// conflict or violation names is `not_found`. The next continue overlays the key, V01 no longer reports it, and the
/// merge lands with lane/x's hierarchy.
#[test]
fn a_partial_resolution_restages() {
    let mut s = staged_with_a_cycle();
    let g = "merge/lane/x/from/main";
    let before = tip(&s, "lane/x");
    s.ok(resolve("#1.priority", Take::Theirs), orch_on(g));
    let r = s.run(continue_("main", "lane/x"), orch());
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "staged");
    assert!(d.conflicts.is_empty(), "{:?}", d.conflicts);
    assert_eq!(violations(d), vec![("#3.parent", "HierarchyCycle")]);
    assert_eq!(d.staging_ref.as_deref(), Some(g));
    assert!(d.notices.is_empty());
    assert_eq!(tip(&s, "lane/x"), before, "dst did not move");
    assert!(s.st.dag.live(g).is_some(), "G stays");
    assert_eq!(field(&s, g, 1, "priority"), prio("P1"));
    let c = &s.st.dag.commits[&tip(&s, g).unwrap()];
    assert_eq!(
        (c.kind, c.parents.clone()),
        ("merge", vec![before.unwrap(), tip(&s, "main").unwrap()])
    );
    assert_eq!(
        c.violations
            .iter()
            .map(|v| (v.class, v.key.clone()))
            .collect::<Vec<_>>(),
        vec![("HierarchyCycle", Some(Key::Node(Nid(3), Aspect::Hierarchy)))],
        "the re-staged commit's `Violation` ops"
    );
    s.refused(resolve("#3.parent", Take::Theirs), fresh_on(g), "E405");
    s.refused(resolve("#2.parent", Take::Ours), fresh_on(g), "not_found");
    let r = s.ok(resolve("3.order", Take::Ours), fresh_on(g));
    let rc = &s.st.dag.commits[&r.commit.expect("the resolve commit")];
    assert!(rc.changeset.is_empty());
    assert_eq!(rc.resolves, vec![Key::Node(Nid(3), Aspect::Hierarchy)]);
    let main_tip = tip(&s, "main").unwrap();
    let r = s.run(continue_("main", "lane/x"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty() && d.notices.is_empty(),
        "{:?} {:?} {:?}",
        d.violations,
        d.conflicts,
        d.notices
    );
    let m = &s.st.dag.commits[&tip(&s, "lane/x").unwrap()];
    assert_eq!(
        (m.kind, m.parents.clone()),
        ("merge", vec![before.unwrap(), main_tip])
    );
    let st = state(&s, "lane/x");
    assert_eq!(
        (st.nodes[&Nid(2)].parent, st.nodes[&Nid(3)].parent),
        (Some(Nid(3)), None),
        "lane/x's hierarchy"
    );
    assert_eq!(field(&s, "lane/x", 1, "priority"), prio("P1"));
    assert!(s.st.dag.live(g).is_none(), "G is deleted");
    ids_verify(&s);
}

/// [F12 §6.5] "On a violation's key" for `DanglingEdge`: lane/x deletes #3 and #4 while main makes #1 block #3 and #2
/// block #4, so the sync stages with two dangling edges. On G each key resolves on its own, although G's view keeps
/// the other violation (a `RESOLVE` there is refused only for a violation it introduces): #1's edge takes lane/x's
/// value (absent), #2's edge is re-pointed to #1 (`repoint`: the edge to the deleted #4 goes, an edge to #1 of its
/// kind comes). The next continue lands with both.
#[test]
fn dangling_edge_keys_resolve_one_at_a_time() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
            task("d", "task d"),
        ]),
        orch(),
    );
    s.ok(branch("x", "main"), orch());
    for n in [3, 4] {
        s.ok(
            tx(vec![Stmt::Delete {
                target: Target::Id(Nid(n)),
                policy: None,
                replaced_by: None,
                release: false,
                reason: Some("gone".into()),
            }]),
            orch_on("lane/x"),
        );
    }
    s.ok(tx(vec![link(1, "blocks", 3), link(2, "blocks", 4)]), orch());
    let r = s.run(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    // In canonical key order, by uid ([F13 §5] VO-2).
    let mut v = violations(d);
    v.sort();
    assert_eq!(
        v,
        vec![
            ("edge:#1:blocks:#3", "DanglingEdge"),
            ("edge:#2:blocks:#4", "DanglingEdge")
        ]
    );
    let g = "merge/lane/x/from/main";
    s.ok(resolve("edge:1:blocks:3", Take::Ours), fresh_on(g));
    s.refused(
        resolve("edge:#2:blocks:#4", Take::Value(P::Int(1))),
        fresh_on(g),
        "usage",
    );
    s.ok(
        resolve("edge:#2:blocks:#4", Take::Repoint(Target::Id(Nid(1)))),
        fresh_on(g),
    );
    let edges = |s: &S, r: &str, n: u32| -> Vec<(String, u32)> {
        state(s, r).nodes[&Nid(n)]
            .out
            .keys()
            .map(|k| (k.kind.clone(), k.dst.0))
            .collect()
    };
    assert!(edges(&s, g, 1).is_empty());
    assert_eq!(edges(&s, g, 2), vec![("blocks".to_string(), 1)]);
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.violations.is_empty(), "{:?}", d.violations);
    assert!(edges(&s, "lane/x", 1).is_empty());
    assert_eq!(edges(&s, "lane/x", 2), vec![("blocks".to_string(), 1)]);
    assert!(s.st.dag.live(g).is_none(), "G is deleted");
    ids_verify(&s);
}

/// The WP-91 review's case for spec finding S1b through the Store API: main has #2 under #1; lane/x restructures over
/// three commits — #2 to the root, #1 under #2, #2 under #3, each state a forest — while main changes only
/// #4.priority. Each lane commit is one Kleppmann step, so `sync lane/x` lands with the lane's hierarchy, #1 under #2
/// under #3, and no `HierarchyCycle`; so does `merge lane/x --into main`, after that sync and without it (its step 0 is
/// the same sync).
#[test]
fn a_restructure_over_three_commits_syncs_and_merges() {
    let setup = || {
        let mut s = S::base();
        s.ok(
            tx(vec![
                task("a", "task a"),
                task("b", "task b"),
                task("c", "task c"),
                task("d", "task d"),
            ]),
            orch(),
        );
        s.ok(mv(2, Some(1)), orch());
        s.ok(branch("x", "main"), orch());
        s.ok(mv(2, None), orch_on("lane/x"));
        s.ok(mv(1, Some(2)), orch_on("lane/x"));
        s.ok(mv(2, Some(3)), orch_on("lane/x"));
        s.ok(tx(vec![set(4, &[("priority", P::Int(1))])]), orch());
        s
    };
    let parents = |s: &S, r: &str| {
        let st = state(s, r);
        let p = |n: u32| st.nodes[&Nid(n)].parent;
        (p(1), p(2), p(3))
    };
    let want = (Some(Nid(2)), Some(Nid(3)), None);
    let mut s = setup();
    let r = s.ok(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.violations.is_empty(), "{:?}", d.violations);
    assert_eq!(parents(&s, "lane/x"), want);
    assert_eq!(field(&s, "lane/x", 4, "priority"), prio("P1"));
    let r = s.ok(merge("lane/x", "main"), orch());
    assert_eq!(data(&r).outcome, "landed");
    assert!(data(&r).violations.is_empty());
    assert_eq!(parents(&s, "main"), want);
    ids_verify(&s);
    let mut s = setup();
    let r = s.ok(merge("lane/x", "main"), orch());
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert_eq!(d.sync.as_ref().map(|x| x.outcome), Some("landed"));
    assert!(d.violations.is_empty(), "{:?}", d.violations);
    assert_eq!(parents(&s, "lane/x"), want);
    assert_eq!(parents(&s, "main"), want);
    assert_eq!(field(&s, "main", 4, "priority"), prio("P1"));
    ids_verify(&s);
}

/// PR-014 ([F12 §9.4] step 2): a resolution made before dst changed its key again is stale — a notice names it, the key
/// keeps the recomputed candidate's value (a new `FieldEdit`) and, under `merge.strict`, the command re-stages on G.
/// Resolved again there, the next continue lands: G's commits are found by their ref, since the re-staged commit's
/// first parent is dst's tip.
#[test]
fn a_stale_resolution_is_named_and_restages() {
    let mut s = two_tasks();
    s.ok(
        Cmd::ConfigSet {
            key: "merge.strict".into(),
            value: "true".into(),
            scope: crate::confcmd::FileScope::Store,
        },
        Ctx::default(),
    );
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(3))])]),
        orch_on("lane/x"),
    );
    let r = s.run(strict_merge("main", "lane/x"), orch());
    assert_eq!(r.outcome, Outcome::Staged);
    let g = "merge/lane/x/from/main";
    s.ok(resolve("#1.priority", Take::Theirs), orch_on(g));
    // dst moves after staging and changes the resolved key again.
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(4))])]),
        orch_on("lane/x"),
    );
    let d1 = tip(&s, "lane/x");
    let r = s.run(continue_("main", "lane/x"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.notices.len(), 1, "{:?}", d.notices);
    assert!(d.notices[0].contains("#1.priority"), "{}", d.notices[0]);
    assert_eq!(
        d.conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    let c = &s.st.dag.commits[&r.commit.unwrap()];
    assert_eq!(c.parents, vec![d1.unwrap(), tip(&s, "main").unwrap()]);
    assert_eq!(tip(&s, g), r.commit);
    // Resolved again on G (lane/x's P4) and continued: the resolution is not stale and lands.
    s.ok(resolve("#1.priority", Take::Ours), fresh_on(g));
    let r = s.run(continue_("main", "lane/x"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.notices.is_empty(), "{:?}", d.notices);
    assert_eq!(field(&s, "lane/x", 1, "priority"), prio("P4"));
    assert!(s.st.dag.live(g).is_none(), "G is deleted");
    ids_verify(&s);
}

/// VBC-6: three LCAs are merged in (gen, id) order with the inner bases of [F12 §5.2]; VBC-7: a nested criss-cross
/// recurses; VBC-12: two stores running one history write byte-identical merge commits.
#[test]
fn vbc_6_7_12_three_lcas_nesting_and_determinism() {
    let build = || {
        let mut s = S::base();
        s.ok(tx(vec![task("k", "task k")]), orch());
        for (l, f) in [("a", "estimate"), ("b", "title"), ("c", "priority")] {
            s.ok(branch(l, "main"), orch());
            let v = match f {
                "estimate" => P::Int(5),
                "title" => t("renamed"),
                _ => P::Int(1),
            };
            s.ok(tx(vec![set(1, &[(f, v)])]), orch_on(&format!("lane/{l}")));
            s.ok(branch(&format!("{l}0"), &format!("lane/{l}")), orch());
        }
        // x ⊇ {L1, L2, L3} through a; y ⊇ {L1, L2, L3} through c.
        s.ok(branch("x", "lane/a0"), orch());
        s.ok(merge("lane/b0", "lane/x"), orch());
        s.ok(merge("lane/c0", "lane/x"), orch());
        s.ok(branch("y", "lane/c0"), orch());
        s.ok(merge("lane/b0", "lane/y"), orch());
        s.ok(merge("lane/a0", "lane/y"), orch());
        let r = s.ok(merge("lane/y", "lane/x"), orch());
        (s, r)
    };
    let (s, r) = build();
    let d = data(&r);
    assert_eq!(d.lca.len(), 3, "{:?}", d.lca);
    assert!(d.virtual_base);
    assert!(d.conflicts.is_empty());
    let st = state(&s, "lane/x");
    let x = &st.nodes[&Nid(1)];
    assert_eq!(x.fields.get("estimate"), Some(&Value::Int(5)));
    assert_eq!(x.fields.get("title"), Some(&Value::Text("renamed".into())));
    assert_eq!(x.fields.get("priority"), Some(&Value::Enum("P1".into())));
    ids_verify(&s);
    let (s2, r2) = build();
    assert_eq!(
        s.st.dag.commits[&r.commit.unwrap()].id,
        s2.st.dag.commits[&r2.commit.unwrap()].id,
        "VBC-12"
    );
    // VBC-7: C and D criss-cross A and B, which criss-cross L1 and L2: LCA(C, D) = {A, B} and the inner base of
    // (A, B) is itself a virtual base.
    let mut s = S::base();
    s.ok(tx(vec![task("k", "task k")]), orch());
    for l in ["a", "b"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(
        tx(vec![set(1, &[("estimate", P::Int(1))])]),
        orch_on("lane/a"),
    );
    s.ok(tx(vec![set(1, &[("title", t("t2"))])]), orch_on("lane/b"));
    s.ok(branch("a1", "lane/a"), orch());
    s.ok(branch("b1", "lane/b"), orch());
    s.ok(merge("lane/b1", "lane/a"), orch());
    s.ok(merge("lane/a1", "lane/b"), orch());
    s.ok(branch("a2", "lane/a"), orch());
    s.ok(branch("b2", "lane/b"), orch());
    s.ok(merge("lane/b2", "lane/a"), orch());
    s.ok(merge("lane/a2", "lane/b"), orch());
    let r = s.ok(merge("lane/b", "lane/a"), orch());
    let d = data(&r);
    assert_eq!(
        (d.lca.len(), d.virtual_base, d.outcome),
        (2, true, "landed")
    );
    // The inner base of the two LCAs is itself virtual: their common ancestors have two maximal elements.
    let (x, y) = (
        s.st.dag.ancestors(Some(d.lca[0])),
        s.st.dag.ancestors(Some(d.lca[1])),
    );
    let common: std::collections::BTreeSet<u64> = x.intersection(&y).copied().collect();
    assert_eq!(s.st.dag.maximal(&common).len(), 2);
    assert!(d.conflicts.is_empty());
    ids_verify(&s);
}

/// VBC-11: a `DeleteVsModify` in the virtual base, then both sides resolved it differently: RVB-4 with class
/// `DeleteVsModify` and RS-008's provisional state.
#[test]
fn vbc_11_delete_versus_modify_in_the_virtual_base() {
    let mut s = S::base();
    s.ok(tx(vec![task("k", "task k")]), orch());
    for l in ["a", "b"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(
        tx(vec![Stmt::Delete {
            target: Target::Id(Nid(1)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("gone".into()),
        }]),
        orch_on("lane/a"),
    );
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(0))])]),
        orch_on("lane/b"),
    );
    s.ok(branch("a1", "lane/a"), orch());
    s.ok(branch("b1", "lane/b"), orch());
    s.ok(merge("lane/b1", "lane/a"), orch());
    s.ok(merge("lane/a1", "lane/b"), orch());
    // lane/a keeps the deletion (ours), lane/b restores the modified node (ours).
    s.ok(resolve("#1.existence", Take::Ours), fresh_on("lane/a"));
    s.ok(resolve("#1.existence", Take::Ours), fresh_on("lane/b"));
    let r = s.ok(merge("lane/b", "lane/a"), orch());
    assert!(data(&r).virtual_base);
    assert_eq!(
        data(&r).conflicts,
        vec![("#1.existence".to_string(), "DeleteVsModify".to_string())]
    );
    let st = state(&s, "lane/a");
    let c = &st.nodes[&Nid(1)].conflicts[&Aspect::Existence];
    assert_eq!(
        c.prov,
        Some(crate::state::Side::Ours),
        "delete-wins: the deleting side, ours"
    );
    assert!(!st.nodes[&Nid(1)].live());
    ids_verify(&s);
}
