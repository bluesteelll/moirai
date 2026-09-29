//! [F06] the `Commit` payload: the header with its presence bitmap (§4), keys, key values, conflict states and node
//! images (§6), every op (§7), carried bodies (§8) and the bulk-commit rules (§9). Every V-rule of §10 is checked on
//! decode; C-rules are not (they need the base state, §2.4), except BD-2's hash, which the oracle checks.

use crate::holes;
use crate::log::sym;
use crate::prim::{Oid, Reader, Result, Writer, blake3_128, err};
use crate::value::{EdgeProps, Item, ItemBody, Value, text_rules};

/// §3.2 import provenance values.
pub mod import {
    /// Written locally.
    pub const LOCAL: u8 = 0;
    /// Verified native import.
    pub const NATIVE: u8 = 1;
    /// Foreign import.
    pub const FOREIGN: u8 = 2;
    /// Checkpoint import.
    pub const CHECKPOINT: u8 = 3;
}

/// §3.1 commit kinds.
pub mod kind {
    /// `ordinary`.
    pub const ORDINARY: u8 = 0;
    /// `merge`.
    pub const MERGE: u8 = 1;
    /// `sync`.
    pub const SYNC: u8 = 2;
    /// `revert`.
    pub const REVERT: u8 = 3;
    /// `cherry-pick`.
    pub const CHERRY_PICK: u8 = 4;
    /// `checkpoint` (import-checkpoint).
    pub const CHECKPOINT: u8 = 5;
}

/// The frozen spelling of a commit kind ([F06 §3.1]).
pub fn kind_name(k: u8) -> Option<&'static str> {
    Some(match k {
        0 => "ordinary",
        1 => "merge",
        2 => "sync",
        3 => "revert",
        4 => "cherry-pick",
        5 => "checkpoint",
        _ => return None,
    })
}

/// A key in store-local form ([F06 §6.1]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CKey {
    /// 1.
    Existence(u32),
    /// 2.
    Status(u32),
    /// 3.
    Hierarchy(u32),
    /// 4: node, field name symbol.
    Field(u32, u32),
    /// 5.
    Observation(u32),
    /// 6: node, counter field name symbol.
    Counter(u32, u32),
    /// 7.
    Edge(EdgeKey),
    /// 8.
    Body(u32),
    /// 9: item class and item key bytes.
    Schema(u8, Vec<u8>),
}

/// The components of an edge key ([F06 §6.1] class 7, §7.5.1 orders 1, 3–6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EdgeKey {
    /// Source `#N`.
    pub src: u32,
    /// Edge kind code.
    pub ekind: u8,
    /// Destination `#N`.
    pub dst: u32,
    /// The discriminator (the anchor uid on an `at` edge).
    pub disc: Option<[u8; 16]>,
}

impl EdgeKey {
    fn decode_after_src(r: &mut Reader<'_>) -> Result<(u8, u32, Option<[u8; 16]>)> {
        let ekind = r.u8()?;
        let dst = r.uvar32()?;
        let at = r.offset();
        let dflag = r.u8()?;
        if dflag & 0xFE != 0 {
            return err(at, "dflag bits 1-7 are not zero [F06 §6.1]");
        }
        let disc = if dflag & 1 != 0 { Some(r.b16()?) } else { None };
        Ok((ekind, dst, disc))
    }

    fn encode_after_src(&self, w: &mut Writer) {
        w.u8(self.ekind);
        w.uvar(u64::from(self.dst));
        match &self.disc {
            Some(d) => {
                w.u8(1);
                w.bytes(d);
            }
            None => w.u8(0),
        }
    }

    fn detail(&self) -> Vec<u8> {
        let mut v = vec![self.ekind];
        v.extend_from_slice(&self.dst.to_be_bytes());
        v.push(u8::from(self.disc.is_some()));
        if let Some(d) = &self.disc {
            v.extend_from_slice(d);
        }
        v
    }
}

impl CKey {
    /// Decodes a `ckey`.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        Ok(match r.u8()? {
            1 => CKey::Existence(r.uvar32()?),
            2 => CKey::Status(r.uvar32()?),
            3 => CKey::Hierarchy(r.uvar32()?),
            4 => CKey::Field(r.uvar32()?, r.uvar32()?),
            5 => CKey::Observation(r.uvar32()?),
            6 => CKey::Counter(r.uvar32()?, r.uvar32()?),
            7 => {
                let src = r.uvar32()?;
                let (ekind, dst, disc) = EdgeKey::decode_after_src(r)?;
                CKey::Edge(EdgeKey {
                    src,
                    ekind,
                    dst,
                    disc,
                })
            }
            8 => CKey::Body(r.uvar32()?),
            9 => CKey::Schema(r.u8()?, r.vbytes()?.to_vec()),
            c => return err(at, format!("ckey class {c} is invalid [F06 §6.1]")),
        })
    }

    /// Encodes a `ckey`.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.class());
        match self {
            CKey::Existence(n)
            | CKey::Status(n)
            | CKey::Hierarchy(n)
            | CKey::Observation(n)
            | CKey::Body(n) => w.uvar(u64::from(*n)),
            CKey::Field(n, s) | CKey::Counter(n, s) => {
                w.uvar(u64::from(*n));
                w.uvar(u64::from(*s));
            }
            CKey::Edge(e) => {
                w.uvar(u64::from(e.src));
                e.encode_after_src(w);
            }
            CKey::Schema(c, k) => {
                w.u8(*c);
                w.vbytes(k);
            }
        }
    }

    /// The class value (and op rank, §7.9).
    pub fn class(&self) -> u8 {
        match self {
            CKey::Existence(_) => 1,
            CKey::Status(_) => 2,
            CKey::Hierarchy(_) => 3,
            CKey::Field(..) => 4,
            CKey::Observation(_) => 5,
            CKey::Counter(..) => 6,
            CKey::Edge(_) => 7,
            CKey::Body(_) => 8,
            CKey::Schema(..) => 9,
        }
    }

    /// The owner `#N` ([F06 §2.3]); `None` for a schema key.
    pub fn owner(&self) -> Option<u32> {
        match self {
            CKey::Existence(n)
            | CKey::Status(n)
            | CKey::Hierarchy(n)
            | CKey::Observation(n)
            | CKey::Body(n)
            | CKey::Field(n, _)
            | CKey::Counter(n, _) => Some(*n),
            CKey::Edge(e) => Some(e.src),
            CKey::Schema(..) => None,
        }
    }

    fn detail(&self) -> Vec<u8> {
        match self {
            CKey::Field(_, s) | CKey::Counter(_, s) => s.to_be_bytes().to_vec(),
            CKey::Edge(e) => e.detail(),
            CKey::Schema(c, k) => {
                let mut v = vec![*c];
                v.extend_from_slice(k);
                v
            }
            _ => Vec::new(),
        }
    }
}

/// A node image entry ([F06 §6.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageEntry {
    /// `ie` 0.
    Status(u8, u8),
    /// `ie` 1: the body hash.
    Body([u8; 16]),
    /// `ie` 2: field name symbol and a non-absent value.
    Field(u32, Value),
}

/// A node image ([F06 §6.3]).
pub type NodeImage = Vec<ImageEntry>;

/// Decodes a node image with its order rule (V).
pub fn decode_image(r: &mut Reader<'_>) -> Result<NodeImage> {
    let n = r.count(2)?;
    let mut v: NodeImage = Vec::with_capacity(n);
    let mut last: (u8, u32) = (0, 0);
    for i in 0..n {
        let at = r.offset();
        let e = match r.u8()? {
            0 => ImageEntry::Status(r.u8()?, r.u8()?),
            1 => ImageEntry::Body(r.b16()?),
            2 => {
                let name = r.uvar32()?;
                ImageEntry::Field(name, Value::decode(r, false)?)
            }
            t => return err(at, format!("node image entry tag {t} invalid [F06 §6.3]")),
        };
        let key = match &e {
            ImageEntry::Status(..) => (0, 0),
            ImageEntry::Body(_) => (1, 0),
            ImageEntry::Field(s, _) => (2, *s),
        };
        if i > 0 && key <= last {
            return err(
                at,
                "node image entries not in order status, body, fields by name [F06 §6.3]",
            );
        }
        last = key;
        v.push(e);
    }
    Ok(v)
}

/// Encodes a node image.
pub fn encode_image(img: &NodeImage, w: &mut Writer) {
    w.uvar(img.len() as u64);
    for e in img {
        match e {
            ImageEntry::Status(s, r) => {
                w.u8(0);
                w.u8(*s);
                w.u8(*r);
            }
            ImageEntry::Body(h) => {
                w.u8(1);
                w.bytes(h);
            }
            ImageEntry::Field(n, v) => {
                w.u8(2);
                w.uvar(u64::from(*n));
                v.encode(w);
            }
        }
    }
}

fn check_node_kind(k: u8, at: usize) -> Result<u8> {
    if (1..=13).contains(&k) || (64..=254).contains(&k) {
        Ok(k)
    } else {
        err(at, format!("node kind {k} invalid [F08 §3.3]"))
    }
}

/// An existence key value ([F06 §6.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExVal {
    /// 0.
    Absent,
    /// 1: kind and, with `snap`, the node image.
    Live(u8, Option<NodeImage>),
    /// 2: kind, reason symbol, replacement `#N`.
    Deleted(u8, u32, u32),
}

/// A key value ([F06 §6.2]); its shape follows the key class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KVal {
    /// `existence`.
    Existence(ExVal),
    /// `status`: absent or (status, resolution).
    Status(Option<(u8, u8)>),
    /// `field`, `counter`.
    Value(Value),
    /// `observation`: six values.
    Observation(Box<[Value; 6]>),
    /// `body`: absent or the hash.
    Body(Option<[u8; 16]>),
    /// `hierarchy`: parent `#N` (0 none) and order (empty none).
    Hierarchy(u32, String),
    /// `edge`: absent or a property block.
    Edge(Option<EdgeProps>),
    /// `schema`: absent or the item bytes, decoded.
    Schema(Option<(Vec<u8>, Item)>),
}

fn flag01(r: &mut Reader<'_>, what: &str) -> Result<bool> {
    let at = r.offset();
    match r.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        v => err(at, format!("{what} flag {v} is not 0 or 1 [F06 §6.2]")),
    }
}

/// Decodes an item carried as `vbytes` ([F06 §6.2], §7.6) and requires it to fill its bytes.
fn decode_item_bytes(r: &mut Reader<'_>) -> Result<(Vec<u8>, Item)> {
    let b = r.vbytes()?;
    let mut ir = Reader::with_base(b, r.offset() - b.len());
    let it = Item::decode(&mut ir)?;
    ir.finish("a schema item")?;
    Ok((b.to_vec(), it))
}

