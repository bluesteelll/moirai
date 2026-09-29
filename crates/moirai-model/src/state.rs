//! The model state of one view: `BTreeMap<#N, Node>` with the view's schema items ([60 §4.2]; [F08]), its keys and key
//! values ([F06 §6.1]–§6.2), the net changeset of a commit as a state diff ([AR §4.6] "Net changeset = state diff") and
//! the fold of a net changeset into a state.
//!
//! A node holds its values in canonical form ([F07 §6.3], [F08 §6.2]): an empty value and a value equal to its field's
//! default are absent. So two states are equal exactly when their key values are, and `diff(a, b)` applied to `a` gives
//! `b` ([`diff`], [`State::apply`]).

use crate::schema::{Item, ItemKey, Schema};
use crate::value::{Nid, Uid, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The six observation fields of an `artifact`, one merge key ([F08 §9.3]; [40 §2.2]), in `kval` order ([F06 §6.2]).
pub const OBSERVATION: [&str; 6] = [
    "path",
    "oid",
    "bytes",
    "observed_git",
    "observed_blob",
    "relink",
];

/// The header enumerations and flags a tombstone keeps besides its status ([F08 §3.5]; [API §15.3]).
pub const TOMBSTONE_FIELDS: [&str; 5] = [
    "title",
    "priority",
    "criticality",
    "confidence",
    "authority",
];

/// `CREATOR`: the actor and role of the creating commit ([F08 §4]; [50] F4).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Creator {
    /// The actor.
    pub actor: String,
    /// The role; empty for none.
    pub role: String,
}

/// What a tombstone records besides its retained values ([F08 §3.5]; [RULES/delete-policy-matrix] TB rows).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Tomb {
    /// The delete's reason.
    pub reason: Option<String>,
    /// The replacement.
    pub replaced_by: Option<Nid>,
}

/// The key of an out-edge on its source node: (kind, destination, discriminator) ([F08 §10.1]).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct EdgeKey {
    /// The stored edge-kind name.
    pub kind: String,
    /// The destination.
    pub dst: Nid,
    /// The discriminator: the anchor uid of an `at` edge, none otherwise.
    pub disc: Option<Uid>,
}

/// An edge's property block ([F08 §10.2]).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EdgeProps {
    /// `flagged`: a retained out-edge of a deleted source (X4).
    pub flagged: bool,
    /// `pinned_commit`.
    pub pinned: Option<[u8; 32]>,
    /// The anchor record of an `at` edge: its hashed selector fields ([F07 §8.2]; [F08 §10.3]).
    pub anchor: Option<Box<crate::canon::Anchor>>,
}

/// The aspect of a node key: the key without its node ([F06 §6.1] classes 1–8).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Aspect {
    /// Class 1.
    Existence,
    /// Class 2: status with its resolution.
    Status,
    /// Class 3: (parent, order).
    Hierarchy,
    /// Class 4: a field.
    Field(String),
    /// Class 5: the artifact observation composite.
    Observation,
    /// Class 6: a counter field.
    Counter(String),
    /// Class 7: an out-edge.
    Edge(EdgeKey),
    /// Class 8: the body.
    Body,
}

/// A key of the canonical changeset ([AR §4.6] item 10; [F06 §6.1]).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Key {
    /// A node key.
    Node(Nid, Aspect),
    /// A schema item (class 9).
    Schema(ItemKey),
}

/// A key value ([F06 §6.2]); an absent value is `None` where it is held.
#[derive(Clone, PartialEq, Debug)]
pub enum KVal {
    /// Existence: live, of this kind.
    Live(String),
    /// Existence: a tombstone of this kind.
    Deleted {
        /// The kind.
        kind: String,
        /// The reason.
        reason: Option<String>,
        /// The replacement.
        replaced_by: Option<Nid>,
    },
    /// Status with its resolution.
    Status {
        /// The status.
        status: String,
        /// The resolution.
        resolution: String,
    },
    /// (parent, order).
    Hierarchy {
        /// The parent.
        parent: Option<Nid>,
        /// The order key.
        order: Option<String>,
    },
    /// A field or counter value.
    Value(Value),
    /// The six observation values.
    Observation(Vec<Option<Value>>),
    /// The body text.
    Body(String),
    /// An edge's properties.
    Edge(EdgeProps),
    /// A schema item.
    Item(Item),
}

