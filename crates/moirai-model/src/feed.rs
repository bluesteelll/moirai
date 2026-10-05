//! The change feed ([AR §6.3]; [60 §4.2] row "Leases, fencing, idempotency, change feed, `next_id`": "a `Vec` of feed
//! entries; relevance filters by definition"): one store-wide sequence of entries, each with its `seq` and its `ref`
//! ([AR §2.16] "change feed `seq` store-wide with `ref` on every entry") — every changed node key of every commit, the
//! lease and marker events of its group, and the ref moves no commit carries — in the order the store recorded them.
//! The rows have the columns of LQ's `changes` relation ([LQ/std §2.9], §2.12: `seq, ref, commit, node, op, aspect,
//! name, actor, affected`); the named queries `std.changes` and `std.delta` ([LQ/std §4.8], §4.20) filter them, and
//! `relevant_to(n, agent)` ([LQ/std §2.10]; [50 §2.6]) is evaluated by its definition over the view and the lease
//! table: nodes the agent claimed, is blocked on, authored or cited ([AR §6.3]).
//!
//! WP-93b's evaluator reads [`Feed::since`] for `CALL changes(...)` and calls [`relevant_to`]; [`changes`] and
//! [`delta`] are the two named queries' semantics, which that evaluation must equal.

use crate::dag::Dag;
use crate::lease::{Lease, LeaseKind};
use crate::state::{Aspect, KState, Key, State};
use crate::value::Nid;
use std::collections::{BTreeMap, BTreeSet};

/// One entry of the feed, one row of `changes` ([LQ/std §2.12], §2.15).
///
/// The entries that are not key changes follow [LQ/std §2.15]'s table: a lease event (`grant`, `end` for every end reason
/// of [F05 §9.4], `renew`, `move`) with `aspect` `lease`, `name` `L-<n>`, the task, the lease's branch (the new branch for
/// `move`), the group's commit (absent for `renew` and outside a commit group) and the holder as `actor`; a marker entry
/// (`settled`, `deleted`, `cleared`) with `aspect` `marker`, `name` `s<seq>` of its origin commit, the node, the origin
/// ref and commit, and the entry's actor, else the origin commit's; a ref move no commit carries (`create`, `delete`,
/// `undo`, `op-restore`, `park`) with `aspect` `ref`, the ref's name, the moved ref, the new tip (absent for a deletion)
/// and the record's actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// `seq`: the commit's seq; for an entry no commit carries, the seq of the newest commit appended before it.
    pub seq: u64,
    /// `ref`: the ref of [LQ/std §2.15]'s table: the commit's ref, the lease's branch, the marker's origin ref, the moved
    /// ref.
    pub ref_: String,
    /// `commit`: the commit column of [LQ/std §2.15]'s table, when the entry has one.
    pub commit: Option<u64>,
    /// `node`: the node, when the entry has one.
    pub node: Option<Nid>,
    /// `op`: `+`, `-` or `~` for a key change ([API §5.7] `change`), else [LQ/std §2.15]'s.
    pub op: String,
    /// `aspect`: the key's aspect word of [API §5.7], or `lease`, `marker`, `ref`.
    pub aspect: String,
    /// `name`: the key's name of [API §5.7]; the lease id, the marker's origin commit (`s<seq>`), or the ref name.
    pub name: String,
    /// `actor`: the commit's actor, the lease's holder, the marker entry's actor, or the ref move's actor.
    pub actor: String,
    /// `affected`: the commit's `affected` ids ([F13 §6.3]); empty for other entries.
    pub affected: Vec<Nid>,
    /// The commit whose group carries the entry ([F05 §4.7]); `None` for an entry no commit carries (a ref group, a
    /// lease-only group), which `since(s)` counts after commit `seq`.
    pub group: Option<u64>,
}

/// The aspect word and name of a node key ([API §5.7]).
fn aspect_name(st: &State, a: &Aspect, kind: &str) -> (String, String) {
    match a {
        Aspect::Existence => ("existence".into(), kind.to_string()),
        Aspect::Status => ("status".into(), "status".into()),
        Aspect::Hierarchy => ("parent".into(), "parent".into()),
        Aspect::Field(f) => ("field".into(), f.clone()),
        Aspect::Observation => ("observation".into(), "observation".into()),
        Aspect::Counter(f) => ("counter".into(), f.clone()),
        Aspect::Edge(e) => (
            "edge".into(),
            st.schema
                .edge(&e.kind)
                .map_or_else(|| e.kind.to_uppercase(), |x| x.lq_name.clone()),
        ),
        Aspect::Body => ("body".into(), "body".into()),
    }
}