impl KVal {
    /// Decodes a key value of `class`.
    pub fn decode(r: &mut Reader<'_>, class: u8) -> Result<Self> {
        Ok(match class {
            1 => {
                let at = r.offset();
                KVal::Existence(match r.u8()? {
                    0 => ExVal::Absent,
                    1 => {
                        let k_at = r.offset();
                        let k = check_node_kind(r.u8()?, k_at)?;
                        let snap = flag01(r, "snap")?;
                        ExVal::Live(k, if snap { Some(decode_image(r)?) } else { None })
                    }
                    2 => {
                        let k_at = r.offset();
                        let k = check_node_kind(r.u8()?, k_at)?;
                        ExVal::Deleted(k, r.uvar32()?, r.uvar32()?)
                    }
                    v => return err(at, format!("existence value tag {v} invalid [F06 §6.2]")),
                })
            }
            2 => KVal::Status(if flag01(r, "status")? {
                Some((r.u8()?, r.u8()?))
            } else {
                None
            }),
            4 | 6 => KVal::Value(Value::decode(r, true)?),
            5 => {
                let mut v = Vec::with_capacity(6);
                for _ in 0..6 {
                    v.push(Value::decode(r, true)?);
                }
                KVal::Observation(Box::new(v.try_into().expect("six values")))
            }
            8 => KVal::Body(if flag01(r, "body")? {
                Some(r.b16()?)
            } else {
                None
            }),
            3 => {
                let p = r.uvar32()?;
                let at = r.offset();
                let o = r.vstr()?;
                text_rules(o, at, true)?;
                KVal::Hierarchy(p, o.to_owned())
            }
            7 => KVal::Edge(if flag01(r, "edge")? {
                Some(EdgeProps::decode(r)?)
            } else {
                None
            }),
            9 => KVal::Schema(if flag01(r, "schema")? {
                Some(decode_item_bytes(r)?)
            } else {
                None
            }),
            c => return err(r.offset(), format!("key class {c} invalid [F06 §6.1]")),
        })
    }

    /// Encodes a key value.
    pub fn encode(&self, w: &mut Writer) {
        match self {
            KVal::Existence(ExVal::Absent) => w.u8(0),
            KVal::Existence(ExVal::Live(k, img)) => {
                w.u8(1);
                w.u8(*k);
                match img {
                    Some(i) => {
                        w.u8(1);
                        encode_image(i, w);
                    }
                    None => w.u8(0),
                }
            }
            KVal::Existence(ExVal::Deleted(k, rs, rb)) => {
                w.u8(2);
                w.u8(*k);
                w.uvar(u64::from(*rs));
                w.uvar(u64::from(*rb));
            }
            KVal::Status(s) => match s {
                Some((a, b)) => {
                    w.u8(1);
                    w.u8(*a);
                    w.u8(*b);
                }
                None => w.u8(0),
            },
            KVal::Value(v) => v.encode(w),
            KVal::Observation(vs) => vs.iter().for_each(|v| v.encode(w)),
            KVal::Body(b) => match b {
                Some(h) => {
                    w.u8(1);
                    w.bytes(h);
                }
                None => w.u8(0),
            },
            KVal::Hierarchy(p, o) => {
                w.uvar(u64::from(*p));
                w.vstr(o);
            }
            KVal::Edge(e) => match e {
                Some(p) => {
                    w.u8(1);
                    p.encode(w);
                }
                None => w.u8(0),
            },
            KVal::Schema(s) => match s {
                Some((b, _)) => {
                    w.u8(1);
                    w.vbytes(b);
                }
                None => w.u8(0),
            },
        }
    }
}

/// The conflict part shared by `cstate` and the `Conflict` op ([F06 §6.2], §7.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictVal {
    /// Value-conflict class, 1–7 ([F12 §6.1]).
    pub class: u8,
    /// Base side.
    pub base: KVal,
    /// Ours (dst).
    pub ours: KVal,
    /// Theirs (src).
    pub theirs: KVal,
    /// Provisional side for an existence key.
    pub prov: Option<u8>,
}

impl ConflictVal {
    fn decode(r: &mut Reader<'_>, key_class: u8) -> Result<Self> {
        let at = r.offset();
        let class = r.u8()?;
        if !(1..=7).contains(&class) {
            return err(at, format!("conflict class {class} outside 1-7 [F12 §6.1]"));
        }
        let base = KVal::decode(r, key_class)?;
        let ours = KVal::decode(r, key_class)?;
        let theirs = KVal::decode(r, key_class)?;
        let prov = if key_class == 1 {
            let p_at = r.offset();
            let p = r.u8()?;
            if p > 1 {
                return err(p_at, "conflict prov is not 0 or 1 [F06 §6.2]");
            }
            Some(p)
        } else {
            None
        };
        Ok(ConflictVal {
            class,
            base,
            ours,
            theirs,
            prov,
        })
    }

    fn encode(&self, w: &mut Writer) {
        w.u8(self.class);
        self.base.encode(w);
        self.ours.encode(w);
        self.theirs.encode(w);
        if let Some(p) = self.prov {
            w.u8(p);
        }
    }
}

/// A `cstate` ([F06 §6.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CState {
    /// `cs` 0.
    Plain(KVal),
    /// `cs` 1.
    Conflict(Box<ConflictVal>),
}

impl CState {
    /// Decodes a `cstate` of a key of `key_class`.
    pub fn decode(r: &mut Reader<'_>, key_class: u8) -> Result<Self> {
        let at = r.offset();
        match r.u8()? {
            0 => Ok(CState::Plain(KVal::decode(r, key_class)?)),
            1 => Ok(CState::Conflict(Box::new(ConflictVal::decode(
                r, key_class,
            )?))),
            v => err(at, format!("cstate cs {v} is not 0 or 1 [F06 §6.2]")),
        }
    }

    /// Encodes a `cstate`.
    pub fn encode(&self, w: &mut Writer) {
        match self {
            CState::Plain(k) => {
                w.u8(0);
                k.encode(w);
            }
            CState::Conflict(c) => {
                w.u8(1);
                c.encode(w);
            }
        }
    }
}

/// One op ([F06 §7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// 1.
    Create {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Node uid.
        uid: [u8; 16],
        /// Node kind.
        kind: u8,
        /// `CREATOR` actor.
        c_actor: u32,
        /// `CREATOR` role.
        c_role: u16,
        /// End-of-commit image.
        image: NodeImage,
    },
    /// 2.
    Delete {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Reason symbol.
        reason: u32,
        /// Replacement `#N`.
        replaced_by: u32,
        /// Full before-image.
        before: NodeImage,
    },
    /// 3.
    Undelete {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Before-image reason.
        reason: u32,
        /// Before-image replacement.
        replaced_by: u32,
        /// End-of-commit image.
        image: NodeImage,
    },
    /// 4.
    SetField {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Field symbol.
        name: u32,
        /// Before-image.
        old: Value,
        /// New value.
        new: Value,
    },
    /// 5.
    SetStatus {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// (status, resolution) before.
        old: (u8, u8),
        /// (status, resolution) after.
        new: (u8, u8),
    },
    /// 6.
    Incr {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Counter symbol.
        name: u32,
        /// Net delta ≠ 0.
        delta: i64,
    },
    /// 7.
    SetBody {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Old body hash.
        old: Option<[u8; 16]>,
        /// New body hash.
        new: Option<[u8; 16]>,
    },
    /// 8 and 9 share this layout; `remove` tells them apart.
    Edge {
        /// true for `RemoveEdge`.
        remove: bool,
        /// Edge key.
        key: EdgeKey,
        /// §7.3.
        prev: u64,
        /// Props (new, or before-image for remove).
        props: EdgeProps,
        /// `aN` when the block carries an anchor.
        anchor_no: Option<u32>,
    },
    /// 10.
    SetEdgeProps {
        /// Edge key.
        key: EdgeKey,
        /// §7.3.
        prev: u64,
        /// Before-image.
        old: EdgeProps,
        /// New props.
        new: EdgeProps,
    },
    /// 11.
    Move {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Old (parent, order).
        old: (u32, String),
        /// New (parent, order).
        new: (u32, String),
    },
    /// 12.
    Schema {
        /// 0 weaken, 1 strengthen.
        mode: u8,
        /// Item class.
        item_class: u8,
        /// Item key bytes.
        item_key: Vec<u8>,
        /// Before-image item.
        old: Option<(Vec<u8>, Item)>,
        /// New item.
        new: Option<(Vec<u8>, Item)>,
    },
    /// 13.
    Conflict {
        /// The conflicted key.
        key: CKey,
        /// §7.3 when the key has an owner.
        prev: Option<u64>,
        /// Before-image.
        old: CState,
        /// The conflict value.
        value: Box<ConflictVal>,
    },
    /// 14.
    Violation {
        /// Structural class 64–127.
        class: u8,
        /// The key it is about.
        key: Option<CKey>,
        /// Detail text.
        description: String,
        /// Suggested statement.
        suggested: String,
        /// The raw body (the §7.9 detail).
        body: Vec<u8>,
    },
    /// 15.
    Resolve {
        /// The resolved key.
        key: CKey,
        /// §7.3 when the key has an owner.
        prev: Option<u64>,
        /// 0 ours … 4 repoint.
        choice: u8,
        /// Repoint target.
        target: Option<u32>,
        /// Before-image.
        old: CState,
        /// Resulting plain value.
        new: KVal,
    },
    /// 16.
    CreateDeleted {
        /// `#N`.
        id: u32,
        /// §7.3.
        prev: u64,
        /// Node uid.
        uid: [u8; 16],
        /// Node kind.
        kind: u8,
        /// `CREATOR` actor.
        c_actor: u32,
        /// `CREATOR` role.
        c_role: u16,
        /// Reason symbol.
        reason: u32,
        /// Replacement.
        replaced_by: u32,
        /// Retained value keys.
        image: NodeImage,
    },
}

fn role16(r: &mut Reader<'_>) -> Result<u16> {
    let at = r.offset();
    let v = r.uvar32()?;
    u16::try_from(v).or_else(|_| err(at, "role symbol above 65,535 [F06 §2.1]"))
}

fn opt_hash(present: bool, r: &mut Reader<'_>) -> Result<Option<[u8; 16]>> {
    if present {
        Ok(Some(r.b16()?))
    } else {
        Ok(None)
    }
}

fn decode_item_opt(present: bool, r: &mut Reader<'_>) -> Result<Option<(Vec<u8>, Item)>> {
    if present {
        Ok(Some(decode_item_bytes(r)?))
    } else {
        Ok(None)
    }
}

