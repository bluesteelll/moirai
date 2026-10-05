//! Commit identity, ancestry and bases by definition ([F12 §3]–§5; [F07 §12.1]; [60 §4.2] "LCA and the virtual
//! base", "ahead/behind `main`"): full ancestor sets, maximal common ancestors ordered by (gen, commit id), the
//! recursive virtual base built by materialising each LCA's state and merging them with the typed rules, the steps of
//! Kleppmann's rule (one per side commit since the base), revisions, and ahead/behind as the size of an ancestor-set
//! difference.

use crate::canon::{self, Header};
use crate::dag::{Commit, Dag, Ref};
use crate::err::{Refusal, Res};
use crate::merge::{self, Ctx, Fresh, Op, Step};
use crate::state::{Alloc, Changeset, Key, State, touched};
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

    /// The Kleppmann steps of one side since the base (RS-007; [F12 §7.4] row "Kleppmann steps"; [F12 §5.3] VM-7): one
    /// per commit of A(side) \ A(B) whose canonical net changeset against its first parent has a hierarchy entry — for a
    /// `sync`, which stores the full state diff against its first parent ([AR §4.6]), that diff — each entry with its
    /// value in that commit's state, ascending by the commit's (hlc, id). A two-parent commit is a step for the entries of
    /// that first-parent changeset only (open point 35 of [RULES/merge-table] is recorded, not adopted, spec sync 2b
    /// S2B-M-2).
    pub fn move_steps(&self, side: &BTreeSet<u64>, base: &BTreeSet<u64>) -> Vec<Step> {
        let mut v: Vec<Step> = side
            .difference(base)
            .filter_map(|c| {
                let x = &self.commits[c];
                Step::of((x.hlc, x.id), &x.changeset)
            })
            .collect();
        v.sort_by_key(|s| s.key);
        v
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
        for li in &l[1..] {
            let al = self.dag.ancestors(Some(*li));
            let common: BTreeSet<u64> = a.intersection(&al).copied().collect();
            let m = self.dag.maximal(&common);
            let (b, ab) = self.of(&m);
            let src = self.state(*li);
            let (mo, mt) = (self.dag.move_steps(&a, &ab), self.dag.move_steps(&al, &ab));
            let cx = Ctx {
                op: Op::Virtual,
                dst_main: false,
                dst_plan: false,
                policy: None,
                auto: &empty_auto,
                moves: [&mo[..], &mt[..]],
                uid: self.uid,
                nid: self.nid,
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
