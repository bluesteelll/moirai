//! [F08] data model bytes: the type byte and every stored value encoding (§5.1–§5.3, §5.5), the field block (§6),
//! `NodeHdr` (§3) and `Creator` (§4), the schema item records with `KindSet` (§8.4.8, §8.5), the edge property block
//! (§10.2), the anchor record (§10.3) with its scope value (§10.3.1) and the window value of [F20 §2.7.3].

use crate::holes;
use crate::prim::{Oid, Reader, Result, Writer, err};

/// `NONE32` ([F08 §12]).
pub const NONE32: u32 = 0xFFFF_FFFF;

/// Type ids of the registry ([F08 §5.1]).
pub mod ty {
    /// `absent`.
    pub const ABSENT: u8 = 0;
    /// `bool`.
    pub const BOOL: u8 = 1;
    /// `int`.
    pub const INT: u8 = 2;
    /// `counter`.
    pub const COUNTER: u8 = 3;
    /// `f64`.
    pub const F64: u8 = 4;
    /// `enum`.
    pub const ENUM: u8 = 5;
    /// `text`.
    pub const TEXT: u8 = 6;
    /// `sym`.
    pub const SYM: u8 = 7;
    /// `set`.
    pub const SET: u8 = 8;
    /// `ref`.
    pub const REF: u8 = 9;
    /// `commitref`.
    pub const COMMITREF: u8 = 10;
    /// `path`.
    pub const PATH: u8 = 11;
    /// `oid`.
    pub const OID: u8 = 12;
    /// `pathmove`.
    pub const PATHMOVE: u8 = 13;
}

/// A `path` value ([F08 §5.2]): a root symbol (≠ 0) and its exact text.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathVal {
    /// Symbol id of class `root`.
    pub root: u16,
    /// Exact path bytes.
    pub text: String,
}

impl PathVal {
    /// Decodes a stored `path` ([F08 §5.2]): `root` ≠ 0, then a non-empty text ([F08 §5.4.1]) under the text rules of
    /// §5.3. Every path of the oracle is read here: values, runtime rows and row images, intent items ([F11 §2.3]).
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let root = r.u16()?;
        if root == 0 {
            return err(at, "path root symbol is 0 [F08 §5.2]");
        }
        let t_at = r.offset();
        let text = r.vstr()?;
        if text.is_empty() {
            return err(t_at, "path text is empty [F08 §5.4.1]");
        }
        text_rules(text, t_at, false)?;
        Ok(PathVal {
            root,
            text: text.to_owned(),
        })
    }

    /// Encodes a stored `path`.
    pub fn encode(&self, w: &mut Writer) {
        w.u16(self.root);
        w.vstr(&self.text);
    }
}

/// A `pathmove` value ([F08 §5.2]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PathMove {
    /// Candidate HLC.
    pub hlc: u64,
    /// 1 explicit, 2 confirmed, 3 committed, 4 observed.
    pub class: u8,
    /// Directory prefix moved from.
    pub from: PathVal,
    /// Directory prefix moved to.
    pub to: PathVal,
    /// Observing git commit, or `none`.
    pub git: Oid,
}

/// One stored value: the logical value of a type byte and its value bytes ([F08 §5.1], §5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `absent` (only where a position admits it).
    Absent,
    /// `bool`.
    Bool(bool),
    /// `int`.
    Int(i64),
    /// `counter`.
    Counter(i64),
    /// `f64` bit pattern.
    F64(u64),
    /// `enum` integer.
    Enum(u16),
    /// `text`, inline.
    Text(String),
    /// `sym`: symbol id of class `text`.
    Sym(u32),
    /// `set`: element type id and elements (each a value of that type).
    Set(u8, Vec<Value>),
    /// `ref`: a `#N`.
    Ref(u32),
    /// `commitref`: a full commit id.
    CommitRef([u8; 32]),
    /// `path`.
    Path(PathVal),
    /// `oid` (never `none` as a value).
    Oid(Oid),
    /// `pathmove`.
    PathMove(Box<PathMove>),
}

/// [F08 §5.3] text rules for a stored text value: no CR, no U+0000 (bodies excepted), one-line when asked.
pub fn text_rules(s: &str, at: usize, one_line: bool) -> Result<()> {
    if let Some(i) = s.bytes().position(|b| b == b'\r') {
        return err(at + i, "CR in a stored text value [F08 §5.3]");
    }
    if let Some(i) = s.bytes().position(|b| b == 0) {
        return err(at + i, "U+0000 in a text value [F08 §5.3]");
    }
    if one_line && let Some(i) = s.bytes().position(|b| b == b'\n') {
        return err(at + i, "LF in a one-line text value [F08 §5.3]");
    }
    Ok(())
}

impl Value {
    /// The type id of the value.
    pub fn type_id(&self) -> u8 {
        match self {
            Value::Absent => ty::ABSENT,
            Value::Bool(_) => ty::BOOL,
            Value::Int(_) => ty::INT,
            Value::Counter(_) => ty::COUNTER,
            Value::F64(_) => ty::F64,
            Value::Enum(_) => ty::ENUM,
            Value::Text(_) => ty::TEXT,
            Value::Sym(_) => ty::SYM,
            Value::Set(..) => ty::SET,
            Value::Ref(_) => ty::REF,
            Value::CommitRef(_) => ty::COMMITREF,
            Value::Path(_) => ty::PATH,
            Value::Oid(_) => ty::OID,
            Value::PathMove(_) => ty::PATHMOVE,
        }
    }

    /// Decodes a type byte and its value bytes. `allow_absent` is true only in the op, key-value and conflict-side
    /// positions of [F06 §5.1].
    pub fn decode(r: &mut Reader<'_>, allow_absent: bool) -> Result<Value> {
        let at = r.offset();
        let tb = r.u8()?;
        if tb & 0x40 != 0 {
            return err(at, "type byte bit 6 is not zero [F08 §5.1]");
        }
        let id = tb & 0x3F;
        let vbit = tb & 0x80 != 0;
        if vbit && id != ty::BOOL {
            return err(at, "vbit set on a type other than bool [F08 §5.1]");
        }
        if id == ty::ABSENT && !allow_absent {
            return err(at, "absent in a position that admits no absence [F08 §5.1]");
        }
        if id == ty::BOOL {
            return Ok(Value::Bool(vbit));
        }
        Self::decode_body(r, id, at)
    }

