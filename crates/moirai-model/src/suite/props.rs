//! Random streams through the Store API ([60 §3.13] GT18's shape on the model): after every command every live head
//! satisfies I12 ([F13 §3.3]: I2, I4, I5′, I6, I7, I8, I11), a refusal writes nothing, the allocation stays a bijection
//! (I1, I35′), every commit's `affected` holds `D(c)` (I42′), the fold of every commit's net changeset equals its
//! candidate (checked as each commit is appended), and the marker cache equals I26′'s definition (MC-7).

use super::*;
use crate::clock::EnvClock;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Create(usize, Option<u32>),
    Status(u32, usize),
    Done(u32),
    Link(u32, usize, u32),
    Unlink(u32, usize, u32),
    Move(u32, Option<u32>),
    Reopen(u32),
    Delete(u32, usize, Option<u32>, bool),
    Claim(u32),
    Complete(u32, usize),
    Branch(usize),
    Switch(usize),
    DropBranch(usize),
    Clock(u64),
}

const KINDS: [&str; 5] = ["task", "note", "question", "decision", "verdict"];
const STATUSES: [&str; 8] = [
    "open",
    "in_progress",
    "done",
    "cancelled",
    "deferred",
    "answered",
    "accepted",
    "superseded",
];
const EDGES: [&str; 6] = [
    "blocks",
    "gates",
    "relates",
    "answers",
    "supersedes",
    "derived_from",
];
const LANES: [&str; 3] = ["main", "lane/a", "lane/b"];
const OUTCOMES: [&str; 3] = ["done", "failed", "abandoned"];

fn op() -> impl Strategy<Value = Op> {
    let id = 1u32..9;
    prop_oneof![
        3 => (0..KINDS.len(), proptest::option::of(id.clone())).prop_map(|(k, p)| Op::Create(k, p)),
        1 => (id.clone(), 0..STATUSES.len()).prop_map(|(n, s)| Op::Status(n, s)),
        1 => id.clone().prop_map(Op::Done),
        2 => (id.clone(), 0..EDGES.len(), id.clone()).prop_map(|(a, k, b)| Op::Link(a, k, b)),
        1 => (id.clone(), 0..EDGES.len(), id.clone()).prop_map(|(a, k, b)| Op::Unlink(a, k, b)),
        1 => (id.clone(), proptest::option::of(id.clone())).prop_map(|(n, p)| Op::Move(n, p)),
        1 => id.clone().prop_map(Op::Reopen),
        1 => (id.clone(), 0..3usize, proptest::option::of(id.clone()), any::<bool>()).prop_map(|(n, p, y, r)| Op::Delete(n, p, y, r)),
        1 => id.clone().prop_map(Op::Claim),
        1 => (id, 0..OUTCOMES.len()).prop_map(|(n, o)| Op::Complete(n, o)),
        1 => (1..3usize).prop_map(Op::Branch),
        1 => (0..3usize).prop_map(Op::Switch),
        1 => (1..3usize).prop_map(Op::DropBranch),
        1 => (0u64..120_000).prop_map(Op::Clock),
    ]
}

/// What a refusal must leave unchanged: counters, live leases, refs and idempotency entries.
type Print = (u64, u32, u64, usize, Vec<(u32, Option<u64>, bool)>, usize);

/// The fingerprint of a store.
fn fingerprint(s: &Store) -> Print {
    (
        s.commit_seq,
        s.next_id,
        s.fence,
        s.leases.values().filter(|l| l.ended.is_none()).count(),
        s.dag
            .refs
            .values()
            .map(|r| (r.id, r.tip, r.deleted))
            .collect(),
        s.idem.entries.len(),
    )
}

fn check(s: &Store) -> Result<(), TestCaseError> {
    for r in s.dag.live_refs() {
        let st = s.dag.state_at(r.tip, &s.alloc);
        if let Err(e) = crate::inv::i12_heads_valid(&st) {
            return Err(TestCaseError::fail(format!("I12 on {}: {e}", r.name)));
        }
    }
    prop_assert_eq!(
        s.alloc.rows.len(),
        s.alloc.uidx.len(),
        "I1: #N and uid are a bijection"
    );
    prop_assert_eq!(s.alloc.rows.len() as u32 + 1, s.next_id);
    // MC-7: the marker cache and the absorbed vectors equal their definitions.
    if let Err(e) = crate::markers::agrees_with_definition(
        &s.dag,
        &s.alloc,
        s.alloc.rows.keys().copied().collect::<Vec<_>>(),
        &s.markers,
    ) {
        return Err(TestCaseError::fail(e));
    }
    Ok(())
}

