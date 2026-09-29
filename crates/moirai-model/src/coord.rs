//! I26′ by its state definition ([F13 §4.1]; [RULES/state-definition] `view-kinds`, `hold-values`, `origin-rules`,
//! PD-012 to PD-015): holds, origins and exclusion evaluated over all live refs and the commit DAG, never from markers
//! ([60 §4.2]), and the tip-only `ready` of [F13 §6.2] with its runtime clauses (PD-009 to PD-017).
//!
//! A hold at a commit is a function of the node's existence and status keys at `state_at(c)`, which
//! [`Dag::hold_keys_at`] reads without materialising the state.

use crate::clock::Env;
use crate::dag::{Dag, RefKind};
use crate::derived::{self, Index, Row};
use crate::lease::{self, Lease};
use crate::rules::rules;
use crate::state::{Alloc, KState, KVal, Side, State};
use crate::value::{Nid, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The hold of a (kind, status) or a tombstone ([RULES/state-definition] `hold-values`): the first row that matches.
// spec: [RULES/state-definition] hold-values
// rule: HV-001, HV-002, HV-003, HV-004
pub fn hold_of(kind: &str, deleted: bool, status: &str) -> &'static str {
    let state = if deleted { "deleted" } else { status };
    for r in &rules().table("hold-values").rows {
        let kind_ok = r.tok("kind") == "*" || r.tok("kind") == kind;
        let st_ok = r.tok("state") == state
            || (r.tok("state") == "deleted" && deleted)
            || r.tok("state") == "other";
        if kind_ok && st_ok {
            return match r.tok("hold") {
                "done" => "done",
                "cancelled" => "cancelled",
                "deleted" => "deleted",
                _ => "none",
            };
        }
    }
    "none"
}

/// The hold of a node at a state: `done`, `cancelled`, `deleted` or `none` (absent from the state).
pub fn hold(st: &State, n: Nid) -> &'static str {
    match st.nodes.get(&n) {
        None => "none",
        Some(x) => hold_of(&x.kind, !x.live(), &x.status),
    }
}

/// Whether a hold value lies in S = {`done`, `cancelled`, `deleted`}.
pub fn closed(h: &str) -> bool {
    matches!(h, "done" | "cancelled" | "deleted")
}

/// The view-kinds row of a ref kind: (holds count, tip reads) ([RULES/state-definition] VK rows).
// rule: VK-001, VK-002, VK-003, VK-004, VK-005, VK-006, VK-007
pub fn view_kind(kind: RefKind) -> (bool, bool) {
    let r = rules()
        .table("view-kinds")
        .rows
        .iter()
        .find(|r| r.tok("ref_kind") == kind.token())
        .unwrap_or_else(|| panic!("view-kinds has no row for {}", kind.token()));
    (r.tok("holds_count") == "yes", r.tok("tip_reads") == "yes")
}

/// The value a node holds for its existence key: the plain value, or a conflict's provisional side, as
/// [`State::apply`] folds an existence key.
fn existence_value(k: &KState) -> Option<KVal> {
    match k {
        KState::Plain(p) => p.clone(),
        KState::Conflict(c) => match (c.prov, &c.ours, &c.theirs) {
            (Some(Side::Theirs), _, t) => t.clone(),
            (_, o, _) => o.clone(),
        },
    }
}

/// The value a node's member holds for any other key: the plain value, or a conflict's provisional side, as
/// [`State::apply`] folds a node key.
fn member_value(k: &KState) -> Option<KVal> {
    match k {
        KState::Plain(p) => p.clone(),
        KState::Conflict(c) => match (c.prov, &c.ours, &c.theirs) {
            (Some(Side::Theirs), _, t) => t.clone(),
            (Some(Side::Ours), o, _) => o.clone(),
            (None, None, t) => t.clone(),
            (None, o, _) => o.clone(),
        },
    }
}

/// The I26′ evaluator over one store's DAG: holds and origins memoised per (commit, node), ancestries per ref.
pub struct Oracle<'a> {
    dag: &'a Dag,
    holds: BTreeMap<(u64, Nid), &'static str>,
    origins: BTreeMap<(u64, Nid), u64>,
    anc: BTreeMap<String, BTreeSet<u64>>,
}

