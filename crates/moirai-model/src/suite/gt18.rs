//! GT18's I26′ oracle on the model ([60 §3.13]; [RULES/state-definition] §6, `door-coverage`; [F13 §4.2] MC-7):
//! random histories over five refs of three kinds, through completions, status writes, reopens, deletes, forks, branch
//! deletions and backward ref moves (`undo`, `op restore`), comparing after every command the marker cache and the
//! maintained absorbed vectors with their definitions ([PLAN §6.2] R12). The result `markers` of a command are the
//! listed entries of its group ([API §10.8]).
//!
//! WP-94 runs the full volume (≥ 10⁶ histories nightly, 10⁴ in the PR tier); this suite keeps its shape at unit-test
//! size, and more cases with `MOIRAI_TEST_TIER=nightly`.
//!
//! Each history runs twice. Without a fold, every command's listed markers must be exactly the entries the
//! definition's holder sets imply (a `settled` or `deleted` entry when a hold origin gains its first live holder, a
//! `cleared` entry when it loses its last; ME-001 to ME-007). With the checkpoint fold of ME-012 run after every
//! command, the `Marker` records and every row's contents equal those without it (ME-012, ME-013; open point 17
//! decided (a), spec sync 2b S2B-M-19).

use super::*;
use crate::api::MarkerOut;
use crate::coord::{Oracle, closed, view_kind};
use crate::dag::{MoveReason, RefKind};
use crate::markers::{Cause, MKind, agrees_with_definition};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

const REFS: [&str; 5] = ["main", "lane/a", "lane/b", "lane/c", "plan/p"];
const PLAN: usize = 4;
const STATUSES: [&str; 4] = ["done", "cancelled", "open", "in_progress"];
const TASKS: u32 = 4;

/// The orchestrator without a default key, so a repeated operation runs again instead of replaying ([API §7.1]).
fn admin() -> Ctx {
    Ctx {
        no_dedupe: true,
        ..orch()
    }
}

/// [`admin`] on a branch.
fn admin_on(r: &str) -> Ctx {
    Ctx {
        no_dedupe: true,
        ..orch_on(r)
    }
}

#[derive(Clone, Debug)]
enum G {
    Complete(u32, usize),
    Status(u32, usize, usize),
    Reopen(u32, usize),
    Rm(u32, usize),
    /// An ordinary field write (`priority`), which changes no hold (ME-010) but moves the ref on (ME-011).
    Set(u32, usize, i64),
    Fork(usize, usize),
    Drop(usize),
    Undo(usize),
    Restore(usize, usize),
}

fn g() -> impl Strategy<Value = G> {
    let n = 1..=TASKS;
    let r = 0..REFS.len();
    prop_oneof![
        3 => (n.clone(), r.clone()).prop_map(|(n, r)| G::Complete(n, r)),
        2 => (n.clone(), r.clone(), 0..STATUSES.len()).prop_map(|(n, r, s)| G::Status(n, r, s)),
        2 => (n.clone(), r.clone()).prop_map(|(n, r)| G::Reopen(n, r)),
        1 => (n.clone(), r.clone()).prop_map(|(n, r)| G::Rm(n, r)),
        2 => (n.clone(), r.clone(), 0..4i64).prop_map(|(n, r, p)| G::Set(n, r, p)),
        2 => (1..REFS.len(), r.clone()).prop_map(|(a, b)| G::Fork(a, b)),
        1 => (1..REFS.len()).prop_map(G::Drop),
        1 => r.clone().prop_map(G::Undo),
        1 => (r, 1..4usize).prop_map(|(r, k)| G::Restore(r, k)),
        // `plan/p` diverges only by existence and ordinary writes (status is masked there, I33′): weight its deletes,
        // undos and forks so a lane holds a `plan/p` origin that the plan ref then leaves (ME-011 on any ref kind).
        2 => n.clone().prop_map(|n| G::Rm(n, PLAN)),
        2 => (n, 0..4i64).prop_map(|(n, p)| G::Set(n, PLAN, p)),
        2 => Just(G::Undo(PLAN)),
        2 => (1..PLAN).prop_map(|a| G::Fork(a, PLAN)),
    ]
}