/// The rows of one commit by definition: one per node key its net changeset changes, in the key order of the
/// changeset, `op` as [API §5.7]'s `change`. `st` is the commit's state (for the edge names and the node kinds).
// spec: [AR §6.3]
pub fn commit_rows(dag: &Dag, st: &State, c: u64) -> Vec<Change> {
    let x = &dag.commits[&c];
    let ref_ = dag.refs[&x.ref_id].name.clone();
    x.changeset
        .iter()
        .filter_map(|(k, (before, after))| {
            let Key::Node(n, a) = k else { return None };
            let kind = st.nodes.get(n).map_or("", |x| x.kind.as_str());
            let (aspect, name) = aspect_name(st, a, kind);
            let op = if *before == KState::ABSENT {
                "+"
            } else if *after == KState::ABSENT {
                "-"
            } else {
                "~"
            };
            Some(Change {
                seq: c,
                ref_: ref_.clone(),
                commit: Some(c),
                node: Some(*n),
                op: op.into(),
                aspect,
                name,
                actor: x.actor.clone(),
                affected: x.affected.clone(),
                group: Some(c),
            })
        })
        .collect()
}

/// The store's feed: every entry in the order it was recorded, which is [LQ/std §2.15]'s order of ties.
#[derive(Clone, Debug, Default)]
pub struct Feed {
    /// The entries.
    pub rows: Vec<Change>,
}

impl Feed {
    /// Records a new commit's rows.
    pub fn commit(&mut self, dag: &Dag, st: &State, c: u64) {
        self.rows.extend(commit_rows(dag, st, c));
    }

    /// Records one entry without a key change (a lease event, a marker entry, a ref move) with its columns of
    /// [LQ/std §2.15] and the commit whose group carries it (`group`).
    #[allow(clippy::too_many_arguments)]
    pub fn event(
        &mut self,
        seq: u64,
        ref_: &str,
        commit: Option<u64>,
        node: Option<Nid>,
        op: &str,
        aspect: &str,
        name: String,
        actor: &str,
        group: Option<u64>,
    ) {
        self.rows.push(Change {
            seq,
            ref_: ref_.to_string(),
            commit,
            node,
            op: op.to_string(),
            aspect: aspect.to_string(),
            name,
            actor: actor.to_string(),
            affected: Vec::new(),
            group,
        });
    }

    /// `CALL changes(since: s, ref: r)` ([LQ/std §2.9]; §2.15 "The `since(s)` cut"): the entries after commit s —
    /// those whose seq is above s, and those of seq s that no commit carries (recorded after commit s's group) — on ref
    /// `r` when given; every entry for `since` = 0.
    // spec: [LQ/std §2.15]
    pub fn since<'a>(
        &'a self,
        since: u64,
        ref_: Option<&'a str>,
    ) -> impl Iterator<Item = &'a Change> + 'a {
        self.rows.iter().filter(move |c| {
            let after = since == 0 || c.seq > since || (c.seq == since && c.group.is_none());
            after && ref_.is_none_or(|r| c.ref_ == r)
        })
    }
}

/// `relevant_to(n, agent)` by its definition ([AR §6.3]; [50 §2.6]) on a view and the lease table: `#N` is relevant to
/// agent A when A **claimed** it (some task lease on it, live or ended, is held by A), A **is blocked on** it (a
/// `blocks` or `gates` edge from it reaches a task A claimed), A **authored** it (its `CREATOR` actor is A), or A
/// **cited** it (a `cites` or `mentions` edge reaches it from a node A authored or claimed). An entry without a node is
/// relevant to nobody.
// spec: [AR §6.3]
// spec: [LQ/std §2.10] relevant_to
pub fn relevant_to(st: &State, leases: &BTreeMap<u64, Lease>, n: Option<Nid>, agent: &str) -> bool {
    let Some(n) = n else { return false };
    let claimed = |m: Nid| {
        leases
            .values()
            .any(|l| l.kind == LeaseKind::Task && l.task == Some(m) && l.holder == agent)
    };
    let authored = |m: Nid| st.nodes.get(&m).is_some_and(|x| x.creator.actor == agent);
    if claimed(n) || authored(n) {
        return true;
    }
    let Some(x) = st.nodes.get(&n) else {
        return false;
    };
    // Blocked on: an out-edge of #N that blocks or gates a claimed task.
    if x.out
        .keys()
        .any(|e| (e.kind == "blocks" || e.kind == "gates") && claimed(e.dst))
    {
        return true;
    }
    // Cited: a cites or mentions edge into #N from a node A authored or claimed.
    st.nodes.iter().any(|(m, y)| {
        (authored(*m) || claimed(*m))
            && y.out
                .keys()
                .any(|e| (e.kind == "cites" || e.kind == "mentions") && e.dst == n)
    })
}