impl<'a> Oracle<'a> {
    /// An evaluator over `dag`. The allocation is not needed: holds read the keys, not states.
    pub fn new(dag: &'a Dag, _alloc: &'a dyn Alloc) -> Oracle<'a> {
        Oracle {
            dag,
            holds: BTreeMap::new(),
            origins: BTreeMap::new(),
            anc: BTreeMap::new(),
        }
    }

    /// `hold(c, #N)` at `state_at(c)`: from the node's existence key (absent: `none`; a tombstone: `deleted`) and its
    /// status key (absent: the kind's initial status, which is never a closed hold).
    pub fn hold_at(&mut self, c: u64, n: Nid) -> &'static str {
        if let Some(h) = self.holds.get(&(c, n)) {
            return h;
        }
        let (exist, status) = self.dag.hold_keys_at(Some(c), n);
        let h = match existence_value(&exist) {
            None => "none",
            Some(KVal::Deleted { kind, .. }) => hold_of(&kind, true, ""),
            Some(KVal::Live(kind)) => match member_value(&status) {
                Some(KVal::Status { status, .. }) => hold_of(&kind, false, &status),
                _ => hold_of(&kind, false, ""),
            },
            Some(other) => panic!("{other:?} is not an existence value"),
        };
        self.holds.insert((c, n), h);
        h
    }

    /// `org(c, #N)` of a closed hold: the OR rows evaluated in order, first match ([RULES/state-definition]
    /// `origin-rules`).
    // spec: [F13 §4.1] origin
    // rule: OR-001, OR-002, OR-003, OR-004, OR-005, OR-006
    pub fn origin(&mut self, c: u64, n: Nid) -> u64 {
        // Walk the chain of `org-p1` steps iteratively, so a long history never deepens the stack; `org-p2` recurses.
        let mut path = Vec::new();
        let mut cur = c;
        let o = loop {
            if let Some(o) = self.origins.get(&(cur, n)) {
                break *o;
            }
            let parents = self.dag.commits[&cur].parents.clone();
            let v = self.hold_at(cur, n);
            let p1_same = parents.first().is_some_and(|p| self.hold_at(*p, n) == v);
            let p2_same = parents.get(1).is_some_and(|p| self.hold_at(*p, n) == v);
            let t = rules().table("origin-rules");
            let row = t
                .rows
                .iter()
                .find(|r| {
                    r.int("parents") as usize == parents.len().min(2)
                        && match r.tok("condition") {
                            "always" => true,
                            "p1-same" => p1_same,
                            "p1-differs" => !p1_same,
                            "p2-same" => p2_same,
                            "both-differ" => !p1_same && !p2_same,
                            other => panic!("origin condition {other} has no implementation"),
                        }
                })
                .unwrap_or_else(|| panic!("origin-rules has no row for commit {cur}"));
            match row.tok("origin") {
                "c" => break cur,
                "org-p1" => {
                    path.push(cur);
                    cur = parents[0];
                }
                "org-p2" => {
                    let o = self.origin(parents[1], n);
                    self.origins.insert((cur, n), o);
                    break o;
                }
                other => panic!("origin {other} has no implementation"),
            }
        };
        self.origins.insert((cur, n), o);
        for p in path {
            self.origins.insert((p, n), o);
        }
        o
    }

    /// `anc*(tip of R)`, memoised per ref.
    fn ancestry(&mut self, r: &str) -> &BTreeSet<u64> {
        if !self.anc.contains_key(r) {
            let tip = self.dag.live(r).and_then(|x| x.tip);
            self.anc.insert(r.to_string(), self.dag.ancestors(tip));
        }
        &self.anc[r]
    }

    /// The live refs X ≠ R that hold `#N` (a closed hold at a ref whose kind counts) with an origin R has not
    /// absorbed: (X's name, hold, origin) — PD-012 over all live refs.
    pub fn holders_elsewhere(&mut self, r: &str, n: Nid) -> Vec<(String, &'static str, u64)> {
        let dag = self.dag;
        let mut out = Vec::new();
        for x in dag.live_refs() {
            if x.name == r || !view_kind(x.kind).0 {
                continue;
            }
            let Some(t) = x.tip else { continue };
            let h = self.hold_at(t, n);
            if !closed(h) {
                continue;
            }
            let o = self.origin(t, n);
            if !self.ancestry(r).contains(&o) {
                out.push((x.name.clone(), h, o));
            }
        }
        out
    }

    /// PD-012 (I26′): `#N` is excluded on R.
    // spec: [F13 §3.4] I26′
    // rule: PD-012
    pub fn i26p_excluded(&mut self, r: &str, n: Nid) -> bool {
        !self.holders_elsewhere(r, n).is_empty()
    }

    /// PD-013: `deleted_elsewhere`.
    // rule: PD-013
    pub fn deleted_elsewhere(&mut self, r: &str, n: Nid) -> bool {
        self.holders_elsewhere(r, n)
            .iter()
            .any(|(_, h, _)| *h == "deleted")
    }

    /// PD-014: `settled_elsewhere`.
    // rule: PD-014
    pub fn settled_elsewhere(&mut self, r: &str, n: Nid) -> bool {
        self.holders_elsewhere(r, n)
            .iter()
            .any(|(_, h, _)| *h == "done" || *h == "cancelled")
    }
}

