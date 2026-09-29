//! `Apply` ([API §9.4]): a run's results ingested as one block, keyed `run:<run>`, with every run-scoped lease of the
//! batch released (LE-003) and carried findings deduplicated ([90 §7.2]).

use super::*;
use crate::api::Data;
use crate::apply::{FindingV1, ResultV1};
use crate::markers::MKind;

/// A run `r1` with #1 and #2 claimed for workers under run-scoped leases.
fn dispatched() -> (S, String, String) {
    let mut s = S::base();
    s.ok(tx(vec![task("a", "A"), task("b", "B")]), orch());
    s.ok(
        Cmd::RunOpen {
            name: "r1".into(),
            fields: vec![],
        },
        orch(),
    );
    let claim = |s: &mut S, n: u32, w: &str| {
        let r = s.ok(
            Cmd::Claim {
                ids: vec![Target::Id(Nid(n))],
                next: false,
                scope: None,
                role: Some("developer".into()),
                agent: Some(w.into()),
                ttl: Some(t("run")),
                start: false,
                run: Some("r1".into()),
                session: false,
            },
            orch(),
        );
        r.yields[0].rows[0]
            .iter()
            .find(|(k, _)| k == "lease")
            .unwrap()
            .1
            .clone()
    };
    let l1 = claim(&mut s, 1, "w1");
    let l2 = claim(&mut s, 2, "w2");
    (s, l1, l2)
}

fn result(task: u32, lease: &str, outcome: &str, findings: Vec<FindingV1>) -> ResultV1 {
    ResultV1 {
        task: Some(task),
        lease: lease.into(),
        outcome: outcome.into(),
        summary: "s".into(),
        evidence: vec![],
        recorded: vec![task],
        findings,
        notes: vec![],
    }
}

#[test]
fn apply_completes_releases_and_dedups_in_one_block() {
    let (mut s, l1, l2) = dispatched();
    let f = FindingV1 {
        title: "leak".into(),
        severity: "important".into(),
        failure_scenario: "grows".into(),
        about: vec![1],
    };
    let cmd = Cmd::Apply {
        run: Some("r1".into()),
        results: vec![
            result(1, &l1, "done", vec![f.clone(), f.clone()]),
            result(2, &l2, "none", vec![]),
        ],
        stmts: vec![],
        message: "run r1".into(),
    };
    let r = s.ok(cmd.clone(), orch());
    let Data::Apply(d) = &r.data else {
        panic!("{:?}", r.data)
    };
    assert_eq!(d.entries.len(), 2);
    assert!(d.entries[0].completed && !d.entries[1].completed);
    assert_eq!(d.created.len(), 1, "the duplicate finding lands once");
    let mut released = vec![
        crate::tx::parse_lease(&l1).unwrap(),
        crate::tx::parse_lease(&l2).unwrap(),
    ];
    released.sort();
    assert_eq!(d.released, released);
    assert_eq!(d.recorded, vec![Nid(1), Nid(2)]);
    let l2n = crate::tx::parse_lease(&l2).unwrap();
    assert_eq!(
        s.st.leases[&l2n].ended,
        Some(crate::lease::EndReason::Apply),
        "LE-003"
    );
    assert!(r.markers.iter().any(|m| m.kind == MKind::Settled
        && m.id == Nid(1)
        && m.outcome.as_deref() == Some("done")));
    let c = &s.st.dag.commits[&r.rev_new.unwrap()];
    assert_eq!(
        (c.stmt_origin, c.stmt_sym.as_deref()),
        ("tx", Some("apply"))
    );
    // Keyed once per run: the same batch replays.
    assert_eq!(s.run(cmd, orch()).outcome, Outcome::Replayed);
}

#[test]
fn a_lease_of_another_run_refuses_the_batch() {
    let (mut s, _, _) = dispatched();
    let r = s.refused(
        Cmd::Apply {
            run: Some("r1".into()),
            results: vec![result(1, "L-1", "done", vec![])],
            stmts: vec![],
            message: String::new(),
        },
        orch(),
        "E407",
    );
    assert_eq!(r.exit, 5);
    assert_eq!(s.st.commit_seq, 2, "nothing written");
}