/// The holder sets by definition: every closed hold (`#N`, origin) of a live work ref, with its hold value and the
/// refs that hold it ([RULES/state-definition] MF-006).
fn def_holders(s: &S) -> BTreeMap<(Nid, u64), (&'static str, BTreeSet<String>)> {
    let mut o = Oracle::new(&s.st.dag, &s.st.alloc);
    let mut out: BTreeMap<(Nid, u64), (&'static str, BTreeSet<String>)> = BTreeMap::new();
    for x in s.st.dag.live_refs().filter(|x| view_kind(x.kind).0) {
        let Some(t) = x.tip else { continue };
        for n in (1..=TASKS).map(Nid) {
            let h = o.hold_at(t, n);
            if closed(h) {
                let origin = o.origin(t, n);
                out.entry((n, origin))
                    .or_insert((h, BTreeSet::new()))
                    .1
                    .insert(x.name.clone());
            }
        }
    }
    out
}

/// The listed entries the definition implies between two holder-set snapshots: `settled` or `deleted` for an origin
/// that gains its first holder (ME-001, ME-003, ME-006, ME-007), `cleared` for one that loses its last (ME-004 to
/// ME-006), as (kind, `#N`, origin commit), sorted.
fn implied(
    before: &BTreeMap<(Nid, u64), (&'static str, BTreeSet<String>)>,
    after: &BTreeMap<(Nid, u64), (&'static str, BTreeSet<String>)>,
) -> Vec<(MKind, Nid, u64)> {
    let mut v = Vec::new();
    for (k, (h, _)) in after {
        if !before.contains_key(k) {
            let kind = if *h == "deleted" {
                MKind::Deleted
            } else {
                MKind::Settled
            };
            v.push((kind, k.0, k.1));
        }
    }
    for k in before.keys() {
        if !after.contains_key(k) {
            v.push((MKind::Cleared, k.0, k.1));
        }
    }
    v.sort();
    v
}

fn listed(m: &[MarkerOut]) -> Vec<(MKind, Nid, u64)> {
    let mut v: Vec<_> = m.iter().map(|x| (x.kind, x.id, x.commit)).collect();
    v.sort();
    v
}

/// A store with tasks #1 to #4 on `main` and every lane forked from it.
fn start() -> S {
    let mut s = S::base();
    s.ok(
        tx((1..=TASKS)
            .map(|i| task(&format!("t{i}"), &format!("task {i}")))
            .collect()),
        admin(),
    );
    for r in &REFS[1..] {
        fork(&mut s, r, "main");
    }
    s
}

/// `branch -D` of a ref.
fn drop_ref(s: &mut S, name: &str) -> Reply {
    s.ok(
        Cmd::BranchDelete {
            name: name.into(),
            force: true,
        },
        admin(),
    )
}

/// Creates `name` from `from` (a `plan/*` name makes a plan ref).
fn create(s: &mut S, name: &str, from: &str) -> Reply {
    s.run(
        Cmd::BranchCreate {
            name: name.into(),
            from: Some(from.into()),
            kind: name.starts_with("plan/").then_some(RefKind::Plan),
        },
        admin(),
    )
}

/// Creates `name` from `from`, deleting a live ref of that name first; the reply of the create.
fn fork(s: &mut S, name: &str, from: &str) -> Reply {
    if s.st.dag.live(name).is_some() {
        drop_ref(s, name);
    }
    create(s, name, from)
}

fn claim_cmd(n: u32) -> Cmd {
    Cmd::Claim {
        ids: vec![Target::Id(Nid(n))],
        next: false,
        scope: None,
        role: None,
        agent: None,
        ttl: None,
        start: false,
        run: None,
        session: false,
    }
}

fn complete_cmd(n: u32) -> Cmd {
    Cmd::Complete {
        id: Target::Id(Nid(n)),
        outcome: "done".into(),
        summary: "s".into(),
        evidence: vec![],
        move_lease: None,
    }
}

fn lease_ctx(lease: String) -> Ctx {
    Ctx {
        lease: Some(lease),
        no_dedupe: true,
        ..Default::default()
    }
}

/// The lease a claim's reply names; `None` when the claim was refused.
fn lease_of(c: &Reply) -> Option<String> {
    Some(
        c.yields
            .first()?
            .rows
            .first()?
            .iter()
            .find(|(k, _)| k == "lease")?
            .1
            .clone(),
    )
}

/// Claims `#n` on `r` and completes it; `None` when the claim is refused (not ready there).
fn complete(s: &mut S, n: u32, r: &str) -> Option<Reply> {
    let c = s.run(claim_cmd(n), admin_on(r));
    let lease = lease_of(&c)?;
    Some(s.run(complete_cmd(n), lease_ctx(lease)))
}

/// Runs one command of a history and compares its listed markers with the definition (without a fold), then folds
/// (with one) and compares the cache's answers and vectors with the definition.
fn checked(
    s: &mut S,
    fold: bool,
    what: &str,
    f: impl FnOnce(&mut S) -> Vec<MarkerOut>,
) -> Result<(), TestCaseError> {
    let before = (!fold).then(|| def_holders(s));
    let got = f(s);
    if let Some(before) = before {
        let want = implied(&before, &def_holders(s));
        if listed(&got) != want {
            return Err(TestCaseError::fail(format!(
                "{what}: listed {:?}, the definition implies {want:?}",
                listed(&got)
            )));
        }
    }
    if fold {
        s.st.markers.fold_inert(&s.st.dag);
    }
    agrees_with_definition(&s.st.dag, &s.st.alloc, (1..=TASKS).map(Nid), &s.st.markers)
        .map_err(|e| TestCaseError::fail(format!("after {what}: {e}")))
}

fn history(ops: Vec<G>, fold: bool) -> Result<S, TestCaseError> {
    let mut s = start();
    for o in ops {
        let live = |s: &S, i: usize| s.st.dag.live(REFS[i]).is_some();
        let what = format!("{o:?}");
        match o {
            G::Complete(n, r) if live(&s, r) => {
                let claim = claim_cmd(n);
                let mut lease = None;
                checked(&mut s, fold, &what, |s| {
                    let c = s.run(claim, admin_on(REFS[r]));
                    lease = lease_of(&c);
                    c.markers
                })?;
                if let Some(l) = lease {
                    checked(&mut s, fold, &what, |s| {
                        s.run(complete_cmd(n), lease_ctx(l)).markers
                    })?;
                }
            }
            G::Status(n, r, st) if live(&s, r) => {
                checked(&mut s, fold, &what, |s| {
                    s.run(
                        tx(vec![set(n, &[("status", t(STATUSES[st]))])]),
                        admin_on(REFS[r]),
                    )
                    .markers
                })?;
            }
            G::Reopen(n, r) if live(&s, r) => {
                checked(&mut s, fold, &what, |s| {
                    s.run(
                        tx(vec![Stmt::Reopen {
                            target: Target::Id(Nid(n)),
                            reason: "again".into(),
                        }]),
                        admin_on(REFS[r]),
                    )
                    .markers
                })?;
            }
            G::Rm(n, r) if live(&s, r) => {
                checked(&mut s, fold, &what, |s| {
                    s.run(
                        tx(vec![Stmt::Delete {
                            target: Target::Id(Nid(n)),
                            policy: None,
                            replaced_by: None,
                            release: true,
                            reason: Some("r".into()),
                        }]),
                        admin_on(REFS[r]),
                    )
                    .markers
                })?;
            }
            G::Set(n, r, p) if live(&s, r) => {
                checked(&mut s, fold, &what, |s| {
                    let r = s.run(
                        tx(vec![set(n, &[("priority", t(&format!("P{p}")))])]),
                        admin_on(REFS[r]),
                    );
                    assert!(
                        r.error.is_none()
                            || r.error.as_ref().is_some_and(|e| e.code == "not_found"),
                        "a priority write is refused only for a deleted task: {:?}",
                        r.error
                    );
                    r.markers
                })?;
            }
            G::Fork(a, b) if a != b && live(&s, b) => {
                if live(&s, a) {
                    checked(&mut s, fold, &what, |s| drop_ref(s, REFS[a]).markers)?;
                }
                checked(&mut s, fold, &what, |s| create(s, REFS[a], REFS[b]).markers)?;
            }
            G::Drop(a) if live(&s, a) => {
                checked(&mut s, fold, &what, |s| drop_ref(s, REFS[a]).markers)?;
            }
            G::Undo(r) if live(&s, r) => {
                let tip = s.st.dag.live(REFS[r]).and_then(|x| x.tip);
                if let Some(p) = tip.and_then(|t| s.st.dag.commits[&t].parents.first().copied()) {
                    checked(&mut s, fold, &what, |s| {
                        s.st.move_ref(REFS[r], Some(p), MoveReason::Undo, "orch")
                    })?;
                }
            }
            G::Restore(r, k) if live(&s, r) => {
                let olds: Vec<Option<u64>> =
                    s.st.dag
                        .live(REFS[r])
                        .map(|x| x.moves.iter().rev().map(|m| m.old).collect())
                        .unwrap_or_default();
                if let Some(Some(target)) = olds.get(k - 1).copied() {
                    checked(&mut s, fold, &what, |s| {
                        s.st.move_ref(REFS[r], Some(target), MoveReason::OpRestore, "orch")
                    })?;
                }
            }
            _ => continue,
        }
    }
    Ok(s)
}

/// The marker rows of both sections by identity: ME-012's move changes only where a row lies.
fn all_rows(s: &S) -> BTreeMap<crate::markers::MKey, crate::markers::Marker> {
    s.st.markers.rows().map(|m| (m.key, m.clone())).collect()
}

/// GT18's oracle on the model, without and with the checkpoint fold between commands; with the fold after every
/// command the `Marker` records and every row's contents equal those without it ([RULES/state-definition] ME-012,
/// ME-013, open point 17 decided (a)).
#[test]
fn the_marker_cache_equals_the_definition() {
    let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") | Ok("exit") => 2048,
        _ => 128,
    };
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases,
        ..ProptestConfig::default()
    });
    if let Err(e) = runner.run(&proptest::collection::vec(g(), 1..24), |ops| {
        let plain = history(ops.clone(), false)?;
        let folded = history(ops, true)?;
        prop_assert_eq!(
            plain.st.markers.records(),
            folded.st.markers.records(),
            "the records differ with the fold"
        );
        prop_assert_eq!(
            all_rows(&plain),
            all_rows(&folded),
            "the rows differ with the fold"
        );
        Ok(())
    }) {
        panic!("{e}");
    }
}