impl Op {
    /// The op tag ([F06 §7.2]).
    pub fn tag(&self) -> u8 {
        match self {
            Op::Create { .. } => 1,
            Op::Delete { .. } => 2,
            Op::Undelete { .. } => 3,
            Op::SetField { .. } => 4,
            Op::SetStatus { .. } => 5,
            Op::Incr { .. } => 6,
            Op::SetBody { .. } => 7,
            Op::Edge { remove: false, .. } => 8,
            Op::Edge { remove: true, .. } => 9,
            Op::SetEdgeProps { .. } => 10,
            Op::Move { .. } => 11,
            Op::Schema { .. } => 12,
            Op::Conflict { .. } => 13,
            Op::Violation { .. } => 14,
            Op::Resolve { .. } => 15,
            Op::CreateDeleted { .. } => 16,
        }
    }

    /// The key the op sets (none for `Violation`).
    pub fn key(&self) -> Option<CKey> {
        Some(match self {
            Op::Create { id, .. }
            | Op::Delete { id, .. }
            | Op::Undelete { id, .. }
            | Op::CreateDeleted { id, .. } => CKey::Existence(*id),
            Op::SetField { id, name, .. } => CKey::Field(*id, *name),
            Op::SetStatus { id, .. } => CKey::Status(*id),
            Op::Incr { id, name, .. } => CKey::Counter(*id, *name),
            Op::SetBody { id, .. } => CKey::Body(*id),
            Op::Edge { key, .. } | Op::SetEdgeProps { key, .. } => CKey::Edge(key.clone()),
            Op::Move { id, .. } => CKey::Hierarchy(*id),
            Op::Schema {
                item_class,
                item_key,
                ..
            } => CKey::Schema(*item_class, item_key.clone()),
            Op::Conflict { key, .. } | Op::Resolve { key, .. } => key.clone(),
            Op::Violation { .. } => return None,
        })
    }

    /// The `prev` the op carries, if any.
    pub fn prev(&self) -> Option<u64> {
        match self {
            Op::Create { prev, .. }
            | Op::Delete { prev, .. }
            | Op::Undelete { prev, .. }
            | Op::SetField { prev, .. }
            | Op::SetStatus { prev, .. }
            | Op::Incr { prev, .. }
            | Op::SetBody { prev, .. }
            | Op::Edge { prev, .. }
            | Op::SetEdgeProps { prev, .. }
            | Op::Move { prev, .. }
            | Op::CreateDeleted { prev, .. } => Some(*prev),
            Op::Conflict { prev, .. } | Op::Resolve { prev, .. } => *prev,
            Op::Schema { .. } | Op::Violation { .. } => None,
        }
    }

    /// The §7.9 sort key: (owner, rank, detail).
    pub fn order_key(&self) -> (u32, u8, Vec<u8>) {
        match self {
            Op::Violation { key, body, .. } => (
                key.as_ref().and_then(CKey::owner).unwrap_or(0),
                10,
                body.clone(),
            ),
            _ => {
                let k = self.key().expect("keyed op");
                (k.owner().unwrap_or(0), k.class(), k.detail())
            }
        }
    }

    /// The `aN` an anchor-creating op carries.
    pub fn anchor_no(&self) -> Option<u32> {
        match self {
            Op::Edge {
                remove: false,
                anchor_no,
                ..
            } => *anchor_no,
            _ => None,
        }
    }

    /// Decodes one op frame and its body ([F06 §7.1]).
    pub fn decode(r: &mut Reader<'_>) -> Result<Op> {
        let at = r.offset();
        let tag = r.u8()?;
        let len = r.uvar32()? as usize;
        let mut b = r.sub(len)?;
        let body_raw = b.rest().to_vec();
        let op = Self::decode_body(tag, &mut b, at, body_raw)?;
        b.finish("an op body")?;
        Ok(op)
    }

    fn decode_body(tag: u8, r: &mut Reader<'_>, at: usize, raw: Vec<u8>) -> Result<Op> {
        Ok(match tag {
            1 => {
                let id = r.uvar32()?;
                let prev = r.uvar64()?;
                let uid = r.b16()?;
                let k_at = r.offset();
                let kind = check_node_kind(r.u8()?, k_at)?;
                Op::Create {
                    id,
                    prev,
                    uid,
                    kind,
                    c_actor: r.uvar32()?,
                    c_role: role16(r)?,
                    image: decode_image(r)?,
                }
            }
            2 => Op::Delete {
                id: r.uvar32()?,
                prev: r.uvar64()?,
                reason: r.uvar32()?,
                replaced_by: r.uvar32()?,
                before: decode_image(r)?,
            },
            3 => Op::Undelete {
                id: r.uvar32()?,
                prev: r.uvar64()?,
                reason: r.uvar32()?,
                replaced_by: r.uvar32()?,
                image: decode_image(r)?,
            },
            4 => {
                let id = r.uvar32()?;
                let prev = r.uvar64()?;
                let name = r.uvar32()?;
                let o_at = r.offset();
                let old = Value::decode(r, true)?;
                let new = Value::decode(r, true)?;
                if old == new {
                    return err(o_at, "SetField old equals new [F06 §7.4]");
                }
                Op::SetField {
                    id,
                    prev,
                    name,
                    old,
                    new,
                }
            }
            5 => {
                let id = r.uvar32()?;
                let prev = r.uvar64()?;
                let o_at = r.offset();
                let old = (r.u8()?, r.u8()?);
                let new = (r.u8()?, r.u8()?);
                if old == new {
                    return err(o_at, "SetStatus pairs are equal [F06 §7.4]");
                }
                Op::SetStatus { id, prev, old, new }
            }
            6 => {
                let id = r.uvar32()?;
                let prev = r.uvar64()?;
                let name = r.uvar32()?;
                let d_at = r.offset();
                let delta = r.svar64()?;
                if delta == 0 {
                    return err(d_at, "Incr delta is 0 [F06 §7.4]");
                }
                Op::Incr {
                    id,
                    prev,
                    name,
                    delta,
                }
            }
            7 => {
                let id = r.uvar32()?;
                let prev = r.uvar64()?;
                let f_at = r.offset();
                let bflags = r.u8()?;
                if bflags & 0xFC != 0 || bflags == 0 {
                    return err(f_at, "SetBody bflags reserved bits set or 0 [F06 §7.4]");
                }
                let old = opt_hash(bflags & 1 != 0, r)?;
                let new = opt_hash(bflags & 2 != 0, r)?;
                if old.is_some() && old == new {
                    return err(f_at, "SetBody new equals old [F06 §7.4]");
                }
                Op::SetBody { id, prev, old, new }
            }
            8 | 9 => {
                let src = r.uvar32()?;
                let prev = r.uvar64()?;
                let (ekind, dst, disc) = EdgeKey::decode_after_src(r)?;
                let p_at = r.offset();
                let props = EdgeProps::decode(r)?;
                let has_anchor = props.pflags & 4 != 0;
                if has_anchor != disc.is_some() {
                    return err(
                        p_at,
                        "pflags anchor bit differs from dflag bit 0 [F06 §7.5.1]",
                    );
                }
                if let (Some(d), Some(a)) = (&disc, &props.anchor)
                    && a.uid != *d
                {
                    return err(p_at, "anchor record uid differs from disc [F06 §7.5.1]");
                }
                let anchor_no = if has_anchor {
                    let n_at = r.offset();
                    let n = r.uvar32()?;
                    if n == 0 {
                        return err(n_at, "anchor_no 0 [F06 §2.2]");
                    }
                    Some(n)
                } else {
                    None
                };
                Op::Edge {
                    remove: tag == 9,
                    key: EdgeKey {
                        src,
                        ekind,
                        dst,
                        disc,
                    },
                    prev,
                    props,
                    anchor_no,
                }
            }
            10 => {
                let src = r.uvar32()?;
                let prev = r.uvar64()?;
                let (ekind, dst, disc) = EdgeKey::decode_after_src(r)?;
                let o_at = r.offset();
                let old = EdgeProps::decode(r)?;
                let new = EdgeProps::decode(r)?;
                if old == new {
                    return err(o_at, "SetEdgeProps new equals old [F06 §7.5.1]");
                }
                if let Some(d) = &disc {
                    match (&old.anchor, &new.anchor) {
                        (Some(a), Some(b))
                            if a.uid == *d
                                && b.uid == *d
                                && a.captured == b.captured
                                && a.pred == b.pred => {}
                        _ => {
                            return err(
                                o_at,
                                "at-edge SetEdgeProps anchors differ in uid, captured or pred [F06 §7.5.1]",
                            );
                        }
                    }
                } else if old.anchor.is_some() || new.anchor.is_some() {
                    return err(
                        o_at,
                        "anchor record on an edge without a discriminator [F06 §7.5.1]",
                    );
                }
                Op::SetEdgeProps {
                    key: EdgeKey {
                        src,
                        ekind,
                        dst,
                        disc,
                    },
                    prev,
                    old,
                    new,
                }
            }
            11 => {
                let id = r.uvar32()?;
                let prev = r.uvar64()?;
                let op_ = r.uvar32()?;
                let np = r.uvar32()?;
                let o_at = r.offset();
                let oo = r.vstr()?.to_owned();
                let no = r.vstr()?.to_owned();
                text_rules(&oo, o_at, true)?;
                text_rules(&no, o_at, true)?;
                if (op_, &oo) == (np, &no) {
                    return err(o_at, "Move pairs are equal [F06 §7.4]");
                }
                Op::Move {
                    id,
                    prev,
                    old: (op_, oo),
                    new: (np, no),
                }
            }
            12 => {
                let m_at = r.offset();
                let mode = r.u8()?;
                if mode > 1 {
                    return err(m_at, "Schema mode outside 0-1 [F06 §7.6]");
                }
                let c_at = r.offset();
                let item_class = r.u8()?;
                if !(1..=5).contains(&item_class) {
                    return err(c_at, "Schema item_class outside 1-5 [F08 §8.5]");
                }
                let k_at = r.offset();
                let item_key = r.vbytes()?.to_vec();
                if item_class == 5 {
                    crate::prim::utf8(&item_key, k_at)?;
                }
                let f_at = r.offset();
                let sflags = r.u8()?;
                if sflags & 0xFC != 0 || sflags == 0 {
                    return err(f_at, "Schema sflags reserved bits set or 0 [F06 §7.6]");
                }
                let old = decode_item_opt(sflags & 1 != 0, r)?;
                let new = decode_item_opt(sflags & 2 != 0, r)?;
                for (_, it) in old.iter().chain(new.iter()) {
                    if it.class() != item_class {
                        return err(f_at, "Schema item class differs from item_class [F06 §7.6]");
                    }
                }
                Op::Schema {
                    mode,
                    item_class,
                    item_key,
                    old,
                    new,
                }
            }
            13 => {
                let key = CKey::decode(r)?;
                let prev = if key.owner().is_some() {
                    Some(r.uvar64()?)
                } else {
                    None
                };
                let old = CState::decode(r, key.class())?;
                let value = Box::new(ConflictVal::decode(r, key.class())?);
                Op::Conflict {
                    key,
                    prev,
                    old,
                    value,
                }
            }
            14 => {
                let c_at = r.offset();
                let class = r.u8()?;
                if !(64..=127).contains(&class) {
                    return err(c_at, "Violation class outside 64-127 [F12 §6.1]");
                }
                let f_at = r.offset();
                let vflags = r.u8()?;
                if vflags & 0xFE != 0 {
                    return err(f_at, "Violation vflags bits 1-7 are not zero [F06 §7.7]");
                }
                let key = if vflags & 1 != 0 {
                    Some(CKey::decode(r)?)
                } else {
                    None
                };
                Op::Violation {
                    class,
                    key,
                    description: r.vstr()?.to_owned(),
                    suggested: r.vstr()?.to_owned(),
                    body: raw,
                }
            }
            15 => {
                let key = CKey::decode(r)?;
                let prev = if key.owner().is_some() {
                    Some(r.uvar64()?)
                } else {
                    None
                };
                let c_at = r.offset();
                let choice = r.u8()?;
                if choice > 4 {
                    return err(c_at, "Resolve choice outside 0-4 [F06 §7.7]");
                }
                let target = if choice == 4 { Some(r.uvar32()?) } else { None };
                let old = CState::decode(r, key.class())?;
                let new = KVal::decode(r, key.class())?;
                Op::Resolve {
                    key,
                    prev,
                    choice,
                    target,
                    old,
                    new,
                }
            }
            16 => {
                let id = r.uvar32()?;
                let prev = r.uvar64()?;
                let uid = r.b16()?;
                let k_at = r.offset();
                let kind = check_node_kind(r.u8()?, k_at)?;
                Op::CreateDeleted {
                    id,
                    prev,
                    uid,
                    kind,
                    c_actor: r.uvar32()?,
                    c_role: role16(r)?,
                    reason: r.uvar32()?,
                    replaced_by: r.uvar32()?,
                    image: decode_image(r)?,
                }
            }
            t => return err(at, format!("op tag {t} invalid [F06 §7.2]")),
        })
    }

