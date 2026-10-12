//! The `Commit` payload of [F06] (`docs/spec/format/06-commit.md`) for the subset a tail of local agent commits holds:
//! a local `ordinary` commit on one ref with at most one parent ([F06 §3.3], `:100-108`), the presence groups `ref_old`,
//! `prev_on_ref`, `git`, `idem`, `stmt_hash`, `msg` and `affected` ([F06 §4.2], `:186-208`), `append_delta` 0
//! ([F06 §4.4.5], `:321`), the ops `Create`, `Delete`, `SetField`, `SetStatus`, `SetBody`, `AddEdge`, `RemoveEdge` and
//! `Move` ([F06 §7.2], `:683-700`) on plain edges, typed values `absent`, `int` and `text` ([F06 §5.1], `:540-547`;
//! [F08 §5.1], `08-data-model.md:210-218`), and bodies with codec `none` ([F06 §8], `:1040-1053`; [F10 §3.1],
//! `10-sealed-files.md:88`).
//!
//! The decoder applies the V-rules ([F06 §2.4], `:61-69`; [F06 §10], `:1199-1205`) that the subset's bytes decide:
//! reserved bits, the bounds of `seq` and `role`, `append_delta` = 0, the `git` group's `algo` and `gflags`, the message
//! length, `affected`'s gaps, `bool8`s, each op's frame and the op's own rules, image order, NF-1, NF-4 and NF-5
//! ([F06 §7.8], `:943-967`), the op order of [F06 §7.9] (`:969-978`), one `prev` per owner ([F06 §7.3], `:722`), and
//! BD-1 and BD-2's length rule ([F06 §8], `:1055-1057`). C-rules (digests, `prev` targets, before-images) are not
//! tested on replay ([F06 §2.4]).

use super::BODY_RAW_CAP;
use super::enc::{
    DecodeError, Reader, put_bool8, put_svar, put_u32, put_u64, put_uvar, put_vbytes, put_vstr,
};
use std::borrow::Cow;

/// The presence bits of the subset ([F06 §4.2], `06-commit.md:186-208`).
pub mod presence {
    /// Bit 0 `ref_old`.
    pub const REF_OLD: u32 = 1 << 0;
    /// Bit 1 `prev_on_ref`.
    pub const PREV_ON_REF: u32 = 1 << 1;
    /// Bit 3 `git`.
    pub const GIT: u32 = 1 << 3;
    /// Bit 6 `idem`.
    pub const IDEM: u32 = 1 << 6;
    /// Bit 12 `stmt_hash`.
    pub const STMT_HASH: u32 = 1 << 12;
    /// Bit 13 `msg`.
    pub const MSG: u32 = 1 << 13;
    /// Bit 14 `affected`.
    pub const AFFECTED: u32 = 1 << 14;
    /// Every bit of the subset.
    pub const SUBSET: u32 = REF_OLD | PREV_ON_REF | GIT | IDEM | STMT_HASH | MSG | AFFECTED;
    /// Bits 20–31, reserved-zero (V).
    pub const RESERVED: u32 = 0xFFF0_0000;
}

/// The op tags of the subset ([F06 §7.2], `06-commit.md:683-700`).
pub mod tag {
    /// `Create`.
    pub const CREATE: u8 = 1;
    /// `Delete`.
    pub const DELETE: u8 = 2;
    /// `SetField`.
    pub const SET_FIELD: u8 = 4;
    /// `SetStatus`.
    pub const SET_STATUS: u8 = 5;
    /// `SetBody`.
    pub const SET_BODY: u8 = 7;
    /// `AddEdge`.
    pub const ADD_EDGE: u8 = 8;
    /// `RemoveEdge`.
    pub const REMOVE_EDGE: u8 = 9;
    /// `Move`.
    pub const MOVE: u8 = 11;
}

/// The type bytes of the subset ([F08 §5.1], `08-data-model.md:210-218`).
pub mod ty {
    /// `absent`: no value bytes.
    pub const ABSENT: u8 = 0;
    /// `int`: `svar64`.
    pub const INT: u8 = 2;
    /// `text`: `vstr`.
    pub const TEXT: u8 = 6;
}

fn subset(r: &Reader<'_>, what: impl core::fmt::Display) -> DecodeError {
    r.err(format!("{what}: outside the probe's subset"))
}

/// A typed value ([F06 §5.1]): its type byte and value bytes, exactly as stored.
#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct TValue<'a>(pub Cow<'a, [u8]>);

impl TValue<'_> {
    /// `absent` (`00`).
    pub fn absent() -> TValue<'static> {
        TValue(Cow::Borrowed(&[ty::ABSENT]))
    }

    /// An `int`.
    pub fn int(i: i64) -> TValue<'static> {
        let mut b = vec![ty::INT];
        put_svar(&mut b, i);
        TValue(Cow::Owned(b))
    }

    /// A `text`.
    pub fn text(s: &str) -> TValue<'static> {
        let mut b = vec![ty::TEXT];
        put_vstr(&mut b, s);
        TValue(Cow::Owned(b))
    }

    /// Whether the value is `absent`.
    pub fn is_absent(&self) -> bool {
        self.0.as_ref() == [ty::ABSENT]
    }

    /// An owned copy.
    pub fn into_owned(self) -> TValue<'static> {
        TValue(Cow::Owned(self.0.into_owned()))
    }

    fn decode<'a>(r: &mut Reader<'a>) -> Result<TValue<'a>, DecodeError> {
        let start = r.pos();
        let mut probe = r.clone();
        let t = probe.u8()?;
        if t & 0x40 != 0 {
            return Err(r.err(format!("type byte {t:#04x} has its reserved bit 6 set")));
        }
        let id = t & 0x3F;
        if id != 1 && t & 0x80 != 0 {
            return Err(r.err(format!("type byte {t:#04x} sets vbit on a non-bool")));
        }
        match id {
            ty::ABSENT => {}
            ty::INT => {
                probe.svar64()?;
            }
            ty::TEXT => {
                probe.vstr()?;
            }
            1 | 3..=5 | 7..=13 => return Err(subset(r, format_args!("value type {id}"))),
            _ => return Err(r.err(format!("value type {id} is reserved"))),
        }
        let len = probe.pos() - start;
        Ok(TValue(Cow::Borrowed(r.take(len)?)))
    }
}

