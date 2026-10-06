//! Tests of the evaluator: a synthetic store shaped like the card's examples ([LQ/card §3]) and the ten mistakes'
//! store ([LQ/errors §7]: tasks #51, #88, #93 and a doc #130 on `main` with `lane/l5np`), the card's seven examples
//! run on it, and unit and property tests of the semantics of [50 §3].

mod props;
mod rels;
mod sem;
mod writes;

use crate::api::{Cmd, Ctx, Outcome, QueryInput, Reply, Store};
use crate::lq::ctx::{Params, Value as P};
use crate::lq::eval::{Ablations, Mode, QueryOut, V};
use crate::suite::{S, orch, tx};
use crate::tx::{Stmt, Target};
use crate::value::Nid;

/// The kind and title of `#i` in the fixture store's first commit.
fn spec(i: u32) -> (&'static str, String) {
    match i {
        9 => ("task", "Lock protocol epic".into()),
        12 => ("task", "Byte-range lock protocol".into()),
        14 => ("task", "Lock file format".into()),
        17 => ("task", "HEAD slot format".into()),
        20..=35 => ("task", format!("Filler task {i}")),
        51 => ("task", "Wire lease reclaim".into()),
        52 => ("task", "Lock protocol v2".into()),
        88 => ("task", "Narrowphase batching (L5)".into()),
        89 => ("task", "Narrowphase SoA layout".into()),
        90 => ("task", "Broadphase pair cache".into()),
        92 => ("task", "Contact manifold reuse".into()),
        93 => ("task", "Bench harness".into()),
        95 => ("task", "Contact cache eviction".into()),
        97 => ("task", "Solver warm start".into()),
        98 => ("task", "Pair cache invalidation".into()),
        130 => ("doc", "Physics design".into()),
        131 => ("doc", "Broadphase".into()),
        133 => ("doc", "Narrowphase".into()),
        136..=139 => ("finding", format!("Finding {i}")),
        140 => (
            "rule",
            "Never hold a World borrow across a system boundary".into(),
        ),
        141 => ("rule", "Never kill processes by image name".into()),
        142 => (
            "note",
            "Lease reclaim runs under the maintenance byte".into(),
        ),
        143 => ("decision", "Fencing tokens on every lease mutation".into()),
        _ => ("note", format!("Note {i}")),
    }
}

/// The parent of `#i`, if any.
fn parent(i: u32) -> Option<u32> {
    match i {
        12 | 14 | 17 => Some(9),
        89 | 90 | 92 | 93 | 95 | 97 => Some(88),
        98 => Some(90),
        131 | 133 => Some(130),
        _ => None,
    }
}

fn var(i: u32) -> String {
    format!("v{i}")
}