    /// Encodes the op frame.
    pub fn encode(&self, w: &mut Writer) {
        let mut b = Writer::new();
        self.encode_body(&mut b);
        w.u8(self.tag());
        w.uvar(b.len() as u64);
        w.bytes(b.as_slice());
    }

    fn encode_body(&self, w: &mut Writer) {
        let u = |w: &mut Writer, v: u32| w.uvar(u64::from(v));
        match self {
            Op::Create {
                id,
                prev,
                uid,
                kind,
                c_actor,
                c_role,
                image,
            } => {
                u(w, *id);
                w.uvar(*prev);
                w.bytes(uid);
                w.u8(*kind);
                u(w, *c_actor);
                w.uvar(u64::from(*c_role));
                encode_image(image, w);
            }
            Op::Delete {
                id,
                prev,
                reason,
                replaced_by,
                before: image,
            }
            | Op::Undelete {
                id,
                prev,
                reason,
                replaced_by,
                image,
            } => {
                u(w, *id);
                w.uvar(*prev);
                u(w, *reason);
                u(w, *replaced_by);
                encode_image(image, w);
            }
            Op::SetField {
                id,
                prev,
                name,
                old,
                new,
            } => {
                u(w, *id);
                w.uvar(*prev);
                u(w, *name);
                old.encode(w);
                new.encode(w);
            }
            Op::SetStatus { id, prev, old, new } => {
                u(w, *id);
                w.uvar(*prev);
                w.u8(old.0);
                w.u8(old.1);
                w.u8(new.0);
                w.u8(new.1);
            }
            Op::Incr {
                id,
                prev,
                name,
                delta,
            } => {
                u(w, *id);
                w.uvar(*prev);
                u(w, *name);
                w.svar(*delta);
            }
            Op::SetBody { id, prev, old, new } => {
                u(w, *id);
                w.uvar(*prev);
                w.u8(u8::from(old.is_some()) | (u8::from(new.is_some()) << 1));
                if let Some(h) = old {
                    w.bytes(h);
                }
                if let Some(h) = new {
                    w.bytes(h);
                }
            }
            Op::Edge {
                key,
                prev,
                props,
                anchor_no,
                ..
            } => {
                u(w, key.src);
                w.uvar(*prev);
                key.encode_after_src(w);
                props.encode(w);
                if let Some(n) = anchor_no {
                    u(w, *n);
                }
            }
            Op::SetEdgeProps {
                key,
                prev,
                old,
                new,
            } => {
                u(w, key.src);
                w.uvar(*prev);
                key.encode_after_src(w);
                old.encode(w);
                new.encode(w);
            }
            Op::Move { id, prev, old, new } => {
                u(w, *id);
                w.uvar(*prev);
                u(w, old.0);
                u(w, new.0);
                w.vstr(&old.1);
                w.vstr(&new.1);
            }
            Op::Schema {
                mode,
                item_class,
                item_key,
                old,
                new,
            } => {
                w.u8(*mode);
                w.u8(*item_class);
                w.vbytes(item_key);
                w.u8(u8::from(old.is_some()) | (u8::from(new.is_some()) << 1));
                for (b, _) in old.iter().chain(new.iter()) {
                    w.vbytes(b);
                }
            }
            Op::Conflict {
                key,
                prev,
                old,
                value,
            } => {
                key.encode(w);
                if let Some(p) = prev {
                    w.uvar(*p);
                }
                old.encode(w);
                value.encode(w);
            }
            Op::Violation { body, .. } => w.bytes(body),
            Op::Resolve {
                key,
                prev,
                choice,
                target,
                old,
                new,
            } => {
                key.encode(w);
                if let Some(p) = prev {
                    w.uvar(*p);
                }
                w.u8(*choice);
                if let Some(t) = target {
                    u(w, *t);
                }
                old.encode(w);
                new.encode(w);
            }
            Op::CreateDeleted {
                id,
                prev,
                uid,
                kind,
                c_actor,
                c_role,
                reason,
                replaced_by,
                image,
            } => {
                u(w, *id);
                w.uvar(*prev);
                w.bytes(uid);
                w.u8(*kind);
                u(w, *c_actor);
                w.uvar(u64::from(*c_role));
                u(w, *reason);
                u(w, *replaced_by);
                encode_image(image, w);
            }
        }
    }
}

/// [F06 §7.8], §7.9 V-rules over a record's op list: NF-1, NF-4, NF-5, the op order and one `prev` per owner.
pub fn check_ops(ops: &[Op], at: usize) -> Result<()> {
    use std::collections::{HashMap, HashSet};
    let mut keys = HashSet::new();
    let mut prev_of: HashMap<u32, u64> = HashMap::new();
    let mut folded: HashSet<u32> = HashSet::new();
    for op in ops {
        if let Some(k) = op.key()
            && !keys.insert(k)
        {
            return err(at, "two ops set one key [F06 §7.8 NF-1]");
        }
        if let Op::Create { id, .. } | Op::Undelete { id, .. } | Op::Delete { id, .. } = op {
            folded.insert(*id);
        }
        if let (Some(p), Some(owner)) = (op.prev(), op.key().and_then(|k| k.owner()))
            && let Some(q) = prev_of.insert(owner, p)
            && q != p
        {
            return err(at, "ops of one owner carry different prev [F06 §7.3]");
        }
    }
    for op in ops {
        if let Op::SetField { id, .. }
        | Op::SetStatus { id, .. }
        | Op::Incr { id, .. }
        | Op::SetBody { id, .. } = op
            && folded.contains(id)
        {
            return err(
                at,
                "a value op of a node its record creates, undeletes or deletes [F06 §7.8 NF-4, NF-5]",
            );
        }
    }
    for w in ops.windows(2) {
        let (a, b) = (w[0].order_key(), w[1].order_key());
        let violations = w[0].tag() == 14 && w[1].tag() == 14;
        if a > b || (a == b && !violations) {
            return err(at, "ops out of (owner, rank, detail) order [F06 §7.9]");
        }
    }
    Ok(())
}

/// A body carried by a commit ([F06 §8]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyEntry {
    /// BLAKE3-128 of the raw bytes.
    pub hash: [u8; 16],
    /// Codec byte ([F10 §3.1]).
    pub codec: u8,
    /// Raw length.
    pub raw_len: u32,
    /// Raw bytes (codec none) or an opaque codec payload.
    pub data: Vec<u8>,
}

/// The git provenance group ([F06 §4.4.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitProv {
    /// Object format, 0 only without digests.
    pub algo: u8,
    /// Git HEAD of the caller's tree.
    pub head: Option<Oid>,
    /// Symbol, class `git-branch`.
    pub branch: u32,
    /// Symbol, class `git-worktree`.
    pub worktree: u32,
    /// The lane's base commit.
    pub base: Option<Oid>,
}

/// The `ckpt` group ([F06 §4.4.11]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ckpt {
    /// `Moirai-Head`.
    pub head: [u8; 32],
    /// `Moirai-Folded` count.
    pub n_folded: u32,
    /// First folded commit.
    pub first: [u8; 32],
    /// Last folded commit.
    pub last: [u8; 32],
}

/// The `xtr` group ([F06 §4.4.12]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Xtr {
    /// `Moirai-Ref` symbol.
    pub x_ref: Option<u32>,
    /// `Moirai-Idem` value.
    pub x_idem: Option<[u8; 16]>,
}

/// A `prov` value of `ckimg` ([F06 §4.4.14]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prov {
    /// The commit id.
    pub commit: [u8; 32],
    /// The `rfc3339ms` text, possibly empty.
    pub time: String,
}

/// One `ckimg` entry ([F06 §4.4.14]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CkimgEntry {
    /// The node `#N`.
    pub id: u32,
    /// `created:`.
    pub created: Option<Prov>,
    /// `updated:`.
    pub updated: Option<Prov>,
    /// `deleted:`.
    pub deleted: Option<Prov>,
    /// Ledger lines (field symbol, delta, token).
    pub ledger: Vec<(u32, i64, String)>,
}

