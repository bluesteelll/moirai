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
/// value (RVB-2, RVB-3), for both lanes. The virtual base orders its conflict value's sides by the LCAs' (gen, id), and
/// a lane's own conflict has its own value as `ours`; [F12 §5.4]'s ≈ equates the two orientations (spec sync 2b,
/// S2B-F-6; R36), so the untouched side is untouched whichever lane it is.
#[test]
fn vbc_3_one_side_untouched_takes_the_resolution() {
    // lane/a leaves its conflict (ours P0, theirs P4) and lane/b resolves to its P4: lane/b's value lands (RVB-2).
    let (s1, r1) = criss_cross(None, Some(Take::Ours));
    assert!(data(&r1).virtual_base);
    assert!(data(&r1).conflicts.is_empty(), "{:?}", data(&r1).conflicts);
    assert_eq!(field(&s1, "lane/a", 1, "priority"), prio("P4"));
    ids_verify(&s1);
    // lane/a resolves to its P0 and lane/b leaves its conflict (ours P4, theirs P0): lane/a's value stays (RVB-3).
    let (s2, r2) = criss_cross(Some(Take::Ours), None);
    assert!(data(&r2).virtual_base);
    assert!(data(&r2).conflicts.is_empty(), "{:?}", data(&r2).conflicts);
    assert_eq!(field(&s2, "lane/a", 1, "priority"), prio("P0"));
    ids_verify(&s2);
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
/// runs directly over the base of §4.3 and the tag keeps its tip (the WP-91 review's spec finding S4). The model runs
/// no `Tag` command ([API §11.6]), so the tag's row is written directly.
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

/// The Kleppmann step of commit `c` among its side's steps since `base` (RS-007; [`crate::dag::Dag::move_steps`]): each
/// moved node with its new parent; empty when `c` has no step key.
fn step_of(s: &S, c: u64, base: Option<u64>) -> Vec<(Nid, Option<Nid>)> {
    let dag = &s.st.dag;
    let x = &dag.commits[&c];
    dag.move_steps(&dag.ancestors(Some(c)), &dag.ancestors(base), &s.st.alloc)
        .into_iter()
        .find(|st| st.key == (x.hlc, x.id))
        .map(|st| {
            st.moves
                .iter()
                .map(|(n, v)| match v {
                    Some(KVal::Hierarchy { parent, .. }) => (*n, *parent),
                    _ => (*n, None),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The history of examples E2 and E3 of [RULES/merge-table] open point 35 case (v), the WP-91 closure's P7. lane/x puts
/// #2 under #1 (Y), then main puts #1 under #2 (X); `sync lane/x` stages `#1.parent HierarchyCycle`, which is resolved
/// to `ours` and continued (N: #1 at the root, #2 under #1; its first-parent changeset has no hierarchy entry). N's step
/// re-asserts only #1, the key main moved and the sync kept against it ([RULES/merge-table] open point 35 case (i) in
/// its narrow form, [AR §11] OQ-A-6 (a)). lane/x then moves #1 under #3 (Z).
fn synced_to_ours_then_moved() -> S {
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
    s.ok(mv(2, Some(1)), orch_on("lane/x"));
    s.ok(mv(1, Some(2)), orch());
    let r = s.run(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(violations(data(&r)), vec![("#1.parent", "HierarchyCycle")]);
    let g = "merge/lane/x/from/main";
    s.ok(resolve("#1.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(parents_12(&s, "lane/x"), (None, Some(Nid(1))));
    let sync = &s.st.dag.commits[&tip(&s, "lane/x").unwrap()];
    assert_eq!(sync.kind, "sync");
    assert!(
        !sync
            .changeset
            .keys()
            .any(|k| matches!(k, Key::Node(_, Aspect::Hierarchy))),
        "the sync kept its first parent's hierarchy"
    );
    assert_eq!(
        step_of(&s, sync.seq, tip(&s, "main")),
        vec![(Nid(1), None)],
        "the sync's step re-asserts #1, which main moved, and not #2"
    );
    s.ok(mv(1, Some(3)), fresh_on("lane/x"));
    s
}

/// Example E2 of [RULES/merge-table] open point 35 case (v) ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann
/// steps"; decided by [AR §11] OQ-A-11 11.1 (A)). On [`synced_to_ours_then_moved`]'s history, `merge lane/x --into
/// main`, with main unchanged since the sync, is one-sided: its base is X = tip(main), so every hierarchy key takes
/// lane/x's value with no replay, and the merge lands with #1 under #3 and #2 under #1. Before the decision it replayed
/// lane/x's commits from X, where Y's move (#2 under #1) closed a cycle with X's #1 under #2 and was undone, and since
/// N re-asserts only #1 the merge staged `#2.parent HierarchyCycle`, although lane/x holds #2 under #1 and main never
/// moved #2.
#[test]
fn a_sync_resolved_to_ours_then_merged_into_an_unmoved_main_lands_the_lane() {
    let mut s = synced_to_ours_then_moved();
    let lane = parents_12(&s, "lane/x");
    assert_eq!(lane, (Some(Nid(3)), Some(Nid(1))));
    let x = tip(&s, "main").unwrap();
    let r = s.run(merge("lane/x", "main"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert!(d.sync.is_none(), "main did not move since the sync");
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    assert_eq!(d.lca, vec![x], "the base is X = tip(main)");
    assert_eq!(parents_12(&s, "main"), lane);
    ids_verify(&s);
}

/// Example E3 of [RULES/merge-table] open point 35 case (v), the daily path "sync, resolve, keep working, sync"
/// ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann steps"; decided by [AR §11] OQ-A-11 11.1 (B)). On
/// [`synced_to_ours_then_moved`]'s history, main sets #3.priority (W, no hierarchy entry) and lane/x syncs again. The
/// sync is two-sided and its base is X, but Y does not descend from X, so the replay starts from the fork F, the
/// greatest commit every other commit of both sides descends from or precedes: Y puts #2 under #1, X's #1 under #2
/// closes a cycle and is undone, N's step puts #1 back at the root (a move that keeps its parent, never undone), Z puts
/// #1 under #3, and W moves nothing. The sync lands with lane/x's hierarchy. Before the decision it replayed from X, as
/// E2's merge did, and staged `#2.parent HierarchyCycle`.
#[test]
fn a_second_sync_after_a_sync_resolved_to_ours_lands_clean() {
    let mut s = synced_to_ours_then_moved();
    let x = tip(&s, "main").unwrap();
    let lane = parents_12(&s, "lane/x");
    s.ok(tx(vec![set(3, &[("priority", P::Int(1))])]), orch());
    let r = s.run(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        fresh_on("lane/x"),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert_eq!(d.lca, vec![x], "the base is X, main's first move");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    assert_eq!(parents_12(&s, "lane/x"), lane);
    assert_eq!(field(&s, "lane/x", 3, "priority"), prio("P1"));
    ids_verify(&s);
}

/// The WP-91 closure's P1, the example of [RULES/merge-table] open point 35 case (ii) ([RULES/merge-table] RS-007; [F12
/// §7.4] row "Kleppmann steps"; DM-003; decided by [AR §11] OQ-A-6 (a)). lane/a put #1 under #2 and then set
/// #4.priority (C); main put #2 under #1 and then #1 under #3. Replayed from b = state(p₁(C)), where #1 is under #2,
/// main's first move closed a cycle and the pick staged `#2.parent HierarchyCycle`, although C moved nothing. A
/// cherry-pick now replays from o, dst has no step and neither side has a (0, 0) step, and C's step is empty: the pick
/// lands main's hierarchy with C's priority and changes no hierarchy key.
#[test]
fn a_cherry_pick_of_a_commit_with_no_move_keeps_dsts_hierarchy() {
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
    s.ok(branch("a", "main"), orch());
    s.ok(mv(1, Some(2)), orch_on("lane/a"));
    let c = s
        .ok(
            tx(vec![set(4, &[("priority", P::Int(0))])]),
            orch_on("lane/a"),
        )
        .commit
        .expect("C");
    s.ok(mv(2, Some(1)), orch());
    s.ok(mv(1, Some(3)), orch());
    let before = tip(&s, "main");
    let r = s.run(
        Cmd::CherryPick {
            commit: format!("s{c}"),
            onto: Some("main".into()),
            message: String::new(),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    assert_eq!(parents_12(&s, "main"), (Some(Nid(3)), Some(Nid(1))));
    assert_eq!(field(&s, "main", 4, "priority"), prio("P0"));
    let m = &s.st.dag.commits[&tip(&s, "main").unwrap()];
    assert_eq!(
        (m.kind, m.parents.clone(), m.origin),
        ("cherry-pick", vec![before.unwrap()], Some(c))
    );
    assert!(
        !m.changeset
            .keys()
            .any(|k| matches!(k, Key::Node(_, Aspect::Hierarchy))),
        "the pick changes no hierarchy key"
    );
    ids_verify(&s);
}

/// RS-007's src step for a revert or a cherry-pick ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann steps"; spec
/// sync 2b S2B-F-98; [RULES/merge-table] open point 35 case (ii), decided by [AR §11] OQ-A-6 (a)): the replay starts
/// from o, dst has no step, and src has one step, the origin C's, keyed by C's (hlc, commit id), valued in src's state,
/// less each key a commit of A(o) after C moved.
/// - A cherry-pick of a lane commit that puts #1 under #2, over a main commit with no hierarchy entry, lands the move.
/// - A revert of main's commit that put #1 under #2 puts #1 back at the root, src's (state(p₁(C))'s) value, although
///   A(src) \ A(base) holds no commit.
/// - When a later main commit put #1 under #3, the revert's step leaves #1 out and main's later move stands (MR-040):
///   the revert lands with #1 under #3, no violation and no hierarchy change.
/// - A cherry-pick of a lane move of #1 that main moved after C leaves it out the same way: #1 stays under #3.
/// - A cherry-pick of a lane move of #1 under #2 onto a main that put #2 under #1 after C: C's step applies on o's
///   state, closes a cycle and is undone, so the pick stages `#1.parent HierarchyCycle`, the picked move's own key
///   (replayed from b, main's move applied after C's and staged `#2.parent` instead).
#[test]
fn a_revert_or_cherry_pick_of_a_move_replays_the_origins_step() {
    let three = || {
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
        s
    };
    let parent = |s: &S, n: u32| state(s, "main").nodes[&Nid(n)].parent;
    let revert = |c: u64| Cmd::Revert {
        commit: format!("s{c}"),
        onto: Some("main".into()),
        mainline: None,
        message: String::new(),
    };
    let landed = |r: &crate::api::Reply| {
        assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
        let d = data(r);
        assert_eq!(d.outcome, "landed");
        assert!(
            d.violations.is_empty() && d.conflicts.is_empty(),
            "{:?} {:?}",
            d.violations,
            d.conflicts
        );
    };
    // A cherry-pick of a lane move.
    let mut s = three();
    let c = s.ok(mv(1, Some(2)), orch_on("lane/x")).commit.unwrap();
    s.ok(tx(vec![set(3, &[("priority", P::Int(1))])]), orch());
    let r = s.run(
        Cmd::CherryPick {
            commit: format!("s{c}"),
            onto: Some("main".into()),
            message: String::new(),
        },
        orch(),
    );
    landed(&r);
    assert_eq!(parent(&s, 1), Some(Nid(2)), "C's move lands");
    assert_eq!(field(&s, "main", 3, "priority"), prio("P1"));
    ids_verify(&s);
    // A revert of main's move.
    let mut s = three();
    let c = s.ok(mv(1, Some(2)), orch()).commit.unwrap();
    s.ok(tx(vec![set(3, &[("priority", P::Int(1))])]), orch());
    let r = s.run(revert(c), orch());
    landed(&r);
    assert_eq!(parent(&s, 1), None, "the revert puts #1 back at the root");
    assert_eq!(field(&s, "main", 3, "priority"), prio("P1"));
    ids_verify(&s);
    // A revert of main's move after main moved #1 again: the later dst step wins by order.
    let mut s = three();
    let c = s.ok(mv(1, Some(2)), orch()).commit.unwrap();
    s.ok(mv(1, Some(3)), orch());
    let r = s.run(revert(c), orch());
    landed(&r);
    assert_eq!(parent(&s, 1), Some(Nid(3)), "main's later move decides");
    let rc = &s.st.dag.commits[&r.commit.expect("the revert commit")];
    assert_eq!((rc.kind, rc.origin), ("revert", Some(c)));
    assert!(
        !rc.changeset
            .keys()
            .any(|k| matches!(k, Key::Node(_, Aspect::Hierarchy))),
        "the revert changes no hierarchy key"
    );
    ids_verify(&s);
    let pick = |c: u64| Cmd::CherryPick {
        commit: format!("s{c}"),
        onto: Some("main".into()),
        message: String::new(),
    };
    // A cherry-pick of a lane move whose key main moved after C: main's later move stands.
    let mut s = three();
    let c = s.ok(mv(1, Some(2)), orch_on("lane/x")).commit.unwrap();
    s.ok(mv(1, Some(3)), orch());
    let r = s.run(pick(c), orch());
    landed(&r);
    assert_eq!(parent(&s, 1), Some(Nid(3)), "main's later move stands");
    ids_verify(&s);
    // A cherry-pick whose move closes a cycle on dst's state stages the picked key.
    let mut s = three();
    let c = s.ok(mv(1, Some(2)), orch_on("lane/x")).commit.unwrap();
    s.ok(mv(2, Some(1)), orch());
    let before = tip(&s, "main");
    let r = s.run(pick(c), orch());
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert!(d.conflicts.is_empty(), "{:?}", d.conflicts);
    assert_eq!(violations(d), vec![("#1.parent", "HierarchyCycle")]);
    let g = d.staging_ref.clone().expect("G");
    assert_eq!(
        parents_12(&s, &g),
        (None, Some(Nid(1))),
        "the staged candidate"
    );
    assert_eq!(tip(&s, "main"), before, "dst did not move");
    ids_verify(&s);
}

/// [RULES/merge-table] open point 35 (v), example E1: the default path into `main`, with no resolution
/// ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann steps"; decided by [AR §11] OQ-A-11 11.1 (A)). lane/x puts #2
/// under #3, then #1 under #2, then #2 back at the root; main then puts #3 under #1. `merge lane/x --into main` runs step
/// 0's sync, which replays the four moves from the fork and lands clean (#1 under #2, #3 under #1). Its step holds only
/// #3, the key main moved, which the sync took unchanged. The merge itself is one-sided, since its base is tip(main),
/// so every hierarchy key takes the synced lane's value with no replay, and it lands with lane/x's hierarchy. Before
/// the decision it replayed lane/x's moves from main's state, where #3 is under #1: #1 under #2 closed the cycle
/// #1 → #2 → #3 → #1 and was undone, and nothing re-asserted it, so the merge staged `#1.parent HierarchyCycle`,
/// although dst made no commit since the base and main never moved #1.
#[test]
fn a_clean_sync_first_merge_into_main_takes_the_lanes_hierarchy() {
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
    s.ok(mv(2, Some(3)), orch_on("lane/x"));
    s.ok(mv(1, Some(2)), orch_on("lane/x"));
    s.ok(mv(2, None), orch_on("lane/x"));
    s.ok(mv(3, Some(1)), orch());
    let r = s.run(merge("lane/x", "main"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    let sync = d.sync.as_ref().expect("step 0's sync");
    assert_eq!(sync.outcome, "landed");
    assert!(sync.violations.is_empty() && sync.conflicts.is_empty());
    let lane = state(&s, "lane/x");
    let want = vec![Some(Nid(2)), None, Some(Nid(1))];
    assert_eq!(
        (1..=3)
            .map(|n| lane.nodes[&Nid(n)].parent)
            .collect::<Vec<_>>(),
        want
    );
    let sc = tip(&s, "lane/x").unwrap();
    assert_eq!(
        step_of(&s, sc, s.st.dag.commits[&sc].parents.get(1).copied()),
        vec![(Nid(3), Some(Nid(1)))]
    );
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    let main = state(&s, "main");
    assert_eq!(
        (1..=3)
            .map(|n| main.nodes[&Nid(n)].parent)
            .collect::<Vec<_>>(),
        want
    );
    ids_verify(&s);
}

/// Example E4 of [RULES/merge-table] open point 35 case (v), a history whose commits all descend from the base
/// ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann steps"; decided by [AR §11] OQ-A-11 11.1 (A)). lane/b puts #1
/// under #2; lane/a, forked at the same commit, then puts #2 under #1 and later #1 under #3; `merge lane/b --into
/// lane/a` stages `#2.parent` (lane/a's move closed the cycle), which is resolved to `ours`, so the merge commit M keeps
/// lane/a's #2 under #1 and re-asserts only #1 against lane/b's move. Merging lane/a into an unmoved `main` is
/// one-sided (its base is tip(main)) and lands with lane/a's hierarchy. Before the decision it replayed lane/a's first
/// move after lane/b's, undid it again, and staged `#2.parent`; the replay start of 11.1 (B) alone does not cover E4,
/// since every commit here descends from the base and the replay is the one M already resolved.
#[test]
fn a_merge_resolved_to_ours_then_merged_into_an_unmoved_main_lands_the_lane() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
        ]),
        orch(),
    );
    let f = tip(&s, "main");
    for l in ["a", "b"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(mv(1, Some(2)), orch_on("lane/b"));
    s.ok(mv(2, Some(1)), orch_on("lane/a"));
    s.ok(mv(1, Some(3)), orch_on("lane/a"));
    let r = s.run(merge("lane/b", "lane/a"), fresh_on("lane/a"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(violations(data(&r)), vec![("#2.parent", "HierarchyCycle")]);
    let g = "merge/lane/a/from/lane/b";
    s.ok(resolve("#2.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("lane/b", "lane/a"), fresh_on("lane/a"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let m = tip(&s, "lane/a").unwrap();
    assert_eq!(s.st.dag.commits[&m].parents.len(), 2);
    let lane = parents_12(&s, "lane/a");
    assert_eq!(lane, (Some(Nid(3)), Some(Nid(1))));
    assert_eq!(
        step_of(&s, m, f),
        vec![(Nid(1), Some(Nid(3)))],
        "M re-asserts only #1, which lane/b moved"
    );
    let r = s.run(merge("lane/a", "main"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert!(d.sync.is_none(), "main did not move since the fork");
    assert_eq!(d.outcome, "landed");
    assert_eq!(d.lca, f.into_iter().collect::<Vec<_>>());
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    assert_eq!(parents_12(&s, "main"), lane);
    ids_verify(&s);
}

/// The case OQ-A-11 leaves open, E5 of [RULES/merge-table] open point 35 (v) ([RULES/merge-table] RS-007; [F12 §7.4]
/// row "Kleppmann steps"): E4's history on a two-sided merge. lane/b puts #1 under #2; lane/a, forked at the same commit
/// F, puts #2 under #1 and later #1 under #3; `merge lane/b --into lane/a` stages `#2.parent`, which is resolved to
/// `ours` (M keeps lane/a's #2 under #1 and re-asserts only #1). main then sets #3.priority, a commit with no hierarchy
/// entry, and lane/a syncs. The sync is not one-sided (lane/a made commits since the base F), and every commit
/// descends from F, so its replay starts from F and replays the cycle M resolved: lane/a's #2 under #1 is undone again
/// and nothing re-asserts it, so the sync stages `#2.parent HierarchyCycle`, although main never moved #2. The test
/// pins the result until a later decision covers it ((C) and (D) of OQ-A-11 were not taken).
#[test]
fn a_sync_after_a_merge_resolved_to_ours_stages_the_kept_move() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
        ]),
        orch(),
    );
    let f = tip(&s, "main");
    for l in ["a", "b"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(mv(1, Some(2)), orch_on("lane/b"));
    s.ok(mv(2, Some(1)), orch_on("lane/a"));
    s.ok(mv(1, Some(3)), orch_on("lane/a"));
    let r = s.run(merge("lane/b", "lane/a"), fresh_on("lane/a"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let g = "merge/lane/a/from/lane/b";
    s.ok(resolve("#2.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("lane/b", "lane/a"), fresh_on("lane/a"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(parents_12(&s, "lane/a"), (Some(Nid(3)), Some(Nid(1))));
    s.ok(tx(vec![set(3, &[("priority", P::Int(1))])]), orch());
    let r = s.run(
        Cmd::Sync {
            lane: Some("lane/a".into()),
            check: false,
        },
        fresh_on("lane/a"),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.lca, f.into_iter().collect::<Vec<_>>());
    assert!(d.conflicts.is_empty(), "{:?}", d.conflicts);
    assert_eq!(violations(d), vec![("#2.parent", "HierarchyCycle")]);
    ids_verify(&s);
}

/// E5 of [RULES/merge-table] open point 35 (v) on the path "sync, resolve to `ours`, sync", in the order where the
/// lane's own move is the later one (`wave-3c-arbiter.md` W3C-ARB-2; RS-007; [F12 §7.4] row "Kleppmann steps"). main
/// puts #2 under #1 (X); lane/x puts #1 under #2 (Y), then #2 under #3; `sync lane/x` replays X, then Y, which closes
/// #1 → #2 → #1 and is undone, and stages `#1.parent`; resolved to `ours`, the sync lands and its step re-asserts only
/// #2, the key main moved. Each later sync, after main makes a commit with no hierarchy entry, replays from the fork,
/// since Y does not descend from its base, undoes Y again, and stages `#1.parent HierarchyCycle`, although main never
/// moved #1. The test pins the result until a later decision covers it.
#[test]
fn a_sync_after_a_sync_that_kept_the_lanes_later_move_stages_it_on_every_sync() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
        ]),
        orch(),
    );
    let f = tip(&s, "main");
    s.ok(branch("x", "main"), orch());
    s.ok(mv(2, Some(1)), orch());
    s.ok(mv(1, Some(2)), orch_on("lane/x"));
    s.ok(mv(2, Some(3)), orch_on("lane/x"));
    let sync = |s: &mut S| {
        s.run(
            Cmd::Sync {
                lane: Some("lane/x".into()),
                check: false,
            },
            fresh_on("lane/x"),
        )
    };
    for round in 0..3 {
        if round > 0 {
            s.ok(
                tx(vec![set(3, &[("priority", P::Int(round))])]),
                fresh_on("main"),
            );
        }
        let r = sync(&mut s);
        assert_eq!(r.outcome, Outcome::Staged, "round {round}: {:?}", r.error);
        let d = data(&r);
        if round > 0 {
            assert_ne!(
                d.lca,
                f.into_iter().collect::<Vec<_>>(),
                "the base moves on"
            );
        }
        assert_eq!(
            violations(d),
            vec![("#1.parent", "HierarchyCycle")],
            "round {round}"
        );
        let g = "merge/lane/x/from/main";
        s.ok(resolve("#1.parent", Take::Ours), fresh_on(g));
        let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
        assert_eq!(r.outcome, Outcome::Ok, "round {round}: {:?}", r.error);
        assert_eq!(parents_12(&s, "lane/x"), (Some(Nid(2)), Some(Nid(3))));
    }
    ids_verify(&s);
}

/// E6 of [RULES/merge-table] open point 35 (v) (`wave-3c-arbiter.md` W3C-ARB-1; RS-007; [F12 §7.2], §7.4 row
/// "Kleppmann steps"): the replay from R stages a key that b, o and t all hold, where the replay from B landed. Tasks
/// #1 to #4; lane/y puts #2 under #3 (Y1); lane/x puts #1 under #3 (X1); main puts #4 under #2 (M1); lane/x puts #3
/// under #4 (X2); main puts #2 under #1 (M2); `merge lane/x --into lane/y` lands (MY); `sync lane/x` stages
/// `#2.parent`, which is resolved to `ours` (N). `merge lane/y --into lane/x` has the base X2, and b, o and t all hold
/// #3 under #4; Y1 does not descend from X2, so R is the fork, and the replay undoes X2's and MY's #3 under #4 (each
/// closes a cycle through #2, which main's moves put under #4's chain) while N puts #2 at the root: #3 ends at the root
/// and the merge stages `#3.parent HierarchyCycle`. The test pins the result until a later decision covers it.
#[test]
fn a_cross_lane_merge_after_a_resolved_sync_stages_a_key_all_three_states_hold() {
    let mut s = S::base();
    s.ok(
        tx((1..=4)
            .map(|i| task(&format!("t{i}"), &format!("task {i}")))
            .collect()),
        orch(),
    );
    let f = tip(&s, "main");
    for l in ["x", "y"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(mv(2, Some(3)), orch_on("lane/y"));
    s.ok(mv(1, Some(3)), orch_on("lane/x"));
    s.ok(mv(4, Some(2)), orch());
    let x2 = s.ok(mv(3, Some(4)), orch_on("lane/x")).commit.expect("X2");
    s.ok(mv(2, Some(1)), orch());
    let r = s.ok(merge("lane/x", "lane/y"), fresh_on("lane/y"));
    assert_eq!(data(&r).outcome, "landed");
    let r = s.run(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        fresh_on("lane/x"),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(violations(data(&r)), vec![("#2.parent", "HierarchyCycle")]);
    s.ok(
        resolve("#2.parent", Take::Ours),
        fresh_on("merge/lane/x/from/main"),
    );
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let three = |s: &S, r: &str| state(s, r).nodes[&Nid(3)].parent;
    assert_eq!(three(&s, "lane/x"), Some(Nid(4)));
    assert_eq!(three(&s, "lane/y"), Some(Nid(4)));
    let r = s.run(merge("lane/y", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.lca, vec![x2]);
    assert_eq!(
        s.st.dag.replay_start(
            &s.st.dag.ancestors(tip(&s, "lane/x")),
            &s.st.dag.ancestors(tip(&s, "lane/y")),
            &s.st.dag.ancestors(Some(x2)),
        ),
        f,
        "R is the fork"
    );
    assert_eq!(
        state(&s, "main").nodes[&Nid(3)].parent,
        None,
        "main never moved #3"
    );
    assert_eq!(violations(d), vec![("#3.parent", "HierarchyCycle")]);
    let g = d.staging_ref.clone().expect("G");
    assert_eq!(three(&s, &g), None, "a value none of b, o and t holds");
    ids_verify(&s);
}

/// A store with tasks #1–#4 on `main` and lanes `lane/x` and `lane/y` forked from it at one commit.
fn four_tasks_two_lanes() -> S {
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
    s.ok(branch("y", "main"), orch());
    assert_eq!(tip(&s, "lane/x"), tip(&s, "lane/y"));
    s
}

/// [RULES/merge-table] open point 35 case (i), its example ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann
/// steps"; decided by [AR §11] OQ-A-6 (a), narrow form). lane/x puts #2 under #1; lane/y, forked before main's move,
/// puts #2 under #4; main then puts #1 under #2; `sync lane/x` stages `#1.parent HierarchyCycle`, which is resolved to
/// `ours` (#1 at the root, #2 under #1). The sync's step re-asserts #1, the key main moved and the sync kept against
/// it, and not #2, which main never moved. `merge lane/x --into lane/y` replays lane/x's move, lane/y's move, main's
/// move and the sync's step: main's move of #1 under #2 closes no cycle (#2 is under #4) and applies, and the sync's
/// step then puts #1 back at the root. The merge lands #1 at the root and #2 under #4 (lane/y's later move, MR-040).
/// Before the decision the sync was no step, and the merge landed #1 under #2, which neither lane holds.
#[test]
fn a_syncs_step_reasserts_the_key_its_resolution_kept_against_main() {
    let mut s = four_tasks_two_lanes();
    s.ok(mv(2, Some(1)), orch_on("lane/x"));
    s.ok(mv(2, Some(4)), orch_on("lane/y"));
    s.ok(mv(1, Some(2)), orch());
    let r = s.run(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(violations(data(&r)), vec![("#1.parent", "HierarchyCycle")]);
    let g = "merge/lane/x/from/main";
    s.ok(resolve("#1.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(parents_12(&s, "lane/x"), (None, Some(Nid(1))));
    let sync = tip(&s, "lane/x").unwrap();
    assert_eq!(s.st.dag.commits[&sync].kind, "sync");
    assert_eq!(
        step_of(&s, sync, tip(&s, "main")),
        vec![(Nid(1), None)],
        "the sync's step re-asserts #1 only"
    );
    let r = s.run(merge("lane/x", "lane/y"), fresh_on("lane/y"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    assert_eq!(parents_12(&s, "lane/y"), (None, Some(Nid(4))));
    ids_verify(&s);
}

/// MR-040 under [RULES/merge-table] open point 35 case (i)'s narrow form ([RULES/merge-table] RS-007, MR-040; decided
/// by [AR §11] OQ-A-6 (a)): a two-parent commit re-asserts only the keys it kept against its second parent's branch, so
/// a side's own earlier move keeps its own (hlc, commit id) against a third branch. lane/x puts #2 under #1; lane/y, a
/// third branch, then puts #2 under #3; main then puts #3 under #4, and `sync lane/x` lands clean. The sync's state
/// differs from main's at #2, but main never moved #2, so the sync's step holds only #3 (the value it takes from main,
/// a no-op in the replay). `merge lane/x --into lane/y` lands with #2 under #3: lane/y's move, later than lane/x's,
/// still wins. The broad form would have re-asserted #2 under #1 at the sync's later (hlc, commit id) and beaten
/// lane/y's move.
#[test]
fn a_third_branchs_later_move_still_wins_over_a_synced_lanes_earlier_one() {
    let mut s = four_tasks_two_lanes();
    s.ok(mv(2, Some(1)), orch_on("lane/x"));
    s.ok(mv(2, Some(3)), orch_on("lane/y"));
    s.ok(mv(3, Some(4)), orch());
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
    let sync = tip(&s, "lane/x").unwrap();
    assert_eq!(s.st.dag.commits[&sync].kind, "sync");
    let lane = state(&s, "lane/x");
    assert_eq!(
        (lane.nodes[&Nid(2)].parent, lane.nodes[&Nid(3)].parent),
        (Some(Nid(1)), Some(Nid(4)))
    );
    assert_eq!(state(&s, "main").nodes[&Nid(2)].parent, None);
    assert_eq!(
        step_of(&s, sync, tip(&s, "main")),
        vec![(Nid(3), Some(Nid(4)))],
        "the sync's step holds #3 only, not #2"
    );
    let r = s.run(merge("lane/x", "lane/y"), fresh_on("lane/y"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    let y = state(&s, "lane/y");
    assert_eq!(
        (y.nodes[&Nid(2)].parent, y.nodes[&Nid(3)].parent),
        (Some(Nid(3)), Some(Nid(4))),
        "lane/y's later move of #2 wins"
    );
    ids_verify(&s);
}

/// RS-007's second-parent keys are limited to keys whose value in the two-parent commit's state differs from their value
/// in state(p₂) ([RULES/merge-table] RS-007, MR-040; [RULES/merge-table] open point 35 case (i), decided by [AR §11]
/// OQ-A-6 (a)). lane/x and main both put #2 under #3; lane/y, a third branch, then puts #2 under #4; `sync lane/x`
/// lands clean. main's move makes #2 a step key of a commit of A(p₂) \ A(p₁), but the sync holds #2 at main's value, so
/// its step leaves #2 out (it has no step at all). `merge lane/x --into lane/y` lands with #2 under #4: lane/y's move,
/// later than both moves to #3, wins. Without the filter the sync's step would re-key #2 under #3 to the sync's later
/// (hlc, commit id) and beat lane/y's move.
#[test]
fn a_syncs_step_leaves_out_a_key_it_holds_at_mains_value() {
    let mut s = four_tasks_two_lanes();
    s.ok(mv(2, Some(3)), orch_on("lane/x"));
    let m = s.ok(mv(2, Some(3)), orch()).commit.expect("main's move");
    s.ok(mv(2, Some(4)), orch_on("lane/y"));
    let r = s.ok(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    let sync = tip(&s, "lane/x").unwrap();
    let sc = &s.st.dag.commits[&sync];
    assert_eq!((sc.kind, sc.parents.len()), ("sync", 2));
    assert_eq!(sc.parents[1], m);
    for b in ["lane/x", "main"] {
        assert_eq!(state(&s, b).nodes[&Nid(2)].parent, Some(Nid(3)), "{b}");
    }
    assert!(
        s.st.dag.step_keys(m, &s.st.alloc).contains(&Nid(2)),
        "main's move is a step key of A(p₂) \\ A(p₁)"
    );
    assert!(
        !s.st.dag.step_keys(sync, &s.st.alloc).contains(&Nid(2)),
        "the sync holds #2 at main's value"
    );
    assert!(step_of(&s, sync, tip(&s, "main")).is_empty());
    let r = s.run(merge("lane/x", "lane/y"), fresh_on("lane/y"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    assert_eq!(
        state(&s, "lane/y").nodes[&Nid(2)].parent,
        Some(Nid(4)),
        "lane/y's later move of #2 wins"
    );
    ids_verify(&s);
}

/// RS-007's src step for a cherry-pick of C leaves out exactly the keys that are a step key of a commit of A(o) ordered
/// after C by (hlc, commit id) ([RULES/merge-table] RS-007, MR-040; [RULES/merge-table] open point 35 case (ii),
/// decided by [AR §11] OQ-A-6 (a)).
/// - dst moved the picked key before C: main puts #1 under #3, then lane/x puts #1 under #2 (C). Picking C onto main
///   replays C's move from o and lands #1 under #2, C's later move (MR-040). A reading that left out every key dst moved
///   since the fork would keep #1 under #3.
/// - dst's later commit holds the key only as a second-parent step key: main puts #1 under #3; lane/x then puts #1 under
///   #4 (X); lane/y puts #1 under #2 (C); `sync lane/x` then lands clean with #1 under #4 (X is later than main's move),
///   so the sync's net changeset has no hierarchy entry and #1 is its step key only because main moved #1 and the sync
///   holds another value. Picking C onto lane/x leaves #1 out of C's step: the sync's step, after C, stands, as it does
///   in `merge lane/y --into lane/x`, and #1 stays under #4.
#[test]
fn a_cherry_pick_leaves_out_only_keys_a_later_dst_step_sets() {
    let pick = |c: u64, onto: &str| Cmd::CherryPick {
        commit: format!("s{c}"),
        onto: Some(onto.into()),
        message: String::new(),
    };
    let landed = |r: &crate::api::Reply| {
        assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
        let d = data(r);
        assert_eq!(d.outcome, "landed");
        assert!(
            d.violations.is_empty() && d.conflicts.is_empty(),
            "{:?} {:?}",
            d.violations,
            d.conflicts
        );
    };
    let parent = |s: &S, r: &str| state(s, r).nodes[&Nid(1)].parent;
    // dst moved the key before C.
    let mut s = four_tasks_two_lanes();
    s.ok(mv(1, Some(3)), orch());
    let c = s.ok(mv(1, Some(2)), orch_on("lane/x")).commit.expect("C");
    let r = s.run(pick(c, "main"), fresh_on("main"));
    landed(&r);
    assert_eq!(parent(&s, "main"), Some(Nid(2)), "C's later move lands");
    ids_verify(&s);
    // dst's later commit holds the key only as a second-parent step key.
    let mut s = four_tasks_two_lanes();
    s.ok(mv(1, Some(3)), orch());
    s.ok(mv(1, Some(4)), orch_on("lane/x"));
    let c = s.ok(mv(1, Some(2)), orch_on("lane/y")).commit.expect("C");
    let r = s.ok(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    assert_eq!(data(&r).outcome, "landed");
    assert!(data(&r).violations.is_empty(), "{:?}", data(&r).violations);
    let sync = tip(&s, "lane/x").unwrap();
    let sc = &s.st.dag.commits[&sync];
    assert_eq!((sc.kind, sc.parents.len()), ("sync", 2));
    assert!(
        !sc.changeset
            .keys()
            .any(|k| matches!(k, Key::Node(_, Aspect::Hierarchy))),
        "the sync kept lane/x's hierarchy"
    );
    assert_eq!(
        step_of(&s, sync, tip(&s, "main")),
        vec![(Nid(1), Some(Nid(4)))],
        "#1 is a second-parent step key of the sync"
    );
    let r = s.run(pick(c, "lane/x"), fresh_on("lane/x"));
    landed(&r);
    assert_eq!(parent(&s, "lane/x"), Some(Nid(4)), "the sync's step stands");
    let pc = &s.st.dag.commits[&tip(&s, "lane/x").unwrap()];
    assert_eq!((pc.kind, pc.origin), ("cherry-pick", Some(c)));
    assert!(
        !pc.changeset
            .keys()
            .any(|k| matches!(k, Key::Node(_, Aspect::Hierarchy))),
        "the pick changes no hierarchy key"
    );
    let r = s.run(merge("lane/y", "lane/x"), fresh_on("lane/x"));
    landed(&r);
    assert_eq!(parent(&s, "lane/x"), Some(Nid(4)), "the merge agrees");
    ids_verify(&s);
}

/// [RULES/merge-table] open point 35 case (iii) through the Store API ([RULES/merge-table] RS-007, MR-039, CS-013;
/// decided by [AR §11] OQ-A-6 (a)): a move that sets its node's current value is never undone. Of #1 and #2, `lo` is
/// the node with the lower uid and `hi` the other. lane/x, in one commit, puts #3 under `hi` and `lo` under #3; lane/y,
/// in a later commit, puts `lo` under #3 too and `hi` under `lo`. `merge lane/y --into lane/x` replays lane/y's step
/// after lane/x's: `lo`'s move changes nothing and `hi`'s closes the cycle `hi` → `lo` → #3 → `hi`, so only `hi`'s move
/// is undone and only `hi.parent` stages, although `lo` has the least uid on the cycle. After `merge --abort`, lane/x
/// puts `hi` under #4 in a still later commit, and the merge lands with nothing staged: `lo` under #3, #3 under `hi`,
/// `hi` under #4.
#[test]
fn a_no_op_move_in_a_cycle_closing_step_is_never_undone() {
    let mut s = four_tasks_two_lanes();
    let uid = |s: &S, n: u32| s.st.alloc.uids[&Nid(n)];
    let (lo, hi) = if uid(&s, 1) < uid(&s, 2) {
        (1, 2)
    } else {
        (2, 1)
    };
    let mv2 = |a: (u32, u32), b: (u32, u32)| {
        tx(vec![
            Stmt::Move {
                target: Target::Id(Nid(a.0)),
                under: Some(Target::Id(Nid(a.1))),
                position: None,
            },
            Stmt::Move {
                target: Target::Id(Nid(b.0)),
                under: Some(Target::Id(Nid(b.1))),
                position: None,
            },
        ])
    };
    s.ok(mv2((3, hi), (lo, 3)), orch_on("lane/x"));
    s.ok(mv2((lo, 3), (hi, lo)), orch_on("lane/y"));
    let r = s.run(merge("lane/y", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert!(d.conflicts.is_empty(), "{:?}", d.conflicts);
    let hi_key = format!("#{hi}.parent");
    assert_eq!(violations(d), vec![(hi_key.as_str(), "HierarchyCycle")]);
    let g = d.staging_ref.clone().expect("G");
    let st = state(&s, &g);
    let parent = |n: u32| st.nodes[&Nid(n)].parent;
    assert_eq!(
        (parent(lo), parent(hi), parent(3)),
        (Some(Nid(3)), None, Some(Nid(hi))),
        "the staged candidate"
    );
    s.ok(
        Cmd::MergeAbort {
            src: Some("lane/y".into()),
            into: Some("lane/x".into()),
        },
        fresh_on("lane/x"),
    );
    s.ok(mv(hi, Some(4)), fresh_on("lane/x"));
    let r = s.run(merge("lane/y", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    let st = state(&s, "lane/x");
    let parent = |n: u32| st.nodes[&Nid(n)].parent;
    assert_eq!(
        (parent(lo), parent(hi), parent(3)),
        (Some(Nid(3)), Some(Nid(4)), Some(Nid(hi)))
    );
    ids_verify(&s);
}

/// [RULES/merge-table] RS-007's step keys of a two-parent commit, "moved" read as "is a step key of", recursively
/// ([RULES/merge-table] open point 35 case (i); decided by [AR §11] OQ-A-11 11.3 (a)): a merge inside the merged branch
/// counts with its own second-parent keys. Lanes a, b and c fork from main at F. lane/c puts #3 under #1 (C₁); lane/b
/// then puts #1 under #3 (Z); lane/a sets #4.priority and merges lane/b, taking Z's move. `merge lane/b --into lane/c`
/// replays C₁ and then Z, which closes a cycle; the key `#1.parent` is resolved to `ours`, so the merge M_z keeps #1 at
/// the root against Z's move, and #1 is one of M_z's second-parent keys. lane/a then puts #1 under #2. `merge lane/c
/// --into lane/a` has the base Z, which C₁ does not descend from, so its replay starts from F (11.1 (B)): C₁ applies,
/// Z's move and lane/a's merge of it close a cycle and are undone, M_z's step keeps #1 at the root (a move that keeps
/// its parent), and lane/a's #1 under #2 applies, so the merge lands clean with #1 under #2 and #3 under #1. Replayed
/// from Z, as before the decision, C₁ was undone on a state it was not made on and the merge staged `#3.parent`. Its
/// commit M keeps lane/a's #1 under #2 against M_z's value. The commits of A(p₂) \ A(p₁) are C₁ and M_z: C₁ moved only
/// #3, so #1 is a step key of M only because it is one of M_z's: M's step holds #1 under #2 and #3 under #1.
#[test]
fn a_merge_inside_the_merged_branch_counts_with_its_second_parent_keys() {
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
    for l in ["a", "b", "c"] {
        s.ok(branch(l, "main"), orch());
    }
    s.ok(mv(3, Some(1)), orch_on("lane/c"));
    let z = s.ok(mv(1, Some(3)), orch_on("lane/b")).commit.expect("Z");
    s.ok(
        tx(vec![set(4, &[("priority", P::Int(0))])]),
        orch_on("lane/a"),
    );
    let r = s.ok(merge("lane/b", "lane/a"), fresh_on("lane/a"));
    assert_eq!(data(&r).outcome, "landed");
    assert_eq!(parents_12(&s, "lane/a"), (Some(Nid(3)), None));
    let r = s.run(merge("lane/b", "lane/c"), fresh_on("lane/c"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(violations(data(&r)), vec![("#1.parent", "HierarchyCycle")]);
    let g = "merge/lane/c/from/lane/b";
    s.ok(resolve("#1.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("lane/b", "lane/c"), fresh_on("lane/c"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let mz = tip(&s, "lane/c").unwrap();
    assert_eq!(s.st.dag.commits[&mz].parents.len(), 2);
    assert_eq!(state(&s, "lane/c").nodes[&Nid(3)].parent, Some(Nid(1)));
    assert_eq!(state(&s, "lane/c").nodes[&Nid(1)].parent, None);
    assert_eq!(
        step_of(&s, mz, Some(z)),
        vec![(Nid(1), None)],
        "M_z re-asserts #1 against Z's move"
    );
    s.ok(mv(1, Some(2)), fresh_on("lane/a"));
    let r = s.run(merge("lane/c", "lane/a"), fresh_on("lane/a"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.lca, vec![z]);
    assert_eq!(d.outcome, "landed");
    assert!(
        d.violations.is_empty() && d.conflicts.is_empty(),
        "{:?} {:?}",
        d.violations,
        d.conflicts
    );
    let m = tip(&s, "lane/a").unwrap();
    assert_eq!(s.st.dag.commits[&m].parents[1], mz);
    let a = state(&s, "lane/a");
    assert_eq!(
        (a.nodes[&Nid(1)].parent, a.nodes[&Nid(3)].parent),
        (Some(Nid(2)), Some(Nid(1)))
    );
    assert_eq!(
        step_of(&s, m, Some(mz)),
        vec![(Nid(1), Some(Nid(2))), (Nid(3), Some(Nid(1)))],
        "#1 is a step key of M through M_z's second-parent keys"
    );
    ids_verify(&s);
}

/// The WP-91 closure's P2 ([F06 §4.4.16]; [F12 §9.4] step 1): a `--strict` merge staged by a `FieldEdit` alone records
/// `strict` in its `stage` group, and `merge --continue`, under `merge.strict` false, recomputes with it: nothing
/// resolved re-stages, and the re-staged commit copies the group; resolved, it lands. A revert or cherry-pick records
/// no group: `merge.strict` does not govern them, and a `DATA` case lands a conflict value.
#[test]
fn a_strict_merge_continues_with_its_recorded_strict() {
    use crate::dag::Stage;
    let mut s = two_tasks();
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(3))])]),
        orch_on("lane/x"),
    );
    let r = s.run(strict_merge("main", "lane/x"), orch());
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let g = "merge/lane/x/from/main";
    let want = Some(Stage {
        base: None,
        policy: None,
        strict: true,
    });
    assert_eq!(s.st.dag.commits[&tip(&s, g).unwrap()].stage, want);
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(
        data(&r).conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    assert_eq!(
        s.st.dag.commits[&tip(&s, g).unwrap()].stage,
        want,
        "a re-stage copies the group"
    );
    s.ok(resolve("#1.priority", Take::Theirs), fresh_on(g));
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(field(&s, "lane/x", 1, "priority"), prio("P1"));
    ids_verify(&s);
    // A cherry-pick under `merge.strict` true lands its `DATA` case as a conflict value.
    let mut s = two_tasks();
    s.ok(
        Cmd::ConfigSet {
            key: "merge.strict".into(),
            value: "true".into(),
            scope: crate::confcmd::FileScope::Store,
        },
        Ctx::default(),
    );
    let c = s
        .ok(
            tx(vec![set(1, &[("priority", P::Int(3))])]),
            orch_on("lane/x"),
        )
        .commit
        .unwrap();
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    let r = s.run(
        Cmd::CherryPick {
            commit: format!("s{c}"),
            onto: Some("main".into()),
            message: String::new(),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(
        data(&r).conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    ids_verify(&s);
}

/// The WP-91 closure's P4b ([F06 §4.4.16]; [F12 §9.4] step 1): `merge main --into lane/x --base s2` stages with a
/// `FieldEdit` on #1.priority (over s2: P1, P2, P4) and a `HierarchyCycle` on #3.parent. With #3.parent resolved, the
/// continue recomputes over the recorded base, so the `FieldEdit` lands as a conflict value; over the LCA (P2 on both
/// base and dst) main's P4 would land silently.
#[test]
fn a_based_merge_continues_over_its_recorded_base() {
    use crate::dag::Stage;
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
        ]),
        orch(),
    );
    let s2 = s
        .ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch())
        .commit
        .unwrap();
    s.ok(tx(vec![set(1, &[("priority", P::Int(2))])]), orch());
    s.ok(branch("x", "main"), orch());
    s.ok(mv(2, Some(3)), orch_on("lane/x"));
    s.ok(tx(vec![set(1, &[("priority", P::Int(4))])]), orch());
    s.ok(mv(3, Some(2)), orch());
    let r = s.run(
        Cmd::Merge {
            src: "main".into(),
            into: Some("lane/x".into()),
            policy: None,
            strict: None,
            base: Some(format!("s{s2}")),
            message: String::new(),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(
        d.conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    assert_eq!(violations(d), vec![("#3.parent", "HierarchyCycle")]);
    let g = "merge/lane/x/from/main";
    assert_eq!(
        s.st.dag.commits[&tip(&s, g).unwrap()].stage,
        Some(Stage {
            base: Some(s2),
            policy: None,
            strict: false,
        })
    );
    s.ok(resolve("#3.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert_eq!(
        d.conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    assert!(matches!(
        field(&s, "lane/x", 1, "priority"),
        KState::Conflict(_)
    ));
    ids_verify(&s);
}

/// Deletes node #n with a reason.
fn delete(n: u32) -> Cmd {
    tx(vec![Stmt::Delete {
        target: Target::Id(Nid(n)),
        policy: None,
        replaced_by: None,
        release: false,
        reason: Some("gone".into()),
    }])
}

/// [F06 §4.4.16]; [F12 §9.4] step 1 (spec sync 2b, S2B-F-1, S2B-M-3): a `--policy resurrect` merge records the override
/// in its `stage` group, and `merge --continue` recomputes with it, whatever dst's configuration says by then. lane/x
/// edits #1 and puts #2 under #3; main deletes #1 and puts #3 under #2. `merge main --into lane/x --policy resurrect`
/// stages `#3.parent HierarchyCycle`, and its `DeleteVsModify` on #1.existence lands provisionally on the modifying
/// side (AP-005: #1 live), where task's EP-001 `delete-wins` would delete it. lane/x then writes `merge.policy.task =
/// delete-wins` (AP-004 for every task merged into it). With #3.parent resolved, the continue keeps the recorded
/// override (`--policy` over `merge.policy.<kind>`, [CFG §10.13]): #1 stays live, provisional on ours.
#[test]
fn a_policy_merge_continues_with_its_recorded_override() {
    use crate::dag::Stage;
    use crate::state::Side;
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
    s.ok(delete(1), orch());
    s.ok(mv(3, Some(2)), orch());
    let r = s.run(
        Cmd::Merge {
            src: "main".into(),
            into: Some("lane/x".into()),
            policy: Some("resurrect".into()),
            strict: None,
            base: None,
            message: String::new(),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(violations(d), vec![("#3.parent", "HierarchyCycle")]);
    let dvm = vec![("#1.existence".to_string(), "DeleteVsModify".to_string())];
    assert_eq!(d.conflicts, dvm);
    let g = "merge/lane/x/from/main";
    assert_eq!(
        s.st.dag.commits[&tip(&s, g).unwrap()].stage,
        Some(Stage {
            base: None,
            policy: Some("resurrect".into()),
            strict: false,
        })
    );
    let provisional = |s: &S, r: &str| {
        let st = state(s, r);
        let x = &st.nodes[&Nid(1)];
        (x.live(), x.conflicts[&Aspect::Existence].prov)
    };
    assert_eq!(provisional(&s, g), (true, Some(Side::Ours)));
    s.ok(
        policy_cmd("merge.policy.task", Some("delete-wins")),
        fresh_on("lane/x"),
    );
    s.ok(resolve("#3.parent", Take::Ours), fresh_on(g));
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.violations.is_empty() && d.notices.is_empty(), "{d:?}");
    assert_eq!(d.conflicts, dvm);
    assert_eq!(provisional(&s, "lane/x"), (true, Some(Side::Ours)));
    assert_eq!(
        state(&s, "lane/x").schema.policy("merge.policy.task"),
        Some("delete-wins")
    );
    assert!(s.st.dag.live(g).is_none(), "G is deleted");
    ids_verify(&s);
}

/// The WP-91 closure's P3 ([F12 §6.5] "On a violation's key"; [F07 §6.4]): a `resolve` never writes a value key onto
/// a tombstone. A cherry-pick of a commit that set #1.priority, onto main after main deleted #1, stages `NotFound` on
/// #1.priority; `theirs` (P0) and `value` are `not_found` as a `SET` on the node is, and `base` (absent) is taken. A
/// revert of main's own priority commit after the delete refuses `base` (its P0) the same way.
#[test]
fn a_resolve_puts_no_value_on_a_tombstone() {
    let del = || {
        tx(vec![Stmt::Delete {
            target: Target::Id(Nid(1)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("gone".into()),
        }])
    };
    let mut s = two_tasks();
    let c = s
        .ok(
            tx(vec![set(1, &[("priority", P::Int(0))])]),
            orch_on("lane/x"),
        )
        .commit
        .unwrap();
    s.ok(del(), orch());
    let r = s.run(
        Cmd::CherryPick {
            commit: format!("s{c}"),
            onto: Some("main".into()),
            message: String::new(),
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert!(
        violations(data(&r)).contains(&("#1.priority", "NotFound")),
        "{:?}",
        data(&r).violations
    );
    let g = data(&r).staging_ref.clone().unwrap();
    s.refused(
        resolve("#1.priority", Take::Theirs),
        fresh_on(&g),
        "not_found",
    );
    s.refused(
        resolve("#1.priority", Take::Value(P::Int(2))),
        fresh_on(&g),
        "not_found",
    );
    s.ok(resolve("#1.priority", Take::Base), fresh_on(&g));
    assert_eq!(field(&s, &g, 1, "priority"), KState::ABSENT);
    ids_verify(&s);
    // A revert: its base is the reverted commit's state.
    let mut s = two_tasks();
    let c = s
        .ok(tx(vec![set(1, &[("priority", P::Int(0))])]), orch())
        .commit
        .unwrap();
    s.ok(del(), orch());
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
    let g = data(&r).staging_ref.clone().unwrap();
    s.refused(
        resolve("#1.priority", Take::Base),
        fresh_on(&g),
        "not_found",
    );
    ids_verify(&s);
}

/// [F12 §6.5] "On a violation's key", the choices besides `ours`: a `HierarchyCycle` key takes `base` (the fork's
/// value, #3 at the root) and the merge lands; a `PlanMask` field key takes a `value` checked against its type, which
/// the continue re-validates; a `repoint` on G that would close a precedence cycle is E405 (VO-3: the view before it
/// had no cycle).
#[test]
fn violation_keys_take_base_value_and_a_checked_repoint() {
    // base on a HierarchyCycle key.
    let mut s = staged_with_a_cycle();
    let g = "merge/lane/x/from/main";
    s.ok(resolve("#1.priority", Take::Theirs), fresh_on(g));
    s.ok(resolve("#3.parent", Take::Base), fresh_on(g));
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(data(&r).outcome, "landed");
    let st = state(&s, "lane/x");
    assert_eq!(
        (st.nodes[&Nid(2)].parent, st.nodes[&Nid(3)].parent),
        (Some(Nid(3)), None)
    );
    ids_verify(&s);
    // value on a PlanMask key: plan/p holds assignee ann, main sets bob.
    let mut s = S::base();
    s.ok(
        tx(vec![node(
            "a",
            "task",
            &[("title", t("task a")), ("assignee", t("ann"))],
        )]),
        orch(),
    );
    s.ok(
        Cmd::BranchCreate {
            name: "plan/p".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    s.ok(tx(vec![set(1, &[("assignee", t("bob"))])]), orch());
    let r = s.run(merge("main", "plan/p"), orch());
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(violations(data(&r)), vec![("#1.assignee", "PlanMask")]);
    let g = "merge/plan/p/from/main";
    s.refused(
        resolve("#1.assignee", Take::Value(P::Int(3))),
        fresh_on(g),
        "E103",
    );
    s.ok(resolve("#1.assignee", Take::Value(t("ann"))), fresh_on(g));
    let r = s.run(continue_("main", "plan/p"), fresh_on("plan/p"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(data(&r).outcome, "landed");
    ids_verify(&s);
    // repoint closing a precedence cycle: #1 blocks #2, and #2's dangling edge to the deleted #4 re-pointed to #1.
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
    s.ok(
        tx(vec![Stmt::Delete {
            target: Target::Id(Nid(4)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("gone".into()),
        }]),
        orch_on("lane/x"),
    );
    s.ok(tx(vec![link(2, "blocks", 4), link(1, "blocks", 2)]), orch());
    let r = s.run(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        orch(),
    );
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(
        violations(data(&r)),
        vec![("edge:#2:blocks:#4", "DanglingEdge")]
    );
    let g = "merge/lane/x/from/main";
    s.refused(
        resolve("edge:#2:blocks:#4", Take::Repoint(Target::Id(Nid(1)))),
        fresh_on(g),
        "E405",
    );
    s.ok(
        resolve("edge:#2:blocks:#4", Take::Repoint(Target::Id(Nid(3)))),
        fresh_on(g),
    );
    // Anything but a `Resolve` is refused on G ([F12 §9.3]).
    s.refused(mv(3, Some(1)), fresh_on(g), "E305");
    ids_verify(&s);
}

/// VO-3 on a re-staged view that holds a cycle ([F13 §5] VO-3, third bullet; [F12 §9.4] steps 2–3). lane/x puts #1
/// under #2, main then puts #2 under #1, and lane/x moves #1 back to the root: `merge main --into lane/x` replays
/// lane/x's first move, undoes main's and stages `#2.parent HierarchyCycle` with both nodes at the root. On G, `theirs`
/// (#2 under #1) is valid; then lane/x moves #1 under #2. The resolution is not stale (lane/x never changed #2.parent),
/// so the continue overlays it onto a candidate with #1 under #2, which closes #1 ↔ #2: V03 and V05 re-stage, keyed by
/// the least uid on the cycle. On the re-staged G a `RESOLVE` of that key that leaves the cycle as it was (the side
/// whose value the view holds: `ours` for #1.parent, `theirs` for #2.parent) makes no node its own ancestor and is
/// taken, while a `MOVE` stays E305; the other side (the root) breaks the cycle, and the next continue lands.
#[test]
fn a_restaged_cycle_takes_a_resolve_that_makes_no_new_one() {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "task a"), task("b", "task b")]), orch());
    s.ok(branch("x", "main"), orch());
    s.ok(mv(1, Some(2)), orch_on("lane/x"));
    s.ok(mv(2, Some(1)), orch());
    s.ok(mv(1, None), orch_on("lane/x"));
    let r = s.run(merge("main", "lane/x"), orch());
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    assert_eq!(violations(data(&r)), vec![("#2.parent", "HierarchyCycle")]);
    let g = "merge/lane/x/from/main";
    assert_eq!(parents_12(&s, g), (None, None), "the staged candidate");
    s.ok(resolve("#2.parent", Take::Theirs), fresh_on(g));
    assert_eq!(parents_12(&s, g), (None, Some(Nid(1))));
    s.ok(mv(1, Some(2)), fresh_on("lane/x"));
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    assert!(d.notices.is_empty(), "{:?}", d.notices);
    // (the key V05 names, the side the view holds, the side that breaks the cycle, the landed parents of #1 and #2)
    let (least, keep, break_, landed) = if s.st.alloc.uids[&Nid(1)] < s.st.alloc.uids[&Nid(2)] {
        ("#1.parent", Take::Ours, Take::Theirs, (None, Some(Nid(1))))
    } else {
        ("#2.parent", Take::Theirs, Take::Ours, (Some(Nid(2)), None))
    };
    // V03 reports the cycle too, child → parent edges being precedence edges (I5′), its witness the least edge.
    assert_eq!(
        violations(d),
        vec![(least, "Cycle"), (least, "HierarchyCycle")]
    );
    assert_eq!(
        parents_12(&s, g),
        (Some(Nid(2)), Some(Nid(1))),
        "the re-staged candidate holds the cycle"
    );
    let r = s.ok(resolve(least, keep), fresh_on(g));
    let rc = &s.st.dag.commits[&r.commit.expect("the resolve commit")];
    assert!(rc.changeset.is_empty(), "the value stays as it was");
    s.refused(mv(1, None), fresh_on(g), "E305");
    s.ok(resolve(least, break_), fresh_on(g));
    assert_eq!(parents_12(&s, g), landed);
    let r = s.run(continue_("main", "lane/x"), fresh_on("lane/x"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.violations.is_empty(), "{:?}", d.violations);
    assert_eq!(parents_12(&s, "lane/x"), landed);
    assert!(s.st.dag.live(g).is_none(), "G is deleted");
    ids_verify(&s);
}

/// [API §11.7] "Sync first" (spec sync 2b, S2B-F-1, S2B-F-12): step 0 runs with the merge's `policy` and `strict`,
/// never its `base`, and a staged step-0 sync records the override and `strict` but no base. When the sync stages no
/// merge is computed: the top-level `conflicts`, `violations`, `markers` and `lca` are empty, `virtual_base` is false,
/// `staging_ref` names the sync's staging ref, and the staged items are reported once, in `sync`.
#[test]
fn a_merge_staged_by_its_sync_reports_the_sync_once() {
    use crate::dag::Stage;
    let mut s = two_tasks();
    let first = tip(&s, "main").unwrap();
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(3))])]),
        orch_on("lane/x"),
    );
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    let r = s.run(
        Cmd::Merge {
            src: "lane/x".into(),
            into: Some("main".into()),
            policy: Some("delete-wins".into()),
            strict: Some(true),
            base: Some(format!("s{first}")),
            message: String::new(),
        },
        orch(),
    );
    assert_eq!((r.outcome, r.exit), (Outcome::Staged, 6), "{:?}", r.error);
    let g = "merge/lane/x/from/main";
    let d = data(&r);
    assert_eq!(d.outcome, "staged");
    assert_eq!(d.staging_ref.as_deref(), Some(g));
    assert!(d.conflicts.is_empty() && d.violations.is_empty() && d.markers.is_empty());
    assert!(d.lca.is_empty() && !d.virtual_base);
    let sync = d.sync.as_ref().expect("the step-0 sync");
    assert_eq!(sync.outcome, "staged");
    assert_eq!(
        sync.conflicts,
        vec![("#1.priority".to_string(), "FieldEdit".to_string())]
    );
    let e = r.error.as_ref().unwrap();
    assert_eq!(e.code, "staged");
    assert!(
        e.detail.starts_with(
            "sync of lane/x staged on merge/lane/x/from/main: 0 violations, 1 conflicts"
        ),
        "{}",
        e.detail
    );
    let c = &s.st.dag.commits[&tip(&s, g).unwrap()];
    assert_eq!(c.kind, "sync");
    assert_eq!(
        c.stage,
        Some(Stage {
            base: None,
            policy: Some("delete-wins".into()),
            strict: true,
        }),
        "the merge's override and strict, never its base"
    );
    ids_verify(&s);
}

/// [API §11.7] "Sync first" (spec sync 2b, S2B-F-1): step 0 applies the merge's `--policy`, not only records it. lane/x
/// edits #1 while main deletes it, and `merge lane/x --into main` syncs lane/x first. With `--policy resurrect` the
/// sync's `DeleteVsModify` on #1.existence is provisional on the modifying side, lane/x's (AP-005): #1 stays live on
/// the lane. Without it, task's EP-001 `delete-wins` makes it provisional on main's deleting side (RS-008). Either sync
/// lands its conflict value, so the merge is refused `conflicted_src` (S2B-F-11) and main does not move.
#[test]
fn a_sync_first_step_applies_the_merges_policy() {
    use crate::state::Side;
    for (policy, live, prov) in [
        (Some("resurrect"), true, Side::Ours),
        (None, false, Side::Theirs),
    ] {
        let mut s = two_tasks();
        s.ok(
            tx(vec![set(1, &[("priority", P::Int(3))])]),
            orch_on("lane/x"),
        );
        s.ok(delete(1), orch());
        let main_before = tip(&s, "main");
        let r = s.run(
            Cmd::Merge {
                src: "lane/x".into(),
                into: Some("main".into()),
                policy: policy.map(str::to_string),
                strict: None,
                base: None,
                message: String::new(),
            },
            orch(),
        );
        assert_eq!(r.exit, 6, "{policy:?}: {:?}", r.error);
        assert_eq!(r.error.as_ref().unwrap().code, "conflicted_src");
        assert_eq!(tip(&s, "main"), main_before, "main did not move");
        let sync = &s.st.dag.commits[&tip(&s, "lane/x").unwrap()];
        assert_eq!(sync.kind, "sync");
        let st = state(&s, "lane/x");
        let x = &st.nodes[&Nid(1)];
        let c = &x.conflicts[&Aspect::Existence];
        assert_eq!(
            (x.live(), c.class.as_str(), c.prov),
            (live, "DeleteVsModify", Some(prov)),
            "{policy:?}"
        );
        ids_verify(&s);
    }
}

/// [API §11.7] "Sync first with conflict values" (spec sync 2b, S2B-F-11): a step-0 sync that lands conflict values is
/// appended on src alone, with no idempotency pair, and the merge is refused `conflicted_src` (exit 6), whose error
/// carries `sync` and `keys` ([F19 §10.3]); main does not move. A retry meets PR-003 before any sync: `sync` is null.
#[test]
fn a_sync_first_merge_that_lands_conflicts_appends_the_sync_and_refuses() {
    let mut s = two_tasks();
    s.ok(
        tx(vec![set(1, &[("priority", P::Int(3))])]),
        orch_on("lane/x"),
    );
    s.ok(tx(vec![set(1, &[("priority", P::Int(1))])]), orch());
    let main_before = tip(&s, "main");
    let ctx = Ctx {
        key: Some("m-conflicted".into()),
        ..orch()
    };
    let r = s.run(merge("lane/x", "main"), ctx.clone());
    assert_eq!(r.exit, 6);
    let e = r.error.as_ref().unwrap();
    assert_eq!(e.code, "conflicted_src");
    let sc = tip(&s, "lane/x").unwrap();
    let c = &s.st.dag.commits[&sc];
    assert_eq!(c.kind, "sync");
    assert_eq!(c.idem, None, "the sync carries no idempotency pair");
    assert_eq!(tip(&s, "main"), main_before, "main did not move");
    use crate::err::Kv;
    let sync = e.get("sync").expect("the sync member");
    assert_eq!(sync.member("commit"), Some(&Kv::Commit(sc)));
    assert_eq!(sync.member("outcome").and_then(Kv::as_str), Some("landed"));
    assert_eq!(sync.member("violations"), Some(&Kv::List(Vec::new())));
    match sync.member("conflicts") {
        Some(Kv::List(v)) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].member("key").and_then(Kv::as_str), Some("#1.priority"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        e.get("keys"),
        Some(&Kv::List(vec![Kv::Str("#1.priority".into())]))
    );
    // The retry, with the same key: nothing was recorded, and src is conflicted before any sync.
    let n = s.st.commit_seq;
    let r = s.run(merge("lane/x", "main"), ctx);
    let e = r.error.as_ref().unwrap();
    assert_eq!(e.code, "conflicted_src");
    assert_eq!(e.get("sync"), Some(&Kv::Null));
    assert_eq!(s.st.commit_seq, n);
    ids_verify(&s);
}

/// R29 through the Store API ([RULES/merge-table] RS-007): a lane that swaps a parent and its child in one commit (#1
/// under #2, #2 at the root) merges into main with the swap and no `HierarchyCycle`: the commit is one step, applied
/// at once.
#[test]
fn a_swap_in_one_commit_merges_clean() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
        ]),
        orch(),
    );
    s.ok(mv(2, Some(1)), orch());
    s.ok(branch("x", "main"), orch());
    s.ok(
        tx(vec![
            Stmt::Move {
                target: Target::Id(Nid(2)),
                under: None,
                position: None,
            },
            Stmt::Move {
                target: Target::Id(Nid(1)),
                under: Some(Target::Id(Nid(2))),
                position: None,
            },
        ]),
        orch_on("lane/x"),
    );
    s.ok(tx(vec![set(3, &[("priority", P::Int(1))])]), orch());
    let r = s.run(merge("lane/x", "main"), orch());
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let d = data(&r);
    assert_eq!(d.outcome, "landed");
    assert!(d.violations.is_empty(), "{:?}", d.violations);
    assert_eq!(parents_12(&s, "main"), (Some(Nid(2)), None));
    ids_verify(&s);
}

/// [F12 §6.5] "A `live` existence side": resolving a provisionally deleted node's `DeleteVsModify` to the live side
/// restores its value keys from the side's node image and its hierarchy key from that side's state at the conflict's
/// introducing commit.
#[test]
fn a_live_existence_side_restores_the_node() {
    let mut s = S::base();
    s.ok(tx(vec![task("p", "parent"), task("k", "task k")]), orch());
    s.ok(mv(2, Some(1)), orch());
    s.ok(branch("x", "main"), orch());
    s.ok(
        tx(vec![set(2, &[("priority", P::Int(0))])]),
        orch_on("lane/x"),
    );
    s.ok(
        tx(vec![Stmt::Delete {
            target: Target::Id(Nid(2)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("gone".into()),
        }]),
        orch(),
    );
    let r = s.run(merge("main", "lane/x"), orch());
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(
        data(&r).conflicts,
        vec![("#2.existence".to_string(), "DeleteVsModify".to_string())]
    );
    assert!(!state(&s, "lane/x").nodes[&Nid(2)].live(), "delete-wins");
    s.ok(resolve("#2.existence", Take::Ours), fresh_on("lane/x"));
    let st = state(&s, "lane/x");
    let x = &st.nodes[&Nid(2)];
    assert!(x.live() && x.conflicts.is_empty());
    assert_eq!(x.parent, Some(Nid(1)), "the hierarchy of ours at the merge");
    assert_eq!(field(&s, "lane/x", 2, "priority"), prio("P0"));
    ids_verify(&s);
}

/// [F12 §6.5] "A flagged edge" (spec sync 2b, S2B-F-10): on a work branch a flagged `blocks` out-edge of a tombstone is
/// re-pointed (I5′ checked) or dropped; `ours`, `theirs`, `base` and `value` are usage there, and `drop` is usage on
/// every other key.
#[test]
fn a_flagged_edge_takes_repoint_or_drop_only() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task("b", "task b"),
            task("c", "task c"),
        ]),
        orch(),
    );
    s.ok(tx(vec![link(1, "blocks", 2), link(2, "blocks", 3)]), orch());
    // #1, an open blocker, deleted: its retained `blocks` edge is flagged (FL-003).
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
    let key = "edge:#1:blocks:#2";
    for take in [Take::Ours, Take::Theirs, Take::Base, Take::Value(P::Int(1))] {
        s.refused(resolve(key, take), fresh_on("main"), "usage");
    }
    s.refused(
        resolve("#2.priority", Take::Drop),
        fresh_on("main"),
        "usage",
    );
    // #3 blocking #2 would close a cycle with #2 blocks #3.
    s.refused(
        resolve(key, Take::Repoint(Target::Id(Nid(3)))),
        fresh_on("main"),
        "E405",
    );
    let r = s.ok(resolve(key, Take::Drop), fresh_on("main"));
    assert!(r.commit.is_some());
    assert!(
        state(&s, "main").nodes[&Nid(1)].out.is_empty(),
        "the flagged edge is gone"
    );
    ids_verify(&s);
}

/// One lane commit of [`a_lane_merged_into_an_unmoved_main_takes_its_hierarchy`]: a move of #n (1–5) under #p (`None`:
/// to the root), or a priority edit of #n, a commit with no hierarchy entry.
#[derive(Clone, Debug)]
enum LaneStep {
    Move(u32, Option<u32>),
    Priority(u32, i64),
}

/// A move of #n (1–3) under #p, or to the root with `None`.
fn hmove() -> impl proptest::strategy::Strategy<Value = (u32, Option<u32>)> {
    use proptest::prelude::*;
    (1u32..=3, prop::option::of(1u32..=3))
}

fn lane_step() -> impl proptest::strategy::Strategy<Value = LaneStep> {
    use proptest::prelude::*;
    prop_oneof![
        3 => hmove().prop_map(|(n, p)| LaneStep::Move(n, p)),
        1 => (1u32..=5, 0i64..4).prop_map(|(n, k)| LaneStep::Priority(n, k)),
    ]
}

/// One step of [`a_lane_merged_into_an_unmoved_main_takes_its_hierarchy`] on lane/x or lane/y, both forked from main at
/// one commit.
#[derive(Clone, Debug)]
enum LaneOp {
    /// A commit on lane/x.
    X(LaneStep),
    /// A commit on lane/y.
    Y(LaneStep),
    /// A cherry-pick onto lane/x of a priority commit of lane/y's that lane/x does not hold: the k-th, modulo their
    /// number.
    Pick(usize),
    /// A revert on lane/x of one of its own priority commits: the k-th, modulo their number.
    Revert(usize),
    /// `merge lane/y --into lane/x`; when it stages, each violation's key is resolved to `ours` and the merge
    /// continued, and aborted if it stages again.
    MergeY,
}

fn lane_op() -> impl proptest::strategy::Strategy<Value = LaneOp> {
    use proptest::prelude::*;
    prop_oneof![
        4 => lane_step().prop_map(LaneOp::X),
        3 => lane_step().prop_map(LaneOp::Y),
        1 => (0usize..8).prop_map(LaneOp::Pick),
        1 => (0usize..8).prop_map(LaneOp::Revert),
        1 => Just(LaneOp::MergeY),
    ]
}

/// What RS-007 guarantees a lane merged into an unmoved `main` ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann
/// steps"; [RULES/merge-table] open point 35 cases (i) to (iii) and (v), decided by [AR §11] OQ-A-6 (a) and OQ-A-11
/// 11.1 (A)). lane/x and lane/y fork from main at one commit B; main's history before the fork is any sequence of
/// moves, and main makes no commit after it (o = b).
/// - A cherry-pick of one of lane/y's commits with no hierarchy entry, and a revert of one of lane/x's, lands with no
///   violation and leaves lane/x's hierarchy as it was: the replay starts from o, dst has no step and C's step is empty
///   ([RULES/merge-table] open point 35 case (ii)). Before OQ-A-6 a pick replayed lane/x's moves from state(p₁(C)),
///   which holds lane/y's moves, and could stage a cycle.
/// - lane/x merges lane/y any number of times; a merge that stages is resolved to `ours` on every violation's key and
///   continued (aborted if it stages again), so lane/x's history can hold merges that kept a cycle-closing move of
///   lane/x's against lane/y's (open point 35 (v), E4).
///
/// `merge lane/x --into main` is one-sided, since its base is tip(main), so it takes lane/x's hierarchy with no replay
/// and lands with no `HierarchyCycle` whatever lane/x's history holds (11.1 (A)). Before OQ-A-11 it replayed lane/x's
/// commits from B, which held for single-parent commits and clean merges only.
#[test]
fn a_lane_merged_into_an_unmoved_main_takes_its_hierarchy() {
    use proptest::prelude::*;
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 48,
        ..ProptestConfig::default()
    });
    let steps = (
        proptest::collection::vec(hmove(), 0..8),
        proptest::collection::vec(lane_op(), 1..16),
    );
    let result = runner.run(&steps, |(before, ops)| {
        let mut s = S::base();
        s.ok(
            tx((0..5)
                .map(|i| task(&format!("t{i}"), &format!("task {i}")))
                .collect()),
            orch(),
        );
        // A move its own branch refuses (a cycle there) is skipped.
        let mv_on = |s: &mut S, b: &str, n: u32, p: Option<u32>| {
            if p != Some(n) {
                let _ = s.run(mv(n, p), fresh_on(b));
            }
        };
        let parents = |s: &S, r: &str| -> Vec<Option<Nid>> {
            let st = state(s, r);
            (1..=5).map(|n| st.nodes[&Nid(n)].parent).collect()
        };
        for (n, p) in before {
            mv_on(&mut s, "main", n, p);
        }
        s.ok(branch("x", "main"), orch());
        s.ok(branch("y", "main"), orch());
        let base = tip(&s, "main");
        prop_assert_eq!(tip(&s, "lane/x"), base);
        prop_assert_eq!(tip(&s, "lane/y"), base);
        // The priority commits of lane/x and of lane/y, by seq.
        let (mut x_prio, mut y_prio): (Vec<u64>, Vec<u64>) = (Vec::new(), Vec::new());
        // lane/x ends by merging lane/y, so its last commit is often a merge resolved to `ours`.
        for op in ops.into_iter().chain([LaneOp::MergeY]) {
            match op {
                LaneOp::X(LaneStep::Move(n, p)) => mv_on(&mut s, "lane/x", n, p),
                LaneOp::Y(LaneStep::Move(n, p)) => mv_on(&mut s, "lane/y", n, p),
                LaneOp::X(LaneStep::Priority(n, k)) | LaneOp::Y(LaneStep::Priority(n, k)) => {
                    let (b, list) = if matches!(op, LaneOp::X(_)) {
                        ("lane/x", &mut x_prio)
                    } else {
                        ("lane/y", &mut y_prio)
                    };
                    // An edit that changes nothing makes no commit.
                    let was = tip(&s, b);
                    s.ok(tx(vec![set(n, &[("priority", P::Int(k))])]), fresh_on(b));
                    let now = tip(&s, b);
                    if now != was {
                        list.extend(now);
                    }
                }
                LaneOp::Pick(k) | LaneOp::Revert(k) => {
                    let pick = matches!(op, LaneOp::Pick(_));
                    let held = s.st.dag.ancestors(tip(&s, "lane/x"));
                    let pool: Vec<u64> = if pick {
                        y_prio
                            .iter()
                            .copied()
                            .filter(|c| !held.contains(c))
                            .collect()
                    } else {
                        x_prio.clone()
                    };
                    if pool.is_empty() {
                        continue;
                    }
                    let c = pool[k % pool.len()];
                    let hier = parents(&s, "lane/x");
                    let cmd = if pick {
                        Cmd::CherryPick {
                            commit: format!("s{c}"),
                            onto: Some("lane/x".into()),
                            message: String::new(),
                        }
                    } else {
                        Cmd::Revert {
                            commit: format!("s{c}"),
                            onto: Some("lane/x".into()),
                            mainline: None,
                            message: String::new(),
                        }
                    };
                    let r = s.run(cmd, fresh_on("lane/x"));
                    // A revert of a commit that later commits depend on is refused (DM-008).
                    if !pick && r.outcome == Outcome::Refused {
                        prop_assert_eq!(
                            r.error.as_ref().map(|e| e.code.as_str()),
                            Some("revert_refused")
                        );
                        continue;
                    }
                    prop_assert!(r.outcome == Outcome::Ok, "{:?}", r.error);
                    let d = data(&r);
                    prop_assert_eq!(d.outcome, "landed");
                    prop_assert!(d.violations.is_empty(), "{:?}", d.violations);
                    prop_assert_eq!(parents(&s, "lane/x"), hier);
                }
                LaneOp::MergeY => {
                    let r = s.run(merge("lane/y", "lane/x"), fresh_on("lane/x"));
                    match r.outcome {
                        Outcome::Ok => {}
                        Outcome::Staged => {
                            let d = data(&r);
                            let g = d.staging_ref.clone().expect("G");
                            let keys: Vec<String> = d
                                .violations
                                .iter()
                                .filter(|v| v.key != "-")
                                .map(|v| v.key.clone())
                                .collect();
                            // A resolution the write path refuses (a cycle it would close) leaves the merge
                            // staged.
                            let mut ok = true;
                            for k in keys {
                                ok &= s.run(resolve(&k, Take::Ours), fresh_on(&g)).outcome
                                    == Outcome::Ok;
                            }
                            let landed = ok
                                && s.run(continue_("lane/y", "lane/x"), fresh_on("lane/x"))
                                    .outcome
                                    == Outcome::Ok;
                            if !landed {
                                s.ok(
                                    Cmd::MergeAbort {
                                        src: Some("lane/y".into()),
                                        into: Some("lane/x".into()),
                                    },
                                    fresh_on("lane/x"),
                                );
                            }
                        }
                        _ => prop_assert!(false, "{:?}", r.error),
                    }
                }
            }
        }
        // A merge into main needs lane/x free of unresolved conflicts (`conflicted_src`): each conflict value a pick, a
        // revert or the merge of lane/y landed is resolved to ours first, a commit with no hierarchy entry.
        let st = state(&s, "lane/x");
        let uid = |n: Nid| s.st.alloc.uids[&n];
        let keys: Vec<String> = st
            .nodes
            .iter()
            .flat_map(|(n, x)| {
                x.conflicts
                    .keys()
                    .map(|a| crate::merge::key_text(&Key::Node(*n, a.clone()), &uid))
            })
            .collect();
        for k in keys {
            s.ok(resolve(&k, Take::Ours), fresh_on("lane/x"));
        }
        let lane = parents(&s, "lane/x");
        let r = s.run(merge("lane/x", "main"), fresh_on("main"));
        prop_assert!(r.outcome == Outcome::Ok, "{:?}", r.error);
        let d = data(&r);
        prop_assert!(d.sync.is_none(), "main did not move since the fork");
        // A lane whose steps all changed nothing made no commit: the merge is `up-to-date`.
        if d.outcome != "up-to-date" {
            prop_assert_eq!(d.outcome, "landed");
            prop_assert_eq!(d.lca.clone(), base.into_iter().collect::<Vec<_>>());
        }
        prop_assert!(d.violations.is_empty(), "{:?}", d.violations);
        prop_assert_eq!(parents(&s, "main"), lane);
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

/// The family of example E4 of [RULES/merge-table] open point 35 case (v) ([RULES/merge-table] RS-007; [F12 §7.4] row
/// "Kleppmann steps"; decided by [AR §11] OQ-A-11 11.1 (A)). lane/y and lane/x fork from main at B; lane/y makes moves,
/// then lane/x makes moves (each later than every move of lane/y's), so lane/x's moves are the ones that close cycles;
/// lane/x merges lane/y, resolving every violation's key to `ours`, which keeps lane/x's cycle-closing moves; a merge
/// that a refused resolution or a re-staging leaves staged is aborted. `merge lane/x --into main`, with main unmoved,
/// lands with lane/x's hierarchy. Every commit here descends from B, so the replay start of 11.1 (B) is B and does not
/// cover the case: the merge commit re-asserts only the keys lane/y moved, and a replay undoes lane/x's resolved moves
/// again. The one-sided rule (A) takes lane/x's values with no replay.
#[test]
fn a_lane_that_kept_its_own_moves_against_a_merged_lane_lands_on_an_unmoved_main() {
    use proptest::prelude::*;
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    });
    // A move of #n (1–3) under another of the three, or now and then to the root: moves that often close cycles.
    let dense = || {
        (
            1u32..=3,
            prop_oneof![4 => (1u32..=2).prop_map(Some), 1 => Just(None)],
        )
            .prop_map(|(n, d)| (n, d.map(|d| (n - 1 + d) % 3 + 1)))
    };
    let moves = (
        proptest::collection::vec(dense(), 1..5),
        proptest::collection::vec(dense(), 1..5),
    );
    let result = runner.run(&moves, |(ys, xs)| {
        let mut s = S::base();
        s.ok(
            tx((0..3)
                .map(|i| task(&format!("t{i}"), &format!("task {i}")))
                .collect()),
            orch(),
        );
        s.ok(branch("x", "main"), orch());
        s.ok(branch("y", "main"), orch());
        let base = tip(&s, "main");
        for (b, list) in [("lane/y", &ys), ("lane/x", &xs)] {
            for (n, p) in list {
                // A move its own branch refuses (a cycle there) is skipped.
                let _ = s.run(mv(*n, *p), fresh_on(b));
            }
        }
        let r = s.run(merge("lane/y", "lane/x"), fresh_on("lane/x"));
        if r.outcome == Outcome::Staged {
            let d = data(&r);
            let g = d.staging_ref.clone().expect("G");
            let keys: Vec<String> = d
                .violations
                .iter()
                .filter(|v| v.key != "-")
                .map(|v| v.key.clone())
                .collect();
            let mut ok = true;
            for k in keys {
                ok &= s.run(resolve(&k, Take::Ours), fresh_on(&g)).outcome == Outcome::Ok;
            }
            let landed = ok
                && s.run(continue_("lane/y", "lane/x"), fresh_on("lane/x"))
                    .outcome
                    == Outcome::Ok;
            if !landed {
                s.ok(
                    Cmd::MergeAbort {
                        src: Some("lane/y".into()),
                        into: Some("lane/x".into()),
                    },
                    fresh_on("lane/x"),
                );
            }
        } else {
            prop_assert!(r.outcome == Outcome::Ok, "{:?}", r.error);
        }
        let lane: Vec<Option<Nid>> = {
            let st = state(&s, "lane/x");
            (1..=3).map(|n| st.nodes[&Nid(n)].parent).collect()
        };
        let r = s.run(merge("lane/x", "main"), fresh_on("main"));
        prop_assert!(r.outcome == Outcome::Ok, "{:?}", r.error);
        let d = data(&r);
        prop_assert!(d.sync.is_none(), "main did not move since the fork");
        if d.outcome != "up-to-date" {
            prop_assert_eq!(d.outcome, "landed");
            prop_assert_eq!(d.lca.clone(), base.into_iter().collect::<Vec<_>>());
        }
        prop_assert!(d.violations.is_empty(), "{:?}", d.violations);
        let st = state(&s, "main");
        prop_assert_eq!(
            (1..=3)
                .map(|n| st.nodes[&Nid(n)].parent)
                .collect::<Vec<_>>(),
            lane
        );
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

/// [RULES/merge-table] open point 35 case (ii)'s guarantee ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann
/// steps"; DM-003; decided by [AR §11] OQ-A-6 (a)): a cherry-pick of a commit with no hierarchy entry leaves every
/// hierarchy key at o's value. lane/y makes moves and then C, a priority edit; lane/x, forked at the same commit, makes
/// its own moves; picking C onto lane/x lands with no violation, with C's priority and lane/x's hierarchy. The moves
/// draw from three nodes, so lane/x's moves often close a cycle when replayed from state(p₁(C)), which holds lane/y's
/// moves: replayed from there, as before the decision, such a pick staged a `HierarchyCycle`.
#[test]
fn a_cherry_pick_of_a_commit_with_no_move_lands_on_dsts_hierarchy() {
    use proptest::prelude::*;
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    });
    // A move of one of #1–#3 under another of them.
    let move3 = || (1u32..=3, 1u32..=2).prop_map(|(n, d)| (n, Some((n - 1 + d) % 3 + 1)));
    let steps = (
        proptest::collection::vec(move3(), 1..6),
        proptest::collection::vec(move3(), 2..9),
    );
    let result = runner.run(&steps, |(on_y, on_x)| {
        let mut s = S::base();
        s.ok(
            tx((0..4)
                .map(|i| task(&format!("t{i}"), &format!("task {i}")))
                .collect()),
            orch(),
        );
        s.ok(branch("x", "main"), orch());
        s.ok(branch("y", "main"), orch());
        // A move its own branch refuses (a cycle there) is skipped.
        for (b, moves) in [("lane/y", on_y), ("lane/x", on_x)] {
            for (n, p) in moves {
                let _ = s.run(mv(n, p), fresh_on(b));
            }
        }
        let c = s
            .ok(
                tx(vec![set(4, &[("priority", P::Int(0))])]),
                fresh_on("lane/y"),
            )
            .commit
            .expect("C");
        let parents = |s: &S| -> Vec<Option<Nid>> {
            let st = state(s, "lane/x");
            (1..=4).map(|n| st.nodes[&Nid(n)].parent).collect()
        };
        let hier = parents(&s);
        let r = s.run(
            Cmd::CherryPick {
                commit: format!("s{c}"),
                onto: Some("lane/x".into()),
                message: String::new(),
            },
            fresh_on("lane/x"),
        );
        prop_assert!(r.outcome == Outcome::Ok, "{:?}", r.error);
        let d = data(&r);
        prop_assert_eq!(d.outcome, "landed");
        prop_assert!(
            d.violations.is_empty() && d.conflicts.is_empty(),
            "{:?} {:?}",
            d.violations,
            d.conflicts
        );
        prop_assert_eq!(parents(&s), hier);
        prop_assert_eq!(field(&s, "lane/x", 4, "priority"), prio("P0"));
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

/// [F11 §3.8], §7 "Retention"; [API §8.5] (spec sync 2b, S2B-R-5, S2B-F-92): a merged, deleted ref's `REFS` row and
/// its entries in the other refs' absorbed vectors are dropped at a `gc` run past the reflog window once no marker row
/// names it as origin ref, and not before: first the run's inertness move and `MARKERS_OLD` retention drop the
/// `deleted` marker it originated, then the row expires. While lane/z, forked before the merge, has not absorbed the
/// marker, the marker stays in `MARKERS` and the row stays too.
#[test]
fn a_merged_deleted_ref_expires_at_a_gc_once_its_markers_are_gone() {
    use crate::clock::EnvClock;
    let mut s = two_tasks();
    s.ok(branch("z", "main"), orch());
    s.ok(
        tx(vec![Stmt::Delete {
            target: Target::Id(Nid(1)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("gone".into()),
        }]),
        orch_on("lane/x"),
    );
    s.ok(merge("lane/x", "main"), orch());
    let x = s.st.dag.live("lane/x").unwrap().id;
    s.ok(
        Cmd::BranchDelete {
            name: "lane/x".into(),
            force: false,
        },
        orch(),
    );
    let gc = || Cmd::Gc {
        reflog_expire_ms: Some(60_000),
        cruft_delay_ms: None,
        force: false,
    };
    let names_x = |s: &S| s.st.markers.rows().any(|m| m.key.1 == x);
    let in_absorbed = |s: &S| s.st.markers.absorbed.values().any(|v| v.contains_key(&x));
    // Inside the reflog window: nothing expires.
    s.ok(gc(), Ctx::default());
    assert!(s.st.dag.refs.contains_key(&x) && names_x(&s) && in_absorbed(&s));
    // Past the window, lane/z has not absorbed the marker, so it is not inert: the row stays.
    s.ok(
        Cmd::EnvClock(EnvClock {
            advance_ms: Some(3_600_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    s.ok(gc(), Ctx::default());
    assert!(s.st.dag.refs.contains_key(&x) && names_x(&s) && in_absorbed(&s));
    assert!(
        s.st.runtime().refs.iter().any(|r| r.name == "lane/x"),
        "the deleted row is listed"
    );
    // lane/z goes; the next gc moves the absorbed marker to MARKERS_OLD, drops it as older than the window, then drops
    // lane/x's row and absorbed entries.
    s.ok(
        Cmd::BranchDelete {
            name: "lane/z".into(),
            force: true,
        },
        orch(),
    );
    s.ok(gc(), Ctx::default());
    assert!(!names_x(&s), "the marker is dropped by the retention");
    assert!(!s.st.dag.refs.contains_key(&x), "lane/x's row expired");
    assert!(!in_absorbed(&s));
    assert!(!s.st.runtime().refs.iter().any(|r| r.name == "lane/x"));
    // The moves older than the window are no longer held ([F12 §3.5]): main's reflog is empty, and `op restore` of a
    // seq older than main's newest dropped move is E301, nothing moved ([API §11.11]).
    assert!(s.st.dag.live("main").unwrap().moves.is_empty());
    let tip_main = tip(&s, "main");
    s.refused(Cmd::OpRestore { seq: 1 }, orch(), "E301");
    assert_eq!(tip(&s, "main"), tip_main);
}

/// A `Schema` command's `policy` item ([API §9.8]; [F08 §8.5.6]).
fn policy_cmd(name: &str, value: Option<&str>) -> Cmd {
    Cmd::Schema {
        items: vec![crate::schema::Item::Policy(crate::schema::PolicyItem {
            name: name.into(),
            value: value.map(str::to_string),
        })],
        message: String::new(),
    }
}

/// Policy rows as schema items ([F08 §8.5.6]; [API §9.8]; [RULES/merge-table] MC-013, MR-055; spec sync 2b, S2B-F-21):
/// `Schema` writes a row's value in canonical form, and its default or `null` removes the item (absence is the
/// default's one form); a row that names nothing or a value of another type is `bad_value`. Two branches that change
/// one row differently stage a `SchemaConflict` on its key, never a conflict value; one side's change merges.
#[test]
fn policy_rows_are_schema_items_that_merge_atomically() {
    use crate::schema::ItemKey;
    let mut s = two_tasks();
    let key = ItemKey::Policy("policy.self-claim-roles".into());
    s.ok(
        policy_cmd(
            "policy.self-claim-roles",
            Some(" tester , developer , architect"),
        ),
        fresh_on("main"),
    );
    assert_eq!(
        state(&s, "main").schema.policy("policy.self-claim-roles"),
        Some("architect,developer,tester"),
        "the canonical form: sorted, joined by ,"
    );
    s.ok(
        policy_cmd("policy.self-claim-roles", Some("developer,tester")),
        fresh_on("main"),
    );
    assert!(
        !state(&s, "main").schema.items.contains_key(&key),
        "the default removes the item"
    );
    s.refused(
        policy_cmd("merge.policy.task", Some("maybe")),
        fresh_on("main"),
        "bad_value",
    );
    s.refused(
        policy_cmd("policy.nosuch", Some("yes")),
        fresh_on("main"),
        "bad_value",
    );
    // One side's change merges; two different changes stage `SchemaConflict` on the row's key.
    s.ok(
        policy_cmd("merge.policy.task", Some("ours")),
        fresh_on("lane/x"),
    );
    let r = s.run(merge("lane/x", "main"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert_eq!(
        state(&s, "main").schema.policy("merge.policy.task"),
        Some("ours")
    );
    s.ok(
        policy_cmd("merge.policy.task", Some("theirs")),
        fresh_on("lane/x"),
    );
    s.ok(policy_cmd("merge.policy.task", None), fresh_on("main"));
    let r = s.run(merge("lane/x", "main"), fresh_on("main"));
    assert_eq!(r.outcome, Outcome::Staged, "{:?}", r.error);
    let d = data(&r);
    let sync = d.sync.as_ref().expect("the step-0 sync");
    assert_eq!(
        sync.violations
            .iter()
            .map(|v| (v.key.as_str(), v.class.as_str()))
            .collect::<Vec<_>>(),
        vec![("schema:policy:merge.policy.task", "SchemaConflict")]
    );
    assert!(sync.conflicts.is_empty(), "never a conflict value");
    ids_verify(&s);
}

/// [F12 §6.5] `SupersedeFork` (V06, I6; spec sync 2b): main and a lane each supersede one note; merging main into the
/// lane puts the conflict value on the src side's (main's) `supersedes` edge. `value` is usage; `theirs` keeps that
/// edge and removes every other active `supersedes` edge to the target by `RemoveEdge` in the same commit, so one
/// superseder remains.
#[test]
fn a_supersede_fork_resolved_theirs_keeps_one_superseder() {
    let mut s = S::base();
    s.ok(tx(vec![node("a", "note", &[("title", t("old"))])]), orch());
    s.ok(branch("x", "main"), orch());
    let supersede = |v: &str, title: &str| {
        tx(vec![
            node(v, "note", &[("title", t(title))]),
            Stmt::Link {
                src: Target::Var(v.into()),
                kind: "supersedes".into(),
                dst: Target::Id(Nid(1)),
                pinned: None,
            },
        ])
    };
    s.ok(supersede("x", "lane"), orch_on("lane/x"));
    s.ok(supersede("y", "main"), orch());
    let r = s.run(merge("main", "lane/x"), orch());
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    let key = "edge:#3:supersedes:#1";
    assert_eq!(
        data(&r).conflicts,
        vec![(key.to_string(), "SupersedeFork".to_string())]
    );
    s.refused(
        resolve(key, Take::Value(P::Int(1))),
        fresh_on("lane/x"),
        "usage",
    );
    let r = s.ok(resolve(key, Take::Theirs), fresh_on("lane/x"));
    assert!(r.commit.is_some());
    let st = state(&s, "lane/x");
    let superseders: Vec<Nid> = st
        .nodes
        .iter()
        .filter(|(_, x)| {
            x.out
                .keys()
                .any(|k| k.kind == "supersedes" && k.dst == Nid(1))
        })
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(
        superseders,
        vec![Nid(3)],
        "theirs keeps main's #3 edge and drops the lane's #2 edge"
    );
    assert!(st.nodes[&Nid(3)].conflicts.is_empty());
    ids_verify(&s);
}
