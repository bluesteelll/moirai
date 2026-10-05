//! The version-control verbs that write through the merge engine or move refs back ([API §11.7]–§11.11; [F12 §8],
//! §9; [AR §5a.5], §5a.7): `Merge` (with the sync-first step 0), `Sync`, `MergeContinue`, `MergeAbort`, `Revert`,
//! `CherryPick`, `Undo` and `OpRestore`. Every three-way operation is one of [RULES/merge-table] `derived-merges`
//! over materialised states ([`crate::merge`]); its result lands on dst or stages on `merge/<dst>/from/<src>` by
//! [`merge::land_or_stage`]; the commit's net changeset is the state diff against dst's tip, its id the canonical
//! encoder's, its markers the cache's rules (RE-003, RE-004), and dst's absorbed vector VR-003's. `Undo` and `OpRestore`
//! move refs through [`crate::refmove`] with their markers recomputed in both directions (ME-006, ME-013).

use crate::api::{Caller, Ctx, Data, MarkerOut, Outcome, Reply, Store};
use crate::dag::{Commit, Dag, MoveReason, Ref, RefKind, RefMove, Stage};
use crate::derived;
use crate::err::{Kv, Refusal, Res};
use crate::idem::{Cj, Recorded, ResultItem};
use crate::markers::Group;
use crate::merge::{self, Ctx as MCtx, Fresh, Merged, Op, Step, Violation};
use crate::state::{Alloc, Aspect, KState, KVal, Key, Node, State, Tomb, diff};
use crate::value::{Nid, Uid, Value, hex};
use crate::vcs::{Bases, RevCtx};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// A violation as a merge-family result lists it ([API §11.7] `violations`; [F19 §12.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViolationOut {
    /// The key text of [F12 §6.6], `-` for none.
    pub key: String,
    /// The class name.
    pub class: String,
    /// The class code.
    pub code: u8,
    /// `description`.
    pub description: String,
    /// `suggested`.
    pub suggested: String,
}

/// The step-0 sync of a merge into `main` ([API §11.7] `sync`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncOut {
    /// The sync commit.
    pub commit: Option<u64>,
    /// `landed` or `staged`.
    pub outcome: &'static str,
    /// (key text, class) of the conflicts it landed.
    pub conflicts: Vec<(String, String)>,
    /// Its violations.
    pub violations: Vec<ViolationOut>,
}

/// The `data` of `Merge`, `MergeContinue` and `Sync` ([API §11.7]) and of `Revert` and `CherryPick` (§11.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeData {
    /// `src`: the source ref; for a revert or cherry-pick the origin commit as `c<64 hex>`.
    pub src: String,
    /// `into` (`onto`).
    pub into: String,
    /// `landed`, `staged` or `up-to-date`.
    pub outcome: &'static str,
    /// Step 0's sync of a merge into `main`.
    pub sync: Option<SyncOut>,
    /// The LCA commits by (gen, id); empty for `--base`, a revert and a cherry-pick.
    pub lca: Vec<u64>,
    /// Whether two or more LCAs were merged into a virtual base.
    pub virtual_base: bool,
    /// (key text, class) of the conflicts landed.
    pub conflicts: Vec<(String, String)>,
    /// The violations.
    pub violations: Vec<ViolationOut>,
    /// The staging ref written, or `None`.
    pub staging_ref: Option<String>,
    /// `into`'s (or the staging ref's) absorbed vector after the command: ref name → `ref_seq`.
    pub absorbed: BTreeMap<String, u64>,
    /// [API §10.8].
    pub markers: Vec<MarkerOut>,
    /// The commit's `affected` ids.
    pub affected: Vec<Nid>,
    /// A revert's or cherry-pick's origin.
    pub origin: Option<u64>,
    /// `merge --continue`'s notices for stale resolutions ([F12 §9.4] step 2).
    pub notices: Vec<String>,
    /// The hints of V13, `(class, text)`.
    pub hints: Vec<(String, String)>,
}

/// The `data` of `Undo` ([API §11.11]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoData {
    /// The ref.
    pub ref_: String,
    /// The tip before.
    pub old: Option<u64>,
    /// The tip after.
    pub new: Option<u64>,
    /// `moved_back`.
    pub moved_back: u32,
    /// [API §10.8].
    pub markers: Vec<MarkerOut>,
    /// One line per listed marker entry.
    pub triage: Vec<String>,
}

/// The `data` of `OpRestore` ([API §11.11]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreData {
    /// `seq`.
    pub seq: u64,
    /// Every moved ref: (name, old tip, new tip).
    pub moved: Vec<(String, Option<u64>, Option<u64>)>,
    /// [API §10.8].
    pub markers: Vec<MarkerOut>,
    /// One line per listed marker entry.
    pub triage: Vec<String>,
}

/// A three-way result ready to land ([RULES/merge-table] PR-009 to PR-012).
struct Plan {
    /// `merge`, `sync`, `revert` or `cherry-pick`.
    kind: &'static str,
    /// The ref it lands on.
    dst: String,
    /// The src part of its staging name: a ref name, or `c` + the origin's 64 hex digits.
    src: String,
    /// The actual parents.
    parents: Vec<u64>,
    origin: Option<u64>,
    sync_base: Option<u64>,
    m: Merged,
    fresh: Fresh,
    lcas: Vec<u64>,
    virtual_base: bool,
    /// The `stage` group a staged commit of this plan records ([F06 §4.4.16]): a merge's or sync's arguments; `None`
    /// for a revert or cherry-pick, and for the absent group.
    stage: Option<Stage>,
}

/// What a landing or staging wrote.
struct Landed {
    outcome: &'static str,
    commit: u64,
    staging: Option<String>,
    markers: Vec<MarkerOut>,
    affected: Vec<Nid>,
}

/// The staged resolutions `merge --continue` overlays on its recomputed candidate ([F12 §9.4] step 2): each key whose
/// resolution applies, with its value at tip(G).
type Overlay = BTreeMap<Key, KState>;

/// The staging ref of a pair ([F12 §9.1]).
pub fn staging_name(dst: &str, src: &str) -> String {
    format!("merge/{dst}/from/{src}")
}

/// Rewrites every `#N` a state holds through `f`: node keys, edge destinations, `ref` values in fields and sets,
/// parents, `replaced_by`, and the same inside conflict values and node images.
fn map_nids(st: &mut State, f: &dyn Fn(Nid) -> Nid) {
    fn value(v: &mut Value, f: &dyn Fn(Nid) -> Nid) {
        match v {
            Value::Ref(n) => *n = f(*n),
            Value::Set(items) => {
                for x in items.iter_mut() {
                    value(x, f);
                }
                items.sort();
            }
            _ => {}
        }
    }
    fn kval(v: &mut KVal, f: &dyn Fn(Nid) -> Nid) {
        match v {
            KVal::Deleted { replaced_by, .. } => *replaced_by = replaced_by.map(f),
            KVal::Hierarchy { parent, .. } => *parent = parent.map(f),
            KVal::Value(x) => value(x, f),
            KVal::Observation(vs) => {
                for x in vs.iter_mut().flatten() {
                    value(x, f);
                }
            }
            _ => {}
        }
    }
    let nodes = std::mem::take(&mut st.nodes);
    for (n, mut x) in nodes {
        for v in x.fields.values_mut() {
            value(v, f);
        }
        x.parent = x.parent.map(f);
        if let Some(t) = &mut x.tomb {
            t.replaced_by = t.replaced_by.map(f);
        }
        let out = std::mem::take(&mut x.out);
        for (mut k, p) in out {
            k.dst = f(k.dst);
            x.out.insert(k, p);
        }
        let conflicts = std::mem::take(&mut x.conflicts);
        for (a, mut c) in conflicts {
            for v in [&mut c.base, &mut c.ours, &mut c.theirs]
                .into_iter()
                .flatten()
            {
                kval(v, f);
            }
            for img in c.images.iter_mut().flatten() {
                for v in img.values_mut() {
                    kval(v, f);
                }
            }
            let a = match a {
                Aspect::Edge(mut k) => {
                    k.dst = f(k.dst);
                    Aspect::Edge(k)
                }
                a => a,
            };
            x.conflicts.insert(a, c);
        }
        st.nodes.insert(f(n), x);
    }
}

/// The base and src states of a revert or cherry-pick of `c` (DM-003 to DM-005): the states of the commit's first parent
/// and of the commit, swapped for a revert. A revert never makes a node `absent` (DM-017): on its src, a node c brought
/// into being reads as the tombstone the inverse `Delete` writes (empty reason, no replacement; the keys [F07 §6.4]
/// keeps, an artifact's title its last `path` text), or as c left it when c landed a tombstone from the absent state;
/// its out-edges go, as c's `AddEdge` ops invert to removals. The cached src state is copied only for such nodes.
// rule: DM-003, DM-004, DM-005, DM-017
pub(crate) fn pick_states(
    dag: &Dag,
    alloc: &dyn Alloc,
    c: u64,
    revert: bool,
) -> (Rc<State>, Rc<State>) {
    let p1 = dag.commits[&c].parents.first().copied();
    let (base_at, src_at) = if revert { (Some(c), p1) } else { (p1, Some(c)) };
    let b = dag.state_at(base_at, alloc);
    let mut t = dag.state_at(src_at, alloc);
    if revert {
        let born: Vec<Nid> = b
            .nodes
            .keys()
            .filter(|n| !t.nodes.contains_key(n))
            .copied()
            .collect();
        if !born.is_empty() {
            let tm = Rc::make_mut(&mut t);
            for n in born {
                let mut y: Node = b.nodes[&n].clone();
                if y.live() {
                    y.tomb = Some(Tomb {
                        reason: Some(String::new()),
                        replaced_by: None,
                    });
                    crate::delete::tombstone_keys(&mut y, &b.schema);
                    y.out.clear();
                }
                tm.nodes.insert(n, y);
            }
        }
    }
    (b, t)
}