    /// Decodes the value bytes of type `id` (a set element has no type byte, [F08 §5.2]).
    fn decode_body(r: &mut Reader<'_>, id: u8, at: usize) -> Result<Value> {
        Ok(match id {
            ty::ABSENT => Value::Absent,
            ty::INT => Value::Int(r.svar64()?),
            ty::COUNTER => Value::Counter(r.svar64()?),
            ty::F64 => {
                let f_at = r.offset();
                let b = r.f64_bits()?;
                check_f64(b, f_at)?;
                Value::F64(b)
            }
            ty::ENUM => Value::Enum(r.uvar16()?),
            ty::TEXT => {
                let t_at = r.offset();
                let s = r.vstr()?;
                if s.is_empty() {
                    return err(
                        t_at,
                        "empty text stored as a value; empty is absent [F08 §5.3]",
                    );
                }
                if s.len() > 65_536 {
                    return err(t_at, "text value above 65,536 bytes [F08 §5.3]");
                }
                text_rules(s, t_at, false)?;
                Value::Text(s.to_owned())
            }
            ty::SYM => {
                let s_at = r.offset();
                let s = r.uvar32()?;
                if s == 0 {
                    return err(s_at, "sym value 0 [F08 §5.1]");
                }
                Value::Sym(s)
            }
            ty::SET => {
                let e_at = r.offset();
                let elem = r.u8()?;
                if !matches!(
                    elem,
                    ty::INT
                        | ty::ENUM
                        | ty::TEXT
                        | ty::SYM
                        | ty::REF
                        | ty::COMMITREF
                        | ty::PATH
                        | ty::OID
                        | ty::PATHMOVE
                ) {
                    return err(
                        e_at,
                        format!("set element type {elem:#04x} not admitted [F08 §5.2]"),
                    );
                }
                let n_at = r.offset();
                let n = r.count(1)?;
                if n == 0 {
                    return err(n_at, "set with 0 elements; empty is absent [F08 §5.2]");
                }
                let mut v: Vec<Value> = Vec::with_capacity(n);
                for _ in 0..n {
                    let x_at = r.offset();
                    let x = Self::decode_body(r, elem, x_at)?;
                    if let Some(prev) = v.last()
                        && stored_cmp(prev, &x) != core::cmp::Ordering::Less
                    {
                        return err(x_at, "set elements not strictly ascending [F08 §5.2, §5.5]");
                    }
                    v.push(x);
                }
                Value::Set(elem, v)
            }
            ty::REF => {
                let n_at = r.offset();
                let n = r.u32()?;
                if n == 0 {
                    return err(n_at, "ref value 0 [F08 §5.1]");
                }
                Value::Ref(n)
            }
            ty::COMMITREF => {
                let c_at = r.offset();
                let c = r.b32()?;
                if c == [0; 32] {
                    return err(c_at, "commitref all zero [F08 §5.1]");
                }
                Value::CommitRef(c)
            }
            ty::PATH => Value::Path(PathVal::decode(r)?),
            ty::OID => {
                let o_at = r.offset();
                let o = r.oidv()?;
                if o == Oid::None {
                    return err(
                        o_at,
                        "oid value of algorithm none; empty is absent [F08 §5.3]",
                    );
                }
                Value::Oid(o)
            }
            ty::PATHMOVE => {
                let hlc = r.u64()?;
                let c_at = r.offset();
                let class = r.u8()?;
                if !(1..=4).contains(&class) {
                    return err(c_at, "pathmove class outside 1-4 [F08 §5.2]");
                }
                let f_at = r.offset();
                let from = PathVal::decode(r)?;
                let to = PathVal::decode(r)?;
                if !is_dir_prefix(&from.text) || !is_dir_prefix(&to.text) {
                    return err(
                        f_at,
                        "pathmove from/to is not a directory prefix [F08 §5.4.2]",
                    );
                }
                if from.root != to.root || from.text == to.text {
                    return err(
                        f_at,
                        "pathmove from/to differ in root or are equal [F08 §5.2]",
                    );
                }
                let git = r.oidv()?;
                Value::PathMove(Box::new(PathMove {
                    hlc,
                    class,
                    from,
                    to,
                    git,
                }))
            }
            other => return err(at, format!("type id {other} is reserved [F08 §5.1]")),
        })
    }

    /// Encodes the type byte and the value bytes.
    pub fn encode(&self, w: &mut Writer) {
        match self {
            Value::Bool(b) => w.u8(ty::BOOL | if *b { 0x80 } else { 0 }),
            v => {
                w.u8(v.type_id());
                v.encode_body(w);
            }
        }
    }

    fn encode_body(&self, w: &mut Writer) {
        match self {
            Value::Absent | Value::Bool(_) => {}
            Value::Int(i) | Value::Counter(i) => w.svar(*i),
            Value::F64(b) => w.u64(*b),
            Value::Enum(e) => w.uvar(u64::from(*e)),
            Value::Text(s) => w.vstr(s),
            Value::Sym(s) => w.uvar(u64::from(*s)),
            Value::Set(elem, v) => {
                w.u8(*elem);
                w.uvar(v.len() as u64);
                for x in v {
                    x.encode_body(w);
                }
            }
            Value::Ref(n) => w.u32(*n),
            Value::CommitRef(c) => w.bytes(c),
            Value::Path(p) => p.encode(w),
            Value::Oid(o) => w.oidv(o),
            Value::PathMove(m) => {
                w.u64(m.hlc);
                w.u8(m.class);
                m.from.encode(w);
                m.to.encode(w);
                w.oidv(&m.git);
            }
        }
    }
}

/// [F08 §5.4.2]: a non-empty `RelPath` followed by one `/`.
fn is_dir_prefix(s: &str) -> bool {
    s.len() >= 2 && s.ends_with('/') && !s.ends_with("//") && !s.starts_with('/')
}

/// [F08 §5.3]: NaN, ±infinity and −0.0 are refused wherever found.
pub fn check_f64(bits: u64, at: usize) -> Result<()> {
    let f = f64::from_bits(bits);
    if f.is_nan() || f.is_infinite() || bits == 0x8000_0000_0000_0000 {
        return err(at, "f64 NaN, infinity or -0.0 [F08 §5.3]");
    }
    Ok(())
}

/// The stored order of [F08 §5.5] between two values of one type.
pub fn stored_cmp(a: &Value, b: &Value) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) | (Value::Counter(x), Value::Counter(y)) => x.cmp(y),
        (Value::Enum(x), Value::Enum(y)) => x.cmp(y),
        (Value::Ref(x), Value::Ref(y)) | (Value::Sym(x), Value::Sym(y)) => x.cmp(y),
        (Value::Text(x), Value::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::CommitRef(x), Value::CommitRef(y)) => x.cmp(y),
        (Value::Path(x), Value::Path(y)) => {
            (x.root, x.text.as_bytes()).cmp(&(y.root, y.text.as_bytes()))
        }
        (Value::Oid(x), Value::Oid(y)) => {
            (x.algo_byte(), x.digest()).cmp(&(y.algo_byte(), y.digest()))
        }
        (Value::PathMove(x), Value::PathMove(y)) => (
            x.hlc,
            x.from.text.as_bytes(),
            x.to.text.as_bytes(),
            x.class,
            (x.git.algo_byte(), x.git.digest()),
        )
            .cmp(&(
                y.hlc,
                y.from.text.as_bytes(),
                y.to.text.as_bytes(),
                y.class,
                (y.git.algo_byte(), y.git.digest()),
            )),
        _ => Ordering::Equal,
    }
}

/// One field-block entry ([F08 §6.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldEntry {
    /// Symbol of class `name`, ≠ 0.
    pub field_sym: u32,
    /// The value (never absent).
    pub value: Value,
}

/// The field block ([F08 §6.1]): `n` ≥ 1 entries strictly ascending by `field_sym`.
pub fn decode_field_block(r: &mut Reader<'_>) -> Result<Vec<FieldEntry>> {
    let at = r.offset();
    let n = r.count(2)?;
    if n == 0 {
        return err(at, "field block with 0 entries [F08 §6.1]");
    }
    let mut v: Vec<FieldEntry> = Vec::with_capacity(n);
    for _ in 0..n {
        let e_at = r.offset();
        let field_sym = r.uvar32()?;
        if field_sym == 0 || v.last().is_some_and(|p| p.field_sym >= field_sym) {
            return err(
                e_at,
                "field block entries not strictly ascending by a non-zero field_sym [F08 §6.1]",
            );
        }
        let value = Value::decode(r, false)?;
        v.push(FieldEntry { field_sym, value });
    }
    Ok(v)
}