/// The random streams: every case keeps the invariants and the op expectations; over all cases at least a quarter
/// of the writes land, so a model that refuses valid writes fails.
#[test]
fn random_streams_keep_the_invariants() {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 48,
        ..ProptestConfig::default()
    });
    let totals = std::cell::Cell::new((0usize, 0usize));
    let result = runner.run(&proptest::collection::vec(op(), 1..28), |ops| {
        stream(ops, &totals)
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    let (ran, accepted) = totals.get();
    assert!(
        accepted * 4 >= ran,
        "only {accepted} of {ran} writes were accepted"
    );
}

fn stream(ops: Vec<Op>, totals: &std::cell::Cell<(usize, usize)>) -> Result<(), TestCaseError> {
    {
        let mut s = S::base();
        // Nodes #1 to #8 of mixed kinds, so the random ops meet allocated ids.
        let seed: Vec<Stmt> = [
            "task", "task", "task", "question", "decision", "verdict", "note", "task",
        ]
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let mut f = vec![("title", t(&format!("n{i}")))];
            if *k == "verdict" {
                f.push(("outcome", t("fail_fixable")));
            }
            node(&format!("s{i}"), k, &f)
        })
        .collect();
        s.ok(tx(seed), orch());
        let mut on = 0usize;
        let mut leases: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
        let (mut ran, mut accepted) = (0usize, 0usize);
        for o in ops {
            let branch = LANES[on].to_string();
            let before = fingerprint(&s.st);
            // What the op must do, where it is decided without the model: a create without a parent by the
            // orchestrator is accepted; a `relates` edge joins any two distinct live nodes; a self-edge is E405.
            let tip_state =
                s.st.dag
                    .state_at(s.st.dag.live(&branch).and_then(|r| r.tip), &s.st.alloc);
            let live = |n: u32| tip_state.live(Nid(n)).is_some();
            let expect_ok = match o {
                Op::Create(_, None) => true,
                Op::Link(a, 2, b) => a != b && live(a) && live(b),
                _ => false,
            };
            let expect_e405 = matches!(o, Op::Link(a, 2, b) if a == b && live(a));
            drop(tip_state);
            let writes = !matches!(o, Op::Clock(_));
            let shown = format!("{o:?}");
            let (cmd, ctx) = match o {
                Op::Create(k, p) => {
                    let mut fields = vec![("title".to_string(), t("n"))];
                    match KINDS[k] {
                        "verdict" => fields.push(("outcome".into(), t("fail_fixable"))),
                        "question" => fields.push(("answer".into(), t("yes"))),
                        _ => {}
                    }
                    (
                        tx(vec![Stmt::Create {
                            name: None,
                            kind: KINDS[k].into(),
                            fields,
                            body: None,
                            under: p.map(|p| Target::Id(Nid(p))),
                            position: None,
                            edges_out: vec![],
                            edges_in: vec![],
                        }]),
                        orch_on(&branch),
                    )
                }
                Op::Status(n, st) => (
                    tx(vec![set(n, &[("status", t(STATUSES[st]))])]),
                    orch_on(&branch),
                ),
                Op::Done(n) => (
                    tx(vec![set(n, &[("done", P::Bool(true))])]),
                    orch_on(&branch),
                ),
                Op::Link(a, k, b) => (tx(vec![link(a, EDGES[k], b)]), orch_on(&branch)),
                Op::Unlink(a, k, b) => (
                    tx(vec![Stmt::Unlink {
                        src: Target::Id(Nid(a)),
                        kind: EDGES[k].into(),
                        dst: Target::Id(Nid(b)),
                    }]),
                    orch_on(&branch),
                ),
                Op::Move(n, p) => (
                    tx(vec![Stmt::Move {
                        target: Target::Id(Nid(n)),
                        under: p.map(|p| Target::Id(Nid(p))),
                        position: None,
                    }]),
                    orch_on(&branch),
                ),
                Op::Reopen(n) => (
                    tx(vec![Stmt::Reopen {
                        target: Target::Id(Nid(n)),
                        reason: "again".into(),
                    }]),
                    orch_on(&branch),
                ),
                Op::Delete(n, p, y, release) => (
                    tx(vec![Stmt::Delete {
                        target: Target::Id(Nid(n)),
                        policy: [
                            None,
                            Some("cascade".to_string()),
                            Some("reparent".to_string()),
                        ][p]
                            .clone(),
                        replaced_by: y.map(|y| Target::Id(Nid(y))),
                        release,
                        reason: Some("r".into()),
                    }]),
                    orch_on(&branch),
                ),
                Op::Claim(n) => (
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
                    },
                    orch_on(&branch),
                ),
                Op::Complete(n, o) => match leases.get(&n) {
                    Some(l) => (
                        Cmd::Complete {
                            id: Target::Id(Nid(n)),
                            outcome: OUTCOMES[o].into(),
                            summary: "s".into(),
                            evidence: vec![],
                            move_lease: None,
                        },
                        Ctx {
                            lease: Some(l.clone()),
                            ..Default::default()
                        },
                    ),
                    None => continue,
                },
                Op::Branch(l) => (
                    Cmd::BranchCreate {
                        name: LANES[l].into(),
                        from: Some("main".into()),
                        kind: None,
                    },
                    orch(),
                ),
                Op::Switch(l) => {
                    if s.st.dag.live(LANES[l]).is_some() {
                        on = l;
                    }
                    continue;
                }
                Op::DropBranch(l) => {
                    if on == l {
                        on = 0;
                    }
                    (
                        Cmd::BranchDelete {
                            name: LANES[l].into(),
                            force: true,
                        },
                        orch(),
                    )
                }
                Op::Clock(ms) => (
                    Cmd::EnvClock(EnvClock {
                        advance_ms: Some(ms),
                        ..Default::default()
                    }),
                    Ctx::default(),
                ),
            };
            let r = s.run(cmd, ctx);
            if writes {
                ran += 1;
                if r.outcome != Outcome::Refused {
                    accepted += 1;
                }
            }
            if expect_ok {
                // The same op on another branch under the same default key is E408 ([API §7.4] row 5).
                let e408 = r.error.as_ref().is_some_and(|e| e.code == "E408");
                prop_assert!(
                    r.outcome != Outcome::Refused || e408,
                    "{} was refused: {:?}",
                    shown,
                    r.error
                );
            }
            if expect_e405 {
                prop_assert_eq!(
                    r.error.as_ref().map(|e| e.code.as_str()),
                    Some("E405"),
                    "{}",
                    shown
                );
            }
            if r.outcome == Outcome::Refused {
                prop_assert_eq!(
                    &before,
                    &fingerprint(&s.st),
                    "a refusal wrote: {:?}",
                    r.error
                );
            } else if let Some(y) = r.yields.iter().find(|y| y.proc == "tx.claim") {
                for row in &y.rows {
                    let lease = row
                        .iter()
                        .find(|(k, _)| k == "lease")
                        .map(|(_, v)| v.clone());
                    let task = row
                        .iter()
                        .find(|(k, _)| k == "task")
                        .map(|(_, v)| v.clone());
                    if let (Some(l), Some(t)) = (lease, task)
                        && let Some(Target::Id(n)) = crate::tx::parse_node(&t)
                    {
                        leases.insert(n.0, l);
                    }
                }
            }
            if let Some(seq) = r.rev_new.filter(|_| r.outcome == Outcome::Ok) {
                let c = &s.st.dag.commits[&seq];
                let parent = s.st.dag.state_at(c.parents.first().copied(), &s.st.alloc);
                let child = s.st.dag.state_at(Some(seq), &s.st.alloc);
                // The cached state of the new commit is the fold of every changeset from the root.
                prop_assert!(
                    *child == s.st.dag.state_from_scratch(Some(seq), &s.st.alloc),
                    "the fold of s{} differs",
                    seq
                );
                // I42′ against an oracle of its own: every node whose open-blocker count or container flag changed,
                // computed here by hand from the edges, is in the commit's affected list.
                if c.affected_complete {
                    for (n, before) in naive(&parent) {
                        let after = naive(&child).get(&n).copied();
                        if Some(before) != after {
                            prop_assert!(
                                c.affected.contains(&n),
                                "{} changed but is not in affected",
                                n
                            );
                        }
                    }
                    for n in naive(&child).keys() {
                        if !naive(&parent).contains_key(n) {
                            prop_assert!(
                                c.affected.contains(n),
                                "{} became live but is not in affected",
                                n
                            );
                        }
                    }
                }
            }
            check(&s.st)?;
        }
        let (r0, a0) = totals.get();
        totals.set((r0 + ran, a0 + accepted));
    }
    Ok(())
}