/// The kinds of a staged commit on a staging ref ([F12 §9.4]: kinds 1–4).
const STAGED_KINDS: [&str; 4] = ["merge", "sync", "revert", "cherry-pick"];

/// The commits on the staging ref with id `g`, in the order they landed: its staged commits and the resolve commits
/// after each. A re-staged commit's first parent is D₁, not G's previous tip, so they are found by their ref.
fn staging_chain(dag: &Dag, g: u32) -> Vec<u64> {
    let mut chain: Vec<u64> = dag
        .commits
        .values()
        .filter(|c| c.ref_id == g)
        .map(|c| c.seq)
        .collect();
    chain.sort_by_key(|c| dag.commits[c].ref_seq);
    chain
}

/// s of [F12 §9.4]: the newest staged commit on the staging ref with id `g`.
pub(crate) fn newest_staged(dag: &Dag, g: u32) -> Option<u64> {
    staging_chain(dag, g)
        .into_iter()
        .rev()
        .find(|c| STAGED_KINDS.contains(&dag.commits[c].kind))
}

/// One side of the operation the staged commit `s` stands for, computed at its first parent D₀ ([F12 §9.4] step 1),
/// as a `Resolve` on a violation's key reads it ([F06 §7.7]; [F12 §6.5]): `side` 0 is the base (for a merge or sync
/// the `--base` its `stage` group records ([F06 §4.4.16]), else the base of §4.3 for (D₀, P₂); for a revert or
/// cherry-pick its DM row's), 1 ours (the state of D₀), 2 theirs (the state of P₂, or the revert's or cherry-pick's
/// src, [`pick_states`]).
pub(crate) fn staged_side(
    dag: &Dag,
    alloc: &dyn Alloc,
    uidx: &BTreeMap<Uid, Nid>,
    s: u64,
    side: usize,
) -> Rc<State> {
    let c = &dag.commits[&s];
    let d0 = c.parents.first().copied();
    if side == 1 {
        return dag.state_at(d0, alloc);
    }
    match c.kind {
        "merge" | "sync" => {
            let p2 = c.parents.get(1).copied();
            if side == 2 {
                return dag.state_at(p2, alloc);
            }
            let forced = c.stage.as_ref().and_then(|g| g.base);
            let uid = |n: Nid| alloc.uid(n);
            let nid = |u: Uid| uidx.get(&u).copied();
            Bases::new(dag, alloc, &uid, &nid).base(d0, p2, forced).st
        }
        _ => {
            let origin = c
                .origin
                .expect("a staged revert or cherry-pick has its origin");
            let (b, t) = pick_states(dag, alloc, origin, c.kind == "revert");
            if side == 0 { b } else { t }
        }
    }
}

/// A key with every `#N` it holds (its node, an edge's destination) rewritten through `f`.
fn map_key(k: &Key, f: &dyn Fn(Nid) -> Nid) -> Key {
    match k {
        Key::Node(n, Aspect::Edge(e)) => Key::Node(
            f(*n),
            Aspect::Edge(crate::state::EdgeKey {
                kind: e.kind.clone(),
                dst: f(e.dst),
                disc: e.disc,
            }),
        ),
        Key::Node(n, a) => Key::Node(f(*n), a.clone()),
        Key::Schema(i) => Key::Schema(i.clone()),
    }
}

impl Store {
    /// The `#N` the store's `UIDX` gives a uid.
    fn uidx(&self, u: Uid) -> Option<Nid> {
        self.alloc.uidx.get(&u).copied()
    }

    pub(crate) fn rev_ctx(&self, caller: &Caller) -> RevCtx {
        RevCtx {
            head: match caller.detached {
                Some(c) if caller.branch.is_empty() => Err(c),
                _ => Ok(caller.branch.clone()),
            },
            now_ms: self.env.wall_ms,
            reflog_expire_ms: self.conf.number("gc.reflog-expire"),
        }
    }

    /// The live ref of a name, or E301.
    fn live_ref(&self, name: &str) -> Res<Ref> {
        self.dag
            .live(name)
            .cloned()
            .ok_or_else(|| Refusal::lq("E301", format!("no ref {name}")))
    }

    /// Sets each overlaid key of a pending candidate to its staged value and settles its typed records ([F12 §9.4]
    /// step 2), so that the validators re-check the overlaid candidate (step 3).
    fn overlay(&self, p: &mut merge::Pending<'_>, ov: &Overlay) {
        let mut cs = crate::state::Changeset::new();
        for (k, want) in ov {
            let now = p.m.st.kstate(k);
            if now != *want {
                cs.insert(k.clone(), (now, want.clone()));
            }
        }
        p.m.st.apply(&cs, &self.alloc);
        p.settle(&|k| ov.contains_key(k));
    }

    /// A merge of `src_tip` into the tip of `dst` (DM-001, DM-002): the base of [F12 §4.3], the typed merge, the staged
    /// resolutions of `merge --continue` when given ([F12 §9.4] step 2), and the validators.
    // rule: DM-001, DM-002, PR-004
    #[allow(clippy::too_many_arguments)]
    fn plan_merge(
        &self,
        op: Op,
        dst: &str,
        src: &str,
        src_tip: Option<u64>,
        forced: Option<u64>,
        policy: Option<&str>,
        overlay: Option<&Overlay>,
    ) -> Plan {
        let d = self.dag.live(dst).expect("dst is live");
        let dst_tip = d.tip;
        let uid = |n: Nid| self.alloc.uid(n);
        let nid = |u: Uid| self.uidx(u);
        let mut bases = Bases::new(&self.dag, &self.alloc, &uid, &nid);
        let base = bases.base(dst_tip, src_tip, forced);
        let o = self.dag.state_at(dst_tip, &self.alloc);
        let t = self.dag.state_at(src_tip, &self.alloc);
        let (ao, at) = (self.dag.ancestors(dst_tip), self.dag.ancestors(src_tip));
        let mo = self.dag.move_steps(&ao, &base.anc);
        let mt = self.dag.move_steps(&at, &base.anc);
        // `merge.policy.<kind>` of dst's view ([CFG §10.13]).
        let auto = crate::policy::merge_policies(&o.schema);
        let cx = MCtx {
            op,
            dst_main: dst == "main",
            dst_plan: d.kind == RefKind::Plan,
            policy,
            auto: &auto,
            moves: [&mo[..], &mt[..]],
            uid: &uid,
            nid: &nid,
        };
        let mut fresh = bases.fresh.clone();
        let mut p = merge::typed(&base.st, &o, &t, &cx, &mut fresh);
        if let Some(ov) = overlay {
            self.overlay(&mut p, ov);
        }
        let m = p.validate(&cx);
        Plan {
            kind: if op == Op::Sync { "sync" } else { "merge" },
            dst: dst.to_string(),
            src: src.to_string(),
            parents: dst_tip.into_iter().chain(src_tip).collect(),
            origin: None,
            sync_base: if op == Op::Sync { src_tip } else { None },
            m,
            fresh,
            lcas: base.lcas,
            virtual_base: base.virtual_base,
            stage: None,
        }
    }

    /// A revert or cherry-pick of `c` onto the tip of `onto` (DM-003 to DM-005): dst = tip(R); src and base are the
    /// commit's and its first parent's states, swapped for a revert; a revert never makes a node `absent` (DM-017); a
    /// src change whose owner is live in the base and gone on dst is `NotFound` (DM-012); the staged resolutions of
    /// `merge --continue` when given ([F12 §9.4] step 2); the validators. A root node's `path_moves` is an ordinary set
    /// field here, so reverting the commit that added an entry removes it and cherry-picking it adds it
    /// ([RULES/link-merge-rules] LH-003, LH-004); the plan reads states only and never touches a project tree (LH-001).
    // rule: DM-003, DM-004, DM-005, DM-012, DM-013, DM-017, LH-001, LH-003, LH-004
    fn plan_pick(&self, onto: &str, c: u64, revert: bool, overlay: Option<&Overlay>) -> Plan {
        let d = self.dag.live(onto).expect("onto is live");
        let dst_tip = d.tip;
        let commit = &self.dag.commits[&c];
        let (b, t) = pick_states(&self.dag, &self.alloc, c, revert);
        let o = self.dag.state_at(dst_tip, &self.alloc);
        let uid = |n: Nid| self.alloc.uid(n);
        let nid = |u: Uid| self.uidx(u);
        let base_at = if revert {
            Some(c)
        } else {
            commit.parents.first().copied()
        };
        let mo = self
            .dag
            .move_steps(&self.dag.ancestors(dst_tip), &self.dag.ancestors(base_at));
        // The src side is one step, the origin's, with each node's value on src: a revert's src (p1(c)) has no commit
        // outside A(base), so its moves are ordered by the commit they invert.
        let moves: merge::Moves = commit
            .changeset
            .keys()
            .filter_map(|k| match k {
                Key::Node(n, Aspect::Hierarchy) => {
                    Some((*n, merge::flat(&merge::cval(&t, *n, &Aspect::Hierarchy))))
                }
                _ => None,
            })
            .collect();
        let mt: Vec<Step> = if moves.is_empty() {
            Vec::new()
        } else {
            vec![Step {
                key: (commit.hlc, commit.id),
                moves,
            }]
        };
        let auto = crate::policy::merge_policies(&o.schema);
        let cx = MCtx {
            op: if revert { Op::Revert } else { Op::CherryPick },
            dst_main: onto == "main",
            dst_plan: d.kind == RefKind::Plan,
            policy: None,
            auto: &auto,
            moves: [&mo[..], &mt[..]],
            uid: &uid,
            nid: &nid,
        };
        let mut fresh = Fresh::default();
        let mut p = merge::typed(&b, &o, &t, &cx, &mut fresh);
        // DM-012: the keys src changes whose owner (an edge's source or destination) is live in the base but deleted or
        // absent on dst.
        let gone = |n: Nid| b.live(n).is_some() && o.live(n).is_none();
        let mut keys: BTreeSet<Key> = BTreeSet::new();
        let mut nodes: BTreeSet<Nid> = b.nodes.keys().copied().collect();
        nodes.extend(t.nodes.keys().copied());
        for n in nodes {
            let mut asp = merge::aspects(&b, n);
            asp.extend(merge::aspects(&t, n));
            asp.insert(Aspect::Existence);
            asp.insert(Aspect::Hierarchy);
            for a in asp {
                let (vb, vt) = (merge::cval(&b, n, &a), merge::cval(&t, n, &a));
                if vb == vt {
                    continue;
                }
                let dst_gone = matches!(&a, Aspect::Edge(e) if gone(e.dst));
                if gone(n) || dst_gone {
                    keys.insert(Key::Node(n, a));
                }
            }
        }
        let mut nf: Vec<Violation> =
            merge::key_order(&keys.into_iter().collect::<Vec<_>>(), &uid, &p.m.st.schema)
                .into_iter()
                .map(|k| Violation {
                    class: "NotFound",
                    code: merge::violation_code("NotFound"),
                    description: format!(
                        "{} changes {}, whose node is gone on {onto}",
                        if revert {
                            "the revert"
                        } else {
                            "the cherry-pick"
                        },
                        merge::key_text(&k, &uid)
                    ),
                    suggested: "resolve the key, or restore its node first".into(),
                    key: Some(k),
                })
                .collect();
        nf.append(&mut p.m.violations);
        p.m.violations = nf;
        if let Some(ov) = overlay {
            self.overlay(&mut p, ov);
        }
        let m = p.validate(&cx);
        Plan {
            kind: if revert { "revert" } else { "cherry-pick" },
            dst: onto.to_string(),
            src: format!("c{}", hex(&commit.id)),
            parents: dst_tip.into_iter().collect(),
            origin: Some(c),
            sync_base: None,
            m,
            fresh,
            lcas: Vec::new(),
            virtual_base: false,
            stage: None,
        }
    }