/// The fixture store: commit 1 creates `#1`–`#143` with their edges; six small commits on `main` follow; `lane/l5np`
/// forks; both sides change `#93.priority`; sixteen filler claims take L-2 … L-17 and dev#1 claims `#89` on the lane
/// (L-18, started).
fn build() -> S {
    let mut s = S::base();
    let mut stmts = Vec::new();
    for i in 1..=143u32 {
        let (kind, title) = spec(i);
        let mut fields = vec![("title".to_string(), P::Text(title))];
        match kind {
            "finding" => {
                fields.push(("failure_scenario".into(), P::Text("frames drop".into())));
                fields.push(("severity".into(), P::Text("important".into())));
                fields.push(("f_kind".into(), P::Text("perf".into())));
                fields.push(("round".into(), P::Int(if i < 138 { 1 } else { 2 })));
            }
            "rule" if i == 140 => {
                fields.push(("criticality".into(), P::Text("critical".into())));
                fields.push((
                    "applies_to".into(),
                    P::List(vec![P::Text("path:crates/ecs/**".into())]),
                ));
            }
            "rule" => fields.push(("criticality".into(), P::Text("critical".into()))),
            _ => {}
        }
        if i == 93 || i == 89 {
            fields.push(("labels".into(), P::List(vec![P::Text("l5".into())])));
        }
        stmts.push(Stmt::Create {
            name: Some(var(i)),
            kind: kind.into(),
            fields,
            body: None,
            under: parent(i).map(|p| Target::Var(var(p))),
            position: None,
            edges_out: Vec::new(),
            edges_in: Vec::new(),
        });
    }
    for (a, k, b) in [
        (12, "blocks", 51),
        (17, "blocks", 51),
        (14, "blocks", 17),
        (90, "blocks", 93),
        (136, "about", 133),
        (137, "about", 133),
        (138, "about", 133),
        (139, "about", 131),
        (133, "depends_on", 131),
    ] {
        stmts.push(Stmt::Link {
            src: Target::Var(var(a)),
            kind: k.into(),
            dst: Target::Var(var(b)),
            pinned: None,
        });
    }
    s.ok(tx(stmts), orch());
    // Six commits on main, so that `main~5` names a commit after commit 1.
    s.ok(
        tx(vec![crate::suite::set(
            136,
            &[("status", P::Text("refuted".into()))],
        )]),
        orch(),
    );
    s.ok(
        tx(vec![crate::suite::set(
            137,
            &[("status", P::Text("confirmed".into()))],
        )]),
        orch(),
    );
    for (n, t) in [(1, "One"), (2, "Two"), (3, "Three"), (4, "Four")] {
        s.ok(
            tx(vec![crate::suite::set(n, &[("title", P::Text(t.into()))])]),
            orch(),
        );
    }
    s.ok(
        Cmd::BranchCreate {
            name: "lane/l5np".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    s.ok(
        tx(vec![crate::suite::set(
            93,
            &[("priority", P::Text("P1".into()))],
        )]),
        crate::suite::orch_on("lane/l5np"),
    );
    s.ok(
        tx(vec![crate::suite::set(
            93,
            &[("priority", P::Text("P3".into()))],
        )]),
        orch(),
    );
    for i in 20..=35u32 {
        s.ok(
            Cmd::Claim {
                ids: vec![Target::Id(Nid(i))],
                next: false,
                scope: None,
                role: None,
                agent: None,
                ttl: Some(P::Text("12h".into())),
                start: false,
                run: None,
                session: false,
            },
            Ctx {
                agent: Some("dev#9".into()),
                ..Default::default()
            },
        );
    }
    let r = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(89))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: Some(P::Text("12h".into())),
            start: true,
            run: None,
            session: false,
        },
        Ctx {
            agent: Some("dev#1".into()),
            branch: Some("lane/l5np".into()),
            ..Default::default()
        },
    );
    assert!(
        r.yields[0].rows[0].contains(&("lease".into(), "L-18".into())),
        "{:?}",
        r.yields
    );
    s
}

thread_local! {
    /// The fixture store, built once per test thread (the model is single-threaded; each test clones it).
    static FIXTURE: S = build();
}

/// A fresh copy of the fixture store.
pub(super) fn store() -> Store {
    FIXTURE.with(|f| f.st.clone())
}

/// The context of an orchestrator query on `main`.
pub(super) fn ctx() -> Ctx {
    orch()
}

/// Runs a read on a store.
pub(super) fn q(st: &mut Store, text: &str) -> Reply {
    q_with(st, text, Params::new(), Ablations::default(), &ctx())
}

/// Runs a read with parameters, ablations and a context.
pub(super) fn q_with(st: &mut Store, text: &str, params: Params, ab: Ablations, c: &Ctx) -> Reply {
    st.run(
        &Cmd::Query {
            input: QueryInput::Lq(text.into()),
            params,
            at: None,
            mode: Mode::Run,
            strict_gql: false,
            ablations: ab,
        },
        c,
    )
}

/// The result of a read that must run.
pub(super) fn rows(st: &mut Store, text: &str) -> QueryOut {
    out(q(st, text), text)
}

/// The result of a reply that must hold one.
pub(super) fn out(r: Reply, text: &str) -> QueryOut {
    assert_eq!(r.outcome, Outcome::Ok, "{text}: {:?}", r.error);
    match r.data {
        crate::api::Data::Query(o) => *o,
        other => panic!("{text}: {other:?}"),
    }
}

