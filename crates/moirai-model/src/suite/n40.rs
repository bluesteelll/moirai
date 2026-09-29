//! The node-40 table of [AR §5d.3] as the owner-signed fixture rows of [RULES/delete-policy-matrix] `n40-nodes`,
//! `n40-edges`, `n40-cases` and `n40-expect`, run through the Store API. The cases whose action is a merge, a revert or
//! an image import need WP-91 and M5; the suite checks that exactly those are left for them.

use super::*;
use crate::coord::{self, Oracle};
use crate::derived::{self, Index};
use crate::lease::{AnchorKind, Lease, LeaseKind};
use crate::rules::rules;
use crate::state::EdgeKey;
use std::collections::BTreeMap;

/// A store in a node-40 state, with the fixture's ids mapped to the allocated `#N`s.
struct N40 {
    s: S,
    ids: BTreeMap<String, Nid>,
    lease19: Option<u64>,
}

impl N40 {
    fn id(&self, fixture: &str) -> Nid {
        *self
            .ids
            .get(fixture)
            .unwrap_or_else(|| panic!("no fixture node {fixture}"))
    }
}

/// `c0`, optionally with `edges.blocks.on-src-deleted = drop-notify`.
fn c0(drop_notify: bool) -> N40 {
    let mut s = S::base();
    if drop_notify {
        s.st.cfg.edge_policies.blocks = "drop-notify".into();
    }
    let r = rules();
    let mut stmts = Vec::new();
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for (i, row) in r.table("n40-nodes").rows.iter().enumerate() {
        let v = format!("v{i}");
        vars.insert(row.tok("node").to_string(), v.clone());
        let under =
            (row.tok("parent") != "-").then(|| Target::Var(vars[row.tok("parent")].clone()));
        stmts.push(Stmt::Create {
            name: Some(v),
            kind: row.tok("kind").into(),
            fields: vec![("title".into(), t(&format!("node {}", row.tok("node"))))],
            body: None,
            under,
            position: None,
            edges_out: vec![],
            edges_in: vec![],
        });
    }
    s.ok(tx(stmts), orch());
    let ids: BTreeMap<String, Nid> = r
        .table("n40-nodes")
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| (row.tok("node").to_string(), Nid(i as u32 + 1)))
        .collect();
    let mut stmts = Vec::new();
    for row in &r.table("n40-edges").rows {
        let (a, b) = (ids[row.tok("src")], ids[row.tok("dst")]);
        match row.tok("kind") {
            "parent" => {}
            "mentions" => stmts.push(Stmt::Set {
                target: Target::Id(a),
                fields: vec![],
                incr: vec![],
                body: Some(Some(format!("see {b}"))),
                guard: None,
            }),
            k => stmts.push(Stmt::Link {
                src: Target::Id(a),
                kind: k.into(),
                dst: Target::Id(b),
                pinned: None,
            }),
        }
    }
    s.ok(tx(stmts), orch());
    for lane in ["x", "y"] {
        s.ok(
            Cmd::BranchCreate {
                name: lane.into(),
                from: Some("main".into()),
                kind: None,
            },
            orch(),
        );
    }
    N40 {
        s,
        ids,
        lease19: None,
    }
}

/// The state a case starts from (`n40-cases` `after`).
fn start(after: &str) -> Option<N40> {
    match after {
        "c0" => Some(c0(false)),
        "c0/drop-notify" => Some(c0(true)),
        "c0/lease-L-19" => {
            let mut x = c0(false);
            // The fixture's lease on #40, held by dev#2 on lane/y (a runtime row of the starting state).
            x.s.st.fence += 1;
            let id = x.s.st.fence;
            let now = x.s.st.env.now();
            x.s.st.leases.insert(
                id,
                Lease {
                    id,
                    token: id,
                    task: Some(x.id("#40")),
                    kind: LeaseKind::Task,
                    role: "developer".into(),
                    holder: "dev#2".into(),
                    branch: "lane/y".into(),
                    anchor: AnchorKind::None,
                    session: None,
                    anchor_boot_hash: now.boot_hash,
                    expires: crate::clock::after(now, 900_000),
                    ttl_ms: 900_000,
                    run: None,
                    run_scoped: false,
                    session_role: false,
                    claimed_hlc: 0,
                    bound: None,
                    root_session: None,
                    files_owned: vec![],
                    ended: None,
                },
            );
            x.lease19 = Some(id);
            Some(x)
        }
        case => {
            let r = rules();
            let row = r
                .table("n40-cases")
                .rows
                .iter()
                .find(|c| c.tok("case") == case)?;
            let mut x = start(row.tok("after"))?;
            run_action(&mut x, row.tok("ref"), row.tok("action"))?;
            Some(x)
        }
    }
}

