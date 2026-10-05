//! Lanes through the Store API ([API §11.5] `LaneOpen`, `LaneClose`): [API §19] example 07 — a lane opened on its
//! designated tree, a claim and a completion on it, a sync-first merge into `main`, and the lane closed through the
//! door `lane-close` — with the computed values the example's golden states, and the doors of `LaneClose`
//! ([RULES/status-machines] DR-013, TR-072 to TR-080).

use super::*;
use crate::api::Data;
use crate::links::{EnvGitCommit, EnvHead};
use crate::r4::tree::TreeOp;
use crate::value::{Algo, Value};

const TREE: &str = "C:/work/moirai-l1";
const GIT: &str = "sha1:1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a";

/// The stream of example 02 through `n` = 4 (commit 1 on `main`: #1, its children #2 and #3, `#2 -blocks-> #3`),
/// then example 07's `EnvTree` and `EnvGit` (steps 5 and 6).
fn with_tree() -> S {
    let mut s = S::base();
    let mut ctx = orch();
    ctx.key = Some("plan-api".into());
    s.ok(
        tx(vec![
            node(
                "api",
                "task",
                &[("title", t("Ship the Store API")), ("priority", t("P1"))],
            ),
            child("ch", "Write the chapter", Target::Var("api".into())),
            child("ex", "Write the examples", Target::Var("api".into())),
            Stmt::Link {
                src: Target::Var("ch".into()),
                kind: "blocks".into(),
                dst: Target::Var("ex".into()),
                pinned: None,
            },
        ]),
        ctx,
    );
    s.ok(
        Cmd::EnvTree {
            tree: TREE.into(),
            volume: Some("C".into()),
            caps: None,
            ops: vec![TreeOp::Write {
                path: "README.md".into(),
                bytes: b"lane l1\n".to_vec(),
                btime_ns: None,
            }],
        },
        Ctx::default(),
    );
    s.ok(
        Cmd::EnvGit {
            repo: "moirai".into(),
            algo: Some(Algo::Sha1),
            commits: vec![EnvGitCommit {
                id: GIT.into(),
                parents: vec![],
                committer_time: 1_789_990_000,
                author_time: 1_789_990_000,
                tree: [(
                    "README.md".to_string(),
                    "sha1:2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b".to_string(),
                )]
                .into_iter()
                .collect(),
            }],
            refs: vec![("refs/heads/l1".into(), Some(GIT.into()))],
            heads: vec![(TREE.into(), EnvHead::Ref("refs/heads/l1".into()))],
        },
        Ctx::default(),
    );
    s
}

fn lane_open() -> Cmd {
    Cmd::LaneOpen {
        name: "l1".into(),
        worktree: TREE.into(),
        git_branch: Some("l1".into()),
        base: Some("1a1a1a1".into()),
    }
}

