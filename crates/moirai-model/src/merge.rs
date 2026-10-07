//! The typed three-way merge of the reference model ([F12 §7]; [RULES/merge-table]; [RULES/link-merge-rules];
//! [60 §4.2] "Merge / sync"): three materialised states — the base b, ours o (dst) and theirs t (src) — and every key
//! decided by the rule tables read as data, first matching row wins ([RULES/merge-table] §2). The procedure is
//! [RULES/merge-table] §10: the re-key (RK rows) on the side that holds a file node the other side ended, then every
//! existence key, then every other key, the hierarchy keys by Kleppmann's moves in (hlc, commit id) order, then the
//! validators of [F13 §5] in their order ([`crate::mvalid`]) unless the merge is a virtual one (VM-3). The typed part
//! ([`typed`]) and the validators ([`Pending::validate`]) are two steps, so that `merge --continue` can overlay the
//! staged resolutions on the recomputed candidate between them ([F12 §9.4] steps 1–3). The three input states are
//! borrowed; only the side the re-key rewrites is copied.
//!
//! Values are compared by their canonical encodings ([F07 §7.3]; [F12 §7.3]): the model's key values are read in
//! canonical form ([`cval`]) — a tombstone holds only its existence, its title and its retained out-edges, a derived title
//! is no key — and two values are equal when their canonical bytes are.

use crate::canon;
use crate::rules::{Row, rules};
use crate::schema::{Schema, Shape, UidDerivation};
use crate::state::{
    Aspect, Changeset, Conflict, EdgeKey, Image, KState, KVal, Key, Node, Side, State,
};
use crate::text3;
use crate::value::{Nid, PathMove, PathVal, Uid, Value};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// The three-way operation a merge realises ([RULES/merge-table] `derived-merges`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// DM-001: `merge SRC --into DST`.
    Merge,
    /// DM-002: `sync`, a merge of `main` into the lane.
    Sync,
    /// DM-004, DM-005: `revert`.
    Revert,
    /// DM-003: `cherry-pick`.
    CherryPick,
    /// DM-009: a merge inside the recursive virtual base.
    Virtual,
}

/// The order key of a hierarchy move: the canonical `hlc` and the id of the commit that made it ([F12 §7.4]).
pub type MoveKey = (u64, [u8; 32]);

/// The moves of one Kleppmann step: each moved node with its (parent, order), ascending by node.
pub type Moves = Vec<(Nid, Option<KVal>)>;

/// One step of Kleppmann's replay (RS-007; [F12 §7.4] row "Kleppmann steps"; VM-7): one commit of A(side) \ A(B),
/// holding its step keys ([`crate::dag::Dag::step_keys`]: the hierarchy entries of its canonical net changeset against
/// its first parent, and a two-parent commit's second-parent keys), each with its value in that commit's state; or a
/// revert's or cherry-pick's one src step, C's, valued in src's state, less the keys a later commit of dst moved.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// The commit's (hlc, id).
    pub key: MoveKey,
    /// Each moved node with its (parent, order) in the commit's state, ascending by node.
    pub moves: Moves,
}

impl Step {
    /// The step of a commit with order key `key` and net changeset `cs`: its hierarchy entries with their values after
    /// the commit; `None` when it moved nothing.
    pub fn of(key: MoveKey, cs: &Changeset) -> Option<Step> {
        let moves: Moves = cs
            .iter()
            .filter_map(|(k, (_, after))| match k {
                Key::Node(n, Aspect::Hierarchy) => Some((*n, flat(after))),
                _ => None,
            })
            .collect();
        (!moves.is_empty()).then_some(Step { key, moves })
    }
}

/// A structural violation ([F12 §7.9]; [F19 §12.2]), recorded as one `Violation` op of a staged commit.
#[derive(Clone, PartialEq, Debug)]
pub struct Violation {
    /// The class name.
    pub class: &'static str,
    /// Its code ([F19 §12.2]).
    pub code: u8,
    /// The key ([F12 §7.9]); `None` for `-`.
    pub key: Option<Key>,
    /// `description`.
    pub description: String,
    /// `suggested`.
    pub suggested: String,
}

/// A hint ([F19 §12.3]): a log line, never stored.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hint {
    /// The class name.
    pub class: &'static str,
    /// The text after `hint[<name>]: `.
    pub text: String,
}

/// The code of a structural class ([F19 §12.2]).
pub fn violation_code(class: &str) -> u8 {
    match class {
        "HierarchyCycle" => 64,
        "Cycle" => 65,
        "DanglingEdge" => 66,
        "DepthExceeded" => 67,
        "Cardinality" => 68,
        "SchemaConflict" => 69,
        "QueryInvalid" => 70,
        "QueryCycle" => 71,
        "PlanMask" => 72,
        "RemovedTextNotInBase" => 73,
        "IdCollision" => 74,
        "ImageParse" => 75,
        "NotFound" => 76,
        "TombstoneRemoved" => 77,
        other => panic!("{other} is not a structural class"),
    }
}

/// The `#N`s a merge gives uids new to the store — a re-keyed file node's uid′ ([RULES/link-merge-rules] RK-008) — kept
/// from the top of the id space down until the landing renumbers them in ascending uid order ([API §9.6] item 2).
#[derive(Clone, Debug, Default)]
pub struct Fresh {
    /// uid → provisional `#N`.
    pub by_uid: BTreeMap<Uid, Nid>,
    /// provisional `#N` → uid.
    pub uid: BTreeMap<Nid, Uid>,
}

impl Fresh {
    /// The provisional `#N` of a new uid.
    pub fn nid(&mut self, u: Uid) -> Nid {
        if let Some(n) = self.by_uid.get(&u) {
            return *n;
        }
        let n = Nid(u32::MAX - self.by_uid.len() as u32);
        self.by_uid.insert(u, n);
        self.uid.insert(n, u);
        n
    }
}

/// Everything a merge reads besides its three states ([F12 §7.1]).
pub struct Ctx<'a> {
    /// The operation.
    pub op: Op,
    /// dst is the ref `main` (never inside a virtual merge, VB-007).
    pub dst_main: bool,
    /// dst is a `plan/*` ref (V12).
    pub dst_plan: bool,
    /// `--policy` (`delete-wins` or `resurrect`, AP-004, AP-005).
    pub policy: Option<&'a str>,
    /// `merge.policy.<kind>`: `none`, `ours` or `theirs` (AP-001 to AP-003).
    pub auto: &'a BTreeMap<String, String>,
    /// Where the hierarchy keys of a merge, a `sync` or a virtual merge start (RS-007; [AR §11] OQ-A-11). A revert or a
    /// cherry-pick reads none of it: its replay starts from o.
    pub start: Start<'a>,
    /// The Kleppmann steps of ours' and theirs' commits since the replay start R, each ascending by key (RS-007, VM-7).
    /// For a revert or a cherry-pick, dst has no step (ours' are not read) and src's is C's one step (RS-007;
    /// [RULES/merge-table] open point 35 case (ii)).
    pub moves: [&'a [Step]; 2],
    /// The store's uid of a `#N` (a node none of the three states holds: a tombstone reference).
    pub uid: &'a dyn Fn(Nid) -> Uid,
    /// The store's `#N` of a uid it knows (`UIDX`).
    pub nid: &'a dyn Fn(Uid) -> Option<Nid>,
    /// The origin time of a side's hierarchy value (side 1: dst, 2: src), when the caller knows the sides' histories:
    /// the (hlc, commit id) of the commit that produced the value ([`crate::dag::Dag::origin`]); read only by the
    /// evaluation harness's candidate `threeway` (`merge/cand.rs`, test builds), `None` for a hand-built merge.
    pub origin: Option<Origin<'a>>,
}

/// The origin time of a side's hierarchy value of a node ([`Ctx::origin`]): side 1 is dst, 2 is src.
pub type Origin<'a> = &'a dyn Fn(usize, Nid, &Option<KVal>) -> MoveKey;

/// Where RS-007 starts the hierarchy keys of a merge, a `sync` or a virtual merge ([RULES/merge-table] RS-007;
/// [F12 §7.4] row "Kleppmann steps"; [AR §11] OQ-A-11 11.1).
#[derive(Clone, Copy, Debug)]
pub enum Start<'a> {
    /// The replay starts from b's (parent, order): the replay start R is the base's own commit, as in every
    /// hand-built case, whose sides' steps all start at b.
    Base,
    /// The replay starts from state(R), the replay start of [`crate::dag::Dag::replay_start`] (state(ε) when there is
    /// none).
    State(&'a State),
    /// A one-sided merge, whose base is state(tip(dst)) (11.1 (A)): every hierarchy key takes src's value, with no
    /// replay.
    TakeSrc,
}

/// A merge's result: the candidate state and what it records ([RULES/merge-table] PR-009 to PR-011).
#[derive(Clone, Debug)]
pub struct Merged {
    /// The candidate state on dst's view.
    pub st: State,
    /// The value conflicts landed, (key, class), in emission order ([F13 §5] VO-2).
    pub conflicts: Vec<(Key, String)>,
    /// The structural violations, in emission order.
    pub violations: Vec<Violation>,
    /// The hints.
    pub hints: Vec<Hint>,
    /// The row of the tables that decided each key the three states do not agree on (the fixtures' `merge-rows`).
    pub rows: BTreeMap<Key, String>,
}

/// Whether a field of a node is a canonical key ([F07 §6.2]).
fn hashed(schema: &Schema, x: &Node, f: &str) -> bool {
    if f == "title" && x.live() && schema.kind(&x.kind).is_some_and(|k| k.title_derived) {
        return false;
    }
    !schema
        .field(&x.kind, f)
        .is_some_and(|fi| fi.class == "none" || fi.class == "derived")
}

/// A key's value in canonical form on a state ([F07 §6.3]–§6.5): a tombstone holds its existence, its title and its
/// retained out-edges only; a derived title and a field of merge class `none` or `derived` hold nothing.
pub fn cval(st: &State, n: Nid, a: &Aspect) -> KState {
    let Some(x) = st.nodes.get(&n) else {
        return KState::ABSENT;
    };
    if let Some(c) = x.conflicts.get(a) {
        return KState::Conflict(Box::new(c.clone()));
    }
    let plain = || KState::Plain(x.get(&st.schema, a));
    match a {
        Aspect::Existence => plain(),
        _ if !x.live() => match a {
            Aspect::Field(f) if f == "title" => plain(),
            Aspect::Edge(_) => plain(),
            _ => KState::ABSENT,
        },
        Aspect::Field(f) | Aspect::Counter(f) if !hashed(&st.schema, x, f) => KState::ABSENT,
        // A value equal to the field's default in this state's schema is absent ([F07 §6.3]), also when it was written
        // under an older default.
        Aspect::Field(f)
            if x.fields
                .get(f)
                .is_some_and(|v| st.schema.default_of(&x.kind, f).as_ref() == Some(v)) =>
        {
            KState::ABSENT
        }
        _ => plain(),
    }
}

/// The canonical keys of a node other than its existence ([F07 §6.1]).
pub fn aspects(st: &State, n: Nid) -> BTreeSet<Aspect> {
    let mut s = BTreeSet::new();
    let Some(x) = st.nodes.get(&n) else { return s };
    if x.live() {
        for a in x.aspects(x.kind == "artifact") {
            match &a {
                Aspect::Field(f) | Aspect::Counter(f) if !hashed(&st.schema, x, f) => {}
                _ => {
                    s.insert(a);
                }
            }
        }
    } else {
        if x.fields.contains_key("title") {
            s.insert(Aspect::Field("title".into()));
        }
        for k in x.out.keys() {
            s.insert(Aspect::Edge(k.clone()));
        }
    }
    s.extend(x.conflicts.keys().cloned());
    s.remove(&Aspect::Existence);
    s
}

/// The provisional value of a key state ([F12 §6.3]): a plain value itself; of a conflict value, the side `prov` names
/// on an existence key and otherwise `ours`, or `theirs` when `ours` is absent. `flat` of [F12 §6.4].
pub fn flat(v: &KState) -> Option<KVal> {
    match v {
        KState::Plain(p) => p.clone(),
        KState::Conflict(c) => match (c.prov, &c.ours, &c.theirs) {
            (Some(Side::Theirs), _, t) => t.clone(),
            (Some(Side::Ours), o, _) => o.clone(),
            (None, None, t) => t.clone(),
            (None, o, _) => o.clone(),
        },
    }
}

/// The node image that goes with [`flat`]'s value of an existence key state.
fn flat_image(v: &KState, own: Option<Image>) -> Option<Image> {
    match v {
        KState::Plain(_) => own,
        KState::Conflict(c) => match (c.prov, &c.ours) {
            (Some(Side::Theirs), _) | (None, None) => c.images[2].clone(),
            _ => c.images[1].clone(),
        },
    }
}

fn is_live(v: &Option<KVal>) -> bool {
    matches!(v, Some(KVal::Live(_)))
}

fn is_deleted(v: &Option<KVal>) -> bool {
    matches!(v, Some(KVal::Deleted { .. }))
}