/// The provisional side of an existence conflict ([F06 §6.2] `prov`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// `ours`.
    Ours,
    /// `theirs`.
    Theirs,
}

/// A node image: the value keys of one node — status, fields, counters, body — in canonical form, which a `live` side
/// of an existence conflict carries (`snap` = 1, [F06 §6.2], §6.3; [F07 §7.4]).
pub type Image = BTreeMap<Aspect, KVal>;

/// A conflict value ([F06 §6.2] `cstate` = 1): its class and three plain sides.
#[derive(Clone, PartialEq, Debug)]
pub struct Conflict {
    /// The conflict class ([F12 §6.1]).
    pub class: String,
    /// The base.
    pub base: Option<KVal>,
    /// Ours.
    pub ours: Option<KVal>,
    /// Theirs.
    pub theirs: Option<KVal>,
    /// The provisional side of an existence key.
    pub prov: Option<Side>,
    /// The node images of the base, ours and theirs sides of an existence conflict, each present exactly when that
    /// side is `live` ([F06 §6.2] "Snapshots").
    pub images: [Option<Image>; 3],
}

/// The state a key holds: a plain value (or absent), or a conflict value ([F06 §6.2] `cstate`).
#[derive(Clone, PartialEq, Debug)]
pub enum KState {
    /// A plain value or absent.
    Plain(Option<KVal>),
    /// A conflict value; the node's own member holds the provisional value.
    Conflict(Box<Conflict>),
}

impl KState {
    /// The absent plain state.
    pub const ABSENT: KState = KState::Plain(None);
}

/// One node of a view ([F08 §3]–§7), in canonical form.
#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    /// The uid, fixed at `Create`.
    pub uid: Uid,
    /// The kind name.
    pub kind: String,
    /// `Some` for a tombstone.
    pub tomb: Option<Tomb>,
    /// The status.
    pub status: String,
    /// The resolution.
    pub resolution: String,
    /// The parent.
    pub parent: Option<Nid>,
    /// The order key.
    pub order: Option<String>,
    /// Every other present field by name: the title, the header enumerations other than status, the flags, cold
    /// columns, kind fields and counters (as [`Value::Counter`]). A default or empty value is never held.
    pub fields: BTreeMap<String, Value>,
    /// The body.
    pub body: Option<String>,
    /// The out-edges.
    pub out: BTreeMap<EdgeKey, EdgeProps>,
    /// `CREATOR`.
    pub creator: Creator,
    /// The node's keys that hold a conflict value, by aspect.
    pub conflicts: BTreeMap<Aspect, Conflict>,
}

impl Node {
    /// A new live node of `kind` with every value at its default.
    pub fn new(uid: Uid, kind: &str, schema: &Schema, creator: Creator) -> Node {
        Node {
            uid,
            kind: kind.to_string(),
            tomb: None,
            status: schema.initial_status(kind).unwrap_or_default(),
            resolution: "none".into(),
            parent: None,
            order: None,
            fields: BTreeMap::new(),
            body: None,
            out: BTreeMap::new(),
            creator,
            conflicts: BTreeMap::new(),
        }
    }

    /// Whether the node is live (not a tombstone).
    pub fn live(&self) -> bool {
        self.tomb.is_none()
    }

    /// A field's value, its default when absent ([50 §3.3]).
    pub fn field(&self, schema: &Schema, name: &str) -> Option<Value> {
        match name {
            "status" => return Some(Value::Enum(self.status.clone())),
            "resolution" => return Some(Value::Enum(self.resolution.clone())),
            "parent" => return self.parent.map(Value::Ref),
            "order" => return self.order.clone().map(Value::Text),
            "body" => return self.body.clone().map(Value::Text),
            _ => {}
        }
        self.fields
            .get(name)
            .cloned()
            .or_else(|| schema.default_of(&self.kind, name))
    }

