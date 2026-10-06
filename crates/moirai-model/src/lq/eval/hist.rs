//! The history relations by replay over the commit DAG ([50 §3.9] item 4; [LQ/std §2.9], §2.12, §2.15): `history`,
//! `blame` and `log` scoped by their range or by reachability from the view's commit, `diff` over a two- or three-dot
//! range (the rows of [API §5.7]), the change feed of `changes`, a staging ref's `violations`, `across` over several
//! views, and `refs`.

use super::val::V;
use super::view::Ev;
use crate::err::{Refusal, Res};
use crate::lq::cast::CExpr;
use crate::lq::diag::Code;
use crate::state::{Aspect, KState, Key, State, diff};
use crate::value::{Nid, hex};
use std::collections::BTreeSet;

/// One key of `across()`: its aspect, its name and its value on each view.
type KeyVals = (String, String, Vec<(String, V)>);

/// A revision argument: one revision, or a range ([LQ/lexical §7.4]).
enum Range {
    One(Option<u64>),
    Two(Option<u64>, Option<u64>),
    Three(Option<u64>, Option<u64>),
}

impl Ev<'_> {
    /// The text of a revision node, for notices.
    fn rev_label(e: &CExpr) -> String {
        match e {
            CExpr::RHead => "HEAD".into(),
            CExpr::RRef(r) => r.clone(),
            CExpr::RCommit(c) => format!("c{}", &hex(c)[..8]),
            CExpr::RSuf(b, k, n) => {
                let s = match k {
                    1 => "~",
                    2 => "^",
                    _ => "@",
                };
                format!("{}{s}{n}", Self::rev_label(b))
            }
            _ => String::new(),
        }
    }

    fn range(&self, e: &CExpr) -> Res<Range> {
        Ok(match e {
            CExpr::RRange(a, 1, b) => Range::Two(self.w.commit_of(a)?, self.w.commit_of(b)?),
            CExpr::RRange(a, _, b) => Range::Three(self.w.commit_of(a)?, self.w.commit_of(b)?),
            x => Range::One(self.w.commit_of(x)?),
        })
    }

    /// The commits a range names: `a..b` those reachable from b and not from a, `a...b` those reachable from exactly
    /// one of them, one revision every commit reachable from it.
    fn commits_of(&self, r: &Range) -> BTreeSet<u64> {
        let dag = &self.store().dag;
        match r {
            Range::One(x) => dag.ancestors(*x),
            Range::Two(a, b) => {
                let na = dag.ancestors(*a);
                dag.ancestors(*b).difference(&na).copied().collect()
            }
            Range::Three(a, b) => {
                let na = dag.ancestors(*a);
                let nb = dag.ancestors(*b);
                na.symmetric_difference(&nb).copied().collect()
            }
        }
    }

    /// The commits a history relation reads: its range, else every commit reachable from the view's ([50 §3.9] item 4).
    fn scope_commits(&self, range: Option<&CExpr>) -> Res<BTreeSet<u64>> {
        Ok(match range {
            Some(r) => self.commits_of(&self.range(r)?),
            None => self.store().dag.ancestors(self.v.commit),
        })
    }

    /// The aspect word and name of a key ([API §5.7]).
    pub fn aspect_name(&self, a: &Aspect, kind: &str) -> (String, String) {
        match a {
            Aspect::Existence => ("existence".into(), kind.to_string()),
            Aspect::Status => ("status".into(), "status".into()),
            Aspect::Hierarchy => ("parent".into(), "parent".into()),
            Aspect::Field(f) => ("field".into(), f.clone()),
            Aspect::Observation => ("observation".into(), "observation".into()),
            Aspect::Counter(f) => ("counter".into(), f.clone()),
            Aspect::Edge(e) => ("edge".into(), self.lq_name(&e.kind)),
            Aspect::Body => ("body".into(), "body".into()),
        }
    }

    /// The kind of a node in a state, else in the view.
    fn kind_in(&self, st: &State, n: Nid) -> String {
        st.nodes
            .get(&n)
            .or_else(|| self.node(n))
            .map_or_else(String::new, |x| x.kind.clone())
    }

    /// The `op` of a key change: `+`, `-` or `~` ([API §5.7] `change`).
    fn op(before: &KState, after: &KState) -> &'static str {
        if *before == KState::ABSENT {
            "+"
        } else if *after == KState::ABSENT {
            "-"
        } else {
            "~"
        }
    }

    /// `history(n, field, in)` ([50 §2.6]): one row per key of n that a commit of the scope changed, newest commit first,
    /// a commit's keys in their order.
    // spec: [50 §2.6] history
    pub fn history_rows(
        &self,
        n: Nid,
        field: Option<&str>,
        range: Option<&CExpr>,
    ) -> Res<Vec<Vec<V>>> {
        let dag = &self.store().dag;
        let mut rows = Vec::new();
        // A node's kind never changes ([F08 §3.1]): the view's node or tombstone gives it, else the state of the
        // first commit that touches the node, read once.
        let mut kind: Option<String> = self.node(n).map(|x| x.kind.clone());
        for c in self.scope_commits(range)? {
            let x = &dag.commits[&c];
            for (k, (before, after)) in &x.changeset {
                let Key::Node(m, a) = k else { continue };
                if *m != n {
                    continue;
                }
                let kind = kind
                    .get_or_insert_with(|| {
                        self.kind_in(&dag.state_at(Some(c), &self.store().alloc), n)
                    })
                    .clone();
                let (aspect, name) = self.aspect_name(a, &kind);
                if field.is_some_and(|f| f != name) {
                    continue;
                }
                rows.push(vec![
                    V::Int(c as i64),
                    V::Rev(c),
                    V::text(
                        dag.refs
                            .get(&x.ref_id)
                            .map_or(String::new(), |r| r.name.clone()),
                    ),
                    V::text(x.actor.clone()),
                    if x.role.is_empty() {
                        V::Absent
                    } else {
                        V::text(x.role.clone())
                    },
                    V::Time((x.append_hlc >> 16) as i64),
                    V::text(Self::op(before, after)),
                    V::text(aspect),
                    V::text(name),
                    self.kstate_v(&kind, a, before),
                    self.kstate_v(&kind, a, after),
                    if x.message.is_empty() {
                        V::Absent
                    } else {
                        V::text(x.message.clone())
                    },
                    x.stmt_sym.clone().map_or(V::Absent, V::Text),
                ]);
            }
        }
        // Newest first, the order of `std.history` ([LQ/std §3]); a commit's keys keep their order.
        rows.sort_by(|a, b| super::val::cmp_total(&b[0], &a[0]));
        Ok(rows)
    }

    /// `blame(n)` ([50 §2.6]): for each key n holds on the view, the newest commit of the view's first-parent chain
    /// that changed it.
    // spec: [50 §2.6] blame
    pub fn blame_rows(&self, n: Nid) -> Vec<Vec<V>> {
        let dag = &self.store().dag;
        let Some(x) = self.node(n) else {
            return Vec::new();
        };
        let chain = dag.chain(self.v.commit);
        let mut rows = Vec::new();
        for a in x.aspects(x.kind == "artifact") {
            let k = Key::Node(n, a.clone());
            let Some(c) = chain
                .iter()
                .find(|c| dag.commits[c].changeset.contains_key(&k))
            else {
                continue;
            };
            let cm = &dag.commits[c];
            let (aspect, name) = self.aspect_name(&a, &x.kind);
            rows.push(vec![
                V::text(aspect),
                V::text(name),
                self.kstate_v(&x.kind, &a, &x.kstate(&self.st().schema, &a)),
                V::Int(*c as i64),
                V::Rev(*c),
                V::text(cm.actor.clone()),
                V::Time((cm.append_hlc >> 16) as i64),
            ]);
        }
        // The order of `std.blame` ([LQ/std §4.13]): aspect, name.
        rows.sort_by(|a, b| {
            super::val::cmp_total(&a[0], &b[0]).then_with(|| super::val::cmp_total(&a[1], &b[1]))
        });
        rows
    }

    /// `log(range, actor, touching)` ([50 §2.6]): the commits of the scope, filtered by actor and by a node they
    /// change, newest first.
    // spec: [50 §2.6] log
    pub fn log_rows(
        &self,
        range: Option<&CExpr>,
        actor: Option<&str>,
        touching: Option<Nid>,
    ) -> Res<Vec<Vec<V>>> {
        let dag = &self.store().dag;
        let mut rows = Vec::new();
        for c in self.scope_commits(range)? {
            let x = &dag.commits[&c];
            if actor.is_some_and(|a| a != x.actor) {
                continue;
            }
            if let Some(t) = touching
                && !x
                    .changeset
                    .keys()
                    .any(|k| matches!(k, Key::Node(m, _) if *m == t))
            {
                continue;
            }
            rows.push(vec![
                V::Rev(c),
                V::Int(c as i64),
                V::text(
                    dag.refs
                        .get(&x.ref_id)
                        .map_or(String::new(), |r| r.name.clone()),
                ),
                V::text(x.kind),
                V::text(x.actor.clone()),
                if x.role.is_empty() {
                    V::Absent
                } else {
                    V::text(x.role.clone())
                },
                V::Time((x.append_hlc >> 16) as i64),
                if x.message.is_empty() {
                    V::Absent
                } else {
                    V::text(x.message.clone())
                },
                V::Int(x.changeset.len() as i64),
            ]);
        }
        // Newest first, the order of `std.log` ([LQ/std §3]).
        rows.sort_by(|a, b| super::val::cmp_total(&b[1], &a[1]));
        Ok(rows)
    }

    /// The newest commit of the first-parent chain from `tip` down to (not including) `base` that changed a key.
    fn last_change(&self, tip: Option<u64>, base: Option<u64>, k: &Key) -> Option<u64> {
        let dag = &self.store().dag;
        dag.chain(tip)
            .into_iter()
            .take_while(|c| Some(*c) != base)
            .find(|c| dag.commits[c].changeset.contains_key(k))
    }

    /// The rows of one state difference ([API §5.7]), with a side and the commit chain their last change is read
    /// from (`None` for a `DRY` diff).
    pub fn diff_table(
        &self,
        before: &State,
        after: &State,
        side: Option<&str>,
        chain: Option<(Option<u64>, Option<u64>)>,
        scope: Option<&BTreeSet<Nid>>,
    ) -> Vec<(Key, Vec<V>)> {
        let mut rows = Vec::new();
        for (k, (b, a)) in diff(before, after) {
            if let Key::Node(n, _) = &k
                && scope.is_some_and(|s| !s.contains(n))
            {
                continue;
            }
            rows.push((
                k.clone(),
                self.diff_row(before, after, &k, &b, &a, side, chain),
            ));
        }
        rows
    }

    /// One `diff` row ([API §5.7]).
    #[allow(clippy::too_many_arguments)]
    // spec: [API §5.7] change
    fn diff_row(
        &self,
        before: &State,
        after: &State,
        k: &Key,
        b: &KState,
        a: &KState,
        side: Option<&str>,
        chain: Option<(Option<u64>, Option<u64>)>,
    ) -> Vec<V> {
        let dag = &self.store().dag;
        let (node, kind, aspect, name, bv, av) = match k {
            Key::Node(n, asp) => {
                let kind = after
                    .nodes
                    .get(n)
                    .or_else(|| before.nodes.get(n))
                    .map_or_else(String::new, |x| x.kind.clone());
                let (aspect, name) = self.aspect_name(asp, &kind);
                (
                    V::Node(*n),
                    V::text(kind.clone()),
                    aspect,
                    name,
                    self.kstate_v(&kind, asp, b),
                    self.kstate_v(&kind, asp, a),
                )
            }
            Key::Schema(i) => (
                V::Absent,
                V::Absent,
                "schema".to_string(),
                i.text(),
                self.kstate_v("", &Aspect::Existence, b),
                self.kstate_v("", &Aspect::Existence, a),
            ),
        };
        let last = chain.and_then(|(tip, base)| self.last_change(tip, base, k));
        vec![
            V::text(Self::op(b, a)),
            node,
            kind,
            V::text(aspect),
            V::text(name),
            bv,
            av,
            side.map_or(V::Absent, V::text),
            last.map_or(V::Absent, V::Rev),
            last.map_or(V::Absent, |c| V::text(dag.commits[&c].actor.clone())),
        ]
    }

    /// `diff(range, scope)` ([50 §2.6]; [LQ/std §4.15]): `a..b` the changes from a to b (from their LCA, notice N05,
    /// when a is not an ancestor of b); `a...b` the keys changed since the merge base on one side (`ours` for a,
    /// `theirs` for b) or on both (`both`, with b's value); one revision, the changes of that commit. Rows by node,
    /// aspect, name; the nodes of `subtree(scope)` only, when given.
    // spec: [50 §2.6] diff
    pub fn diff_rows(&self, range: &CExpr, scope: Option<Nid>) -> Res<Vec<Vec<V>>> {
        let store = self.store();
        let dag = &store.dag;
        let alloc = &store.alloc;
        let scope_set: Option<BTreeSet<Nid>> = scope.map(|s| {
            super::func::subtree(self, s, None, true)
                .into_iter()
                .collect()
        });
        let mut rows: Vec<(Key, Vec<V>)> = match self.range(range)? {
            Range::One(x) => {
                let p = x.and_then(|c| dag.commits[&c].parents.first().copied());
                let (b, a) = (dag.state_at(p, alloc), dag.state_at(x, alloc));
                self.diff_table(&b, &a, None, Some((x, p)), scope_set.as_ref())
            }
            Range::Two(a, b) => {
                let base = if dag.ancestors(b).contains(&a.unwrap_or(0)) || a.is_none() {
                    a
                } else {
                    let l = dag.lcas(a, b).first().copied();
                    if let (CExpr::RRange(ea, _, eb), Some(c)) = (range, l) {
                        self.w.note(
                            Code::N05,
                            format!(
                                "{} is not an ancestor of {}; the diff starts at their LCA rev {c} c{}",
                                Self::rev_label(ea),
                                Self::rev_label(eb),
                                &hex(&dag.commits[&c].id)[..8]
                            ),
                            Vec::new(),
                            None,
                        );
                    }
                    l
                };
                let (sb, sa) = (dag.state_at(base, alloc), dag.state_at(b, alloc));
                self.diff_table(&sb, &sa, None, Some((b, base)), scope_set.as_ref())
            }
            Range::Three(a, b) => {
                let base = dag.lcas(a, b).first().copied();
                let sbase = dag.state_at(base, alloc);
                let (sa, sb) = (dag.state_at(a, alloc), dag.state_at(b, alloc));
                let ours = diff(&sbase, &sa);
                let theirs = diff(&sbase, &sb);
                let mut out = Vec::new();
                for (k, (bv, av)) in &theirs {
                    if let Key::Node(n, _) = k
                        && scope_set.as_ref().is_some_and(|s| !s.contains(n))
                    {
                        continue;
                    }
                    let side = if ours.contains_key(k) {
                        "both"
                    } else {
                        "theirs"
                    };
                    out.push((
                        k.clone(),
                        self.diff_row(&sbase, &sb, k, bv, av, Some(side), Some((b, base))),
                    ));
                }
                for (k, (bv, av)) in &ours {
                    if theirs.contains_key(k) {
                        continue;
                    }
                    if let Key::Node(n, _) = k
                        && scope_set.as_ref().is_some_and(|s| !s.contains(n))
                    {
                        continue;
                    }
                    out.push((
                        k.clone(),
                        self.diff_row(&sbase, &sa, k, bv, av, Some("ours"), Some((a, base))),
                    ));
                }
                out
            }
        };
        rows.sort_by(|(ka, ra), (kb, rb)| {
            let node = |k: &Key| match k {
                Key::Node(n, _) => (0u8, n.0),
                Key::Schema(_) => (1, 0),
            };
            let edge = |k: &Key| match k {
                Key::Node(_, Aspect::Edge(e)) => Some((e.dst, e.disc)),
                _ => None,
            };
            node(ka)
                .cmp(&node(kb))
                .then_with(|| super::val::cmp_total(&ra[3], &rb[3]))
                .then_with(|| super::val::cmp_total(&ra[4], &rb[4]))
                .then_with(|| edge(ka).cmp(&edge(kb)))
        });
        Ok(rows.into_iter().map(|(_, r)| r).collect())
    }

    /// `changes(since, ref)` ([LQ/std §2.15]): the change-feed entries after commit `since`, on ref `ref` when given.
    // spec: [LQ/std §2.15]
    pub fn changes_rows(&self, since: &(V, Option<CExpr>), r: Option<&CExpr>) -> Res<Vec<Vec<V>>> {
        let s = match since {
            (V::Rev(c), _) => *c,
            (V::Int(i), _) if *i >= 0 => *i as u64,
            (_, Some(e)) => self.w.commit_of(e)?.unwrap_or(0),
            _ => 0,
        };
        let ref_name = match r {
            Some(CExpr::RRef(n)) => Some(n.clone()),
            Some(CExpr::RHead) => Some(self.w.caller.branch.clone()),
            Some(_) => return Err(Refusal::lq("E110", "changes(ref:) takes a ref name")),
            None => None,
        };
        Ok(self
            .store()
            .feed
            .since(s, ref_name.as_deref())
            .map(|c| {
                vec![
                    V::Int(c.seq as i64),
                    V::text(c.ref_.clone()),
                    c.commit.map_or(V::Absent, V::Rev),
                    c.node.map_or(V::Absent, V::Node),
                    V::text(c.op.clone()),
                    V::text(c.aspect.clone()),
                    V::text(c.name.clone()),
                    V::text(c.actor.clone()),
                    if c.group.is_some()
                        && c.commit.is_some()
                        && c.aspect != "lease"
                        && c.aspect != "marker"
                    {
                        V::List(c.affected.iter().map(|n| V::Node(*n)).collect())
                    } else {
                        V::Absent
                    },
                ]
            })
            .collect())
    }

    /// `violations(ref)` ([50 §3.9] item 9): the staged violations of a staging ref's commit; E302 on any other view.
    // spec: [50 §3.9] item 9
    pub fn violation_rows(&self, r: Option<&CExpr>) -> Res<Vec<Vec<V>>> {
        let view = match r {
            Some(e) => self.w.open(Some(e))?,
            None => self.v.clone(),
        };
        if view.kind != super::view::ViewKind::Staged {
            return Err(Refusal::lq(
                "E302",
                format!(
                    "violations() reads a staging ref; the view is {}",
                    view.ref_name
                ),
            ));
        }
        let dag = &self.store().dag;
        Ok(view
            .commit
            .and_then(|c| dag.commits.get(&c))
            .map(|c| {
                c.violations
                    .iter()
                    .map(|v| {
                        vec![
                            V::text(v.key.as_ref().map_or("-".to_string(), |k| self.key_text(k))),
                            V::text(v.class),
                            V::text(v.description.clone()),
                            V::text(v.suggested.clone()),
                        ]
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `across(refs, ids, aspects)` ([50 §2.6]): each key of each node on each view, with whether the views differ.
    // spec: [50 §2.6] across
    pub fn across_rows(
        &self,
        refs: Option<&CExpr>,
        ids: &[Nid],
        aspects: Option<&[String]>,
    ) -> Res<Vec<Vec<V>>> {
        let revs: Vec<CExpr> = match refs {
            Some(CExpr::RList(v)) => v.clone(),
            Some(e) => vec![e.clone()],
            None => Vec::new(),
        };
        let mut views = Vec::new();
        for r in &revs {
            let v = self.w.open(Some(r))?;
            let name = match r {
                CExpr::RHead => self.w.caller.branch.clone(),
                other => Self::rev_label(other),
            };
            views.push((name, v));
        }
        let mut rows = Vec::new();
        let mut ids = ids.to_vec();
        ids.sort();
        ids.dedup();
        for n in ids {
            let mut keys: BTreeSet<Aspect> = BTreeSet::new();
            for (_, v) in &views {
                if let Some(x) = v.st.nodes.get(&n) {
                    keys.extend(x.aspects(x.kind == "artifact"));
                }
            }
            let mut per_key: Vec<KeyVals> = Vec::new();
            for a in keys {
                let mut vals = Vec::new();
                let mut kind = String::new();
                for (name, v) in &views {
                    let val = match v.st.nodes.get(&n) {
                        Some(x) => {
                            kind = x.kind.clone();
                            self.kstate_v(&x.kind, &a, &x.kstate(&v.st.schema, &a))
                        }
                        None => V::Absent,
                    };
                    vals.push((name.clone(), val));
                }
                let (aspect, name) = self.aspect_name(&a, &kind);
                if aspects.is_some_and(|f| !f.contains(&aspect)) {
                    continue;
                }
                per_key.push((aspect, name, vals));
            }
            for (aspect, name, vals) in per_key {
                let diverged = vals.windows(2).any(|w| !super::val::same(&w[0].1, &w[1].1));
                for (r, v) in vals {
                    rows.push(vec![
                        V::Node(n),
                        V::text(aspect.clone()),
                        V::text(name.clone()),
                        V::text(r),
                        v,
                        V::Bool(diverged),
                    ]);
                }
            }
        }
        // The order of `std.across` ([LQ/std §4.16]): node, name, ref.
        rows.sort_by(|a, b| {
            super::val::cmp_total(&a[0], &b[0])
                .then_with(|| super::val::cmp_total(&a[2], &b[2]))
                .then_with(|| super::val::cmp_total(&a[3], &b[3]))
        });
        Ok(rows)
    }

    /// `refs()` ([50 §2.6]): every live ref by name, with its tip, its distance from the default branch and its fork.
    // spec: [50 §2.6] refs
    pub fn refs_rows(&self) -> Vec<Vec<V>> {
        let store = self.store();
        let dag = &store.dag;
        let main = store
            .inited
            .as_ref()
            .map_or_else(|| "main".to_string(), |i| i.default_branch.clone());
        let main_tip = dag.live(&main).and_then(|r| r.tip);
        let mut refs: Vec<&crate::dag::Ref> = dag.live_refs().collect();
        refs.sort_by(|a, b| a.name.cmp(&b.name));
        refs.into_iter()
            .map(|r| {
                let (ahead, behind) = dag.ahead_behind(r.tip, main_tip);
                vec![
                    V::text(r.name.clone()),
                    V::text(r.kind.token()),
                    r.tip.map_or(V::Absent, V::Rev),
                    r.tip.map_or(V::Absent, |t| V::Int(t as i64)),
                    V::Int(ahead as i64),
                    V::Int(behind as i64),
                    r.fork.map_or(V::Absent, V::Rev),
                    V::Bool(r.kind == crate::dag::RefKind::Merge),
                ]
            })
            .collect()
    }
}
