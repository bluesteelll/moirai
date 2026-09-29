//! Delete policies as data ([RULES/delete-policy-matrix]; [AR §3.3], [AR §2.5] T5): the deleted set, the edge-policy
//! decision per edge (`edge-policy` with its vocabularies), the steps of one delete in its commit (`delete-steps`), the
//! tombstone (`tombstone`) and flagged edges (`flagged-edges`). The role check (DP-001) and the lease check (DP-005)
//! read the caller and the lease table, so the write path runs them ([`crate::tx`]); this module runs the others.

use crate::derived::{self, Index};
use crate::err::{Refusal, Res};
use crate::rules::{Row, rules};
use crate::schema::EdgeClass;
use crate::state::{EdgeKey, EdgeProps, OBSERVATION, State, TOMBSTONE_FIELDS, Tomb};
use crate::value::{Nid, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The options of one delete ([RULES/delete-policy-matrix] `delete-options`; [API §9.2] `delete`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeleteOpts {
    /// `REPLACED BY y` (DO-002).
    pub replaced_by: Option<Nid>,
    /// `POLICY CASCADE` (DO-003).
    pub cascade: bool,
    /// `POLICY REPARENT` (DO-004).
    pub reparent: bool,
    /// `--reassign` (DO-005).
    pub reassign: bool,
    /// `RELEASE` (DO-006).
    pub release: bool,
    /// `REASON r`.
    pub reason: Option<String>,
}

/// The policy-data rows a delete reads: `edges.<kind>.on-src-deleted` on the deleting branch ([AR §13]), `flag` or
/// `drop-notify`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgePolicies {
    /// `edges.blocks.on-src-deleted`.
    pub blocks: String,
    /// `edges.gates.on-src-deleted`.
    pub gates: String,
}

impl Default for EdgePolicies {
    /// Both `flag` ([AR §13]).
    fn default() -> EdgePolicies {
        EdgePolicies {
            blocks: "flag".into(),
            gates: "flag".into(),
        }
    }
}

/// One edge a delete meets: (source, kind, destination, discriminator), `parent` for the hierarchy.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Met {
    /// The source.
    pub src: Nid,
    /// The stored kind.
    pub kind: String,
    /// The destination.
    pub dst: Nid,
    /// The discriminator.
    pub disc: Option<crate::value::Uid>,
}