/// Encodes a field block.
pub fn encode_field_block(v: &[FieldEntry], w: &mut Writer) {
    w.uvar(v.len() as u64);
    for e in v {
        w.uvar(u64::from(e.field_sym));
        e.value.encode(w);
    }
}

/// `NodeHdr` ([F08 §3.1]), 60 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NodeHdr {
    /// Kind id.
    pub kind: u8,
    /// Status.
    pub status: u8,
    /// Resolution.
    pub resolution: u8,
    /// Priority 0–4.
    pub priority: u8,
    /// Criticality.
    pub criticality: u8,
    /// Confidence.
    pub confidence: u8,
    /// Authority.
    pub authority: u8,
    /// §3.2 flags.
    pub flags: u16,
    /// Last touching commit `seq`.
    pub rev_seq: u64,
    /// Parent `#N`.
    pub parent: u32,
    /// Creating commit `seq`.
    pub created_tx: u32,
    /// Last primary change `seq`.
    pub updated_tx: u32,
    /// Head of the op chain.
    pub last_op_lsn: u64,
    /// Derived counters.
    pub open_blockers: u16,
    /// Derived.
    pub open_blockers_exo: u16,
    /// Derived.
    pub children_total: u16,
    /// Derived.
    pub children_done: u16,
    /// `TITLE_BLOB` offset or `NONE32`.
    pub title_off: u32,
    /// `FIELDS_BLOB` offset or `NONE32`.
    pub fields_off: u32,
    /// 1-based `BLOBTAB` index; 0 = none.
    pub body_ref: u32,
}

impl NodeHdr {
    /// Decodes 60 bytes; checks the reserved bytes and bits and the §3.3 structural rules decidable from the row.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let h = NodeHdr {
            kind: r.u8()?,
            status: r.u8()?,
            resolution: r.u8()?,
            priority: r.u8()?,
            criticality: r.u8()?,
            confidence: r.u8()?,
            authority: r.u8()?,
            flags: r.u16()?,
            rev_seq: r.u64()?,
            parent: r.u32()?,
            created_tx: r.u32()?,
            updated_tx: r.u32()?,
            last_op_lsn: r.u64()?,
            open_blockers: r.u16()?,
            open_blockers_exo: r.u16()?,
            children_total: r.u16()?,
            children_done: r.u16()?,
            title_off: r.u32()?,
            fields_off: r.u32()?,
            body_ref: r.u32()?,
        };
        r.zeros(3, "NodeHdr._reserved")?;
        if h.flags & 0xFF00 != 0 {
            return err(at + 7, "NodeHdr.flags bits 8-15 are not zero [F08 §3.2]");
        }
        if h.kind == 0 && h != NodeHdr::default() {
            return err(at, "a kind-0 NodeHdr row is not all zero [F08 §3.3]");
        }
        if h.kind != 0 && !((1..=13).contains(&h.kind) || (64..=254).contains(&h.kind)) {
            return err(at, format!("NodeHdr.kind {} is invalid [F08 §3.3]", h.kind));
        }
        if h.kind != 0 && h.priority > 4 {
            return err(at + 3, "NodeHdr.priority above 4 [F08 §3.3]");
        }
        Ok(h)
    }

    /// Encodes 60 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.kind);
        w.u8(self.status);
        w.u8(self.resolution);
        w.u8(self.priority);
        w.u8(self.criticality);
        w.u8(self.confidence);
        w.u8(self.authority);
        w.u16(self.flags);
        w.u64(self.rev_seq);
        w.u32(self.parent);
        w.u32(self.created_tx);
        w.u32(self.updated_tx);
        w.u64(self.last_op_lsn);
        w.u16(self.open_blockers);
        w.u16(self.open_blockers_exo);
        w.u16(self.children_total);
        w.u16(self.children_done);
        w.u32(self.title_off);
        w.u32(self.fields_off);
        w.u32(self.body_ref);
        w.zeros(3);
    }
}

/// `Creator` ([F08 §4]), 6 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Creator {
    /// Symbol of class `actor`.
    pub actor: u32,
    /// Symbol of class `role`.
    pub role: u16,
}

impl Creator {
    /// Decodes 6 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        Ok(Creator {
            actor: r.u32()?,
            role: r.u16()?,
        })
    }

    /// Encodes 6 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u32(self.actor);
        w.u16(self.role);
    }
}

/// `KindSet` ([F08 §8.4.8]), 33 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindSet {
    /// Bit 0 `any`.
    pub ks_flags: u8,
    /// Core kind bits.
    pub core: u64,
    /// Project kind bits.
    pub ext: [u8; 24],
}

impl KindSet {
    /// Decodes and validates 33 bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let k = KindSet {
            ks_flags: r.u8()?,
            core: r.u64()?,
            ext: r.array()?,
        };
        if k.ks_flags & 0xFE != 0 {
            return err(at, "KindSet.ks_flags bits 1-7 are not zero [F08 §8.4.8]");
        }
        if k.core & 1 != 0 {
            return err(at + 1, "KindSet.core bit 0 is set [F08 §8.4.8]");
        }
        if k.ks_flags & 1 != 0 && (k.core != 0 || k.ext != [0; 24]) {
            return err(at, "KindSet with any has non-zero masks [F08 §8.4.8]");
        }
        Ok(k)
    }

    /// Encodes 33 bytes.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.ks_flags);
        w.u64(self.core);
        w.bytes(&self.ext);
    }
}

/// A schema item record ([F08 §8.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Bit 0 `retired` (not for class 5).
    pub iflags: u8,
    /// The class body.
    pub body: ItemBody,
}

/// The class bodies of [F08 §8.5.1]–§8.5.5.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemBody {
    /// Class 1.
    Kind {
        /// Name symbol.
        name: u32,
        /// Store-local id.
        kind_id: u8,
        /// §8.4.7.
        uid_derivation: u8,
        /// 0 or `root-key`.
        root_variant: u8,
        /// §8.4.5.
        existence_policy: u8,
        /// Bits 0–3.
        kflags: u8,
    },
    /// Class 2.
    Field {
        /// Kind symbol or 0 for `*`.
        kind: u32,
        /// Field name symbol.
        name: u32,
        /// Type byte with vbit 0.
        ty: u8,
        /// Element type or 0.
        elem: u8,
        /// Merge class.
        class: u8,
        /// Storage.
        storage: u8,
        /// Declaration order.
        decl: u16,
        /// F2 optional.
        optional: bool,
        /// F2/F5 index.
        index: u8,
        /// F2 coerce.
        coerce: u8,
        /// Bits 0–3.
        cflags: u8,
        /// The default value.
        default: Option<Value>,
        /// The `int` range.
        range: Option<(i64, i64)>,
    },
    /// Class 3.
    EnumValue {
        /// Kind symbol or 0.
        kind: u32,
        /// Field symbol.
        field: u32,
        /// Value name symbol.
        name: u32,
        /// Stored integer.
        value: u16,
        /// F2 rank.
        sort_rank: u16,
        /// Bits 0–1.
        eflags: u8,
        /// Lattice covers.
        covers: Vec<u16>,
    },
    /// Class 4.
    EdgeKind(Box<EdgeKindItem>),
    /// Class 5.
    Query {
        /// Name symbol.
        name: u32,
        /// LQ grammar version.
        lq_version: u16,
        /// Parameter signature.
        params: String,
        /// Shape word.
        shape: String,
        /// Budget class word.
        budget: String,
        /// Portable text.
        text: String,
        /// Canonical-AST hash.
        ast_hash: [u8; 16],
    },
}

