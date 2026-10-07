//! Commit identity, ancestry and bases by definition ([F12 §3]–§5; [F07 §12.1]; [60 §4.2] "LCA and the virtual
//! base", "ahead/behind `main`"): full ancestor sets, maximal common ancestors ordered by (gen, commit id), the
//! recursive virtual base built by materialising each LCA's state and merging them with the typed rules, the steps of
//! Kleppmann's rule (one per side commit since the base that has step keys, a two-parent commit's second-parent keys
//! included), revisions, and ahead/behind as the size of an ancestor-set difference.

use crate::canon::{self, Header};
use crate::dag::{Commit, Dag, Ref};
use crate::err::{Refusal, Res};
use crate::merge::{self, Ctx, Fresh, MoveKey, Op, Step};
use crate::state::{Alloc, Aspect, Changeset, KVal, Key, State, touched};
use crate::value::{Nid, Uid, hex};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

impl Dag {
    /// The order key of a commit: (gen, id) ([F12 §4.1]).
    pub fn order_key(&self, c: u64) -> (u32, [u8; 32]) {
        let x = &self.commits[&c];
        (x.generation, x.id)
    }

    /// The maximal elements of a set of commits — those that are no parent of another member, which for a set closed
    /// under ancestry are those no member descends from — sorted ascending by (gen, id) ([F12 §4.2]).
    pub fn maximal(&self, set: &BTreeSet<u64>) -> Vec<u64> {
        let parents: BTreeSet<u64> = set
            .iter()
            .flat_map(|c| self.commits[c].parents.iter().copied())
            .collect();
        let mut v: Vec<u64> = set
            .iter()
            .copied()
            .filter(|c| !parents.contains(c))
            .collect();
        v.sort_by_key(|c| self.order_key(*c));
        v
    }

    /// LCA(x, y): the maximal elements of anc*(x) ∩ anc*(y), ascending by (gen, id); empty when x or y is ε
    /// ([F12 §4.2]).
    // spec: [F12 §4.2]
    pub fn lcas(&self, x: Option<u64>, y: Option<u64>) -> Vec<u64> {
        let (ax, ay) = (self.ancestors(x), self.ancestors(y));
        let common: BTreeSet<u64> = ax.intersection(&ay).copied().collect();
        self.maximal(&common)
    }

    /// ahead/behind of `a` against `b`: |anc*(a) \ anc*(b)| and |anc*(b) \ anc*(a)| ([60 §4.2]).
    // spec: [60 §4.2] ahead/behind
    pub fn ahead_behind(&self, a: Option<u64>, b: Option<u64>) -> (u64, u64) {
        let (x, y) = (self.ancestors(a), self.ancestors(b));
        (
            x.difference(&y).count() as u64,
            y.difference(&x).count() as u64,
        )
    }

    /// The commits whose id, in lower-case hex, begins with `prefix`, ascending by seq ([F12 §3.4]).
    pub fn by_prefix(&self, prefix: &str) -> Vec<u64> {
        self.commits
            .values()
            .filter(|c| hex(&c.id).starts_with(prefix))
            .map(|c| c.seq)
            .collect()
    }

    /// The replay start R of RS-007 for a merge, a `sync` or a virtual merge of sides with ancestor sets `a` and `b`
    /// over a base with ancestor set `base` ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann steps"; [AR §11]
    /// OQ-A-11 11.1 (B)): the greatest commit of A(B) ∩ A(o) ∩ A(t) that every other commit of A(o) ∪ A(t) descends
    /// from or is an ancestor of; `None` (ε) when no commit is. Such commits are pairwise comparable, so they form a
    /// chain and the greatest is unique, and no commit of A(o) ∪ A(t) that is not an ancestor-or-self of R is replayed
    /// on a state it was not made on: each descends from R. When every commit of both sides since a single-LCA base
    /// descends from it, R is the base's commit. Candidates are tried from the greatest (gen, id) down: a commit
    /// qualifies when the commits of A(o) ∪ A(t) with a gen at most its own are exactly its ancestors-or-self, and
    /// every other one descends from it.
    // spec: [RULES/merge-table] results
    // spec: [F12 §7.4]
    pub fn replay_start(
        &self,
        a: &BTreeSet<u64>,
        b: &BTreeSet<u64>,
        base: &BTreeSet<u64>,
    ) -> Option<u64> {
        let gen_of = |c: &u64| self.commits[c].generation;
        // A(o) ∪ A(t) in ascending (gen, id): every parent before its children.
        let mut all: Vec<u64> = a.union(b).copied().collect();
        all.sort_by_key(|c| self.order_key(*c));
        let mut cand: Vec<u64> = base
            .iter()
            .copied()
            .filter(|c| a.contains(c) && b.contains(c))
            .collect();
        cand.sort_by_key(|c| std::cmp::Reverse(self.order_key(*c)));
        cand.into_iter().find(|r| {
            let g = gen_of(r);
            // A proper descendant has a greater gen, so every commit with a gen at most g must be an ancestor-or-self.
            let anc = self.ancestors(Some(*r));
            if all.iter().filter(|c| gen_of(c) <= g).count() != anc.len() {
                return false;
            }
            let mut desc: BTreeSet<u64> = BTreeSet::from([*r]);
            for c in all.iter().filter(|c| gen_of(c) > g) {
                if self.commits[c].parents.iter().any(|p| desc.contains(p)) {
                    desc.insert(*c);
                }
            }
            anc.len() + desc.len() - 1 == all.len()
        })
    }

    /// The Kleppmann steps of one side since the replay start (RS-007; [F12 §7.4] row "Kleppmann steps"; [F12 §5.3]
    /// VM-7): one per commit of A(side) \ A(R) that has step keys ([`Dag::step_keys`]), `start` being A(R) (the
    /// ancestor set of [`Dag::replay_start`]'s commit), each key with its value in that commit's state, ascending by the
    /// commit's (hlc, id). A first-parent key takes the `after` value of the commit's net changeset (for a `sync`,
    /// which stores the full state diff against its first parent, [AR §4.6], that diff); a second-parent key of a
    /// two-parent commit, which that changeset does not hold, takes its canonical value in the commit's state
    /// ([RULES/merge-table] open point 35 case (i), narrow form; [AR §11] OQ-A-6 (a)).
    // spec: [RULES/merge-table] results
    // spec: [F12 §7.4]
    pub fn move_steps(
        &self,
        side: &BTreeSet<u64>,
        start: &BTreeSet<u64>,
        alloc: &dyn Alloc,
    ) -> Vec<Step> {
        let mut v: Vec<Step> = side
            .difference(start)
            .filter_map(|c| self.step(*c, alloc))
            .collect();
        v.sort_by_key(|s| s.key);
        v
    }