/// Runs one `n40-cases` action; `None` when it needs a later work package (merge, revert, import).
fn run_action(x: &mut N40, r: &str, action: &str) -> Option<Reply> {
    let parts: Vec<&str> = action.split(':').collect();
    let ctx = orch_on(r);
    Some(match parts[0] {
        "none" => Reply::refused(crate::err::Refusal::usage("none")),
        "rm" => {
            let target = Target::Id(x.id(&format!("#{}", parts[1])));
            let mut policy = None;
            let mut replaced_by = None;
            let mut release = false;
            for p in &parts[2..] {
                match *p {
                    "reparent" => policy = Some("reparent".to_string()),
                    "cascade" => policy = Some("cascade".to_string()),
                    "release" => release = true,
                    o => {
                        replaced_by = o
                            .strip_prefix("replaced-by=")
                            .map(|y| Target::Id(x.id(&format!("#{y}"))))
                    }
                }
            }
            x.s.run(
                tx(vec![Stmt::Delete {
                    target,
                    policy,
                    replaced_by,
                    release,
                    reason: Some("obsolete".into()),
                }]),
                ctx,
            )
        }
        "resolve" => {
            // resolve:edge:#A:kind:#B:drop | repoint=<id>
            let key = format!("edge:{}:{}:{}", x.id(parts[2]), parts[3], x.id(parts[4]));
            let take = match parts[5] {
                "drop" => crate::tx::Take::Ours,
                p => crate::tx::Take::Repoint(Target::Id(
                    x.id(&format!("#{}", p.strip_prefix("repoint=")?)),
                )),
            };
            x.s.run(tx(vec![Stmt::Resolve { key, take }]), ctx)
        }
        "set" => {
            let (f, v) = parts[2].split_once('=')?;
            let n = x.id(&format!("#{}", parts[1]));
            x.s.run(tx(vec![set(n.0, &[(f, P::Int(v.parse().ok()?))])]), ctx)
        }
        "add" => {
            let (k, d) = parts[2].split_once('=')?;
            let d = x.id(&format!("#{d}"));
            let reply = x.s.run(
                tx(vec![Stmt::Create {
                    name: Some("n".into()),
                    kind: "task".into(),
                    fields: vec![("title".into(), t("new"))],
                    body: None,
                    under: None,
                    position: None,
                    edges_out: vec![(k.into(), Target::Id(d))],
                    edges_in: vec![],
                }]),
                ctx,
            );
            x.ids
                .insert(format!("#{}", parts[1]), Nid(x.s.st.next_id - 1));
            reply
        }
        "branch" => {
            let from = parts[2].strip_prefix("from=")?;
            x.s.run(
                Cmd::BranchCreate {
                    name: parts[1].into(),
                    from: Some(from.into()),
                    kind: None,
                },
                orch(),
            )
        }
        "branch-D" => x.s.run(
            Cmd::BranchDelete {
                name: parts[1].into(),
                force: true,
            },
            orch(),
        ),
        "merge" | "revert" | "import" => return None,
        other => panic!("unknown n40 action {other}"),
    })
}

