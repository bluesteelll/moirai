//! The I26′ worked scenarios of [RULES/state-definition] `scenarios` and `scenario-expect`, run through the Store API:
//! PD-012 on the check ref after each step, evaluated by the definition over all live refs ([F13 §4.1]), and the same
//! answer from the marker cache (AB-004; [F13 §4.2] MC-7). Every action is a Store API command: `undo`, `op-restore`,
//! `sync`, `merge`, `merge-abort` and `revert` are WP-91's history verbs ([`crate::history`]).

use super::*;
use crate::coord::Oracle;
use crate::markers::agrees_with_definition;
use crate::rules::rules;
use std::collections::BTreeMap;

struct Sc {
    s: S,
    task: Nid,
    other: Nid,
    /// The store's `commit_seq` before each step of the running scenario, for `op-restore`.
    seq_before: BTreeMap<u64, u64>,
    /// The tips of the live refs after each step, for `revert`.
    after: BTreeMap<u64, BTreeMap<String, Option<u64>>>,
}

fn setup() -> Sc {
    let mut s = S::base();
    s.ok(
        tx(vec![task("t89", "task 89"), task("t90", "task 90")]),
        orch(),
    );
    for lane in ["a", "b", "c"] {
        s.ok(
            Cmd::BranchCreate {
                name: lane.into(),
                from: Some("main".into()),
                kind: None,
            },
            orch(),
        );
    }
    Sc {
        s,
        task: Nid(1),
        other: Nid(2),
        seq_before: BTreeMap::new(),
        after: BTreeMap::new(),
    }
}

/// Runs one scenario action on `r`; `None` for a history verb of WP-91.
fn act(sc: &mut Sc, r: &str, action: &str) -> Option<()> {
    let parts: Vec<&str> = action.split(':').collect();
    let t89 = sc.task;
    match parts[0] {
        "complete" => {
            let c = sc.s.ok(
                Cmd::Claim {
                    ids: vec![Target::Id(t89)],
                    next: false,
                    scope: None,
                    role: None,
                    agent: None,
                    ttl: None,
                    start: false,
                    run: None,
                    session: false,
                },
                orch_on(r),
            );
            let lease = c.yields[0].rows[0]
                .iter()
                .find(|(k, _)| k == "lease")
                .map(|(_, v)| v.clone())
                .expect("a lease");
            sc.s.ok(
                Cmd::Complete {
                    id: Target::Id(t89),
                    outcome: "done".into(),
                    summary: "done".into(),
                    evidence: vec![],
                    move_lease: None,
                },
                Ctx {
                    lease: Some(lease),
                    ..Default::default()
                },
            );
        }
        "reopen" => {
            sc.s.ok(
                tx(vec![Stmt::Reopen {
                    target: Target::Id(t89),
                    reason: "again".into(),
                }]),
                orch_on(r),
            );
        }
        "set-done" => {
            sc.s.ok(tx(vec![set(t89.0, &[("done", P::Bool(true))])]), orch_on(r));
        }
        "set" => {
            let (f, v) = parts[2].split_once('=')?;
            let n = if parts[1] == "90" { sc.other } else { t89 };
            sc.s.ok(
                tx(vec![set(n.0, &[(f, P::Int(v.parse().ok()?))])]),
                orch_on(r),
            );
        }
        "rm" => {
            sc.s.ok(
                tx(vec![Stmt::Delete {
                    target: Target::Id(t89),
                    policy: None,
                    replaced_by: None,
                    release: false,
                    reason: Some("gone".into()),
                }]),
                orch_on(r),
            );
        }
        "branch" => {
            let name = parts[1];
            let from = parts[2].strip_prefix("from=")?;
            let kind = parts.get(3).and_then(|k| k.strip_prefix("kind=")).map(|k| {
                if k == "plan" {
                    crate::dag::RefKind::Plan
                } else {
                    crate::dag::RefKind::Work
                }
            });
            if sc.s.st.dag.live(name).is_some() {
                // A re-fork: the ref is deleted and forked again.
                sc.s.ok(
                    Cmd::BranchDelete {
                        name: name.into(),
                        force: true,
                    },
                    orch(),
                );
            }
            sc.s.ok(
                Cmd::BranchCreate {
                    name: name.into(),
                    from: Some(from.into()),
                    kind,
                },
                orch(),
            );
        }
        "branch-D" => {
            sc.s.ok(
                Cmd::BranchDelete {
                    name: parts[1].into(),
                    force: true,
                },
                orch(),
            );
        }
        "tx" => {
            assert_eq!(parts[1], "reopen-then-set-done");
            sc.s.ok(
                tx(vec![
                    Stmt::Reopen {
                        target: Target::Id(t89),
                        reason: "again".into(),
                    },
                    set(t89.0, &[("done", P::Bool(true))]),
                ]),
                orch_on(r),
            );
        }
        "undo" => {
            // One move back on the step's ref ([API §11.11]).
            sc.s.ok(
                Cmd::Undo {
                    ref_: Some(r.into()),
                    n: None,
                    expect: None,
                },
                orch(),
            );
        }
        "op-restore" => {
            // Every ref back to its value after the last commit before that step ([API §11.11]).
            let step: u64 = parts[1].split_once('-')?.1.parse().ok()?;
            let seq = *sc.seq_before.get(&step)?;
            sc.s.ok(Cmd::OpRestore { seq }, orch());
        }
        "sync" => {
            sc.s.ok(
                Cmd::Sync {
                    lane: Some(r.into()),
                    check: false,
                },
                orch(),
            );
        }
        "merge" => {
            let src = parts[1].to_string();
            if parts.get(2) == Some(&"stages") {
                // The merge stages: a FieldEdit on #90 under `--strict` (LS-002).
                let other = sc.other.0;
                sc.s.ok(
                    tx(vec![set(other, &[("priority", P::Int(1))])]),
                    orch_on(&src),
                );
                sc.s.ok(tx(vec![set(other, &[("priority", P::Int(3))])]), orch_on(r));
                let rep = sc.s.run(
                    Cmd::Merge {
                        src,
                        into: Some(r.into()),
                        policy: None,
                        strict: Some(true),
                        base: None,
                        message: String::new(),
                    },
                    orch(),
                );
                assert_eq!(rep.outcome, crate::api::Outcome::Staged, "{:?}", rep.error);
            } else {
                sc.s.ok(
                    Cmd::Merge {
                        src,
                        into: Some(r.into()),
                        policy: None,
                        strict: None,
                        base: None,
                        message: String::new(),
                    },
                    orch(),
                );
            }
        }
        "merge-abort" => {
            // The staging ref the merge wrote: `merge/<r>/from/<src>`, or its sync-first sync's `merge/<src>/from/main`
            // ([F12 §9.6]: a merge into `main` whose sync stages is staged by its sync).
            let src = parts[1];
            let (s, i) = if sc.s.st.dag.live(&format!("merge/{r}/from/{src}")).is_some() {
                (src.to_string(), r.to_string())
            } else {
                ("main".to_string(), src.to_string())
            };
            sc.s.ok(
                Cmd::MergeAbort {
                    src: Some(s),
                    into: Some(i),
                },
                orch(),
            );
        }
        "revert" => {
            // The commit the named step left on this ref.
            let step: u64 = parts[1].split_once('-')?.1.parse().ok()?;
            let c = sc.after.get(&step)?.get(r).copied().flatten()?;
            sc.s.ok(
                Cmd::Revert {
                    commit: format!("s{c}"),
                    onto: Some(r.into()),
                    mainline: None,
                    message: String::new(),
                },
                orch(),
            );
        }
        other => panic!("unknown scenario action {other}"),
    }
    Some(())
}