    /// The Kleppmann step of commit `c` (RS-007: "the step sets each of its step keys to its value in that commit's
    /// state"): its first-parent entries, the `after` values of its net changeset ([`Step::of`]), and for a two-parent
    /// commit its second-parent keys valued in its state; `None` when it has no step key.
    // spec: [RULES/merge-table] results
    // spec: [F12 §7.4]
    fn step(&self, c: u64, alloc: &dyn Alloc) -> Option<Step> {
        let x = &self.commits[&c];
        let key = (x.hlc, x.id);
        let first = Step::of(key, &x.changeset);
        if x.parents.len() < 2 {
            return first;
        }
        let keys = self.step_keys(c, alloc);
        let mut moves = first.map(|s| s.moves).unwrap_or_default();
        let held: BTreeSet<Nid> = moves.iter().map(|(n, _)| *n).collect();
        let extra: Vec<Nid> = keys.difference(&held).copied().collect();
        if !extra.is_empty() {
            let st = self.state_at(Some(c), alloc);
            moves.extend(
                extra
                    .into_iter()
                    .map(|n| (n, merge::flat(&merge::cval(&st, n, &Aspect::Hierarchy)))),
            );
            moves.sort_by_key(|(n, _)| *n);
        }
        (!moves.is_empty()).then_some(Step { key, moves })
    }

    /// The step keys of commit `c` (RS-007; [F12 §7.4] row "Kleppmann steps"): the hierarchy keys of its canonical net
    /// changeset against its first parent; for a two-parent commit M (a merge or a `sync`) with parents p₁ and p₂, also
    /// each hierarchy key whose canonical value in state(M) differs from its value in state(p₂) and that is a step key
    /// of some commit of A(p₂) \ A(p₁), so that M re-asserts what it kept against the merged branch's moves
    /// ([RULES/merge-table] open point 35 case (i) in its narrow form, "moved" read as "is a step key of", recursively:
    /// a merge inside the merged branch counts with its own second-parent keys, [AR §11] OQ-A-11 11.3), and each of
    /// its resolved keys ([`Dag::resolved_keys`], OQ-A-12 (c)). Step keys depend on the commit graph alone, so they are
    /// computed once per commit and kept on the DAG for every later merge, `sync`, virtual merge, revert and
    /// cherry-pick. A commit's keys read only its ancestors' (its own merge's candidate reads its parents' histories), so
    /// every ancestor not yet kept is computed first, ascending by (gen, id), with no recursion however deeply a history
    /// of syncs nests its merges.
    // spec: [RULES/merge-table] results
    // spec: [F12 §7.4]
    pub fn step_keys(&self, c: u64, alloc: &dyn Alloc) -> Rc<BTreeSet<Nid>> {
        if let Some(k) = self.step_memo.borrow().get(&c) {
            return k.clone();
        }
        let mut todo: Vec<u64> = {
            let memo = self.step_memo.borrow();
            self.ancestors(Some(c))
                .into_iter()
                .filter(|x| !memo.contains_key(x))
                .collect()
        };
        todo.sort_by_key(|x| self.order_key(*x));
        for x in todo {
            let keys = self.own_step_keys(x, alloc);
            self.step_memo.borrow_mut().insert(x, Rc::new(keys));
        }
        self.step_memo.borrow()[&c].clone()
    }

    /// The step keys of commit `x` ([`Dag::step_keys`]), every ancestor's being kept already.
    fn own_step_keys(&self, x: u64, alloc: &dyn Alloc) -> BTreeSet<Nid> {
        let commit = &self.commits[&x];
        let mut keys: BTreeSet<Nid> = commit
            .changeset
            .keys()
            .filter_map(|k| match k {
                Key::Node(n, Aspect::Hierarchy) => Some(*n),
                _ => None,
            })
            .collect();
        let [p1, p2] = commit.parents[..] else {
            return keys;
        };
        let moved: BTreeSet<Nid> = {
            let memo = self.step_memo.borrow();
            self.second_parent_only(p1, p2)
                .iter()
                .flat_map(|d| memo[d].iter().copied())
                .filter(|n| !keys.contains(n))
                .collect()
        };
        if !moved.is_empty() {
            let (sm, s2) = (
                self.state_at(Some(x), alloc),
                self.state_at(Some(p2), alloc),
            );
            let h = Aspect::Hierarchy;
            keys.extend(
                moved
                    .into_iter()
                    .filter(|n| merge::cval(&sm, *n, &h) != merge::cval(&s2, *n, &h)),
            );
        }
        #[cfg(test)]
        if !merge::rule().resolved_keys() {
            return keys;
        }
        keys.extend(self.resolved_keys(x, alloc));
        keys
    }