    /// The text of a field, when it holds one.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.fields.get(name).and_then(Value::as_str)
    }

    /// Sets a field in canonical form: `None`, an empty value and the field's default are absent.
    pub fn set_field(&mut self, schema: &Schema, name: &str, v: Option<Value>) {
        let v = v.filter(|v| !is_empty(v));
        let default = schema.default_of(&self.kind, name);
        match v {
            Some(v) if Some(&v) != default.as_ref() && v != Value::Counter(0) => {
                self.fields.insert(name.to_string(), v);
            }
            _ => {
                self.fields.remove(name);
            }
        }
    }

    /// The plain value of one aspect in canonical form ([F06 §6.2]; [F07 §6.3]: defaults are absent).
    pub fn get(&self, schema: &Schema, aspect: &Aspect) -> Option<KVal> {
        match aspect {
            Aspect::Existence => Some(match &self.tomb {
                None => KVal::Live(self.kind.clone()),
                Some(t) => KVal::Deleted {
                    kind: self.kind.clone(),
                    reason: t.reason.clone(),
                    replaced_by: t.replaced_by,
                },
            }),
            Aspect::Status => {
                let initial = schema.initial_status(&self.kind).unwrap_or_default();
                if self.status == initial && self.resolution == "none" {
                    None
                } else {
                    Some(KVal::Status {
                        status: self.status.clone(),
                        resolution: self.resolution.clone(),
                    })
                }
            }
            Aspect::Hierarchy => {
                (self.parent.is_some() || self.order.is_some()).then(|| KVal::Hierarchy {
                    parent: self.parent,
                    order: self.order.clone(),
                })
            }
            Aspect::Field(f) | Aspect::Counter(f) => self.fields.get(f).cloned().map(KVal::Value),
            Aspect::Observation => {
                let v: Vec<Option<Value>> = OBSERVATION
                    .iter()
                    .map(|f| self.fields.get(*f).cloned())
                    .collect();
                v.iter()
                    .any(Option::is_some)
                    .then_some(KVal::Observation(v))
            }
            Aspect::Edge(k) => self.out.get(k).cloned().map(KVal::Edge),
            Aspect::Body => self.body.clone().map(KVal::Body),
        }
    }

    /// The state of one aspect: its conflict value, or its plain value.
    pub fn kstate(&self, schema: &Schema, aspect: &Aspect) -> KState {
        match self.conflicts.get(aspect) {
            Some(c) => KState::Conflict(Box::new(c.clone())),
            None => KState::Plain(self.get(schema, aspect)),
        }
    }

    /// Every aspect that holds a value or a conflict, the existence included.
    pub fn aspects(&self, kind_is_artifact: bool) -> BTreeSet<Aspect> {
        let mut s = BTreeSet::new();
        s.insert(Aspect::Existence);
        s.insert(Aspect::Status);
        if self.parent.is_some() || self.order.is_some() {
            s.insert(Aspect::Hierarchy);
        }
        for (f, v) in &self.fields {
            if kind_is_artifact && OBSERVATION.contains(&f.as_str()) {
                s.insert(Aspect::Observation);
            } else if matches!(v, Value::Counter(_)) {
                s.insert(Aspect::Counter(f.clone()));
            } else {
                s.insert(Aspect::Field(f.clone()));
            }
        }
        if self.body.is_some() {
            s.insert(Aspect::Body);
        }
        for k in self.out.keys() {
            s.insert(Aspect::Edge(k.clone()));
        }
        s.extend(self.conflicts.keys().cloned());
        s
    }

    /// Sets one aspect's plain value. Existence is handled by [`State::apply`].
    pub(crate) fn put_value(&mut self, schema: &Schema, aspect: &Aspect, v: Option<KVal>) {
        self.put(schema, aspect, v);
    }

    fn put(&mut self, schema: &Schema, aspect: &Aspect, v: Option<KVal>) {
        match (aspect, v) {
            (Aspect::Status, None) => {
                self.status = schema.initial_status(&self.kind).unwrap_or_default();
                self.resolution = "none".into();
            }
            (Aspect::Status, Some(KVal::Status { status, resolution })) => {
                self.status = status;
                self.resolution = resolution;
            }
            (Aspect::Hierarchy, None) => {
                self.parent = None;
                self.order = None;
            }
            (Aspect::Hierarchy, Some(KVal::Hierarchy { parent, order })) => {
                self.parent = parent;
                self.order = order;
            }
            (Aspect::Field(f) | Aspect::Counter(f), None) => {
                self.fields.remove(f);
            }
            (Aspect::Field(f) | Aspect::Counter(f), Some(KVal::Value(v))) => {
                self.fields.insert(f.clone(), v);
            }
            (Aspect::Observation, None) => {
                for f in OBSERVATION {
                    self.fields.remove(f);
                }
            }
            (Aspect::Observation, Some(KVal::Observation(vs))) => {
                for (f, v) in OBSERVATION.iter().zip(vs) {
                    match v {
                        Some(v) => self.fields.insert(f.to_string(), v),
                        None => self.fields.remove(*f),
                    };
                }
            }
            (Aspect::Edge(k), None) => {
                self.out.remove(k);
            }
            (Aspect::Edge(k), Some(KVal::Edge(p))) => {
                self.out.insert(k.clone(), p);
            }
            (Aspect::Body, None) => self.body = None,
            (Aspect::Body, Some(KVal::Body(b))) => self.body = Some(b),
            (a, v) => panic!("key value {v:?} does not fit aspect {a:?}"),
        }
    }
}