    /// The uids new to the store that a merge result holds get `#N`s in ascending uid order ([API §9.6] item 2).
    fn renumber(&mut self, st: &mut State, fresh: &Fresh) -> Vec<(Nid, Uid)> {
        let mut new: Vec<(Uid, Nid)> = fresh
            .uid
            .iter()
            .filter(|(n, _)| st.nodes.contains_key(n))
            .map(|(n, u)| (*u, *n))
            .collect();
        new.sort();
        let mut map: BTreeMap<Nid, Nid> = BTreeMap::new();
        let mut out = Vec::new();
        for (u, tmp) in new {
            let n = Nid(self.next_id);
            self.next_id += 1;
            map.insert(tmp, n);
            out.push((n, u));
        }
        if !map.is_empty() {
            map_nids(st, &|n| map.get(&n).copied().unwrap_or(n));
        }
        out
    }

    /// Lands a plan on dst, or stages it on its staging ref, by [`merge::land_or_stage`] ([RULES/merge-table] PR-011,
    /// PR-012; [F12 §8.1], §9.2): the commit's net changeset against dst's tip, its `affected`, its markers (none on a
    /// staging ref, RE-004) and dst's absorbed vector. `restage` names the open staging ref of a `merge --continue`,
    /// which a staged result is appended to instead of a new one ([F12 §9.4] step 4).
    // rule: PR-011, PR-012, RE-001, RE-002, RE-003, RE-004, RE-005, RE-006, RE-007, RE-008, RE-009, RE-010, RE-011
    // rule: RE-012, RE-013, RE-014, RE-015, LF-007, FL-010, CP-012
    fn land(
        &mut self,
        caller: &Caller,
        mut plan: Plan,
        strict: bool,
        message: &str,
        idem: Option<([u8; 16], [u8; 16])>,
        restage: Option<&str>,
    ) -> Landed {
        let decision =
            merge::land_or_stage(plan.m.violations.len(), plan.m.conflicts.len(), strict);
        let dst_tip = self.dag.live(&plan.dst).and_then(|r| r.tip);
        let mut st = std::mem::take(&mut plan.m.st);
        let fresh = std::mem::take(&mut plan.fresh);
        let new = self.renumber(&mut st, &fresh);
        let renum: BTreeMap<Nid, Nid> = new
            .iter()
            .filter_map(|(n, u)| fresh.by_uid.get(u).map(|t| (*t, *n)))
            .collect();
        let on = match (decision, restage) {
            ("stage", Some(g)) => g.to_string(),
            ("stage", None) => {
                let g = staging_name(&plan.dst, &plan.src);
                self.create_staging(&g, dst_tip, &caller.actor);
                g
            }
            _ => plan.dst.clone(),
        };
        // The tip of the ref the commit is appended to, before it: dst's, a new staging ref's (a fork at dst's tip),
        // or a re-staged G's.
        let old = self.dag.live(&on).and_then(|r| r.tip);
        for (n, u) in &new {
            let creator = st
                .nodes
                .get(n)
                .map(|x| x.creator.clone())
                .unwrap_or_default();
            self.alloc
                .rows
                .insert(*n, (*u, creator, on.clone(), self.commit_seq + 1));
            self.alloc.uidx.insert(*u, *n);
            self.alloc.uids.insert(*n, *u);
        }
        let base = self.dag.state_at(dst_tip, &self.alloc);
        let cs = diff(&base, &st);
        let parent_rows = self.rows_at(dst_tip);
        let child_rows = Rc::new(derived::recompute_all(&st, &|_| None));
        let (affected, complete) =
            derived::affected_rows(&parent_rows, &child_rows, self.cfg.suspect_budget);
        let mut c = Commit::new(0, 0, 0, plan.kind, plan.parents.clone(), 0, cs);
        c.message = message.to_string();
        c.origin = plan.origin;
        c.sync_base = plan.sync_base;
        c.stmt_origin = match plan.kind {
            "revert" | "cherry-pick" => "verb",
            _ => "merge",
        };
        c.stmt_sym = match plan.kind {
            "revert" => Some("revert".into()),
            "cherry-pick" => Some("cherry-pick".into()),
            _ => None,
        };
        c.idem = idem;
        c.affected = affected.to_vec();
        c.affected_complete = complete;
        if decision == "stage" {
            // [F12 §9.2] item 3: the command's arguments in the `stage` group ([F06 §4.4.16]).
            c.stage = plan.stage.clone();
            // [F12 §9.2] item 3: one `Violation` op per structural violation, its key on the landed `#N`s.
            let f = |n: Nid| renum.get(&n).copied().unwrap_or(n);
            c.violations = plan
                .m
                .violations
                .iter()
                .map(|v| Violation {
                    key: v.key.as_ref().map(|k| map_key(k, &f)),
                    ..v.clone()
                })
                .collect();
        }
        let st = Rc::new(st);
        let seq = self.append_on(caller, &on, c, st.clone());
        if decision != "stage" {
            self.rows.insert(seq, child_rows);
        }
        self.prune_rows();
        // Markers: the landing commit changes the cache where dst's holds change (RE-003); a staging ref's commit
        // writes none (RE-004: its kind holds nothing), and only its vector is kept.
        let entries =
            self.markers
                .commit_lands(&self.dag, seq, old, &[], &mut self.hlc, self.env.wall_ms);
        let markers = self.listed(&entries);
        self.feed_markers(&entries, Some(seq));
        Landed {
            outcome: if decision == "stage" {
                "staged"
            } else {
                "landed"
            },
            commit: seq,
            staging: (decision == "stage").then_some(on),
            markers,
            affected: affected.into_iter().collect(),
        }
    }

    /// Creates a staging ref at dst's tip ([F12 §9.2] items 1 and 2): a fork of dst, kind `merge`, `RefUpdate` create.
    fn create_staging(&mut self, name: &str, at: Option<u64>, actor: &str) {
        let id = self.next_ref_id;
        self.next_ref_id += 1;
        let hlc = self.hlc.record(self.env.wall_ms);
        self.dag.refs.insert(
            id,
            Ref {
                id,
                name: name.to_string(),
                kind: RefKind::Merge,
                tip: at,
                ref_seq_next: 1,
                fork: at,
                deleted: false,
                message: None,
                pinned: false,
                moves: vec![RefMove {
                    old: None,
                    new: at,
                    reason: MoveReason::Create,
                    actor: actor.to_string(),
                    hlc,
                }],
            },
        );
        let y = self.dag.refs[&id].clone();
        let st = self.dag.state_at(at, &self.alloc);
        let entries = self.markers.fork(
            &self.dag,
            &y,
            &st,
            Group::Move(hlc),
            &mut self.hlc,
            self.env.wall_ms,
        );
        self.feed_markers(&entries, None);
    }

    /// Deletes a staging ref ([F12 §9.4] item 4, §9.5): `RefUpdate` reason 2; it held no marker (RE-004).
    fn delete_staging(&mut self, name: &str, actor: &str) -> Option<(u32, Option<u64>)> {
        let hlc = self.hlc.record(self.env.wall_ms);
        let r = self.dag.live_mut(name)?;
        r.deleted = true;
        let old = r.tip;
        r.moves.push(RefMove {
            old,
            new: None,
            reason: MoveReason::Delete,
            actor: actor.to_string(),
            hlc,
        });
        let (id, r) = (r.id, r.clone());
        let entries = self.markers.ref_deleted(
            &self.dag,
            &r,
            Group::Move(hlc),
            &mut self.hlc,
            self.env.wall_ms,
        );
        self.feed.event(
            self.commit_seq,
            name,
            None,
            None,
            "delete",
            "ref",
            name.to_string(),
            actor,
            None,
        );
        self.feed_markers(&entries, None);
        Some((id, old))
    }

    fn violations_out(&self, v: &[Violation]) -> Vec<ViolationOut> {
        let uid = |n: Nid| self.alloc.uid(n);
        v.iter()
            .map(|x| ViolationOut {
                key: x
                    .key
                    .as_ref()
                    .map_or_else(|| "-".to_string(), |k| merge::key_text(k, &uid)),
                class: x.class.to_string(),
                code: x.code,
                description: x.description.clone(),
                suggested: x.suggested.clone(),
            })
            .collect()
    }