/// A node image ([F06 §6.3], `06-commit.md:647-667`): the status entry, the body entry, then field entries by name
/// ascending; a field never holds `absent`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Image<'a> {
    /// `ie` 0: status and resolution.
    pub status: Option<(u8, u8)>,
    /// `ie` 1: the body hash.
    pub body: Option<[u8; 16]>,
    /// `ie` 2: `(name, value)`, names strictly ascending.
    pub fields: Vec<(u32, TValue<'a>)>,
}

impl Image<'_> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), String> {
        let n = usize::from(self.status.is_some())
            + usize::from(self.body.is_some())
            + self.fields.len();
        put_uvar(out, n as u64);
        if let Some((s, r)) = self.status {
            out.extend_from_slice(&[0, s, r]);
        }
        if let Some(h) = self.body {
            out.push(1);
            out.extend_from_slice(&h);
        }
        for (i, (name, v)) in self.fields.iter().enumerate() {
            if v.is_absent() || (i > 0 && self.fields[i - 1].0 >= *name) {
                return Err("image fields are sorted by name, unique and never absent".into());
            }
            out.push(2);
            put_uvar(out, u64::from(*name));
            out.extend_from_slice(&v.0);
        }
        Ok(())
    }

    fn decode<'a>(r: &mut Reader<'a>) -> Result<Image<'a>, DecodeError> {
        let n = r.uvar32()?;
        let mut img = Image::default();
        let mut stage = 0u8;
        for _ in 0..n {
            let ie = r.u8()?;
            match ie {
                0 if stage == 0 => {
                    img.status = Some((r.u8()?, r.u8()?));
                    stage = 1;
                }
                1 if stage <= 1 => {
                    img.body = Some(r.array()?);
                    stage = 2;
                }
                2 => {
                    let name = r.uvar32()?;
                    if img.fields.last().is_some_and(|(p, _)| *p >= name) {
                        return Err(r.err("image fields are not strictly ascending by name"));
                    }
                    let v = TValue::decode(r)?;
                    if v.is_absent() {
                        return Err(r.err("an image field entry holds absent"));
                    }
                    img.fields.push((name, v));
                    stage = 3;
                }
                0 | 1 => return Err(r.err("image entries out of order")),
                _ => return Err(r.err(format!("image entry kind {ie} is invalid"))),
            }
        }
        Ok(img)
    }
}

/// A plain edge's key and `prev` ([F06 §7.5.1], `06-commit.md:844-857`): `dflag` 0 and the one-byte property block
/// `00` ([F08 §10.2], `08-data-model.md:1233-1249`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Edge {
    /// The source node (the owner).
    pub src: u32,
    /// [F06 §7.3].
    pub prev: u64,
    /// The edge kind.
    pub ekind: u8,
    /// The destination node.
    pub dst: u32,
}

/// One op of the subset ([F06 §7.4], [F06 §7.5]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Op<'a> {
    /// `Create` (1).
    Create {
        /// The new `#N`.
        id: u32,
        /// [F06 §7.3].
        prev: u64,
        /// The node's uid.
        uid: [u8; 16],
        /// The node kind.
        kind: u8,
        /// The creator actor symbol.
        c_actor: u32,
        /// The creator role symbol.
        c_role: u32,
        /// The value keys at the end of the commit.
        image: Image<'a>,
    },
    /// `Delete` (2).
    Delete {
        /// The node.
        id: u32,
        /// [F06 §7.3].
        prev: u64,
        /// The reason symbol.
        reason: u32,
        /// The replacement, 0 = none.
        replaced_by: u32,
        /// The value keys in the base state.
        before: Image<'a>,
    },
    /// `SetField` (4).
    SetField {
        /// The node.
        id: u32,
        /// [F06 §7.3].
        prev: u64,
        /// The field's name symbol.
        name: u32,
        /// The before-image.
        old: TValue<'a>,
        /// The new value; differs from `old`.
        new: TValue<'a>,
    },
    /// `SetStatus` (5).
    SetStatus {
        /// The node.
        id: u32,
        /// [F06 §7.3].
        prev: u64,
        /// The old (status, resolution).
        old: (u8, u8),
        /// The new pair; differs from `old`.
        new: (u8, u8),
    },
    /// `SetBody` (7).
    SetBody {
        /// The node.
        id: u32,
        /// [F06 §7.3].
        prev: u64,
        /// BLAKE3-128 of the old body.
        old: Option<[u8; 16]>,
        /// BLAKE3-128 of the new body.
        new: Option<[u8; 16]>,
    },
    /// `AddEdge` (8).
    AddEdge(Edge),
    /// `RemoveEdge` (9).
    RemoveEdge(Edge),
    /// `Move` (11).
    Move {
        /// The node.
        id: u32,
        /// [F06 §7.3].
        prev: u64,
        /// The old parent, 0 = none.
        old_parent: u32,
        /// The new parent.
        new_parent: u32,
        /// The old fractional index.
        old_order: Cow<'a, str>,
        /// The new one; (parent, order) differs from the old pair.
        new_order: Cow<'a, str>,
    },
}