/// Runs every scenario; with `fold`, the checkpoint fold of ME-012 runs after every step (before its checks), and the
/// answers must not change ([RULES/state-definition] ME-013, open point 17). After every step the expected rows hold by
/// the definition and by the cache, and the cache equals the definition on every live ref (MC-7). Returns the
/// scenarios that ran to the end and those that stopped at a history verb of WP-91.
fn run_scenarios(fold: bool) -> (Vec<String>, Vec<String>) {
    let r = rules();
    let mut by: BTreeMap<String, Vec<(u64, String, String)>> = BTreeMap::new();
    for row in &r.table("scenarios").rows {
        by.entry(row.tok("scenario").to_string())
            .or_default()
            .push((
                row.int("step"),
                row.tok("ref").to_string(),
                row.tok("action").to_string(),
            ));
    }
    let mut ran = Vec::new();
    let mut later = Vec::new();
    for (name, mut steps) in by {
        steps.sort();
        let mut sc = setup();
        let mut complete = true;
        for (step, rf, action) in steps {
            sc.seq_before.insert(step, sc.s.st.commit_seq);
            if act(&mut sc, &rf, &action).is_none() {
                complete = false;
                break;
            }
            let tips =
                sc.s.st
                    .dag
                    .live_refs()
                    .map(|x| (x.name.clone(), x.tip))
                    .collect();
            sc.after.insert(step, tips);
            if fold {
                sc.s.st.markers.fold_inert(&sc.s.st.dag);
            }
            for x in r
                .table("scenario-expect")
                .rows
                .iter()
                .filter(|x| x.tok("scenario") == name && x.int("step") == step)
            {
                let mut o = Oracle::new(&sc.s.st.dag, &sc.s.st.alloc);
                let got = o.i26p_excluded(x.tok("check_ref"), sc.task);
                assert_eq!(
                    got,
                    x.tok("excluded") == "yes",
                    "{} ({name} step {step} on {}, fold {fold})",
                    x.id,
                    x.tok("check_ref")
                );
                let check =
                    sc.s.st
                        .dag
                        .live(x.tok("check_ref"))
                        .expect("a live check ref");
                assert_eq!(
                    sc.s.st.markers.excluded(&sc.s.st.dag, check, sc.task),
                    got,
                    "{}: the marker cache disagrees ({name} step {step}, fold {fold})",
                    x.id
                );
            }
            if let Err(e) = agrees_with_definition(
                &sc.s.st.dag,
                &sc.s.st.alloc,
                [sc.task, sc.other],
                &sc.s.st.markers,
            ) {
                panic!("{name} step {step} ({action}), fold {fold}: {e}");
            }
        }
        if complete {
            ran.push(name);
        } else {
            later.push(name);
        }
    }
    (ran, later)
}

#[test]
fn i26_scenarios_hold() {
    let (ran, later) = run_scenarios(false);
    assert_eq!(ran.len(), 14, "every scenario runs to its end: {ran:?}");
    assert!(later.is_empty(), "{later:?}");
}

/// The same scenarios with the checkpoint fold after every step: the answers do not change (ME-012, ME-013; S1 step 3
/// re-emits a folded `cleared` marker).
#[test]
fn i26_scenarios_hold_under_the_fold() {
    let (ran, _) = run_scenarios(true);
    assert_eq!(ran.len(), 14);
}