pub use crate::r4::uid::{uid_file, uid_root};

/// The disposition of a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Disp {
    Clean,
    Value,
    Structural,
    Hint,
    None,
}

fn disp(r: &Row) -> Disp {
    match r.tok("disposition") {
        "clean" => Disp::Clean,
        "value" => Disp::Value,
        "structural" => Disp::Structural,
        "hint" => Disp::Hint,
        "none" => Disp::None,
        "gap" => panic!("SpecGap({})", r.id),
        o => panic!("disposition {o} of {} has no implementation", r.id),
    }
}

/// The rows of a merge class in table order: [RULES/merge-table] `merge-rules`, or [RULES/link-merge-rules]
/// `link-merge-rules` for the R4 classes.
fn class_rows(class: &str) -> Vec<&'static Row> {
    let r = rules();
    let file = r
        .table("merge-classes")
        .rows
        .iter()
        .find(|x| x.tok("class") == class)
        .unwrap_or_else(|| panic!("merge class {class} is not in merge-classes"))
        .tok("rules_file");
    let table = if file == "link-merge-rules" {
        "link-merge-rules"
    } else {
        "merge-rules"
    };
    r.table(table)
        .rows
        .iter()
        .filter(|x| x.tok("class") == class)
        .collect()
}

/// Which state a node of a fixed uid is copied from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum From {
    Base,
    Ours,
    Theirs,
}

/// How a uid's keys are decided after its existence key ([RULES/merge-table] PR-007).
#[derive(Clone, Debug)]
enum Mode {
    /// The node is absent from the result.
    Absent,
    /// The node is copied whole from a side, with this conflict value on its existence key (RS-008, RS-010, RS-015; a
    /// tombstone one side left while the other only read the node; an automatic side; a staged `IdCollision`).
    Fixed(From, Option<Box<Conflict>>),
    /// The node is a tombstone on both sides: copied from the side whose existence value won, its title and retained
    /// out-edges merged key by key ([AR §3.4] I39′).
    Tomb(From),
    /// The node is live and every other key is merged by its own class.
    Merged,
}

/// One key's decision.
struct Decided {
    value: KState,
    row: String,
    violation: Option<&'static str>,
    /// The side whose value the key takes as that side holds it: ours for `take-o` and `stage-take-o`, theirs for
    /// `take-t`, the side of an automatic policy (AP-002, AP-003); `None` for a value the row computes.
    taken: Option<From>,
}

/// The merge engine over three states: b, o and t borrowed, the side the re-key rewrites copied on its first change.
struct Engine<'s, 'c> {
    s: [Cow<'s, State>; 3],
    cx: &'c Ctx<'c>,
    fresh: &'c mut Fresh,
    out: Merged,
    /// The pre-composition paths of each node, for LR-004.
    precomp: BTreeMap<Nid, Vec<Value>>,
    /// The nodes whose hierarchy move Kleppmann skipped.
    skipped: BTreeSet<Nid>,
    /// Per state, the uids the re-key moved on it: U's `#N` → uid′'s `#N` (RK-005).
    rekeyed: [BTreeMap<Nid, Nid>; 3],
}

impl Engine<'_, '_> {
    fn st(&self, i: usize) -> &State {
        &self.s[i]
    }

    fn uid(&self, n: Nid) -> Uid {
        for s in &self.s {
            if let Some(x) = s.nodes.get(&n) {
                return x.uid;
            }
        }
        if let Some(u) = self.fresh.uid.get(&n) {
            return *u;
        }
        (self.cx.uid)(n)
    }

    /// The `#N` of a uid any of the three states holds, the store knows, or the merge allocated.
    fn nid_of(&mut self, u: Uid) -> Nid {
        for s in &self.s {
            if let Some((n, _)) = s.nodes.iter().find(|(_, x)| x.uid == u) {
                return *n;
            }
        }
        if let Some(n) = (self.cx.nid)(u) {
            return n;
        }
        self.fresh.nid(u)
    }

    /// The canonical bytes of a key state, for equality ([F07 §7.3]).
    fn enc(&self, k: &Key, v: &KState) -> Vec<u8> {
        let uid = |n: Nid| self.uid(n);
        match k {
            Key::Node(_, a @ Aspect::Counter(_)) => match v {
                KState::Plain(Some(KVal::Value(Value::Counter(t)))) => t.to_le_bytes().to_vec(),
                KState::Plain(None) => 0i64.to_le_bytes().to_vec(),
                other => canon::cstate(canon::kstate_cval(a, other, &uid).as_ref()),
            },
            Key::Node(_, a) => canon::cstate(canon::kstate_cval(a, v, &uid).as_ref()),
            Key::Schema(_) => {
                let side = |x: &Option<KVal>| match x {
                    Some(KVal::Item(i)) => canon::item_value(i, &uid),
                    None => vec![0],
                    Some(o) => panic!("a schema key holds {o:?}"),
                };
                match v {
                    KState::Plain(p) => {
                        let mut b = vec![0];
                        b.extend(side(p));
                        b
                    }
                    KState::Conflict(c) => {
                        let mut b = vec![1];
                        crate::value::lp(&mut b, c.class.as_bytes());
                        b.extend(side(&c.base));
                        b.extend(side(&c.ours));
                        b.extend(side(&c.theirs));
                        b
                    }
                }
            }
        }
    }

    fn eq(&self, k: &Key, a: &KState, b: &KState) -> bool {
        a == b || self.enc(k, a) == self.enc(k, b)
    }

    /// [F12 §5.4]'s ≈: §7.3's equality, widened for two conflict values that have the same class and base and
    /// exchanged `ours` and `theirs` (on an existence key, the opposite `prov`, their node images exchanged with them):
    /// a side that left its own merge's conflict untouched holds it in its own orientation (RVB-1 to RVB-4; VBC-3).
    fn approx(&self, k: &Key, a: &KState, b: &KState) -> bool {
        if self.eq(k, a, b) {
            return true;
        }
        let (KState::Conflict(x), KState::Conflict(y)) = (a, b) else {
            return false;
        };
        if x.class != y.class {
            return false;
        }
        let exchanged = Conflict {
            class: y.class.clone(),
            base: y.base.clone(),
            ours: y.theirs.clone(),
            theirs: y.ours.clone(),
            prov: y.prov.map(|p| match p {
                Side::Ours => Side::Theirs,
                Side::Theirs => Side::Ours,
            }),
            images: [
                y.images[0].clone(),
                y.images[2].clone(),
                y.images[1].clone(),
            ],
        };
        self.eq(k, a, &KState::Conflict(Box::new(exchanged)))
    }

    fn val(&self, i: usize, k: &Key) -> KState {
        match k {
            Key::Node(n, a) => cval(self.st(i), *n, a),
            Key::Schema(s) => self.st(i).kstate(&Key::Schema(s.clone())),
        }
    }

    /// The kind of a node on the first state that holds it (ours, theirs, base).
    fn kind(&self, n: Nid) -> Option<String> {
        [1, 2, 0]
            .iter()
            .find_map(|i| self.st(*i).nodes.get(&n).map(|x| x.kind.clone()))
    }

    /// The merge class of a key ([RULES/merge-table] §3, §8): from the schema's field row, the edge kind, or the node's
    /// uid derivation for existence.
    fn class_of(&self, k: &Key) -> String {
        let Key::Node(n, a) = k else {
            return match k {
                Key::Schema(crate::schema::ItemKey::Query(_)) => "query".into(),
                _ => "schema-item".into(),
            };
        };
        let kind = self.kind(*n).unwrap_or_default();
        let schema = &self.st(1).schema;
        match a {
            Aspect::Existence => {
                let root_node = kind == "area"
                    && [1, 2, 0].iter().any(|i| {
                        self.st(*i)
                            .nodes
                            .get(n)
                            .is_some_and(|x| x.fields.contains_key("root"))
                    });
                let file_key = schema
                    .kind(&kind)
                    .or_else(|| self.st(2).schema.kind(&kind))
                    .is_some_and(|k| k.uid == UidDerivation::FileKey);
                if file_key || root_node {
                    "derived-existence".into()
                } else {
                    "existence".into()
                }
            }
            Aspect::Status => "status".into(),
            Aspect::Hierarchy => "hierarchy".into(),
            Aspect::Observation => "observation".into(),
            Aspect::Counter(_) => "counter".into(),
            Aspect::Edge(e) if e.kind == "at" => "anchor".into(),
            Aspect::Edge(_) => "edge".into(),
            Aspect::Body => {
                let section = [1, 2, 0].iter().any(|i| {
                    self.st(*i).nodes.get(n).is_some_and(|x| {
                        x.kind == "doc"
                            && matches!(x.fields.get("doc_kind"), Some(Value::Enum(d)) if d == "section")
                    })
                });
                if section {
                    "section-text".into()
                } else {
                    "text".into()
                }
            }
            Aspect::Field(f) => {
                let fi = schema
                    .field(&kind, f)
                    .or_else(|| self.st(2).schema.field(&kind, f))
                    .or_else(|| self.st(0).schema.field(&kind, f));
                // An `area` whose `root` field is present is a root node: its `root` is an identity input (FC-137).
                if f == "root" && kind == "area" {
                    return "identity".into();
                }
                fi.map_or("scalar", |fi| fi.class).to_string()
            }
        }
    }

    // ----------------------------------------------------------------------------------------------------------
    // Cases
    // ----------------------------------------------------------------------------------------------------------

    fn case(&self, case: &str, k: &Key, b: &KState, o: &KState, t: &KState) -> bool {
        // Over a conflict-valued base the `conflicted-key` rows compare by ≈ ([F12 §5.4]; MR-001, MR-003, MR-004).
        let cmp = |x: &KState, y: &KState| {
            if matches!(b, KState::Conflict(_)) {
                self.approx(k, x, y)
            } else {
                self.eq(k, x, y)
            }
        };
        let same = cmp(o, t);
        let ob = cmp(o, b);
        let tb = cmp(t, b);
        let both = !ob && !tb && !same;
        let pb = || flat(b);
        let (po, pt) = (flat(o), flat(t));
        match case {
            "same" => same,
            "ours-only" => !ob && tb,
            "theirs-only" => !tb && ob,
            "both" => both,
            "any" => true,
            "base-conflicted" => matches!(b, KState::Conflict(_)) && both,
            "both-forward-comparable" => both && self.forward_comparable(k, b, o, t),
            "dst-main-changed" => self.cx.dst_main && self.cx.op != Op::Virtual && !same,
            "dst-main-owner-involved" => {
                self.cx.dst_main
                    && self.cx.op != Op::Virtual
                    && !same
                    && [pb(), po.clone(), pt.clone()]
                        .iter()
                        .any(|v| matches!(v, Some(KVal::Value(Value::Enum(e))) if e == "owner"))
            }
            "both-owner-involved" => {
                both && [pb(), po, pt]
                    .iter()
                    .any(|v| matches!(v, Some(KVal::Value(Value::Enum(e))) if e == "owner"))
            }
            "both-diff3-clean" => both && self.diff3(b, o, t).is_some(),
            "both-guard-fail" => {
                both && self.diff3(b, o, t).is_some_and(|r| {
                    let (bt, ot, tt) = (text_of(b), text_of(o), text_of(t));
                    text3::guard_fails(&bt, &ot, &tt, &r)
                })
            }
            "kleppmann-skipped" => matches!(k, Key::Node(n, _) if self.skipped.contains(n)),
            "deleted-vs-modified" => {
                let Key::Node(n, _) = k else { return false };
                is_live(&pb())
                    && ((is_deleted(&po) && is_live(&pt) && self.modified(2, *n))
                        || (is_deleted(&pt) && is_live(&po) && self.modified(1, *n)))
            }
            "both-created" => pb().is_none() && is_live(&po) && is_live(&pt),
            "both-present" => both && po.is_some() && pt.is_some(),
            "both-strengthen" => {
                both && (strengthening(k, &pb(), &po) || strengthening(k, &pb(), &pt))
            }
            "dropped-vs-modified" => both && (po.is_none() != pt.is_none()),
            "both-same-ast" => both && self.same_ast(&po, &pt),
            // R4 ([RULES/link-merge-rules] §3).
            "both-same-path" => {
                both && matches!((&po, &pt), (Some(KVal::Observation(x)), Some(KVal::Observation(y))) if x[0] == y[0])
            }
            "both-compose" => both && self.composer(k, b, o, t).is_some(),
            "created-vs-dead" | "root-created-vs-dead" => {
                let Key::Node(n, _) = k else { return false };
                let rootish = case == "root-created-vs-dead";
                pb().is_none() && self.created_vs_dead(*n, rootish).is_some()
            }
            "created-both-live" => {
                let Key::Node(n, _) = k else { return false };
                pb().is_none()
                    && [1, 2].iter().all(|i| {
                        self.st(*i)
                            .nodes
                            .get(n)
                            .is_some_and(|x| x.live() && x.status != "removed")
                    })
            }
            other => panic!("case {other} has no implementation"),
        }
    }