/// The edge-kind item body ([F08 §8.5.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeKindItem {
    /// Name symbol.
    pub name: u32,
    /// Store-local id.
    pub edge_id: u8,
    /// 1 structural, 2 historical.
    pub eclass: u8,
    /// §8.4.6.
    pub on_dst: u8,
    /// §8.4.6.
    pub on_src: u8,
    /// §8.4.6.
    pub acyclic: u8,
    /// §8.4.6.
    pub card: u8,
    /// Forest depth.
    pub max_depth: u8,
    /// `none` or `anchor-key`.
    pub uid_derivation: u8,
    /// §8.4.6.
    pub props: u8,
    /// Bits 0–1.
    pub eflags: u8,
    /// LQ name symbol.
    pub lq_name: u32,
    /// F1 sources.
    pub src_kinds: KindSet,
    /// F1 destinations.
    pub dst_kinds: KindSet,
    /// Reverse names.
    pub reverse_names: Vec<u32>,
    /// Reading template.
    pub reading: String,
}

fn in_range(v: u8, lo: u8, hi: u8, at: usize, what: &str) -> Result<u8> {
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        err(at, format!("{what} {v} outside {lo}-{hi} [F08 §8.4]"))
    }
}

fn byte_in(r: &mut Reader<'_>, lo: u8, hi: u8, what: &str) -> Result<u8> {
    let at = r.offset();
    let v = r.u8()?;
    in_range(v, lo, hi, at, what)
}

fn flags_mask(r: &mut Reader<'_>, allowed: u8, what: &str) -> Result<u8> {
    let at = r.offset();
    let v = r.u8()?;
    if v & !allowed != 0 {
        return err(at, format!("{what} has reserved bits set"));
    }
    Ok(v)
}

impl Item {
    /// Decodes an item record ([F08 §8.5]).
    pub fn decode(r: &mut Reader<'_>) -> Result<Item> {
        let at = r.offset();
        let class = r.u8()?;
        let f_at = r.offset();
        let iflags = r.u8()?;
        if iflags & 0xFE != 0 || (class == 5 && iflags != 0) {
            return err(
                f_at,
                "item iflags reserved bits set, or retired on a named query [F08 §8.5]",
            );
        }
        let body = match class {
            1 => ItemBody::Kind {
                name: r.uvar32()?,
                kind_id: r.u8()?,
                uid_derivation: byte_in(r, 1, 4, "kind uid_derivation")?,
                root_variant: {
                    let a = r.offset();
                    let v = r.u8()?;
                    if v != 0 && v != 3 {
                        return err(
                            a,
                            "kind root_variant is neither 0 nor root-key [F08 §8.5.1]",
                        );
                    }
                    v
                },
                existence_policy: byte_in(r, 1, 3, "existence_policy")?,
                kflags: flags_mask(r, 0x0F, "kind kflags")?,
            },
            2 => {
                let kind = r.uvar32()?;
                let name = r.uvar32()?;
                let t_at = r.offset();
                let ty = r.u8()?;
                if ty & 0xC0 != 0 || ty == 0 || ty > ty::PATHMOVE {
                    return err(t_at, "field type byte invalid or with vbit [F08 §8.5.2]");
                }
                let e_at = r.offset();
                let elem = r.u8()?;
                if (ty == ty::SET) != (elem != 0) {
                    return err(e_at, "field elem is non-zero exactly for set [F08 §8.5.2]");
                }
                let class = byte_in(r, 0, 15, "field merge class")?;
                let storage = byte_in(r, 1, 6, "field storage")?;
                let decl = r.u16()?;
                let optional = r.bool8()?;
                let index = byte_in(r, 0, 2, "field index")?;
                let coerce = byte_in(r, 0, 3, "field coerce")?;
                let cflags = flags_mask(r, 0x0F, "field cflags")?;
                let default = if cflags & 1 != 0 {
                    Some(Value::decode(r, false)?)
                } else {
                    None
                };
                let range = if cflags & 2 != 0 {
                    let a = r.offset();
                    let lo = r.svar64()?;
                    let hi = r.svar64()?;
                    if hi < lo {
                        return err(a, "field range_max below range_min [F08 §8.5.2]");
                    }
                    Some((lo, hi))
                } else {
                    None
                };
                ItemBody::Field {
                    kind,
                    name,
                    ty,
                    elem,
                    class,
                    storage,
                    decl,
                    optional,
                    index,
                    coerce,
                    cflags,
                    default,
                    range,
                }
            }
            3 => {
                let kind = r.uvar32()?;
                let field = r.uvar32()?;
                let name = r.uvar32()?;
                let value = r.uvar16()?;
                let sort_rank = r.u16()?;
                let eflags = flags_mask(r, 0x03, "enum eflags")?;
                let n = usize::from(r.u8()?);
                let mut covers = Vec::with_capacity(n);
                for _ in 0..n {
                    covers.push(r.uvar16()?);
                }
                ItemBody::EnumValue {
                    kind,
                    field,
                    name,
                    value,
                    sort_rank,
                    eflags,
                    covers,
                }
            }
            4 => {
                let name = r.uvar32()?;
                let edge_id = r.u8()?;
                let eclass = byte_in(r, 1, 2, "edge class")?;
                let on_dst = byte_in(r, 1, 9, "on_dst")?;
                let on_src = byte_in(r, 1, 8, "on_src")?;
                let acyclic = byte_in(r, 0, 4, "acyclic")?;
                let card = byte_in(r, 0, 5, "card")?;
                let max_depth = r.u8()?;
                let u_at = r.offset();
                let uid_derivation = r.u8()?;
                if uid_derivation != 0 && uid_derivation != 4 {
                    return err(
                        u_at,
                        "edge uid_derivation neither none nor anchor-key [F08 §8.5.4]",
                    );
                }
                let props = byte_in(r, 0, 3, "edge props")?;
                let eflags = flags_mask(r, 0x03, "edge eflags")?;
                let lq_name = r.uvar32()?;
                let src_kinds = KindSet::decode(r)?;
                let dst_kinds = KindSet::decode(r)?;
                let n = usize::from(r.u8()?);
                let mut reverse_names = Vec::with_capacity(n);
                for _ in 0..n {
                    reverse_names.push(r.uvar32()?);
                }
                let rd_at = r.offset();
                let reading = r.vstr()?;
                if reading.is_empty()
                    || reading.len() > 200
                    || !reading.bytes().all(|b| (0x20..0x7F).contains(&b))
                    || reading.matches("{a}").count() != 1
                    || reading.matches("{b}").count() != 1
                {
                    return err(
                        rd_at,
                        "edge reading is not ASCII one-line 1-200 B with {a} and {b} once [F08 §8.5.4]",
                    );
                }
                ItemBody::EdgeKind(Box::new(EdgeKindItem {
                    name,
                    edge_id,
                    eclass,
                    on_dst,
                    on_src,
                    acyclic,
                    card,
                    max_depth,
                    uid_derivation,
                    props,
                    eflags,
                    lq_name,
                    src_kinds,
                    dst_kinds,
                    reverse_names,
                    reading: reading.to_owned(),
                }))
            }
            5 => {
                let name = r.uvar32()?;
                let v_at = r.offset();
                let lq_version = r.u16()?;
                if lq_version == 0 {
                    return err(v_at, "named query lq_version 0 [F08 §8.5.5]");
                }
                ItemBody::Query {
                    name,
                    lq_version,
                    params: r.vstr()?.to_owned(),
                    shape: r.vstr()?.to_owned(),
                    budget: r.vstr()?.to_owned(),
                    text: r.vstr()?.to_owned(),
                    ast_hash: r.b16()?,
                }
            }
            c => return err(at, format!("schema item class {c} outside 1-5 [F08 §8.5]")),
        };
        Ok(Item { iflags, body })
    }