    fn conflicts_out(&self, c: &[(Key, String)]) -> Vec<(String, String)> {
        let uid = |n: Nid| self.alloc.uid(n);
        c.iter()
            .map(|(k, cl)| (merge::key_text(k, &uid), cl.clone()))
            .collect()
    }

    /// A ref's absorbed vector by ref name.
    fn absorbed_named(&self, name: &str) -> BTreeMap<String, u64> {
        let Some(r) = self.dag.live(name) else {
            return BTreeMap::new();
        };
        self.markers
            .absorbed
            .get(&r.id)
            .map(|v| {
                v.iter()
                    .map(|(id, s)| (self.dag.refs[id].name.clone(), *s))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `staged` refusal of [F19 §10.2] with its success keys ([API §2.3]): the staged commit's violations and its
    /// number of conflicts.
    fn staged_reply(
        &self,
        mut r: Reply,
        op_text: String,
        staging: &str,
        dst: &str,
        violations: &[ViolationOut],
        conflicts: usize,
    ) -> Reply {
        let viol: Vec<Kv> = violations
            .iter()
            .map(|v| {
                Kv::Obj(vec![
                    ("key".into(), Kv::Str(v.key.clone())),
                    ("class".into(), Kv::Str(v.class.clone())),
                    ("code".into(), Kv::Int(i64::from(v.code))),
                    ("description".into(), Kv::Str(v.description.clone())),
                    ("suggested".into(), Kv::Str(v.suggested.clone())),
                ])
            })
            .collect();
        let e = Refusal::new(
            "staged",
            6,
            format!(
                "{op_text} staged on {staging}: {} violations, {conflicts} conflicts; {dst} did not move",
                violations.len(),
            ),
        )
        .key("staging_ref", staging)
        .key("violations", Kv::List(viol))
        .key("conflicts", conflicts as i64);
        r.outcome = Outcome::Staged;
        r.exit = 6;
        r.error = Some(e);
        r
    }

    /// The payload of a family-W command ([API §7.3]).
    fn w_payload(name: &str, args: &[(&str, Option<Cj>)]) -> [u8; 16] {
        let m: BTreeMap<String, Cj> = args
            .iter()
            .filter_map(|(k, v)| v.clone().map(|v| (k.to_string(), v)))
            .collect();
        crate::idem::payload(name, &m)
    }

    /// The refusals every merge into a destination shares ([API §11.7]): `into` a writable branch, `src` a mergeable
    /// ref other than `into`.
    fn check_pair(&self, src: &Ref, into: &Ref) -> Res<()> {
        if !matches!(into.kind, RefKind::Work | RefKind::Plan) {
            return Err(Refusal::lq(
                "E305",
                format!("{} is not a branch a merge lands on", into.name),
            ));
        }
        let orphan_of_branch = src.kind == RefKind::Orphans
            && src
                .name
                .strip_prefix("orphans/")
                .is_some_and(|b| b == "main" || b.starts_with("lane/") || b.starts_with("plan/"));
        if !(matches!(
            src.kind,
            RefKind::Work | RefKind::Plan | RefKind::Tag | RefKind::Import
        ) || orphan_of_branch)
        {
            return Err(Refusal::usage_arg(
                "src",
                format!(
                    "{} is not a ref a merge takes; cherry-pick applies one commit",
                    src.name
                ),
            ));
        }
        if src.id == into.id {
            return Err(Refusal::usage_arg(
                "src",
                "a ref is never merged into itself",
            ));
        }
        Ok(())
    }

    fn staging_exists(&self, g: &str) -> Res<()> {
        if self.dag.live(g).is_some() {
            return Err(
                Refusal::new("staging_exists", 6, format!("{g} is open")).key("staging_ref", g)
            );
        }
        Ok(())
    }

    /// The conflict keys a view holds, as key texts.
    fn conflict_keys(&self, tip: Option<u64>) -> Vec<String> {
        let st = self.dag.state_at(tip, &self.alloc);
        let uid = |n: Nid| self.alloc.uid(n);
        let mut v: Vec<String> = Vec::new();
        for (n, x) in &st.nodes {
            for a in x.conflicts.keys() {
                v.push(merge::key_text(&Key::Node(*n, a.clone()), &uid));
            }
        }
        for k in st.schema_conflicts.keys() {
            v.push(k.text());
        }
        v
    }

    /// `Merge` ([API §11.7]; [AR §5a.7] steps 0–8).
    // spec: [API §11.7]
    // rule: PR-001, PR-002, PR-003
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn merge_cmd(
        &mut self,
        src: &str,
        into: Option<&str>,
        policy: Option<&str>,
        strict: Option<bool>,
        base: Option<&str>,
        message: &str,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        // [API §7.3] `args′`: `into`, whose default is the resolved branch, only when given.
        let given_into = into.map(|i| Cj::Str(i.into()));
        let into = into.map_or_else(|| caller.branch.clone(), str::to_string);
        let base_full = base.map(|b| match self.dag.rev_commit(b, &self.rev_ctx(&caller)) {
            Ok(Some(c)) => format!("c{}", hex(&self.dag.commits[&c].id)),
            _ => b.to_string(),
        });
        let payload = Self::w_payload(
            "Merge",
            &[
                ("src", Some(Cj::Str(src.into()))),
                ("into", given_into),
                ("policy", policy.map(|p| Cj::Str(p.into()))),
                ("strict", strict.map(Cj::Bool)),
                ("base", base_full.map(Cj::Str)),
                (
                    "message",
                    (!message.is_empty()).then(|| Cj::Str(message.into())),
                ),
            ],
        );
        let key = Self::explicit_key(ctx);
        if let Some(r) = self.keyed(&key, &payload, &into, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("merge")
            .map_err(|e| e.finish(None))?;
        if let Some(p) = policy
            && p != "delete-wins"
            && p != "resurrect"
        {
            return Err(Refusal::usage_arg(
                "policy",
                format!("--policy takes delete-wins or resurrect, not {p}"),
            ));
        }
        let msg = crate::canon::normalise_message(message)?;
        let s = self.live_ref(src)?;
        let d = self.live_ref(&into)?;
        self.check_pair(&s, &d)?;
        self.staging_exists(&staging_name(&into, src))?;
        let forced = match base {
            Some(b) => Some(
                self.dag
                    .rev_commit(b, &self.rev_ctx(&caller))?
                    .ok_or_else(|| Refusal::lq("E301", format!("{b} names no commit")))?,
            ),
            None => None,
        };
        let strict = strict.unwrap_or_else(|| self.conf.flag("merge.strict"));
        let mut data = MergeData {
            src: src.to_string(),
            into: into.clone(),
            outcome: "up-to-date",
            sync: None,
            lca: Vec::new(),
            virtual_base: false,
            conflicts: Vec::new(),
            violations: Vec::new(),
            staging_ref: None,
            absorbed: BTreeMap::new(),
            markers: Vec::new(),
            affected: Vec::new(),
            origin: None,
            notices: Vec::new(),
            hints: Vec::new(),
        };
        let mut reply = Reply::ok(Data::None);
        reply.branch = Some(into.clone());
        reply.rev = Some(d.tip.unwrap_or(0));
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        // Up to date: src's tip is an ancestor-or-self of into's tip.
        if s.tip.is_none_or(|t| self.dag.ancestors(d.tip).contains(&t)) {
            data.absorbed = self.absorbed_named(&into);
            reply.data = Data::Merge(Box::new(data));
            return Ok(reply);
        }
        // PR-003: a merge into `main` while src holds unresolved conflicts; `sync` is null, as no step 0 ran.
        if into == "main" {
            let keys = self.conflict_keys(s.tip);
            if !keys.is_empty() {
                return Err(conflicted_src(src, keys, None));
            }
        }
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            reply.data = Data::Merge(Box::new(data));
            return Ok(reply);
        }
        // PR-001: sync first when `main`'s tip is not in src's history. Only a branch is synced (`work` or `plan`): a
        // tag never moves ([F12 §2.2]) and an import or orphans ref takes no sync commit, so such a src merges directly
        // over the base of §4.3 and nothing is written on it ([F12 §8.1], §9.6). Step 0 runs with the merge's policy
        // override and effective `strict` over §4.3's base, never with its `--base`; a staged step-0 sync records the
        // override and `strict` but no base ([F06 §4.4.16]).
        let mut src_tip = s.tip;
        let src_branch = matches!(s.kind, RefKind::Work | RefKind::Plan);
        if into == "main"
            && src_branch
            && !d.tip.is_none_or(|t| self.dag.ancestors(s.tip).contains(&t))
        {
            self.staging_exists(&staging_name(src, "main"))?;
            let mut plan = self.plan_merge(Op::Sync, src, "main", d.tip, None, policy, None);
            plan.stage = Stage::of(None, policy, strict);
            let conflicts = self.conflicts_out(&plan.m.conflicts);
            let violations = self.violations_out(&plan.m.violations);
            let decision =
                merge::land_or_stage(plan.m.violations.len(), plan.m.conflicts.len(), strict);
            let landed = self.land(&caller, plan, strict, "", None, None);
            let sync = SyncOut {
                commit: Some(landed.commit),
                outcome: landed.outcome,
                conflicts,
                violations,
            };
            if decision == "land-conflicted" {
                // [API §11.7] "Sync first with conflict values": the sync's group alone on src, with no idempotency
                // pair, then PR-003's refusal carrying that sync; nothing is written for `main` or the merge's key.
                let keys = self.conflict_keys(Some(landed.commit));
                return Err(conflicted_src(src, keys, Some(&sync)));
            }
            if landed.outcome == "staged" {
                // [API §11.7]: no merge is computed; the staged items are reported once, in `sync`, and the top-level
                // `conflicts`, `violations`, `markers` and `lca` stay empty.
                let g = landed.staging.clone().expect("a staging ref");
                let (sv, sc) = (sync.violations.clone(), sync.conflicts.len());
                data.sync = Some(sync);
                data.outcome = "staged";
                data.staging_ref = Some(g.clone());
                data.absorbed = self.absorbed_named(&g);
                self.record_idem(
                    key,
                    payload,
                    &into,
                    Some(landed.commit),
                    ctx,
                    Recorded {
                        cmd: "Merge".into(),
                        ..Recorded::default()
                    },
                );
                reply.commit = Some(landed.commit);
                reply.rev_new = Some(landed.commit);
                self.vcs_results.insert(landed.commit, data.clone());
                let op_text = format!("sync of {src}");
                let r = self.staged_reply(reply, op_text, &g, src, &sv, sc);
                return Ok(Reply {
                    data: Data::Merge(Box::new(data)),
                    ..r
                });
            }
            data.sync = Some(sync);
            src_tip = Some(landed.commit);
            data.markers = landed.markers;
        }
        let mut plan = self.plan_merge(Op::Merge, &into, src, src_tip, forced, policy, None);
        plan.stage = Stage::of(forced, policy, strict);
        data.lca = plan.lcas.clone();
        data.virtual_base = plan.virtual_base;
        data.conflicts = self.conflicts_out(&plan.m.conflicts);
        data.violations = self.violations_out(&plan.m.violations);
        data.hints = hints_out(&plan.m.hints);
        let landed = self.land(
            &caller,
            plan,
            strict,
            &msg,
            key.map(|(k, _)| (k, payload)),
            None,
        );
        self.finish_merge(
            reply,
            data,
            landed,
            key,
            payload,
            &into,
            "Merge",
            ctx,
            format!("merge of {src} into {into}"),
        )
    }

    /// The reply, idempotency entry and replay record of a landed or staged merge-family commit.
    #[allow(clippy::too_many_arguments)]
    fn finish_merge(
        &mut self,
        mut reply: Reply,
        mut data: MergeData,
        landed: Landed,
        key: Option<([u8; 16], bool)>,
        payload: [u8; 16],
        branch: &str,
        cmd: &str,
        ctx: &Ctx,
        op_text: String,
    ) -> Res<Reply> {
        data.outcome = landed.outcome;
        data.markers.extend(landed.markers);
        data.affected = landed.affected;
        data.staging_ref = landed.staging.clone();
        data.absorbed = self.absorbed_named(landed.staging.as_deref().unwrap_or(branch));
        self.record_idem(
            key,
            payload,
            branch,
            Some(landed.commit),
            ctx,
            Recorded {
                cmd: cmd.into(),
                ..Recorded::default()
            },
        );
        reply.commit = Some(landed.commit);
        reply.rev_new = Some(landed.commit);
        reply.markers = data.markers.clone();
        self.vcs_results.insert(landed.commit, data.clone());
        if let Some(g) = &landed.staging {
            let r = self.staged_reply(
                reply,
                op_text,
                g,
                branch,
                &data.violations,
                data.conflicts.len(),
            );
            return Ok(Reply {
                data: Data::Merge(Box::new(data)),
                ..r
            });
        }
        reply.data = Data::Merge(Box::new(data));
        Ok(reply)
    }

    /// `Sync` ([API §11.9]): `Merge(src: main, into: lane)` with a commit of kind `sync`; `check` previews only.
    // spec: [API §11.9]
    pub(crate) fn sync_cmd(&mut self, lane: Option<&str>, check: bool, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let given_lane = lane.map(|l| Cj::Str(l.into()));
        let lane = lane.map_or_else(|| caller.branch.clone(), str::to_string);
        if lane == "main" {
            return Err(Refusal::usage_arg("lane", "main is never synced"));
        }
        let payload = Self::w_payload(
            "Sync",
            &[
                ("lane", given_lane),
                ("check", check.then_some(Cj::Bool(true))),
            ],
        );
        let key = if check { None } else { Self::explicit_key(ctx) };
        if let Some(r) = self.keyed(&key, &payload, &lane, ctx, &caller)? {
            return Ok(r);
        }
        if !check {
            self.rights(&caller, ctx)
                .verb("sync")
                .map_err(|e| e.finish(None))?;
        }
        let m = self.live_ref("main")?;
        let d = self.live_ref(&lane)?;
        self.check_pair(&m, &d)?;
        self.staging_exists(&staging_name(&lane, "main"))?;
        let strict = self.conf.flag("merge.strict");
        let mut data = MergeData {
            src: "main".into(),
            into: lane.clone(),
            outcome: "up-to-date",
            sync: None,
            lca: Vec::new(),
            virtual_base: false,
            conflicts: Vec::new(),
            violations: Vec::new(),
            staging_ref: None,
            absorbed: BTreeMap::new(),
            markers: Vec::new(),
            affected: Vec::new(),
            origin: None,
            notices: Vec::new(),
            hints: Vec::new(),
        };
        let mut reply = Reply::ok(Data::None);
        reply.branch = Some(lane.clone());
        reply.rev = Some(d.tip.unwrap_or(0));
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        if m.tip.is_none_or(|t| self.dag.ancestors(d.tip).contains(&t)) {
            data.absorbed = self.absorbed_named(&lane);
            reply.data = Data::Merge(Box::new(data));
            return Ok(reply);
        }
        let mut plan = self.plan_merge(Op::Sync, &lane, "main", m.tip, None, None, None);
        plan.stage = Stage::of(None, None, strict);
        data.lca = plan.lcas.clone();
        data.virtual_base = plan.virtual_base;
        data.conflicts = self.conflicts_out(&plan.m.conflicts);
        data.violations = self.violations_out(&plan.m.violations);
        if check || ctx.dry {
            // `--check` and DRY: the preview ([AR §5a.7] step 0, D5); nothing is appended.
            data.outcome =
                if merge::land_or_stage(plan.m.violations.len(), plan.m.conflicts.len(), strict)
                    == "stage"
                {
                    "staged"
                } else {
                    "landed"
                };
            data.affected =
                crate::state::touched(&diff(&self.dag.state_at(d.tip, &self.alloc), &plan.m.st))
                    .into_iter()
                    .collect();
            reply.outcome = if ctx.dry { Outcome::Dry } else { Outcome::Ok };
            reply.data = Data::Merge(Box::new(data));
            return Ok(reply);
        }
        let landed = self.land(
            &caller,
            plan,
            strict,
            "",
            key.map(|(k, _)| (k, payload)),
            None,
        );
        self.finish_merge(
            reply,
            data,
            landed,
            key,
            payload,
            &lane,
            "Sync",
            ctx,
            format!("sync of {lane}"),
        )
    }

    /// The live staging ref `merge --continue` and `merge --abort` name ([F12 §9.4]): `src` and `into` when given,
    /// else the caller's branch's single open staging ref.
    fn named_staging(&self, caller: &Caller, src: Option<&str>, into: Option<&str>) -> Res<Ref> {
        if let Some(s) = src {
            let into = into.map_or_else(|| caller.branch.clone(), str::to_string);
            let g = staging_name(&into, s);
            return self.live_ref(&g);
        }
        let prefix = format!("merge/{}/from/", into.unwrap_or(&caller.branch));
        let open: Vec<&Ref> = self
            .dag
            .live_refs()
            .filter(|r| r.kind == RefKind::Merge && r.name.starts_with(&prefix))
            .collect();
        match open.as_slice() {
            [g] => Ok((*g).clone()),
            [] => Err(Refusal::usage(format!(
                "{} has no open staging ref",
                caller.branch
            ))),
            _ => Err(Refusal::usage(
                "several staging refs are open: name src and into",
            )),
        }
    }

    /// `MergeAbort` ([API §11.8]; [F12 §9.5]): the staging ref is deleted; nothing else changes.
    // spec: [F12 §9.5]
    // rule: PR-015
    pub(crate) fn merge_abort(
        &mut self,
        src: Option<&str>,
        into: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let g = self.named_staging(&caller, src, into)?;
        let dst = g.name["merge/".len()..]
            .split("/from/")
            .next()
            .unwrap_or("")
            .to_string();
        let payload = Self::w_payload(
            "MergeAbort",
            &[
                ("src", src.map(|s| Cj::Str(s.into()))),
                ("into", into.map(|s| Cj::Str(s.into()))),
            ],
        );
        let key = self.key_of(ctx, &caller, &dst, &payload);
        if let Some(r) = self.keyed(&key, &payload, &dst, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("merge")
            .map_err(|e| e.finish(None))?;
        let mut reply = Reply::ok(Data::MergeAbort(g.name.clone()));
        reply.branch = Some(dst.clone());
        reply.rev = Some(self.dag.live(&dst).and_then(|r| r.tip).unwrap_or(0));
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            return Ok(reply);
        }
        let (id, old) = self.delete_staging(&g.name, &caller.actor).expect("live");
        self.record_idem(
            key,
            payload,
            &dst,
            None,
            ctx,
            Recorded {
                cmd: "MergeAbort".into(),
                items: vec![ResultItem::RefMove {
                    ref_id: id,
                    reason: MoveReason::Delete.code(),
                    old,
                    new: None,
                }],
                yields: Vec::new(),
            },
        );
        Ok(reply)
    }

    /// `MergeContinue` ([API §11.8]; [F12 §9.4]): the operation the newest staged commit of G stands for, recomputed
    /// against dst's current tip; each staged resolution applies unless dst changed its key since the tip it was made
    /// against (a notice then names it); validated in I37′ order; landed with G deleted, or staged again on G.
    // spec: [F12 §9.4]
    // rule: PR-013, PR-014
    pub(crate) fn merge_continue(
        &mut self,
        src: Option<&str>,
        into: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let g = self.named_staging(&caller, src, into)?;
        let (dst, src_name) = {
            let rest = &g.name["merge/".len()..];
            let (a, b) = rest.split_once("/from/").expect("a staging name");
            (a.to_string(), b.to_string())
        };
        let payload = Self::w_payload(
            "MergeContinue",
            &[
                ("src", src.map(|s| Cj::Str(s.into()))),
                ("into", into.map(|s| Cj::Str(s.into()))),
            ],
        );
        let key = Self::explicit_key(ctx);
        if let Some(r) = self.keyed(&key, &payload, &dst, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("merge")
            .map_err(|e| e.finish(None))?;
        let chain = staging_chain(&self.dag, g.id);
        let s = newest_staged(&self.dag, g.id).expect("a staging ref holds a staged commit");
        let sc = self.dag.commits[&s].clone();
        let d = self.live_ref(&dst)?;
        let d1 = d.tip;
        // Step 1: s's own arguments, its `stage` group ([F06 §4.4.16]); without the group (a revert or cherry-pick
        // always), no `--base`, no override and `strict` false, never the continue's configuration.
        let (forced, policy, strict) = match &sc.stage {
            Some(g) => (g.base, g.policy.clone(), g.strict),
            None => (None, None, false),
        };
        // Step 2's choice first: which staged resolutions apply. For each key a `Resolve` commit on G set, the newest
        // one counts; it applies when dst's value of the key at D₁ equals its value at D_k, the first parent of the
        // newest staged commit before it, and is otherwise stale (PR-014).
        let tip_g = self.dag.state_at(g.tip, &self.alloc);
        let st_d1 = self.dag.state_at(d1, &self.alloc);
        let mut resolved: BTreeMap<Key, u64> = BTreeMap::new();
        let mut staged_before = None;
        for c in &chain {
            let x = &self.dag.commits[c];
            if STAGED_KINDS.contains(&x.kind) {
                staged_before = Some(*c);
                continue;
            }
            let dk = staged_before.and_then(|sb| self.dag.commits[&sb].parents.first().copied());
            // The keys its ops changed and the keys its `Resolve` ops name, a violation's key that kept its value
            // included.
            for k in x.changeset.keys().chain(&x.resolves) {
                resolved.insert(k.clone(), dk.unwrap_or(0));
            }
        }
        let mut notices = Vec::new();
        let uid = |n: Nid| self.alloc.uid(n);
        let mut overlay = Overlay::new();
        for (k, dk) in &resolved {
            let st_dk = self.dag.state_at((*dk != 0).then_some(*dk), &self.alloc);
            if st_d1.kstate(k) == st_dk.kstate(k) {
                overlay.insert(k.clone(), tip_g.kstate(k));
            } else {
                notices.push(format!(
                    "the resolution of {} is stale: {dst} changed it after staging",
                    merge::key_text(k, &uid)
                ));
            }
        }
        drop(tip_g);
        drop(st_d1);
        // Steps 1–3: the operation s stands for, computed afresh against D₁; the overlay on its typed candidate, whose
        // records on the overlaid keys are settled while every other key keeps the typed rules' violations and
        // conflicts; the validators over the merge's own base, dst and src states, V01 with the moves Kleppmann
        // skipped on keys no resolution set.
        let mut plan = match sc.kind {
            "merge" | "sync" => {
                let op = if sc.kind == "sync" {
                    Op::Sync
                } else {
                    Op::Merge
                };
                let p2 = sc.parents.get(1).copied();
                self.plan_merge(
                    op,
                    &dst,
                    &src_name,
                    p2,
                    forced,
                    policy.as_deref(),
                    Some(&overlay),
                )
            }
            _ => self.plan_pick(
                &dst,
                sc.origin.expect("a pick has its origin"),
                sc.kind == "revert",
                Some(&overlay),
            ),
        };
        plan.src = src_name.clone();
        // A re-staged commit copies s's group ([F06 §4.4.16]).
        plan.stage = sc.stage.clone();
        let data = MergeData {
            src: src_name.clone(),
            into: dst.clone(),
            outcome: "landed",
            sync: None,
            lca: plan.lcas.clone(),
            virtual_base: plan.virtual_base,
            conflicts: self.conflicts_out(&plan.m.conflicts),
            violations: self.violations_out(&plan.m.violations),
            staging_ref: None,
            absorbed: BTreeMap::new(),
            markers: Vec::new(),
            affected: Vec::new(),
            origin: plan.origin,
            notices,
            hints: hints_out(&plan.m.hints),
        };
        let mut reply = Reply::ok(Data::None);
        reply.branch = Some(dst.clone());
        reply.rev = Some(d1.unwrap_or(0));
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            reply.data = Data::Merge(Box::new(data));
            return Ok(reply);
        }
        // Step 4: land (G deleted) or re-stage on G, with s's kind, parents, message and origin.
        let msg = sc.message.clone();
        let op_text = format!("merge of {src_name} into {dst}");
        let landed = self.land(
            &caller,
            plan,
            strict,
            &msg,
            key.map(|(k, _)| (k, payload)),
            Some(&g.name),
        );
        if landed.outcome == "landed" {
            self.delete_staging(&g.name, &caller.actor);
        }
        self.finish_merge(
            reply,
            data,
            landed,
            key,
            payload,
            &dst,
            "MergeContinue",
            ctx,
            op_text,
        )
    }

    /// `Revert` and `CherryPick` ([API §11.10]; [AR §5a.5]; I34′).
    // spec: [API §11.10]
    // rule: DM-006, DM-007, DM-008, DM-016, UD-001, UD-002, UD-003, UD-004, UD-005, UD-006, UD-007, UD-008, FL-009
    // rule: LH-005
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pick_cmd(
        &mut self,
        commit: &str,
        onto: Option<&str>,
        mainline: Option<u32>,
        message: &str,
        revert: bool,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let given_onto = onto.map(|o| Cj::Str(o.into()));
        let onto = onto.map_or_else(|| caller.branch.clone(), str::to_string);
        let name = if revert { "Revert" } else { "CherryPick" };
        // [API §7.3] `args′`: a commit prefix as the full id, so every spelling of one commit gives one payload.
        let full = match self.dag.rev_commit(commit, &self.rev_ctx(&caller)) {
            Ok(Some(c)) => format!("c{}", hex(&self.dag.commits[&c].id)),
            _ => commit.to_string(),
        };
        let payload = Self::w_payload(
            name,
            &[
                ("commit", Some(Cj::Str(full))),
                ("onto", given_onto),
                ("mainline", mainline.map(|m| Cj::Int(i64::from(m)))),
                (
                    "message",
                    (!message.is_empty()).then(|| Cj::Str(message.into())),
                ),
            ],
        );
        let key = self.key_of(ctx, &caller, &onto, &payload);
        if let Some(r) = self.keyed(&key, &payload, &onto, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb(if revert { "revert" } else { "cherry-pick" })
            .map_err(|e| e.finish(None))?;
        if let Some(m) = mainline
            && m != 1
        {
            return Err(Refusal::usage_arg(
                "mainline",
                "only --mainline 1 is supported",
            ));
        }
        let msg = crate::canon::normalise_message(message)?;
        let c = self
            .dag
            .rev_commit(commit, &self.rev_ctx(&caller))?
            .ok_or_else(|| Refusal::lq("E301", format!("{commit} names no commit")))?;
        // DM-016: a commit gc pruned to its header.
        if self.pruned.contains(&c) {
            return Err(Refusal::new(
                "commit_pruned",
                3,
                format!(
                    "commit c{} was pruned by gc; its changes are gone",
                    &hex(&self.dag.commits[&c].id)[..8]
                ),
            )
            .key("commit", Kv::Commit(c)));
        }
        let r = self.live_ref(&onto)?;
        self.writable(&onto, false)?;
        let x = self.dag.commits[&c].clone();
        let c8 = hex(&x.id)[..8].to_string();
        if revert {
            let refused = |case: &str, why: String, deps: Vec<Kv>| {
                Refusal::new("revert_refused", 6, why)
                    .key("commit", Kv::Commit(c))
                    .key("case", case)
                    .key("dependents", Kv::List(deps))
            };
            if x.kind == "sync" {
                return Err(refused(
                    "sync",
                    format!("c{c8} is a sync commit; a sync is never reverted"),
                    vec![],
                ));
            }
            if x.parents.len() == 2 && mainline != Some(1) {
                return Err(refused(
                    "mainline",
                    format!("c{c8} is a merge; revert it with --mainline 1"),
                    vec![],
                ));
            }
            let deps = self.dependents(c, r.tip);
            if !deps.is_empty() {
                return Err(refused(
                    "dependents",
                    format!("c{c8} has dependent commits"),
                    deps.into_iter().map(Kv::Commit).collect(),
                ));
            }
        }
        self.staging_exists(&staging_name(&onto, &format!("c{}", hex(&x.id))))?;
        // A revert or cherry-pick takes no `--strict` and `merge.strict` governs merges and syncs only: a `DATA` case
        // lands a conflict value ([API §11.10]; [F06 §4.4.16]; [CFG §10] `merge.strict`).
        let strict = false;
        let plan = self.plan_pick(&onto, c, revert, None);
        let data = MergeData {
            src: plan.src.clone(),
            into: onto.clone(),
            outcome: "landed",
            sync: None,
            lca: Vec::new(),
            virtual_base: false,
            conflicts: self.conflicts_out(&plan.m.conflicts),
            violations: self.violations_out(&plan.m.violations),
            staging_ref: None,
            absorbed: BTreeMap::new(),
            markers: Vec::new(),
            affected: Vec::new(),
            origin: Some(c),
            notices: Vec::new(),
            hints: Vec::new(),
        };
        let mut reply = Reply::ok(Data::None);
        reply.branch = Some(onto.clone());
        reply.rev = Some(r.tip.unwrap_or(0));
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        // LH-005: a revert or cherry-pick of a commit whose group carried an `FsIntentDone` is graph-only; the warning
        // names `file revert`, which moves the files back ([F19 §10.4] `graph_only_revert`).
        if self
            .files
            .intents
            .iter()
            .any(|i| i.commit == Some(c) && i.state == crate::links::IntentState::Done)
        {
            reply.warnings.push("graph_only_revert".into());
        }
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            reply.data = Data::Merge(Box::new(data));
            return Ok(reply);
        }
        let landed = self.land(
            &caller,
            plan,
            strict,
            &msg,
            key.map(|(k, _)| (k, payload)),
            None,
        );
        let op_text = format!(
            "{} of c{c8} onto {onto}",
            if revert { "revert" } else { "cherry-pick" }
        );
        self.finish_merge(reply, data, landed, key, payload, &onto, name, ctx, op_text)
    }

    /// DM-008: the later commits on R that depend structurally on c — one that added a structural edge to, or a child
    /// of, a node c created; one that completed a task c reopened; one that resolved a conflict c introduced.
    // rule: DM-008
    fn dependents(&self, c: u64, tip: Option<u64>) -> Vec<u64> {
        let x = &self.dag.commits[&c];
        let (mut created, mut reopened, mut introduced) =
            (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        let done = |v: &KState| matches!(v, KState::Plain(Some(KVal::Status { status, .. })) if status == "done" || status == "cancelled");
        for (k, (before, after)) in &x.changeset {
            match k {
                Key::Node(n, Aspect::Existence)
                    if *before == KState::ABSENT
                        && matches!(after, KState::Plain(Some(KVal::Live(_)))) =>
                {
                    created.insert(*n);
                }
                Key::Node(n, Aspect::Status) if done(before) && !done(after) => {
                    reopened.insert(*n);
                }
                _ => {}
            }
            if matches!(after, KState::Conflict(_)) {
                introduced.insert(k.clone());
            }
        }
        let later: Vec<u64> = self
            .dag
            .ancestors(tip)
            .into_iter()
            .filter(|d| *d != c && self.dag.ancestors(Some(*d)).contains(&c))
            .collect();
        let st = self.dag.state_at(tip, &self.alloc);
        later
            .into_iter()
            .filter(|d| {
                self.dag.commits[d].changeset.iter().any(|(k, (before, after))| match k {
                    Key::Node(_, Aspect::Edge(e)) => {
                        created.contains(&e.dst)
                            && *before == KState::ABSENT
                            && *after != KState::ABSENT
                            && st
                                .schema
                                .edge(&e.kind)
                                .is_some_and(|x| x.class == crate::schema::EdgeClass::Structural)
                    }
                    Key::Node(_, Aspect::Hierarchy) => matches!(after, KState::Plain(Some(KVal::Hierarchy { parent: Some(p), .. })) if created.contains(p)),
                    Key::Node(n, Aspect::Status) => reopened.contains(n) && done(after),
                    _ => introduced.contains(k) && matches!(before, KState::Conflict(_)) && matches!(after, KState::Plain(_)),
                })
            })
            .collect()
    }

    /// The triage lines of listed marker entries.
    fn triage(markers: &[MarkerOut]) -> Vec<String> {
        markers
            .iter()
            .map(|m| {
                format!(
                    "{} {} (origin {} s{}, cause {})",
                    m.kind.name(),
                    m.id,
                    m.ref_,
                    m.commit,
                    m.cause.name()
                )
            })
            .collect()
    }

    /// `Undo` ([API §11.11]; [AR §5a.5]): R moves back to its value n moves ago, `R@n` of [F12 §3.5], with its markers
    /// recomputed over the move (ME-006).
    // spec: [API §11.11] Undo
    pub(crate) fn undo_cmd(
        &mut self,
        ref_: Option<&str>,
        n: u32,
        expect: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let name = ref_.map_or_else(|| caller.branch.clone(), str::to_string);
        let payload = Self::w_payload(
            "Undo",
            &[
                ("ref", ref_.map(|r| Cj::Str(r.into()))),
                ("n", (n != 1).then_some(Cj::Int(i64::from(n)))),
                ("expect", expect.map(|e| Cj::Str(e.into()))),
            ],
        );
        let key = Self::explicit_key(ctx);
        if let Some(r) = self.keyed(&key, &payload, &name, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("undo")
            .map_err(|e| e.finish(None))?;
        let r = self.live_ref(&name)?;
        if !matches!(r.kind, RefKind::Work | RefKind::Plan) {
            return Err(Refusal::lq(
                "E305",
                format!("{name} is not a branch undo moves"),
            ));
        }
        if n == 0 {
            return Err(Refusal::usage_arg("n", "undo moves back at least one move"));
        }
        if let Some(e) = expect {
            let want = self.dag.rev_commit(e, &self.rev_ctx(&caller))?;
            if want != r.tip {
                return Err(Refusal::lq(
                    "E402",
                    format!(
                        "{name} moved: expected s{}, the tip is s{}",
                        want.unwrap_or(0),
                        r.tip.unwrap_or(0)
                    ),
                )
                .key("statement", Kv::Null)
                .key("tip", r.tip.map_or(Kv::Null, Kv::Commit))
                .key("expected_tip", want.map_or(Kv::Null, Kv::Commit))
                .key("targets", Kv::Null)
                .key("written", false));
            }
        }
        let target = r
            .moves
            .iter()
            .rev()
            .nth(n as usize - 1)
            .ok_or_else(|| {
                Refusal::lq(
                    "E301",
                    format!("{name}@{n}: {name} has {} recorded moves", r.moves.len()),
                )
            })?
            .old;
        // `undo N` moves R to the value `R@N` names, and a zero value is E301 ([F12 §3.5]): the move that created R, or
        // `main`'s first commit, is not undone.
        if target.is_none() {
            return Err(Refusal::lq(
                "E301",
                format!("{name}@{n} is the zero value: {name} had no commit before that move"),
            ));
        }
        // A commit `gc` pruned stops resolving ([API §8.5]): `undo` cannot reach it.
        if let Some(c) = target.filter(|c| self.pruned.contains(c)) {
            return Err(Refusal::lq(
                "E301",
                format!("{name}@{n}: s{c} was pruned by gc"),
            ));
        }
        let mut reply = Reply::ok(Data::None);
        reply.branch = Some(name.clone());
        reply.rev = Some(r.tip.unwrap_or(0));
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        let mut data = UndoData {
            ref_: name.clone(),
            old: r.tip,
            new: target,
            moved_back: n,
            markers: Vec::new(),
            triage: Vec::new(),
        };
        if ctx.dry {
            reply.outcome = Outcome::Dry;
            reply.data = Data::Undo(Box::new(data));
            return Ok(reply);
        }
        let markers = self.move_ref(&name, target, MoveReason::Undo, &caller.actor);
        let hlc = self.dag.refs[&r.id].moves.last().map_or(0, |m| m.hlc);
        self.moves_back.insert((r.id, hlc), n);
        data.triage = Self::triage(&markers);
        data.markers = markers.clone();
        reply.markers = markers;
        self.record_idem(
            key,
            payload,
            &name,
            None,
            ctx,
            Recorded {
                cmd: "Undo".into(),
                items: vec![ResultItem::RefMove {
                    ref_id: r.id,
                    reason: MoveReason::Undo.code(),
                    old: r.tip,
                    new: target,
                }],
                yields: Vec::new(),
            },
        );
        self.prune_rows();
        reply.data = Data::Undo(Box::new(data));
        Ok(reply)
    }

    /// Whether a ref move lies at or before commit `seq`: a commit's move at that commit's seq; a move no commit carries
    /// (a `RefUpdate`) after the newest commit appended before it, so before `seq` exactly when commit `seq` was
    /// appended after it (append HLCs increase with seq, [API §6.2]) — [F05 §9.2] "Where a `RefUpdate` lies among
    /// commit seqs" and [API §11.11] `OpRestore`. `restore_seq` names a commit seq ([F05 §9.2] field 10), so `op
    /// restore` of the newest seq takes back every ref move made since that commit, an earlier `op restore` included;
    /// a position between two commits needs an op-log position, which format v1 does not carry. No `RefUpdate` lies
    /// at or before seq 0.
    fn at_or_before(&self, m: &RefMove, seq: u64) -> bool {
        match m.reason {
            MoveReason::Commit => m.new.is_some_and(|c| c <= seq),
            _ => self
                .dag
                .commits
                .get(&seq)
                .is_some_and(|c| c.append_hlc > m.hlc),
        }
    }

    /// Each restorable ref at commit `seq` ([API §11.11]): (ref id, live at `seq`, its tip then), by replaying its moves
    /// at or before `seq` in order. A create, a commit, an `undo` or a restoring `op restore` leaves it live at the
    /// move's new tip; a delete or a deleting `op restore` ends it. `main` is live from `init`, with no commit. Staging
    /// and orphans refs are never restored: a staging ref belongs to its merge ([F12 §8]), an orphans ref to replay.
    fn refs_at(&self, seq: u64) -> Vec<(u32, bool, Option<u64>)> {
        let mut plan = Vec::new();
        for r in self.dag.refs.values() {
            if matches!(r.kind, RefKind::Merge | RefKind::Orphans) {
                continue;
            }
            // A ref whose older moves a `Gc` run dropped starts as its newest dropped move left it ([F12 §3.5]); the
            // caller refuses a `seq` older than that move.
            let (mut live, mut tip) = match self.moves_dropped.get(&r.id) {
                Some(m) if m.reason == MoveReason::Delete => (false, None),
                Some(m) => (true, m.new),
                None => (r.id == 0, None),
            };
            for m in r.moves.iter().filter(|m| self.at_or_before(m, seq)) {
                let ends = m.reason == MoveReason::Delete
                    || (m.reason == MoveReason::OpRestore
                        && self.restores.get(&(r.id, m.hlc)).is_some_and(|x| x.1));
                if ends {
                    live = false;
                } else {
                    live = true;
                    tip = m.new;
                }
            }
            plan.push((r.id, live, tip));
        }
        plan
    }

    /// `OpRestore` ([API §11.11]; [AR §5a.5]): every ref back to its value at commit `seq` ([`Store::refs_at`]); a ref
    /// created after it is deleted (its task leases released, LE-008), a ref deleted after it whose row the store still
    /// holds is restored when its name is free, and the markers of every moved ref are recomputed in both directions
    /// (ME-006, ME-013). Deletions apply first, so a name a later ref took is free for the ref restored under it. A
    /// target `gc` pruned stops resolving: E301, nothing moved.
    // spec: [API §11.11] OpRestore
    // rule: ME-006, ME-013, LE-008
    pub(crate) fn op_restore_cmd(&mut self, seq: u64, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let payload = Self::w_payload("OpRestore", &[("seq", Some(Cj::Int(seq as i64)))]);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("undo")
            .map_err(|e| e.finish(None))?;
        if seq > self.commit_seq {
            return Err(Refusal::lq("E301", format!("no commit s{seq}")));
        }
        let plan = self.refs_at(seq);
        // The names that stay taken: refs live now and at `seq`.
        let kept: BTreeSet<&str> = plan
            .iter()
            .filter(|p| p.1 && !self.dag.refs[&p.0].deleted)
            .map(|p| self.dag.refs[&p.0].name.as_str())
            .collect();
        let (mut deletions, mut moves, mut restorations) = (Vec::new(), Vec::new(), Vec::new());
        for &(id, live_at, target) in &plan {
            let r = &self.dag.refs[&id];
            match (r.deleted, live_at) {
                (false, true) if r.tip != target => moves.push((id, target)),
                (false, false) => deletions.push(id),
                (true, true) if !kept.contains(r.name.as_str()) => restorations.push((id, target)),
                _ => {}
            }
        }
        if let Some(c) = moves
            .iter()
            .chain(&restorations)
            .filter_map(|m| m.1)
            .find(|c| self.pruned.contains(c))
        {
            return Err(Refusal::lq(
                "E301",
                format!("s{c} was pruned by gc; op restore cannot reach it"),
            ));
        }
        // [API §11.11]: a `seq` older than a moved ref's oldest held move is E301, nothing moves; after a `Gc` run the
        // moves older than its reflog window are no longer held ([F12 §3.5]). A ref that holds no move at all is
        // judged by its newest dropped move.
        let older = |id: u32| {
            self.moves_dropped.get(&id).is_some_and(|d| {
                let oldest = self.dag.refs[&id].moves.first().unwrap_or(d);
                !self.at_or_before(oldest, seq)
            })
        };
        if let Some(id) = deletions
            .iter()
            .copied()
            .chain(moves.iter().chain(&restorations).map(|m| m.0))
            .find(|id| older(*id))
        {
            return Err(Refusal::lq(
                "E301",
                format!(
                    "s{seq} is older than the oldest held move of {}; op restore cannot reach it",
                    self.dag.refs[&id].name
                ),
            ));
        }
        let mut reply = Reply::ok(Data::None);
        reply.key = ctx.key.clone();
        reply.warnings = caller.warnings.clone();
        let mut data = RestoreData {
            seq,
            moved: Vec::new(),
            markers: Vec::new(),
            triage: Vec::new(),
        };
        if ctx.dry {
            for id in deletions {
                let r = &self.dag.refs[&id];
                data.moved.push((r.name.clone(), r.tip, None));
            }
            for (id, target) in moves {
                let r = &self.dag.refs[&id];
                data.moved.push((r.name.clone(), r.tip, target));
            }
            for (id, target) in restorations {
                data.moved
                    .push((self.dag.refs[&id].name.clone(), None, target));
            }
            reply.outcome = Outcome::Dry;
            reply.data = Data::OpRestore(Box::new(data));
            return Ok(reply);
        }
        let actor = caller.actor.clone();
        let mut items = Vec::new();
        for id in deletions {
            // Created after `seq`: deleted, as `branch -d` deletes (ME-005, LE-008), by a `RefUpdate` of reason 4.
            let r = self.dag.refs[&id].clone();
            let hlc = self.hlc.record(self.env.wall_ms);
            let e = self.markers.ref_deleted(
                &self.dag,
                &r,
                Group::Move(hlc),
                &mut self.hlc,
                self.env.wall_ms,
            );
            data.markers.extend(self.listed(&e));
            self.feed.event(
                self.commit_seq,
                &r.name,
                None,
                None,
                MoveReason::OpRestore.name(),
                "ref",
                r.name.clone(),
                &actor,
                None,
            );
            self.feed_markers(&e, None);
            let released = self.release_branch_leases(&r.name);
            let x = self.dag.refs.get_mut(&id).expect("a ref");
            x.deleted = true;
            x.moves.push(RefMove {
                old: r.tip,
                new: None,
                reason: MoveReason::OpRestore,
                actor: actor.clone(),
                hlc,
            });
            self.restores.insert((id, hlc), (seq, true));
            data.moved.push((r.name.clone(), r.tip, None));
            items.push(ResultItem::RefMove {
                ref_id: id,
                reason: MoveReason::OpRestore.code(),
                old: r.tip,
                new: None,
            });
            items.extend(released.into_iter().map(|l| ResultItem::LeaseEnd {
                lease: l,
                reason: crate::lease::EndReason::BranchDeleted.code(),
            }));
        }
        for (id, target) in moves {
            let r = self.dag.refs[&id].clone();
            let m = self.move_ref(&r.name, target, MoveReason::OpRestore, &actor);
            let hlc = self.dag.refs[&id].moves.last().map_or(0, |m| m.hlc);
            self.restores.insert((id, hlc), (seq, false));
            data.moved.push((r.name.clone(), r.tip, target));
            data.markers.extend(m);
            items.push(ResultItem::RefMove {
                ref_id: id,
                reason: MoveReason::OpRestore.code(),
                old: r.tip,
                new: target,
            });
        }
        for (id, target) in restorations {
            // Deleted after `seq` and still held: restored at its tip then, as a fork at that commit (ME-007, ME-013).
            let hlc = self.hlc.record(self.env.wall_ms);
            let x = self.dag.refs.get_mut(&id).expect("a ref");
            x.deleted = false;
            x.tip = target;
            x.moves.push(RefMove {
                old: None,
                new: target,
                reason: MoveReason::OpRestore,
                actor: actor.clone(),
                hlc,
            });
            let y = x.clone();
            self.restores.insert((id, hlc), (seq, false));
            let st = self.dag.state_at(target, &self.alloc);
            let e = self.markers.fork(
                &self.dag,
                &y,
                &st,
                Group::Move(hlc),
                &mut self.hlc,
                self.env.wall_ms,
            );
            data.markers.extend(self.listed(&e));
            self.feed.event(
                self.commit_seq,
                &y.name,
                target,
                None,
                MoveReason::OpRestore.name(),
                "ref",
                y.name.clone(),
                &actor,
                None,
            );
            self.feed_markers(&e, None);
            data.moved.push((y.name.clone(), None, target));
            items.push(ResultItem::RefMove {
                ref_id: id,
                reason: MoveReason::OpRestore.code(),
                old: None,
                new: target,
            });
        }
        data.triage = Self::triage(&data.markers);
        reply.markers = data.markers.clone();
        if !items.is_empty() {
            self.record_idem(
                key,
                payload,
                &caller.branch,
                None,
                ctx,
                Recorded {
                    cmd: "OpRestore".into(),
                    items,
                    yields: Vec::new(),
                },
            );
            self.prune_rows();
        }
        reply.data = Data::OpRestore(Box::new(data));
        Ok(reply)
    }

    /// `sync --check` of a lane as the `SubagentStart` hook reads it ([AR §7.5]; WH-002): the conflicts, violations and
    /// keys a sync would land; `None` when the lane is not a branch, is up to date, or its sync pair is staged.
    pub fn sync_preview(&self, lane: &str) -> Option<crate::hooks::SyncPreview> {
        let d = self
            .dag
            .live(lane)
            .filter(|r| matches!(r.kind, RefKind::Work | RefKind::Plan))?;
        let m = self.dag.live("main")?;
        if lane == "main"
            || self.dag.live(&staging_name(lane, "main")).is_some()
            || m.tip.is_none_or(|t| self.dag.ancestors(d.tip).contains(&t))
        {
            return None;
        }
        let plan = self.plan_merge(Op::Sync, lane, "main", m.tip, None, None, None);
        let cs = diff(&self.dag.state_at(d.tip, &self.alloc), &plan.m.st);
        Some(crate::hooks::SyncPreview {
            conflicts: plan.m.conflicts.len() as u64,
            violations: plan.m.violations.len() as u64,
            keys: cs.len() as u64,
        })
    }

    /// The data a replay of a merge-family command rebuilds ([API §7.5]).
    pub(crate) fn replay_merge(&self, commit: Option<u64>) -> Option<MergeData> {
        commit.and_then(|c| self.vcs_results.get(&c).cloned())
    }
}

/// The hints of V13 as a merge-family result lists them, `(class, text)`.
fn hints_out(h: &[merge::Hint]) -> Vec<(String, String)> {
    h.iter()
        .map(|h| (h.class.to_string(), h.text.clone()))
        .collect()
}

/// `conflicted_src` of [F19 §10.2] (PR-003), with [F19 §10.3]'s keys: `keys` (at most 10) and `sync`, the step-0
/// sync the refused merge appended on src ([API §11.7] "Sync first with conflict values"), or null when src held the
/// conflicts before the command.
fn conflicted_src(src: &str, keys: Vec<String>, sync: Option<&SyncOut>) -> Refusal {
    let sync = match sync {
        None => Kv::Null,
        Some(x) => Kv::Obj(vec![
            ("commit".into(), x.commit.map_or(Kv::Null, Kv::Commit)),
            ("outcome".into(), Kv::Str(x.outcome.into())),
            (
                "conflicts".into(),
                Kv::List(
                    x.conflicts
                        .iter()
                        .map(|(k, c)| {
                            Kv::Obj(vec![
                                ("key".into(), Kv::Str(k.clone())),
                                ("class".into(), Kv::Str(c.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("violations".into(), Kv::List(Vec::new())),
        ]),
    };
    Refusal::new(
        "conflicted_src",
        6,
        format!(
            "{src} holds {} unresolved conflicts; a merge into main needs none",
            keys.len()
        ),
    )
    .key(
        "keys",
        Kv::List(keys.into_iter().take(10).map(Kv::Str).collect()),
    )
    .key("sync", sync)
}