impl Op<'_> {
    /// The op tag.
    pub fn tag(&self) -> u8 {
        match self {
            Op::Create { .. } => tag::CREATE,
            Op::Delete { .. } => tag::DELETE,
            Op::SetField { .. } => tag::SET_FIELD,
            Op::SetStatus { .. } => tag::SET_STATUS,
            Op::SetBody { .. } => tag::SET_BODY,
            Op::AddEdge(_) => tag::ADD_EDGE,
            Op::RemoveEdge(_) => tag::REMOVE_EDGE,
            Op::Move { .. } => tag::MOVE,
        }
    }

    /// The owner ([F06 §2.3], `06-commit.md:55-59`): the node, or an edge's source.
    pub fn owner(&self) -> u32 {
        match self {
            Op::Create { id, .. }
            | Op::Delete { id, .. }
            | Op::SetField { id, .. }
            | Op::SetStatus { id, .. }
            | Op::SetBody { id, .. }
            | Op::Move { id, .. } => *id,
            Op::AddEdge(e) | Op::RemoveEdge(e) => e.src,
        }
    }

    /// The op's `prev`.
    pub fn prev(&self) -> u64 {
        match self {
            Op::Create { prev, .. }
            | Op::Delete { prev, .. }
            | Op::SetField { prev, .. }
            | Op::SetStatus { prev, .. }
            | Op::SetBody { prev, .. }
            | Op::Move { prev, .. } => *prev,
            Op::AddEdge(e) | Op::RemoveEdge(e) => e.prev,
        }
    }

    /// The op-order key of [F06 §7.9] (`06-commit.md:971-976`): (owner, rank, detail), the rank the key class
    /// (existence 1, status 2, hierarchy 3, field 4, edge 7, body 8), the detail the field name or the edge's
    /// (`ekind`, `dst`, `dflag`). Two ops with equal keys have the same key (NF-1).
    pub fn order_key(&self) -> (u32, u8, u64) {
        let (rank, detail) = match self {
            Op::Create { .. } | Op::Delete { .. } => (1, 0),
            Op::SetStatus { .. } => (2, 0),
            Op::Move { .. } => (3, 0),
            Op::SetField { name, .. } => (4, u64::from(*name)),
            Op::AddEdge(e) | Op::RemoveEdge(e) => {
                (7, (u64::from(e.ekind) << 40) | (u64::from(e.dst) << 8))
            }
            Op::SetBody { .. } => (8, 0),
        };
        (self.owner(), rank, detail)
    }

    /// Appends the op's body (the fields of [F06 §7.4]–§7.5, without the frame).
    fn encode_body(&self, out: &mut Vec<u8>) -> Result<(), String> {
        let head = |out: &mut Vec<u8>, id: u32, prev: u64| {
            put_uvar(out, u64::from(id));
            put_uvar(out, prev);
        };
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
                head(out, *id, *prev);
                out.extend_from_slice(uid);
                out.push(*kind);
                put_uvar(out, u64::from(*c_actor));
                if *c_role > u32::from(u16::MAX) {
                    return Err("c_role above 65,535".into());
                }
                put_uvar(out, u64::from(*c_role));
                image.encode(out)?;
            }
            Op::Delete {
                id,
                prev,
                reason,
                replaced_by,
                before,
            } => {
                head(out, *id, *prev);
                put_uvar(out, u64::from(*reason));
                put_uvar(out, u64::from(*replaced_by));
                before.encode(out)?;
            }
            Op::SetField {
                id,
                prev,
                name,
                old,
                new,
            } => {
                if old == new {
                    return Err("a SetField whose old and new values are equal".into());
                }
                head(out, *id, *prev);
                put_uvar(out, u64::from(*name));
                out.extend_from_slice(&old.0);
                out.extend_from_slice(&new.0);
            }
            Op::SetStatus { id, prev, old, new } => {
                if old == new {
                    return Err("a SetStatus whose old and new pairs are equal".into());
                }
                head(out, *id, *prev);
                out.extend_from_slice(&[old.0, old.1, new.0, new.1]);
            }
            Op::SetBody { id, prev, old, new } => {
                if old == new {
                    return Err("a SetBody whose old and new hashes are equal".into());
                }
                head(out, *id, *prev);
                out.push(u8::from(old.is_some()) | (u8::from(new.is_some()) << 1));
                if let Some(h) = old {
                    out.extend_from_slice(h);
                }
                if let Some(h) = new {
                    out.extend_from_slice(h);
                }
            }
            Op::AddEdge(e) | Op::RemoveEdge(e) => {
                head(out, e.src, e.prev);
                out.push(e.ekind);
                put_uvar(out, u64::from(e.dst));
                out.push(0); // dflag: no discriminator
                out.push(0); // the property block of a plain edge: pflags 0
            }
            Op::Move {
                id,
                prev,
                old_parent,
                new_parent,
                old_order,
                new_order,
            } => {
                if old_parent == new_parent && old_order == new_order {
                    return Err("a Move whose (parent, order) pair does not change".into());
                }
                head(out, *id, *prev);
                put_uvar(out, u64::from(*old_parent));
                put_uvar(out, u64::from(*new_parent));
                put_vstr(out, old_order);
                put_vstr(out, new_order);
            }
        }
        Ok(())
    }

    /// Appends the op with its frame ([F06 §7.1]): `op` u8, `len` uvar32, the body.
    pub fn encode(&self, out: &mut Vec<u8>, scratch: &mut Vec<u8>) -> Result<(), String> {
        scratch.clear();
        self.encode_body(scratch)?;
        out.push(self.tag());
        put_uvar(out, scratch.len() as u64);
        out.extend_from_slice(scratch);
        Ok(())
    }

    /// Reads one framed op ([F06 §7.1]); its body must fill `len` exactly (V).
    pub fn decode<'a>(r: &mut Reader<'a>) -> Result<Op<'a>, DecodeError> {
        let t = r.u8()?;
        let len = r.uvar32()? as usize;
        let at = r.pos();
        let body = r.take(len)?;
        let mut b = Reader::new(body);
        let shift = |e: DecodeError| DecodeError {
            at: at + e.at,
            what: e.what,
        };
        let op = Op::decode_body(t, &mut b).map_err(shift)?;
        b.end().map_err(shift)?;
        Ok(op)
    }

    fn decode_body<'a>(t: u8, b: &mut Reader<'a>) -> Result<Op<'a>, DecodeError> {
        Ok(match t {
            tag::CREATE => {
                let (id, prev) = (b.uvar32()?, b.uvar64()?);
                let uid = b.array()?;
                let kind = b.u8()?;
                let c_actor = b.uvar32()?;
                let c_role = b.uvar32()?;
                if c_role > u32::from(u16::MAX) {
                    return Err(b.err("c_role above 65,535"));
                }
                Op::Create {
                    id,
                    prev,
                    uid,
                    kind,
                    c_actor,
                    c_role,
                    image: Image::decode(b)?,
                }
            }
            tag::DELETE => Op::Delete {
                id: b.uvar32()?,
                prev: b.uvar64()?,
                reason: b.uvar32()?,
                replaced_by: b.uvar32()?,
                before: Image::decode(b)?,
            },
            tag::SET_FIELD => {
                let (id, prev, name) = (b.uvar32()?, b.uvar64()?, b.uvar32()?);
                let old = TValue::decode(b)?;
                let new = TValue::decode(b)?;
                if old == new {
                    return Err(b.err("a SetField whose old and new values are equal"));
                }
                Op::SetField {
                    id,
                    prev,
                    name,
                    old,
                    new,
                }
            }
            tag::SET_STATUS => {
                let (id, prev) = (b.uvar32()?, b.uvar64()?);
                let old = (b.u8()?, b.u8()?);
                let new = (b.u8()?, b.u8()?);
                if old == new {
                    return Err(b.err("a SetStatus whose old and new pairs are equal"));
                }
                Op::SetStatus { id, prev, old, new }
            }
            tag::SET_BODY => {
                let (id, prev) = (b.uvar32()?, b.uvar64()?);
                let f = b.u8()?;
                if f == 0 || f & !3 != 0 {
                    return Err(b.err(format!("SetBody bflags {f:#04x}")));
                }
                let old = if f & 1 != 0 { Some(b.array()?) } else { None };
                let new = if f & 2 != 0 { Some(b.array()?) } else { None };
                if old.is_some() && old == new {
                    return Err(b.err("a SetBody whose old and new hashes are equal"));
                }
                Op::SetBody { id, prev, old, new }
            }
            tag::ADD_EDGE | tag::REMOVE_EDGE => {
                let (src, prev) = (b.uvar32()?, b.uvar64()?);
                let ekind = b.u8()?;
                let dst = b.uvar32()?;
                let dflag = b.u8()?;
                if dflag & !1 != 0 {
                    return Err(b.err(format!("dflag {dflag:#04x} sets a reserved bit")));
                }
                if dflag != 0 {
                    return Err(subset(b, "an edge discriminator (an anchor edge)"));
                }
                let pflags = b.u8()?;
                if pflags & 0xF8 != 0 {
                    return Err(b.err(format!("pflags {pflags:#04x} sets a reserved bit")));
                }
                if pflags != 0 {
                    return Err(subset(b, format_args!("edge property flags {pflags:#04x}")));
                }
                let e = Edge {
                    src,
                    prev,
                    ekind,
                    dst,
                };
                if t == tag::ADD_EDGE {
                    Op::AddEdge(e)
                } else {
                    Op::RemoveEdge(e)
                }
            }
            tag::MOVE => {
                let (id, prev) = (b.uvar32()?, b.uvar64()?);
                let old_parent = b.uvar32()?;
                let new_parent = b.uvar32()?;
                let old_order = b.vstr()?;
                let new_order = b.vstr()?;
                if old_parent == new_parent && old_order == new_order {
                    return Err(b.err("a Move whose (parent, order) pair does not change"));
                }
                Op::Move {
                    id,
                    prev,
                    old_parent,
                    new_parent,
                    old_order: Cow::Borrowed(old_order),
                    new_order: Cow::Borrowed(new_order),
                }
            }
            3 | 6 | 10 | 12..=16 => return Err(subset(b, format_args!("op tag {t}"))),
            _ => return Err(b.err(format!("op tag {t} is invalid"))),
        })
    }
}