/// The plan of one delete: the deleted set and the edge-policy row of every edge it meets (DS-002 to DS-004).
#[derive(Clone, Debug)]
pub struct Plan {
    /// The target.
    pub target: Nid,
    /// The deleted set: the target, then its descendants under `--cascade`.
    pub set: Vec<Nid>,
    /// Every edge with exactly one end in the set, with its `edge-policy` row.
    pub edges: Vec<(Met, &'static Row)>,
    /// Edges with both ends in the set (DS-003).
    pub internal: Vec<Met>,
}

/// The effects of an applied delete that are not derived state: the nodes to name in `affected` with the reason
/// (EA-003, EF-010, EF-017) and the questions it reopened (EF-014).
#[derive(Clone, Debug, Default)]
pub struct Effects {
    /// Nodes named in `affected` and the change feed with a reason.
    pub notified: BTreeSet<Nid>,
    /// Questions whose status moved `answered → open` by the door `delete-policy`.
    pub reopened: Vec<Nid>,
}

/// DS-002: the target, plus every live descendant under `--cascade`.
// rule: DS-002
pub fn deleted_set(st: &State, target: Nid, cascade: bool) -> Vec<Nid> {
    let mut set = vec![target];
    if cascade {
        let ix = Index::new(st);
        let mut d = derived::descendants(&ix, target);
        d.sort();
        set.extend(d);
    }
    set
}

/// CD rows: the condition of an `edge-policy` row, evaluated on the state before the delete.
// spec: [RULES/delete-policy-matrix] edge-conditions
// rule: CD-001, CD-002, CD-003, CD-004, CD-005, CD-006, CD-007, CD-008, CD-009
pub fn condition(ix: &Index<'_>, cond: &str, m: &Met, set: &[Nid]) -> bool {
    match cond {
        "-" => true,
        "src-unfinished" => {
            let s = ix.st.live(m.src);
            s.is_some_and(|x| match x.kind.as_str() {
                "task" => x.status != "done" && x.status != "cancelled",
                "question" => derived::answered(ix, m.src) != Some(true),
                _ => false,
            })
        }
        "src-finished" => !condition(ix, "src-unfinished", m, set),
        "src-gating" => derived::gating_verdict(ix, m.src),
        "src-not-gating" => !derived::gating_verdict(ix, m.src),
        "last-answer" => !ix
            .in_edges(m.dst, "answers")
            .any(|(s, _, _)| !set.contains(&s) && ix.st.live(s).is_some()),
        "other-answer" => !condition(ix, "last-answer", m, set),
        "has-parent-area" => parent_area(ix.st, m.dst).is_some(),
        "no-parent-area" => parent_area(ix.st, m.dst).is_none(),
        other => panic!("edge condition {other} has no implementation"),
    }
}

fn parent_area(st: &State, area: Nid) -> Option<Nid> {
    let p = st.live(area)?.parent?;
    st.live(p).filter(|x| x.kind == "area").map(|_| p)
}

/// The `edge-policy` row of one edge ([RULES/delete-policy-matrix] §5): the rows of its kind and end whose option is
/// the delete option that matters for this edge (or `*`), whose policy is the branch's `on-src-deleted` value (or `-`,
/// `*`) and whose condition holds. Exactly one row matches; anything else is a table fault.
// spec: [RULES/delete-policy-matrix] edge-policy
pub fn edge_policy(
    ix: &Index<'_>,
    m: &Met,
    end: &str,
    opts: &DeleteOpts,
    pol: &EdgePolicies,
    set: &[Nid],
) -> &'static Row {
    let t = rules().table("edge-policy");
    let rows: Vec<&Row> = t
        .rows
        .iter()
        .filter(|r| r.tok("edge") == m.kind && r.tok("end") == end)
        .collect();
    let given = |o: &str| match o {
        "replaced-by" => opts.replaced_by.is_some(),
        "cascade" => opts.cascade,
        "reparent" => opts.reparent,
        "reassign" => opts.reassign,
        _ => false,
    };
    let relevant = rows
        .iter()
        .map(|r| r.tok("option"))
        .find(|o| *o != "none" && *o != "*" && given(o))
        .unwrap_or("none");
    let policy = match m.kind.as_str() {
        "blocks" => pol.blocks.as_str(),
        "gates" => pol.gates.as_str(),
        _ => "-",
    };
    let hits: Vec<&Row> = rows
        .into_iter()
        .filter(|r| r.tok("option") == relevant || r.tok("option") == "*")
        .filter(|r| matches!(r.tok("policy"), "-" | "*") || r.tok("policy") == policy)
        .filter(|r| condition(ix, r.tok("condition"), m, set))
        .collect();
    assert!(
        hits.len() == 1,
        "edge-policy: {} rows match {m:?} at {end} with option {relevant}: {:?}",
        hits.len(),
        hits.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    hits[0]
}

/// Every edge a node of the deleted set shares with a node outside it, and the internal edges (DS-003, DS-004).
fn met_edges(st: &State, set: &[Nid]) -> (Vec<(Met, &'static str)>, Vec<Met>) {
    let inside = |n: Nid| set.contains(&n);
    let mut cross = Vec::new();
    let mut internal = Vec::new();
    for (n, node) in &st.nodes {
        if node.live()
            && let Some(p) = node.parent
            && (inside(*n) || inside(p))
        {
            let m = Met {
                src: *n,
                kind: "parent".into(),
                dst: p,
                disc: None,
            };
            if inside(*n) && inside(p) {
                internal.push(m);
            } else {
                cross.push((
                    m,
                    if inside(p) {
                        "dst-deleted"
                    } else {
                        "src-deleted"
                    },
                ));
            }
        }
        for k in node.out.keys() {
            if !(inside(*n) || inside(k.dst)) {
                continue;
            }
            let m = Met {
                src: *n,
                kind: k.kind.clone(),
                dst: k.dst,
                disc: k.disc,
            };
            if inside(*n) && inside(k.dst) {
                internal.push(m);
            } else {
                cross.push((
                    m,
                    if inside(k.dst) {
                        "dst-deleted"
                    } else {
                        "src-deleted"
                    },
                ));
            }
        }
    }
    (cross, internal)
}

/// DS-001 to DS-004 without the role check: DP-002 (view), DP-003 (target live), DP-004 (options), DP-010 (root
/// node), DP-005 (a live lease, decided by `leased` over the deleted set, which reads the lease table), DP-006
/// (restrict, with the impact list) and DP-007 (replacement), in that order.
// spec: [RULES/delete-policy-matrix] delete-preconditions
// rule: DP-002, DP-003, DP-004, DP-010, DP-006, DP-007, DS-001, DS-003, DS-004
pub fn plan(
    st: &State,
    view: &str,
    target: Nid,
    opts: &DeleteOpts,
    pol: &EdgePolicies,
    leased: &dyn Fn(&[Nid]) -> Option<Refusal>,
) -> Res<Plan> {
    if view != "work" && view != "plan" {
        return Err(Refusal::lq(
            "E305",
            format!("a delete needs a work or plan tip, not a {view} view"),
        ));
    }
    if st.live(target).is_none() {
        return Err(Refusal::not_found("node", target.to_string()));
    }
    if opts.cascade && opts.reparent {
        return Err(Refusal::usage(
            "--cascade and --reparent cannot be combined",
        ));
    }
    let set = deleted_set(st, target, opts.cascade);
    if let Some(r) = set
        .iter()
        .filter(|n| {
            st.live(**n)
                .is_some_and(|x| x.kind == "area" && x.fields.contains_key("root"))
        })
        .min()
    {
        return Err(Refusal::lq(
            "E409",
            format!("DELETE {target} refused: {r} is a root node"),
        ));
    }
    if let Some(e) = leased(&set) {
        return Err(e);
    }
    let ix = Index::new(st);
    let (cross, internal) = met_edges(st, &set);
    let mut edges = Vec::new();
    let mut restricted = Vec::new();
    for (m, end) in cross {
        let r = edge_policy(&ix, &m, end, opts, pol, &set);
        if r.tok("action") == "refuse" {
            restricted.push(format!("{} {} {}", m.src, m.kind, m.dst));
        }
        edges.push((m, r));
    }
    if !restricted.is_empty() {
        return Err(Refusal::lq(
            "E409",
            format!("restricted references: {}", restricted.join("; ")),
        ));
    }
    if let Some(y) = opts.replaced_by {
        let fits = |m: &Met, end: &str| -> bool {
            let Some(e) = st.schema.edge(&m.kind) else {
                return false;
            };
            let Some(yk) = st.live(y).map(|x| x.kind.clone()) else {
                return false;
            };
            if end == "dst-deleted" {
                e.dst.allows(&yk) && (!e.same_kind || st.live(m.src).is_some_and(|s| s.kind == yk))
            } else {
                e.src.allows(&yk) && (!e.same_kind || st.live(m.dst).is_some_and(|d| d.kind == yk))
            }
        };
        if st.live(y).is_none() {
            return Err(Refusal::lq("E409", format!("replacement {y} is not live")));
        }
        if set.contains(&y) {
            return Err(Refusal::lq(
                "E409",
                format!("replacement {y} is in the deleted set"),
            ));
        }
        for (m, r) in &edges {
            if r.tok("action") == "repoint" && !opts.reassign {
                let end = if set.contains(&m.dst) {
                    "dst-deleted"
                } else {
                    "src-deleted"
                };
                if !fits(m, end) {
                    return Err(Refusal::lq(
                        "E409",
                        format!(
                            "replacement {y} does not fit {} -[:{}]-> {}",
                            m.src, m.kind, m.dst
                        ),
                    ));
                }
            }
        }
    }
    Ok(Plan {
        target,
        set,
        edges,
        internal,
    })
}

/// DS-005 to DS-006 and the tombstone rows: turns every node of the set into a tombstone and writes the policy ops
/// the actions imply. Lease release (DS-007), markers (DS-008) and `affected` (DS-009) are the write path's.
// spec: [RULES/delete-policy-matrix] delete-steps
// rule: DS-005, DS-006, TB-001, TB-002, TB-003, TB-004, TB-005, TB-006, TB-007, TB-008, TB-009, TB-010, TB-011, TB-012, TB-013
pub fn apply(st: &mut State, p: &Plan, opts: &DeleteOpts) -> Effects {
    let mut fx = Effects::default();
    let schema = st.schema.clone();
    // DS-003: internal edges — a historical edge stays as its source tombstone's out-edge; a structural one is dropped.
    for m in &p.internal {
        if m.kind == "parent" {
            continue;
        }
        let historical = schema
            .edge(&m.kind)
            .is_some_and(|e| e.class == EdgeClass::Historical);
        if !historical && let Some(x) = st.nodes.get_mut(&m.src) {
            x.out.remove(&EdgeKey {
                kind: m.kind.clone(),
                dst: m.dst,
                disc: m.disc,
            });
        }
    }
    // DS-004 and DS-006: the action of every crossing edge.
    let deleted_parent = st.live(p.target).and_then(|x| x.parent);
    for (m, r) in &p.edges {
        let key = EdgeKey {
            kind: m.kind.clone(),
            dst: m.dst,
            disc: m.disc,
        };
        let src_deleted = p.set.contains(&m.src);
        let other = if src_deleted { m.dst } else { m.src };
        match r.tok("action") {
            "drop" | "drop-notify" => {
                if m.kind != "parent"
                    && let Some(x) = st.nodes.get_mut(&m.src)
                {
                    x.out.remove(&key);
                }
                if r.tok("action") == "drop-notify" {
                    fx.notified.insert(other);
                }
            }
            "repoint" => {
                let y = if opts.reassign {
                    parent_area(st, m.dst).expect("CD-008 held when the row was chosen")
                } else {
                    opts.replaced_by
                        .expect("DO-002 held when the row was chosen")
                };
                let props = st
                    .nodes
                    .get(&m.src)
                    .and_then(|x| x.out.get(&key))
                    .cloned()
                    .unwrap_or_default();
                let props = EdgeProps {
                    flagged: false,
                    ..props
                };
                if let Some(x) = st.nodes.get_mut(&m.src) {
                    x.out.remove(&key);
                }
                let (s, d) = if src_deleted { (y, m.dst) } else { (m.src, y) };
                if s != d
                    && let Some(x) = st.nodes.get_mut(&s)
                {
                    x.out.insert(
                        EdgeKey {
                            kind: m.kind.clone(),
                            dst: d,
                            disc: m.disc,
                        },
                        props,
                    );
                }
            }
            "flag" => {
                if let Some(e) = st.nodes.get_mut(&m.src).and_then(|x| x.out.get_mut(&key)) {
                    e.flagged = true;
                }
            }
            "tombstone-ref" | "retain" => {}
            "move-up" => {
                if let Some(c) = st.nodes.get_mut(&m.src) {
                    c.parent = deleted_parent;
                }
            }
            "delete-subtree" => {}
            "refuse" => unreachable!("plan() refuses restricted edges"),
            other => panic!("edge action {other} has no implementation"),
        }
        match r.tok("effect") {
            "question-reopened" => {
                if let Some(q) = st.nodes.get_mut(&other)
                    && q.live()
                    && q.status == "answered"
                {
                    q.status = "open".into();
                    q.resolution = "none".into();
                    fx.reopened.push(other);
                }
            }
            "src-affected" | "dependent-notified" => {
                fx.notified.insert(other);
            }
            _ => {}
        }
    }
    // DS-005 and the tombstone rows: kind, status, the header enumerations and the title stay; the retained out-edges
    // (flagged structural edges, historical edges) stay; every other field, the body and the parent go.
    for n in &p.set {
        let Some(x) = st.nodes.get_mut(n) else {
            continue;
        };
        tombstone_keys(x, &schema);
        let retained: BTreeMap<EdgeKey, EdgeProps> = x
            .out
            .iter()
            .filter(|(k, props)| {
                props.flagged
                    || schema
                        .edge(&k.kind)
                        .is_some_and(|e| e.class == EdgeClass::Historical)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        x.out = retained;
        x.tomb = Some(Tomb {
            reason: opts.reason.clone(),
            replaced_by: opts.replaced_by,
        });
    }
    fx
}

/// The keys a node keeps as a tombstone besides its existence and out-edges ([F07 §6.4]; [F08 §3.5]; DS-005): a kind
/// with a derived title takes its last `path` text as `title`; the title and the header enumerations stay; every
/// other field, the body, the hierarchy and every conflict value go. The caller sets the tombstone and decides the
/// out-edges (the retained ones of a delete; none for a revert's inverse of a creation).
pub fn tombstone_keys(x: &mut crate::state::Node, schema: &crate::schema::Schema) {
    if schema.kind(&x.kind).is_some_and(|k| k.title_derived)
        && let Some(Value::Path(pv)) = x.fields.get("path")
    {
        let t = pv.text.clone();
        x.fields.insert("title".into(), Value::Text(t));
    }
    x.fields.retain(|f, _| {
        TOMBSTONE_FIELDS.contains(&f.as_str()) && !OBSERVATION.contains(&f.as_str())
    });
    x.body = None;
    x.parent = None;
    x.order = None;
    x.conflicts.clear();
}

/// FL-005 and FL-006: resolving a flagged edge re-points it to `y` without the flag, or removes it.
// rule: FL-005, FL-006
pub fn resolve_flagged(st: &mut State, src: Nid, key: &EdgeKey, repoint: Option<Nid>) -> Res<()> {
    let flagged = st
        .nodes
        .get(&src)
        .and_then(|x| x.out.get(key))
        .is_some_and(|p| p.flagged);
    if !flagged {
        return Err(Refusal::not_found(
            "conflict key",
            format!("edge:{src}:{}:{}", key.kind, key.dst),
        ));
    }
    if let Some(x) = st.nodes.get_mut(&src) {
        x.out.remove(key);
    }
    if let Some(y) = repoint {
        if st.live(y).is_none() {
            return Err(Refusal::not_found("node", y.to_string()));
        }
        if let Some(x) = st.nodes.get_mut(&y) {
            x.out.insert(
                EdgeKey {
                    kind: key.kind.clone(),
                    dst: key.dst,
                    disc: key.disc,
                },
                EdgeProps::default(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Schema;
    use crate::state::{Creator, Node};
    use crate::value::Uid;

    /// For every edge kind, end, option, policy value and condition outcome, exactly one `edge-policy` row matches
    /// ([RULES/delete-policy-matrix] §2: "the model checks this at load for every combination of the vocabularies").
    #[test]
    fn exactly_one_edge_policy_row_matches_every_combination() {
        let t = rules().table("edge-policy");
        let kinds: BTreeSet<&str> = t.rows.iter().map(|r| r.tok("edge")).collect();
        let options = ["none", "replaced-by", "cascade", "reparent", "reassign"];
        let conds = rules()
            .table("edge-conditions")
            .rows
            .iter()
            .map(|r| r.tok("condition"))
            .collect::<Vec<_>>();
        for k in kinds {
            for end in ["dst-deleted", "src-deleted"] {
                for opt in options {
                    for pol in ["flag", "drop-notify"] {
                        // The condition outcomes: a consistent assignment where each negated pair takes opposite values.
                        for bits in 0..16u32 {
                            let holds = |c: &str| -> bool {
                                match c {
                                    "-" => true,
                                    "src-unfinished" => bits & 1 != 0,
                                    "src-finished" => bits & 1 == 0,
                                    "src-gating" => bits & 2 != 0,
                                    "src-not-gating" => bits & 2 == 0,
                                    "last-answer" => bits & 4 != 0,
                                    "other-answer" => bits & 4 == 0,
                                    "has-parent-area" => bits & 8 != 0,
                                    "no-parent-area" => bits & 8 == 0,
                                    other => {
                                        panic!("{other} is not in edge-conditions ({conds:?})")
                                    }
                                }
                            };
                            let rows: Vec<&Row> = t
                                .rows
                                .iter()
                                .filter(|r| r.tok("edge") == k && r.tok("end") == end)
                                .collect();
                            let relevant = rows
                                .iter()
                                .map(|r| r.tok("option"))
                                .find(|o| *o == opt && *o != "none" && *o != "*")
                                .unwrap_or("none");
                            let policy = if k == "blocks" || k == "gates" {
                                pol
                            } else {
                                "-"
                            };
                            let hits = rows
                                .iter()
                                .filter(|r| r.tok("option") == relevant || r.tok("option") == "*")
                                .filter(|r| {
                                    matches!(r.tok("policy"), "-" | "*")
                                        || r.tok("policy") == policy
                                })
                                .filter(|r| holds(r.tok("condition")))
                                .count();
                            assert_eq!(
                                hits, 1,
                                "{k} {end} option {opt} policy {pol} bits {bits:04b}"
                            );
                        }
                    }
                }
            }
        }
    }

    fn uid(n: u32) -> Uid {
        let mut b = [0u8; 16];
        b[12..].copy_from_slice(&n.to_be_bytes());
        Uid(b)
    }

    fn n40() -> State {
        let s = Schema::default();
        let mut st = State::default();
        let r = rules();
        for row in &r.table("n40-nodes").rows {
            let n: u32 = row.tok("node")[1..].parse().unwrap();
            let mut x = Node::new(uid(n), row.tok("kind"), &s, Creator::default());
            x.status = row.tok("status").to_string();
            if row.tok("parent") != "-" {
                x.parent = Some(Nid(row.tok("parent")[1..].parse().unwrap()));
            }
            x.set_field(&s, "title", Some(Value::Text(format!("n{n}"))));
            st.nodes.insert(Nid(n), x);
        }
        for row in &r.table("n40-edges").rows {
            let a: u32 = row.tok("src")[1..].parse().unwrap();
            let b: u32 = row.tok("dst")[1..].parse().unwrap();
            if row.tok("kind") == "parent" {
                continue;
            }
            st.nodes.get_mut(&Nid(a)).unwrap().out.insert(
                EdgeKey {
                    kind: row.tok("kind").into(),
                    dst: Nid(b),
                    disc: None,
                },
                EdgeProps::default(),
            );
        }
        st
    }

    #[test]
    fn node_40_c1_is_restricted_and_c2_repoints() {
        let st = n40();
        let pol = EdgePolicies::default();
        let c1 = DeleteOpts {
            replaced_by: Some(Nid(52)),
            ..Default::default()
        };
        let e = plan(&st, "work", Nid(40), &c1, &pol, &|_| None).unwrap_err();
        assert_eq!((e.code.as_str(), e.exit), ("E409", 6), "NX-004, NX-005");
        let c2 = DeleteOpts {
            replaced_by: Some(Nid(52)),
            reparent: true,
            reason: Some("dup of #52".into()),
            ..Default::default()
        };
        let p = plan(&st, "work", Nid(40), &c2, &pol, &|_| None).unwrap();
        let mut after = st.clone();
        apply(&mut after, &p, &c2);
        let has = |st: &State, a: u32, k: &str, b: u32| {
            st.nodes[&Nid(a)].out.contains_key(&EdgeKey {
                kind: k.into(),
                dst: Nid(b),
                disc: None,
            })
        };
        assert!(!after.nodes[&Nid(40)].live(), "NX-007");
        assert_eq!(
            after.nodes[&Nid(40)].tomb.as_ref().unwrap().replaced_by,
            Some(Nid(52)),
            "NX-008"
        );
        assert!(!has(&after, 40, "blocks", 12), "NX-009");
        assert!(has(&after, 52, "blocks", 12), "NX-010");
        assert!(!has(&after, 203, "blocks", 40), "NX-011");
        assert!(has(&after, 203, "blocks", 52), "NX-012");
        assert_eq!(after.nodes[&Nid(41)].parent, Some(Nid(9)), "NX-013");
        assert!(has(&after, 17, "derived_from", 40), "NX-018");
        let ix = Index::new(&after);
        assert_eq!(derived::open_blockers(&ix, Nid(12)), 1, "NX-014");
        assert!(derived::suspect(&ix, Nid(17), &|_| None), "NX-017");
        assert!(!derived::suspect(&ix, Nid(77), &|_| None), "NX-019");
        assert!(crate::inv::i2_structural_edges_live(&after).is_ok());
    }

    /// [F07 §6.4]: a tombstone keeps its title and header enumerations; an artifact, whose live title is derived, takes
    /// its last `path` text as its title; every other field, the body and the hierarchy go.
    #[test]
    fn a_tombstone_keeps_its_title_and_an_artifact_its_path() {
        use crate::value::PathVal;
        let s = Schema::default();
        let mut a = Node::new(Uid([7; 16]), "artifact", &s, Creator::default());
        a.fields.insert(
            "path".into(),
            Value::Path(PathVal {
                root: "project".into(),
                text: "docs/a.md".into(),
            }),
        );
        a.fields.insert("bytes".into(), Value::Int(12));
        a.parent = Some(Nid(3));
        tombstone_keys(&mut a, &s);
        assert_eq!(
            a.fields.keys().collect::<Vec<_>>(),
            vec!["title"],
            "{:?}",
            a.fields
        );
        assert_eq!(a.fields["title"], Value::Text("docs/a.md".into()));
        assert_eq!(a.parent, None);
        let mut t = Node::new(Uid([8; 16]), "task", &s, Creator::default());
        t.set_field(&s, "title", Some(Value::Text("t".into())));
        t.set_field(&s, "priority", Some(Value::Enum("P0".into())));
        t.set_field(&s, "estimate", Some(Value::Int(3)));
        t.body = Some("b".into());
        tombstone_keys(&mut t, &s);
        assert_eq!(
            t.fields.keys().collect::<Vec<_>>(),
            vec!["priority", "title"]
        );
        assert_eq!(t.body, None);
    }
}