    /// CS-014's "modified": side `i` changed a field, status, counter, body or hierarchy key of the uid, or an out-edge
    /// key whose source is the uid, since the base.
    fn modified(&self, i: usize, n: Nid) -> bool {
        let mut keys = aspects(self.st(0), n);
        keys.extend(aspects(self.st(i), n));
        keys.insert(Aspect::Hierarchy);
        keys.insert(Aspect::Status);
        keys.into_iter().any(|a| {
            let k = Key::Node(n, a);
            !self.eq(&k, &self.val(0, &k), &self.val(i, &k))
        })
    }

    /// CS-007 over the kind's merge order (the transitive closure of the SL `covers` relation, from the schema's
    /// status values): neither o nor t a side state, b absent or not a side state, b < o and b < t, o and t with
    /// different statuses that are comparable.
    fn forward_comparable(&self, k: &Key, b: &KState, o: &KState, t: &KState) -> bool {
        let Key::Node(n, _) = k else { return false };
        let kind = self.kind(*n).unwrap_or_default();
        let schema = &self.st(1).schema;
        let st_of = |v: &KState| -> Option<String> {
            match flat(v) {
                Some(KVal::Status { status, .. }) => Some(status),
                _ => None,
            }
        };
        let side = |s: &str| schema.value(&kind, "status", s).is_some_and(|e| e.side);
        let below = |lo: Option<&str>, hi: &str| -> bool {
            // `absent` lies below every non-side state.
            let Some(lo) = lo else { return !side(hi) };
            let mut stack = vec![hi.to_string()];
            let mut seen = BTreeSet::new();
            while let Some(x) = stack.pop() {
                if !seen.insert(x.clone()) {
                    continue;
                }
                if let Some(e) = schema.value(&kind, "status", &x) {
                    for c in &e.covers {
                        if c == lo {
                            return true;
                        }
                        stack.push(c.clone());
                    }
                }
            }
            false
        };
        let (Some(so), Some(stt)) = (st_of(o), st_of(t)) else {
            return false;
        };
        let sb = st_of(b);
        if side(&so) || side(&stt) || sb.as_deref().is_some_and(side) {
            return false;
        }
        so != stt
            && below(sb.as_deref(), &so)
            && below(sb.as_deref(), &stt)
            && (below(Some(&so), &stt) || below(Some(&stt), &so))
    }

    fn diff3(&self, b: &KState, o: &KState, t: &KState) -> Option<String> {
        let r = text3::diff3(&text_of(b), &text_of(o), &text_of(t))?;
        (r.len() <= text3::MAX_TEXT).then_some(r)
    }