    /// The item class.
    pub fn class(&self) -> u8 {
        match self.body {
            ItemBody::Kind { .. } => 1,
            ItemBody::Field { .. } => 2,
            ItemBody::EnumValue { .. } => 3,
            ItemBody::EdgeKind(_) => 4,
            ItemBody::Query { .. } => 5,
        }
    }

    /// Encodes the item record.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.class());
        w.u8(self.iflags);
        match &self.body {
            ItemBody::Kind {
                name,
                kind_id,
                uid_derivation,
                root_variant,
                existence_policy,
                kflags,
            } => {
                w.uvar(u64::from(*name));
                w.u8(*kind_id);
                w.u8(*uid_derivation);
                w.u8(*root_variant);
                w.u8(*existence_policy);
                w.u8(*kflags);
            }
            ItemBody::Field {
                kind,
                name,
                ty,
                elem,
                class,
                storage,
                decl,
                optional,
                index,
                coerce,
                cflags,
                default,
                range,
            } => {
                w.uvar(u64::from(*kind));
                w.uvar(u64::from(*name));
                w.u8(*ty);
                w.u8(*elem);
                w.u8(*class);
                w.u8(*storage);
                w.u16(*decl);
                w.bool8(*optional);
                w.u8(*index);
                w.u8(*coerce);
                w.u8(*cflags);
                if let Some(d) = default {
                    d.encode(w);
                }
                if let Some((lo, hi)) = range {
                    w.svar(*lo);
                    w.svar(*hi);
                }
            }
            ItemBody::EnumValue {
                kind,
                field,
                name,
                value,
                sort_rank,
                eflags,
                covers,
            } => {
                w.uvar(u64::from(*kind));
                w.uvar(u64::from(*field));
                w.uvar(u64::from(*name));
                w.uvar(u64::from(*value));
                w.u16(*sort_rank);
                w.u8(*eflags);
                w.u8(covers.len() as u8);
                for c in covers {
                    w.uvar(u64::from(*c));
                }
            }
            ItemBody::EdgeKind(e) => {
                w.uvar(u64::from(e.name));
                w.u8(e.edge_id);
                w.u8(e.eclass);
                w.u8(e.on_dst);
                w.u8(e.on_src);
                w.u8(e.acyclic);
                w.u8(e.card);
                w.u8(e.max_depth);
                w.u8(e.uid_derivation);
                w.u8(e.props);
                w.u8(e.eflags);
                w.uvar(u64::from(e.lq_name));
                e.src_kinds.encode(w);
                e.dst_kinds.encode(w);
                w.u8(e.reverse_names.len() as u8);
                for n in &e.reverse_names {
                    w.uvar(u64::from(*n));
                }
                w.vstr(&e.reading);
            }
            ItemBody::Query {
                name,
                lq_version,
                params,
                shape,
                budget,
                text,
                ast_hash,
            } => {
                w.uvar(u64::from(*name));
                w.u16(*lq_version);
                w.vstr(params);
                w.vstr(shape);
                w.vstr(budget);
                w.vstr(text);
                w.bytes(ast_hash);
            }
        }
    }
}

/// The edge property block ([F08 §10.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeProps {
    /// Bits 0–2.
    pub pflags: u8,
    /// `pinned_commit` when `has_pin`.
    pub pinned_commit: Option<[u8; 32]>,
    /// The anchor record when `anchor`.
    pub anchor: Option<Box<AnchorRec>>,
}

impl EdgeProps {
    /// Decodes a block.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let pflags = r.u8()?;
        if pflags & 0xF8 != 0 {
            return err(at, "edge pflags bits 3-7 are not zero [F08 §10.2]");
        }
        let pinned_commit = if pflags & 1 != 0 {
            let c_at = r.offset();
            let c = r.b32()?;
            if c == [0; 32] {
                return err(c_at, "pinned_commit is all zero [F08 §10.2]");
            }
            Some(c)
        } else {
            None
        };
        let anchor = if pflags & 4 != 0 {
            Some(Box::new(AnchorRec::decode(r)?))
        } else {
            None
        };
        Ok(EdgeProps {
            pflags,
            pinned_commit,
            anchor,
        })
    }

    /// Encodes a block.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.pflags);
        if let Some(c) = &self.pinned_commit {
            w.bytes(c);
        }
        if let Some(a) = &self.anchor {
            a.encode(w);
        }
    }

    /// [F08 §10.2]: the bits a kind's `props` admits (0 none, 1 pinned, 2 flagged, 3 anchor).
    pub fn admitted_by(&self, props: u8) -> bool {
        match props {
            0 => self.pflags == 0,
            1 => self.pflags & !1 == 0,
            2 => self.pflags & !2 == 0,
            3 => self.pflags == 4,
            _ => false,
        }
    }
}

/// One scope segment ([F08 §10.3.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeSeg {
    /// Item kind by language.
    pub skind: u8,
    /// Item name.
    pub name: String,
    /// Qualifier; may be empty.
    pub qual: String,
}

/// The scope value ([F08 §10.3.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// 1 rust, 2 markdown, 3 toml.
    pub lang: u8,
    /// 1–64 segments, outermost first.
    pub segments: Vec<ScopeSeg>,
}

impl Scope {
    /// Decodes a scope value from its bytes.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let lang = byte_in(r, 1, 3, "scope lang")?;
        let n_at = r.offset();
        let n = usize::from(r.u8()?);
        if !(1..=64).contains(&n) {
            return err(n_at, "scope segment count outside 1-64 [F08 §10.3.1]");
        }
        let hi = match lang {
            1 => 9,
            2 => 6,
            _ => 3,
        };
        let mut segments = Vec::with_capacity(n);
        for _ in 0..n {
            let skind = byte_in(r, 1, hi, "scope skind")?;
            let a = r.offset();
            let name = r.vstr()?;
            if name.is_empty() {
                return err(a, "scope segment name is empty [F08 §10.3.1]");
            }
            text_rules(name, a, true)?;
            let q = r.offset();
            let qual = r.vstr()?;
            text_rules(qual, q, true)?;
            segments.push(ScopeSeg {
                skind,
                name: name.to_owned(),
                qual: qual.to_owned(),
            });
        }
        Ok(Scope { lang, segments })
    }

    /// Encodes a scope value.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.lang);
        w.u8(self.segments.len() as u8);
        for s in &self.segments {
            w.u8(s.skind);
            w.vstr(&s.name);
            w.vstr(&s.qual);
        }
    }
}

/// The window value W ([F20 §2.7.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// Hashes before the span.
    pub before: Vec<u16>,
    /// Hashes after the span.
    pub after: Vec<u16>,
}