    /// The resolved keys of a two-parent commit M with parents p₁ and p₂ (RS-007; [AR §11] OQ-A-12 (c)): the hierarchy
    /// keys of the nodes live in state(M) whose (parent, order), compared by uid, differs from their value in the
    /// candidate of M's own merge, the typed merge of state(p₂) into state(p₁) as a `merge` (a `sync` for a sync
    /// commit) over the base of [F12 §4.3] for (p₁, p₂), with no `--base`, no policy override and no resolution: a
    /// landed commit records neither its `--base` nor its override ([F06 §4.4.16] records them on a staged commit
    /// only), so the keys a resolution set, a `--base` moved or an override fixed differently are all keys where M holds
    /// what its default merge would not give, and M's step re-asserts them at its (hlc, commit id). The candidate's
    /// hierarchy follows RS-007 itself, so it reads the step keys of the commits of A(p₁) ∪ A(p₂), every one an
    /// ancestor of M. A node the candidate does not hold live counts as differing.
    // spec: [RULES/merge-table] results
    // spec: [F12 §7.4]
    pub fn resolved_keys(&self, m: u64, alloc: &dyn Alloc) -> BTreeSet<Nid> {
        let c = &self.commits[&m];
        let [p1, p2] = c.parents[..] else {
            return BTreeSet::new();
        };
        // The harness counts RS-007's work here apart from the merges a command runs (test builds only).
        #[cfg(test)]
        let _deriving = merge::Deriving::enter();
        let uid = |n: Nid| alloc.uid(n);
        let nid = |u: Uid| alloc.nid(u);
        let mut bases = Bases::new(self, alloc, &uid, &nid);
        let base = bases.base(Some(p1), Some(p2), None);
        let (o, t) = (
            self.state_at(Some(p1), alloc),
            self.state_at(Some(p2), alloc),
        );
        let rp = self.replay(Some(p1), Some(p2), &base, None, alloc);
        let auto = crate::policy::merge_policies(&o.schema);
        let r = &self.refs[&c.ref_id];
        let og = |side: usize, n: Nid, v: &Option<KVal>| {
            self.origin_among(&[if side == 1 { p1 } else { p2 }], n, v, alloc)
        };
        let cx = Ctx {
            op: if c.kind == "sync" {
                Op::Sync
            } else {
                Op::Merge
            },
            dst_main: r.name == "main",
            dst_plan: r.kind == crate::dag::RefKind::Plan,
            policy: None,
            auto: &auto,
            start: rp.start(),
            moves: [&rp.moves[0][..], &rp.moves[1][..]],
            uid: &uid,
            nid: &nid,
            origin: Some(&og),
        };
        let mut fresh = bases.fresh.clone();
        let p = merge::typed(&base.st, &o, &t, &cx, &mut fresh);
        let cuid = |n: Nid| p.uids.get(&n).copied().unwrap_or_else(|| alloc.uid(n));
        let cand: BTreeMap<Uid, (Option<Uid>, Option<String>)> =
            p.m.st
                .nodes
                .iter()
                .filter(|(_, x)| x.live())
                .map(|(n, x)| (cuid(*n), (x.parent.map(cuid), x.order.clone())))
                .collect();
        let st = self.state_at(Some(m), alloc);
        st.nodes
            .iter()
            .filter(|(_, x)| x.live())
            .filter(|(n, x)| {
                cand.get(&alloc.uid(**n))
                    != Some(&(x.parent.map(|q| alloc.uid(q)), x.order.clone()))
            })
            .map(|(n, _)| *n)
            .collect()
    }

    /// RS-007's hierarchy inputs for a merge of `y` into `x` over `base`, `forced` being its `--base` ([RULES/merge-table]
    /// RS-007; [AR §11] OQ-A-11 11.1): one-sided when the base's commit is `x` (the single LCA is `x`, `--base` names it,
    /// or both are ε), and otherwise the state of the replay start R ([`Dag::replay_start`]) with each side's steps
    /// since R.
    // spec: [RULES/merge-table] results
    // spec: [F12 §7.4]
    pub fn replay(
        &self,
        x: Option<u64>,
        y: Option<u64>,
        base: &Base,
        forced: Option<u64>,
        alloc: &dyn Alloc,
    ) -> Replay {
        let one_sided = match forced {
            Some(c) => Some(c) == x,
            None => base.lcas.len() <= 1 && base.lcas.first().copied() == x,
        };
        #[cfg(test)]
        if merge::rule().replays_from_b() {
            return Replay {
                one_sided: false,
                state: base.st.clone(),
                moves: [
                    self.move_steps(&self.ancestors(x), &base.anc, alloc),
                    self.move_steps(&self.ancestors(y), &base.anc, alloc),
                ],
            };
        }
        // A rule with no replay reads neither the replay start nor any step (the evaluation harness's candidate).
        if one_sided || merge::no_replay() {
            return Replay {
                one_sided,
                state: Rc::new(State::default()),
                moves: [Vec::new(), Vec::new()],
            };
        }
        let (ax, ay) = (self.ancestors(x), self.ancestors(y));
        let r = self.replay_start(&ax, &ay, &base.anc);
        let ar = self.ancestors(r);
        Replay {
            one_sided,
            state: self.state_at(r, alloc),
            moves: [
                self.move_steps(&ax, &ar, alloc),
                self.move_steps(&ay, &ar, alloc),
            ],
        }
    }

    /// The origin time of the hierarchy value commit `c` holds for node `n`: the (hlc, commit id) of the commit that
    /// produced it. The walk starts at `c`; a commit whose net changeset against its first parent leaves the key's value
    /// as it was passes it on to its first parent; a two-parent commit that changed it against its first parent but holds
    /// its second parent's value passes it on to its second parent (a `sync` that takes `main`'s move, a merge that takes
    /// the merged branch's); any other commit that changed it produced it. ε, and a root commit that left the key absent,
    /// give (0, 0). The walk reads only each commit's own net changeset and, at a two-parent commit that changed the
    /// key, its second parent's value of it: an engine follows the key's per-node chain (the commits whose net changeset
    /// holds the key) and hops to the second parent's chain at such a commit, and since a commit's origins never change
    /// they may be kept with it (the evaluation harness's candidate `threeway`, `merge/cand.rs`).
    pub fn origin(&self, c: Option<u64>, n: Nid, alloc: &dyn Alloc) -> MoveKey {
        let k = Key::Node(n, Aspect::Hierarchy);
        let h = Aspect::Hierarchy;
        let mut cur = c;
        while let Some(x) = cur {
            let cm = &self.commits[&x];
            let changed = cm
                .changeset
                .get(&k)
                .filter(|(b, a)| merge::flat(b) != merge::flat(a));
            match changed {
                None => cur = cm.parents.first().copied(),
                Some((_, after)) => {
                    if let [_, p2] = cm.parents[..] {
                        let s2 = self.state_at(Some(p2), alloc);
                        if merge::flat(&merge::cval(&s2, n, &h)) == merge::flat(after) {
                            cur = Some(p2);
                            continue;
                        }
                    }
                    return (cm.hlc, cm.id);
                }
            }
        }
        (0, [0; 32])
    }