    /// CS-019: both definitions bind and their canonical-AST encodings are equal ([50 §4.4]).
    fn same_ast(&self, o: &Option<KVal>, t: &Option<KVal>) -> bool {
        let ast = |v: &Option<KVal>, i: usize| -> Option<Vec<u8>> {
            let Some(KVal::Item(crate::schema::Item::Query(q))) = v else {
                return None;
            };
            let lq = crate::lqh::lq_schema(&self.st(i).schema);
            let nq = crate::lq::catalog::named_query_checked(&q.name, &q.text, &lq).ok()?;
            Some(crate::lq::cast::encode(crate::lq::cast::Root::Define(
                &nq.cast,
            )))
        };
        match (ast(o, 1), ast(t, 2)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    // ----------------------------------------------------------------------------------------------------------
    // Rows
    // ----------------------------------------------------------------------------------------------------------

    /// Decides one key by its class's rows, the `conflicted-key` rows first when b, o or t holds a conflict value
    /// ([RULES/merge-table] §2 "Evaluation"; PR-008).
    fn decide(&mut self, k: &Key, b: &KState, o: &KState, t: &KState) -> Decided {
        let conflicted = [b, o, t].iter().any(|v| matches!(v, KState::Conflict(_)));
        let class = if conflicted {
            "conflicted-key".to_string()
        } else {
            self.class_of(k)
        };
        for r in class_rows(&class) {
            if !self.case(r.tok("case"), k, b, o, t) {
                continue;
            }
            let d = disp(r);
            let value = self.result(r, k, b, o, t);
            let conflict = r.tok("conflict");
            let violation = match d {
                Disp::Structural if self.cx.op != Op::Virtual => Some(static_class(conflict)),
                _ => None,
            };
            // AP-002, AP-003: an automatic side for a value conflict on a key of the node's kind (never inside a
            // virtual merge, VB-019).
            if d == Disp::Value
                && let Some(side) = self.auto_side(k)
            {
                let v = if side == Side::Ours {
                    o.clone()
                } else {
                    t.clone()
                };
                return Decided {
                    value: v,
                    row: format!(
                        "{}+{}",
                        r.id,
                        if side == Side::Ours {
                            "AP-002"
                        } else {
                            "AP-003"
                        }
                    ),
                    violation: None,
                    taken: Some(if side == Side::Ours {
                        From::Ours
                    } else {
                        From::Theirs
                    }),
                };
            }
            let taken = match r.tok("result") {
                "take-o" | "stage-take-o" => Some(From::Ours),
                "take-t" => Some(From::Theirs),
                _ => None,
            };
            return Decided {
                value,
                row: r.id.clone(),
                violation,
                taken,
            };
        }
        panic!("class {class} has no row for {k:?}: its rows are not exhaustive")
    }

    /// The automatic side `merge.policy.<kind>` names for the key's node (AP-002, AP-003).
    fn auto_side(&self, k: &Key) -> Option<Side> {
        if self.cx.op == Op::Virtual {
            return None;
        }
        let Key::Node(n, _) = k else { return None };
        let kind = self.kind(*n)?;
        match auto_policy(self.cx.auto, &kind) {
            Some("ours") => Some(Side::Ours),
            Some("theirs") => Some(Side::Theirs),
            _ => None,
        }
    }

    fn result(&mut self, r: &Row, k: &Key, b: &KState, o: &KState, t: &KState) -> KState {
        let conflict_cell = r.tok("conflict").to_string();
        match r.tok("result") {
            "take-o" | "stage-take-o" => o.clone(),
            "take-t" => t.clone(),
            "join" => self.join(k, o, t),
            "sum" => {
                let tot = |v: &KState| match flat(v) {
                    Some(KVal::Value(Value::Counter(c))) => i128::from(c),
                    _ => 0,
                };
                let s = tot(o) + tot(t) - tot(b);
                let s = i64::try_from(s).unwrap_or_else(|_| {
                    panic!(
                        "RS-004: the counter sum of {k:?} overflows i64 (a specification finding)"
                    )
                });
                KState::Plain((s != 0).then_some(KVal::Value(Value::Counter(s))))
            }
            "union3" => KState::Plain(union3(&flat(b), &flat(o), &flat(t))),
            "union3-aliases" => {
                let mut v = union3(&flat(b), &flat(o), &flat(t));
                if let Key::Node(n, _) = k
                    && let Some(extra) = self.precomp.get(n)
                {
                    let mut items: Vec<Value> = v
                        .map(|x| match x {
                            KVal::Value(Value::Set(s)) => s,
                            _ => Vec::new(),
                        })
                        .unwrap_or_default();
                    items.extend(extra.iter().cloned());
                    v = Value::set(items).map(KVal::Value);
                }
                KState::Plain(v)
            }
            "union3-compose" => KState::Plain(self.union3_compose(k, b, o, t)),
            "diff3" | "stage-diff3" => {
                let r = self
                    .diff3(b, o, t)
                    .expect("the row's case holds a clean diff3");
                KState::Plain(text_value(k, r))
            }
            "kleppmann" => o.clone(),
            "policy" => self.policy(k, b, o, t, &conflict_cell),
            "conflict-value" => {
                KState::Conflict(Box::new(self.conflict(k, &conflict_cell, flat(b), b, o, t)))
            }
            "conflict-as-class" => {
                let KState::Conflict(bc) = b else {
                    panic!("RS-010 needs a conflict-valued base")
                };
                let inner = KState::Plain(bc.base.clone());
                let class = self
                    .value_class(k, &inner, o, t)
                    .unwrap_or_else(|| bc.class.clone());
                let mut c = self.conflict(k, &class, bc.base.clone(), &inner, o, t);
                c.images[0] = bc.images[0].clone();
                KState::Conflict(Box::new(c))
            }
            "conflict-plain-base" => {
                let class = self
                    .value_class(k, b, o, t)
                    .unwrap_or_else(|| match (o, t) {
                        (KState::Conflict(c), _) | (KState::Plain(_), KState::Conflict(c)) => {
                            c.class.clone()
                        }
                        _ => panic!("RS-015 needs a conflict-valued side"),
                    });
                KState::Conflict(Box::new(self.conflict(k, &class, flat(b), b, o, t)))
            }
            "keep-defined" => {
                let mut c = self.conflict(k, &conflict_cell, flat(b), b, o, t);
                c.prov = None;
                KState::Conflict(Box::new(c))
            }
            "compose" => {
                let (from, rewritten) = self.composer(k, b, o, t).expect("LC-002 holds");
                let _ = from;
                KState::Plain(Some(rewritten))
            }
            "equal-existence" => o.clone(),
            "rekey" => o.clone(),
            "refuse-internal" => panic!(
                "internal error: {} failed ({k:?}: a root node dead on one side, which [F08 §11.3] makes unreachable)",
                r.id
            ),
            "gap" => panic!("SpecGap({})", r.id),
            other => panic!("result {other} of {} has no implementation", r.id),
        }
    }

    /// RS-010 and RS-015's class: the key's own rows evaluated on (b, flat(o), flat(t)), only rows whose disposition is
    /// `value`; the first match's `conflict` cell.
    fn value_class(&mut self, k: &Key, b: &KState, o: &KState, t: &KState) -> Option<String> {
        let (fb, fo, ft) = (
            KState::Plain(flat(b)),
            KState::Plain(flat(o)),
            KState::Plain(flat(t)),
        );
        let class = self.class_of(k);
        for r in class_rows(&class) {
            if r.tok("disposition") != "value" {
                continue;
            }
            if self.case(r.tok("case"), k, &fb, &fo, &ft) {
                return Some(r.tok("conflict").to_string());
            }
        }
        None
    }

    /// A conflict value {class, base, flat(o), flat(t)} with the images of its live existence sides and, on an
    /// existence key, the provisional side ([F12 §6.3]; RS-008, RS-010, RS-015).
    fn conflict(
        &self,
        k: &Key,
        class: &str,
        base: Option<KVal>,
        b: &KState,
        o: &KState,
        t: &KState,
    ) -> Conflict {
        let (fo, ft) = (flat(o), flat(t));
        let mut c = Conflict {
            class: class.to_string(),
            base,
            ours: fo.clone(),
            theirs: ft.clone(),
            prov: None,
            images: [None, None, None],
        };
        if let Key::Node(n, Aspect::Existence) = k {
            let image = |i: usize| -> Option<Image> {
                let x = self.st(i).nodes.get(n)?;
                x.live().then(|| canon::node_image(&self.st(i).schema, x))
            };
            c.images = [
                is_live(&c.base).then(|| flat_image(b, image(0))).flatten(),
                is_live(&fo).then(|| flat_image(o, image(1))).flatten(),
                is_live(&ft).then(|| flat_image(t, image(2))).flatten(),
            ];
            c.prov = Some(self.prov(*n, &fo, &ft));
        }
        c
    }

    /// The provisional side of a `DeleteVsModify` (RS-008): `delete-wins` the deleting side, `resurrect` the modifying
    /// side, `none` dst's; `--policy` overrides the kind's EP row outside a virtual merge (AP-004, AP-005, VB-019).
    /// When neither or both sides are live, dst's (RS-010's "o's values otherwise").
    fn prov(&self, n: Nid, fo: &Option<KVal>, ft: &Option<KVal>) -> Side {
        if is_live(fo) == is_live(ft) {
            return Side::Ours;
        }
        let kind = self.kind(n).unwrap_or_default();
        let auto =
            auto_policy(self.cx.auto, &kind).filter(|p| *p == "delete-wins" || *p == "resurrect");
        let policy = match (self.cx.op, self.cx.policy, auto) {
            (Op::Virtual, _, _) => self.existence_policy(n),
            (_, Some(p), _) => p.to_string(),
            (_, None, Some(p)) => p.to_string(),
            (_, None, None) => self.existence_policy(n),
        };
        let deleting = if is_live(fo) {
            Side::Theirs
        } else {
            Side::Ours
        };
        match policy.as_str() {
            "delete-wins" => deleting,
            "resurrect" => {
                if deleting == Side::Ours {
                    Side::Theirs
                } else {
                    Side::Ours
                }
            }
            _ => Side::Ours,
        }
    }

    /// The kind's existence policy: the most specific EP row (`area/root` over `area`), a project kind's schema row
    /// otherwise, from dst's schema ([RULES/merge-table] EP rows; VB-019).
    fn existence_policy(&self, n: Nid) -> String {
        let kind = self.kind(n).unwrap_or_default();
        let root_node = [1, 2, 0].iter().any(|i| {
            self.st(*i)
                .nodes
                .get(&n)
                .is_some_and(|x| x.kind == "area" && x.fields.contains_key("root"))
        });
        let t = rules().table("existence-policy");
        let want = if root_node {
            format!("{kind}/root")
        } else {
            kind.clone()
        };
        t.rows
            .iter()
            .find(|r| r.tok("kind") == want)
            .or_else(|| t.rows.iter().find(|r| r.tok("kind") == kind))
            .map(|r| r.tok("policy").to_string())
            .or_else(|| {
                self.st(1)
                    .schema
                    .kind(&kind)
                    .map(|k| k.existence_policy.to_string())
            })
            .unwrap_or_else(|| "none".into())
    }

    /// RS-008 `policy`: the `DeleteVsModify` conflict value; the provisional state is fixed by the node's mode.
    fn policy(&mut self, k: &Key, b: &KState, o: &KState, t: &KState, class: &str) -> KState {
        KState::Conflict(Box::new(self.conflict(k, class, flat(b), b, o, t)))
    }

    /// RS-003 `join`: the greater of o and t in the kind's merge order.
    fn join(&self, k: &Key, o: &KState, t: &KState) -> KState {
        let Key::Node(n, _) = k else { return o.clone() };
        let kind = self.kind(*n).unwrap_or_default();
        let schema = &self.st(1).schema;
        let st = |v: &KState| match flat(v) {
            Some(KVal::Status { status, .. }) => status,
            _ => String::new(),
        };
        let (so, stt) = (st(o), st(t));
        // o < t when o is covered, transitively, by t.
        let mut stack = vec![stt.clone()];
        let mut seen = BTreeSet::new();
        while let Some(x) = stack.pop() {
            if !seen.insert(x.clone()) {
                continue;
            }
            if let Some(e) = schema.value(&kind, "status", &x) {
                for c in &e.covers {
                    if *c == so {
                        return t.clone();
                    }
                    stack.push(c.clone());
                }
            }
        }
        o.clone()
    }

    /// LR-005 `union3-compose`: `union3`, then every glob a side added rewritten through the other side's gained
    /// directory moves of the `project` root node (CP-009 to CP-011).
    // rule: CP-009, CP-010, CP-011
    fn union3_compose(&self, k: &Key, b: &KState, o: &KState, t: &KState) -> Option<KVal> {
        let Key::Node(n, Aspect::Field(f)) = k else {
            return union3(&flat(b), &flat(o), &flat(t));
        };
        let elems = |v: &Option<KVal>| -> Vec<Value> {
            match v {
                Some(KVal::Value(Value::Set(s))) => s.clone(),
                _ => Vec::new(),
            }
        };
        let (eb, eo, et) = (elems(&flat(b)), elems(&flat(o)), elems(&flat(t)));
        let kind = self.kind(*n).unwrap_or_default();
        let shape = self
            .st(1)
            .schema
            .field(&kind, f)
            .map_or(Shape::Plain, |fi| fi.shape);
        let project = uid_root("project");
        let gained_by = |i: usize| self.gained_moves(i, project);
        // Each side's added elements are rewritten through the other side's gained entries.
        let rewrite = |added: Vec<Value>, entries: &[PathMove]| -> Vec<Value> {
            added
                .into_iter()
                .map(|v| match v {
                    Value::Text(s) => Value::Text(rewrite_glob(&s, shape, entries)),
                    other => other,
                })
                .collect()
        };
        let o_added: Vec<Value> = eo.iter().filter(|x| !eb.contains(x)).cloned().collect();
        let t_added: Vec<Value> = et.iter().filter(|x| !eb.contains(x)).cloned().collect();
        let kept: Vec<Value> = eb
            .iter()
            .filter(|x| eo.contains(x) && et.contains(x))
            .cloned()
            .collect();
        let mut all = kept;
        all.extend(rewrite(o_added, &gained_by(2)));
        all.extend(rewrite(t_added, &gained_by(1)));
        Value::set(all).map(KVal::Value)
    }

    /// CP-002, CP-003: the `path_moves` entries side `i`'s root node (uid `root`) holds and the base's does not, of class
    /// `explicit`, `confirmed` or `committed`, in (hlc, from, to) order.
    // rule: CP-001, CP-002, CP-003
    fn gained_moves(&self, i: usize, root: Uid) -> Vec<PathMove> {
        let moves_of = |s: &State| -> Vec<PathMove> {
            s.nodes
                .values()
                .find(|x| x.uid == root && x.live())
                .and_then(|x| x.fields.get("path_moves"))
                .map(|v| {
                    v.elems()
                        .iter()
                        .filter_map(|e| match e {
                            Value::PathMove(m) => Some((**m).clone()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let base = moves_of(self.st(0));
        let mut out: Vec<PathMove> = moves_of(self.st(i))
            .into_iter()
            .filter(|m| !base.contains(m))
            .filter(|m| m.class != crate::value::MoveClass::Observed)
            .collect();
        out.sort_by(|a, b| {
            (a.hlc, &a.from.text, &a.to.text).cmp(&(b.hlc, &b.from.text, &b.to.text))
        });
        out
    }

    /// LC-002 and LR-001 `compose`: when exactly one side S′ gained on the file node's root node an entry whose `from`
    /// prefixes both b's `path` and the other side S's `path`, S's composite with `path` rewritten through S′'s gained
    /// entries and `relink` = `merge-compose/prefix` (CP-004 to CP-006). Returns S's pre-composition path and the
    /// composed value.
    // rule: CP-004, CP-005, CP-006, CP-007, CP-008
    fn composer(&self, k: &Key, b: &KState, o: &KState, t: &KState) -> Option<(PathVal, KVal)> {
        let Key::Node(n, Aspect::Observation) = k else {
            return None;
        };
        let path_of = |v: &Option<KVal>| -> Option<PathVal> {
            match v {
                Some(KVal::Observation(vs)) => match vs.first() {
                    Some(Some(Value::Path(p))) => Some(p.clone()),
                    _ => None,
                },
                _ => None,
            }
        };
        let (pb, po, pt) = (path_of(&flat(b))?, path_of(&flat(o))?, path_of(&flat(t))?);
        if po == pt {
            return None;
        }
        let root = [1, 2, 0].iter().find_map(|i| {
            self.st(*i)
                .nodes
                .get(n)
                .and_then(|x| x.text("root").map(str::to_string))
        })?;
        let ru = uid_root(&root);
        let covers = |i: usize, other: &PathVal| -> Vec<PathMove> {
            self.gained_moves(i, ru)
                .into_iter()
                .filter(|m| {
                    m.from.root == pb.root
                        && pb.text.starts_with(&m.from.text)
                        && m.from.root == other.root
                        && other.text.starts_with(&m.from.text)
                })
                .collect()
        };
        // S′ = ours (1) composes theirs' path, or S′ = theirs (2) composes ours'.
        let o_gains = !covers(1, &pt).is_empty();
        let t_gains = !covers(2, &po).is_empty();
        let (s, s_prime) = match (o_gains, t_gains) {
            (true, false) => (2, 1),
            (false, true) => (1, 2),
            _ => return None,
        };
        let sv = flat(if s == 1 { o } else { t })?;
        let KVal::Observation(mut vs) = sv else {
            return None;
        };
        let before = if s == 1 { po } else { pt };
        let mut p = before.clone();
        for m in self.gained_moves(s_prime, ru) {
            if m.from.root == p.root && p.text.starts_with(&m.from.text) {
                p.text = format!("{}{}", m.to.text, &p.text[m.from.text.len()..]);
                p.root = m.to.root.clone();
            }
        }
        vs[0] = Some(Value::Path(p));
        vs[5] = Some(Value::Text("merge-compose/prefix".into()));
        Some((before, KVal::Observation(vs)))
    }

    /// LC-003, LC-005: `Some(S)` when the uid is absent in the base, side S holds it live with a status other than
    /// `removed`, and the other side's final state is deleted or live `removed`; `rootish` selects root-key uids,
    /// otherwise file-key uids.
    fn created_vs_dead(&self, n: Nid, rootish: bool) -> Option<usize> {
        let kind = self.kind(n)?;
        let file_key = self
            .st(1)
            .schema
            .kind(&kind)
            .or_else(|| self.st(2).schema.kind(&kind))
            .is_some_and(|k| k.uid == UidDerivation::FileKey);
        let root_node = kind == "area"
            && [1, 2].iter().any(|i| {
                self.st(*i)
                    .nodes
                    .get(&n)
                    .is_some_and(|x| x.fields.contains_key("root"))
            });
        if (rootish && !root_node) || (!rootish && !file_key) {
            return None;
        }
        let alive = |i: usize| {
            self.st(i)
                .nodes
                .get(&n)
                .is_some_and(|x| x.live() && x.status != "removed")
        };
        let dead = |i: usize| {
            self.st(i)
                .nodes
                .get(&n)
                .is_some_and(|x| !x.live() || x.status == "removed")
        };
        if self.st(0).nodes.contains_key(&n) {
            return None;
        }
        if alive(1) && dead(2) {
            Some(1)
        } else if alive(2) && dead(1) {
            Some(2)
        } else {
            None
        }
    }

    // ----------------------------------------------------------------------------------------------------------
    // The procedure
    // ----------------------------------------------------------------------------------------------------------

    /// RK-001 to RK-011: every file-node uid created on one side while the other side ended it moves, on that side, to
    /// `uid_file(root, origin_path, U)` — re-derived while the result names a node of b, o or t — with every edge
    /// that side added to U re-pointed, and U keeps the other side's state ([F12 §7.6]).
    // rule: RK-001, RK-002, RK-003, RK-004, RK-005, RK-006, RK-007, RK-008, RK-009, RK-010, RK-011
    fn rekey(&mut self) {
        let mut ids: BTreeSet<Nid> = self.st(1).nodes.keys().copied().collect();
        ids.extend(self.st(2).nodes.keys().copied());
        for u in ids {
            let Some(s) = self.created_vs_dead(u, false) else {
                continue;
            };
            let x = self.st(s).nodes[&u].clone();
            let root = x.text("root").unwrap_or_default().to_string();
            let path = match x.fields.get("origin_path") {
                Some(Value::Path(p)) => p.text.clone(),
                _ => String::new(),
            };
            // RK-003, RK-004: derive, re-deriving while the uid names a node of the three states.
            let names = |w: Uid| {
                self.s
                    .iter()
                    .any(|st| st.nodes.values().any(|y| y.uid == w))
            };
            let bound: usize = self
                .s
                .iter()
                .map(|st| st.nodes.values().filter(|y| y.kind == "artifact").count())
                .sum();
            let mut q = x.uid;
            let mut next = uid_file(&root, &path, Some(q));
            let mut loops = 0;
            while names(next) {
                loops += 1;
                assert!(
                    loops <= bound + 1,
                    "internal error: the re-key derivation loop exceeded its bound (RK-004)"
                );
                q = next;
                next = uid_file(&root, &path, Some(q));
            }
            let nu = self.nid_of(next);
            let q_nid = self.nid_of(q);
            // The base is read in place; the side is copied on its first change (the input states are borrowed).
            let [b0, s1, s2] = &mut self.s;
            let base: &State = b0;
            let st: &mut State = if s == 1 { s1.to_mut() } else { s2.to_mut() };
            // RK-005: every key U owns moves to uid′, with `origin_pred` = q.
            let mut moved = st.nodes.remove(&u).expect("the side holds U");
            moved.uid = next;
            moved.fields.insert("origin_pred".into(), Value::Ref(q_nid));
            // RK-007: U keeps the other side's state — here the base's, which does not hold it.
            if let Some(bx) = base.nodes.get(&u) {
                st.nodes.insert(u, bx.clone());
            }
            // RK-006: edges to U the side added since the base, and `replaced_by` and `ref` values equal to U it set.
            for (m, y) in st.nodes.iter_mut() {
                let by = base.nodes.get(m);
                let added: Vec<EdgeKey> = y
                    .out
                    .keys()
                    .filter(|k| k.dst == u && by.is_none_or(|b| !b.out.contains_key(*k)))
                    .cloned()
                    .collect();
                for k in added {
                    let p = y.out.remove(&k).expect("present");
                    y.out.insert(
                        EdgeKey {
                            kind: k.kind,
                            dst: nu,
                            disc: k.disc,
                        },
                        p,
                    );
                }
                for (f, v) in y.fields.iter_mut() {
                    if *v == Value::Ref(u)
                        && by.is_none_or(|b| b.fields.get(f) != Some(&Value::Ref(u)))
                    {
                        *v = Value::Ref(nu);
                    }
                }
                if y.parent == Some(u) && by.is_none_or(|b| b.parent != Some(u)) {
                    y.parent = Some(nu);
                }
                if let Some(t) = &mut y.tomb
                    && t.replaced_by == Some(u)
                    && by.is_none_or(|b| b.tomb.as_ref().is_none_or(|bt| bt.replaced_by != Some(u)))
                {
                    t.replaced_by = Some(nu);
                }
            }
            st.nodes.insert(nu, moved);
            self.rekeyed[s].insert(u, nu);
            self.out
                .rows
                .insert(Key::Node(u, Aspect::Existence), "LM-007".into());
        }
    }

    /// Decides every existence key and each uid's mode ([RULES/merge-table] PR-007).
    fn existence(&mut self) -> BTreeMap<Nid, Mode> {
        let mut ids: BTreeSet<Nid> = BTreeSet::new();
        for s in &self.s {
            ids.extend(s.nodes.keys().copied());
        }
        let mut modes = BTreeMap::new();
        for n in ids {
            let k = Key::Node(n, Aspect::Existence);
            let (b, o, t) = (self.val(0, &k), self.val(1, &k), self.val(2, &k));
            if self.eq(&k, &b, &o) && self.eq(&k, &o, &t) {
                modes.insert(n, self.mode_of(n, &o, None, &b, &o, &t, false));
                continue;
            }
            let d = self.decide(&k, &b, &o, &t);
            if let Some(class) = d.violation {
                self.violation(class, Some(k.clone()));
            }
            let fixed_policy = d.row.contains("AP-00");
            let mode = self.mode_of(
                n,
                &d.value,
                d.taken,
                &b,
                &o,
                &t,
                fixed_policy || d.violation.is_some(),
            );
            if let KState::Conflict(c) = &d.value
                && !self.eq(&k, &d.value, &o)
            {
                self.out.conflicts.push((k.clone(), c.class.clone()));
            }
            self.out.rows.insert(k, d.row);
            modes.insert(n, mode);
        }
        modes
    }

    /// The mode of a uid from its existence result `r`; `taken` is the side whose value `r` is, as that side holds it,
    /// when the key takes a side's value. A conflict value fixes the node: it is copied whole, with `r` on its
    /// existence key, from a side that holds it in `r`'s provisional state ([F12 §6.3]). For a value this merge makes
    /// (RS-008, RS-010, RS-015) that is the side its `prov` names, a side of this merge (a conflict-valued side
    /// contributes its provisional node). For a value the key takes from one side (MR-003, MR-004, AP-002, AP-003) it
    /// is that side: the value is carried whole, its `prov` names a side of the merge that made it, and the side that
    /// holds the value holds the node in its provisional state. When o and t both hold `r` (b, o and t equal, or
    /// MR-001), both hold the node in that state and [`Engine::shared_holder`] picks the side. A tombstone is copied
    /// from the side that holds it; an automatic side or a staged value fixes the node to that side; a live result
    /// merges the other keys.
    // spec: [F12 §6.3]
    // spec: [RULES/merge-table] PR-007
    #[allow(clippy::too_many_arguments)]
    fn mode_of(
        &self,
        n: Nid,
        r: &KState,
        taken: Option<From>,
        b: &KState,
        o: &KState,
        t: &KState,
        fixed: bool,
    ) -> Mode {
        let side_of = |v: &KState| -> From {
            let key = Key::Node(n, Aspect::Existence);
            if self.eq(&key, v, o) {
                From::Ours
            } else if self.eq(&key, v, t) {
                From::Theirs
            } else if self.eq(&key, v, b) {
                From::Base
            } else {
                From::Ours
            }
        };
        match r {
            KState::Conflict(c) => {
                let from = if self.eq(&Key::Node(n, Aspect::Existence), o, t) {
                    self.shared_holder(n)
                } else {
                    taken.unwrap_or(match c.prov {
                        Some(Side::Theirs) => From::Theirs,
                        _ => From::Ours,
                    })
                };
                Mode::Fixed(from, Some(c.clone()))
            }
            KState::Plain(None) => Mode::Absent,
            KState::Plain(Some(KVal::Deleted { .. })) => {
                let tomb = |v: &KState| matches!(v, KState::Plain(Some(KVal::Deleted { .. })));
                if tomb(o) && tomb(t) {
                    Mode::Tomb(side_of(r))
                } else {
                    Mode::Fixed(side_of(r), None)
                }
            }
            KState::Plain(Some(_)) if fixed => Mode::Fixed(side_of(r), None),
            KState::Plain(Some(_)) => Mode::Merged,
        }
    }

    /// The side a uid is copied from when o and t hold the same conflict value on its existence key, so both hold the
    /// node in the value's provisional state ([F12 §6.3]): theirs when theirs changed one of the node's keys since the
    /// base (CS-014's "modified") and ours did not, ours otherwise. When at most one side changed the node, its keys are
    /// then the ones PR-007's key-by-key merge gives them, each key one side did not touch taking the other side's
    /// value ([AR §3.4] I25′; the `ours-only` and `theirs-only` rows). When both changed it, ours is copied and theirs'
    /// changes are lost, which PR-007 read literally does not call for (an open specification question).
    // spec: [RULES/merge-table] PR-007
    // spec: [F12 §6.3]
    fn shared_holder(&self, n: Nid) -> From {
        if self.modified(2, n) && !self.modified(1, n) {
            From::Theirs
        } else {
            From::Ours
        }
    }

    /// The row decisions of every other key of the merged uids, and the schema keys.
    fn keys(&mut self, modes: &BTreeMap<Nid, Mode>) -> BTreeMap<Key, KState> {
        let mut out = BTreeMap::new();
        // Schema keys first: a node's defaults depend on them.
        let mut sk: BTreeSet<crate::schema::ItemKey> = BTreeSet::new();
        for s in &self.s {
            sk.extend(s.schema.items.keys().cloned());
            sk.extend(s.schema_conflicts.keys().cloned());
        }
        for i in sk {
            let k = Key::Schema(i);
            let (b, o, t) = (self.val(0, &k), self.val(1, &k), self.val(2, &k));
            if self.eq(&k, &b, &o) && self.eq(&k, &o, &t) {
                out.insert(k, o);
                continue;
            }
            let d = self.decide(&k, &b, &o, &t);
            self.record(&k, &d, &o);
            out.insert(k, d.value);
        }
        for (n, m) in modes {
            let tomb = match m {
                Mode::Merged => false,
                Mode::Tomb(_) => true,
                Mode::Fixed(from, None) => {
                    // A tombstone copied from one side: its title and retained edges are that side's; the rows that
                    // decide them are recorded for the record only.
                    let i = match from {
                        From::Base => 0,
                        From::Ours => 1,
                        From::Theirs => 2,
                    };
                    if self.st(i).nodes.get(n).is_some_and(|x| !x.live()) {
                        let mut asp = aspects(self.st(i), *n);
                        asp.retain(|a| {
                            matches!(a, Aspect::Edge(_)) || *a == Aspect::Field("title".into())
                        });
                        for a in asp {
                            let k = Key::Node(*n, a);
                            let (b, o, t) = (self.val(0, &k), self.val(1, &k), self.val(2, &k));
                            let row = [(o.clone(), "ours-only"), (t.clone(), "theirs-only")]
                                .into_iter()
                                .find(|(v, _)| {
                                    self.eq(&k, v, &self.val(i, &k)) && !self.eq(&k, v, &b)
                                })
                                .map(|(_, r)| r);
                            if let Some(r) = row {
                                self.out.rows.insert(k, r.into());
                            }
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            let mut asp = aspects(self.st(0), *n);
            asp.extend(aspects(self.st(1), *n));
            asp.extend(aspects(self.st(2), *n));
            asp.remove(&Aspect::Hierarchy);
            if tomb {
                // A tombstone's keys are its title and its retained out-edges ([F07 §6.4]).
                asp.retain(|a| matches!(a, Aspect::Edge(_)) || *a == Aspect::Field("title".into()));
            }
            for a in asp {
                let k = Key::Node(*n, a);
                let (b, o, t) = (self.val(0, &k), self.val(1, &k), self.val(2, &k));
                if self.eq(&k, &b, &o) && self.eq(&k, &o, &t) {
                    out.insert(k, o);
                    continue;
                }
                let d = self.decide(&k, &b, &o, &t);
                // A composition's pre-composition path joins the node's aliases (CP-007, LR-004).
                if d.row == "LM-005"
                    && let Some((before, _)) = self.composer(&k, &b, &o, &t)
                {
                    self.precomp
                        .entry(*n)
                        .or_default()
                        .push(Value::Path(before));
                }
                self.record(&k, &d, &o);
                out.insert(k, d.value);
            }
        }
        // Aliases decided before a composition of the same node take its pre-composition path.
        let redo: Vec<Nid> = self.precomp.keys().copied().collect();
        for n in redo {
            let k = Key::Node(n, Aspect::Field("aliases".into()));
            let (b, o, t) = (self.val(0, &k), self.val(1, &k), self.val(2, &k));
            let d = self.decide(&k, &b, &o, &t);
            self.out.rows.insert(k.clone(), d.row);
            out.insert(k, d.value);
        }
        out
    }

    /// Records a key's decision: its violation, and its conflict value when the merge lands one dst did not hold
    /// ([F12 §6.3]: the commit that introduces a conflict value carries its `Conflict` op).
    fn record(&mut self, k: &Key, d: &Decided, o: &KState) {
        if let Some(class) = d.violation {
            self.violation(class, Some(k.clone()));
        }
        if let KState::Conflict(c) = &d.value
            && d.violation.is_none()
            && !self.eq(k, &d.value, o)
        {
            self.out.conflicts.push((k.clone(), c.class.clone()));
        }
        self.out.rows.insert(k.clone(), d.row.clone());
    }

    fn violation(&mut self, class: &'static str, key: Option<Key>) {
        if self.cx.op == Op::Virtual {
            return;
        }
        let description = match &key {
            Some(k) => format!("{class} on {}", key_text(k, &|n| self.uid(n))),
            None => class.to_string(),
        };
        self.out.violations.push(Violation {
            class,
            code: violation_code(class),
            key,
            description,
            suggested: "moirai resolve the key with --take ours|theirs|base".into(),
        });
    }

    /// RS-007 `kleppmann` over the merged live nodes ([F12 §7.4] row "Kleppmann steps"; [F12 §5.3] VM-7; MR-039,
    /// MR-040, CS-013; [AR §11] OQ-A-11). A one-sided merge, whose base is state(tip(dst)) ([`Start::TakeSrc`]),
    /// replays nothing: every merged node takes src's (parent, order) (11.1 (A); dst touched no key since the base, so
    /// I25′ forbids a conflict on any key). Otherwise, for a merge, a `sync` or a virtual merge, every merged node
    /// starts from its (parent, order) in state(R), the replay start ([`crate::dag::Dag::replay_start`]: a commit that
    /// every other commit of A(o) ∪ A(t) descends from or precedes, so no commit is replayed on a state it was not
    /// made on, 11.1 (B)); each commit of A(o) \ A(R) and of A(t) \ A(R) that has step keys is one step
    /// ([`crate::dag::Dag::move_steps`]: the hierarchy keys of its net changeset against its first parent, and a
    /// two-parent commit's second-parent keys, read recursively, [RULES/merge-table] open point 35 case (i) and 11.3),
    /// keyed by its (hlc, commit id) and setting those keys to their values in that commit's state; and for each side,
    /// a hierarchy key whose value on that side differs from its value at the start while no step of that side sets
    /// it is set to that side's value in a step keyed (0, 0), before every commit. For a revert or a cherry-pick of C,
    /// every merged node starts from o's (parent, order), dst has no step and neither side has a (0, 0) step: src's one
    /// step is C's, which the caller builds (keyed by C's (hlc, id), valued in src's state, less the keys a commit of
    /// A(o) after C moved; [RULES/merge-table] open point 35 case (ii)).
    ///
    /// The steps apply in ascending (hlc, commit id) order, commit ids compared bytewise, all moves of a step at once;
    /// where two steps share a key (a commit that is a step of both sides, or the two sides' (0, 0) steps), dst's moves
    /// apply first, then src's. Two steps that share a key stay two steps: dst's applies and is checked, then src's, so
    /// a cycle that src's step closes undoes src's moves, never dst's (the reading of "dst's moves apply first, then
    /// src's" this model takes; a commit that is a step of both sides moves the same keys to the same values twice,
    /// which gives the result of applying it once). Only a move that changes its node's parent can close a cycle, so
    /// only such a move is ever undone: a move that keeps its node's parent (it sets the node's current value, case
    /// (iii), or changes only its order, 11.2) is never undone and never makes its key `kleppmann-skipped`. After a step
    /// in which a node is its own ancestor, the step's moves that changed their node's parent are undone one at a time,
    /// the least uid among those moves' nodes that lie on a cycle and are not yet undone first, until none is (MR-039);
    /// since the state before the step is a forest, every cycle holds such a move. A key whose last move was undone is
    /// `kleppmann-skipped` (CS-013); a later move of it that applied decides its value (MR-040). Each key's value is its
    /// node's final (parent, order). Step entries follow the re-key (RK-005, RK-006): U's moves are uid′'s, and a parent
    /// U that the side set is uid′. Keys of a uid fixed by an existence policy (PR-007) take no part. Then, as the
    /// model prototypes OQ-A-12 (b) ([`replay_rule`]): a key the same in b, o and t takes that value and is never
    /// `kleppmann-skipped`, and a cycle left after that has its replay-decided keys reset to their value in b, the least
    /// uid on a cycle first, each reset key `kleppmann-skipped` (a backstop whose rule RS-007 does not state yet,
    /// `docs/spec/reviews/wave-3d-verify.md`). In test builds the thread's [`Rule`] selects the variant, and `Rule::Cand`
    /// first asks the candidate slot ([`cand::hierarchy`]).
    // spec: [RULES/merge-table] results
    // spec: [F12 §7.4]
    // spec: [F12 §5.3]
    fn kleppmann(&mut self, modes: &BTreeMap<Nid, Mode>) -> BTreeMap<Nid, Option<KVal>> {
        let hk = |n: Nid| Key::Node(n, Aspect::Hierarchy);
        let get = |st: &State, n: Nid| -> Option<KVal> { flat(&cval(st, n, &Aspect::Hierarchy)) };
        let merged: BTreeSet<Nid> = modes
            .iter()
            .filter(|(_, m)| matches!(m, Mode::Merged))
            .map(|(n, _)| *n)
            .collect();
        for n in &merged {
            let k = hk(*n);
            let (b, o, t) = (self.val(0, &k), self.val(1, &k), self.val(2, &k));
            if !self.eq(&k, &o, &b) || !self.eq(&k, &t, &b) {
                self.out.rows.insert(k, "MR-040".into());
            }
        }
        // A revert or a cherry-pick replays from o, with no dst step and no (0, 0) step ([RULES/merge-table] open point
        // 35 case (ii)).
        let pick = matches!(self.cx.op, Op::Revert | Op::CherryPick);
        // A one-sided merge takes src's (parent, order) for every merged node, with no replay (OQ-A-11 11.1 (A)).
        let one_sided = !pick && matches!(self.cx.start, Start::TakeSrc);
        let out = {
            let start: &State = match self.cx.start {
                _ if pick => self.st(1),
                Start::State(st) => st,
                Start::Base | Start::TakeSrc => self.st(0),
            };
            // Every result node's (parent, order) as the moves start: merged nodes from the replay start (from o for a
            // revert or a cherry-pick), fixed nodes as copied.
            let mut init: BTreeMap<Nid, Option<KVal>> = BTreeMap::new();
            for (n, m) in modes {
                let v = match m {
                    Mode::Merged => get(start, *n),
                    Mode::Fixed(From::Base, _) | Mode::Tomb(From::Base) => get(self.st(0), *n),
                    Mode::Fixed(From::Ours, _) | Mode::Tomb(From::Ours) => get(self.st(1), *n),
                    Mode::Fixed(From::Theirs, _) | Mode::Tomb(From::Theirs) => get(self.st(2), *n),
                    Mode::Absent => continue,
                };
                init.insert(*n, v);
            }
            let steps = if one_sided {
                Vec::new()
            } else {
                self.hsteps(&merged, start, pick)
            };
            // The hierarchy keys whose value is the same in b, o and t (OQ-A-12 (b)'s kept keys).
            let same3: BTreeSet<Nid> = merged
                .iter()
                .copied()
                .filter(|n| {
                    let k = hk(*n);
                    let (b, o, t) = (self.val(0, &k), self.val(1, &k), self.val(2, &k));
                    self.eq(&k, &o, &b) && self.eq(&k, &t, &b)
                })
                .collect();
            let side = |i: usize| -> BTreeMap<Nid, Option<KVal>> {
                merged.iter().map(|n| (*n, get(self.st(i), *n))).collect()
            };
            let uid = |n: Nid| self.uid(n);
            let x = HInput {
                op: self.cx.op,
                one_sided,
                merged: &merged,
                b: side(0),
                o: side(1),
                t: side(2),
                init,
                steps,
                same3,
                uid: &uid,
                origin: self.cx.origin,
                live: modes
                    .iter()
                    .filter(|(n, m)| match m {
                        Mode::Merged => true,
                        Mode::Fixed(f, _) => {
                            let i = match f {
                                From::Base => 0,
                                From::Ours => 1,
                                From::Theirs => 2,
                            };
                            self.st(i).live(**n).is_some()
                        }
                        Mode::Tomb(_) | Mode::Absent => false,
                    })
                    .map(|(n, _)| *n)
                    .collect(),
                dead: [0, 1, 2].map(|i| {
                    merged
                        .iter()
                        .copied()
                        .filter(|n| self.st(i).live(*n).is_none())
                        .collect()
                }),
            };
            // The RS-007 variant of a test thread ([`Rule`]); the candidate slot's hook first ([`cand::hierarchy`]).
            #[cfg(test)]
            let out = (rule() == Rule::Cand)
                .then(|| cand::hierarchy(&x))
                .flatten()
                .unwrap_or_else(|| replay_rule(&x, rule().kept_keys(), rule().backstop()));
            #[cfg(not(test))]
            let out = replay_rule(&x, true, true);
            out
        };
        for n in &out.skipped {
            self.skipped.insert(*n);
            self.out.rows.insert(hk(*n), "MR-039".into());
        }
        out.values
    }

    /// RS-007's steps over the merged nodes `merged` ([`Engine::kleppmann`]), in the order they apply: for each side
    /// (dst's, then src's) its (0, 0) step first (none for a revert or a cherry-pick, whose replay starts from o), then
    /// one step per commit of the side's [`Ctx::moves`] that moves a merged node, its entries re-keyed (RK-005, RK-006);
    /// a stable sort by (hlc, commit id) then keeps dst's step before src's where two steps share a key.
    fn hsteps(&self, merged: &BTreeSet<Nid>, start: &State, pick: bool) -> Vec<HStep> {
        let get = |st: &State, n: Nid| -> Option<KVal> { flat(&cval(st, n, &Aspect::Hierarchy)) };
        let mut steps: Vec<HStep> = Vec::new();
        for side in 0..2 {
            if pick && side == 0 {
                continue;
            }
            let i = side + 1;
            let (rk, base) = (&self.rekeyed[i], self.st(0));
            let mut listed: BTreeSet<Nid> = BTreeSet::new();
            let mut own = Vec::new();
            for s in self.cx.moves[side] {
                let moves: Moves = s
                    .moves
                    .iter()
                    .filter_map(|(n, v)| {
                        let m = rk.get(n).copied().unwrap_or(*n);
                        if !merged.contains(&m) {
                            return None;
                        }
                        let v = match v {
                            Some(KVal::Hierarchy {
                                parent: Some(p),
                                order,
                            }) if rk.contains_key(p)
                                && base.nodes.get(n).and_then(|x| x.parent) != Some(*p) =>
                            {
                                Some(KVal::Hierarchy {
                                    parent: Some(rk[p]),
                                    order: order.clone(),
                                })
                            }
                            v => v.clone(),
                        };
                        Some((m, v))
                    })
                    .collect();
                if !moves.is_empty() {
                    listed.extend(moves.iter().map(|(n, _)| *n));
                    own.push(HStep {
                        key: s.key,
                        side,
                        moves,
                    });
                }
            }
            // A key whose value on the side differs from its value at the start while no step of the side sets it: a
            // step keyed (0, 0), before every commit; none for a revert or a cherry-pick, whose replay starts from o.
            if pick {
                steps.extend(own);
                continue;
            }
            let drift: Moves = merged
                .iter()
                .filter(|n| !listed.contains(n))
                .filter_map(|n| {
                    let x = get(self.st(i), *n);
                    (x != get(start, *n)).then_some((*n, x))
                })
                .collect();
            if !drift.is_empty() {
                steps.push(HStep {
                    key: (0, [0; 32]),
                    side,
                    moves: drift,
                });
            }
            steps.extend(own);
        }
        steps.sort_by_key(|s| s.key);
        steps
    }

    /// Builds the candidate state ([RULES/merge-table] PR-009).
    fn assemble(
        &mut self,
        modes: &BTreeMap<Nid, Mode>,
        keys: BTreeMap<Key, KState>,
        hier: BTreeMap<Nid, Option<KVal>>,
    ) -> State {
        let mut st = State::default();
        for (k, v) in &keys {
            if let Key::Schema(i) = k {
                match v {
                    KState::Plain(Some(KVal::Item(it))) => {
                        st.schema.items.insert(i.clone(), it.clone());
                    }
                    KState::Plain(_) => {}
                    KState::Conflict(c) => {
                        if let Some(KVal::Item(it)) = flat(v) {
                            st.schema.items.insert(i.clone(), it);
                        }
                        st.schema_conflicts.insert(i.clone(), (**c).clone());
                    }
                }
            }
        }
        for (n, m) in modes {
            match m {
                Mode::Absent => {}
                Mode::Fixed(from, c) => {
                    let i = match from {
                        From::Base => 0,
                        From::Ours => 1,
                        From::Theirs => 2,
                    };
                    let mut x = self.st(i).nodes[n].clone();
                    x.conflicts.remove(&Aspect::Existence);
                    if let Some(c) = c {
                        x.conflicts.insert(Aspect::Existence, (**c).clone());
                    }
                    st.nodes.insert(*n, x);
                }
                Mode::Tomb(from) => {
                    let i = match from {
                        From::Base => 0,
                        From::Ours => 1,
                        From::Theirs => 2,
                    };
                    let mut x = self.st(i).nodes[n].clone();
                    x.conflicts.clear();
                    x.out.clear();
                    x.fields.remove("title");
                    for (k, v) in keys.range(Key::Node(*n, Aspect::Existence)..) {
                        let Key::Node(m, a) = k else { break };
                        if m != n {
                            break;
                        }
                        x.set_kstate(&st.schema, a, v.clone());
                    }
                    st.nodes.insert(*n, x);
                }
                Mode::Merged => {
                    let proto = [1, 2, 0]
                        .iter()
                        .find_map(|i| self.st(*i).nodes.get(n).filter(|x| x.live()))
                        .or_else(|| [1, 2, 0].iter().find_map(|i| self.st(*i).nodes.get(n)))
                        .expect("a merged uid is on some side")
                        .clone();
                    let mut x =
                        Node::new(proto.uid, &proto.kind, &st.schema, proto.creator.clone());
                    for (k, v) in keys.range(Key::Node(*n, Aspect::Existence)..) {
                        let Key::Node(m, a) = k else { break };
                        if m != n {
                            break;
                        }
                        x.set_kstate(&st.schema, a, v.clone());
                    }
                    if let Some(h) = hier.get(n) {
                        x.set_kstate(&st.schema, &Aspect::Hierarchy, KState::Plain(h.clone()));
                    }
                    st.nodes.insert(*n, x);
                }
            }
        }
        st
    }
}

/// A kind's automatic merge policy ([RULES/merge-table] `auto-policy`; [RULES/policy-keys] KF-032, PV-011): the value
/// of the policy data `merge.policy.<kind>` — `ours` and `theirs` resolve that kind's value conflicts to a side
/// (AP-002, AP-003), `delete-wins` and `resurrect` set its existence policy (AP-004, AP-005) — or `None` for `none`
/// (AP-001), the default. A value outside the table is a model bug.
// rule: AP-001, AP-002, AP-003, AP-004, AP-005
pub fn auto_policy<'a>(policies: &'a BTreeMap<String, String>, kind: &str) -> Option<&'a str> {
    let v = policies.get(kind).map(String::as_str)?;
    let row = rules()
        .table("auto-policy")
        .rows
        .iter()
        .find(|r| r.tok("value") == v)
        .unwrap_or_else(|| panic!("merge.policy.{kind} = {v} is not an auto-policy value"));
    (row.tok("effect") != "no-auto-resolution").then_some(v)
}

/// The land-or-stage decision of a merge's step 8 ([RULES/merge-table] `land-or-stage`, read as data;
/// [RULES/policy-keys] KF-031): `stage`, `land-conflicted` or `land`, from the numbers of violations and conflicts
/// and `strict` (`--strict` or `merge.strict`).
// spec: [AR §5a.7] step 8
pub fn land_or_stage(violations: usize, conflicts: usize, strict: bool) -> &'static str {
    let count = |cell: &str, n: usize| match cell {
        "any" => true,
        ">=1" => n >= 1,
        "0" => n == 0,
        other => panic!("land-or-stage count {other} has no implementation"),
    };
    let r = rules()
        .table("land-or-stage")
        .rows
        .iter()
        .find(|r| {
            count(r.tok("violations"), violations)
                && count(r.tok("conflicts"), conflicts)
                && match r.tok("strict") {
                    "any" => true,
                    "yes" => strict,
                    "no" => !strict,
                    other => panic!("land-or-stage strict {other} has no implementation"),
                }
        })
        .expect("land-or-stage is exhaustive");
    match r.tok("outcome") {
        "stage" => "stage",
        "land-conflicted" => "land-conflicted",
        "land" => "land",
        other => panic!("land-or-stage outcome {other} has no implementation"),
    }
}

/// Whether b → x is a strengthening of a schema item (CS-017): any change other than adding a kind, a field, an
/// enumeration value or an edge kind ([AR §2.12]; [F08 §8.1]).
fn strengthening(k: &Key, b: &Option<KVal>, x: &Option<KVal>) -> bool {
    if b == x {
        return false;
    }
    match (b, k) {
        // A policy row's every change is a strengthening, its addition included ([RULES/merge-table] MC-013, MR-055;
        // [F08 §8.5.6]: one atomic merge value).
        (
            None,
            Key::Schema(crate::schema::ItemKey::Query(_) | crate::schema::ItemKey::Policy(_)),
        ) => true,
        (None, Key::Schema(_)) => false,
        _ => true,
    }
}

/// The RS-007 variant a test runs on its thread (test builds only): the lockstep search that accepts OQ-A-12
/// (`suite::kleppmann`) and the evaluation harness that compares RS-007 alternatives (`suite::rs007eval`) run one store
/// per variant. Every thread starts under [`default_rule`], which `MOIRAI_RS007_RULE` sets, so the whole suite can run
/// under any variant.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum Rule {
    /// RS-007 as specified, with the OQ-A-12 prototype: kept keys (b) with their backstop, resolved keys (c).
    Current,
    /// RS-007 as wave 3c left it: no kept key (OQ-A-12 (b)), no backstop and no resolved key (OQ-A-12 (c)).
    Wave3c,
    /// The replay from B, as spec sync 3 stated RS-007: no one-sided merge, no replay start, no kept key, no backstop and
    /// no resolved key; the parent-only undo of OQ-A-11 11.2 and the recursive step keys of OQ-A-11 11.3 stay.
    FromB,
    /// The candidate slot of the evaluation harness ([`cand`]): `Current` until a candidate fills it.
    Cand,
}

#[cfg(test)]
impl Rule {
    /// Every variant, in the harness's column order.
    pub(crate) const ALL: [Rule; 4] = [Rule::Current, Rule::Wave3c, Rule::FromB, Rule::Cand];

    /// The variant's name as `MOIRAI_RS007_RULE` spells it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Rule::Current => "current",
            Rule::Wave3c => "wave3c",
            Rule::FromB => "fromb",
            Rule::Cand => "cand",
        }
    }

    /// The variant `MOIRAI_RS007_RULE` names (`current`, `wave3c`, `fromb` or `cand`, any case).
    pub(crate) fn parse(s: &str) -> Option<Rule> {
        Rule::ALL
            .into_iter()
            .find(|r| r.name().eq_ignore_ascii_case(s.trim()))
    }

    /// OQ-A-12 (b): a key the same in b, o and t keeps that value.
    pub(crate) fn kept_keys(self) -> bool {
        match self {
            Rule::Current => true,
            Rule::Cand => cand::KEPT_KEYS,
            Rule::Wave3c | Rule::FromB => false,
        }
    }

    /// OQ-A-12 (b)'s backstop for a cycle left after the replay.
    pub(crate) fn backstop(self) -> bool {
        match self {
            Rule::Current => true,
            Rule::Cand => cand::BACKSTOP,
            Rule::Wave3c | Rule::FromB => false,
        }
    }

    /// OQ-A-12 (c): a two-parent commit's resolved keys are step keys.
    pub(crate) fn resolved_keys(self) -> bool {
        match self {
            Rule::Current => true,
            Rule::Cand => cand::RESOLVED_KEYS,
            Rule::Wave3c | Rule::FromB => false,
        }
    }

    /// The replay from B (spec sync 3): no one-sided merge and no replay start.
    pub(crate) fn replays_from_b(self) -> bool {
        match self {
            Rule::FromB => true,
            Rule::Cand => cand::FROM_B,
            Rule::Current | Rule::Wave3c => false,
        }
    }
}

/// The variant every test thread starts under: `MOIRAI_RS007_RULE` (`current`, `wave3c`, `fromb` or `cand`), read once
/// per process, else `current` (test builds only).
#[cfg(test)]
pub(crate) fn default_rule() -> Rule {
    static DEFAULT: std::sync::OnceLock<Rule> = std::sync::OnceLock::new();
    *DEFAULT.get_or_init(|| match std::env::var("MOIRAI_RS007_RULE") {
        Ok(v) if !v.trim().is_empty() => Rule::parse(&v).unwrap_or_else(|| {
            panic!("MOIRAI_RS007_RULE={v:?}: expected current, wave3c, fromb or cand")
        }),
        _ => Rule::Current,
    })
}

/// What a variant's counters count (test builds only), per [`Rule`]: [`bump`] adds one to the counter of the thread's
/// current rule.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum Counter {
    /// A key OQ-A-12 (b)'s backstop reset to b ([`replay_rule`]).
    Backstop,
    /// A repair a candidate made (`cand`'s own count, for example a cycle repair in place of the replay's undo).
    Repair,
}

#[cfg(test)]
thread_local! {
    /// The variant this thread runs.
    pub(crate) static RULE: std::cell::Cell<Rule> = std::cell::Cell::new(default_rule());
    /// The counters, by (rule, counter, derived): `derived` counts what RS-007 did while a commit's resolved keys were
    /// derived ([`crate::dag::Dag::resolved_keys`] recomputes a landed merge's candidate), apart from the merges a
    /// command ran.
    static COUNTS: std::cell::RefCell<BTreeMap<(Rule, Counter, bool), usize>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
    /// The depth of resolved-key derivations on this thread.
    static DERIVING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Whether the merge reads no replay inputs (RS-007's replay start, steps and step keys): the evaluation harness's
/// candidate when it decides the hierarchy without a replay (`cand::NO_REPLAY`, test builds only); always `false`
/// outside test builds.
pub(crate) fn no_replay() -> bool {
    #[cfg(test)]
    {
        rule() == Rule::Cand && cand::NO_REPLAY
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// The variant this thread runs (test builds only).
#[cfg(test)]
pub(crate) fn rule() -> Rule {
    RULE.with(|r| r.get())
}

/// Adds one to `c` of the thread's current rule (test builds only).
#[cfg(test)]
pub(crate) fn bump(c: Counter) {
    let derived = DERIVING.with(|d| d.get() > 0);
    COUNTS.with(|m| *m.borrow_mut().entry((rule(), c, derived)).or_default() += 1);
}

/// The count of `c` under `r` on this thread since the last [`reset_counts`], outside resolved-key derivations.
#[cfg(test)]
pub(crate) fn count(r: Rule, c: Counter) -> usize {
    COUNTS.with(|m| m.borrow().get(&(r, c, false)).copied().unwrap_or(0))
}

/// The count of `c` under `r` on this thread inside resolved-key derivations.
#[cfg(test)]
pub(crate) fn count_derived(r: Rule, c: Counter) -> usize {
    COUNTS.with(|m| m.borrow().get(&(r, c, true)).copied().unwrap_or(0))
}

/// Clears this thread's counters.
#[cfg(test)]
pub(crate) fn reset_counts() {
    COUNTS.with(|m| m.borrow_mut().clear());
}

/// While alive, RS-007 runs on this thread count as a resolved-key derivation ([`count_derived`]).
#[cfg(test)]
pub(crate) struct Deriving(());

#[cfg(test)]
impl Deriving {
    /// Enters a derivation.
    pub(crate) fn enter() -> Deriving {
        DERIVING.with(|d| d.set(d.get() + 1));
        Deriving(())
    }
}

#[cfg(test)]
impl Drop for Deriving {
    fn drop(&mut self) {
        DERIVING.with(|d| d.set(d.get() - 1));
    }
}

/// The parent of a hierarchy value; `None` for a root and for `absent`.
fn parent_of(v: &Option<KVal>) -> Option<Nid> {
    match v {
        Some(KVal::Hierarchy { parent, .. }) => *parent,
        _ => None,
    }
}

/// Whether node `n` is its own ancestor in a (parent, order) map: its parent chain comes back to it. A chain that enters
/// a cycle without `n` stops after as many steps as the map has nodes.
fn on_cycle(cur: &BTreeMap<Nid, Option<KVal>>, n: Nid) -> bool {
    let parent = |x: Nid| match cur.get(&x) {
        Some(Some(KVal::Hierarchy { parent, .. })) => *parent,
        _ => None,
    };
    let mut x = parent(n);
    for _ in 0..=cur.len() {
        match x {
            None => return false,
            Some(y) if y == n => return true,
            Some(y) => x = parent(y),
        }
    }
    false
}

/// One step of RS-007's replay as [`Engine::kleppmann`] applies it: the order key of the commit (or (0, 0)), the side
/// whose step it is (0 dst, 1 src) and its moves over the merged nodes, ascending by node.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HStep {
    /// The (hlc, commit id) of the step's commit, or (0, 0) for a side's (0, 0) step.
    pub key: MoveKey,
    /// 0: dst's (ours) step; 1: src's (theirs).
    pub side: usize,
    /// Each moved merged node with its (parent, order), ascending by node.
    pub moves: Moves,
}

/// The inputs of RS-007's hierarchy decision for one merge, as [`Engine::kleppmann`] computes them: every value is a
/// flat (parent, order) hierarchy value ([`flat`] of the key's canonical state), `None` for a root with no order and
/// for `absent`.
pub(crate) struct HInput<'a> {
    /// The operation (a merge, a `sync`, a virtual merge, a revert or a cherry-pick).
    #[allow(dead_code)]
    pub op: Op,
    /// A one-sided merge: the base's commit is tip(dst) (OQ-A-11 11.1 (A)); `steps` is then empty.
    pub one_sided: bool,
    /// The merged nodes, those whose keys the merge decides key by key (live in the result, no existence policy).
    pub merged: &'a BTreeSet<Nid>,
    /// Each merged node's value in b.
    pub b: BTreeMap<Nid, Option<KVal>>,
    /// Each merged node's value in o (dst).
    #[allow(dead_code)]
    pub o: BTreeMap<Nid, Option<KVal>>,
    /// Each merged node's value in t (src).
    pub t: BTreeMap<Nid, Option<KVal>>,
    /// Every result node's value as the replay starts: merged nodes at the replay start R (at o for a revert or a
    /// cherry-pick), nodes fixed by an existence policy as copied from their side (the replay never moves them).
    pub init: BTreeMap<Nid, Option<KVal>>,
    /// RS-007's steps in the order they apply ([`Engine::hsteps`]).
    pub steps: Vec<HStep>,
    /// The merged nodes whose key is the same in b, o and t (canonical equality, [F07 §7.3]).
    pub same3: BTreeSet<Nid>,
    /// The uid of a `#N` (RS-007's undo order is by least uid).
    pub uid: &'a dyn Fn(Nid) -> Uid,
    /// The origin time of a side's value ([`Ctx::origin`]); `None` for a hand-built merge.
    #[allow(dead_code)]
    pub origin: Option<Origin<'a>>,
    /// The merged nodes that are not live (absent, or a tombstone) in b, o and t: a flat value reads `None` for them
    /// as for a live root with no order.
    #[allow(dead_code)]
    pub dead: [BTreeSet<Nid>; 3],
    /// The nodes live in the result: the merged nodes and the live nodes an existence policy fixed (a parent outside
    /// it is a `DanglingEdge`, V04).
    #[allow(dead_code)]
    pub live: BTreeSet<Nid>,
}

/// RS-007's hierarchy decision for one merge: every merged node's final value, and the nodes whose key is
/// `kleppmann-skipped` (MR-039: a `HierarchyCycle` violation on the key, CS-013).
pub(crate) struct HOutput {
    /// Each merged node's final (parent, order).
    pub values: BTreeMap<Nid, Option<KVal>>,
    /// The `kleppmann-skipped` keys' nodes.
    pub skipped: BTreeSet<Nid>,
}

/// Applies RS-007's steps to `cur` in order, all moves of a step at once; after a step in which a node is its own
/// ancestor, the step's moves that changed their node's parent are undone one at a time, the least uid among those
/// moves' nodes that lie on a cycle first, until none is (MR-039; a move that keeps its node's parent is never undone,
/// [RULES/merge-table] open point 35 cases (iii) and (vi)). The result maps each moved node to whether its last move
/// applied.
pub(crate) fn replay_steps(
    cur: &mut BTreeMap<Nid, Option<KVal>>,
    steps: &[HStep],
    uid: &dyn Fn(Nid) -> Uid,
) -> BTreeMap<Nid, bool> {
    // Whether each moved node's last move applied.
    let mut last: BTreeMap<Nid, bool> = BTreeMap::new();
    for s in steps {
        // The step: all its moves at once, each node's value before the step kept.
        let mut before: BTreeMap<Nid, Option<KVal>> = BTreeMap::new();
        for (n, v) in &s.moves {
            let old = cur.insert(*n, v.clone()).flatten();
            before.entry(*n).or_insert(old);
        }
        // The moves that changed their node's parent: only these can close a cycle, so only these are undone
        // ([RULES/merge-table] open point 35 case (iii); OQ-A-11 11.2).
        let changed: Vec<Nid> = before
            .iter()
            .filter(|(n, old)| parent_of(&cur.get(n).cloned().flatten()) != parent_of(old))
            .map(|(n, _)| *n)
            .collect();
        let mut undone: BTreeSet<Nid> = BTreeSet::new();
        loop {
            let worst = changed
                .iter()
                .copied()
                .filter(|n| !undone.contains(n) && on_cycle(cur, *n))
                .min_by_key(|n| uid(*n));
            let Some(n) = worst else { break };
            cur.insert(n, before[&n].clone());
            undone.insert(n);
        }
        for n in before.keys() {
            last.insert(*n, !undone.contains(n));
        }
    }
    last
}

/// RS-007's hierarchy result over its inputs ([`Engine::kleppmann`]). A one-sided merge takes t's value for every
/// merged node. Otherwise the steps replay from `init` ([`replay_steps`]); a key whose last move was undone is
/// `kleppmann-skipped`. With `kept_keys` (OQ-A-12 (b)), a key the same in b, o and t then takes that value and is never
/// `kleppmann-skipped`; with `backstop`, a cycle left after that (one the kept values close, or one through a node an
/// existence policy fixed) has its replay-decided keys reset to their value in b one at a time, the least uid on a
/// cycle first, until none is, each reset key `kleppmann-skipped`. Both are on outside test builds.
pub(crate) fn replay_rule(x: &HInput<'_>, kept_keys: bool, backstop: bool) -> HOutput {
    if x.one_sided {
        return HOutput {
            values: x.t.clone(),
            skipped: BTreeSet::new(),
        };
    }
    let mut cur = x.init.clone();
    let mut last = replay_steps(&mut cur, &x.steps, x.uid);
    // A hierarchy key whose value is the same in b, o and t keeps that value and is never `kleppmann-skipped`; the
    // replay decides the others ([AR §11] OQ-A-12 (b); [F12 §7.2]).
    let none = BTreeSet::new();
    let kept = if kept_keys { &x.same3 } else { &none };
    for n in kept {
        cur.insert(*n, x.b[n].clone());
        last.remove(n);
    }
    // The backstop for a cycle the kept values close: the cycle's replay-decided keys are reset to their value in b
    // one at a time, the least uid on a cycle first, until none is; each reset key is `kleppmann-skipped`.
    if backstop {
        loop {
            let worst = x
                .merged
                .iter()
                .copied()
                .filter(|n| {
                    !kept.contains(n)
                        && on_cycle(&cur, *n)
                        && cur.get(n).cloned().flatten() != x.b[n]
                })
                .min_by_key(|n| (x.uid)(*n));
            let Some(n) = worst else { break };
            cur.insert(n, x.b[&n].clone());
            last.insert(n, false);
            #[cfg(test)]
            bump(Counter::Backstop);
        }
    }
    let skipped = last
        .into_iter()
        .filter(|(_, applied)| !applied)
        .map(|(n, _)| n)
        .collect();
    cur.retain(|n, _| x.merged.contains(n));
    HOutput {
        values: cur,
        skipped,
    }
}

/// The text a key state holds for the text rule: `absent` reads as the empty text ([F12 §7.5]).
fn text_of(v: &KState) -> String {
    match flat(v) {
        Some(KVal::Value(Value::Text(s))) => s,
        Some(KVal::Body(s)) => s,
        _ => String::new(),
    }
}

/// A merged text as the key's value: an empty result is `absent` ([F12 §7.5] "Empty result").
fn text_value(k: &Key, r: String) -> Option<KVal> {
    if r.is_empty() {
        return None;
    }
    Some(match k {
        Key::Node(_, Aspect::Body) => KVal::Body(r),
        _ => KVal::Value(Value::Text(r)),
    })
}

/// RS-005 `union3`: (b ∩ o ∩ t) ∪ (o − b) ∪ (t − b), each `absent` read as the empty set; empty is `absent`.
fn union3(b: &Option<KVal>, o: &Option<KVal>, t: &Option<KVal>) -> Option<KVal> {
    let elems = |v: &Option<KVal>| -> Vec<Value> {
        match v {
            Some(KVal::Value(Value::Set(s))) => s.clone(),
            Some(KVal::Value(x)) => vec![x.clone()],
            _ => Vec::new(),
        }
    };
    let (eb, eo, et) = (elems(b), elems(o), elems(t));
    let mut out: Vec<Value> = eb
        .iter()
        .filter(|x| eo.contains(x) && et.contains(x))
        .cloned()
        .collect();
    out.extend(eo.iter().filter(|x| !eb.contains(x)).cloned());
    out.extend(et.iter().filter(|x| !eb.contains(x)).cloned());
    Value::set(out).map(KVal::Value)
}

/// CP-009, CP-010: a glob's literal prefix runs to the last `/` before its first wildcard (`*`, `?`, `[`); when it
/// starts with an entry's `from`, that leading `from` becomes the entry's `to`, for each entry in order. A tagged
/// `applies_to` element rewrites its `path:` component only.
fn rewrite_glob(s: &str, shape: Shape, entries: &[PathMove]) -> String {
    let (tag, glob) = match shape {
        Shape::Tagged => match s.strip_prefix("path:") {
            Some(g) => ("path:", g),
            None => return s.to_string(),
        },
        _ => ("", s),
    };
    let mut g = glob.to_string();
    for m in entries.iter().filter(|m| m.from.root == "project") {
        let wild = g.find(['*', '?', '[']).unwrap_or(g.len());
        let lit_end = g[..wild].rfind('/').map_or(0, |i| i + 1);
        if g[..lit_end].starts_with(&m.from.text) {
            g = format!("{}{}", m.to.text, &g[m.from.text.len()..]);
        }
    }
    format!("{tag}{g}")
}

/// The structural class of a `conflict` cell, as a static name.
fn static_class(c: &str) -> &'static str {
    [
        "HierarchyCycle",
        "Cycle",
        "DanglingEdge",
        "DepthExceeded",
        "Cardinality",
        "SchemaConflict",
        "QueryInvalid",
        "QueryCycle",
        "PlanMask",
        "RemovedTextNotInBase",
        "IdCollision",
        "ImageParse",
        "NotFound",
        "TombstoneRemoved",
    ]
    .into_iter()
    .find(|x| *x == c)
    .unwrap_or_else(|| panic!("{c} is not a structural class"))
}

/// The key text of [F12 §6.6] with `#N` for nodes (output form).
pub fn key_text(k: &Key, uid: &dyn Fn(Nid) -> Uid) -> String {
    let _ = uid;
    match k {
        Key::Schema(i) => i.text(),
        Key::Node(n, a) => match a {
            Aspect::Existence => format!("{n}.existence"),
            Aspect::Status => format!("{n}.status"),
            Aspect::Hierarchy => format!("{n}.parent"),
            Aspect::Observation => format!("{n}.observation"),
            Aspect::Body => format!("{n}.body"),
            Aspect::Field(f) | Aspect::Counter(f) => format!("{n}.{f}"),
            Aspect::Edge(e) => format!("edge:{n}:{}:{}", e.kind, e.dst),
        },
    }
}

/// The canonical order of keys for emission ([F13 §5] VO-2): by the canonical key of [F07 §10.3].
pub fn canonical_key(k: &Key, uid: &dyn Fn(Nid) -> Uid, schema: &Schema) -> canon::CKey {
    match k {
        Key::Schema(i) => canon::schema_ckey(i),
        Key::Node(n, a) => {
            let sym =
                matches!(a, Aspect::Edge(e) if schema.edge(&e.kind).is_some_and(|x| x.symmetric));
            canon::node_ckey(uid(*n), a, uid, sym)
        }
    }
}

/// A typed merge before its validators ([RULES/merge-table] PR-009): the candidate with the typed rules' conflicts and
/// violations in canonical key order ([F13 §5] VO-2), the nodes whose moves Kleppmann skipped, and the three states the
/// merge read (the re-keyed side as the re-key left it), which the validators read as the sides.
pub struct Pending<'a> {
    s: [Cow<'a, State>; 3],
    /// The candidate and the typed rules' records.
    pub m: Merged,
    /// The nodes whose hierarchy move Kleppmann skipped (MR-039), which V01 reports.
    pub skipped: BTreeSet<Nid>,
    /// The uid of every `#N` the three states hold or the merge allocated.
    pub(crate) uids: BTreeMap<Nid, Uid>,
}

impl Pending<'_> {
    /// Drops the typed records of the keys `settled` names — keys a staged resolution set on the candidate
    /// ([F12 §9.4] step 2): their conflicts, their violations (DM-012 `NotFound` included) and, for a hierarchy key,
    /// the skipped move V01 would report. The value such a key now holds is re-checked by the validators ([F12 §6.5]
    /// "On a violation's key").
    pub fn settle(&mut self, settled: &dyn Fn(&Key) -> bool) {
        self.m.conflicts.retain(|(k, _)| !settled(k));
        self.m
            .violations
            .retain(|v| v.key.as_ref().is_none_or(|k| !settled(k)));
        self.skipped
            .retain(|n| !settled(&Key::Node(*n, Aspect::Hierarchy)));
    }

    /// The validators V01–V13 of [F13 §5] on the candidate, in their order after the typed records, over the merge's
    /// own three states as the sides; none inside a virtual merge (VM-3).
    pub fn validate(mut self, cx: &Ctx<'_>) -> Merged {
        if cx.op != Op::Virtual {
            let uids = &self.uids;
            let uid = |n: Nid| uids.get(&n).copied().unwrap_or_else(|| (cx.uid)(n));
            let skipped: Vec<Nid> = self.skipped.iter().copied().collect();
            let v = crate::mvalid::validate(
                &mut self.m.st,
                [&self.s[0], &self.s[1], &self.s[2]],
                cx.dst_plan,
                &skipped,
                &uid,
            );
            self.m.conflicts.extend(v.conflicts);
            self.m.violations.extend(v.violations);
            self.m.hints = v.hints;
        }
        self.m
    }
}

/// The typed part of the three-way merge of b (base), o (dst) and t (src) ([F12 §7]; [RULES/merge-table] §10 PR-004 to
/// PR-009): the re-key, the existence keys, every other key and Kleppmann's moves; the validators are
/// [`Pending::validate`]'s.
// rule: PR-005, PR-006, PR-007, PR-008, PR-009, PR-016, PR-017
// rule: VB-009, VB-010, VB-011, VB-012, VB-019
pub fn typed<'a>(
    b: &'a State,
    o: &'a State,
    t: &'a State,
    cx: &Ctx<'_>,
    fresh: &mut Fresh,
) -> Pending<'a> {
    let mut e = Engine {
        s: [Cow::Borrowed(b), Cow::Borrowed(o), Cow::Borrowed(t)],
        cx,
        fresh,
        out: Merged {
            st: State::default(),
            conflicts: Vec::new(),
            violations: Vec::new(),
            hints: Vec::new(),
            rows: BTreeMap::new(),
        },
        precomp: BTreeMap::new(),
        skipped: BTreeSet::new(),
        rekeyed: Default::default(),
    };
    e.rekey();
    let modes = e.existence();
    let keys = e.keys(&modes);
    let hier = e.kleppmann(&modes);
    let st = e.assemble(&modes, keys, hier);
    let mut uids: BTreeMap<Nid, Uid> = e.fresh.uid.clone();
    for s in &e.s {
        for (n, x) in &s.nodes {
            uids.insert(*n, x.uid);
        }
    }
    let Engine {
        s,
        mut out,
        skipped,
        ..
    } = e;
    // Emission order ([F13 §5] VO-2): the typed rules' conflicts and violations in canonical key order; the
    // validators follow in their order.
    let uid = |n: Nid| uids.get(&n).copied().unwrap_or_else(|| (cx.uid)(n));
    out.conflicts
        .sort_by_cached_key(|(k, _)| canonical_key(k, &uid, &st.schema));
    out.violations
        .sort_by_cached_key(|v| v.key.as_ref().map(|k| canonical_key(k, &uid, &st.schema)));
    out.st = st;
    Pending {
        s,
        m: out,
        skipped,
        uids,
    }
}

/// The typed three-way merge of b (base), o (dst) and t (src) ([F12 §7]; [RULES/merge-table] §10 PR-004 to PR-010),
/// with the validators of [F13 §5] run on the candidate unless the merge is virtual.
// rule: PR-010
pub fn merge(b: &State, o: &State, t: &State, cx: &Ctx<'_>, fresh: &mut Fresh) -> Merged {
    typed(b, o, t, cx, fresh).validate(cx)
}

/// The canonical key order of a `#N` key set for tests and callers.
pub fn key_order(keys: &[Key], uid: &dyn Fn(Nid) -> Uid, schema: &Schema) -> Vec<Key> {
    let mut v = keys.to_vec();
    v.sort_by_cached_key(|k| canonical_key(k, uid, schema));
    v
}

impl Node {
    /// Sets one aspect's state as a merge result: a conflict value with its provisional member value, or a plain
    /// value ([F12 §6.3]).
    pub fn set_kstate(&mut self, schema: &Schema, a: &Aspect, v: KState) {
        self.conflicts.remove(a);
        let plain = match v {
            KState::Plain(p) => p,
            KState::Conflict(c) => {
                let p = flat(&KState::Conflict(c.clone()));
                self.conflicts.insert(a.clone(), *c);
                p
            }
        };
        self.put_value(schema, a, plain);
    }
}

#[cfg(test)]
pub(crate) mod cand;
#[cfg(test)]
mod tests;
