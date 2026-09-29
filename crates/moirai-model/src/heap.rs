//! The heap bytes the model's states, changesets and commits hold, by which WP-91 measures the `state_at` cache against
//! its bound ([PLAN] WP-91: "`state_at` memoised within ≤ 512 MB per case"; [`crate::dag`]).
//!
//! The crate instruments no allocator: it holds no `unsafe` code ([PLAN §2.1]), and only a composition root may reach
//! `moirai-os`'s counting allocator. The measure is computed from the types' layouts instead, as an upper bound up to
//! the allocator's own rounding: every owned buffer counts at its capacity, every boxed value at its size plus its own
//! heap, and every B-tree entry at [`BTREE_SLOT_NUM`]/[`BTREE_SLOT_DEN`] of its slot size plus a node header share —
//! the cost of a node filled to its minimum of 5 of its 11 slots, the worst a Rust B-tree leaves outside its root.

use crate::canon::Anchor;
use crate::dag::Commit;
use crate::schema::{Ends, Item, ItemKey};
use crate::state::{Aspect, Conflict, EdgeKey, EdgeProps, KState, KVal, Key, Node, State};
use crate::value::{Oid, PathVal, Value};
use std::collections::BTreeMap;
use std::mem::size_of;

/// A B-tree entry costs its slot times `BTREE_SLOT_NUM / BTREE_SLOT_DEN`: 11 slots per node, at least 5 of them used.
pub const BTREE_SLOT_NUM: usize = 11;
/// See [`BTREE_SLOT_NUM`].
pub const BTREE_SLOT_DEN: usize = 5;
/// A B-tree node's header (parent pointer, parent index, length) and its share of the edge array of an internal node,
/// per entry, rounded up.
pub const BTREE_ENTRY_EXTRA: usize = 8;

/// The heap bytes a value owns beyond its own inline size.
pub trait Heap {
    /// The owned heap bytes, an upper bound up to allocator rounding.
    fn heap(&self) -> usize;
}

/// The heap of a B-tree map: every entry's slot share and the heap its key and value own.
pub fn btree<K: Heap, V: Heap>(m: &BTreeMap<K, V>) -> usize {
    let slot =
        (size_of::<K>() + size_of::<V>()) * BTREE_SLOT_NUM / BTREE_SLOT_DEN + BTREE_ENTRY_EXTRA;
    m.iter().map(|(k, v)| slot + k.heap() + v.heap()).sum()
}

/// The heap of a vector: its buffer at capacity and every element's own heap.
pub fn vec<T: Heap>(v: &[T], capacity: usize) -> usize {
    capacity * size_of::<T>() + v.iter().map(Heap::heap).sum::<usize>()
}

macro_rules! no_heap {
    ($($t:ty),*) => {
        $(impl Heap for $t {
            fn heap(&self) -> usize {
                0
            }
        })*
    };
}

no_heap!(
    u8,
    u16,
    u32,
    u64,
    i64,
    bool,
    crate::value::Nid,
    crate::value::Uid
);

impl Heap for String {
    fn heap(&self) -> usize {
        self.capacity()
    }
}

impl<T: Heap> Heap for Option<T> {
    fn heap(&self) -> usize {
        self.as_ref().map_or(0, Heap::heap)
    }
}

impl<T: Heap> Heap for Vec<T> {
    fn heap(&self) -> usize {
        vec(self, self.capacity())
    }
}

impl<T: Heap> Heap for Box<T> {
    fn heap(&self) -> usize {
        size_of::<T>() + (**self).heap()
    }
}

impl<A: Heap, B: Heap> Heap for (A, B) {
    fn heap(&self) -> usize {
        self.0.heap() + self.1.heap()
    }
}

impl<K: Heap, V: Heap> Heap for BTreeMap<K, V> {
    fn heap(&self) -> usize {
        btree(self)
    }
}

impl Heap for PathVal {
    fn heap(&self) -> usize {
        self.root.heap() + self.text.heap()
    }
}

impl Heap for Oid {
    fn heap(&self) -> usize {
        self.digest.capacity()
    }
}

impl Heap for Value {
    fn heap(&self) -> usize {
        match self {
            Value::Enum(s) | Value::Text(s) => s.heap(),
            Value::Set(v) => v.heap(),
            Value::Path(p) => p.heap(),
            Value::Oid(o) => o.heap(),
            Value::PathMove(m) => {
                size_of::<crate::value::PathMove>() + m.from.heap() + m.to.heap() + m.git.heap()
            }
            Value::Bool(_)
            | Value::Int(_)
            | Value::Counter(_)
            | Value::F64(_)
            | Value::Ref(_)
            | Value::Commit(_) => 0,
        }
    }
}

impl Heap for Anchor {
    fn heap(&self) -> usize {
        self.kind.heap()
            + self.mode.heap()
            + self.watch.heap()
            + self.scope.capacity()
            + self.window.capacity()
            + self.blob.heap()
            + self.git.heap()
            + self.marker.heap()
    }
}

impl Heap for EdgeKey {
    fn heap(&self) -> usize {
        self.kind.heap()
    }
}

impl Heap for EdgeProps {
    fn heap(&self) -> usize {
        self.anchor.heap()
    }
}

impl Heap for Aspect {
    fn heap(&self) -> usize {
        match self {
            Aspect::Field(s) | Aspect::Counter(s) => s.heap(),
            Aspect::Edge(e) => e.heap(),
            Aspect::Existence
            | Aspect::Status
            | Aspect::Hierarchy
            | Aspect::Observation
            | Aspect::Body => 0,
        }
    }
}