/// The value of one `n40-properties` property for a subject on a ref.
fn property(x: &N40, reply: &Reply, r: &str, subject: &str, prop: &str) -> Option<String> {
    let st =
        x.s.st
            .dag
            .state_at(x.s.st.dag.live(r).and_then(|z| z.tip), &x.s.st.alloc);
    let ix = Index::new(&st);
    let yn = |b: bool| {
        if b {
            "yes".to_string()
        } else {
            "no".to_string()
        }
    };
    if subject == "rm" || subject == "merge" || subject == "import" {
        let e = reply.error.as_ref();
        return match prop {
            "refusal" => e.map(|e| e.code.clone()),
            "exit" => Some(reply.exit.to_string()),
            "refusal-names" => e
                .and_then(|e| match e.get("leases") {
                    Some(crate::err::Kv::List(v)) => v
                        .first()
                        .and_then(|l| l.member("id"))
                        .and_then(|i| i.as_str())
                        .map(str::to_string),
                    _ => None,
                })
                .map(|l| {
                    if crate::tx::parse_lease(&l) == x.lease19 {
                        "L-19".into()
                    } else {
                        l
                    }
                }),
            _ => None,
        };
    }
    if let Some(l) = subject.strip_prefix("lease:") {
        assert_eq!(l, "L-19");
        let lease = &x.s.st.leases[&x.lease19?];
        return Some(yn(crate::lease::is_live(lease, &x.s.st.env).is_live()));
    }
    if let Some(rest) = subject.strip_prefix("edge:") {
        let p: Vec<&str> = rest.split(':').collect();
        let (a, k, b) = (x.id(p[0]), p[1], x.id(p[2]));
        let e = st.nodes.get(&a).and_then(|n| {
            n.out.get(&EdgeKey {
                kind: k.into(),
                dst: b,
                disc: None,
            })
        });
        return match prop {
            "exists" => Some(yn(e.is_some())),
            "flagged" => Some(yn(e.is_some_and(|e| e.flagged))),
            _ => None,
        };
    }
    let n = x.id(subject);
    let node = st.nodes.get(&n);
    let mut o = Oracle::new(&x.s.st.dag, &x.s.st.alloc);
    let name_of = |m: Nid| {
        x.ids
            .iter()
            .find(|(_, v)| **v == m)
            .map_or(m.to_string(), |(k, _)| k.clone())
    };
    Some(match prop {
        "exists" => yn(st.live(n).is_some()),
        "deleted" => yn(node.is_some_and(|z| !z.live())),
        "parent" => node.and_then(|z| z.parent).map_or("-".into(), name_of),
        "status" => node?.status.clone(),
        "open_blockers" => derived::open_blockers(&ix, n).to_string(),
        "unblocked" => yn(derived::unblocked(&ix, n)),
        "has_dangling" => yn(derived::has_dangling(&ix, n)),
        "is_blocker" => yn(derived::is_blocker(&ix, n)),
        "suspect" => yn(derived::suspect(&ix, n, &|_| None)),
        "replaced_by" => node?.tomb.as_ref()?.replaced_by.map_or("-".into(), name_of),
        "hold" => coord::hold(&st, n).to_string(),
        "deleted_elsewhere" => yn(o.deleted_elsewhere(r, n)),
        "excluded" => yn(o.i26p_excluded(r, n)),
        "blocking-listed" => {
            yn(derived::is_blocker(&ix, n) && node?.kind == "task" && !o.i26p_excluded(r, n))
        }
        _ => return None,
    })
}

/// Every `n40-expect` row of every case whose action this work package runs holds; the rest need merges, reverts or
/// imports.
#[test]
fn node_40_rows_hold() {
    let r = rules();
    let mut checked = 0;
    let mut later = Vec::new();
    for case in &r.table("n40-cases").rows {
        let name = case.tok("case");
        let Some(mut x) = start(case.tok("after")) else {
            later.push(name.to_string());
            continue;
        };
        let Some(reply) = run_action(&mut x, case.tok("ref"), case.tok("action")) else {
            later.push(name.to_string());
            continue;
        };
        for e in r
            .table("n40-expect")
            .rows
            .iter()
            .filter(|e| e.tok("case") == name)
        {
            let got = property(
                &x,
                &reply,
                e.tok("ref"),
                e.tok("subject"),
                e.tok("property"),
            );
            match got {
                Some(v) => {
                    assert_eq!(
                        v,
                        e.tok("value"),
                        "{} ({name} {} {} {})",
                        e.id,
                        e.tok("ref"),
                        e.tok("subject"),
                        e.tok("property")
                    );
                    checked += 1;
                }
                None => later.push(e.id.clone()),
            }
        }
    }
    assert!(checked >= 50, "{checked} rows checked");
    // What remains is the merge, revert and import cases (C6, C7a–C7b, C8a–C8b, C11, C12) of WP-91 and M5.
    for l in &later {
        let case = r
            .table("n40-expect")
            .row(l)
            .map_or(l.as_str(), |e| e.tok("case"));
        assert!(
            ["C6", "C7a", "C7b", "C8a", "C8b", "C11", "C12"].contains(&case),
            "{l} ({case}) is not a merge, revert or import case"
        );
    }
}