    /// The origin time of value `v` of node `n`'s hierarchy key on a side whose state is a fold of the commits `tips`
    /// (one commit for a real side; the LCAs folded so far for a virtual merge's dst): the latest [`Dag::origin`] among
    /// the tips that hold `v`, and (0, 0) when none does (a value only a base or a virtual merge produced).
    pub fn origin_among(
        &self,
        tips: &[u64],
        n: Nid,
        v: &Option<KVal>,
        alloc: &dyn Alloc,
    ) -> MoveKey {
        let h = Aspect::Hierarchy;
        tips.iter()
            .filter(|c| merge::flat(&merge::cval(&self.state_at(Some(**c), alloc), n, &h)) == *v)
            .map(|c| self.origin(Some(*c), n, alloc))
            .max()
            .unwrap_or((0, [0; 32]))
    }

    /// A(p₂) \ A(p₁) of a two-parent commit ([F12 §5.2]): the walk from p₂ stops at every member of A(p₁), whose
    /// ancestors are all in A(p₁) too.
    // spec: [F12 §5.2]
    fn second_parent_only(&self, p1: u64, p2: u64) -> Vec<u64> {
        let a1 = self.ancestors(Some(p1));
        let mut out = BTreeSet::new();
        let mut stack = vec![p2];
        while let Some(d) = stack.pop() {
            if !a1.contains(&d) && out.insert(d) {
                stack.extend(self.commits[&d].parents.iter().copied());
            }
        }
        out.into_iter().collect()
    }

    /// The members of `keys` that are a step key ([`Dag::step_keys`]) of a commit of `side` ordered after `after` by
    /// (hlc, commit id), commit ids compared bytewise: the keys whose later move stands against a revert's or a
    /// cherry-pick's step (RS-007; [RULES/merge-table] open point 35 case (ii); MR-040).
    // spec: [RULES/merge-table] results
    pub fn moved_after(
        &self,
        side: &BTreeSet<u64>,
        after: MoveKey,
        keys: &BTreeSet<Nid>,
        alloc: &dyn Alloc,
    ) -> BTreeSet<Nid> {
        let mut out = BTreeSet::new();
        for c in side {
            if out.len() == keys.len() {
                break;
            }
            let x = &self.commits[c];
            if (x.hlc, x.id) > after {
                let sk = self.step_keys(*c, alloc);
                out.extend(keys.intersection(&sk).copied());
            }
        }
        out
    }

    /// Items 1–9 of a stored commit ([F07 §12.1]): its kind, its parents' ids, its `hlc`, actor, role, session, git
    /// provenance, message, schema version and origin.
    pub fn header(&self, c: &Commit) -> Header {
        Header {
            kind: c.kind.to_string(),
            parents: c.parents.iter().map(|p| self.commits[p].id).collect(),
            hlc: c.hlc,
            actor: c.actor.clone(),
            role: c.role.clone(),
            session: c.session.clone(),
            git: c.git.clone(),
            message: c.message.clone(),
            schema_version: c.schema_version,
            origin: c.origin.map(|o| self.commits[&o].id),
            foreign: None,
        }
    }

    /// Sets a new commit's `changeset_digest` and `commit_id` from the state at its first parent and its own state
    /// ([F07 §10.1], §3.1): the canonical keys of the nodes its net changeset touches, or of every node when it changes
    /// a schema item. Its parents must be in the DAG.
    // spec: [F07 §10.1]
    // spec: [F07 §12.1]
    pub fn identify(&self, c: &mut Commit, parent: &State, st: &State, uid: &dyn Fn(Nid) -> Uid) {
        let (p, q) = canonical_pair(parent, st, &c.changeset, uid);
        c.digest = canon::changeset_digest(&p, &q);
        c.id = canon::commit_id(&self.header(c), &c.digest);
    }

    /// Recomputes every commit's `changeset_digest` from `state_at` of it and of its first parent and its id from its
    /// header, the definition [`Dag::identify`] must agree with (`doctor --verify`'s check, [F07 §15]); the first
    /// commit whose stored value differs is the error.
    pub fn verify_ids(&self, alloc: &dyn Alloc) -> Result<(), String> {
        let uid = |n: Nid| alloc.uid(n);
        for c in self.commits.values() {
            let parent = self.state_from_scratch(c.parents.first().copied(), alloc);
            let st = self.state_from_scratch(Some(c.seq), alloc);
            let p = canon::canonical_state(&parent, &uid);
            let q = canon::canonical_state(&st, &uid);
            let d = canon::changeset_digest(&p, &q);
            if d != c.digest {
                return Err(format!(
                    "s{}: changeset_digest differs from its states",
                    c.seq
                ));
            }
            if canon::commit_id(&self.header(c), &d) != c.id {
                return Err(format!("s{}: commit_id differs from its header", c.seq));
            }
        }
        Ok(())
    }
}

/// The canonical states the item 10 of a changeset compares: restricted to its touched nodes and the schema, or whole
/// when it changes a schema item.
fn canonical_pair(
    parent: &State,
    st: &State,
    cs: &Changeset,
    uid: &dyn Fn(Nid) -> Uid,
) -> (canon::Cs, canon::Cs) {
    if cs.keys().any(|k| matches!(k, Key::Schema(_))) {
        return (
            canon::canonical_state(parent, uid),
            canon::canonical_state(st, uid),
        );
    }
    let nodes: BTreeSet<Nid> = touched(cs);
    (
        canon::canonical_of(parent, &nodes, uid),
        canon::canonical_of(st, &nodes, uid),
    )
}

/// A state with its ancestor set ([F12 §5.2]).
type StateAnc = (Rc<State>, Rc<BTreeSet<u64>>);

/// A base of a merge ([F12 §4.3]): its state and its ancestor set A(B) (§5.2), with the LCAs it came from.
#[derive(Clone, Debug)]
pub struct Base {
    /// The state.
    pub st: Rc<State>,
    /// A(B).
    pub anc: Rc<BTreeSet<u64>>,
    /// The LCAs, ascending by (gen, id); empty for `--base` and for unrelated histories.
    pub lcas: Vec<u64>,
    /// Whether two or more LCAs were merged into a virtual base.
    pub virtual_base: bool,
}