impl Window {
    /// Decodes W from its whole byte string; counts above `WIN` are invalid ([F20 §2.7.3]).
    pub fn decode(b: &[u8], at: usize) -> Result<Self> {
        let mut r = Reader::with_base(b, at);
        let nb = usize::from(r.u16()?);
        let na = usize::from(r.u16()?);
        if nb > holes::WIN_MAX || na > holes::WIN_MAX {
            return err(at, "window count above WIN [F20 §2.7.3]");
        }
        let mut before = Vec::with_capacity(nb);
        for _ in 0..nb {
            before.push(r.u16()?);
        }
        let mut after = Vec::with_capacity(na);
        for _ in 0..na {
            after.push(r.u16()?);
        }
        r.finish("the window value")?;
        Ok(Window { before, after })
    }

    /// Encodes W.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u16(self.before.len() as u16);
        w.u16(self.after.len() as u16);
        for h in self.before.iter().chain(&self.after) {
            w.u16(*h);
        }
        w.into_vec()
    }
}

/// A selector text held exactly or as its digest ([F08 §10.3] orders 12–19).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Held {
    /// The exact bytes of N.
    Text(Vec<u8>),
    /// BLAKE3-128 of them (`text_unavailable`).
    Digest([u8; 16]),
}

impl Held {
    /// BLAKE3-128 of the text, or the stored digest ([F07 §8.2]).
    pub fn digest(&self) -> [u8; 16] {
        match self {
            Held::Text(t) => crate::prim::blake3_128(t),
            Held::Digest(d) => *d,
        }
    }
}

/// The quote group of an anchor ([F08 §10.3] orders 12–17).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    /// `quote` or `quote_h`.
    pub quote: Held,
    /// `prefix` or `prefix_h`.
    pub prefix: Held,
    /// `suffix` or `suffix_h`.
    pub suffix: Held,
}

/// The anchor record ([F08 §10.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorRec {
    /// Bits 0–5.
    pub aflags: u16,
    /// The anchor uid.
    pub uid: [u8; 16],
    /// 1 file … 6 lines.
    pub kind: u8,
    /// 1 live, 2 pinned.
    pub mode: u8,
    /// 1 header, 2 span.
    pub watch: u8,
    /// Resolver version ≥ 1.
    pub resolver: u16,
    /// Capture digest.
    pub captured: [u8; 16],
    /// Predecessor term.
    pub pred: Option<[u8; 16]>,
    /// Hint lines (`kind` ≠ file).
    pub hint: Option<(u32, u32)>,
    /// Scope value bytes, decoded.
    pub scope: Option<Scope>,
    /// Quote group for kinds 2–5.
    pub quote: Option<Quote>,
    /// End quote for `range`.
    pub end: Option<Held>,
    /// Occurrence ≥ 1.
    pub occurrence: Option<u16>,
    /// Window (`kind` ≠ file).
    pub window: Option<Window>,
    /// Span hash (`kind` ≠ file).
    pub span_hash: Option<u64>,
    /// File `oid` at capture, or `none` for a planned `file` anchor.
    pub blob: Oid,
    /// Observed git commit.
    pub git: Option<Oid>,
    /// In-file marker id.
    pub marker: Option<String>,
}

impl AnchorRec {
    /// Decodes an anchor record with every V rule of [F08 §10.3] "Validity".
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let aflags = r.u16()?;
        if aflags & 0xFFC0 != 0 {
            return err(at, "anchor aflags bits 6-15 are not zero [F08 §10.3]");
        }
        let uid = r.b16()?;
        let kind = byte_in(r, 1, 6, "anchor kind")?;
        let mode = byte_in(r, 1, 2, "anchor mode")?;
        let watch = byte_in(r, 1, 2, "anchor watch")?;
        let rv_at = r.offset();
        let resolver = r.u16()?;
        if resolver == 0 {
            return err(rv_at, "anchor resolver version 0 [F08 §10.3]");
        }
        let captured = r.b16()?;
        let unavailable = aflags & 0x20 != 0;
        let has_quote = (2..=5).contains(&kind);
        if unavailable && !has_quote {
            return err(
                at,
                "text_unavailable on an anchor kind without a quote [F08 §10.3]",
            );
        }
        let pred = if aflags & 1 != 0 {
            Some(r.b16()?)
        } else {
            None
        };
        let hint = if kind != 1 {
            let h_at = r.offset();
            let first = r.u32()?;
            let last = r.u32()?;
            if first == 0 || last < first {
                return err(h_at, "anchor hint lines not 1 <= first <= last [F08 §10.3]");
            }
            Some((first, last))
        } else {
            None
        };
        let scope = if aflags & 2 != 0 {
            let s_at = r.offset();
            let b = r.vbytes()?;
            if b.is_empty() {
                return err(s_at, "anchor scope is empty [F08 §10.3]");
            }
            let mut sr = Reader::with_base(b, r.offset() - b.len());
            let s = Scope::decode(&mut sr)?;
            sr.finish("the scope value")?;
            Some(s)
        } else {
            None
        };
        let held = |r: &mut Reader<'_>| -> Result<Held> {
            Ok(if unavailable {
                Held::Digest(r.b16()?)
            } else {
                Held::Text(r.vbytes()?.to_vec())
            })
        };
        let quote = if has_quote {
            Some(Quote {
                quote: held(r)?,
                prefix: held(r)?,
                suffix: held(r)?,
            })
        } else {
            None
        };
        let end = if kind == 5 { Some(held(r)?) } else { None };
        let occurrence = if aflags & 4 != 0 {
            let o_at = r.offset();
            let o = r.u16()?;
            if o == 0 {
                return err(o_at, "anchor occurrence 0 [F08 §10.3]");
            }
            Some(o)
        } else {
            None
        };
        let (window, span_hash) = if kind != 1 {
            let w_at = r.offset();
            let b = r.vbytes()?;
            if b.is_empty() {
                return err(w_at, "anchor window is empty [F08 §10.3]");
            }
            let wb_at = r.offset() - b.len();
            (Some(Window::decode(b, wb_at)?), Some(r.u64()?))
        } else {
            (None, None)
        };
        let b_at = r.offset();
        let blob = r.oidv()?;
        if blob == Oid::None && kind != 1 {
            return err(
                b_at,
                "anchor blob none on a kind other than file [F08 §10.3]",
            );
        }
        let git = if aflags & 8 != 0 {
            let g_at = r.offset();
            let g = r.oidv()?;
            if g == Oid::None {
                return err(g_at, "anchor git of algorithm none [F08 §10.3]");
            }
            Some(g)
        } else {
            None
        };
        let marker = if aflags & 16 != 0 {
            let m_at = r.offset();
            let m = r.vstr()?;
            if m.is_empty() || m.len() > 64 {
                return err(m_at, "anchor marker not 1-64 bytes [F08 §10.3]");
            }
            text_rules(m, m_at, true)?;
            Some(m.to_owned())
        } else {
            None
        };
        Ok(AnchorRec {
            aflags,
            uid,
            kind,
            mode,
            watch,
            resolver,
            captured,
            pred,
            hint,
            scope,
            quote,
            end,
            occurrence,
            window,
            span_hash,
            blob,
            git,
            marker,
        })
    }

    /// Encodes the record.
    pub fn encode(&self, w: &mut Writer) {
        w.u16(self.aflags);
        w.bytes(&self.uid);
        w.u8(self.kind);
        w.u8(self.mode);
        w.u8(self.watch);
        w.u16(self.resolver);
        w.bytes(&self.captured);
        if let Some(p) = &self.pred {
            w.bytes(p);
        }
        if let Some((a, b)) = self.hint {
            w.u32(a);
            w.u32(b);
        }
        if let Some(s) = &self.scope {
            let mut sw = Writer::new();
            s.encode(&mut sw);
            w.vbytes(sw.as_slice());
        }
        let put = |w: &mut Writer, h: &Held| match h {
            Held::Text(t) => w.vbytes(t),
            Held::Digest(d) => w.bytes(d),
        };
        if let Some(q) = &self.quote {
            put(w, &q.quote);
            put(w, &q.prefix);
            put(w, &q.suffix);
        }
        if let Some(e) = &self.end {
            put(w, e);
        }
        if let Some(o) = self.occurrence {
            w.u16(o);
        }
        if let Some(win) = &self.window {
            w.vbytes(&win.encode());
        }
        if let Some(h) = self.span_hash {
            w.u64(h);
        }
        w.oidv(&self.blob);
        if let Some(g) = &self.git {
            w.oidv(g);
        }
        if let Some(m) = &self.marker {
            w.vstr(m);
        }
    }
}