/// [API §19] example 07 with the values its golden computes (commit ids and budgets are illustrative there).
#[test]
fn example_07_lane_merge() {
    let mut s = with_tree();
    // Step 7: `LaneOpen` — the lane node's commit on main first (seq 2), then lane/l1 forked from it, then the
    // designated binding ([API §11.5], spec sync 2b).
    let r = s.ok(lane_open(), orch());
    assert_eq!(
        (r.branch.as_deref(), r.rev, r.commit),
        (Some("main"), Some(1), Some(2))
    );
    let Data::LaneOpen(d) = &r.data else {
        panic!("{:?}", r.data)
    };
    assert_eq!(
        (d.lane, d.ref_.as_str(), d.ref_id, d.fork),
        (Nid(4), "lane/l1", 1, 2)
    );
    let b = &d.binding;
    assert_eq!(
        (
            b.dir.as_str(),
            b.ref_.as_deref(),
            b.designated,
            b.expected_ref.as_deref(),
            b.base.as_deref()
        ),
        (TREE, Some("lane/l1"), true, Some("l1"), Some(GIT))
    );
    assert!(b.replaced.is_empty());
    let main = s.st.dag.state_at(Some(2), &s.st.alloc);
    let lane = &main.nodes[&Nid(4)];
    assert_eq!(
        (lane.kind.as_str(), lane.status.as_str()),
        ("lane", "active")
    );
    assert_eq!(lane.text("moirai_branch"), Some("lane/l1"));
    assert_eq!(lane.text("git_branch"), Some("l1"));
    assert!(
        matches!(lane.fields.get("worktree_path"), Some(Value::Path(p)) if p.root == "abs" && p.text == TREE)
    );
    assert!(matches!(lane.fields.get("base_sha"), Some(Value::Oid(o)) if o.algo == Algo::Sha1));
    assert_eq!(s.st.dag.live("lane/l1").and_then(|x| x.tip), Some(2));
    let c = &s.st.dag.commits[&2];
    assert_eq!(
        (c.stmt_origin, c.stmt_sym.as_deref()),
        ("verb", Some("lane open"))
    );
    // Step 8: dev1 claims #2 on lane/l1 (seq 3), lease L-2.
    let r = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: None,
            agent: Some("dev1".into()),
            ttl: None,
            start: true,
            run: None,
            session: false,
        },
        Ctx {
            branch: Some("lane/l1".into()),
            agent: Some("dev1".into()),
            client: Some("claude".into()),
            ..Default::default()
        },
    );
    assert_eq!(
        (r.branch.as_deref(), r.rev, r.commit),
        (Some("lane/l1"), Some(2), Some(3))
    );
    let lease = r.yields[0].rows[0]
        .iter()
        .find(|(k, _)| k == "lease")
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_eq!(lease, "L-2");
    // Step 9: complete #2 (seq 4): #3 becomes ready on the lane; the `settled` marker's origin is lane/l1, seq 4.
    let r = s.ok(
        Cmd::Complete {
            id: Target::Id(Nid(2)),
            outcome: "done".into(),
            summary: "chapter written".into(),
            evidence: vec![],
            move_lease: None,
        },
        Ctx {
            lease: Some(lease),
            client: Some("claude".into()),
            ..Default::default()
        },
    );
    assert_eq!((r.rev, r.commit), (Some(3), Some(4)));
    assert_eq!(r.ready, vec![Nid(3)]);
    assert_eq!(
        r.markers
            .iter()
            .map(|m| (m.id, m.ref_.as_str(), m.commit))
            .collect::<Vec<_>>(),
        vec![(Nid(2), "lane/l1", 4)]
    );
    // Step 10: a review task under #1 on main (seq 5).
    let r = s.ok(
        Cmd::Tx {
            stmts: vec![child("rv", "Review the examples", Target::Id(Nid(1)))],
            message: "add the review task".into(),
        },
        orch(),
    );
    assert_eq!(
        (r.branch.as_deref(), r.rev, r.commit),
        (Some("main"), Some(2), Some(5))
    );
    // Step 11: the sync-first merge: the sync on lane/l1 (seq 6), then the merge on main (seq 7), lca {5}.
    let r = s.ok(
        Cmd::Merge {
            src: "lane/l1".into(),
            into: Some("main".into()),
            policy: None,
            strict: None,
            base: None,
            message: String::new(),
        },
        orch(),
    );
    assert_eq!((r.rev, r.commit), (Some(5), Some(7)));
    let Data::Merge(d) = &r.data else {
        panic!("{:?}", r.data)
    };
    assert_eq!(d.outcome, "landed");
    let sync = d.sync.as_ref().unwrap();
    assert_eq!((sync.commit, sync.outcome), (Some(6), "landed"));
    assert_eq!(d.lca, vec![5]);
    assert!(!d.virtual_base && d.conflicts.is_empty() && d.violations.is_empty());
    assert_eq!(d.absorbed.get("lane/l1"), Some(&3));
    assert_eq!(d.affected, vec![Nid(1), Nid(2), Nid(3)]);
    // The merge commit takes the lane's hold of #2 (origin-rules OR-005: its origin stays lane/l1's seq 4) and main
    // joins that marker's holders (ME-002), an entry results do not list ([API §10.8]).
    assert!(d.markers.is_empty(), "{:?}", d.markers);
    // Step 12: `LaneClose` (seq 8): main contains the lane's tip, so the lane goes `active` → `merged` through the door
    // `lane-close` (TR-072), and its binding is removed.
    let r = s.ok(
        Cmd::LaneClose {
            name: "l1".into(),
            mode: None,
        },
        orch(),
    );
    assert_eq!(
        (r.branch.as_deref(), r.rev, r.commit),
        (Some("main"), Some(7), Some(8))
    );
    assert_eq!(
        r.data,
        Data::LaneClose(Nid(4), "merged".into(), Some(TREE.into()))
    );
    assert_eq!(s.st.dag.commits[&8].stmt_sym.as_deref(), Some("lane close"));
    assert!(
        s.st.heads
            .get(crate::heads::HeadKind::Directory, TREE)
            .is_none(),
        "the lane's binding is removed"
    );
    s.st.dag.verify_ids(&s.st.alloc).unwrap();
}

/// A keyed `LaneOpen` retried replays its data from the lane node's commit and the tree's binding ([API §7.5]).
#[test]
fn a_keyed_lane_open_replays() {
    let mut s = with_tree();
    let ctx = Ctx {
        key: Some("open-l1".into()),
        ..orch()
    };
    let a = s.ok(lane_open(), ctx.clone());
    let n = s.st.commit_seq;
    let b = s.ok(lane_open(), ctx);
    assert_eq!(b.outcome, crate::api::Outcome::Replayed);
    assert_eq!(s.st.commit_seq, n);
    assert_eq!((b.commit, &b.data), (a.commit, &a.data));
}

/// `LaneClose` ([API §11.5]; [RULES/status-machines] DR-013, TR-075, TR-078): a lane `main` has not absorbed closes to
/// `abandoned`; `freeze` moves it to `frozen`; a lane `set-status` cannot reach `merged` from `active` directly, and a
/// mode other than `close` or `freeze` is usage.
#[test]
fn lane_close_picks_its_target() {
    let mut s = with_tree();
    s.ok(lane_open(), orch());
    s.ok(
        tx(vec![set(1, &[("priority", t("P2"))])]),
        orch_on("lane/l1"),
    );
    let set_merged = tx(vec![set(4, &[("status", t("merged"))])]);
    s.refused(set_merged, orch(), "E404");
    s.refused(
        Cmd::LaneClose {
            name: "l1".into(),
            mode: Some("later".into()),
        },
        orch(),
        "usage",
    );
    let mut frozen = S { st: s.st.clone() };
    let r = frozen.ok(
        Cmd::LaneClose {
            name: "l1".into(),
            mode: Some("freeze".into()),
        },
        orch(),
    );
    assert!(matches!(&r.data, Data::LaneClose(_, st, _) if st == "frozen"));
    let r = s.ok(
        Cmd::LaneClose {
            name: "l1".into(),
            mode: None,
        },
        orch(),
    );
    assert!(matches!(&r.data, Data::LaneClose(_, st, _) if st == "abandoned"));
    // A lane `main` has no node of is `not_found`.
    s.refused(
        Cmd::LaneClose {
            name: "nosuch".into(),
            mode: None,
        },
        orch(),
        "not_found",
    );
}