/// RS-007's hierarchy inputs of one merge ([`Dag::replay`]).
#[derive(Clone, Debug)]
pub struct Replay {
    /// The merge is one-sided: every hierarchy key takes src's value, with no replay (OQ-A-11 11.1 (A)).
    pub one_sided: bool,
    /// The state of the replay start R (OQ-A-11 11.1 (B)); empty when one-sided.
    pub state: Rc<State>,
    /// Each side's steps since R, dst's then src's; empty when one-sided.
    pub moves: [Vec<Step>; 2],
}

impl Replay {
    /// Where the typed merge starts the hierarchy keys ([`merge::Start`]).
    pub fn start(&self) -> merge::Start<'_> {
        if self.one_sided {
            merge::Start::TakeSrc
        } else {
            merge::Start::State(&self.state)
        }
    }
}

/// The base-selection rule of I31′ with the recursive virtual base ([F12 §4.3], §5): memoised by the sorted LCA list
/// within one merge (§5.5).
pub struct Bases<'a> {
    dag: &'a Dag,
    alloc: &'a dyn Alloc,
    uid: &'a dyn Fn(Nid) -> Uid,
    nid: &'a dyn Fn(Uid) -> Option<Nid>,
    memo: BTreeMap<Vec<u64>, StateAnc>,
    /// The `#N`s the virtual merges gave re-keyed uids.
    pub fresh: Fresh,
}

impl<'a> Bases<'a> {
    /// A base selector over one store.
    pub fn new(
        dag: &'a Dag,
        alloc: &'a dyn Alloc,
        uid: &'a dyn Fn(Nid) -> Uid,
        nid: &'a dyn Fn(Uid) -> Option<Nid>,
    ) -> Bases<'a> {
        Bases {
            dag,
            alloc,
            uid,
            nid,
            memo: BTreeMap::new(),
            fresh: Fresh::default(),
        }
    }

    fn state(&self, c: u64) -> Rc<State> {
        self.dag.state_at(Some(c), self.alloc)
    }

    /// The base of a merge of `y` into `x` ([F12 §4.3]): `forced` (`--base`), else by the LCAs: none → the empty state,
    /// one → its state, several → the recursive virtual base.
    // spec: [F12 §4.3]
    // rule: VB-001, VB-002, VB-003, VB-004
    pub fn base(&mut self, x: Option<u64>, y: Option<u64>, forced: Option<u64>) -> Base {
        if let Some(c) = forced {
            return Base {
                st: self.state(c),
                anc: Rc::new(self.dag.ancestors(Some(c))),
                lcas: Vec::new(),
                virtual_base: false,
            };
        }
        let l = self.dag.lcas(x, y);
        let (st, anc) = self.of(&l);
        Base {
            st,
            anc,
            virtual_base: l.len() >= 2,
            lcas: l,
        }
    }

    /// The state and ancestor set a sorted LCA list stands for.
    fn of(&mut self, l: &[u64]) -> StateAnc {
        match l {
            [] => (Rc::new(State::default()), Rc::new(BTreeSet::new())),
            [m] => (self.state(*m), Rc::new(self.dag.ancestors(Some(*m)))),
            _ => self.vbase(l),
        }
    }

    /// VBase(L) of [F12 §5.1]: V₁ = state(L₁); for i = 2 … k, Vᵢ = VM(Vᵢ₋₁, state(Lᵢ), Bᵢ) with Bᵢ the base of the pair
    /// over the ancestor sets of §5.2, A(Vᵢ) = A(Vᵢ₋₁) ∪ anc*(Lᵢ).
    // spec: [F12 §5.1]
    // spec: [F12 §5.2]
    // rule: VB-005, VB-006, VB-007, VB-008, VB-013, VB-014, VB-015, VB-016, VB-017, VB-018, DM-009
    fn vbase(&mut self, l: &[u64]) -> StateAnc {
        if let Some(v) = self.memo.get(l) {
            return v.clone();
        }
        let mut v = self.state(l[0]);
        let mut a: BTreeSet<u64> = self.dag.ancestors(Some(l[0]));
        let empty_auto = BTreeMap::new();
        for (i, li) in l[1..].iter().enumerate() {
            let al = self.dag.ancestors(Some(*li));
            let common: BTreeSet<u64> = a.intersection(&al).copied().collect();
            let m = self.dag.maximal(&common);
            let (b, ab) = self.of(&m);
            let src = self.state(*li);
            // VM-7: RS-007's replay from the replay start of the pair (OQ-A-11 11.1 (B)); a virtual merge's dst is no
            // ref tip, so it is never one-sided. A rule with no replay reads none of it.
            let (start, mo, mt) = if merge::no_replay() {
                (Rc::new(State::default()), Vec::new(), Vec::new())
            } else {
                let r = self.dag.replay_start(&a, &al, &ab);
                let ar = self.dag.ancestors(r);
                let start = self.dag.state_at(r, self.alloc);
                // The replay from B starts at the inner base itself (test builds only).
                #[cfg(test)]
                let (ar, start) = if merge::rule().replays_from_b() {
                    ((*ab).clone(), b.clone())
                } else {
                    (ar, start)
                };
                (
                    start,
                    self.dag.move_steps(&a, &ar, self.alloc),
                    self.dag.move_steps(&al, &ar, self.alloc),
                )
            };
            // The origin time of each side's value: dst is the fold of the LCAs before this one, src this LCA.
            let (dag, alloc, li_c) = (self.dag, self.alloc, *li);
            let folded: Vec<u64> = l[..=i].to_vec();
            let og = move |side: usize, n: Nid, v: &Option<KVal>| {
                if side == 1 {
                    dag.origin_among(&folded, n, v, alloc)
                } else {
                    dag.origin_among(&[li_c], n, v, alloc)
                }
            };
            let cx = Ctx {
                op: Op::Virtual,
                dst_main: false,
                dst_plan: false,
                policy: None,
                auto: &empty_auto,
                start: merge::Start::State(&start),
                moves: [&mo[..], &mt[..]],
                uid: self.uid,
                nid: self.nid,
                origin: Some(&og),
            };
            let merged = merge::merge(&b, &v, &src, &cx, &mut self.fresh);
            v = Rc::new(merged.st);
            a.extend(al);
        }
        let out = (v, Rc::new(a));
        self.memo.insert(l.to_vec(), out.clone());
        out
    }
}