/// Whether a value is an empty value, which is absent everywhere ([F08 §5.3]).
pub fn is_empty(v: &Value) -> bool {
    match v {
        Value::Text(s) | Value::Enum(s) => s.is_empty(),
        Value::Set(e) => e.is_empty(),
        Value::Oid(o) => o.digest.is_empty(),
        _ => false,
    }
}

/// The state of one view: its nodes and schema items ([60 §4.2]).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct State {
    /// Every node the view holds, live or tombstone.
    pub nodes: BTreeMap<Nid, Node>,
    /// The view's schema items.
    pub schema: Schema,
    /// Schema keys that hold a conflict value.
    pub schema_conflicts: BTreeMap<ItemKey, Conflict>,
}

/// The net changeset of a commit: every key whose state differs, with (before, after) ([AR §4.6]).
pub type Changeset = BTreeMap<Key, (KState, KState)>;

/// What folding a changeset needs besides the changeset: the uid and `CREATOR` of a `#N` that becomes live on the
/// view for the first time (the store-wide allocation, [F08 §2.1]).
pub trait Alloc {
    /// The uid bound to `#N`.
    fn uid(&self, n: Nid) -> Uid;
    /// The `CREATOR` of `#N`.
    fn creator(&self, n: Nid) -> Creator;
}

impl State {
    /// A live node.
    pub fn live(&self, n: Nid) -> Option<&Node> {
        self.nodes.get(&n).filter(|x| x.live())
    }

    /// The state of one key.
    pub fn kstate(&self, key: &Key) -> KState {
        match key {
            Key::Node(n, a) => match self.nodes.get(n) {
                None => KState::ABSENT,
                Some(node) => node.kstate(&self.schema, a),
            },
            Key::Schema(k) => match self.schema_conflicts.get(k) {
                Some(c) => KState::Conflict(Box::new(c.clone())),
                None => KState::Plain(self.schema.items.get(k).cloned().map(KVal::Item)),
            },
        }
    }

    /// Every key that holds a value or a conflict.
    pub fn keys(&self) -> BTreeSet<Key> {
        let mut s = BTreeSet::new();
        for (n, node) in &self.nodes {
            for a in node.aspects(node.kind == "artifact") {
                s.insert(Key::Node(*n, a));
            }
        }
        for k in self.schema.items.keys().chain(self.schema_conflicts.keys()) {
            s.insert(Key::Schema(k.clone()));
        }
        s
    }