/// A body carried with codec `none` ([F06 §8], `06-commit.md:1043-1050`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Body<'a> {
    /// BLAKE3-128 of the raw bytes.
    pub hash: [u8; 16],
    /// The raw bytes, at most 64 KiB.
    pub data: Cow<'a, [u8]>,
}

/// BLAKE3-128: the first 16 bytes of BLAKE3-256 ([F01 §5.6]).
pub fn blake3_16(b: &[u8]) -> [u8; 16] {
    let mut h = [0u8; 16];
    h.copy_from_slice(&blake3::hash(b).as_bytes()[..16]);
    h
}

/// The `git` provenance group ([F06 §4.4.6], `06-commit.md:327-337`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Git<'a> {
    /// 1 `sha1`, 2 `sha256`; 0 only without digests.
    pub algo: u8,
    /// The caller tree's `HEAD`, 20 or 32 bytes by `algo`.
    pub head: Option<Cow<'a, [u8]>>,
    /// The `git-branch` symbol.
    pub branch: u32,
    /// The `git-worktree` symbol.
    pub worktree: u32,
    /// The lane's base commit.
    pub base: Option<Cow<'a, [u8]>>,
}

/// `affected` ([F06 §4.4.13], `06-commit.md:398-407`): ids strictly ascending.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Affected {
    /// `affected_complete`.
    pub complete: bool,
    /// The ids, strictly ascending.
    pub ids: Vec<u32>,
}

/// One parent: the first 16 bytes of its commit id and the lsn of its record ([F06 §4.3] order 4).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Parent {
    /// `id16`.
    pub id16: [u8; 16],
    /// The parent record's lsn.
    pub lsn: u64,
}

/// A `Commit` payload of the subset ([F06 §4.3], `06-commit.md:214-261`). `kind` (0 `ordinary`), `import` (0 `local`) and
/// `append_delta` (0) are fixed by the subset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commit<'a> {
    /// Order 2.
    pub commit_id: [u8; 32],
    /// Orders 3–4: 0 or 1 parent.
    pub parent: Option<Parent>,
    /// Order 7.
    pub gen_no: u32,
    /// Order 8, at most 2^32 − 1.
    pub seq: u64,
    /// Order 9, the `ref` symbol.
    pub ref_sym: u32,
    /// Order 10.
    pub ref_id: u32,
    /// Order 11 (bit 0).
    pub ref_old: Option<[u8; 16]>,
    /// Order 12 (bit 1): an absolute lsn.
    pub prev_on_ref: Option<u64>,
    /// Order 13.
    pub ref_seq: u32,
    /// Order 16.
    pub hlc: u64,
    /// Order 17.
    pub actor: u32,
    /// Order 18, at most 65,535.
    pub role: u32,
    /// Order 19.
    pub session: u32,
    /// Order 20.
    pub schema_version: u32,
    /// Order 21 (bit 3).
    pub git: Option<Git<'a>>,
    /// Orders 24–25 (bit 6): `idem_key`, `idem_payload`.
    pub idem: Option<([u8; 16], [u8; 16])>,
    /// Order 31, [F06 §3.4].
    pub stmt_origin: u8,
    /// Order 32, [F06 §3.5].
    pub actor_src: u8,
    /// Order 33.
    pub stmt_sym: u32,
    /// Order 34 (bit 12).
    pub stmt_hash: Option<[u8; 16]>,
    /// Order 37 (bit 13): 1 to 65,535 bytes.
    pub msg: Option<Cow<'a, str>>,
    /// Order 38 (bit 14).
    pub affected: Option<Affected>,
    /// Order 39.
    pub changeset_digest: [u8; 32],
    /// Orders 41–42, in op order.
    pub ops: Vec<Op<'a>>,
    /// Orders 43–44, by hash ascending.
    pub bodies: Vec<Body<'a>>,
}