/// The `Commit` payload ([F06 §4.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// Presence bitmap.
    pub presence: u32,
    /// BLAKE3-256 of the canonical form.
    pub commit_id: [u8; 32],
    /// Actual parents (id16, lsn).
    pub parents: Vec<([u8; 16], u64)>,
    /// Stated ids by parent index.
    pub stated: Vec<Option<[u8; 32]>>,
    /// Generation.
    pub gen_: u32,
    /// Store-wide sequence number.
    pub seq: u64,
    /// Ref name symbol.
    pub ref_sym: u32,
    /// Ref id.
    pub ref_id: u32,
    /// Replaced tip.
    pub ref_old: Option<[u8; 16]>,
    /// Previous commit on the ref.
    pub prev_on_ref: Option<u64>,
    /// Position on the ref chain.
    pub ref_seq: u32,
    /// §3.1.
    pub kind: u8,
    /// §3.2.
    pub import: u8,
    /// HLC.
    pub hlc: u64,
    /// Actor symbol.
    pub actor: u32,
    /// Role symbol.
    pub role: u16,
    /// Session symbol.
    pub session: u32,
    /// Schema version.
    pub schema_version: u32,
    /// Git provenance.
    pub git: Option<GitProv>,
    /// Foreign git object.
    pub foreign_git: Option<Oid>,
    /// Reverted or cherry-picked commit.
    pub origin: Option<[u8; 32]>,
    /// (idem_key, idem_payload).
    pub idem: Option<([u8; 16], [u8; 16])>,
    /// Absorbed src commit.
    pub sync_base: Option<[u8; 16]>,
    /// Absorbed vector.
    pub absorbed: Option<Vec<(u32, u32)>>,
    /// Verified flag.
    pub verified: Option<bool>,
    /// Checkpoint origin.
    pub ckpt: Option<Ckpt>,
    /// Imported informational trailers.
    pub xtr: Option<Xtr>,
    /// §3.4.
    pub stmt_origin: u8,
    /// §3.5.
    pub actor_src: u8,
    /// Statement symbol.
    pub stmt_sym: u32,
    /// Statement hash.
    pub stmt_hash: Option<[u8; 16]>,
    /// `append_hlc − hlc`.
    pub append_delta: i64,
    /// Message.
    pub msg: Option<String>,
    /// (ids, complete).
    pub affected: Option<(Vec<u32>, bool)>,
    /// Canonical item 10 digest.
    pub changeset_digest: [u8; 32],
    /// Bulk changeset file (file, len, b3).
    pub cs_ref: Option<(u32, u64, [u8; 16])>,
    /// Ops.
    pub ops: Vec<Op>,
    /// Carried bodies.
    pub bodies: Vec<BodyEntry>,
    /// Import-checkpoint image-only data.
    pub ckimg: Option<Vec<CkimgEntry>>,
}

fn bit(p: u32, b: u32) -> bool {
    p & (1 << b) != 0
}

/// Decodes one `ckimg` entry with its V-rules ([F06 §4.4.14]); also the row of [F09 §16.4] `CKIMG`.
pub fn decode_ckimg_entry(r: &mut Reader<'_>) -> Result<CkimgEntry> {
    let id = r.uvar32()?;
    let f_at = r.offset();
    let f = r.u8()?;
    if f & 0xF8 != 0 || f == 0 || (f & 4 != 0 && f & 3 != 0) {
        return err(f_at, "ckimg iflags invalid [F06 §4.4.14]");
    }
    let created = if f & 1 != 0 {
        Some(decode_prov(r)?)
    } else {
        None
    };
    let updated = if f & 2 != 0 {
        Some(decode_prov(r)?)
    } else {
        None
    };
    let deleted = if f & 4 != 0 {
        Some(decode_prov(r)?)
    } else {
        None
    };
    let nl = r.count(3)?;
    let mut ledger = Vec::with_capacity(nl);
    for _ in 0..nl {
        let l_at = r.offset();
        let field = r.uvar32()?;
        let delta = r.svar64()?;
        let token = r.vstr()?;
        if delta == 0 || token.is_empty() {
            return err(l_at, "ckimg ledger delta 0 or empty token [F06 §4.4.14]");
        }
        ledger.push((field, delta, token.to_owned()));
    }
    Ok(CkimgEntry {
        id,
        created,
        updated,
        deleted,
        ledger,
    })
}

/// Encodes one `ckimg` entry.
pub fn encode_ckimg_entry(e: &CkimgEntry, w: &mut Writer) {
    w.uvar(u64::from(e.id));
    w.u8(u8::from(e.created.is_some())
        | (u8::from(e.updated.is_some()) << 1)
        | (u8::from(e.deleted.is_some()) << 2));
    for p in [&e.created, &e.updated, &e.deleted].into_iter().flatten() {
        w.bytes(&p.commit);
        w.vstr(&p.time);
    }
    w.uvar(e.ledger.len() as u64);
    for (f, d, t) in &e.ledger {
        w.uvar(u64::from(*f));
        w.svar(*d);
        w.vstr(t);
    }
}

fn decode_prov(r: &mut Reader<'_>) -> Result<Prov> {
    Ok(Prov {
        commit: r.b32()?,
        time: r.vstr()?.to_owned(),
    })
}