/// Every node relevant to agent A on a view, the set [`relevant_to`] tests one node against, built in one pass over the
/// view's nodes and out-edges and the lease table: what A claimed or authored, the sources of `blocks` and `gates`
/// edges into a task A claimed, and the live destinations of `cites` and `mentions` edges from a node A authored or
/// claimed. `std.changes` and `std.delta` build it once per evaluation, so each entry costs one lookup.
// spec: [LQ/std §2.10] relevant_to
pub fn relevant_set(st: &State, leases: &BTreeMap<u64, Lease>, agent: &str) -> BTreeSet<Nid> {
    let claimed: BTreeSet<Nid> = leases
        .values()
        .filter(|l| l.kind == LeaseKind::Task && l.holder == agent)
        .filter_map(|l| l.task)
        .collect();
    let authored: BTreeSet<Nid> = st
        .nodes
        .iter()
        .filter(|(_, x)| x.creator.actor == agent)
        .map(|(n, _)| *n)
        .collect();
    let mut out: BTreeSet<Nid> = claimed.union(&authored).copied().collect();
    for (n, x) in &st.nodes {
        let source = claimed.contains(n) || authored.contains(n);
        for e in x.out.keys() {
            match e.kind.as_str() {
                "blocks" | "gates" if claimed.contains(&e.dst) => {
                    out.insert(*n);
                }
                "cites" | "mentions" if source && st.nodes.contains_key(&e.dst) => {
                    out.insert(e.dst);
                }
                _ => {}
            }
        }
    }
    out
}

/// `std.changes($since, $about, $for_agent, $all)` ([LQ/std §4.8]): the entries after `since` on the view's ref (every
/// ref with `all`), restricted to `about` and to the entries relevant to `for_agent`, ordered by `seq` (stably, in
/// recorded order).
// spec: [LQ/std §4.8]
#[allow(clippy::too_many_arguments)]
pub fn changes<'a>(
    feed: &'a Feed,
    view_ref: &str,
    since: u64,
    about: Option<&[Nid]>,
    for_agent: Option<&str>,
    all: bool,
    st: &State,
    leases: &BTreeMap<u64, Lease>,
) -> Vec<&'a Change> {
    let relevant = for_agent.map(|a| relevant_set(st, leases, a));
    let mut v: Vec<&Change> = feed
        .since(since, None)
        .filter(|c| all || c.ref_ == view_ref)
        .filter(|c| about.is_none_or(|a| c.node.is_some_and(|n| a.contains(&n))))
        .filter(|c| {
            relevant
                .as_ref()
                .is_none_or(|r| c.node.is_some_and(|n| r.contains(&n)))
        })
        .collect();
    v.sort_by_key(|c| c.seq);
    v
}

/// `std.delta($since, $agent)` ([LQ/std §4.20]; the `UserPromptSubmit` delta and C8): the entries after `since`, of
/// every ref, relevant to the agent and not its own, by `seq`, at most 12.
// spec: [LQ/std §4.20]
pub fn delta<'a>(
    feed: &'a Feed,
    since: u64,
    agent: &str,
    st: &State,
    leases: &BTreeMap<u64, Lease>,
) -> Vec<&'a Change> {
    let relevant = relevant_set(st, leases, agent);
    let mut v: Vec<&Change> = feed
        .since(since, None)
        .filter(|c| c.node.is_some_and(|n| relevant.contains(&n)) && c.actor != agent)
        .collect();
    v.sort_by_key(|c| c.seq);
    v.truncate(12);
    v
}