    /// Sets a key's state. Existence keys are applied by [`State::apply`], which orders them.
    fn set(&mut self, key: &Key, v: KState) {
        match key {
            Key::Schema(k) => {
                self.schema_conflicts.remove(k);
                let plain = match v {
                    KState::Plain(p) => p,
                    KState::Conflict(c) => {
                        let prov = c.ours.clone().or_else(|| c.theirs.clone());
                        self.schema_conflicts.insert(k.clone(), *c);
                        prov
                    }
                };
                match plain {
                    Some(KVal::Item(i)) => {
                        self.schema.items.insert(k.clone(), i);
                    }
                    None => {
                        self.schema.items.remove(k);
                    }
                    Some(other) => panic!("key value {other:?} does not fit a schema key"),
                }
            }
            Key::Node(n, a) => {
                let State { nodes, schema, .. } = self;
                let node = nodes.get_mut(n).unwrap_or_else(|| {
                    panic!("a changeset sets {a:?} of {n}, which the view does not hold")
                });
                node.conflicts.remove(a);
                let plain = match v {
                    KState::Plain(p) => p,
                    KState::Conflict(c) => {
                        // The node's member holds the provisional value ([API §15.3], [F12 §6.3]).
                        let prov = match (c.prov, &c.ours, &c.theirs) {
                            (Some(Side::Theirs), _, t) => t.clone(),
                            (Some(Side::Ours), o, _) => o.clone(),
                            (None, None, t) => t.clone(),
                            (None, o, _) => o.clone(),
                        };
                        node.conflicts.insert(a.clone(), *c);
                        prov
                    }
                };
                node.put(schema, a, plain);
            }
        }
    }

    /// Folds a net changeset into this state: `apply(a, diff(a, b)) = b` ([AR §4.6] "Net changeset = state diff").
    /// Nodes that become present are created first, keys set next, nodes that become absent removed last.
    pub fn apply(&mut self, cs: &Changeset, alloc: &dyn Alloc) {
        // Schema items first: a node's defaults depend on them.
        for (k, (_, after)) in cs.iter().filter(|(k, _)| matches!(k, Key::Schema(_))) {
            self.set(k, after.clone());
        }
        let exist = |st: &KState| -> Option<KVal> {
            match st {
                KState::Plain(p) => p.clone(),
                KState::Conflict(c) => match (c.prov, &c.ours, &c.theirs) {
                    (Some(Side::Theirs), _, t) => t.clone(),
                    (_, o, _) => o.clone(),
                },
            }
        };
        let mut removed = Vec::new();
        for (k, (_, after)) in cs {
            let Key::Node(n, Aspect::Existence) = k else {
                continue;
            };
            match exist(after) {
                None => removed.push(*n),
                Some(KVal::Live(kind)) | Some(KVal::Deleted { kind, .. }) => {
                    let node = self.nodes.entry(*n).or_insert_with(|| {
                        Node::new(alloc.uid(*n), &kind, &self.schema, alloc.creator(*n))
                    });
                    node.conflicts.remove(&Aspect::Existence);
                    if let KState::Conflict(c) = after {
                        node.conflicts.insert(Aspect::Existence, (**c).clone());
                    }
                    match exist(after) {
                        Some(KVal::Deleted {
                            reason,
                            replaced_by,
                            ..
                        }) => {
                            node.tomb = Some(Tomb {
                                reason,
                                replaced_by,
                            });
                        }
                        _ => node.tomb = None,
                    }
                }
                Some(other) => panic!("key value {other:?} does not fit an existence key"),
            }
        }
        for (k, (_, after)) in cs {
            match k {
                Key::Node(n, Aspect::Existence) => {
                    let _ = n;
                }
                Key::Node(n, _) if removed.contains(n) => {}
                Key::Node(..) => self.set(k, after.clone()),
                Key::Schema(_) => {}
            }
        }
        for n in removed {
            self.nodes.remove(&n);
        }
    }
}