impl Commit {
    /// Decodes a whole payload, requiring it to be consumed exactly ([F06 §4.1]).
    pub fn decode(r: &mut Reader<'_>) -> Result<Commit> {
        let start = r.offset();
        let presence = r.u32()?;
        if presence >> 18 != 0 {
            return err(start, "presence bits 18-31 are not zero [F06 §4.2]");
        }
        let commit_id = r.b32()?;
        let np_at = r.offset();
        let n_parents = usize::from(r.u8()?);
        if n_parents > 2 {
            return err(np_at, "n_parents above 2 [F06 §4.3]");
        }
        let mut parents = Vec::with_capacity(n_parents);
        for _ in 0..n_parents {
            parents.push((r.b16()?, r.uvar64()?));
        }
        let mut stated = vec![None; n_parents];
        if bit(presence, 2) {
            let at = r.offset();
            let mask = r.u8()?;
            if mask == 0 || usize::from(mask) >> n_parents != 0 {
                return err(
                    at,
                    "stated_mask is 0 or has bits at or above n_parents [F06 §4.3]",
                );
            }
            for (i, s) in stated.iter_mut().enumerate() {
                if mask & (1 << i) != 0 {
                    *s = Some(r.b32()?);
                }
            }
        }
        let gen_ = r.uvar32()?;
        let s_at = r.offset();
        let seq = r.uvar64()?;
        if seq > u64::from(u32::MAX) {
            return err(s_at, "seq above 2^32 - 1 [F06 §4.4.3]");
        }
        let ref_sym = r.uvar32()?;
        let ref_id = r.uvar32()?;
        let ref_old = if bit(presence, 0) {
            Some(r.b16()?)
        } else {
            None
        };
        let prev_on_ref = if bit(presence, 1) {
            Some(r.uvar64()?)
        } else {
            None
        };
        let ref_seq = r.uvar32()?;
        let k_at = r.offset();
        let kind = r.u8()?;
        if kind > 5 {
            return err(k_at, "commit kind outside 0-5 [F06 §3.1]");
        }
        let import = r.u8()?;
        if import > 3 {
            return err(k_at + 1, "import provenance outside 0-3 [F06 §3.2]");
        }
        let hlc = r.u64()?;
        let actor = r.uvar32()?;
        let role = role16(r)?;
        let session = r.uvar32()?;
        let schema_version = r.uvar32()?;
        let git = if bit(presence, 3) {
            let at = r.offset();
            let algo = r.u8()?;
            if algo > 2 {
                return err(at, "git algo outside 0-2 [F06 §4.4.6]");
            }
            let gflags = r.u8()?;
            if gflags & 0xFC != 0 || (algo == 0 && gflags != 0) {
                return err(
                    at + 1,
                    "git gflags reserved bits set, or a digest with algo 0 [F06 §4.4.6]",
                );
            }
            let dig = |r: &mut Reader<'_>| -> Result<Oid> {
                let a = crate::prim::Algo::from_byte(algo, at)?;
                r.digest(a)
            };
            let head = if gflags & 1 != 0 { Some(dig(r)?) } else { None };
            let branch = r.uvar32()?;
            let worktree = r.uvar32()?;
            let base = if gflags & 2 != 0 { Some(dig(r)?) } else { None };
            Some(GitProv {
                algo,
                head,
                branch,
                worktree,
                base,
            })
        } else {
            None
        };
        let foreign_git = if bit(presence, 4) {
            let at = r.offset();
            let o = r.oidv()?;
            if o == Oid::None {
                return err(at, "foreign_git algo is none [F06 §4.3]");
            }
            Some(o)
        } else {
            None
        };
        let origin = if bit(presence, 5) {
            Some(r.b32()?)
        } else {
            None
        };
        let idem = if bit(presence, 6) {
            Some((r.b16()?, r.b16()?))
        } else {
            None
        };
        let sync_base = if bit(presence, 7) {
            Some(r.b16()?)
        } else {
            None
        };
        let absorbed = if bit(presence, 8) {
            let at = r.offset();
            let n = r.uvar32()?;
            if n == 0 || n > 65_535 {
                return err(at, "n_absorbed outside 1-65535 [F06 §4.4.9]");
            }
            let mut v: Vec<(u32, u32)> = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let e_at = r.offset();
                let e = (r.uvar32()?, r.uvar32()?);
                if e.1 == 0 || e.0 == ref_id || v.last().is_some_and(|p| p.0 >= e.0) {
                    return err(
                        e_at,
                        "absorbed entries unsorted, duplicated, ref_seq 0 or own ref [F06 §4.4.9]",
                    );
                }
                v.push(e);
            }
            Some(v)
        } else {
            None
        };
        let verified = if bit(presence, 9) {
            Some(r.bool8()?)
        } else {
            None
        };
        let ckpt = if bit(presence, 10) {
            let at = r.offset();
            let c = Ckpt {
                head: r.b32()?,
                n_folded: r.uvar32()?,
                first: r.b32()?,
                last: r.b32()?,
            };
            let zero = c.first == [0; 32] && c.last == [0; 32];
            let any_zero = c.first == [0; 32] || c.last == [0; 32];
            if (c.n_folded == 0) != zero || (c.n_folded != 0 && any_zero) {
                return err(
                    at,
                    "ckpt first/last zero exactly when n_folded is 0 [F06 §4.4.11]",
                );
            }
            Some(c)
        } else {
            None
        };
        let xtr = if bit(presence, 11) {
            let at = r.offset();
            let xf = r.u8()?;
            if xf & 0xFC != 0 || xf == 0 {
                return err(at, "xtr xflags reserved bits set or 0 [F06 §4.4.12]");
            }
            Some(Xtr {
                x_ref: if xf & 1 != 0 { Some(r.uvar32()?) } else { None },
                x_idem: if xf & 2 != 0 { Some(r.b16()?) } else { None },
            })
        } else {
            None
        };
        let so_at = r.offset();
        let stmt_origin = r.u8()?;
        if stmt_origin > 6 {
            return err(so_at, "stmt_origin outside 0-6 [F06 §3.4]");
        }
        let actor_src = r.u8()?;
        if actor_src > 6 {
            return err(so_at + 1, "actor_src outside 0-6 [F06 §3.5]");
        }
        let stmt_sym = r.uvar32()?;
        let stmt_hash = if bit(presence, 12) {
            Some(r.b16()?)
        } else {
            None
        };
        let d_at = r.offset();
        let append_delta = r.svar64()?;
        if i128::from(hlc) + i128::from(append_delta) < 0
            || i128::from(hlc) + i128::from(append_delta) > i128::from(u64::MAX)
        {
            return err(d_at, "append_hlc outside [0, 2^64 - 1] [F06 §4.4.5]");
        }
        let msg = if bit(presence, 13) {
            let at = r.offset();
            let m = r.vstr()?;
            if m.is_empty() || m.len() > 65_535 {
                return err(at, "msg length outside 1-65535 [F06 §4.3]");
            }
            Some(m.to_owned())
        } else {
            None
        };
        let affected = if bit(presence, 14) {
            let n = r.count(1)?;
            let complete = r.bool8()?;
            let mut ids: Vec<u32> = Vec::with_capacity(n);
            if n >= 1 {
                let at = r.offset();
                let first = r.uvar32()?;
                if first == 0 {
                    return err(at, "affected id 0 [F06 §2.2]");
                }
                ids.push(first);
                for _ in 1..n {
                    let g_at = r.offset();
                    let g = r.uvar32()?;
                    let last = *ids.last().expect("non-empty");
                    if g == 0 || u64::from(last) + u64::from(g) > u64::from(u32::MAX) {
                        return err(g_at, "affected gap 0 or id above 2^32 - 1 [F06 §4.4.13]");
                    }
                    ids.push(last + g);
                }
            }
            Some((ids, complete))
        } else {
            None
        };
        let changeset_digest = r.b32()?;
        let cs_ref = if bit(presence, 15) {
            Some((r.uvar32()?, r.uvar64()?, r.b16()?))
        } else {
            None
        };
        let ops_at = r.offset();
        let n_ops = r.count(2)?;
        let mut ops = Vec::with_capacity(n_ops);
        for _ in 0..n_ops {
            ops.push(Op::decode(r)?);
        }
        let n_bodies = r.count(18)?;
        let mut bodies: Vec<BodyEntry> = Vec::with_capacity(n_bodies);
        for _ in 0..n_bodies {
            let e_at = r.offset();
            let hash = r.b16()?;
            let codec = r.u8()?;
            if codec > holes::CODEC_MAX {
                return err(
                    e_at + 16,
                    format!("codec byte {codec} outside the registry [F10 §3.1]"),
                );
            }
            let raw_len = r.uvar32()?;
            let data = r.vbytes()?.to_vec();
            if bodies.last().is_some_and(|p| p.hash >= hash) {
                return err(
                    e_at,
                    "bodies not sorted by hash, or duplicated [F06 §8 BD-1]",
                );
            }
            if codec == 0 {
                if data.len() != raw_len as usize {
                    return err(
                        e_at,
                        "codec none body with len(data) != raw_len [F06 §8 BD-2]",
                    );
                }
                if blake3_128(&data) != hash {
                    return err(
                        e_at,
                        "body hash is not BLAKE3-128 of its raw bytes [F06 §8 BD-2]",
                    );
                }
            }
            bodies.push(BodyEntry {
                hash,
                codec,
                raw_len,
                data,
            });
        }
        let ckimg = if bit(presence, 17) {
            let at = r.offset();
            let n = r.uvar32()?;
            if n == 0 {
                return err(at, "ckimg n_files is 0 [F06 §4.4.14]");
            }
            let mut v: Vec<CkimgEntry> = Vec::new();
            for _ in 0..n {
                let e_at = r.offset();
                let e = decode_ckimg_entry(r)?;
                if v.last().is_some_and(|p| p.id >= e.id) {
                    return err(e_at, "ckimg entries not ascending by id [F06 §4.4.14]");
                }
                v.push(e);
            }
            Some(v)
        } else {
            None
        };
        r.finish("a Commit payload")?;
        let c = Commit {
            presence,
            commit_id,
            parents,
            stated,
            gen_,
            seq,
            ref_sym,
            ref_id,
            ref_old,
            prev_on_ref,
            ref_seq,
            kind,
            import,
            hlc,
            actor,
            role,
            session,
            schema_version,
            git,
            foreign_git,
            origin,
            idem,
            sync_base,
            absorbed,
            verified,
            ckpt,
            xtr,
            stmt_origin,
            actor_src,
            stmt_sym,
            stmt_hash,
            append_delta,
            msg,
            affected,
            changeset_digest,
            cs_ref,
            ops,
            bodies,
            ckimg,
        };
        c.check_v(start)?;
        check_ops(&c.ops, ops_at)?;
        Ok(c)
    }

    /// The V-rules of §3.3, §4.2, §4.4 that relate fields to each other.
    fn check_v(&self, at: usize) -> Result<()> {
        let p = self.presence;
        let np = self.parents.len();
        let fail = |m: &str| err(at, format!("{m} [F06 §3.3, §4.2]"));
        match (self.import, self.kind) {
            (import::LOCAL, kind::ORDINARY) if np <= 1 => {}
            (import::LOCAL, kind::MERGE | kind::SYNC) if np == 2 => {}
            (import::LOCAL, kind::REVERT | kind::CHERRY_PICK) if np == 1 => {}
            (import::NATIVE, kind::ORDINARY | kind::CHECKPOINT) if np <= 1 => {}
            (import::NATIVE, kind::MERGE | kind::SYNC) if np == 2 => {}
            (import::NATIVE, kind::REVERT | kind::CHERRY_PICK) if np == 1 => {}
            (import::FOREIGN, kind::ORDINARY) if np <= 1 => {}
            (import::FOREIGN, kind::MERGE) if np == 2 => {}
            (import::CHECKPOINT, kind::CHECKPOINT) if np <= 1 => {}
            _ => return fail("import, kind and n_parents do not form a valid combination"),
        }
        let fg = bit(p, 4);
        let fg_ok = match self.import {
            import::LOCAL => !fg,
            import::NATIVE => true,
            _ => fg,
        };
        if !fg_ok {
            return fail("foreign_git presence breaks the import rule");
        }
        let rc = matches!(self.kind, kind::REVERT | kind::CHERRY_PICK);
        if bit(p, 5) != rc {
            return fail("origin present exactly for revert and cherry-pick");
        }
        let ms = matches!(self.kind, kind::MERGE | kind::SYNC);
        if bit(p, 7) != ms {
            return fail("sync_base present exactly for merge and sync");
        }
        if bit(p, 8) && !ms {
            return fail("absorbed only for merge and sync");
        }
        if let Some(sb) = &self.sync_base
            && (np != 2 || self.parents[1].0 != *sb)
        {
            return fail("sync_base differs from parents[1].id16");
        }
        match (self.import, self.verified) {
            (import::NATIVE, Some(true)) | (import::FOREIGN, Some(false) | None) => {}
            (import::LOCAL | import::CHECKPOINT, None) => {}
            _ => return fail("verified present and valued only as the import allows"),
        }
        if bit(p, 10) != (self.kind == kind::CHECKPOINT) {
            return fail("ckpt present exactly for kind checkpoint");
        }
        if bit(p, 11) && !matches!(self.import, import::NATIVE | import::CHECKPOINT) {
            return fail("xtr only for native and checkpoint imports");
        }
        if bit(p, 6) && self.import != import::LOCAL {
            return fail("idem only for local commits");
        }
        let sh = bit(p, 12);
        let sh_ok = match self.stmt_origin {
            0 => true,
            1..=3 => sh,
            _ => !sh,
        };
        if !sh_ok {
            return fail("stmt_hash presence breaks the stmt_origin table [F06 §3.4]");
        }
        if (self.import != import::LOCAL) != (self.stmt_origin == 5) {
            return fail("stmt_origin import exactly for import != local [F06 §3.4]");
        }
        if self.import == import::LOCAL && self.append_delta != 0 {
            return fail("a local commit has append_delta != 0 [F06 §4.4.5]");
        }
        if (bit(p, 15) || bit(p, 16)) && (!self.ops.is_empty() || !self.bodies.is_empty()) {
            return fail("a bulk or pruned commit carries ops or bodies [F06 §9 BK-1, §4.4.15]");
        }
        if bit(p, 16) && (bit(p, 15) || bit(p, 17)) {
            return fail("a pruned commit with cs_ref or ckimg [F06 §4.4.15]");
        }
        if bit(p, 17) && (self.kind != kind::CHECKPOINT || bit(p, 15)) {
            return fail("ckimg only on an inline checkpoint [F06 §4.2]");
        }
        Ok(())
    }

    /// `append_hlc` = `hlc` + `append_delta` ([F06 §4.4.5]).
    pub fn append_hlc(&self) -> u64 {
        (i128::from(self.hlc) + i128::from(self.append_delta)) as u64
    }

    /// Re-encodes the payload.
    pub fn encode(&self, w: &mut Writer) {
        let u = |w: &mut Writer, v: u32| w.uvar(u64::from(v));
        w.u32(self.presence);
        w.bytes(&self.commit_id);
        w.u8(self.parents.len() as u8);
        for (id, lsn) in &self.parents {
            w.bytes(id);
            w.uvar(*lsn);
        }
        if bit(self.presence, 2) {
            let mask = self
                .stated
                .iter()
                .enumerate()
                .fold(0u8, |m, (i, s)| m | (u8::from(s.is_some()) << i));
            w.u8(mask);
            for s in self.stated.iter().flatten() {
                w.bytes(s);
            }
        }
        u(w, self.gen_);
        w.uvar(self.seq);
        u(w, self.ref_sym);
        u(w, self.ref_id);
        if let Some(x) = &self.ref_old {
            w.bytes(x);
        }
        if let Some(x) = self.prev_on_ref {
            w.uvar(x);
        }
        u(w, self.ref_seq);
        w.u8(self.kind);
        w.u8(self.import);
        w.u64(self.hlc);
        u(w, self.actor);
        w.uvar(u64::from(self.role));
        u(w, self.session);
        u(w, self.schema_version);
        if let Some(g) = &self.git {
            w.u8(g.algo);
            w.u8(u8::from(g.head.is_some()) | (u8::from(g.base.is_some()) << 1));
            if let Some(h) = &g.head {
                w.digest(h);
            }
            u(w, g.branch);
            u(w, g.worktree);
            if let Some(b) = &g.base {
                w.digest(b);
            }
        }
        if let Some(o) = &self.foreign_git {
            w.oidv(o);
        }
        if let Some(o) = &self.origin {
            w.bytes(o);
        }
        if let Some((k, pl)) = &self.idem {
            w.bytes(k);
            w.bytes(pl);
        }
        if let Some(s) = &self.sync_base {
            w.bytes(s);
        }
        if let Some(v) = &self.absorbed {
            w.uvar(v.len() as u64);
            for (a, b) in v {
                u(w, *a);
                u(w, *b);
            }
        }
        if let Some(v) = self.verified {
            w.bool8(v);
        }
        if let Some(c) = &self.ckpt {
            w.bytes(&c.head);
            u(w, c.n_folded);
            w.bytes(&c.first);
            w.bytes(&c.last);
        }
        if let Some(x) = &self.xtr {
            w.u8(u8::from(x.x_ref.is_some()) | (u8::from(x.x_idem.is_some()) << 1));
            if let Some(s) = x.x_ref {
                u(w, s);
            }
            if let Some(i) = &x.x_idem {
                w.bytes(i);
            }
        }
        w.u8(self.stmt_origin);
        w.u8(self.actor_src);
        u(w, self.stmt_sym);
        if let Some(h) = &self.stmt_hash {
            w.bytes(h);
        }
        w.svar(self.append_delta);
        if let Some(m) = &self.msg {
            w.vstr(m);
        }
        if let Some((ids, complete)) = &self.affected {
            w.uvar(ids.len() as u64);
            w.bool8(*complete);
            let mut last = 0u32;
            for (i, id) in ids.iter().enumerate() {
                u(w, if i == 0 { *id } else { id - last });
                last = *id;
            }
        }
        w.bytes(&self.changeset_digest);
        if let Some((f, l, b)) = &self.cs_ref {
            u(w, *f);
            w.uvar(*l);
            w.bytes(b);
        }
        w.uvar(self.ops.len() as u64);
        for op in &self.ops {
            op.encode(w);
        }
        w.uvar(self.bodies.len() as u64);
        for b in &self.bodies {
            w.bytes(&b.hash);
            w.u8(b.codec);
            u(w, b.raw_len);
            w.vbytes(&b.data);
        }
        if let Some(v) = &self.ckimg {
            w.uvar(v.len() as u64);
            for e in v {
                encode_ckimg_entry(e, w);
            }
        }
    }

    /// Every symbol reference of the payload as (class code of [F05 §8.1], id), for SD-3: the header (`ref`, `actor`,
    /// `role`, `session`, `stmt`, the git provenance, `xtr`), every op's own symbols ([`Op::symbol_refs`]) and the
    /// `ckimg` ledger fields. Id 0 ("none") is not a reference.
    pub fn symbol_refs(&self) -> Vec<(u8, u32)> {
        let mut v = vec![
            (sym::REF, self.ref_sym),
            (sym::ACTOR, self.actor),
            (sym::ROLE, u32::from(self.role)),
            (sym::SESSION, self.session),
            (sym::STMT, self.stmt_sym),
        ];
        if let Some(g) = &self.git {
            v.push((sym::GIT_BRANCH, g.branch));
            v.push((sym::GIT_WORKTREE, g.worktree));
        }
        if let Some(Xtr { x_ref: Some(s), .. }) = &self.xtr {
            v.push((sym::REF, *s));
        }
        for op in &self.ops {
            op.symbol_refs(&mut v);
        }
        if let Some(c) = &self.ckimg {
            for e in c {
                for (f, _, _) in &e.ledger {
                    v.push((sym::NAME, *f));
                }
            }
        }
        v.retain(|&(_, id)| id != 0);
        v
    }
}

