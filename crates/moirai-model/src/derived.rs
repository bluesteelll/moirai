//! Derived state by definition ([F13 §6.2] `P_F15`; [AR §3.5]; [RULES/state-definition] §4): every predicate is
//! recomputed from scratch over one view's state ([60 §4.2]), one function per predicate and per clause, the blocker
//! terms evaluated as data from the table `blocker-terms`. The engine maintains the same predicates incrementally;
//! GT6 and GT2 compare the two, and [`affected`] is the model's `affected` list of a commit ([F13 §6.3]).
//!
//! The pinned clause of `suspect` compares an edge's `pinned_commit` with its target's current commit, which is a fact
//! of the view's history, not of its state: every function here takes it as `current`.

use crate::rules::rules;
use crate::state::{EdgeKey, EdgeProps, State};
use crate::value::{Nid, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// The current commit of a node on the view: the id of the newest commit of the view's first-parent chain whose net
/// changeset changed a key the node owns ([API §15.4] `rev`), when commit ids are known.
pub type Current<'a> = &'a dyn Fn(Nid) -> Option<[u8; 32]>;

/// The `P_F15` values of one live node ([F13 §6.2]; [API §15.5] with `gated`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Row {
    /// The virtual `done`; `None` for a kind without it.
    pub done: Option<bool>,
    /// `NOT done`; `None` for a kind without `done`.
    pub unfinished: Option<bool>,
    /// The structural `unblocked` (PD-001 to PD-007).
    pub unblocked: bool,
    /// `blocked` (PD-019 to PD-021).
    pub blocked: bool,
    /// `open_blockers`.
    pub open_blockers: u32,
    /// `open_blockers_exo`.
    pub open_blockers_exo: u32,
    /// `is_blocker` (PD-023).
    pub is_blocker: bool,
    /// Live direct children.
    pub children_total: u32,
    /// Live direct children whose `done` is true.
    pub children_done: u32,
    /// A container whose children are all done.
    pub ready_to_close: bool,
    /// Has a live child (PD-022).
    pub container: bool,
    /// Single-hop `suspect`.
    pub suspect: bool,
    /// `answered`; `None` for a kind other than `question`.
    pub answered: Option<bool>,
    /// Holds a conflict value.
    pub conflicted: bool,
    /// Has a flagged in-edge (FL-003).
    pub has_dangling: bool,
    /// Depth in the `parent` forest (a root is 0).
    pub depth: u32,
    /// `gated` (PD-024).
    pub gated: bool,
}

/// The adjacency of one view, derived on the fly ([F13 §5] V02: never materialised in the store), with a memo of
/// `open_blockers_exo`, which PD-007 reads for every ancestor of every node.
pub struct Index<'a> {
    /// The view.
    pub st: &'a State,
    /// In-edges by destination: (source, key, properties).
    pub inn: BTreeMap<Nid, Vec<(Nid, &'a EdgeKey, &'a EdgeProps)>>,
    /// Live children by parent.
    pub children: BTreeMap<Nid, Vec<Nid>>,
    exo: RefCell<BTreeMap<Nid, u32>>,
}