impl Commit<'_> {
    /// The presence bitmap the record's groups imply.
    pub fn presence(&self) -> u32 {
        let mut p = 0;
        let mut set = |on: bool, bit: u32| {
            if on {
                p |= bit;
            }
        };
        set(self.ref_old.is_some(), presence::REF_OLD);
        set(self.prev_on_ref.is_some(), presence::PREV_ON_REF);
        set(self.git.is_some(), presence::GIT);
        set(self.idem.is_some(), presence::IDEM);
        set(self.stmt_hash.is_some(), presence::STMT_HASH);
        set(self.msg.is_some(), presence::MSG);
        set(self.affected.is_some(), presence::AFFECTED);
        p
    }

    /// `body_bytes(r)` of [F17 §5.1] (`17-store-parameters.md:317`): the `data` bytes of the carried bodies (the rest
    /// of each entry counts in `rec_bytes`; cut finding F-2).
    pub fn body_bytes(&self) -> u64 {
        self.bodies.iter().map(|b| b.data.len() as u64).sum()
    }

    /// Appends the payload.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), String> {
        put_u32(out, self.presence());
        out.extend_from_slice(&self.commit_id);
        match &self.parent {
            None => out.push(0),
            Some(p) => {
                out.push(1);
                out.extend_from_slice(&p.id16);
                put_uvar(out, p.lsn);
            }
        }
        put_uvar(out, u64::from(self.gen_no));
        if self.seq > u64::from(u32::MAX) {
            return Err("seq above 2^32 − 1".into());
        }
        put_uvar(out, self.seq);
        put_uvar(out, u64::from(self.ref_sym));
        put_uvar(out, u64::from(self.ref_id));
        if let Some(h) = &self.ref_old {
            out.extend_from_slice(h);
        }
        if let Some(l) = self.prev_on_ref {
            put_uvar(out, l);
        }
        put_uvar(out, u64::from(self.ref_seq));
        out.extend_from_slice(&[0, 0]); // kind ordinary, import local
        put_u64(out, self.hlc);
        put_uvar(out, u64::from(self.actor));
        if self.role > u32::from(u16::MAX) {
            return Err("role above 65,535".into());
        }
        put_uvar(out, u64::from(self.role));
        put_uvar(out, u64::from(self.session));
        put_uvar(out, u64::from(self.schema_version));
        if let Some(g) = &self.git {
            let dlen = match g.algo {
                1 => 20,
                2 => 32,
                _ => 0,
            };
            if [&g.head, &g.base]
                .iter()
                .any(|d| d.as_ref().is_some_and(|d| d.len() != dlen || dlen == 0))
            {
                return Err("a git digest whose length does not match algo".into());
            }
            out.push(g.algo);
            out.push(u8::from(g.head.is_some()) | (u8::from(g.base.is_some()) << 1));
            if let Some(h) = &g.head {
                out.extend_from_slice(h);
            }
            put_uvar(out, u64::from(g.branch));
            put_uvar(out, u64::from(g.worktree));
            if let Some(b) = &g.base {
                out.extend_from_slice(b);
            }
        }
        if let Some((k, p)) = &self.idem {
            out.extend_from_slice(k);
            out.extend_from_slice(p);
        }
        out.push(self.stmt_origin);
        out.push(self.actor_src);
        put_uvar(out, u64::from(self.stmt_sym));
        if let Some(h) = &self.stmt_hash {
            out.extend_from_slice(h);
        }
        put_svar(out, 0); // append_delta, 0 for a local commit
        if let Some(m) = &self.msg {
            if m.is_empty() || m.len() > usize::from(u16::MAX) {
                return Err(format!("a message of {} bytes", m.len()));
            }
            put_vstr(out, m);
        }
        if let Some(a) = &self.affected {
            put_uvar(out, a.ids.len() as u64);
            put_bool8(out, a.complete);
            for (i, id) in a.ids.iter().enumerate() {
                if i == 0 {
                    put_uvar(out, u64::from(*id));
                } else if *id <= a.ids[i - 1] {
                    return Err("affected ids are not strictly ascending".into());
                } else {
                    put_uvar(out, u64::from(id - a.ids[i - 1]));
                }
            }
        }
        out.extend_from_slice(&self.changeset_digest);
        put_uvar(out, self.ops.len() as u64);
        let mut scratch = Vec::new();
        for (i, op) in self.ops.iter().enumerate() {
            if i > 0 && self.ops[i - 1].order_key() >= op.order_key() {
                return Err("ops are not in strictly ascending op order (F06 §7.9, NF-1)".into());
            }
            op.encode(out, &mut scratch)?;
        }
        put_uvar(out, self.bodies.len() as u64);
        for (i, b) in self.bodies.iter().enumerate() {
            if i > 0 && self.bodies[i - 1].hash >= b.hash {
                return Err("bodies are not strictly ascending by hash (BD-1)".into());
            }
            if b.data.len() > BODY_RAW_CAP as usize {
                return Err("a body above the 64 KiB cap".into());
            }
            out.extend_from_slice(&b.hash);
            out.push(0); // codec none
            put_uvar(out, b.data.len() as u64);
            put_vbytes(out, &b.data);
        }
        Ok(())
    }

    /// Decodes a payload of the subset; the fields fill it exactly ([F06 §4.1], V).
    pub fn decode(payload: &[u8]) -> Result<Commit<'_>, DecodeError> {
        let mut r = Reader::new(payload);
        let p = r.u32()?;
        if p & presence::RESERVED != 0 {
            return Err(r.err(format!("presence {p:#010x} sets a reserved bit 20-31")));
        }
        if p & !presence::SUBSET != 0 {
            return Err(subset(
                &r,
                format_args!("presence bits {:#010x}", p & !presence::SUBSET),
            ));
        }
        let commit_id = r.array()?;
        let parent = match r.u8()? {
            0 => None,
            1 => Some(Parent {
                id16: r.array()?,
                lsn: r.uvar64()?,
            }),
            2 => return Err(subset(&r, "two parents (a merge or sync)")),
            n => return Err(r.err(format!("n_parents {n}"))),
        };
        let gen_no = r.uvar32()?;
        let seq = r.uvar64()?;
        if seq > u64::from(u32::MAX) {
            return Err(r.err("seq above 2^32 − 1 (F06 §4.4.3)"));
        }
        let ref_sym = r.uvar32()?;
        let ref_id = r.uvar32()?;
        let ref_old = if p & presence::REF_OLD != 0 {
            Some(r.array()?)
        } else {
            None
        };
        let prev_on_ref = if p & presence::PREV_ON_REF != 0 {
            Some(r.uvar64()?)
        } else {
            None
        };
        let ref_seq = r.uvar32()?;
        let (kind, import) = (r.u8()?, r.u8()?);
        if kind > 5 || import > 3 {
            return Err(r.err(format!("commit kind {kind} or import {import} is invalid")));
        }
        if kind != 0 || import != 0 {
            return Err(subset(
                &r,
                format_args!("commit kind {kind}, import {import}"),
            ));
        }
        let hlc = r.u64()?;
        let actor = r.uvar32()?;
        let role = r.uvar32()?;
        if role > u32::from(u16::MAX) {
            return Err(r.err("role above 65,535"));
        }
        let session = r.uvar32()?;
        let schema_version = r.uvar32()?;
        let git = if p & presence::GIT != 0 {
            let algo = r.u8()?;
            let gflags = r.u8()?;
            if gflags & !3 != 0 {
                return Err(r.err(format!("gflags {gflags:#04x} sets a reserved bit")));
            }
            let dlen = match algo {
                0 if gflags == 0 => 0,
                1 => 20,
                2 => 32,
                _ => return Err(r.err(format!("git algo {algo} with gflags {gflags}"))),
            };
            let head = if gflags & 1 != 0 {
                Some(Cow::Borrowed(r.take(dlen)?))
            } else {
                None
            };
            let branch = r.uvar32()?;
            let worktree = r.uvar32()?;
            let base = if gflags & 2 != 0 {
                Some(Cow::Borrowed(r.take(dlen)?))
            } else {
                None
            };
            Some(Git {
                algo,
                head,
                branch,
                worktree,
                base,
            })
        } else {
            None
        };
        let idem = if p & presence::IDEM != 0 {
            Some((r.array()?, r.array()?))
        } else {
            None
        };
        let stmt_origin = r.u8()?;
        if stmt_origin > 6 || stmt_origin == 5 {
            return Err(r.err(format!("stmt_origin {stmt_origin} on a local commit")));
        }
        let actor_src = r.u8()?;
        if actor_src > 6 {
            return Err(r.err(format!("actor_src {actor_src}")));
        }
        let stmt_sym = r.uvar32()?;
        let stmt_hash = if p & presence::STMT_HASH != 0 {
            Some(r.array()?)
        } else {
            None
        };
        if r.svar64()? != 0 {
            return Err(r.err("append_delta of a local commit is not 0 (F06 §4.4.5)"));
        }
        let msg = if p & presence::MSG != 0 {
            let m = r.vstr()?;
            if m.is_empty() || m.len() > usize::from(u16::MAX) {
                return Err(r.err(format!("a message of {} bytes", m.len())));
            }
            Some(Cow::Borrowed(m))
        } else {
            None
        };
        let affected = if p & presence::AFFECTED != 0 {
            let n = r.uvar32()?;
            let complete = r.bool8()?;
            let mut ids = Vec::with_capacity(n.min(1 << 16) as usize);
            let mut last = 0u32;
            for i in 0..n {
                let v = r.uvar32()?;
                let id = if i == 0 {
                    v
                } else if v == 0 {
                    return Err(r.err("an affected gap of 0"));
                } else {
                    last.checked_add(v)
                        .ok_or_else(|| r.err("an affected id above 2^32 − 1"))?
                };
                ids.push(id);
                last = id;
            }
            Some(Affected { complete, ids })
        } else {
            None
        };
        let changeset_digest = r.array()?;
        let n_ops = r.uvar32()?;
        let mut ops: Vec<Op<'_>> = Vec::with_capacity(n_ops.min(1 << 16) as usize);
        for _ in 0..n_ops {
            let at = r.pos();
            let op = Op::decode(&mut r)?;
            let err = |what: &str| DecodeError {
                at,
                what: what.into(),
            };
            if let Some(last) = ops.last() {
                if last.order_key() >= op.order_key() {
                    return Err(err(
                        "ops out of the op order of F06 §7.9, or two ops of one key (NF-1)",
                    ));
                }
                if last.owner() == op.owner() && last.prev() != op.prev() {
                    return Err(err("two ops of one owner with different prev (F06 §7.3)"));
                }
            }
            ops.push(op);
        }
        nf_folding(&ops).map_err(|w| r.err(w))?;
        let n_bodies = r.uvar32()?;
        let mut bodies: Vec<Body<'_>> = Vec::with_capacity(n_bodies.min(1 << 10) as usize);
        for _ in 0..n_bodies {
            let hash: [u8; 16] = r.array()?;
            if bodies.last().is_some_and(|b| b.hash >= hash) {
                return Err(r.err("bodies are not strictly ascending by hash (BD-1)"));
            }
            let codec = r.u8()?;
            if codec != 0 {
                return Err(subset(&r, format_args!("body codec {codec}")));
            }
            let raw_len = r.uvar32()?;
            if raw_len > BODY_RAW_CAP {
                return Err(r.err(format!("a body raw_len of {raw_len}, above 64 KiB")));
            }
            let data = r.vbytes()?;
            if data.len() != raw_len as usize {
                return Err(r.err("codec none with len(data) ≠ raw_len (BD-2)"));
            }
            bodies.push(Body {
                hash,
                data: Cow::Borrowed(data),
            });
        }
        r.end()?;
        Ok(Commit {
            commit_id,
            parent,
            gen_no,
            seq,
            ref_sym,
            ref_id,
            ref_old,
            prev_on_ref,
            ref_seq,
            hlc,
            actor,
            role,
            session,
            schema_version,
            git,
            idem,
            stmt_origin,
            actor_src,
            stmt_sym,
            stmt_hash,
            msg,
            affected,
            changeset_digest,
            ops,
            bodies,
        })
    }
}