/// The symbol references of a stored value ([F08 §5.1]–§5.2): a `sym` is class `text`, a `path`'s and a `pathmove`'s
/// roots are class `root`, and a set's elements are walked.
pub fn value_symbol_refs(v: &Value, out: &mut Vec<(u8, u32)>) {
    match v {
        Value::Sym(s) => out.push((sym::TEXT, *s)),
        Value::Path(p) => out.push((sym::ROOT, u32::from(p.root))),
        Value::PathMove(m) => {
            out.push((sym::ROOT, u32::from(m.from.root)));
            out.push((sym::ROOT, u32::from(m.to.root)));
        }
        Value::Set(_, xs) => xs.iter().for_each(|x| value_symbol_refs(x, out)),
        _ => {}
    }
}

/// The symbol references of a node image ([F06 §6.3]): each field entry's name (class `name`) and value.
pub fn image_symbol_refs(img: &NodeImage, out: &mut Vec<(u8, u32)>) {
    for e in img {
        if let ImageEntry::Field(name, v) = e {
            out.push((sym::NAME, *name));
            value_symbol_refs(v, out);
        }
    }
}

/// The symbol references of a schema item ([F08 §8.5]): every name it holds is class `name` (0 is `*` or none), and a
/// field item's default value.
pub fn item_symbol_refs(it: &Item, out: &mut Vec<(u8, u32)>) {
    match &it.body {
        ItemBody::Kind { name, .. } | ItemBody::Query { name, .. } => out.push((sym::NAME, *name)),
        ItemBody::Field {
            kind,
            name,
            default,
            ..
        } => {
            out.push((sym::NAME, *kind));
            out.push((sym::NAME, *name));
            if let Some(d) = default {
                value_symbol_refs(d, out);
            }
        }
        ItemBody::EnumValue {
            kind, field, name, ..
        } => {
            out.extend([(sym::NAME, *kind), (sym::NAME, *field), (sym::NAME, *name)]);
        }
        ItemBody::EdgeKind(e) => {
            out.push((sym::NAME, e.name));
            out.push((sym::NAME, e.lq_name));
            out.extend(e.reverse_names.iter().map(|n| (sym::NAME, *n)));
        }
    }
}

impl CKey {
    /// The symbol references of a key ([F06 §6.1]): the field or counter name of classes 4 and 6 (class `name`). A
    /// schema key's `item_key` holds name strings, not symbol ids ([F08 §8.5]).
    pub fn symbol_refs(&self, out: &mut Vec<(u8, u32)>) {
        if let CKey::Field(_, s) | CKey::Counter(_, s) = self {
            out.push((sym::NAME, *s));
        }
    }
}

impl KVal {
    /// The symbol references of a key value ([F06 §6.2]): an existence value's node image or `Deleted` reason (class
    /// `reason`), the stored values of field, counter and observation keys, and a schema item.
    pub fn symbol_refs(&self, out: &mut Vec<(u8, u32)>) {
        match self {
            KVal::Existence(ExVal::Live(_, Some(img))) => image_symbol_refs(img, out),
            KVal::Existence(ExVal::Deleted(_, reason, _)) => out.push((sym::REASON, *reason)),
            KVal::Value(v) => value_symbol_refs(v, out),
            KVal::Observation(vs) => vs.iter().for_each(|v| value_symbol_refs(v, out)),
            KVal::Schema(Some((_, it))) => item_symbol_refs(it, out),
            _ => {}
        }
    }
}

impl ConflictVal {
    fn symbol_refs(&self, out: &mut Vec<(u8, u32)>) {
        for side in [&self.base, &self.ours, &self.theirs] {
            side.symbol_refs(out);
        }
    }
}

impl CState {
    fn symbol_refs(&self, out: &mut Vec<(u8, u32)>) {
        match self {
            CState::Plain(k) => k.symbol_refs(out),
            CState::Conflict(c) => c.symbol_refs(out),
        }
    }
}