impl<'a> Index<'a> {
    /// Builds the reverse adjacency and the live children of every node.
    pub fn new(st: &'a State) -> Index<'a> {
        let mut inn: BTreeMap<Nid, Vec<(Nid, &EdgeKey, &EdgeProps)>> = BTreeMap::new();
        let mut children: BTreeMap<Nid, Vec<Nid>> = BTreeMap::new();
        for (n, node) in &st.nodes {
            for (k, p) in &node.out {
                inn.entry(k.dst).or_default().push((*n, k, p));
            }
            if node.live()
                && let Some(p) = node.parent
            {
                children.entry(p).or_default().push(*n);
            }
        }
        Index {
            st,
            inn,
            children,
            exo: RefCell::new(BTreeMap::new()),
        }
    }

    /// The in-edges of `n` of one kind.
    pub fn in_edges<'s>(
        &'s self,
        n: Nid,
        kind: &'s str,
    ) -> impl Iterator<Item = (Nid, &'a EdgeKey, &'a EdgeProps)> + 's {
        self.inn
            .get(&n)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |(_, k, _)| k.kind == kind)
    }

    /// The live out-edges of `n` of one kind: (destination, properties).
    pub fn out_edges(&self, n: Nid, kind: &str) -> Vec<(Nid, &'a EdgeProps)> {
        self.st
            .nodes
            .get(&n)
            .map(|x| {
                x.out
                    .iter()
                    .filter(|(k, _)| k.kind == kind)
                    .map(|(k, p)| (k.dst, p))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The ancestors of `n`, nearest first, never through a cycle and at most 64 of them, so that a candidate that
    /// breaks I4 (forest depth ≤ 12) still ends; on a valid view there are at most 12.
    pub fn ancestors(&self, n: Nid) -> Vec<Nid> {
        let mut v = Vec::new();
        let mut cur = self.st.live(n).and_then(|x| x.parent);
        while let Some(p) = cur {
            if v.contains(&p) || p == n || v.len() >= 64 {
                break;
            }
            v.push(p);
            cur = self.st.live(p).and_then(|x| x.parent);
        }
        v
    }

    /// Whether `x` lies in `subtree(p)`: `x` is `p` or one of its descendants.
    pub fn in_subtree(&self, x: Nid, p: Nid) -> bool {
        x == p || self.ancestors(x).contains(&p)
    }
}

/// The virtual `done` of a live node ([F13 §6.2]; [RULES/status-machines] `statuses`): `yes` statuses for tasks and
/// verdicts, the `answered` predicate for questions (ST-024 to ST-026), `None` for the other kinds.
// spec: [F13 §6.2] done
pub fn done(ix: &Index<'_>, n: Nid) -> Option<bool> {
    let node = ix.st.live(n)?;
    let k = ix.st.schema.kind(&node.kind)?;
    if !k.has_done {
        return None;
    }
    if k.done_derived {
        return answered(ix, n);
    }
    Some(
        ix.st
            .schema
            .value(&node.kind, "status", &node.status)
            .is_some_and(|v| v.done),
    )
}

/// `answered`: a live `answers` in-edge from a live `decision` or `note` ([F13 §6.2]; PD-026).
// spec: [F13 §6.2] answered
// rule: PD-026
pub fn answered(ix: &Index<'_>, n: Nid) -> Option<bool> {
    let node = ix.st.live(n)?;
    if node.kind != "question" {
        return None;
    }
    Some(ix.in_edges(n, "answers").any(|(s, _, _)| {
        ix.st
            .live(s)
            .is_some_and(|x| x.kind == "decision" || x.kind == "note")
    }))
}

/// Whether a verdict gates completion: status `open` with outcome `fail_fixable` or `fail_fundamental`
/// ([RULES/status-machines] GD-002; [RULES/delete-policy-matrix] CD-004).
pub fn gating_verdict(ix: &Index<'_>, v: Nid) -> bool {
    ix.st.live(v).is_some_and(|x| {
        x.kind == "verdict"
            && x.status == "open"
            && matches!(x.fields.get("outcome"), Some(Value::Enum(o)) if o == "fail_fixable" || o == "fail_fundamental")
    })
}

/// The `source_state` token of [RULES/state-definition] `blocker-terms` for one in-edge.
fn source_state(ix: &Index<'_>, src: Nid, p: &EdgeProps) -> &'static str {
    if p.flagged {
        return "flagged";
    }
    let Some(node) = ix.st.live(src) else {
        return "dead";
    };
    match node.kind.as_str() {
        "verdict" => {
            if gating_verdict(ix, src) {
                "live-verdict-gating"
            } else {
                "live-verdict-not-gating"
            }
        }
        "question" if answered(ix, src) != Some(true) => "live-question-unanswered",
        "task" if done(ix, src) != Some(true) => "live-task-unfinished",
        _ => "live-finished",
    }
}

/// The weight the `blocker-terms` rows give one in-edge in the bucket `counts_in`: the sum of the matching rows'
/// weights, a row matching on its edge kind and on its `source_state` or `*` ([RULES/state-definition] BT rows,
/// evaluated as data).
// spec: [RULES/state-definition] blocker-terms
// rule: BT-001, BT-002, BT-003, BT-004, BT-005, BT-006, BT-007, BT-008
// rule: FL-001
pub fn term_weight(ix: &Index<'_>, kind: &str, src: Nid, p: &EdgeProps, counts_in: &str) -> u32 {
    let state = source_state(ix, src, p);
    rules()
        .table("blocker-terms")
        .rows
        .iter()
        .filter(|r| r.tok("edge") == kind && r.tok("counts_in") == counts_in)
        .filter(|r| r.tok("source_state") == state || r.tok("source_state") == "*")
        .map(|r| r.int("weight") as u32)
        .sum()
}

fn bucket(ix: &Index<'_>, n: Nid, counts_in: &str, exo: bool) -> u32 {
    let mut total = 0;
    for kind in ["blocks", "gates"] {
        for (src, _, p) in ix.in_edges(n, kind) {
            // A flagged edge's dead source lies outside every subtree.
            if exo && !p.flagged && ix.in_subtree(src, n) {
                continue;
            }
            total += term_weight(ix, kind, src, p, counts_in);
        }
    }
    total
}

/// `open_blockers`: the `blocker-terms` rows counted in `open_blockers` ([F13 §6.2]).
// spec: [F13 §6.2] open_blockers
pub fn open_blockers(ix: &Index<'_>, n: Nid) -> u32 {
    bucket(ix, n, "open_blockers", false)
}

/// `open_blockers_exo`: the same terms over in-edges whose source lies outside `subtree(n)` ([F13 §6.2]); memoised
/// per index.
// spec: [F13 §6.2] open_blockers_exo
pub fn open_blockers_exo(ix: &Index<'_>, n: Nid) -> u32 {
    if let Some(v) = ix.exo.borrow().get(&n) {
        return *v;
    }
    let v = bucket(ix, n, "open_blockers", true);
    ix.exo.borrow_mut().insert(n, v);
    v
}

/// `gated` (PD-024): a task with at least one `blocker-terms` row counted in `gated` with weight 1.
// spec: [F13 §6.2] gated
// rule: PD-024
pub fn gated(ix: &Index<'_>, n: Nid) -> bool {
    ix.st.live(n).is_some_and(|x| x.kind == "task") && bucket(ix, n, "gated", false) > 0
}

/// PD-001: `kind = task`.
// rule: PD-001
pub fn pd001_kind_task(ix: &Index<'_>, n: Nid) -> bool {
    ix.st.nodes.get(&n).is_some_and(|x| x.kind == "task")
}

/// PD-002: `status = open`.
// rule: PD-002
pub fn pd002_status_open(ix: &Index<'_>, n: Nid) -> bool {
    ix.st.nodes.get(&n).is_some_and(|x| x.status == "open")
}

/// PD-003: not deleted.
// rule: PD-003
pub fn pd003_not_deleted(ix: &Index<'_>, n: Nid) -> bool {
    ix.st.live(n).is_some()
}

/// PD-004: no unresolved conflict value on the node.
// rule: PD-004
pub fn pd004_not_conflicted(ix: &Index<'_>, n: Nid) -> bool {
    !conflicted(ix, n)
}

/// PD-005: not a container (PD-022).
// rule: PD-005
pub fn pd005_not_container(ix: &Index<'_>, n: Nid) -> bool {
    !container(ix, n)
}

/// PD-006: `open_blockers = 0`.
// rule: PD-006
pub fn pd006_open_blockers_zero(ix: &Index<'_>, n: Nid) -> bool {
    open_blockers(ix, n) == 0
}

/// PD-007: no ancestor with `open_blockers_exo > 0` (at most 12 parents, I4).
// rule: PD-007
pub fn pd007_no_ancestor_exo(ix: &Index<'_>, n: Nid) -> bool {
    ix.ancestors(n)
        .into_iter()
        .take(12)
        .all(|a| open_blockers_exo(ix, a) == 0)
}

/// The structural `unblocked` ([F13 §6.2]; PD-001 to PD-007). The time clause PD-008 is outside `P_F15`: the tip-only
/// `ready` of [`crate::coord::ready`] adds it.
// spec: [F13 §6.2] unblocked
pub fn unblocked(ix: &Index<'_>, n: Nid) -> bool {
    pd001_kind_task(ix, n)
        && pd002_status_open(ix, n)
        && pd003_not_deleted(ix, n)
        && pd004_not_conflicted(ix, n)
        && pd005_not_container(ix, n)
        && pd006_open_blockers_zero(ix, n)
        && pd007_no_ancestor_exo(ix, n)
}

/// PD-019: `kind = task`.
// rule: PD-019
pub fn pd019_kind_task(ix: &Index<'_>, n: Nid) -> bool {
    pd001_kind_task(ix, n)
}

/// PD-020: unfinished (PD-025).
// rule: PD-020
pub fn pd020_unfinished(ix: &Index<'_>, n: Nid) -> bool {
    unfinished(ix, n) == Some(true)
}

/// PD-021: an open blocker, or an ancestor with an open exogenous blocker.
// rule: PD-021
pub fn pd021_open_blocker_or_exo_ancestor(ix: &Index<'_>, n: Nid) -> bool {
    open_blockers(ix, n) > 0 || !pd007_no_ancestor_exo(ix, n)
}

/// `blocked` ([F13 §6.2]; PD-019 to PD-021).
// spec: [F13 §6.2] blocked
pub fn blocked(ix: &Index<'_>, n: Nid) -> bool {
    pd019_kind_task(ix, n) && pd020_unfinished(ix, n) && pd021_open_blocker_or_exo_ancestor(ix, n)
}

/// `unfinished`: `NOT done` for a kind with `done` (PD-025).
// spec: [F13 §6.2] unfinished
// rule: PD-025
pub fn unfinished(ix: &Index<'_>, n: Nid) -> Option<bool> {
    done(ix, n).map(|d| !d)
}

/// `is_blocker`: not done, with an outgoing `blocks` edge to a live task that is not done (PD-023).
// spec: [F13 §6.2] is_blocker
// rule: PD-023
pub fn is_blocker(ix: &Index<'_>, n: Nid) -> bool {
    ix.st.live(n).is_some()
        && done(ix, n) != Some(true)
        && ix.out_edges(n, "blocks").into_iter().any(|(d, _)| {
            ix.st.live(d).is_some_and(|x| x.kind == "task") && done(ix, d) != Some(true)
        })
}

/// `children_total`: live direct children ([F13 §6.2]).
// spec: [F13 §6.2] children_total
pub fn children_total(ix: &Index<'_>, n: Nid) -> u32 {
    ix.children.get(&n).map_or(0, |c| c.len() as u32)
}

/// `children_done`: live direct children whose `done` is true.
// spec: [F13 §6.2] children_done
pub fn children_done(ix: &Index<'_>, n: Nid) -> u32 {
    ix.children.get(&n).map_or(0, |c| {
        c.iter().filter(|x| done(ix, **x) == Some(true)).count() as u32
    })
}

/// `container`: at least one live node has this node as its parent (PD-022).
// spec: [F13 §6.2] container
// rule: PD-022
pub fn container(ix: &Index<'_>, n: Nid) -> bool {
    children_total(ix, n) > 0
}

/// `ready_to_close`: a container whose children are all done ([AR §3.5]).
// spec: [F13 §6.2] ready_to_close
pub fn ready_to_close(ix: &Index<'_>, n: Nid) -> bool {
    container(ix, n) && children_done(ix, n) == children_total(ix, n)
}

/// `conflicted`: the node holds an unresolved conflict value on this view.
// spec: [F13 §6.2] conflicted
pub fn conflicted(ix: &Index<'_>, n: Nid) -> bool {
    ix.st.nodes.get(&n).is_some_and(|x| !x.conflicts.is_empty())
}

/// `has_dangling`: at least one flagged in-edge ([RULES/delete-policy-matrix] FL-003).
// spec: [F13 §6.2] has_dangling
// rule: FL-003
pub fn has_dangling(ix: &Index<'_>, n: Nid) -> bool {
    ix.inn
        .get(&n)
        .is_some_and(|v| v.iter().any(|(_, _, p)| p.flagged))
}

/// `depth`: the number of ancestors in the `parent` forest.
// spec: [F13 §6.2] depth
pub fn depth(ix: &Index<'_>, n: Nid) -> u32 {
    ix.ancestors(n).len() as u32
}

/// `suspect`, single hop ([F13 §6.2]; [AR §3.5]): a `derived_from`, `cites`, `implements` or `depends_on` target is
/// retracted, superseded or deleted, or its current commit differs from the edge's `pinned_commit`; or an `at` target
/// file node is `removed` or deleted.
// spec: [F13 §6.2] suspect
pub fn suspect(ix: &Index<'_>, n: Nid, current: Current<'_>) -> bool {
    let Some(node) = ix.st.live(n) else {
        return false;
    };
    for (k, p) in &node.out {
        let target = ix.st.nodes.get(&k.dst);
        match k.kind.as_str() {
            "derived_from" | "cites" | "implements" | "depends_on" => {
                let bad = match target {
                    None => false,
                    Some(t) if !t.live() => true,
                    Some(t) => t.status == "retracted" || t.status == "superseded",
                };
                let moved = p.pinned.is_some_and(|pin| current(k.dst) != Some(pin));
                if bad || moved {
                    return true;
                }
            }
            "at" if target.is_some_and(|t| !t.live() || t.status == "removed") => return true,
            _ => {}
        }
    }
    false
}

/// Every `P_F15` value of one live node.
pub fn row(ix: &Index<'_>, n: Nid, current: Current<'_>) -> Row {
    let ct = children_total(ix, n);
    Row {
        done: done(ix, n),
        unfinished: unfinished(ix, n),
        unblocked: unblocked(ix, n),
        blocked: blocked(ix, n),
        open_blockers: open_blockers(ix, n),
        open_blockers_exo: open_blockers_exo(ix, n),
        is_blocker: is_blocker(ix, n),
        children_total: ct,
        children_done: children_done(ix, n),
        ready_to_close: ready_to_close(ix, n),
        container: ct > 0,
        suspect: suspect(ix, n, current),
        answered: answered(ix, n),
        conflicted: conflicted(ix, n),
        has_dangling: has_dangling(ix, n),
        depth: depth(ix, n),
        gated: gated(ix, n),
    }
}

/// Every live node's `P_F15` values, recomputed from scratch ([F13] I9: the model's from-scratch definitions are the
/// oracle).
// spec: [F13 §3.3] I9
pub fn recompute_all(st: &State, current: Current<'_>) -> BTreeMap<Nid, Row> {
    let ix = Index::new(st);
    st.nodes
        .iter()
        .filter(|(_, x)| x.live())
        .map(|(n, _)| (*n, row(&ix, *n, current)))
        .collect()
}

/// `D(c)`: the nodes whose value of some `P_F15` predicate differs between the two states ([F13 §6.3]); a node that
/// becomes live or stops being live differs.
// spec: [F13 §6.3] D(c)
// rule: GR-015, DS-009
pub fn affected(parent: &BTreeMap<Nid, Row>, child: &BTreeMap<Nid, Row>) -> BTreeSet<Nid> {
    let mut d = BTreeSet::new();
    for n in parent.keys().chain(child.keys()) {
        if parent.get(n) != child.get(n) {
            d.insert(*n);
        }
    }
    d
}

/// `affected(c)` and `affected_complete` under `store.suspect-budget` ([F17 §8.2]; I42′): `S` the nodes whose
/// `suspect` differs, `A` those whose any other predicate differs; beyond the budget `affected = A`, incomplete.
// spec: [F17 §8.2]
// spec: [F13 §3.6] I42′
// rule: DS-010
pub fn affected_with_budget(
    parent: &State,
    child: &State,
    budget: u32,
    current_parent: Current<'_>,
    current_child: Current<'_>,
) -> (Vec<Nid>, bool) {
    let p = recompute_all(parent, current_parent);
    let c = recompute_all(child, current_child);
    affected_rows(&p, &c, budget)
}

/// [`affected_with_budget`] over the from-scratch rows of the two states.
// spec: [F17 §8.2]
pub fn affected_rows(
    p: &BTreeMap<Nid, Row>,
    c: &BTreeMap<Nid, Row>,
    budget: u32,
) -> (Vec<Nid>, bool) {
    let mut s = BTreeSet::new();
    let mut a = BTreeSet::new();
    for n in p.keys().chain(c.keys()) {
        let (x, y) = (p.get(n), c.get(n));
        if x == y {
            continue;
        }
        let other_differs = match (x, y) {
            (Some(x), Some(y)) => {
                let mut x = x.clone();
                x.suspect = y.suspect;
                x != *y
            }
            _ => true,
        };
        if other_differs {
            a.insert(*n);
        }
        if x.map(|r| r.suspect) != y.map(|r| r.suspect) {
            s.insert(*n);
        }
    }
    if s.len() as u64 <= u64::from(budget) {
        (a.union(&s).copied().collect(), true)
    } else {
        (a.into_iter().collect(), false)
    }
}

/// The subjects a status change of `n` (of `kind`, `from` → `to`) can change a predicate of, by the rows of
/// [RULES/status-machines] `derived-effects` evaluated as data, the node itself included; `@finished` and
/// `@unfinished` are the kind's statuses whose `done` is and is not `yes`.
// spec: [RULES/status-machines] derived-effects
// rule: DE-001, DE-002, DE-003, DE-004, DE-005, DE-006, DE-007, DE-008, DE-009, DE-010, DE-011, DE-012, DE-013, DE-014
// rule: DE-015, DE-016, DE-017, DE-018, DE-019, DE-020, DE-021, DE-022, DE-023, DE-024, DE-025, DE-026, DE-027, DE-028
// rule: DE-029
pub fn effect_subjects(ix: &Index<'_>, n: Nid, kind: &str, from: &str, to: &str) -> BTreeSet<Nid> {
    let schema = &ix.st.schema;
    let finished = |s: &str| schema.value(kind, "status", s).is_some_and(|v| v.done);
    let set_ok = |cell: &str, s: &str| match cell {
        "*" => true,
        "@finished" => finished(s),
        "@unfinished" => !finished(s),
        x => x == s,
    };
    let mut out = BTreeSet::from([n]);
    for r in &crate::rules::rules().table("derived-effects").rows {
        if r.tok("kind") != kind || !set_ok(r.tok("from"), from) || !set_ok(r.tok("to"), to) {
            continue;
        }
        let blocks_dst: Vec<Nid> = ix
            .out_edges(n, "blocks")
            .into_iter()
            .map(|(d, _)| d)
            .collect();
        match r.tok("subject") {
            "self" | "store" | "-" => {}
            "blocks-dst" => out.extend(blocks_dst),
            "blocks-dst-subtree" => {
                for d in blocks_dst {
                    out.insert(d);
                    out.extend(descendants(ix, d));
                }
            }
            "blocks-src" => out.extend(ix.in_edges(n, "blocks").map(|(s, _, _)| s)),
            "gates-dst" => out.extend(ix.out_edges(n, "gates").into_iter().map(|(d, _)| d)),
            "parent" => out.extend(ix.st.live(n).and_then(|x| x.parent)),
            "derivation-src" => {
                for k in ["derived_from", "cites", "implements", "depends_on"] {
                    out.extend(ix.in_edges(n, k).map(|(s, _, _)| s));
                }
            }
            "at-src" => out.extend(ix.in_edges(n, "at").map(|(s, _, _)| s)),
            other => panic!("derived-effects subject {other} has no implementation"),
        }
    }
    out
}

/// The edges of I5′'s combined precedence graph ([F13 §3.2] I5′): `blocks ∪ gates ∪ child→parent ∪
/// {X→D : blocks(X,P), X ∉ subtree(P), D ∈ subtree(P)}`, implied edges re-derived by definition. Each edge is (from,
/// to, the kind that produced it: `blocks`, `gates`, `parent` or `implied`).
pub fn precedence_edges(ix: &Index<'_>) -> Vec<(Nid, Nid, &'static str)> {
    let mut v = Vec::new();
    for (n, node) in &ix.st.nodes {
        if node.live()
            && let Some(p) = node.parent
        {
            v.push((*n, p, "parent"));
        }
        for k in node.out.keys() {
            if !ix.st.nodes.contains_key(&k.dst) {
                continue;
            }
            match k.kind.as_str() {
                "blocks" => {
                    v.push((*n, k.dst, "blocks"));
                    if !ix.in_subtree(*n, k.dst) {
                        for d in descendants(ix, k.dst) {
                            v.push((*n, d, "implied"));
                        }
                    }
                }
                "gates" => v.push((*n, k.dst, "gates")),
                _ => {}
            }
        }
    }
    v
}

/// The live descendants of `p`, `p` excluded.
pub fn descendants(ix: &Index<'_>, p: Nid) -> Vec<Nid> {
    let mut out = Vec::new();
    let mut stack = vec![p];
    let mut seen = BTreeSet::new();
    while let Some(x) = stack.pop() {
        for c in ix.children.get(&x).into_iter().flatten() {
            if seen.insert(*c) {
                out.push(*c);
                stack.push(*c);
            }
        }
    }
    out
}

/// The strongly connected components of a graph by Tarjan's algorithm, iteratively (one DFS, O(V + E)): the component
/// number of every vertex.
pub(crate) fn scc(adj: &BTreeMap<Nid, Vec<Nid>>) -> BTreeMap<Nid, usize> {
    let mut index: BTreeMap<Nid, usize> = BTreeMap::new();
    let mut low: BTreeMap<Nid, usize> = BTreeMap::new();
    let mut on_stack: BTreeSet<Nid> = BTreeSet::new();
    let mut stack: Vec<Nid> = Vec::new();
    let mut comp: BTreeMap<Nid, usize> = BTreeMap::new();
    let mut next = 0usize;
    let mut ncomp = 0usize;
    let empty: Vec<Nid> = Vec::new();
    for &root in adj.keys() {
        if index.contains_key(&root) {
            continue;
        }
        // Frames: (vertex, position of the next successor to visit).
        let mut frames: Vec<(Nid, usize)> = vec![(root, 0)];
        index.insert(root, next);
        low.insert(root, next);
        next += 1;
        stack.push(root);
        on_stack.insert(root);
        while let Some(&(v, i)) = frames.last() {
            let succ = adj.get(&v).unwrap_or(&empty);
            if i < succ.len() {
                let w = succ[i];
                if let Some(top) = frames.last_mut() {
                    top.1 += 1;
                }
                match index.entry(w) {
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        slot.insert(next);
                        low.insert(w, next);
                        next += 1;
                        stack.push(w);
                        on_stack.insert(w);
                        frames.push((w, 0));
                    }
                    std::collections::btree_map::Entry::Occupied(slot) => {
                        if on_stack.contains(&w) {
                            let lw = *slot.get();
                            let lv = low.get_mut(&v).expect("visited");
                            *lv = (*lv).min(lw);
                        }
                    }
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                let lv = low[&v];
                let lp = low.get_mut(&parent).expect("visited");
                *lp = (*lp).min(lv);
            }
            if low[&v] == index[&v] {
                while let Some(w) = stack.pop() {
                    on_stack.remove(&w);
                    comp.insert(w, ncomp);
                    if w == v {
                        break;
                    }
                }
                ncomp += 1;
            }
        }
    }
    comp
}

/// I5′ over the whole combined precedence graph ([F13 §3.2]; [60 §4.2]): `None` when it is acyclic, else the canonical
/// witness, the least edge (by source uid, kind, destination uid) that lies on a cycle ([F13 §5] V03). An edge lies on
/// a cycle exactly when both its ends are in one strongly connected component (a self-loop is one too), which one
/// Tarjan pass decides.
// spec: [F13 §3.2] I5′
pub fn i5p_cycle_witness(st: &State) -> Option<(Nid, &'static str, Nid)> {
    let ix = Index::new(st);
    let edges = precedence_edges(&ix);
    let mut adj: BTreeMap<Nid, Vec<Nid>> = BTreeMap::new();
    for (a, b, _) in &edges {
        adj.entry(*a).or_default().push(*b);
        adj.entry(*b).or_default();
    }
    let comp = scc(&adj);
    let uid = |n: Nid| st.nodes.get(&n).map(|x| x.uid);
    edges
        .iter()
        .filter(|(a, b, _)| a == b || comp[a] == comp[b])
        .map(|(a, b, k)| (*a, *k, *b))
        .min_by_key(|(a, k, b)| (uid(*a), *k, uid(*b)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Schema;
    use crate::state::{Creator, Node, Tomb};
    use crate::value::Uid;
    use proptest::prelude::*;

    fn uid(n: u32) -> Uid {
        let mut b = [0u8; 16];
        b[12..].copy_from_slice(&n.to_be_bytes());
        Uid(b)
    }

    fn add(st: &mut State, n: u32, kind: &str) {
        st.nodes.insert(
            Nid(n),
            Node::new(uid(n), kind, &Schema::default(), Creator::default()),
        );
    }

    fn edge(st: &mut State, a: u32, kind: &str, b: u32) {
        st.nodes.get_mut(&Nid(a)).unwrap().out.insert(
            EdgeKey {
                kind: kind.into(),
                dst: Nid(b),
                disc: None,
            },
            EdgeProps::default(),
        );
    }

    fn none(_: Nid) -> Option<[u8; 32]> {
        None
    }

    /// The node-40 starting state `c0` of [RULES/delete-policy-matrix] `n40-nodes`, `n40-edges` (NX-001 to NX-003).
    #[test]
    fn node_40_starts_blocked() {
        let mut st = State::default();
        for n in [9, 12, 40, 41, 52, 203] {
            add(&mut st, n, "task");
        }
        add(&mut st, 17, "note");
        add(&mut st, 77, "note");
        st.nodes.get_mut(&Nid(40)).unwrap().parent = Some(Nid(9));
        st.nodes.get_mut(&Nid(41)).unwrap().parent = Some(Nid(40));
        edge(&mut st, 40, "blocks", 12);
        edge(&mut st, 203, "blocks", 40);
        let ix = Index::new(&st);
        assert_eq!(open_blockers(&ix, Nid(12)), 1);
        assert!(!unblocked(&ix, Nid(12)));
        assert!(is_blocker(&ix, Nid(40)));
        assert!(container(&ix, Nid(9)));
        assert!(!unblocked(&ix, Nid(40)), "a container");
        assert!(
            !unblocked(&ix, Nid(41)),
            "#203 blocks #40 from outside its subtree"
        );
        assert_eq!(open_blockers_exo(&ix, Nid(40)), 1);
        assert!(blocked(&ix, Nid(41)));
        assert_eq!(depth(&ix, Nid(41)), 2);
        assert_eq!(i5p_cycle_witness(&st), None);
        // A flagged edge from a tombstone keeps its dependent blocked (FL-001, FL-003).
        let x = st.nodes.get_mut(&Nid(40)).unwrap();
        x.tomb = Some(Tomb::default());
        x.parent = None;
        x.out.values_mut().for_each(|p| p.flagged = true);
        let ix = Index::new(&st);
        assert_eq!(open_blockers(&ix, Nid(12)), 1);
        assert!(has_dangling(&ix, Nid(12)));
    }

    #[test]
    fn a_blocker_of_its_own_descendant_is_a_cycle() {
        let mut st = State::default();
        add(&mut st, 1, "task");
        add(&mut st, 2, "task");
        st.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(1));
        edge(&mut st, 1, "blocks", 2);
        assert_eq!(i5p_cycle_witness(&st).map(|w| w.1), Some("blocks"));
    }

    #[test]
    fn gates_count_only_in_gated() {
        let mut st = State::default();
        add(&mut st, 1, "task");
        add(&mut st, 2, "verdict");
        st.nodes
            .get_mut(&Nid(2))
            .unwrap()
            .fields
            .insert("outcome".into(), Value::Enum("fail_fixable".into()));
        edge(&mut st, 2, "gates", 1);
        let ix = Index::new(&st);
        assert!(gated(&ix, Nid(1)));
        assert_eq!(open_blockers(&ix, Nid(1)), 0);
        assert!(unblocked(&ix, Nid(1)));
    }

    /// A brute-force cycle test: the graph has a cycle iff repeatedly removing nodes without in-edges leaves some.
    fn kahn_cyclic(st: &State) -> bool {
        let ix = Index::new(st);
        let edges = precedence_edges(&ix);
        let mut nodes: BTreeSet<Nid> = edges.iter().flat_map(|(a, b, _)| [*a, *b]).collect();
        loop {
            let free: Vec<Nid> = nodes
                .iter()
                .copied()
                .filter(|n| !edges.iter().any(|(a, b, _)| b == n && nodes.contains(a)))
                .collect();
            if free.is_empty() {
                return !nodes.is_empty();
            }
            for f in free {
                nodes.remove(&f);
            }
        }
    }

    proptest! {
        /// GR-015 and the `derived-effects` table: after a status change, every node whose `P_F15` value changed is
        /// one of the subjects the table names for the change (the table is complete).
        #[test]
        fn a_status_change_touches_only_the_named_subjects(
            parents in proptest::collection::vec(proptest::option::of(1u32..7), 6),
            blocks in proptest::collection::vec((1u32..7, 1u32..7), 0..8),
            done in proptest::collection::vec(any::<bool>(), 6),
            who in 1u32..7,
            to_done in any::<bool>(),
        ) {
            let mut st = State::default();
            for n in 1..7 {
                add(&mut st, n, "task");
                if done[n as usize - 1] {
                    st.nodes.get_mut(&Nid(n)).unwrap().status = "done".into();
                }
            }
            for (i, p) in parents.iter().enumerate() {
                let n = i as u32 + 1;
                if let Some(p) = p && *p != n {
                    st.nodes.get_mut(&Nid(n)).unwrap().parent = Some(Nid(*p));
                }
            }
            for (a, b) in blocks {
                if a != b {
                    edge(&mut st, a, "blocks", b);
                }
            }
            if i5p_cycle_witness(&st).is_some() || crate::inv::i4_parent_forest(&st).is_err() {
                return Ok(());
            }
            let before = recompute_all(&st, &none);
            let from = st.nodes[&Nid(who)].status.clone();
            let to = if to_done { "done" } else { "open" };
            let subjects = effect_subjects(&Index::new(&st), Nid(who), "task", &from, to);
            let mut after_st = st.clone();
            after_st.nodes.get_mut(&Nid(who)).unwrap().status = to.into();
            let after = recompute_all(&after_st, &none);
            for n in affected(&before, &after) {
                prop_assert!(subjects.contains(&n), "{} changed but derived-effects names {:?}", n, subjects);
            }
        }

        /// I5′'s DFS witness exists exactly when Kahn's algorithm leaves nodes behind.
        #[test]
        fn the_dfs_agrees_with_kahn(
            parents in proptest::collection::vec(proptest::option::of(1u32..8), 7),
            blocks in proptest::collection::vec((1u32..8, 1u32..8), 0..8),
        ) {
            let mut st = State::default();
            for n in 1..8 {
                add(&mut st, n, "task");
            }
            for (i, p) in parents.iter().enumerate() {
                let n = i as u32 + 1;
                if let Some(p) = p && *p != n {
                    st.nodes.get_mut(&Nid(n)).unwrap().parent = Some(Nid(*p));
                }
            }
            for (a, b) in blocks {
                if a != b {
                    edge(&mut st, a, "blocks", b);
                }
            }
            prop_assert_eq!(i5p_cycle_witness(&st).is_some(), kahn_cyclic(&st));
            let _ = recompute_all(&st, &none);
        }
    }
}