/// The net changeset from `a` to `b`: every key whose state differs ([AR §4.6] "Net changeset = state diff").
///
/// When the two schemas are equal only the nodes that differ are visited (a node's key values are a function of the
/// node and the schema); otherwise every key of both states is compared.
pub fn diff(a: &State, b: &State) -> Changeset {
    let mut cs = Changeset::new();
    if a.schema != b.schema || a.schema_conflicts != b.schema_conflicts {
        let mut keys = a.keys();
        keys.extend(b.keys());
        for k in keys {
            let before = a.kstate(&k);
            let after = b.kstate(&k);
            if before != after {
                cs.insert(k, (before, after));
            }
        }
        return cs;
    }
    let mut ids: BTreeSet<Nid> = BTreeSet::new();
    for (n, x) in &a.nodes {
        if b.nodes.get(n) != Some(x) {
            ids.insert(*n);
        }
    }
    for n in b.nodes.keys() {
        if !a.nodes.contains_key(n) {
            ids.insert(*n);
        }
    }
    for n in ids {
        let (x, y) = (a.nodes.get(&n), b.nodes.get(&n));
        let mut aspects = BTreeSet::new();
        for node in [x, y].into_iter().flatten() {
            aspects.extend(node.aspects(node.kind == "artifact"));
        }
        for asp in aspects {
            let before = x.map_or(KState::ABSENT, |x| x.kstate(&a.schema, &asp));
            let after = y.map_or(KState::ABSENT, |y| y.kstate(&b.schema, &asp));
            if before != after {
                cs.insert(Key::Node(n, asp), (before, after));
            }
        }
    }
    cs
}

/// The nodes a changeset touches, with every key it changes of theirs ([F06 §2.3] owner nodes: an edge key is owned by
/// its source).
pub fn touched(cs: &Changeset) -> BTreeSet<Nid> {
    cs.keys()
        .filter_map(|k| match k {
            Key::Node(n, _) => Some(*n),
            Key::Schema(_) => None,
        })
        .collect()
}

/// Whether `items` differ between two schemas.
pub fn schema_changed(a: &State, b: &State) -> bool {
    a.schema != b.schema
}

