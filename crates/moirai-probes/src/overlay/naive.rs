//! The naive map the overlay is checked against: every key the subset can set mapped to its last value in log order,
//! the final set memberships over the base, `ALLOC` and `UIDX` rows, the newest `K_RT` row per key and the symbols,
//! all in ordered maps, with none of the overlay's indexes or `prev` walks. It reads each op's (key, value) pairs
//! itself ([`written`]), not through the overlay's [`sets`](super::sets), so a wrong value reading in the probe
//! disagrees with this map.

use super::{BitsetKey, DELETED_KEY, Key, Value, kind_key, kind_status_key};
use crate::overlay::Base;
use crate::tail::commit::Op;
use crate::tail::rec::{Payload, Record};
use std::collections::{BTreeMap, BTreeSet};

/// A node's existence and membership state.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum State {
    Absent,
    Live { kind: u8, status: Option<u8> },
    Deleted,
}

impl State {
    fn members(&self) -> BTreeSet<BitsetKey> {
        match *self {
            State::Absent => BTreeSet::new(),
            State::Deleted => [DELETED_KEY].into(),
            State::Live { kind, status } => {
                let mut s: BTreeSet<BitsetKey> = [kind_key(kind)].into();
                if let Some(st) = status {
                    s.insert(kind_status_key(kind, st));
                }
                s
            }
        }
    }
}

/// The naive map of one tail.
#[derive(Debug, Default)]
pub struct Naive {
    /// (owner, key) → the last value set in log order.
    pub values: BTreeMap<(u32, Key), Value>,
    states: BTreeMap<u32, State>,
    /// (`#N`, uid, `ref_id`, `create_seq`) of every `Create`, in log order.
    pub creates: Vec<(u32, [u8; 16], u32, u32)>,
    /// (`n`, `tree`) → (lsn, row index), the last row written.
    pub files: BTreeMap<(u32, [u8; 16]), (u64, u32)>,
    /// (`tree`, `scope_kind`, `scope_ref`) → lsn, the last epoch written.
    pub epochs: BTreeMap<([u8; 16], u8, u32), u64>,
    /// (class, id, text) in log order.
    pub symbols: Vec<(u8, u32, String)>,
    /// (owner, lsn) of every op, in log order.
    pub touches: Vec<(u32, u64)>,
}

fn base_state(base: &Base, id: u32) -> State {
    if id >= 1 && id <= base.n {
        State::Live {
            kind: base.kind,
            status: Some(base.status(id)),
        }
    } else {
        State::Absent
    }
}

impl Naive {
    /// Applies `records` in log order over `base`.
    pub fn of(records: &[Record<'_>], base: &Base) -> Naive {
        let mut n = Naive::default();
        for rec in records {
            for d in &rec.symdefs {
                n.symbols.push((d.class, d.id, d.text.to_string()));
            }
            match &rec.payload {
                Payload::Commit(c) => {
                    for op in &c.ops {
                        let id = op.owner();
                        n.touches.push((id, rec.lsn));
                        for (k, v) in written(op) {
                            n.values.insert((id, k), v);
                        }
                        let now = *n.states.entry(id).or_insert_with(|| base_state(base, id));
                        let next = match op {
                            Op::Create { kind, image, .. } => {
                                n.creates.push((id, uid_of(op), c.ref_id, c.seq as u32));
                                State::Live {
                                    kind: *kind,
                                    status: image.status.map(|s| s.0),
                                }
                            }
                            Op::Delete { .. } => State::Deleted,
                            Op::SetStatus { new, .. } => match now {
                                State::Live { kind, .. } => State::Live {
                                    kind,
                                    status: Some(new.0),
                                },
                                other => other,
                            },
                            _ => now,
                        };
                        n.states.insert(id, next);
                    }
                }
                Payload::TreeReg(t) => {
                    n.epochs
                        .insert((t.tree, t.epoch.scope_kind, t.epoch.scope_ref), rec.lsn);
                }
                Payload::FileObs(b) => {
                    for (i, r) in b.rows.iter().enumerate() {
                        n.files.insert(r.key(), (rec.lsn, i as u32));
                    }
                }
                Payload::ExtentHead(_) | Payload::Noop(_) => {}
            }
        }
        n
    }

    /// Per bitset key, the members of the final state not in the base (`plus`) and those of the base not in the final
    /// state (`minus`), each ascending; keys with neither are left out.
    pub fn plus_minus(&self, base: &Base) -> BTreeMap<BitsetKey, (Vec<u32>, Vec<u32>)> {
        let mut out: BTreeMap<BitsetKey, (Vec<u32>, Vec<u32>)> = BTreeMap::new();
        for (&id, state) in &self.states {
            let b = base_state(base, id).members();
            let f = state.members();
            for k in f.difference(&b) {
                out.entry(*k).or_default().0.push(id);
            }
            for k in b.difference(&f) {
                out.entry(*k).or_default().1.push(id);
            }
        }
        out
    }
}

/// The (key, value) pairs `op` leaves ([F06 §7.4]–§7.5), read here on its own: a `Create` its existence (live, of its
/// kind) and every entry of its image; a `Delete` its existence (deleted, with its `reason` and `replaced_by`);
/// `SetField`, `SetStatus`, `SetBody` and `Move` the new side of their key; an `AddEdge` the edge present and a
/// `RemoveEdge` the edge absent.
pub(super) fn written(op: &Op<'_>) -> Vec<(Key, Value)> {
    let mut out = Vec::new();
    match op {
        Op::Create { kind, image, .. } => {
            out.push((Key::Exist, Value::Live(*kind)));
            if let Some((status, resolution)) = image.status {
                out.push((Key::Status, Value::Status(status, resolution)));
            }
            if let Some(hash) = image.body {
                out.push((Key::Body, Value::Body(Some(hash))));
            }
            for (name, value) in &image.fields {
                out.push((Key::Field(*name), Value::Field(value.0.to_vec())));
            }
        }
        Op::Delete {
            reason,
            replaced_by,
            ..
        } => out.push((Key::Exist, Value::Deleted(*reason, *replaced_by))),
        Op::SetField { name, new, .. } => {
            out.push((Key::Field(*name), Value::Field(new.0.to_vec())));
        }
        Op::SetStatus {
            new: (status, resolution),
            ..
        } => out.push((Key::Status, Value::Status(*status, *resolution))),
        Op::SetBody { new, .. } => out.push((Key::Body, Value::Body(*new))),
        Op::AddEdge(e) => out.push((Key::Edge(e.ekind, e.dst), Value::Edge(true))),
        Op::RemoveEdge(e) => out.push((Key::Edge(e.ekind, e.dst), Value::Edge(false))),
        Op::Move {
            new_parent,
            new_order,
            ..
        } => out.push((Key::Hier, Value::Hier(*new_parent, new_order.to_string()))),
    }
    out
}

fn uid_of(op: &Op<'_>) -> [u8; 16] {
    match op {
        Op::Create { uid, .. } => *uid,
        _ => [0; 16],
    }
}