/// What a revision resolves to ([F12 §3.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// A ref (live, or a deleted row named through a reflog suffix), by id.
    Ref(u32),
    /// A commit, or ε (`None`).
    Commit(Option<u64>),
}

/// What resolving a revision reads besides the DAG ([F12 §3.3]): the caller's `HEAD` and the clock and reflog window of
/// `@T`.
pub struct RevCtx {
    /// `HEAD`: the caller's branch, or a detached commit.
    pub head: Result<String, u64>,
    /// Now, in ms.
    pub now_ms: i64,
    /// `gc.reflog-expire` in ms.
    pub reflog_expire_ms: u64,
}

fn e301(text: &str, why: impl Into<String>) -> Refusal {
    Refusal::lq("E301", format!("{text}: {}", why.into()))
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DDTHH:MM:SSZ` in ms since the epoch.
fn time_ms(t: &str) -> Option<i64> {
    let n = |a: usize, b: usize| t.get(a..b)?.parse::<i64>().ok();
    let days = days_from_civil(n(0, 4)?, n(5, 7)?, n(8, 10)?);
    Some(((days * 24 + n(11, 13)?) * 60 + n(14, 16)?) * 60_000 + n(17, 19)? * 1000)
}

impl Dag {
    /// The held moves of a ref, newest first: its reflog ([F12 §3.5] "Moves").
    fn held<'b>(&self, r: &'b Ref) -> impl Iterator<Item = &'b crate::dag::RefMove> {
        r.moves.iter().rev()
    }

    /// Resolves a revision's text ([F12 §3]): its base, then its suffixes left to right. A malformed text is `usage`
    /// (exit 2); a revision that names nothing is E301 (exit 3).
    // spec: [F12 §3.4]
    // spec: [F12 §3.5]
    pub fn resolve_rev(&self, text: &str, cx: &RevCtx) -> Res<Resolved> {
        use crate::lq::ast::{Rev, RevKind, Suffix};
        let lx = crate::lq::lexer::Lexer::new(text);
        let mut toks = Vec::new();
        let (rev, end) = lx
            .revspec(0, &mut toks)
            .map_err(|_| Refusal::usage(format!("{text} is not a revision")))?;
        if end != text.len() {
            return Err(Refusal::usage(format!("{text} is not a revision")));
        }
        // Flatten the suffixes: base first.
        let mut sufs: Vec<Suffix> = Vec::new();
        let mut cur: &Rev = &rev;
        while let RevKind::Suf(b, s) = &cur.kind {
            sufs.push(s.clone());
            cur = b;
        }
        sufs.reverse();
        let tip = |r: &Ref| Resolved::Commit(r.tip);
        let mut value = match &cur.kind {
            RevKind::Head => match &cx.head {
                Ok(b) => Resolved::Ref(
                    self.live(b)
                        .ok_or_else(|| e301(text, format!("no live ref {b}")))?
                        .id,
                ),
                Err(c) => Resolved::Commit(Some(*c)),
            },
            RevKind::Ref(name) => match self.live(name) {
                Some(r) => Resolved::Ref(r.id),
                None if matches!(sufs.first(), Some(Suffix::At(_) | Suffix::AtTime(_))) => {
                    Resolved::Ref(
                        self.refs
                            .values()
                            .rev()
                            .find(|r| r.deleted && r.name == *name)
                            .ok_or_else(|| e301(text, format!("no ref {name}")))?
                            .id,
                    )
                }
                None => return Err(e301(text, format!("no ref {name}"))),
            },
            RevKind::Commit(h) => {
                let v = self.by_prefix(h);
                match v.as_slice() {
                    [c] => Resolved::Commit(Some(*c)),
                    [] => return Err(e301(text, "no commit has this id")),
                    many => {
                        return Err(e301(
                            text,
                            format!(
                                "several commits match: {}",
                                many.iter()
                                    .map(|c| format!("s{c}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ));
                    }
                }
            }
            RevKind::Seq(0) => Resolved::Commit(None),
            RevKind::Seq(n) => {
                if !self.commits.contains_key(n) {
                    return Err(e301(text, format!("no commit s{n}")));
                }
                Resolved::Commit(Some(*n))
            }
            RevKind::Param(_) => return Err(Refusal::usage(format!("{text} is not a revision"))),
            RevKind::Suf(..) => unreachable_suffix(),
        };
        for (i, s) in sufs.iter().enumerate() {
            value = match (s, &value) {
                (Suffix::At(n), Resolved::Ref(id)) if i == 0 => {
                    let r = &self.refs[id];
                    let c = if *n == 0 {
                        r.tip
                    } else {
                        self.held(r)
                            .nth(*n as usize - 1)
                            .ok_or_else(|| {
                                e301(
                                    text,
                                    format!("{} has {} recorded moves", r.name, r.moves.len()),
                                )
                            })?
                            .old
                    };
                    Resolved::Commit(Some(c.ok_or_else(|| e301(text, "the ref had no commit"))?))
                }
                (Suffix::AtTime(t), Resolved::Ref(id)) if i == 0 => {
                    let r = &self.refs[id];
                    let t =
                        time_ms(t).ok_or_else(|| Refusal::usage(format!("{text}: a bad time")))?;
                    let newest = self.held(r).find(|m| ((m.hlc >> 16) as i64) <= t);
                    let c = match newest {
                        Some(m) => m.new,
                        None if t >= cx.now_ms - cx.reflog_expire_ms as i64 => {
                            r.moves.first().map_or(r.tip, |m| m.old)
                        }
                        None => return Err(e301(text, "no move of the reflog is that old")),
                    };
                    Resolved::Commit(Some(c.ok_or_else(|| e301(text, "the ref had no commit"))?))
                }
                (Suffix::At(_) | Suffix::AtTime(_), _) => {
                    return Err(e301(
                        text,
                        "a reflog suffix follows only a ref name or HEAD",
                    ));
                }
                (Suffix::Tilde(n), v) => {
                    let mut c = match v {
                        Resolved::Ref(id) => self.refs[id].tip,
                        Resolved::Commit(c) => *c,
                    };
                    for _ in 0..*n {
                        let x = c.ok_or_else(|| e301(text, "the revision has no parent"))?;
                        c = Some(
                            self.commits[&x]
                                .parents
                                .first()
                                .copied()
                                .ok_or_else(|| e301(text, "a root commit has no parent"))?,
                        );
                    }
                    Resolved::Commit(c)
                }
                (Suffix::Caret(n), v) => {
                    let c = match v {
                        Resolved::Ref(id) => self.refs[id].tip,
                        Resolved::Commit(c) => *c,
                    };
                    if *n == 0 {
                        Resolved::Commit(c)
                    } else {
                        let x = c.ok_or_else(|| e301(text, "the revision has no parent"))?;
                        Resolved::Commit(Some(
                            self.commits[&x]
                                .parents
                                .get(*n as usize - 1)
                                .copied()
                                .ok_or_else(|| {
                                    e301(text, format!("the commit has no parent {n}"))
                                })?,
                        ))
                    }
                }
            };
        }
        Ok(match value {
            Resolved::Ref(id) if !sufs.is_empty() => tip(&self.refs[&id]),
            v => v,
        })
    }

    /// A revision as the commit it stands for: a ref's tip, a commit, or ε.
    pub fn rev_commit(&self, text: &str, cx: &RevCtx) -> Res<Option<u64>> {
        Ok(match self.resolve_rev(text, cx)? {
            Resolved::Ref(id) => self.refs[&id].tip,
            Resolved::Commit(c) => c,
        })
    }
}

fn unreachable_suffix() -> ! {
    panic!("a flattened revision has no suffix at its base")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{MoveReason, RefKind, RefMove};

    fn commit(dag: &mut Dag, seq: u64, parents: Vec<u64>) {
        let mut c = Commit::new(
            seq,
            0,
            seq,
            "ordinary",
            parents,
            seq << 16,
            Changeset::new(),
        );
        c.id = canon::b3_256(&seq.to_le_bytes());
        dag.insert(c);
    }

    /// A criss-cross: R; L1, L2 off R; A = merge(L1, L2), B = merge(L2, L1): LCA(A, B) = {L1, L2} by (gen, id).
    #[test]
    fn a_criss_cross_has_two_lcas_in_gen_then_id_order() {
        let mut d = Dag::default();
        commit(&mut d, 1, vec![]);
        commit(&mut d, 2, vec![1]);
        commit(&mut d, 3, vec![1]);
        commit(&mut d, 4, vec![2, 3]);
        commit(&mut d, 5, vec![3, 2]);
        let l = d.lcas(Some(4), Some(5));
        let mut want = vec![2, 3];
        want.sort_by_key(|c| d.order_key(*c));
        assert_eq!(l, want);
        assert_eq!(d.commits[&4].generation, 3);
        assert_eq!(
            d.lcas(Some(2), Some(4)),
            vec![2],
            "an ancestor is its own LCA"
        );
        assert!(d.lcas(None, Some(4)).is_empty());
        assert_eq!(d.ahead_behind(Some(4), Some(5)), (1, 1));
        assert_eq!(d.ahead_behind(Some(4), Some(1)), (3, 0));
    }

    /// RS-007's replay start R ([RULES/merge-table] RS-007; [AR §11] OQ-A-11 11.1 (B)): the greatest commit of
    /// A(B) ∩ A(o) ∩ A(t) that every other commit of A(o) ∪ A(t) descends from or precedes, or ε.
    #[test]
    fn the_replay_start_is_the_greatest_commit_every_other_one_is_comparable_with() {
        let mut d = Dag::default();
        let anc = |d: &Dag, c: u64| d.ancestors(Some(c));
        let start = |d: &Dag, o: u64, t: u64| {
            let l = d.lcas(Some(o), Some(t));
            let base: BTreeSet<u64> = l.iter().flat_map(|c| d.ancestors(Some(*c))).collect();
            d.replay_start(&anc(d, o), &anc(d, t), &base)
        };
        // F (1); the lane's Y (2) and main's X (3) off F; the sync N = merge(Y, X) (4); the lane's Z (5); main's W (6).
        for (seq, parents) in [
            (1, vec![]),
            (2, vec![1]),
            (3, vec![1]),
            (4, vec![2, 3]),
            (5, vec![4]),
            (6, vec![3]),
        ] {
            commit(&mut d, seq, parents);
        }
        assert_eq!(start(&d, 2, 3), Some(1), "a plain fork starts at the base");
        assert_eq!(
            start(&d, 6, 3),
            Some(3),
            "every commit since B descends from it: R = B"
        );
        assert_eq!(d.lcas(Some(5), Some(3)), vec![3]);
        assert_eq!(
            start(&d, 5, 3),
            Some(1),
            "Y does not descend from B = X, so R is the fork"
        );
        assert_eq!(d.lcas(Some(5), Some(6)), vec![3]);
        assert_eq!(start(&d, 5, 6), Some(1), "the second sync: the same");
        // A criss-cross over the fork: L1 (7) and L2 (8) off W, A = merge(L1, L2) (9), B = merge(L2, L1) (10).
        for (seq, parents) in [
            (7, vec![6]),
            (8, vec![6]),
            (9, vec![7, 8]),
            (10, vec![8, 7]),
        ] {
            commit(&mut d, seq, parents);
        }
        assert_eq!(d.lcas(Some(9), Some(10)).len(), 2);
        assert_eq!(start(&d, 9, 10), Some(6), "below both LCAs");
        // `--base` names a commit below the replay start that the LCA alone gives: R stays inside A(B).
        assert_eq!(
            d.replay_start(&anc(&d, 9), &anc(&d, 10), &anc(&d, 3)),
            Some(3)
        );
        // An unrelated root: no commit is comparable with both histories.
        commit(&mut d, 11, vec![]);
        assert_eq!(start(&d, 6, 11), None);
    }

    /// [`Dag::replay_start`] equals its definition taken literally on random DAGs (criss-crosses and several roots
    /// among them), over a base of the LCAs, of `--base` naming any commit, or ε (`wave-3c-arbiter.md` W3C-ARB-9):
    /// among the commits of A(B) ∩ A(o) ∩ A(t) that every other commit of A(o) ∪ A(t) descends from or is an ancestor
    /// of, which are pairwise comparable, the one every other is an ancestor of; ε when there is none.
    #[test]
    fn the_replay_start_meets_its_definition_on_random_dags() {
        use proptest::prelude::*;
        // Commit i + 1 takes up to two distinct parents among the commits before it (none: a root).
        let dag = proptest::collection::vec((0usize..64, 0usize..64, 0u8..4), 1..24);
        // The sides, and the base: the LCAs, `--base` naming any commit, or ε.
        let pick = (0usize..64, 0usize..64, 0usize..64, 0u8..3);
        proptest!(ProptestConfig::with_cases(256), |(shape in dag, (x, y, z, kind) in pick)| {
            let mut d = Dag::default();
            for (i, (p, q, k)) in shape.iter().enumerate() {
                let seq = i as u64 + 1;
                let mut parents: Vec<u64> = Vec::new();
                if i > 0 && *k > 0 {
                    parents.push((p % i) as u64 + 1);
                    let q = (q % i) as u64 + 1;
                    if *k > 2 && !parents.contains(&q) {
                        parents.push(q);
                    }
                }
                commit(&mut d, seq, parents);
            }
            let n = shape.len() as u64;
            let (o, t) = (x as u64 % n + 1, y as u64 % n + 1);
            let (ao, at) = (d.ancestors(Some(o)), d.ancestors(Some(t)));
            let base: BTreeSet<u64> = match kind {
                0 => d
                    .lcas(Some(o), Some(t))
                    .iter()
                    .flat_map(|c| d.ancestors(Some(*c)))
                    .collect(),
                1 => d.ancestors(Some(z as u64 % n + 1)),
                _ => BTreeSet::new(),
            };
            let all: BTreeSet<u64> = ao.union(&at).copied().collect();
            let comparable = |r: u64, c: u64| {
                d.ancestors(Some(r)).contains(&c) || d.ancestors(Some(c)).contains(&r)
            };
            let ok: Vec<u64> = base
                .iter()
                .copied()
                .filter(|r| ao.contains(r) && at.contains(r))
                .filter(|r| all.iter().all(|c| comparable(*r, *c)))
                .collect();
            for r in &ok {
                for s in &ok {
                    prop_assert!(comparable(*r, *s), "qualifying commits form a chain");
                }
            }
            let want = ok
                .iter()
                .copied()
                .find(|r| ok.iter().all(|s| d.ancestors(Some(*r)).contains(s)));
            prop_assert_eq!(want.is_some(), !ok.is_empty());
            prop_assert_eq!(d.replay_start(&ao, &at, &base), want);
        });
    }

    /// VBC-8 ([F12 §5.7]): two unrelated roots have no LCA, and their base is the empty state with no ancestors.
    #[test]
    fn vbc_8_unrelated_roots_have_the_empty_base() {
        struct NoAlloc;
        impl Alloc for NoAlloc {
            fn uid(&self, n: Nid) -> Uid {
                let mut b = [0u8; 16];
                b[12..].copy_from_slice(&n.0.to_be_bytes());
                Uid(b)
            }
            fn creator(&self, _: Nid) -> crate::state::Creator {
                crate::state::Creator {
                    actor: "a".into(),
                    role: "orchestrator".into(),
                }
            }
        }
        let mut d = Dag::default();
        commit(&mut d, 1, vec![]);
        commit(&mut d, 2, vec![]);
        commit(&mut d, 3, vec![1]);
        assert!(d.lcas(Some(3), Some(2)).is_empty());
        assert_eq!(d.ahead_behind(Some(3), Some(2)), (2, 1));
        let uid = |n: Nid| NoAlloc.uid(n);
        let nid = |_: Uid| None;
        let mut bases = Bases::new(&d, &NoAlloc, &uid, &nid);
        let b = bases.base(Some(3), Some(2), None);
        assert!(b.lcas.is_empty() && !b.virtual_base);
        assert_eq!(*b.st, State::default());
        assert!(b.anc.is_empty());
    }

    #[test]
    fn revisions_resolve_with_their_suffixes() {
        let mut d = Dag::default();
        commit(&mut d, 1, vec![]);
        commit(&mut d, 2, vec![1]);
        commit(&mut d, 3, vec![2, 1]);
        d.refs.insert(
            0,
            Ref {
                id: 0,
                name: "main".into(),
                kind: RefKind::Work,
                tip: Some(3),
                ref_seq_next: 4,
                fork: None,
                deleted: false,
                message: None,
                pinned: false,
                moves: (1..=3)
                    .map(|s| RefMove {
                        old: (s > 1).then_some(s - 1),
                        new: Some(s),
                        reason: MoveReason::Commit,
                        actor: String::new(),
                        hlc: s << 16,
                    })
                    .collect(),
            },
        );
        let cx = RevCtx {
            head: Ok("main".into()),
            now_ms: 10,
            reflog_expire_ms: 1000,
        };
        let r = |t: &str| d.rev_commit(t, &cx);
        assert_eq!(r("main").unwrap(), Some(3));
        assert_eq!(r("HEAD~1").unwrap(), Some(2));
        assert_eq!(r("main^2").unwrap(), Some(1));
        assert_eq!(r("main@1").unwrap(), Some(2));
        assert_eq!(r("main@{2}").unwrap(), Some(1));
        assert_eq!(r("s2").unwrap(), Some(2));
        assert_eq!(r("s0").unwrap(), None);
        let id = hex(&d.commits[&2].id);
        assert_eq!(r(&format!("c{}", &id[..12])).unwrap(), Some(2));
        assert_eq!(r("main~3").unwrap_err().code, "E301");
        assert_eq!(r("main@3").unwrap_err().code, "E301");
        assert_eq!(r("s2@1").unwrap_err().code, "E301");
        assert_eq!(r("lane/x").unwrap_err().code, "E301");
        assert_eq!(r("main junk").unwrap_err().code, "usage");
    }

    #[test]
    fn civil_days_count_from_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(time_ms("1970-01-02T00:00:01Z"), Some(86_401_000));
    }
}