/// `uid_file(r, p, q)` ([F08 §11.2]).
pub fn uid_file(root: &str, path: &str, pred: Option<&[u8; 16]>) -> [u8; 16] {
    let mut v = Vec::new();
    crate::prim::lp(&mut v, b"moirai-file-v1");
    crate::prim::lp(&mut v, root.as_bytes());
    crate::prim::lp(&mut v, path.as_bytes());
    crate::prim::lp(&mut v, pred.map_or(&[][..], |p| &p[..]));
    crate::prim::blake3_128(&v)
}

/// `uid_root(r)` ([F08 §11.3]).
pub fn uid_root(root: &str) -> [u8; 16] {
    let mut v = Vec::new();
    crate::prim::lp(&mut v, b"moirai-root-v1");
    crate::prim::lp(&mut v, root.as_bytes());
    crate::prim::blake3_128(&v)
}

/// `uid_anchor(s, c, p)` ([F08 §11.4]).
pub fn uid_anchor(src: &[u8; 16], captured: &[u8; 16], pred: Option<&[u8; 16]>) -> [u8; 16] {
    let mut v = Vec::new();
    crate::prim::lp(&mut v, b"moirai-anchor-v1");
    crate::prim::lp(&mut v, src);
    crate::prim::lp(&mut v, captured);
    crate::prim::lp(&mut v, pred.map_or(&[][..], |p| &p[..]));
    crate::prim::blake3_128(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prim::unhex;

    fn hexb(s: &str) -> Vec<u8> {
        unhex(&s.replace(' ', "").to_lowercase()).unwrap()
    }

    fn rt(bytes: &[u8]) -> Value {
        let mut r = Reader::new(bytes);
        let v = Value::decode(&mut r, true).unwrap();
        assert!(r.is_empty(), "value consumed exactly");
        let mut w = Writer::new();
        v.encode(&mut w);
        assert_eq!(w.as_slice(), bytes, "re-encode");
        v
    }

    /// [F08 §5.6]: one value of every type in its stored form (U, C, D chosen as 0x11/0x22/0x33 fills).
    #[test]
    fn one_value_of_every_type() {
        assert_eq!(rt(&[0x00]), Value::Absent);
        assert_eq!(rt(&[0x01]), Value::Bool(false));
        assert_eq!(rt(&[0x81]), Value::Bool(true));
        assert_eq!(rt(&hexb("02 05")), Value::Int(-3));
        assert_eq!(rt(&hexb("03 D8 04")), Value::Counter(300));
        assert_eq!(
            rt(&hexb("04 00 00 00 00 00 00 F8 3F")),
            Value::F64(1.5f64.to_bits())
        );
        assert_eq!(rt(&hexb("05 02")), Value::Enum(2));
        assert_eq!(rt(&hexb("06 02 6F 6B")), Value::Text("ok".into()));
        assert_eq!(rt(&hexb("07 07")), Value::Sym(7));
        assert_eq!(
            rt(&hexb("08 02 02 02 0A")),
            Value::Set(2, vec![Value::Int(1), Value::Int(5)])
        );
        assert_eq!(rt(&hexb("09 28 00 00 00")), Value::Ref(40));
        let mut c = vec![0x0A];
        c.extend_from_slice(&[0x22; 32]);
        assert_eq!(rt(&c), Value::CommitRef([0x22; 32]));
        assert_eq!(
            rt(&hexb("0B 01 00 09 64 6F 63 73 2F 61 2E 6D 64")),
            Value::Path(PathVal {
                root: 1,
                text: "docs/a.md".into()
            })
        );
        let mut o = vec![0x0C, 0x01];
        o.extend_from_slice(&[0x33; 20]);
        assert_eq!(rt(&o), Value::Oid(Oid::Sha1([0x33; 20])));
        let pm = rt(&hexb(
            "0D 03 00 00 6C 50 C4 A0 01 01 01 00 02 61 2F 01 00 02 62 2F 00",
        ));
        match pm {
            Value::PathMove(m) => {
                assert_eq!(m.hlc, 0x01A0_C450_6C00_0003);
                assert_eq!(m.class, 1);
                assert_eq!(m.from.text, "a/");
                assert_eq!(m.to.text, "b/");
                assert_eq!(m.git, Oid::None);
            }
            other => panic!("{other:?}"),
        }
    }

    /// [F08 §5.3]: refused patterns.
    #[test]
    fn value_refusals() {
        let bad: &[&str] = &[
            "04 00 00 00 00 00 00 F8 7F", // NaN
            "04 00 00 00 00 00 00 F0 7F", // +inf
            "04 00 00 00 00 00 00 00 80", // -0.0
            "06 00",                      // empty text
            "07 00",                      // sym 0
            "08 02 00",                   // empty set
            "08 02 02 0A 02",             // not ascending
            "09 00 00 00 00",             // ref 0
            "0C 00",                      // oid none
            "0E",                         // reserved type
            "42",                         // bit 6
            "82",                         // vbit on int
            "06 02 0D 0A",                // CR
        ];
        for h in bad {
            assert!(
                Value::decode(&mut Reader::new(&hexb(h)), true).is_err(),
                "{h}"
            );
        }
        assert!(Value::decode(&mut Reader::new(&[0]), false).is_err());
    }

    /// [F08 §6.2] the informative 12-byte field block.
    #[test]
    fn field_block_example() {
        let b = hexb("03 05 08 07 02 07 0C 15 02 06 1E 81");
        let mut r = Reader::new(&b);
        let fb = decode_field_block(&mut r).unwrap();
        assert!(r.is_empty());
        assert_eq!(fb.len(), 3);
        assert_eq!(
            fb[0].value,
            Value::Set(7, vec![Value::Sym(7), Value::Sym(12)])
        );
        assert_eq!(fb[1].value, Value::Int(3));
        assert_eq!(fb[2].value, Value::Bool(true));
        let mut w = Writer::new();
        encode_field_block(&fb, &mut w);
        assert_eq!(w.as_slice(), &b[..]);
    }

    /// [F08 §3.1] a 60-byte row built from the offset table; a kind-0 row must be all zero.
    #[test]
    fn node_hdr_round_trip() {
        let h = NodeHdr {
            kind: 1,
            status: 1,
            priority: 2,
            criticality: 2,
            authority: 4,
            flags: 0x0008,
            rev_seq: 9,
            created_tx: 3,
            updated_tx: 9,
            last_op_lsn: 4660,
            title_off: 0,
            fields_off: NONE32,
            ..NodeHdr::default()
        };
        let mut w = Writer::new();
        h.encode(&mut w);
        assert_eq!(w.len(), 60);
        assert_eq!(&w.as_slice()[45..49], &0u32.to_le_bytes());
        assert_eq!(NodeHdr::decode(&mut Reader::new(w.as_slice())).unwrap(), h);
        let mut z = [0u8; 60];
        assert!(NodeHdr::decode(&mut Reader::new(&z)).is_ok());
        z[9] = 1;
        assert!(NodeHdr::decode(&mut Reader::new(&z)).is_err());
    }

    /// [F08 §11.2] the informative 46 hashed bytes of `uid_file("project", "docs/a.md", empty)`.
    #[test]
    fn uid_file_input() {
        let want = hexb(
            "0E000000 6D6F697261692D66696C652D7631 07000000 70726F6A656374 09000000 646F63732F612E6D64 00000000",
        );
        assert_eq!(want.len(), 46);
        assert_eq!(
            uid_file("project", "docs/a.md", None),
            crate::prim::blake3_128(&want)
        );
    }

    /// [F08 §10.2], §10.3: a `quote` anchor block round trip, text-unavailable digests, admitted bits.
    #[test]
    fn anchor_block_round_trip() {
        let a = AnchorRec {
            aflags: 0b0000_0101,
            uid: [1; 16],
            kind: 4,
            mode: 1,
            watch: 2,
            resolver: 1,
            captured: [2; 16],
            pred: Some([3; 16]),
            hint: Some((10, 12)),
            scope: None,
            quote: Some(Quote {
                quote: Held::Text(b"fn main".to_vec()),
                prefix: Held::Text(Vec::new()),
                suffix: Held::Text(b"()".to_vec()),
            }),
            end: None,
            occurrence: Some(2),
            window: Some(Window {
                before: vec![7],
                after: vec![8, 9],
            }),
            span_hash: Some(0xABCD),
            blob: Oid::Sha1([4; 20]),
            git: None,
            marker: None,
        };
        let p = EdgeProps {
            pflags: 4,
            pinned_commit: None,
            anchor: Some(Box::new(a)),
        };
        let mut w = Writer::new();
        p.encode(&mut w);
        let back = EdgeProps::decode(&mut Reader::new(w.as_slice())).unwrap();
        assert_eq!(back, p);
        assert!(back.admitted_by(3));
        assert!(!back.admitted_by(0));
        let mut bad = w.into_vec();
        bad[0] = 0x0C;
        assert!(EdgeProps::decode(&mut Reader::new(&bad)).is_err());
    }

    /// [F20 §2.7.3]: the window length rule and the WIN bound.
    #[test]
    fn window_rules() {
        let w = Window {
            before: vec![1, 2],
            after: vec![],
        };
        let b = w.encode();
        assert_eq!(b.len(), 4 + 2 * 2);
        assert_eq!(Window::decode(&b, 0).unwrap(), w);
        let mut over = vec![17, 0, 0, 0];
        over.extend(std::iter::repeat_n(0u8, 34));
        assert!(Window::decode(&over, 0).is_err());
        assert!(Window::decode(&b[..7], 0).is_err());
    }

    /// [F08 §8.5] item records: a field item with default and range; a named query.
    #[test]
    fn item_round_trip() {
        let f = Item {
            iflags: 0,
            body: ItemBody::Field {
                kind: 3,
                name: 9,
                ty: ty::INT,
                elem: 0,
                class: 1,
                storage: 4,
                decl: 20,
                optional: false,
                index: 1,
                coerce: 0,
                cflags: 3,
                default: Some(Value::Int(1)),
                range: Some((0, 10)),
            },
        };
        let q = Item {
            iflags: 0,
            body: ItemBody::Query {
                name: 4,
                lq_version: 1,
                params: String::new(),
                shape: "nodes".into(),
                budget: "small".into(),
                text: "MATCH (t:task) RETURN t".into(),
                ast_hash: [0; 16],
            },
        };
        for it in [f, q] {
            let mut w = Writer::new();
            it.encode(&mut w);
            let mut r = Reader::new(w.as_slice());
            assert_eq!(Item::decode(&mut r).unwrap(), it);
            assert!(r.is_empty());
        }
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    fn text() -> impl Strategy<Value = String> {
        "[^\r\u{0}]{1,40}"
    }

    fn scalar() -> impl Strategy<Value = Value> {
        prop_oneof![
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(Value::Int),
            any::<i64>().prop_map(Value::Counter),
            any::<f64>()
                .prop_filter("finite, not -0.0", |x| {
                    x.is_finite() && x.to_bits() != 0x8000_0000_0000_0000
                })
                .prop_map(|x| Value::F64(x.to_bits())),
            any::<u16>().prop_map(Value::Enum),
            text().prop_map(Value::Text),
            (1..=u32::MAX).prop_map(Value::Sym),
            (1..=u32::MAX).prop_map(Value::Ref),
            any::<[u8; 32]>()
                .prop_filter("not all zero", |c| *c != [0; 32])
                .prop_map(Value::CommitRef),
            any::<[u8; 20]>().prop_map(|h| Value::Oid(Oid::Sha1(h))),
            any::<[u8; 32]>().prop_map(|h| Value::Oid(Oid::Sha256(h))),
        ]
    }

    fn set(elem: u8, mut v: Vec<Value>) -> Value {
        v.sort_by(stored_cmp);
        v.dedup_by(|a, b| stored_cmp(a, b) == core::cmp::Ordering::Equal);
        Value::Set(elem, v)
    }

    fn value() -> impl Strategy<Value = Value> {
        prop_oneof![
            4 => scalar(),
            1 => proptest::collection::vec(any::<i64>().prop_map(Value::Int), 1..8)
                .prop_map(|v| set(ty::INT, v)),
            1 => proptest::collection::vec(text().prop_map(Value::Text), 1..8)
                .prop_map(|v| set(ty::TEXT, v)),
            1 => proptest::collection::vec((1..=u32::MAX).prop_map(Value::Ref), 1..8)
                .prop_map(|v| set(ty::REF, v)),
        ]
    }

    proptest! {
        /// [F08 §5]: every admitted value decodes back from its encoding, and the encoding is the only one accepted.
        #[test]
        fn value_round_trip(v in value()) {
            let mut w = Writer::new();
            v.encode(&mut w);
            let b = w.into_vec();
            let mut r = Reader::new(&b);
            let back = Value::decode(&mut r, false).unwrap();
            prop_assert!(r.is_empty());
            prop_assert_eq!(&back, &v);
            let mut w2 = Writer::new();
            back.encode(&mut w2);
            prop_assert_eq!(w2.as_slice(), b.as_slice());
        }

        /// [F08 §5.1]: decoding arbitrary bytes either refuses them or yields a value that re-encodes to the bytes it
        /// consumed.
        #[test]
        fn value_decode_is_canonical(b in proptest::collection::vec(any::<u8>(), 1..48)) {
            let mut r = Reader::new(&b);
            if let Ok(v) = Value::decode(&mut r, true) {
                let used = b.len() - r.remaining();
                let mut w = Writer::new();
                v.encode(&mut w);
                prop_assert_eq!(w.as_slice(), &b[..used]);
            }
        }
    }
}