impl Op {
    /// The symbol references an op carries ([F05 §8.1] SD-3): `CREATOR`, reasons, field and counter names, every stored
    /// value and node image (before-images included), schema items, and the keys and sides of conflicts, violations and
    /// resolutions.
    pub fn symbol_refs(&self, out: &mut Vec<(u8, u32)>) {
        match self {
            Op::Create {
                c_actor,
                c_role,
                image,
                ..
            } => {
                out.push((sym::ACTOR, *c_actor));
                out.push((sym::ROLE, u32::from(*c_role)));
                image_symbol_refs(image, out);
            }
            Op::CreateDeleted {
                c_actor,
                c_role,
                reason,
                image,
                ..
            } => {
                out.push((sym::ACTOR, *c_actor));
                out.push((sym::ROLE, u32::from(*c_role)));
                out.push((sym::REASON, *reason));
                image_symbol_refs(image, out);
            }
            Op::Delete { reason, before, .. } => {
                out.push((sym::REASON, *reason));
                image_symbol_refs(before, out);
            }
            Op::Undelete { reason, image, .. } => {
                out.push((sym::REASON, *reason));
                image_symbol_refs(image, out);
            }
            Op::SetField { name, old, new, .. } => {
                out.push((sym::NAME, *name));
                value_symbol_refs(old, out);
                value_symbol_refs(new, out);
            }
            Op::Incr { name, .. } => out.push((sym::NAME, *name)),
            Op::Schema { old, new, .. } => {
                for (_, it) in old.iter().chain(new.iter()) {
                    item_symbol_refs(it, out);
                }
            }
            Op::Conflict {
                key, old, value, ..
            } => {
                key.symbol_refs(out);
                old.symbol_refs(out);
                value.symbol_refs(out);
            }
            Op::Violation { key, .. } => {
                if let Some(k) = key {
                    k.symbol_refs(out);
                }
            }
            Op::Resolve { key, old, new, .. } => {
                key.symbol_refs(out);
                old.symbol_refs(out);
                new.symbol_refs(out);
            }
            Op::SetStatus { .. }
            | Op::SetBody { .. }
            | Op::Edge { .. }
            | Op::SetEdgeProps { .. }
            | Op::Move { .. } => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// [F06 §11]: the informative `claim --start` commit, 205 payload bytes, with digests filled as 0xC1…/0xB2….
    pub(super) fn example_payload() -> Vec<u8> {
        let mut b = vec![0x43, 0x70, 0x00, 0x00];
        b.extend_from_slice(&[0xC1; 32]);
        b.push(0x01);
        b.extend_from_slice(&[0xB2; 16]);
        b.extend_from_slice(&[0xB4, 0x24]);
        b.push(0x07);
        b.extend_from_slice(&[0xF7, 0x22]);
        b.extend_from_slice(&[0x01, 0x00]);
        b.extend_from_slice(&[0xB2; 16]);
        b.extend_from_slice(&[0xB4, 0x24]);
        b.push(0x07);
        b.extend_from_slice(&[0x00, 0x00]);
        b.extend_from_slice(&[0x03, 0x00, 0x00, 0x6C, 0x50, 0xC4, 0xA0, 0x01]);
        b.extend_from_slice(&[0x05, 0x02, 0x03]);
        b.push(0x01);
        b.extend_from_slice(&[0x4B; 16]);
        b.extend_from_slice(&[0x51; 16]);
        b.extend_from_slice(&[0x01, 0x01, 0x04]);
        b.extend_from_slice(&[0x53; 16]);
        b.push(0x00);
        b.push(0x0D);
        b.extend_from_slice(b"claim --start");
        b.extend_from_slice(&[0x01, 0x01, 0x0C]);
        b.extend_from_slice(&[0xD4; 32]);
        b.push(0x01);
        b.extend_from_slice(&[0x05, 0x07, 0x0C, 0xC0, 0x25, 0x00, 0x00, 0x01, 0x00]);
        b.push(0x00);
        b
    }

    /// [F06 §11]: the worked payload decodes to the fields the example names.
    #[test]
    fn example_commit() {
        let b = example_payload();
        assert_eq!(b.len(), 205, "the payload is 205 bytes [F06 §11]");
        let mut r = Reader::new(&b);
        let c = Commit::decode(&mut r).unwrap();
        assert_eq!(c.presence, 0x7043);
        assert_eq!(c.seq, 4471);
        assert_eq!(c.parents[0].1, 4660);
        assert_eq!(c.hlc, 0x01A0_C450_6C00_0003);
        assert_eq!(c.msg.as_deref(), Some("claim --start"));
        assert_eq!(c.affected, Some((vec![12], true)));
        assert_eq!(
            c.ops,
            vec![Op::SetStatus {
                id: 12,
                prev: 4800,
                old: (0, 0),
                new: (1, 0)
            }]
        );
        let mut w = Writer::new();
        c.encode(&mut w);
        assert_eq!(w.as_slice(), &b[..]);
        let mut bad = b.clone();
        bad[3] = 0x04;
        assert!(Commit::decode(&mut Reader::new(&bad)).is_err());
    }

    pub(crate) fn base_commit() -> Commit {
        Commit {
            presence: 0,
            commit_id: [1; 32],
            parents: vec![([2; 16], 138)],
            stated: vec![None],
            gen_: 2,
            seq: 1,
            ref_sym: 1,
            ref_id: 0,
            ref_old: None,
            prev_on_ref: None,
            ref_seq: 1,
            kind: kind::ORDINARY,
            import: import::LOCAL,
            hlc: 1 << 16,
            actor: 1,
            role: 0,
            session: 0,
            schema_version: 1,
            git: None,
            foreign_git: None,
            origin: None,
            idem: None,
            sync_base: None,
            absorbed: None,
            verified: None,
            ckpt: None,
            xtr: None,
            stmt_origin: 2,
            actor_src: 0,
            stmt_sym: 0,
            stmt_hash: Some([9; 16]),
            append_delta: 0,
            msg: None,
            affected: None,
            changeset_digest: [3; 32],
            cs_ref: None,
            ops: Vec::new(),
            bodies: Vec::new(),
            ckimg: None,
        }
    }

    fn round_trip(c: &Commit) -> Commit {
        let mut w = Writer::new();
        c.encode(&mut w);
        let mut r = Reader::new(w.as_slice());
        let d = Commit::decode(&mut r).unwrap();
        let mut w2 = Writer::new();
        d.encode(&mut w2);
        assert_eq!(w.as_slice(), w2.as_slice());
        d
    }

    /// Every op kind in one record, in §7.9 order, with a carried body (BD-2 hash) and presence bit 12.
    #[test]
    fn every_op_round_trip() {
        let body = b"hello\n".to_vec();
        let h = blake3_128(&body);
        let mut c = base_commit();
        c.presence = 1 << 12;
        c.ops = vec![
            Op::Schema {
                mode: 0,
                item_class: 5,
                item_key: b"q1".to_vec(),
                old: None,
                new: Some({
                    let it = Item {
                        iflags: 0,
                        body: crate::value::ItemBody::Query {
                            name: 3,
                            lq_version: 1,
                            params: String::new(),
                            shape: "rows".into(),
                            budget: "small".into(),
                            text: "RETURN 1".into(),
                            ast_hash: [0; 16],
                        },
                    };
                    let mut w = Writer::new();
                    it.encode(&mut w);
                    (w.into_vec(), it)
                }),
            },
            Op::Violation {
                class: 64,
                key: None,
                description: "x".into(),
                suggested: String::new(),
                body: vec![64, 0, 1, b'x', 0],
            },
            Op::Create {
                id: 5,
                prev: 0,
                uid: [7; 16],
                kind: 1,
                c_actor: 1,
                c_role: 0,
                image: vec![
                    ImageEntry::Status(0, 0),
                    ImageEntry::Body(h),
                    ImageEntry::Field(4, Value::Text("t".into())),
                ],
            },
            Op::Move {
                id: 5,
                prev: 0,
                old: (0, String::new()),
                new: (6, "V".into()),
            },
            Op::Edge {
                remove: false,
                key: EdgeKey {
                    src: 5,
                    ekind: 2,
                    dst: 6,
                    disc: None,
                },
                prev: 0,
                props: EdgeProps {
                    pflags: 0,
                    pinned_commit: None,
                    anchor: None,
                },
                anchor_no: None,
            },
            Op::SetField {
                id: 6,
                prev: 20,
                name: 8,
                old: Value::Absent,
                new: Value::Int(3),
            },
            Op::Incr {
                id: 6,
                prev: 20,
                name: 9,
                delta: -2,
            },
            Op::Conflict {
                key: CKey::Body(6),
                prev: Some(20),
                old: CState::Plain(KVal::Body(None)),
                value: Box::new(ConflictVal {
                    class: 3,
                    base: KVal::Body(None),
                    ours: KVal::Body(Some([1; 16])),
                    theirs: KVal::Body(Some([2; 16])),
                    prov: None,
                }),
            },
            Op::Delete {
                id: 7,
                prev: 30,
                reason: 2,
                replaced_by: 0,
                before: vec![ImageEntry::Status(1, 0)],
            },
            Op::CreateDeleted {
                id: 8,
                prev: 0,
                uid: [8; 16],
                kind: 3,
                c_actor: 1,
                c_role: 1,
                reason: 0,
                replaced_by: 0,
                image: vec![],
            },
            Op::Resolve {
                key: CKey::Status(9),
                prev: Some(40),
                choice: 1,
                target: None,
                old: CState::Conflict(Box::new(ConflictVal {
                    class: 2,
                    base: KVal::Status(Some((0, 0))),
                    ours: KVal::Status(Some((1, 0))),
                    theirs: KVal::Status(Some((4, 2))),
                    prov: None,
                })),
                new: KVal::Status(Some((4, 2))),
            },
            Op::SetBody {
                id: 10,
                prev: 1,
                old: None,
                new: Some(h),
            },
        ];
        c.bodies = vec![BodyEntry {
            hash: h,
            codec: 0,
            raw_len: body.len() as u32,
            data: body,
        }];
        let d = round_trip(&c);
        assert_eq!(d.ops.len(), 12);
        let mut swapped = c.clone();
        swapped.ops.swap(2, 3);
        let mut w = Writer::new();
        swapped.encode(&mut w);
        assert!(
            Commit::decode(&mut Reader::new(w.as_slice())).is_err(),
            "order rule"
        );
    }

    /// NF-4, NF-1 and BD-2 refusals.
    #[test]
    fn net_form_refusals() {
        let mut c = base_commit();
        c.presence = 1 << 12;
        c.ops = vec![
            Op::Create {
                id: 5,
                prev: 0,
                uid: [7; 16],
                kind: 1,
                c_actor: 1,
                c_role: 0,
                image: vec![],
            },
            Op::SetStatus {
                id: 5,
                prev: 0,
                old: (0, 0),
                new: (1, 0),
            },
        ];
        let mut w = Writer::new();
        c.encode(&mut w);
        assert!(Commit::decode(&mut Reader::new(w.as_slice())).is_err());
        let mut c = base_commit();
        c.presence = 1 << 12;
        c.bodies = vec![BodyEntry {
            hash: [0; 16],
            codec: 0,
            raw_len: 1,
            data: vec![b'a'],
        }];
        let mut w = Writer::new();
        c.encode(&mut w);
        assert!(Commit::decode(&mut Reader::new(w.as_slice())).is_err());
    }

    /// A native merge with stated ids, git provenance, absorbed vector, verified, xtr and a foreign_git.
    #[test]
    fn header_groups_round_trip() {
        let mut c = base_commit();
        c.presence =
            (1 << 2) | (1 << 3) | (1 << 4) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 11) | (1 << 14);
        c.kind = kind::MERGE;
        c.import = import::NATIVE;
        c.stmt_origin = 5;
        c.stmt_hash = None;
        c.append_delta = -5;
        c.parents = vec![([2; 16], 138), ([3; 16], 300)];
        c.stated = vec![None, Some([4; 32])];
        c.git = Some(GitProv {
            algo: 2,
            head: Some(Oid::Sha256([5; 32])),
            branch: 1,
            worktree: 0,
            base: None,
        });
        c.foreign_git = Some(Oid::Sha1([6; 20]));
        c.sync_base = Some([3; 16]);
        c.absorbed = Some(vec![(2, 4), (5, 1)]);
        c.verified = Some(true);
        c.xtr = Some(Xtr {
            x_ref: Some(2),
            x_idem: None,
        });
        c.affected = Some((vec![3, 9, 10], false));
        let d = round_trip(&c);
        assert_eq!(d.append_hlc(), (1 << 16) - 5);
        let mut bad = c.clone();
        bad.sync_base = Some([9; 16]);
        let mut w = Writer::new();
        bad.encode(&mut w);
        assert!(Commit::decode(&mut Reader::new(w.as_slice())).is_err());
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    fn canonical_or_refused(b: &[u8]) -> core::result::Result<(), TestCaseError> {
        let mut r = Reader::new(b);
        if let Ok(c) = Commit::decode(&mut r) {
            let used = b.len() - r.remaining();
            let mut w = Writer::new();
            c.encode(&mut w);
            prop_assert_eq!(w.as_slice(), &b[..used]);
        }
        let mut r = Reader::new(b);
        if let Ok(op) = Op::decode(&mut r) {
            let used = b.len() - r.remaining();
            let mut w = Writer::new();
            op.encode(&mut w);
            prop_assert_eq!(w.as_slice(), &b[..used]);
        }
        Ok(())
    }

    proptest! {
        /// [F06 §3], §7: decoding arbitrary bytes as a Commit payload or an op never panics, and what is accepted
        /// re-encodes to the bytes consumed.
        #[test]
        fn commit_decode_is_canonical(b in proptest::collection::vec(any::<u8>(), 0..200)) {
            canonical_or_refused(&b)?;
        }

        /// [F06 §11] with damage: the worked payload with up to three bytes changed is refused or read canonically.
        #[test]
        fn damaged_example_is_canonical_or_refused(
            edits in proptest::collection::vec((0usize..205, any::<u8>()), 1..4),
        ) {
            let mut b = super::tests::example_payload();
            for (at, v) in edits {
                b[at] = v;
            }
            canonical_or_refused(&b)?;
        }
    }
}