/// NF-4 and NF-5 ([F06 §7.8], `06-commit.md:955-958`): a record with a `Create` or a `Delete` of n holds no `SetField`,
/// `SetStatus` or `SetBody` of n. Ops are in op order, so an owner's ops are adjacent and its existence op is first.
fn nf_folding(ops: &[Op<'_>]) -> Result<(), String> {
    let mut i = 0;
    while i < ops.len() {
        let owner = ops[i].owner();
        let j = i + ops[i..].iter().take_while(|o| o.owner() == owner).count();
        if matches!(ops[i], Op::Create { .. } | Op::Delete { .. })
            && ops[i + 1..j].iter().any(|o| {
                matches!(
                    o,
                    Op::SetField { .. } | Op::SetStatus { .. } | Op::SetBody { .. }
                )
            })
        {
            return Err(format!(
                "a Create or Delete of #{owner} with a value op of the same node (NF-4, NF-5)"
            ));
        }
        i = j;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tail::rec::record_len;

    /// The commit of [F06 §11] (`06-commit.md:1214-1239`): `C…`, `P…`, `K…`, `Q…`, `S…` and `D…` as fixed bytes.
    fn example() -> Commit<'static> {
        Commit {
            commit_id: [0xC0; 32],
            parent: Some(Parent {
                id16: [0x50; 16],
                lsn: 4_660,
            }),
            gen_no: 7,
            seq: 4_471,
            ref_sym: 1,
            ref_id: 0,
            ref_old: Some([0x50; 16]),
            prev_on_ref: Some(4_660),
            ref_seq: 7,
            hlc: 0x01A0_C450_6C00_0003,
            actor: 5,
            role: 2,
            session: 3,
            schema_version: 1,
            git: None,
            idem: Some(([0x4B; 16], [0x51; 16])),
            stmt_origin: 1,
            actor_src: 1,
            stmt_sym: 4,
            stmt_hash: Some([0x53; 16]),
            msg: Some(Cow::Borrowed("claim --start")),
            affected: Some(Affected {
                complete: true,
                ids: vec![12],
            }),
            changeset_digest: [0xD0; 32],
            ops: vec![Op::SetStatus {
                id: 12,
                prev: 4_800,
                old: (0, 0),
                new: (1, 0),
            }],
            bodies: Vec::new(),
        }
    }

    /// The example's bytes as the chapter lists them (`06-commit.md:1214-1239`).
    fn example_bytes() -> Vec<u8> {
        let mut b = vec![0x43, 0x70, 0x00, 0x00];
        b.extend([0xC0; 32]);
        b.push(0x01);
        b.extend([0x50; 16]);
        b.extend([0xB4, 0x24, 0x07, 0xF7, 0x22, 0x01, 0x00]);
        b.extend([0x50; 16]);
        b.extend([0xB4, 0x24, 0x07, 0x00, 0x00]);
        b.extend([0x03, 0x00, 0x00, 0x6C, 0x50, 0xC4, 0xA0, 0x01]);
        b.extend([0x05, 0x02, 0x03, 0x01]);
        b.extend([0x4B; 16]);
        b.extend([0x51; 16]);
        b.extend([0x01, 0x01, 0x04]);
        b.extend([0x53; 16]);
        b.push(0x00);
        b.push(0x0D);
        b.extend(b"claim --start");
        b.extend([0x01, 0x01, 0x0C]);
        b.extend([0xD0; 32]);
        b.push(0x01);
        b.extend([0x05, 0x07, 0x0C, 0xC0, 0x25, 0x00, 0x00, 0x01, 0x00]);
        b.push(0x00);
        b
    }

    /// [F06 §11]: "The payload is 205 bytes, 237 with the 32-byte `RecHdr`" (`06-commit.md:1241`); its header part
    /// without git is 194 B (`:523-525`).
    #[test]
    fn f06_example_commit_is_205_bytes() {
        let c = example();
        let mut out = Vec::new();
        c.encode(&mut out).unwrap();
        assert_eq!(out, example_bytes());
        assert_eq!(out.len(), 205);
        assert_eq!(record_len(out.len(), false), 237);
        // The header part runs to `changeset_digest` (`06-commit.md:173`): 205 − n_ops − op − n_bodies.
        assert_eq!(out.len() - 1 - 9 - 1, 194);
        assert_eq!(Commit::decode(&out), Ok(c));
    }

    /// Its `SetStatus` op ([F06 §7.1], §7.4, `06-commit.md:1237`): `05 07 0C C0 25 00 00 01 00`.
    #[test]
    fn f06_example_setstatus_op_bytes() {
        let op = Op::SetStatus {
            id: 12,
            prev: 4_800,
            old: (0, 0),
            new: (1, 0),
        };
        let (mut out, mut scratch) = (Vec::new(), Vec::new());
        op.encode(&mut out, &mut scratch).unwrap();
        assert_eq!(out, [0x05, 0x07, 0x0C, 0xC0, 0x25, 0x00, 0x00, 0x01, 0x00]);
        assert_eq!(Op::decode(&mut Reader::new(&out)), Ok(op));
        // A body one byte longer than its contents is refused (V: the fields fill `len` exactly).
        let long = [0x05, 0x08, 0x0C, 0xC0, 0x25, 0x00, 0x00, 0x01, 0x00, 0x00];
        assert!(
            Op::decode(&mut Reader::new(&long))
                .unwrap_err()
                .what
                .contains("left after")
        );
    }

    /// The informative values of [F06 §5.1] (`06-commit.md:546-547`) in the subset, and the refusal of the others.
    #[test]
    fn tvalue_examples_of_f06() {
        assert_eq!(TValue::int(3).0.as_ref(), [0x02, 0x06]);
        assert_eq!(TValue::text("ab").0.as_ref(), [0x06, 0x02, 0x61, 0x62]);
        assert_eq!(TValue::absent().0.as_ref(), [0x00]);
        for v in [TValue::int(3), TValue::text("ab"), TValue::absent()] {
            let mut r = Reader::new(&v.0);
            assert_eq!(TValue::decode(&mut r), Ok(v.clone()));
            assert_eq!(r.remaining(), 0);
        }
        let what = |b: &[u8]| TValue::decode(&mut Reader::new(b)).unwrap_err().what;
        // `false` 01, `true` 81 and a ref 09 0C 00 00 00 are values of F08 the probe does not write.
        assert!(what(&[0x01]).contains("outside the probe's subset"));
        assert!(what(&[0x81]).contains("outside the probe's subset"));
        assert!(what(&[0x09, 0x0C, 0, 0, 0]).contains("outside the probe's subset"));
        // Type 14 is reserved, bit 6 is reserved, and vbit is only a bool's.
        assert!(what(&[0x0E]).contains("reserved"));
        assert!(what(&[0x42, 0x06]).contains("reserved bit 6"));
        assert!(what(&[0x82, 0x06]).contains("vbit"));
        // A cut-off text.
        assert!(what(&[0x06, 0x05, 0x61]).contains("cut off"));
    }

    /// The presence bitmap ([F06 §4.2], `06-commit.md:186-208`): each group sets its bit; bits 20–31 are reserved and
    /// the bits of groups outside the subset are refused as such.
    #[test]
    fn presence_bits_follow_f06() {
        let c = example();
        assert_eq!(c.presence(), 0x7043);
        let mut g = example();
        g.git = Some(Git {
            algo: 1,
            head: Some(Cow::Owned(vec![0xAB; 20])),
            branch: 2,
            worktree: 3,
            base: None,
        });
        assert_eq!(g.presence(), 0x7043 | presence::GIT);
        let mut out = Vec::new();
        g.encode(&mut out).unwrap();
        // With git and SHA-1: 205 + algo, gflags, 20 digest bytes, branch, worktree = 229, in F06 §4.6's ≈ 230–250 B.
        assert_eq!(out.len(), 229);
        assert_eq!(Commit::decode(&out), Ok(g));
        let mut bare = example();
        bare.ref_old = None;
        bare.prev_on_ref = None;
        bare.idem = None;
        bare.stmt_hash = None;
        bare.msg = None;
        bare.affected = None;
        assert_eq!(bare.presence(), 0);
        let mut out = Vec::new();
        bare.encode(&mut out).unwrap();
        assert_eq!(Commit::decode(&out), Ok(bare));
        // Bit 20 is reserved; bit 2 (`stated`) is a valid group outside the subset.
        let mut b = example_bytes();
        b[2] = 0x10;
        assert!(
            Commit::decode(&b)
                .unwrap_err()
                .what
                .contains("reserved bit 20-31")
        );
        let mut b = example_bytes();
        b[0] |= 0x04;
        assert!(
            Commit::decode(&b)
                .unwrap_err()
                .what
                .contains("outside the probe's subset")
        );
        // Bytes left after the last field are refused.
        let mut b = example_bytes();
        b.push(0);
        assert!(Commit::decode(&b).unwrap_err().what.contains("left after"));
    }

    fn op_set() -> Vec<Op<'static>> {
        vec![
            Op::Create {
                id: 51,
                prev: 0,
                uid: [7; 16],
                kind: 2,
                c_actor: 5,
                c_role: 2,
                image: Image {
                    status: Some((1, 0)),
                    body: Some([9; 16]),
                    fields: vec![(1, TValue::text("t")), (3, TValue::int(-5))],
                },
            },
            Op::Delete {
                id: 52,
                prev: 300,
                reason: 4,
                replaced_by: 51,
                before: Image {
                    status: Some((2, 1)),
                    body: None,
                    fields: vec![(1, TValue::text("old"))],
                },
            },
            Op::SetField {
                id: 53,
                prev: 1 << 40,
                name: 6,
                old: TValue::absent(),
                new: TValue::text("v"),
            },
            Op::SetStatus {
                id: 54,
                prev: 1,
                old: (0, 0),
                new: (3, 2),
            },
            Op::SetBody {
                id: 55,
                prev: 2,
                old: Some([1; 16]),
                new: None,
            },
            Op::AddEdge(Edge {
                src: 56,
                prev: 3,
                ekind: 4,
                dst: 51,
            }),
            Op::RemoveEdge(Edge {
                src: 57,
                prev: 4,
                ekind: 1,
                dst: 2,
            }),
            Op::Move {
                id: 58,
                prev: 5,
                old_parent: 0,
                new_parent: 9,
                old_order: Cow::Borrowed(""),
                new_order: Cow::Borrowed("a1"),
            },
        ]
    }

    /// Each of the eight ops of the subset ([F06 §7.4], §7.5) round-trips; the V-rules of op order, NF-1, NF-4 and one
    /// `prev` per owner refuse a record that breaks them.
    #[test]
    fn every_op_of_the_subset_round_trips() {
        let ops = op_set();
        let tags: Vec<u8> = ops.iter().map(Op::tag).collect();
        assert_eq!(tags, [1, 2, 4, 5, 7, 8, 9, 11]);
        let mut scratch = Vec::new();
        for op in &ops {
            let mut out = Vec::new();
            op.encode(&mut out, &mut scratch).unwrap();
            assert_eq!(out[0], op.tag());
            let mut r = Reader::new(&out);
            assert_eq!(Op::decode(&mut r).as_ref(), Ok(op));
            assert_eq!(r.remaining(), 0);
        }
        let mut c = example();
        c.ops = ops.clone();
        c.bodies = vec![Body {
            hash: blake3_16(b"body"),
            data: Cow::Borrowed(b"body"),
        }];
        let mut out = Vec::new();
        c.encode(&mut out).unwrap();
        assert_eq!(Commit::decode(&out), Ok(c.clone()));
        assert_eq!(c.body_bytes(), 4);
        let refused = |ops: Vec<Op<'static>>| {
            let mut c = example();
            c.ops = ops;
            let mut full = Vec::new();
            c.encode(&mut full).unwrap();
            Commit::decode(&full).map(|_| ()).unwrap_err().what
        };
        // Out of order: the encoder refuses; NF-1, NF-4 and two prevs of one owner are refused by the decoder.
        let mut swapped = example();
        swapped.ops = vec![ops[1].clone(), ops[0].clone()];
        assert!(
            swapped
                .encode(&mut Vec::new())
                .unwrap_err()
                .contains("op order")
        );
        let nf4 = vec![
            ops[0].clone(),
            Op::SetStatus {
                id: 51,
                prev: 0,
                old: (1, 0),
                new: (2, 0),
            },
        ];
        assert!(refused(nf4).contains("NF-4"));
        let two_prevs = vec![
            Op::SetStatus {
                id: 60,
                prev: 1,
                old: (1, 0),
                new: (2, 0),
            },
            Op::SetField {
                id: 60,
                prev: 2,
                name: 1,
                old: TValue::absent(),
                new: TValue::int(1),
            },
        ];
        assert!(refused(two_prevs).contains("different prev"));
        // An edge with a discriminator, a pinned edge and a reserved op tag.
        let anchor = [8u8, 6, 4, 3, 1, 9, 1, 0];
        assert!(
            Op::decode(&mut Reader::new(&anchor))
                .unwrap_err()
                .what
                .contains("outside the probe's subset")
        );
        let pinned = [8u8, 6, 4, 3, 1, 9, 0, 1];
        assert!(
            Op::decode(&mut Reader::new(&pinned))
                .unwrap_err()
                .what
                .contains("outside the probe's subset")
        );
        let plain = [8u8, 6, 4, 3, 1, 9, 0, 0];
        assert!(Op::decode(&mut Reader::new(&plain)).is_ok());
        assert!(
            Op::decode(&mut Reader::new(&[3, 0]))
                .unwrap_err()
                .what
                .contains("outside the probe's subset")
        );
        assert!(
            Op::decode(&mut Reader::new(&[0, 0]))
                .unwrap_err()
                .what
                .contains("invalid")
        );
        // A SetField whose values are equal is refused on both sides.
        let same = Op::SetField {
            id: 1,
            prev: 1,
            name: 1,
            old: TValue::int(1),
            new: TValue::int(1),
        };
        assert!(same.encode(&mut Vec::new(), &mut scratch).is_err());
    }
}