/// A hand-written oracle of two `P_F15` values of every live node, independent of [`crate::derived`]: the number of
/// `blocks` in-edges from a flagged edge, a live unfinished task or a live question with no live `answers` in-edge
/// from a live decision or note ([RULES/state-definition] BT-001 to BT-004), and whether a live node has it as its
/// parent.
fn naive(st: &crate::state::State) -> std::collections::BTreeMap<Nid, (u32, bool)> {
    let answered = |q: Nid| {
        st.nodes.iter().any(|(m, x)| {
            x.live()
                && (x.kind == "decision" || x.kind == "note")
                && x.out.keys().any(|k| k.kind == "answers" && k.dst == q)
                && *m != q
        })
    };
    let mut out = std::collections::BTreeMap::new();
    for (n, x) in &st.nodes {
        if !x.live() {
            continue;
        }
        let mut blockers = 0;
        for (m, y) in &st.nodes {
            for (k, p) in &y.out {
                if k.kind != "blocks" || k.dst != *n {
                    continue;
                }
                let open = p.flagged
                    || (y.live()
                        && ((y.kind == "task" && y.status != "done" && y.status != "cancelled")
                            || (y.kind == "question" && !answered(*m))));
                if open {
                    blockers += 1;
                }
            }
        }
        let container = st.nodes.values().any(|y| y.live() && y.parent == Some(*n));
        out.insert(*n, (blockers, container));
    }
    out
}