impl Heap for ItemKey {
    fn heap(&self) -> usize {
        match self {
            ItemKey::Kind(a) | ItemKey::Edge(a) | ItemKey::Query(a) => a.heap(),
            ItemKey::Field(a, b) => a.heap() + b.heap(),
            ItemKey::Enum(a, b, c) => a.heap() + b.heap() + c.heap(),
        }
    }
}

impl Heap for Ends {
    fn heap(&self) -> usize {
        match self {
            Ends::Any => 0,
            Ends::Kinds(v) => v.heap(),
        }
    }
}

impl Heap for Item {
    fn heap(&self) -> usize {
        match self {
            Item::Kind(k) => k.name.heap(),
            Item::Field(f) => f.kind.heap() + f.name.heap() + f.default.heap(),
            Item::Enum(e) => e.kind.heap() + e.field.heap() + e.name.heap() + e.covers.heap(),
            Item::Edge(e) => {
                e.name.heap()
                    + e.lq_name.heap()
                    + e.src.heap()
                    + e.dst.heap()
                    + e.reverse.heap()
                    + e.reading.heap()
            }
            Item::Query(q) => {
                q.name.heap() + q.params.heap() + q.shape.heap() + q.budget.heap() + q.text.heap()
            }
        }
    }
}

impl Heap for KVal {
    fn heap(&self) -> usize {
        match self {
            KVal::Live(k) => k.heap(),
            KVal::Deleted { kind, reason, .. } => kind.heap() + reason.heap(),
            KVal::Status { status, resolution } => status.heap() + resolution.heap(),
            KVal::Hierarchy { order, .. } => order.heap(),
            KVal::Value(v) => v.heap(),
            KVal::Observation(v) => v.heap(),
            KVal::Body(b) => b.heap(),
            KVal::Edge(e) => e.heap(),
            KVal::Item(i) => i.heap(),
        }
    }
}

impl Heap for Conflict {
    fn heap(&self) -> usize {
        self.class.heap()
            + self.base.heap()
            + self.ours.heap()
            + self.theirs.heap()
            + self.images.iter().map(Heap::heap).sum::<usize>()
    }
}

impl Heap for KState {
    fn heap(&self) -> usize {
        match self {
            KState::Plain(v) => v.heap(),
            KState::Conflict(c) => c.heap(),
        }
    }
}

impl Heap for Key {
    fn heap(&self) -> usize {
        match self {
            Key::Node(_, a) => a.heap(),
            Key::Schema(i) => i.heap(),
        }
    }
}

impl Heap for Node {
    fn heap(&self) -> usize {
        self.kind.heap()
            + self.tomb.as_ref().map_or(0, |t| t.reason.heap())
            + self.status.heap()
            + self.resolution.heap()
            + self.order.heap()
            + self.fields.heap()
            + self.body.heap()
            + self.out.heap()
            + self.creator.actor.heap()
            + self.creator.role.heap()
            + self.conflicts.heap()
    }
}

impl Heap for State {
    fn heap(&self) -> usize {
        self.nodes.heap() + self.schema.items.heap() + self.schema_conflicts.heap()
    }
}

impl Heap for Commit {
    fn heap(&self) -> usize {
        self.parents.heap()
            + self.actor.heap()
            + self.actor_src.heap()
            + self.role.heap()
            + self.session.heap()
            + self.message.heap()
            + self.stmt_sym.heap()
            + self.changeset.heap()
            + self.affected.heap()
            + self.violations.heap()
            + self.resolves.heap()
    }
}

impl Heap for crate::merge::Violation {
    fn heap(&self) -> usize {
        self.key.heap() + self.description.heap() + self.suggested.heap()
    }
}

/// The bytes a state takes, inline and on the heap.
pub fn state_bytes(st: &State) -> usize {
    size_of::<State>() + st.heap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_vectors_count_their_capacity() {
        let mut s = String::with_capacity(40);
        s.push_str("abc");
        assert_eq!(s.heap(), 40);
        let v: Vec<String> = vec!["ab".into(), String::with_capacity(9)];
        assert_eq!(v.heap(), 2 * size_of::<String>() + 2 + 9);
        assert_eq!(Some(Box::new(7u64)).heap(), 8);
    }

    #[test]
    fn a_btree_entry_costs_its_slot_share_and_its_heap() {
        let mut m: BTreeMap<u64, String> = BTreeMap::new();
        assert_eq!(m.heap(), 0);
        m.insert(1, "xyz".to_string());
        let slot = (8 + size_of::<String>()) * BTREE_SLOT_NUM / BTREE_SLOT_DEN + BTREE_ENTRY_EXTRA;
        assert_eq!(m.heap(), slot + 3);
    }

    #[test]
    fn a_node_counts_its_fields_body_and_edges() {
        let mut st = State::default();
        let base = size_of::<State>();
        assert_eq!(state_bytes(&st), base);
        let n = Node::new(
            crate::value::Uid([1; 16]),
            "task",
            &st.schema,
            crate::state::Creator {
                actor: "a".into(),
                role: "developer".into(),
            },
        );
        st.nodes.insert(crate::value::Nid(1), n);
        let one = state_bytes(&st);
        assert!(one > base + size_of::<Node>(), "{one}");
        let x = st.nodes.get_mut(&crate::value::Nid(1)).unwrap();
        x.body = Some("b".repeat(1000));
        x.fields
            .insert("title".into(), Value::Text("t".repeat(100)));
        assert!(state_bytes(&st) >= one + 1100);
    }
}