/// PD-018: `blocking` on R lists `#N` when it is a blocker, a task, and not excluded on R ([AR §3.4] I26′: "never
/// listed there as a live blocker").
// rule: PD-018
pub fn blocking_listed(o: &mut Oracle<'_>, ix: &Index<'_>, r: &str, n: Nid) -> bool {
    derived::is_blocker(ix, n)
        && ix.st.live(n).is_some_and(|x| x.kind == "task")
        && !o.i26p_excluded(r, n)
}

/// PD-008 and PD-016: `defer_until ≤ now()` with the wall clock in whole seconds (CK-7).
// rule: PD-008, PD-016
pub fn defer_ok(st: &State, n: Nid, now_s: i64) -> bool {
    match st.nodes.get(&n).and_then(|x| x.fields.get("defer_until")) {
        Some(Value::Int(t)) => *t <= now_s,
        _ => true,
    }
}

/// `ready` of `#N` on ref R for a caller (PD-009 to PD-011, PD-015, PD-016): at R's tip, `unblocked` ∧ no live lease
/// of another holder ∧ not excluded ∧ `defer_until ≤` the wall clock now. Tip only (VD-005).
// spec: [RULES/state-definition] predicates
// rule: PD-009, PD-010, PD-011, PD-015
pub fn ready(
    o: &mut Oracle<'_>,
    ix: &Index<'_>,
    r: &str,
    n: Nid,
    leases: &BTreeMap<u64, Lease>,
    env: &Env,
    caller: Option<&str>,
) -> bool {
    derived::unblocked(ix, n)
        && defer_ok(ix.st, n, env.now_s())
        && !lease::excludes_from_ready(leases.values(), env, n, caller)
        && !o.i26p_excluded(r, n)
}

/// The ready tasks of ref R for a caller, ascending by `#N`: [`ready`] of every node, with `unblocked` read from
/// the view's from-scratch rows when the caller has them.
pub fn ready_set(
    dag: &Dag,
    alloc: &dyn Alloc,
    r: &str,
    leases: &BTreeMap<u64, Lease>,
    env: &Env,
    caller: Option<&str>,
    rows: Option<&BTreeMap<Nid, Row>>,
) -> Vec<Nid> {
    let tip = dag.live(r).and_then(|x| x.tip);
    let st = dag.state_at(tip, alloc);
    let mut o = Oracle::new(dag, alloc);
    // The live task leases by task, so each node reads only its own.
    let mut by_task: BTreeMap<Nid, Vec<&Lease>> = BTreeMap::new();
    for l in leases.values().filter(|l| l.ended.is_none()) {
        if let Some(t) = l.task {
            by_task.entry(t).or_default().push(l);
        }
    }
    let computed;
    let rows = match rows {
        Some(r) => r,
        None => {
            computed = derived::recompute_all(&st, &|_| None);
            &computed
        }
    };
    st.nodes
        .keys()
        .copied()
        .filter(|n| {
            rows.get(n).is_some_and(|x| x.unblocked)
                && defer_ok(&st, *n, env.now_s())
                && !lease::excludes_from_ready(
                    by_task.get(n).into_iter().flatten().copied(),
                    env,
                    *n,
                    caller,
                )
                && !o.i26p_excluded(r, *n)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_follow_the_table() {
        assert_eq!(hold_of("task", false, "done"), "done");
        assert_eq!(hold_of("task", false, "cancelled"), "cancelled");
        assert_eq!(hold_of("task", false, "open"), "none");
        assert_eq!(hold_of("note", true, ""), "deleted");
        assert_eq!(hold_of("note", false, "retracted"), "none");
        assert!(closed("deleted") && !closed("none"));
        assert_eq!(view_kind(RefKind::Work), (true, true));
    }
}