/// The reviewer's case of ME-011 on a plan ref, as a fixed history: `rm #1` on `plan/p`, a lane forked from it, `undo`
/// on `plan/p`, then `rm #2` there. The lane holds a `deleted` origin on `plan/p` that `plan/p` no longer contains.
#[test]
fn a_plan_ref_that_diverges_flags_the_markers_it_originated() {
    let ops = vec![
        G::Rm(1, PLAN),
        G::Fork(2, PLAN),
        G::Undo(PLAN),
        G::Rm(2, PLAN),
        G::Fork(3, PLAN),
    ];
    for fold in [false, true] {
        history(ops.clone(), fold).unwrap();
    }
}

/// The reviewer's case of ME-013 with a fold: `cleared` rows move to `MARKERS_OLD`, and an `undo` that makes the
/// origin ref hold again re-emits the marker, so it excludes again ([RULES/state-definition] SX-003).
#[test]
fn a_folded_cleared_marker_is_re_emitted_when_held_again() {
    let ops = vec![G::Complete(1, 1), G::Reopen(1, 1), G::Undo(1)];
    history(ops, true).unwrap();
}

/// ME-013 for an absorbed row: `main` completes `#1` and every other ref is forked from it, so the fold moves the
/// active marker to `MARKERS_OLD`; `undo` on `main` then leaves the origin while the lanes still hold it, and a fork of
/// `plan/p` from before the completion lacks it too: each revives the row, so `#1` is excluded there.
#[test]
fn an_absorbed_marker_returns_when_a_ref_leaves_its_origin() {
    let mut ops = vec![G::Complete(1, 0)];
    ops.extend((1..REFS.len()).map(|a| G::Fork(a, 0)));
    ops.push(G::Undo(0));
    history(ops.clone(), true).unwrap();
    history(ops, false).unwrap();
    let mut s = start();
    let before = s.st.dag.live("main").unwrap().tip.unwrap();
    complete(&mut s, 1, "main").expect("ready on main");
    for r in &REFS[1..] {
        fork(&mut s, r, "main");
    }
    s.st.markers.fold_inert(&s.st.dag);
    assert_eq!(s.st.markers.old.len(), 1, "the absorbed marker is inert");
    // A fork from the commit before the completion: the new ref lacks the origin that the lanes hold.
    drop_ref(&mut s, "plan/p");
    let r = s.run(
        Cmd::BranchCreate {
            name: "plan/p".into(),
            from: Some(format!("s{before}")),
            kind: Some(RefKind::Plan),
        },
        admin(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    assert!(s.st.markers.old.is_empty(), "ME-013 revives the row");
    let plan = s.st.dag.live("plan/p").unwrap().clone();
    assert!(s.st.markers.excluded(&s.st.dag, &plan, Nid(1)));
    agrees_with_definition(&s.st.dag, &s.st.alloc, (1..=TASKS).map(Nid), &s.st.markers).unwrap();
}

fn row(s: &S, n: u32) -> Vec<crate::api::MarkerSnap> {
    s.st.marker_rows()
        .into_iter()
        .filter(|m| m.marker.key.0 == Nid(n))
        .collect()
}

/// A completion writes the `settled` marker with the lease holder and the outcome (ME-001, MF-009); a fork joins its
/// holders (ME-007) without a listed entry; a reopen on the origin ref leaves them (ME-004) and the fork keeps the
/// marker active; `branch -D` of the last holder clears it (ME-005) with cause `branch-delete`.
#[test]
fn markers_follow_holds_through_the_doors() {
    let mut s = start();
    let r = complete(&mut s, 1, "lane/a").expect("ready on lane/a");
    assert_eq!(r.markers.len(), 1, "{:?}", r.markers);
    let m = &r.markers[0];
    assert_eq!(
        (m.kind, m.id, m.ref_.as_str(), m.cause, m.outcome.as_deref()),
        (MKind::Settled, Nid(1), "lane/a", Cause::Ops, Some("done"))
    );
    let origin = m.commit;
    let rows = row(&s, 1);
    assert_eq!(rows[0].marker.actor.as_deref(), Some("orch"));
    assert_eq!(rows[0].holders, ["lane/a"]);
    assert!(rows[0].active_on.contains(&"main".to_string()));
    assert!(!rows[0].active_on.contains(&"lane/a".to_string()));
    // A fork of lane/a joins the holder set: a holders entry, not listed.
    let f = fork(&mut s, "lane/b", "lane/a");
    assert!(f.markers.is_empty(), "{:?}", f.markers);
    assert_eq!(row(&s, 1)[0].holders, ["lane/a", "lane/b"]);
    // Reopen on lane/a: lane/a leaves; lane/b still holds, so no cleared entry and #1 stays excluded on main.
    let re = s.ok(
        tx(vec![Stmt::Reopen {
            target: Target::Id(Nid(1)),
            reason: "again".into(),
        }]),
        admin_on("lane/a"),
    );
    assert!(re.markers.is_empty(), "{:?}", re.markers);
    let main = s.st.dag.live("main").unwrap().clone();
    assert!(s.st.markers.excluded(&s.st.dag, &main, Nid(1)));
    // branch -D lane/b: the last holder leaves and the marker is cleared.
    let keyed = Ctx {
        key: Some("drop-b".into()),
        ..orch()
    };
    let d = s.ok(
        Cmd::BranchDelete {
            name: "lane/b".into(),
            force: true,
        },
        keyed.clone(),
    );
    assert_eq!(d.markers.len(), 1);
    assert_eq!(
        (d.markers[0].kind, d.markers[0].cause, d.markers[0].commit),
        (MKind::Cleared, Cause::BranchDelete, origin)
    );
    assert!(!s.st.markers.excluded(&s.st.dag, &main, Nid(1)));
    // A replay of the delete rebuilds its markers from its group ([API §7.5]).
    let again = s.run(
        Cmd::BranchDelete {
            name: "lane/b".into(),
            force: true,
        },
        keyed,
    );
    assert_eq!(again.outcome, Outcome::Replayed);
    assert_eq!(again.markers, d.markers);
}

/// The runtime snapshot's `exclusions` list pairs of a live ref and a task only ([API §15.7]): a note created and
/// deleted on a lane is held `deleted` there and absent on `main`, yet it is not a task, so no pair names it.
#[test]
fn runtime_exclusions_name_tasks_only() {
    let mut s = start();
    s.ok(
        tx(vec![node("n", "note", &[("title", t("scratch"))])]),
        admin_on("lane/a"),
    );
    let note = Nid(TASKS + 1);
    s.ok(
        tx(vec![Stmt::Delete {
            target: Target::Id(note),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("r".into()),
        }]),
        admin_on("lane/a"),
    );
    complete(&mut s, 1, "lane/a").expect("ready on lane/a");
    let mut o = Oracle::new(&s.st.dag, &s.st.alloc);
    assert!(
        o.i26p_excluded("main", note),
        "the definition holds the note deleted elsewhere"
    );
    let ex = s.st.runtime().exclusions;
    assert!(ex.iter().all(|(_, n, _)| *n != note), "{ex:?}");
    assert!(
        ex.contains(&("main".into(), Nid(1), vec!["lane/a".into()])),
        "{ex:?}"
    );
}

/// ME-011: after `undo` moves lane/a off a marker's origin, the next commit on lane/a flags the marker nonlinear, and
/// the exact test (AB-002) keeps `main` excluded after it merges nothing ([RULES/state-definition] S12).
#[test]
fn a_diverged_ref_flags_its_markers_nonlinear() {
    let mut s = start();
    complete(&mut s, 1, "lane/a").expect("ready");
    fork(&mut s, "lane/b", "lane/a");
    let tip = s.st.dag.live("lane/a").unwrap().tip.unwrap();
    // The completion is the claim-free commit of `complete`; undo steps back over it.
    let parent = s.st.dag.commits[&tip].parents[0];
    let moved =
        s.st.move_ref("lane/a", Some(parent), MoveReason::Undo, "orch");
    assert!(
        moved.is_empty(),
        "lane/b still holds: holders only ({moved:?})"
    );
    s.ok(
        tx(vec![set(2, &[("priority", P::Int(1))])]),
        admin_on("lane/a"),
    );
    assert!(row(&s, 1)[0].marker.nonlinear, "ME-011");
    agrees_with_definition(&s.st.dag, &s.st.alloc, (1..=TASKS).map(Nid), &s.st.markers).unwrap();
}