/// A map from the key of a schema item to the item, the form [`Schema`] keeps.
pub type Items = BTreeMap<ItemKey, Item>;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    struct Fixed;
    impl Alloc for Fixed {
        fn uid(&self, n: Nid) -> Uid {
            let mut b = [0u8; 16];
            b[..4].copy_from_slice(&n.0.to_le_bytes());
            Uid(b)
        }
        fn creator(&self, _: Nid) -> Creator {
            Creator {
                actor: "a".into(),
                role: "orchestrator".into(),
            }
        }
    }

    fn node(n: u32, kind: &str) -> Node {
        Node::new(
            Fixed.uid(Nid(n)),
            kind,
            &Schema::default(),
            Fixed.creator(Nid(n)),
        )
    }

    #[derive(Debug, Clone)]
    enum Op {
        Create(u32),
        Delete(u32),
        Remove(u32),
        Title(u32, u8),
        Priority(u32, u8),
        Status(u32, bool),
        Parent(u32, Option<u32>),
        Edge(u32, u32, bool),
        Counter(u32, i64),
        Body(u32, Option<u8>),
    }

    fn op() -> impl Strategy<Value = Op> {
        let n = 1u32..6;
        prop_oneof![
            n.clone().prop_map(Op::Create),
            n.clone().prop_map(Op::Delete),
            n.clone().prop_map(Op::Remove),
            (n.clone(), 0u8..3).prop_map(|(a, b)| Op::Title(a, b)),
            (n.clone(), 0u8..5).prop_map(|(a, b)| Op::Priority(a, b)),
            (n.clone(), any::<bool>()).prop_map(|(a, b)| Op::Status(a, b)),
            (n.clone(), proptest::option::of(1u32..6)).prop_map(|(a, b)| Op::Parent(a, b)),
            (n.clone(), 1u32..6, any::<bool>()).prop_map(|(a, b, c)| Op::Edge(a, b, c)),
            (n.clone(), -2i64..3).prop_map(|(a, b)| Op::Counter(a, b)),
            (n, proptest::option::of(0u8..3)).prop_map(|(a, b)| Op::Body(a, b)),
        ]
    }

    fn run(ops: &[Op]) -> State {
        let s = Schema::default();
        let mut st = State::default();
        for o in ops {
            match o {
                Op::Create(n) => {
                    st.nodes
                        .entry(Nid(*n))
                        .or_insert_with(|| node(*n, if n % 2 == 1 { "task" } else { "note" }));
                }
                Op::Delete(n) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*n)) {
                        x.tomb = Some(Tomb {
                            reason: Some("r".into()),
                            replaced_by: None,
                        });
                        x.body = None;
                    }
                }
                Op::Remove(n) => {
                    st.nodes.remove(&Nid(*n));
                }
                Op::Title(n, t) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*n)) {
                        x.set_field(&s, "title", Some(Value::Text(format!("t{t}"))));
                    }
                }
                Op::Priority(n, p) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*n)) {
                        x.set_field(&s, "priority", Some(Value::Enum(format!("P{p}"))));
                    }
                }
                Op::Status(n, d) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*n))
                        && x.kind == "task"
                    {
                        x.status = if *d { "done".into() } else { "open".into() };
                        x.resolution = if *d {
                            "completed".into()
                        } else {
                            "none".into()
                        };
                    }
                }
                Op::Parent(n, p) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*n)) {
                        x.parent = p.map(Nid);
                    }
                }
                Op::Edge(a, b, on) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*a)) {
                        let k = EdgeKey {
                            kind: "relates".into(),
                            dst: Nid(*b),
                            disc: None,
                        };
                        if *on {
                            x.out.insert(k, EdgeProps::default());
                        } else {
                            x.out.remove(&k);
                        }
                    }
                }
                Op::Counter(n, d) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*n))
                        && x.kind == "task"
                    {
                        let cur = match x.fields.get("reopen_count") {
                            Some(Value::Counter(c)) => *c,
                            _ => 0,
                        };
                        x.set_field(&s, "reopen_count", Some(Value::Counter(cur + d)));
                    }
                }
                Op::Body(n, b) => {
                    if let Some(x) = st.nodes.get_mut(&Nid(*n)) {
                        x.body = b.map(|b| format!("body {b}"));
                    }
                }
            }
        }
        st
    }

    proptest! {
        /// `apply(a, diff(a, b)) = b` for arbitrary pairs of states: the fold of the net changeset is exact.
        #[test]
        fn the_fold_of_the_diff_is_the_target(a in proptest::collection::vec(op(), 0..30), b in proptest::collection::vec(op(), 0..30)) {
            let sa = run(&a);
            let sb = run(&b);
            let cs = diff(&sa, &sb);
            let mut got = sa.clone();
            got.apply(&cs, &Fixed);
            prop_assert_eq!(&got, &sb);
            prop_assert!(diff(&sb, &got).is_empty());
            // The inverse changeset (a revert) returns to a.
            let inv: Changeset = cs.iter().map(|(k, (x, y))| (k.clone(), (y.clone(), x.clone()))).collect();
            let mut back = sb.clone();
            back.apply(&inv, &Fixed);
            prop_assert_eq!(&back, &sa);
        }
    }

    #[test]
    fn defaults_are_absent() {
        let s = Schema::default();
        let mut x = node(1, "task");
        x.set_field(&s, "priority", Some(Value::Enum("P2".into())));
        assert!(x.fields.is_empty());
        assert_eq!(x.get(&s, &Aspect::Status), None);
        x.set_field(&s, "priority", Some(Value::Enum("P1".into())));
        assert_eq!(
            x.get(&s, &Aspect::Field("priority".into())),
            Some(KVal::Value(Value::Enum("P1".into())))
        );
        x.set_field(&s, "title", Some(Value::Text(String::new())));
        assert!(!x.fields.contains_key("title"));
    }
}