/// The nodes of a column.
pub(super) fn col_nodes(o: &QueryOut, c: usize) -> Vec<u32> {
    o.rows
        .iter()
        .filter_map(|r| r.get(c).and_then(V::node).map(|n| n.0))
        .collect()
}

/// Runs a `TX` block in LQ text.
pub(super) fn tx_lq(st: &mut Store, text: &str, params: Params, c: &Ctx) -> Reply {
    st.run(
        &Cmd::TxLq {
            lq: text.into(),
            params,
            message: String::new(),
            if_targets: None,
        },
        c,
    )
}

/// The inputs of the card's examples ([LQ/card §3]): the `%% input` of each case of `fixtures/lq/cases/card.cases`.
fn card_inputs() -> Vec<(String, String, String)> {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/lq/cases/card.cases");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut out = Vec::new();
    let (mut name, mut entry) = (String::new(), String::new());
    let mut lines = text.lines();
    while let Some(l) = lines.next() {
        if let Some(n) = l.strip_prefix("%% case ") {
            name = n.to_string();
        } else if let Some(e) = l.strip_prefix("%% entry ") {
            entry = e.to_string();
        } else if l == "%% input" {
            let mut input = Vec::new();
            for x in lines.by_ref() {
                if x.starts_with("%% ") {
                    break;
                }
                input.push(x);
            }
            out.push((name.clone(), entry.clone(), input.join("\n")));
        }
    }
    out
}

/// The card's seven examples and the GQL spelling of example 2 run on the fixture store ([LQ/card §3]; PLAN WP-93b
/// acceptance): every read returns rows, example 7 commits.
#[test]
fn the_cards_examples_run() {
    let inputs = card_inputs();
    assert_eq!(
        inputs.len(),
        8,
        "card.cases holds the seven examples and the GQL variant"
    );
    for (name, entry, input) in inputs {
        let mut st = store();
        if entry == "write" {
            let r = tx_lq(
                &mut st,
                &input,
                Params::new(),
                &Ctx {
                    agent: Some("dev#1".into()),
                    ..Default::default()
                },
            );
            assert_eq!(r.outcome, Outcome::Ok, "{name}: {:?}", r.error);
            assert!(r.rev_new.is_some(), "{name} commits");
            let snap = st.snapshot("lane/l5np", st.dag.live("lane/l5np").unwrap().tip);
            let n89 = snap.nodes.iter().find(|x| x.id == Nid(89)).unwrap();
            assert_eq!(n89.node.status, "done", "{name}");
            continue;
        }
        let o = rows(&mut st, &input);
        assert!(!o.rows.is_empty(), "{name} returns rows: {input}");
        match name.as_str() {
            // #88 and #90 are containers, #89 is claimed and started, #93 is blocked by #90.
            "card-1" => assert_eq!(col_nodes(&o, 0), vec![92, 95, 97, 98]),
            "card-2" | "card-2-gql" => assert_eq!(col_nodes(&o, 0), vec![12, 14, 17]),
            "card-3" => assert_eq!(o.rows.len(), 2, "{:?}", o.rows),
            // #141 has no `applies_to`, which means `*` ([F08 §5.4.6]).
            "card-4" => assert_eq!(col_nodes(&o, 0), vec![140, 141]),
            "card-5" => assert_eq!(o.rows.len(), 1),
            "card-6" => assert!(
                o.rows.iter().any(|r| r[1] == V::Node(Nid(93))),
                "{:?}",
                o.rows
            ),
            other => panic!("unexpected case {other}"),
        }
    }
}

/// `%% outcome runs` of `fixtures/lq` ([fixtures/lq INDEX.md §2.3]): the text evaluates without error on the fixture
/// store — a read as the orchestrator on `main`, a `TX` as the orchestrator with its lease.
pub(crate) fn runs(text: &str, write: bool) -> Result<(), String> {
    let mut st = store();
    let r = if write {
        tx_lq(&mut st, text, Params::new(), &ctx())
    } else {
        q(&mut st, text)
    };
    match r.outcome {
        Outcome::Ok | Outcome::Dry | Outcome::Replayed => Ok(()),
        _ => Err(format!("does not run on the fixture store: {:?}", r.error)),
    }
}
