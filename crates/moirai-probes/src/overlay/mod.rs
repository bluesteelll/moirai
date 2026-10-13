//! Measurement 10's compact tail overlay (WP-53d): what a process builds when it replays a log tail on open
//! (`docs/ARCHITECTURE-RESEARCH.md:709`, [AR §4.5] step 1): "a sorted `#N` → newest record lsn index (12–16 B per op)
//! plus the derived-state ± lists, values decoded on probe … lazy runtime records are indexed by key". Its tables
//! have the shapes of the segment and runtime-table chapters:
//!
//! | Table | Shape | Specification |
//! |---|---|---|
//! | node index | `#N` → the lsn of the newest `Commit` whose ops have `#N` as owner, sorted, unique (16 B) | `AR:709`; owners [F06 §2.3] (`06-commit.md:55-59`) |
//! | ± lists | per bitset key, `plus` and `minus` strictly ascending, disjoint, no 0, relative to the base | [F09 §4.6] (`09-segments.md:319-332`); keys [F09 §9.2] (`:577-594`) |
//! | `ALLOC` | the tail's new `#N`, dense from the first: (`uid`, `ref_id`, `create_seq`), 24 B | [F11 §9.1] (`11-runtime-tables.md:669-700`) |
//! | `UIDX` | `uid` → `#N`, sorted by `uid`, 20 B | [F11 §9.2] (`:702-714`) |
//! | runtime index | `K_RT` rows by key → (lsn, row index), the newest per key, not decoded | [F05 §10.4] (`05-log.md:1296-1302`) |
//! | symbols | the tail's `SymDefs` definitions in log order | [F05 §10.5] (`05-log.md:1304-1307`) |
//! | offsets (offsets mode only) | parallel to the node index: where `#N`'s frames lie in its newest record, the offset of its first frame from the record's lsn and their bytes (8 B; no frame count, so no 255-frame cap) | design-A's A2 (`design-A.md:135-144`); u20 cut D-2 |
//!
//! **The offsets mode** ([`Overlay::replay_at`], u20 cut D-2, I-4) builds the node entry's offset and length during the
//! replay, from the frame spans the decode walks anyway ([`crate::tail::scan::decode_record_spans`]), in its own builder
//! beside the plain one. The plain [`Overlay::replay`] keeps ba4fc23's types and code: its node map holds the lsn
//! alone, it computes no span, and its offsets are empty. The product's packed entry is 16 B (`AR:709`, R-53 F-3); the
//! probe's offsets mode charges 24 B per node, and every stop rule keeps the generator's 16 B.
//!
//! A probe ([`Overlay::get`]) reads the record the node index names through a [`LogSource`], decodes the owner's ops
//! and follows their `prev` ([F06 §7.3], `06-commit.md:705-713`) while it stays in the tail; values are never stored.
//! T1's diagnostic reads [`Overlay::get_op`] and [`Overlay::get_op_at`] ([`op`]) return the same values on a valid tail
//! while they decode only the owner's ops, located by their frames ([F06 §7.1], [F06 §7.9]) or by an offset kept beside
//! the node index. The ± lists cover the bitset keys the subset changes: family 1 (kind) and 2 (kind, status) of live nodes, and
//! family 3 predicate 3 `deleted` (the probe's reading of [F09 §9.2]'s families; their membership rules are [F08]'s
//! and [F13]'s, which the probe does not use).
//!
//! **The charge.** [`Overlay::heap_bytes`] is Σ `len × size_of` over the overlay's owned slices; after
//! [`Builder::finish`] every table is a `Box<[T]>` of exact length, so this is what a counting allocator sees
//! (`crates/moirai-os/src/windows/mem.rs:57-64`: requested sizes). It is the probe's `ovl(T)` ([F17 §5.1],
//! `17-store-parameters.md:319`), a function of the tail alone.

use crate::tail::commit::{Op, TValue};
use crate::tail::rec::{Payload, RecHdr, Record};
use crate::tail::scan::{
    Expect, GroupView, LogSource, RawRecord, ScanError, decode_record, decode_record_spans, scan,
};
use crate::tail::{Locator, REC_HDR_LEN, TRAILER_LEN};
use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

/// The base the tail applies to: nodes `1..=n` exist and are live, of kind `kind`, with status `#N mod statuses` and
/// resolution 0, and hold no field, body, parent or edge; new `#N` start at `n + 1`. The probe's model of the segment
/// set below the tail.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Base {
    /// The node count.
    pub n: u32,
    /// The kind of every base node.
    pub kind: u8,
    /// The number of status codes.
    pub statuses: u8,
}

impl Base {
    /// The status of base node `id`.
    pub fn status(&self, id: u32) -> u8 {
        (id % u32::from(self.statuses.max(1))) as u8
    }

    /// The existence and membership state of `id` in the base.
    fn state(&self, id: u32) -> Derived {
        if id >= 1 && id <= self.n {
            Derived {
                exist: Exist::Live,
                kind: self.kind,
                status: Some(self.status(id)),
            }
        } else {
            Derived {
                exist: Exist::Absent,
                kind: 0,
                status: None,
            }
        }
    }
}

/// A bitset key in [F09 §9.2]'s `BMDIR` form: `family`, `kind`, `status`, `pred`.
pub type BitsetKey = [u8; 4];

/// Family 1: the live nodes of kind `k`.
pub const fn kind_key(k: u8) -> BitsetKey {
    [1, k, 0, 0]
}

/// Family 2: the live nodes of kind `k` with status `s`.
pub const fn kind_status_key(k: u8, s: u8) -> BitsetKey {
    [2, k, s, 0]
}

/// Family 3, predicate 3: the deleted nodes.
pub const DELETED_KEY: BitsetKey = [3, 0, 0, 3];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Exist {
    Absent,
    Live,
    Deleted,
}

/// The derived state of one node that the ± lists depend on.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Derived {
    exist: Exist,
    kind: u8,
    status: Option<u8>,
}

impl Derived {
    /// The bitset keys the node is a member of (at most two).
    fn keys(&self) -> ([BitsetKey; 2], usize) {
        match self.exist {
            Exist::Absent => ([[0; 4]; 2], 0),
            Exist::Deleted => ([DELETED_KEY, [0; 4]], 1),
            Exist::Live => match self.status {
                Some(s) => ([kind_key(self.kind), kind_status_key(self.kind, s)], 2),
                None => ([kind_key(self.kind), [0; 4]], 1),
            },
        }
    }
}

/// The ± contribution of a node: the keys it joined (`plus`) and left (`minus`) relative to its base state.
fn delta(base: Derived, now: Derived) -> (Vec<BitsetKey>, Vec<BitsetKey>) {
    let (b, nb) = base.keys();
    let (f, nf) = now.keys();
    let (b, f) = (&b[..nb], &f[..nf]);
    let plus = f.iter().filter(|k| !b.contains(k)).copied().collect();
    let minus = b.iter().filter(|k| !f.contains(k)).copied().collect();
    (plus, minus)
}

/// One entry of the node index: 16 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NodeEntry {
    /// The owner `#N`.
    pub n: u32,
    /// The lsn of the newest `Commit` whose ops have `#N` as owner.
    pub lsn: u64,
}

/// Where a node's frames lie in its newest record (u20 cut D-2; design-A's A2): 8 bytes beside its [`NodeEntry`].
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Loc {
    /// The offset of its first frame from the record's lsn, the header included.
    pub off: u32,
    /// The bytes of its frames there.
    pub len: u32,
}

/// The ± list of one bitset key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlusMinus {
    /// The key.
    pub key: BitsetKey,
    /// Members added relative to the base, strictly ascending.
    pub plus: Box<[u32]>,
    /// Members removed, strictly ascending.
    pub minus: Box<[u32]>,
}

/// An `ALLOC` row ([F11 §9.1]): 24 bytes; row i is `#N` = first + i.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AllocRow {
    /// The node's uid.
    pub uid: [u8; 16],
    /// The ref of the creating commit.
    pub ref_id: u32,
    /// The creating commit's `seq`; 0 is a hole.
    pub create_seq: u32,
}

/// A `UIDX` row ([F11 §9.2]): 20 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct UidxRow {
    /// The uid.
    pub uid: [u8; 16],
    /// Its `#N`.
    pub n: u32,
}

/// The newest `FileObs` row of one key (`n`, `tree`): 32 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FileObsEntry {
    /// The tree key.
    pub tree: [u8; 16],
    /// The file node.
    pub n: u32,
    /// The row's index in its batch.
    pub row: u32,
    /// The record's lsn.
    pub lsn: u64,
}

/// The newest `TreeReg` epoch of one key (`tree`, `scope_kind`, `scope_ref`) ([F05 §9.23], `05-log.md:1030`): 32 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EpochEntry {
    /// The tree key.
    pub tree: [u8; 16],
    /// The scope's ref.
    pub scope_ref: u32,
    /// The scope kind.
    pub scope_kind: u8,
    /// The record's lsn.
    pub lsn: u64,
}

/// A symbol the tail defines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Symbol {
    /// The class.
    pub class: u8,
    /// The id.
    pub id: u32,
    /// The string.
    pub text: Box<str>,
}

/// A key of the subset a probe asks for ([F06 §6.1], `06-commit.md:600-612`).
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Key {
    /// The existence key.
    Exist,
    /// The status key.
    Status,
    /// The hierarchy key.
    Hier,
    /// A field key.
    Field(u32),
    /// The body key.
    Body,
    /// A plain edge key (`ekind`, `dst`).
    Edge(u8, u32),
}

/// A value of a key, decoded from the op that set it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    /// Live, with its kind.
    Live(u8),
    /// Deleted, with `reason` and `replaced_by`.
    Deleted(u32, u32),
    /// (status, resolution).
    Status(u8, u8),
    /// (parent, order).
    Hier(u32, String),
    /// A field's typed value bytes ([F06 §5.1]).
    Field(Vec<u8>),
    /// A body hash, or none.
    Body(Option<[u8; 16]>),
    /// Whether the edge exists.
    Edge(bool),
}

/// What a probe finds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Probe {
    /// The tail sets the key: its newest value.
    Tail(Value),
    /// The tail does not set it: the base decides.
    NotInTail,
}

fn field(v: &TValue<'_>) -> Value {
    Value::Field(v.0.to_vec())
}

/// The (key, value) pairs an op sets ([F06 §7.4]–§7.5): an existence op its existence and, for a `Create`, every value
/// key of its image; a value op the new side of its key. The naive map reads the same rules on its own
/// (`overlay/naive.rs`), so the probe's values are checked against it, not shared with it.
pub fn sets(op: &Op<'_>) -> Vec<(Key, Value)> {
    match op {
        Op::Create { kind, image, .. } => {
            let mut v = vec![(Key::Exist, Value::Live(*kind))];
            if let Some((s, r)) = image.status {
                v.push((Key::Status, Value::Status(s, r)));
            }
            if let Some(h) = image.body {
                v.push((Key::Body, Value::Body(Some(h))));
            }
            v.extend(image.fields.iter().map(|(n, t)| (Key::Field(*n), field(t))));
            v
        }
        Op::Delete {
            reason,
            replaced_by,
            ..
        } => vec![(Key::Exist, Value::Deleted(*reason, *replaced_by))],
        Op::SetField { name, new, .. } => vec![(Key::Field(*name), field(new))],
        Op::SetStatus { new, .. } => vec![(Key::Status, Value::Status(new.0, new.1))],
        Op::SetBody { new, .. } => vec![(Key::Body, Value::Body(*new))],
        Op::AddEdge(e) => vec![(Key::Edge(e.ekind, e.dst), Value::Edge(true))],
        Op::RemoveEdge(e) => vec![(Key::Edge(e.ekind, e.dst), Value::Edge(false))],
        Op::Move {
            new_parent,
            new_order,
            ..
        } => vec![(Key::Hier, Value::Hier(*new_parent, new_order.to_string()))],
    }
}

/// A multiply-xor hasher for the builder's integer and digest keys (the default SipHash costs more than the probe's
/// replay should spend on hashing).
#[derive(Default)]
struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for c in bytes.chunks(8) {
            let mut w = [0u8; 8];
            w[..c.len()].copy_from_slice(c);
            self.write_u64(u64::from_le_bytes(w));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.write_u64(u64::from(i));
    }

    fn write_u32(&mut self, i: u32) {
        self.write_u64(u64::from(i));
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

type Map<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

/// Builds an [`Overlay`] from a tail's records in log order ([F05 §10.1]). It clones, so a long-lived process can keep
/// its warm state and catch up from it (measurement 14's MCP model, `m14.rs`).
#[derive(Clone, Debug)]
pub struct Builder {
    base: Base,
    start: u64,
    nodes: Map<u32, u64>,
    state: Map<u32, Derived>,
    pm: Map<BitsetKey, (u32, u32)>,
    alloc_first: u32,
    alloc: Vec<AllocRow>,
    uidx: Vec<UidxRow>,
    files: Map<(u32, [u8; 16]), (u64, u32)>,
    epochs: Map<([u8; 16], u8, u32), u64>,
    symbols: Vec<Symbol>,
    sym_bytes: usize,
}

impl Builder {
    /// A builder for a tail that starts at lsn `start` over `base`.
    pub fn new(start: u64, base: Base) -> Builder {
        Builder {
            base,
            start,
            nodes: Map::default(),
            state: Map::default(),
            pm: Map::default(),
            alloc_first: 0,
            alloc: Vec::new(),
            uidx: Vec::new(),
            files: Map::default(),
            epochs: Map::default(),
            symbols: Vec::new(),
            sym_bytes: 0,
        }
    }

    /// Moves node `id` to `now`, keeping the ± counts.
    fn derive(&mut self, id: u32, now: Derived) {
        let base = self.base.state(id);
        let old = self.state.get(&id).copied().unwrap_or(base);
        if old == now {
            return;
        }
        let (p, m) = delta(base, old);
        for k in p {
            self.pm.entry(k).or_default().0 -= 1;
        }
        for k in m {
            self.pm.entry(k).or_default().1 -= 1;
        }
        let (p, m) = delta(base, now);
        for k in p {
            self.pm.entry(k).or_default().0 += 1;
        }
        for k in m {
            self.pm.entry(k).or_default().1 += 1;
        }
        self.state.insert(id, now);
    }

    fn current(&self, id: u32) -> Derived {
        self.state.get(&id).copied().unwrap_or(self.base.state(id))
    }

    /// Applies one record ([F05 §10.3]–§10.5).
    pub fn apply(&mut self, rec: &Record<'_>) {
        for d in &rec.symdefs {
            self.sym_bytes += d.text.len();
            self.symbols.push(Symbol {
                class: d.class,
                id: d.id,
                text: d.text.as_ref().into(),
            });
        }
        match &rec.payload {
            Payload::Commit(c) => {
                for op in &c.ops {
                    let id = op.owner();
                    self.nodes.insert(id, rec.lsn);
                    match op {
                        Op::Create {
                            uid, kind, image, ..
                        } => {
                            let now = Derived {
                                exist: Exist::Live,
                                kind: *kind,
                                status: image.status.map(|s| s.0),
                            };
                            self.derive(id, now);
                            if self.alloc_first == 0 {
                                self.alloc_first = id;
                            }
                            let next = self.alloc_first + self.alloc.len() as u32;
                            if id >= next {
                                self.alloc.resize(
                                    (id - self.alloc_first) as usize,
                                    AllocRow {
                                        uid: [0; 16],
                                        ref_id: 0,
                                        create_seq: 0,
                                    },
                                );
                                self.alloc.push(AllocRow {
                                    uid: *uid,
                                    ref_id: c.ref_id,
                                    create_seq: c.seq as u32,
                                });
                            }
                            self.uidx.push(UidxRow { uid: *uid, n: id });
                        }
                        Op::Delete { .. } => {
                            let d = self.current(id);
                            self.derive(
                                id,
                                Derived {
                                    exist: Exist::Deleted,
                                    ..d
                                },
                            );
                        }
                        Op::SetStatus { new, .. } => {
                            let d = self.current(id);
                            self.derive(
                                id,
                                Derived {
                                    status: Some(new.0),
                                    ..d
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }
            Payload::TreeReg(t) => {
                self.epochs
                    .insert((t.tree, t.epoch.scope_kind, t.epoch.scope_ref), rec.lsn);
            }
            Payload::FileObs(b) => {
                for (i, r) in b.rows.iter().enumerate() {
                    self.files.insert(r.key(), (rec.lsn, i as u32));
                }
            }
            Payload::ExtentHead(_) | Payload::Noop(_) => {}
        }
    }

    /// Decodes and applies every record of a valid group.
    pub fn apply_group(&mut self, g: &GroupView<'_>) -> Result<(), ScanError> {
        for raw in g.records() {
            let rec = decode_record(&raw)?;
            self.apply(&rec);
        }
        Ok(())
    }

    /// The charge the overlay will have once finished: Σ `len × size_of` of its tables as they stand.
    pub fn charge(&self) -> u64 {
        use std::mem::size_of;
        let pm_keys = self.pm.values().filter(|(p, m)| p + m > 0).count();
        let pm_ids: u64 = self.pm.values().map(|(p, m)| u64::from(p + m)).sum();
        (self.nodes.len() * size_of::<NodeEntry>()
            + pm_keys * size_of::<PlusMinus>()
            + self.alloc.len() * size_of::<AllocRow>()
            + self.uidx.len() * size_of::<UidxRow>()
            + self.files.len() * size_of::<FileObsEntry>()
            + self.epochs.len() * size_of::<EpochEntry>()
            + self.symbols.len() * size_of::<Symbol>()
            + self.sym_bytes) as u64
            + pm_ids * size_of::<u32>() as u64
    }

    /// Sorts every table into its final, exact-length form. The overlay reads its tail in the night's form;
    /// [`Overlay::replay`] gives it the form of the tail it replayed.
    pub fn finish(self) -> Overlay {
        let mut nodes: Vec<NodeEntry> = self
            .nodes
            .iter()
            .map(|(&n, &lsn)| NodeEntry { n, lsn })
            .collect();
        nodes.sort_unstable_by_key(|e| e.n);
        let mut lists: BTreeMap<BitsetKey, (Vec<u32>, Vec<u32>)> = BTreeMap::new();
        for (&id, &now) in &self.state {
            let (p, m) = delta(self.base.state(id), now);
            for k in p {
                lists.entry(k).or_default().0.push(id);
            }
            for k in m {
                lists.entry(k).or_default().1.push(id);
            }
        }
        let pm: Vec<PlusMinus> = lists
            .into_iter()
            .filter(|(_, (p, m))| !p.is_empty() || !m.is_empty())
            .map(|(key, (mut p, mut m))| {
                p.sort_unstable();
                m.sort_unstable();
                PlusMinus {
                    key,
                    plus: p.into_boxed_slice(),
                    minus: m.into_boxed_slice(),
                }
            })
            .collect();
        let mut uidx = self.uidx;
        uidx.sort_unstable_by_key(|r| r.uid);
        let mut files: Vec<FileObsEntry> = self
            .files
            .iter()
            .map(|(&(n, tree), &(lsn, row))| FileObsEntry { tree, n, row, lsn })
            .collect();
        files.sort_unstable_by_key(|f| (f.n, f.tree));
        let mut epochs: Vec<EpochEntry> = self
            .epochs
            .iter()
            .map(|(&(tree, scope_kind, scope_ref), &lsn)| EpochEntry {
                tree,
                scope_ref,
                scope_kind,
                lsn,
            })
            .collect();
        epochs.sort_unstable_by_key(|e| (e.tree, e.scope_kind, e.scope_ref));
        Overlay {
            start: self.start,
            locator: Locator::None,
            nodes: nodes.into_boxed_slice(),
            at: Box::default(),
            pm: pm.into_boxed_slice(),
            alloc_first: self.alloc_first,
            alloc: self.alloc.into_boxed_slice(),
            uidx: uidx.into_boxed_slice(),
            files: files.into_boxed_slice(),
            epochs: epochs.into_boxed_slice(),
            symbols: self.symbols.into_boxed_slice(),
        }
    }
}

/// The offsets mode's builder (u20 cut D-2, I-4): a plain [`Builder`] and, beside it, each owner's frame span in its
/// newest record, from [`decode_record_spans`]. Only [`Overlay::replay_at`] uses it.
struct OffsetBuilder {
    plain: Builder,
    at: Map<u32, (u64, Loc)>,
}

impl OffsetBuilder {
    /// Decodes every record of a valid group with its frame spans, applies it, and keeps each owner's newest span.
    fn apply_group(&mut self, g: &GroupView<'_>) -> Result<(), ScanError> {
        for raw in g.records() {
            let (rec, spans) = decode_record_spans(&raw)?;
            self.plain.apply(&rec);
            for (owner, off, len) in spans {
                self.at.insert(owner, (rec.lsn, Loc { off, len }));
            }
        }
        Ok(())
    }

    /// The plain overlay with its offsets, parallel to the node index; refuses a node whose newest record's span is
    /// missing.
    fn finish(self) -> Result<Overlay, ScanError> {
        let mut o = self.plain.finish();
        let at = &self.at;
        o.at = o
            .nodes
            .iter()
            .map(|e| match at.get(&e.n) {
                Some(&(lsn, loc)) if lsn == e.lsn => Ok(loc),
                _ => Err(ScanError {
                    lsn: e.lsn,
                    what: format!("no frame span of #{} in its newest record", e.n),
                }),
            })
            .collect::<Result<Box<[Loc]>, ScanError>>()?;
        Ok(o)
    }
}

/// The compact overlay of one tail.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Overlay {
    start: u64,
    locator: Locator,
    nodes: Box<[NodeEntry]>,
    at: Box<[Loc]>,
    pm: Box<[PlusMinus]>,
    alloc_first: u32,
    alloc: Box<[AllocRow]>,
    uidx: Box<[UidxRow]>,
    files: Box<[FileObsEntry]>,
    epochs: Box<[EpochEntry]>,
    symbols: Box<[Symbol]>,
}

/// Reads the whole record at `lsn` through `source`.
fn read_record<S: LogSource>(
    source: &mut S,
    lsn: u64,
    buf: &mut Vec<u8>,
) -> Result<RecHdr, String> {
    let read = |source: &mut S, at: u64, dst: &mut [u8]| -> Result<(), String> {
        let mut done = 0;
        while done < dst.len() {
            let n = source.read_at(at + done as u64, &mut dst[done..])?;
            if n == 0 {
                return Err(format!("the log ends inside the record at lsn {lsn}"));
            }
            done += n;
        }
        Ok(())
    };
    buf.clear();
    buf.resize(REC_HDR_LEN, 0);
    read(source, lsn, buf)?;
    let hdr = RecHdr::decode(buf[..REC_HDR_LEN].try_into().expect("32 bytes"));
    if (hdr.len as usize) < REC_HDR_LEN || hdr.lsn != lsn {
        return Err(format!("no record at lsn {lsn}"));
    }
    buf.resize(hdr.len as usize, 0);
    read(source, lsn + REC_HDR_LEN as u64, &mut buf[REC_HDR_LEN..])?;
    Ok(hdr)
}

impl Overlay {
    /// Replays `[from, to)` of `source` (a group boundary with chain value `chain_in`) through a buffer of `chunk`
    /// bytes: scan, validate, decode, apply, finish.
    pub fn replay<S: LogSource>(
        source: &mut S,
        expect: &Expect,
        from: u64,
        to: u64,
        chain_in: u64,
        base: Base,
        chunk: usize,
    ) -> Result<Overlay, ScanError> {
        let mut b = Builder::new(from, base);
        scan(source, expect, from, to, chain_in, chunk, &mut |g| {
            b.apply_group(g)
        })?;
        let mut o = b.finish();
        o.locator = expect.locator;
        Ok(o)
    }

    /// [`Overlay::replay`] in the offsets mode (u20 cut D-2): the same scan, validation, decode and apply, and each
    /// node's offset and length in its newest record built from the frame spans the decode walks, so the overlay holds
    /// [`Overlay::at`] beside its node index.
    pub fn replay_at<S: LogSource>(
        source: &mut S,
        expect: &Expect,
        from: u64,
        to: u64,
        chain_in: u64,
        base: Base,
        chunk: usize,
    ) -> Result<Overlay, ScanError> {
        let mut b = OffsetBuilder {
            plain: Builder::new(from, base),
            at: Map::default(),
        };
        scan(source, expect, from, to, chain_in, chunk, &mut |g| {
            b.apply_group(g)
        })?;
        let mut o = b.finish()?;
        o.locator = expect.locator;
        Ok(o)
    }

    /// The value of `key` of node `n`: the newest op of the tail that sets it, found from the node index and the
    /// owner's `prev` chain, decoded from the log; [`Probe::NotInTail`] when the chain leaves the tail first.
    pub fn get<S: LogSource>(&self, source: &mut S, n: u32, key: &Key) -> Result<Probe, String> {
        let Ok(i) = self.nodes.binary_search_by_key(&n, |e| e.n) else {
            return Ok(Probe::NotInTail);
        };
        let mut lsn = self.nodes[i].lsn;
        let mut buf = Vec::new();
        loop {
            let hdr = read_record(source, lsn, &mut buf)?;
            let t = if hdr.group_end() { TRAILER_LEN } else { 0 };
            let rec = decode_record(&RawRecord {
                hdr,
                payload: &buf[REC_HDR_LEN..buf.len() - t],
                locator: self.locator,
            })
            .map_err(|e| e.to_string())?;
            let Payload::Commit(c) = &rec.payload else {
                return Err(format!("the record at lsn {lsn} is not a Commit"));
            };
            let mut prev = None;
            for op in c.ops.iter().filter(|o| o.owner() == n) {
                prev = Some(op.prev());
                if let Some((_, v)) = sets(op).into_iter().find(|(k, _)| k == key) {
                    return Ok(Probe::Tail(v));
                }
            }
            match prev {
                None => return Err(format!("the record at lsn {lsn} holds no op of #{n}")),
                Some(0) => return Ok(Probe::NotInTail),
                Some(p) if lsn.saturating_sub(p) < self.start || p > lsn => {
                    return Ok(Probe::NotInTail);
                }
                Some(p) => lsn -= p,
            }
        }
    }

    /// The form of item 1a's locator the overlay reads its tail's records in.
    pub fn locator(&self) -> Locator {
        self.locator
    }

    /// The node index, sorted by `#N`.
    pub fn nodes(&self) -> &[NodeEntry] {
        &self.nodes
    }

    /// The offsets, parallel to the node index: empty unless the overlay was built in the offsets mode.
    pub fn at(&self) -> &[Loc] {
        &self.at
    }

    /// The ± lists, sorted by key; a list with no entry is not kept.
    pub fn plus_minus(&self) -> &[PlusMinus] {
        &self.pm
    }

    /// The first new `#N` (0 without a `Create`) and the `ALLOC` rows from it.
    pub fn alloc(&self) -> (u32, &[AllocRow]) {
        (self.alloc_first, &self.alloc)
    }

    /// `UIDX`, sorted by uid.
    pub fn uidx(&self) -> &[UidxRow] {
        &self.uidx
    }

    /// The newest `FileObs` row per (`n`, `tree`), sorted by key.
    pub fn files(&self) -> &[FileObsEntry] {
        &self.files
    }

    /// The newest `TreeReg` epoch per (`tree`, `scope_kind`, `scope_ref`), sorted by key.
    pub fn epochs(&self) -> &[EpochEntry] {
        &self.epochs
    }

    /// The tail's symbols in log order.
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// The overlay charge `ovl(T)`: Σ `len × size_of` over the owned slices.
    pub fn heap_bytes(&self) -> u64 {
        use std::mem::size_of;
        let lists: usize = self.pm.iter().map(|p| p.plus.len() + p.minus.len()).sum();
        let text: usize = self.symbols.iter().map(|s| s.text.len()).sum();
        (self.nodes.len() * size_of::<NodeEntry>()
            + self.at.len() * size_of::<Loc>()
            + self.pm.len() * size_of::<PlusMinus>()
            + lists * size_of::<u32>()
            + self.alloc.len() * size_of::<AllocRow>()
            + self.uidx.len() * size_of::<UidxRow>()
            + self.files.len() * size_of::<FileObsEntry>()
            + self.epochs.len() * size_of::<EpochEntry>()
            + self.symbols.len() * size_of::<Symbol>()
            + text) as u64
    }

    /// The number of entries of every table.
    pub fn entries(&self) -> usize {
        self.nodes.len()
            + self.pm.len()
            + self.alloc.len()
            + self.uidx.len()
            + self.files.len()
            + self.epochs.len()
            + self.symbols.len()
    }
}

pub mod op;

#[cfg(test)]
mod naive;

#[cfg(test)]
mod tests {
    use super::naive::{self, Naive};
    use super::*;
    use crate::tail::TAIL_READ_CHUNK;
    use crate::tail::commit::{Edge, Image, blake3_16};
    use crate::tail::scan::{MemLog, decode};
    use crate::tail::testdata::{self, CHAIN_IN, START};

    /// The hand-built tail's base: 50 live nodes of kind 1, status `#N mod 4`.
    const BASE: Base = Base {
        n: 50,
        kind: 1,
        statuses: 4,
    };

    struct Built {
        bytes: Vec<u8>,
        overlay: Overlay,
        naive: Naive,
    }

    fn built() -> Built {
        let x = testdata::expect();
        let hb = testdata::hand_built();
        let end = START + hb.bytes.len() as u64;
        let overlay = Overlay::replay(
            &mut MemLog::new(START, &hb.bytes),
            &x,
            START,
            end,
            CHAIN_IN,
            BASE,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        let recs = decode(&hb.bytes, START, &x, CHAIN_IN).unwrap();
        let naive = Naive::of(&recs, &BASE);
        Built {
            bytes: hb.bytes,
            overlay,
            naive,
        }
    }

    /// Every key the tail sets probes to the naive map's last value; keys the tail never sets are left to the base.
    /// The naive map's values are pinned by hand from `testdata::hand_built`: 14 keys, set by its 15 ops of the eight
    /// kinds (a `Create` sets its existence and each key of its image), the newest op winning (#5's field 2 is
    /// `second`, #7's edge is removed, #51's status is the later `SetStatus`); #9's `Delete` has distinct `reason` and
    /// `replaced_by`, so a swap of the two is red here.
    #[test]
    fn overlay_equals_naive_map_on_a_hand_built_tail() {
        let b = built();
        let mut src = MemLog::new(START, &b.bytes);
        let text = |s: &str| Value::Field(TValue::text(s).0.to_vec());
        let pinned: BTreeMap<(u32, Key), Value> = [
            ((3, Key::Status), Value::Status(1, 0)),
            ((3, Key::Hier), Value::Hier(2, "a0".to_string())),
            ((3, Key::Field(4)), Value::Field(TValue::int(42).0.to_vec())),
            ((5, Key::Field(2)), text("second")),
            ((7, Key::Edge(1, 51)), Value::Edge(false)),
            ((9, Key::Exist), Value::Deleted(3, 52)),
            ((20, Key::Field(6)), text("x")),
            ((21, Key::Field(6)), text("x")),
            ((22, Key::Field(6)), text("x")),
            ((51, Key::Exist), Value::Live(2)),
            ((51, Key::Status), Value::Status(2, 1)),
            ((51, Key::Field(1)), text("title 51")),
            ((51, Key::Body), Value::Body(Some(blake3_16(&[0x5B; 300])))),
            ((52, Key::Exist), Value::Live(1)),
        ]
        .into_iter()
        .collect();
        assert_eq!(b.naive.values, pinned, "the naive map, by hand");
        for ((n, key), v) in &b.naive.values {
            assert_eq!(
                b.overlay.get(&mut src, *n, key),
                Ok(Probe::Tail(v.clone())),
                "#{n} {key:?}"
            );
        }
        // An untouched base node, and keys of touched nodes the tail never sets.
        for (n, key) in [
            (40, Key::Status),
            (5, Key::Status),
            (7, Key::Field(1)),
            (51, Key::Hier),
            (3, Key::Edge(1, 51)),
        ] {
            assert!(!b.naive.values.contains_key(&(n, key.clone())));
            assert_eq!(
                b.overlay.get(&mut src, n, &key),
                Ok(Probe::NotInTail),
                "#{n} {key:?}"
            );
        }
        let pm: BTreeMap<BitsetKey, (Vec<u32>, Vec<u32>)> = b
            .overlay
            .plus_minus()
            .iter()
            .map(|p| (p.key, (p.plus.to_vec(), p.minus.to_vec())))
            .collect();
        assert_eq!(pm, b.naive.plus_minus(&BASE));
        let (first, rows) = b.overlay.alloc();
        let alloc: Vec<(u32, [u8; 16], u32, u32)> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (first + i as u32, r.uid, r.ref_id, r.create_seq))
            .collect();
        assert_eq!(alloc, b.naive.creates);
        let files: BTreeMap<(u32, [u8; 16]), (u64, u32)> = b
            .overlay
            .files()
            .iter()
            .map(|f| ((f.n, f.tree), (f.lsn, f.row)))
            .collect();
        assert_eq!(files, b.naive.files);
        let epochs: BTreeMap<([u8; 16], u8, u32), u64> = b
            .overlay
            .epochs()
            .iter()
            .map(|e| ((e.tree, e.scope_kind, e.scope_ref), e.lsn))
            .collect();
        assert_eq!(epochs, b.naive.epochs);
    }

    /// The node index (`AR:709`): sorted by `#N`, unique, each entry the lsn of the newest commit that touched it.
    #[test]
    fn node_index_is_sorted_with_the_newest_lsn() {
        let b = built();
        let nodes = b.overlay.nodes();
        assert!(nodes.len() >= 8, "a non-empty index");
        assert!(
            nodes.windows(2).all(|w| w[0].n < w[1].n),
            "sorted and unique"
        );
        let mut newest: BTreeMap<u32, u64> = BTreeMap::new();
        for &(n, lsn) in &b.naive.touches {
            newest.insert(n, lsn);
        }
        let got: BTreeMap<u32, u64> = nodes.iter().map(|e| (e.n, e.lsn)).collect();
        assert_eq!(got, newest);
        // #3 and #51 were touched in more than one commit: their entries name the later one.
        let first_touch = |n: u32| b.naive.touches.iter().find(|t| t.0 == n).unwrap().1;
        assert!(got[&3] > first_touch(3) && got[&51] > first_touch(51));
        assert_eq!(std::mem::size_of::<NodeEntry>(), 16, "16 B per entry");
    }

    /// Each op sets the new side of its key ([F06 §7.4]–§7.5), in the probe ([`sets`]) and in the naive map alike,
    /// pinned by hand on ops whose old and new sides differ and on a `Delete` whose `reason` and `replaced_by` differ:
    /// a reading of the old side, a swap or a wrong edge presence is red.
    #[test]
    fn each_op_sets_the_new_side_of_its_key() {
        let edge = Edge {
            src: 7,
            prev: 1,
            ekind: 3,
            dst: 60,
        };
        let field = |t: TValue<'_>| Value::Field(t.0.to_vec());
        let cases: Vec<(Op<'static>, Vec<(Key, Value)>)> = vec![
            (
                Op::Create {
                    id: 61,
                    prev: 0,
                    uid: [0xC2; 16],
                    kind: 4,
                    c_actor: 5,
                    c_role: 2,
                    image: Image {
                        status: Some((3, 1)),
                        body: Some([0xB1; 16]),
                        fields: vec![(1, TValue::text("t")), (6, TValue::int(-7))],
                    },
                },
                vec![
                    (Key::Exist, Value::Live(4)),
                    (Key::Status, Value::Status(3, 1)),
                    (Key::Body, Value::Body(Some([0xB1; 16]))),
                    (Key::Field(1), field(TValue::text("t"))),
                    (Key::Field(6), field(TValue::int(-7))),
                ],
            ),
            (
                Op::Delete {
                    id: 9,
                    prev: 1,
                    reason: 4,
                    replaced_by: 60,
                    before: Image::default(),
                },
                vec![(Key::Exist, Value::Deleted(4, 60))],
            ),
            (
                Op::SetField {
                    id: 5,
                    prev: 1,
                    name: 2,
                    old: TValue::text("old"),
                    new: TValue::text("new"),
                },
                vec![(Key::Field(2), field(TValue::text("new")))],
            ),
            (
                Op::SetStatus {
                    id: 3,
                    prev: 1,
                    old: (1, 0),
                    new: (2, 3),
                },
                vec![(Key::Status, Value::Status(2, 3))],
            ),
            (
                Op::SetBody {
                    id: 51,
                    prev: 1,
                    old: Some([0xB1; 16]),
                    new: Some([0xB2; 16]),
                },
                vec![(Key::Body, Value::Body(Some([0xB2; 16])))],
            ),
            (
                Op::SetBody {
                    id: 51,
                    prev: 1,
                    old: Some([0xB2; 16]),
                    new: None,
                },
                vec![(Key::Body, Value::Body(None))],
            ),
            (
                Op::AddEdge(edge),
                vec![(Key::Edge(3, 60), Value::Edge(true))],
            ),
            (
                Op::RemoveEdge(edge),
                vec![(Key::Edge(3, 60), Value::Edge(false))],
            ),
            (
                Op::Move {
                    id: 3,
                    prev: 1,
                    old_parent: 2,
                    new_parent: 8,
                    old_order: "a0".into(),
                    new_order: "b1".into(),
                },
                vec![(Key::Hier, Value::Hier(8, "b1".to_string()))],
            ),
        ];
        for (op, want) in &cases {
            assert_eq!(&sets(op), want, "the probe: {op:?}");
            assert_eq!(&naive::written(op), want, "the naive map: {op:?}");
        }
    }

    /// A probe of a key the newest record of its owner does not set follows the owner's `prev` back
    /// ([F06 §7.3], `06-commit.md:707-713`): #51's title is in the `Create` two records back, and #3's status in the
    /// record before its newest.
    #[test]
    fn probe_walks_the_prev_chain() {
        let b = built();
        let mut src = MemLog::new(START, &b.bytes);
        assert_eq!(
            b.overlay.get(&mut src, 3, &Key::Status),
            Ok(Probe::Tail(Value::Status(1, 0)))
        );
        let headers = src.reads.iter().filter(|&&(_, n)| n == REC_HDR_LEN).count();
        assert_eq!(
            headers, 2,
            "the newest record of #3, then the one its prev names"
        );
        let mut src = MemLog::new(START, &b.bytes);
        assert_eq!(
            b.overlay.get(&mut src, 51, &Key::Field(1)),
            Ok(Probe::Tail(Value::Field(
                TValue::text("title 51").0.to_vec()
            )))
        );
        let headers = src.reads.iter().filter(|&&(_, n)| n == REC_HDR_LEN).count();
        assert_eq!(headers, 3, "three records of #51's chain");
        // #5's status: its chain leaves the tail ("not in the tail", the base decides).
        let mut src = MemLog::new(START, &b.bytes);
        assert_eq!(
            b.overlay.get(&mut src, 5, &Key::Status),
            Ok(Probe::NotInTail)
        );
        assert_eq!(
            src.reads.iter().filter(|&&(_, n)| n == REC_HDR_LEN).count(),
            2
        );
    }

    /// The ± lists ([F09 §4.6], `09-segments.md:319-332`): strictly ascending, disjoint, no 0, every `plus` id outside
    /// the base set and every `minus` id inside it.
    #[test]
    fn plus_minus_lists_are_sorted_disjoint_without_zero() {
        let b = built();
        let pm = b.overlay.plus_minus();
        let asc = |l: &[u32]| l.windows(2).all(|w| w[0] < w[1]) && !l.contains(&0);
        for p in pm {
            assert!(
                !p.plus.is_empty() || !p.minus.is_empty(),
                "{:?} is kept empty",
                p.key
            );
            assert!(asc(&p.plus) && asc(&p.minus), "{:?}", p.key);
            assert!(
                p.plus.iter().all(|i| !p.minus.contains(i)),
                "{:?} disjoint",
                p.key
            );
        }
        assert!(pm.windows(2).all(|w| w[0].key < w[1].key), "sorted by key");
        let get = |k: BitsetKey| {
            pm.iter()
                .find(|p| p.key == k)
                .map(|p| (p.plus.to_vec(), p.minus.to_vec()))
        };
        // #3: status 3 → 1; #9 deleted (status 1); #51 created kind 2 and set to status 2; #52 created kind 1, no status.
        assert_eq!(get(kind_status_key(1, 3)), Some((vec![], vec![3])));
        assert_eq!(get(kind_status_key(1, 1)), Some((vec![3], vec![9])));
        assert_eq!(get(kind_key(1)), Some((vec![52], vec![9])));
        assert_eq!(get(DELETED_KEY), Some((vec![9], vec![])));
        assert_eq!(get(kind_key(2)), Some((vec![51], vec![])));
        assert_eq!(get(kind_status_key(2, 2)), Some((vec![51], vec![])));
        assert_eq!(
            get(kind_status_key(2, 0)),
            None,
            "#51's interim status nets out"
        );
        assert_eq!(pm.len(), 6);
    }

    /// `ALLOC` ([F11 §9.1], `11-runtime-tables.md:673-684`): the tail's new ids, dense from the first, with the creating
    /// commit's `ref_id` and `seq`.
    #[test]
    fn alloc_rows_are_dense_from_the_first_new_id() {
        let b = built();
        let (first, rows) = b.overlay.alloc();
        assert_eq!(first, 51);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].uid, rows[0].ref_id, rows[0].create_seq),
            ([0xC1; 16], 0, 101)
        );
        assert_eq!(
            (rows[1].uid, rows[1].ref_id, rows[1].create_seq),
            ([0x0A; 16], 0, 103)
        );
        assert_eq!(std::mem::size_of::<AllocRow>(), 24);
    }

    /// `UIDX` ([F11 §9.2], `11-runtime-tables.md:704-711`): `uid → #N`, sorted by uid.
    #[test]
    fn uidx_is_sorted_by_uid() {
        let b = built();
        let u: Vec<([u8; 16], u32)> = b.overlay.uidx().iter().map(|r| (r.uid, r.n)).collect();
        assert_eq!(u, vec![([0x0A; 16], 52), ([0xC1; 16], 51)]);
        assert_eq!(std::mem::size_of::<UidxRow>(), 20);
    }

    /// Runtime rows are indexed by key, the newest per key winning ([F05 §10.4], `05-log.md:1298-1299`): file 11's row
    /// is the second settle's, file 10's the first's, and the tree's epoch the second's.
    #[test]
    fn runtime_index_keeps_the_newest_row_per_key() {
        let b = built();
        let recs = decode(&b.bytes, START, &testdata::expect(), CHAIN_IN).unwrap();
        let lsns = |k: u8| -> Vec<u64> {
            recs.iter()
                .filter(|r| r.kind() == k)
                .map(|r| r.lsn)
                .collect()
        };
        let (obs, regs) = (
            lsns(crate::tail::kind::FILE_OBS),
            lsns(crate::tail::kind::TREE_REG),
        );
        assert_eq!((obs.len(), regs.len()), (2, 2));
        let files = b.overlay.files();
        let row = |n: u32| files.iter().find(|f| f.n == n).map(|f| (f.lsn, f.row));
        assert_eq!(files.len(), 2);
        assert_eq!(row(10), Some((obs[0], 0)));
        assert_eq!(row(11), Some((obs[1], 0)), "the newer row of file 11 wins");
        let epochs = b.overlay.epochs();
        assert_eq!(epochs.len(), 1);
        assert_eq!((epochs[0].tree, epochs[0].lsn), ([0xE1; 16], regs[1]));
    }

    /// Tail symbols register in log order ([F05 §10.5], `05-log.md:1306-1307`).
    #[test]
    fn tail_symbols_register_in_log_order() {
        let b = built();
        let s: Vec<(u8, u32, &str)> = b
            .overlay
            .symbols()
            .iter()
            .map(|s| (s.class, s.id, &*s.text))
            .collect();
        assert_eq!(s, vec![(3, 17, "probe:session-1"), (7, 9, "tx.claim")]);
        let n: Vec<(u8, u32, String)> = s.iter().map(|&(c, i, t)| (c, i, t.to_string())).collect();
        assert_eq!(n, b.naive.symbols);
    }

    /// `heap_bytes` is Σ `len × size_of` over every owned slice ([F17 §5.1] `ovl(T)`), and the builder's charge before
    /// `finish` equals it.
    #[test]
    fn heap_bytes_count_every_owned_slice() {
        use std::mem::size_of_val;
        let x = testdata::expect();
        let hb = testdata::hand_built();
        let mut builder = Builder::new(START, BASE);
        for r in decode(&hb.bytes, START, &x, CHAIN_IN).unwrap() {
            builder.apply(&r);
        }
        let charge = builder.charge();
        let o = builder.finish();
        let lists: usize = o
            .plus_minus()
            .iter()
            .map(|p| p.plus.len() + p.minus.len())
            .sum();
        let text: usize = o.symbols().iter().map(|s| s.text.len()).sum();
        let expect = size_of_val(o.nodes())
            + size_of_val(o.plus_minus())
            + lists * 4
            + size_of_val(o.alloc().1)
            + size_of_val(o.uidx())
            + size_of_val(o.files())
            + size_of_val(o.epochs())
            + size_of_val(o.symbols())
            + text;
        assert!(expect > 300, "a non-empty overlay");
        assert_eq!(o.heap_bytes(), expect as u64);
        assert_eq!(charge, o.heap_bytes());
        assert_eq!(
            o.entries(),
            o.nodes().len()
                + o.plus_minus().len()
                + o.alloc().1.len()
                + o.uidx().len()
                + o.files().len()
                + o.epochs().len()
                + o.symbols().len()
        );
        assert_eq!(
            o,
            built().overlay,
            "replay and record-by-record build agree"
        );
    }

    /// A generated tail over a small base (N = 50, so owners repeat and probes walk `prev`): the overlay equals the
    /// naive map on every key the tail sets, and on the ± lists, `ALLOC`, `UIDX`, the runtime index and the symbols
    /// (spec item 2).
    fn equals_naive(seed: u64) {
        use crate::tail::synth::{Clause, generate, tests::test_target};
        let mut t = test_target(Clause::Ops, 50, seed);
        t.threshold = 120;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, true).unwrap();
        let o = tail.overlay().unwrap();
        let naive = Naive::of(&tail.records, &tail.base);
        let mut owners = std::collections::BTreeMap::<u32, u32>::new();
        for &(n, _) in &naive.touches {
            *owners.entry(n).or_default() += 1;
        }
        assert!(owners.values().any(|&c| c > 1), "an owner repeats");
        let mut src = MemLog::new(tail.start, &tail.bytes);
        for ((n, key), v) in &naive.values {
            assert_eq!(
                o.get(&mut src, *n, key),
                Ok(Probe::Tail(v.clone())),
                "#{n} {key:?}"
            );
        }
        assert_eq!(o.get(&mut src, 1, &Key::Field(99)), Ok(Probe::NotInTail));
        let pm: BTreeMap<BitsetKey, (Vec<u32>, Vec<u32>)> = o
            .plus_minus()
            .iter()
            .map(|p| (p.key, (p.plus.to_vec(), p.minus.to_vec())))
            .collect();
        assert_eq!(pm, naive.plus_minus(&tail.base));
        let (first, rows) = o.alloc();
        let alloc: Vec<(u32, [u8; 16], u32, u32)> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (first + i as u32, r.uid, r.ref_id, r.create_seq))
            .collect();
        assert_eq!(alloc, naive.creates);
        let mut uidx: Vec<([u8; 16], u32)> = naive.creates.iter().map(|c| (c.1, c.0)).collect();
        uidx.sort();
        assert_eq!(
            o.uidx().iter().map(|r| (r.uid, r.n)).collect::<Vec<_>>(),
            uidx
        );
        let files: BTreeMap<(u32, [u8; 16]), (u64, u32)> = o
            .files()
            .iter()
            .map(|f| ((f.n, f.tree), (f.lsn, f.row)))
            .collect();
        assert_eq!(files, naive.files);
        let epochs: BTreeMap<([u8; 16], u8, u32), u64> = o
            .epochs()
            .iter()
            .map(|e| ((e.tree, e.scope_kind, e.scope_ref), e.lsn))
            .collect();
        assert_eq!(epochs, naive.epochs);
        let syms: Vec<(u8, u32, String)> = o
            .symbols()
            .iter()
            .map(|s| (s.class, s.id, s.text.to_string()))
            .collect();
        assert_eq!(syms, naive.symbols);
    }

    proptest::proptest! {
        #![proptest_config(crate::tail::enc::tests::config())]

        /// Overlay apply equals a naive map over generated tails (spec item 2).
        #[test]
        fn overlay_apply_equals_naive_map(seed in proptest::prelude::any::<u64>()) {
            equals_naive(seed);
        }
    }

    /// The generator's `ovl(T)`, kept as it appends, is the replayed overlay's `heap_bytes`, for every clause.
    #[test]
    fn heap_bytes_equal_the_generators_ovl() {
        use crate::tail::synth::{Clause, generate, tests::test_target};
        for clause in [
            Clause::Ops,
            Clause::Rec,
            Clause::Body,
            Clause::Ovl,
            Clause::Quiet,
        ] {
            for seed in 1..=4 {
                let tail = generate(&test_target(clause, 1_000_000, seed), false).unwrap();
                let o = tail.overlay().unwrap();
                assert!(o.heap_bytes() > 0);
                assert_eq!(tail.measures.ovl, o.heap_bytes(), "{clause:?} seed {seed}");
            }
        }
    }

    /// A tail in A1 (u20 cut §3.1): the overlay replays it in its form, and on every key the tail sets it probes to the
    /// naive map's value; the hand-built A1 tail sets exactly the night tail's values (its locators change no value),
    /// and generated A1 tails at N = 50 carry locators while owners repeat.
    #[test]
    fn a1_tail_overlay_equals_the_naive_map() {
        use crate::tail::synth::tests::{a1, test_target};
        use crate::tail::synth::{Clause, generate};
        let hb = testdata::hand_built_in(Locator::A1);
        let end = START + hb.bytes.len() as u64;
        let o = Overlay::replay(
            &mut MemLog::new(START, &hb.bytes),
            &hb.expect,
            START,
            end,
            CHAIN_IN,
            BASE,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        assert_eq!(o.locator(), Locator::A1);
        let recs = decode(&hb.bytes, START, &hb.expect, CHAIN_IN).unwrap();
        let naive = Naive::of(&recs, &BASE);
        assert_eq!(
            naive.values,
            built().naive.values,
            "the night tail's values"
        );
        let mut src = MemLog::new(START, &hb.bytes);
        for ((n, key), v) in &naive.values {
            assert_eq!(
                o.get(&mut src, *n, key),
                Ok(Probe::Tail(v.clone())),
                "hand-built A1 #{n} {key:?}"
            );
        }
        assert_eq!(o.get(&mut src, 5, &Key::Status), Ok(Probe::NotInTail));
        for seed in 1..=3 {
            let mut t = a1(test_target(Clause::Ops, 50, seed));
            t.threshold = 120;
            t.rt_bound = Some(u64::MAX);
            let tail = generate(&t, true).unwrap();
            let o = tail.overlay().unwrap();
            assert_eq!(o.locator(), Locator::A1);
            let located = tail
                .records
                .iter()
                .filter(|r| matches!(&r.payload, Payload::Commit(c) if !c.locs.is_empty()))
                .count();
            assert!(located > 10, "seed {seed}: {located} commits with locators");
            let naive = Naive::of(&tail.records, &tail.base);
            let mut src = MemLog::new(tail.start, &tail.bytes);
            for ((n, key), v) in &naive.values {
                assert_eq!(
                    o.get(&mut src, *n, key),
                    Ok(Probe::Tail(v.clone())),
                    "seed {seed} #{n} {key:?}"
                );
            }
        }
    }

    /// The tails of the offsets-mode tests: the hand-built tail and test-profile `ops` tails of 120 ops at N = 50
    /// (owners repeat), in the night's form and in A1: (name, expect, start, bytes, chain value, base).
    fn offset_tails() -> Vec<(String, Expect, u64, Vec<u8>, u64, Base)> {
        use crate::tail::synth::tests::{a1, test_target};
        use crate::tail::synth::{Clause, generate};
        let mut tails = Vec::new();
        for form in [Locator::None, Locator::A1] {
            let hb = testdata::hand_built_in(form);
            tails.push((
                format!("hand-built {form:?}"),
                hb.expect,
                START,
                hb.bytes,
                CHAIN_IN,
                BASE,
            ));
            for seed in 1..=2 {
                let mut t = test_target(Clause::Ops, 50, seed);
                if form == Locator::A1 {
                    t = a1(t);
                }
                t.threshold = 120;
                t.rt_bound = Some(u64::MAX);
                let tail = generate(&t, false).unwrap();
                tails.push((
                    format!("seed {seed} {form:?}"),
                    tail.expect,
                    tail.start,
                    tail.bytes,
                    tail.chain_in,
                    tail.base,
                ));
            }
        }
        tails
    }

    /// The offsets mode (u20 cut D-2): for every node entry, `replay_at`'s offset and length are where the frame walk
    /// ([`op::locate`]) finds the node's frames in its newest record, on [`offset_tails`] at chunks of 13 and 64 KiB;
    /// with its offsets set aside, the overlay is the plain replay's.
    #[test]
    fn replay_at_offsets_equal_the_located_frames() {
        let mut buf = Vec::new();
        let mut checked = 0;
        for (name, x, start, bytes, chain, base) in &offset_tails() {
            let end = start + bytes.len() as u64;
            for chunk in [13, TAIL_READ_CHUNK] {
                let o = Overlay::replay_at(
                    &mut MemLog::new(*start, bytes),
                    x,
                    *start,
                    end,
                    *chain,
                    *base,
                    chunk,
                )
                .unwrap();
                assert_eq!(o.at().len(), o.nodes().len(), "{name}: one offset per node");
                for (e, loc) in o.nodes().iter().zip(o.at()) {
                    let s = op::locate(
                        &mut MemLog::new(*start, bytes),
                        &mut buf,
                        TAIL_READ_CHUNK,
                        e.lsn,
                        e.n,
                    )
                    .unwrap()
                    .unwrap();
                    assert_eq!(
                        (u64::from(loc.off), u64::from(loc.len)),
                        (s.first - e.lsn, s.end - s.first),
                        "{name} chunk {chunk} #{}",
                        e.n
                    );
                    checked += 1;
                }
                let plain = Overlay::replay(
                    &mut MemLog::new(*start, bytes),
                    x,
                    *start,
                    end,
                    *chain,
                    *base,
                    chunk,
                )
                .unwrap();
                let mut stripped = o.clone();
                stripped.at = Box::default();
                assert_eq!(
                    stripped, plain,
                    "{name}: the other tables are the plain replay's"
                );
            }
        }
        assert!(checked > 300, "{checked} offsets checked");
    }

    /// No 255-frame cap (u20 cut D-2; `t1.md:348-353`): a tail whose one commit holds 300 frames of #7 (`SetField` of
    /// names 1–300) replays in the offsets mode with #7's offset and the bytes of all 300 frames, which decode to the
    /// 300 ops; u17's entry of an 8-bit count refuses the same record.
    #[test]
    fn replay_at_holds_an_owner_of_more_than_255_frames() {
        use crate::tail::enc::Reader;
        use crate::tail::rec::Appender;
        let x = testdata::expect();
        let mut app = Appender::new(x.geo, x.epoch, START, CHAIN_IN, testdata::head(&x));
        let ops = |lsn: u64| -> Vec<Op<'static>> {
            (1..=300)
                .map(|name| Op::SetField {
                    id: 7,
                    prev: lsn - (START - 1),
                    name,
                    old: TValue::absent(),
                    new: TValue::int(i64::from(name)),
                })
                .collect()
        };
        let (_, g) = app
            .place(|lsn| {
                let c = testdata::commit(101, START - 1, ops(lsn), "three hundred frames");
                Ok(testdata::commit_group(lsn, c, Vec::new()))
            })
            .unwrap();
        let lsn = g[0].lsn;
        let bytes = app.into_bytes();
        let end = START + bytes.len() as u64;
        let o = Overlay::replay_at(
            &mut MemLog::new(START, &bytes),
            &x,
            START,
            end,
            CHAIN_IN,
            BASE,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        assert_eq!(o.nodes(), [NodeEntry { n: 7, lsn }]);
        assert_eq!(o.at().len(), 1, "#7's offset");
        let loc = o.at()[0];
        let from = (lsn - START) as usize + loc.off as usize;
        let mut r = Reader::new(&bytes[from..from + loc.len as usize]);
        let mut got = Vec::new();
        while r.remaining() > 0 {
            got.push(Op::decode(&mut r).unwrap());
        }
        assert_eq!(got, ops(lsn), "the 300 frames at #7's offset");
        let e = op::OpIndex::build(
            &o,
            &mut MemLog::new(START, &bytes),
            TAIL_READ_CHUNK,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(e.contains("8-bit count"), "{e}");
    }

    /// The offsets mode charges 8 B per node beside the 16-B entry (u20 cut D-2, F-3): for every clause's test-profile
    /// tail in both forms, `replay_at`'s `heap_bytes` is the plain replay's plus 8 × the node index's length, and the
    /// generator's `ovl` stays the plain replay's, so every stop rule keeps the 16-B entry.
    #[test]
    fn replay_at_charges_its_offsets() {
        use crate::tail::synth::tests::{a1, test_target};
        use crate::tail::synth::{Clause, generate};
        for clause in [
            Clause::Ops,
            Clause::Rec,
            Clause::Body,
            Clause::Ovl,
            Clause::Quiet,
        ] {
            for form in [Locator::None, Locator::A1] {
                let mut t = test_target(clause, 1_000_000, 3);
                if form == Locator::A1 {
                    t = a1(t);
                }
                let tail = generate(&t, false).unwrap();
                let plain = tail.overlay().unwrap();
                let at = Overlay::replay_at(
                    &mut MemLog::new(tail.start, &tail.bytes),
                    &tail.expect,
                    tail.start,
                    tail.end,
                    tail.chain_in,
                    tail.base,
                    TAIL_READ_CHUNK,
                )
                .unwrap();
                assert!(!plain.nodes().is_empty());
                assert_eq!(
                    at.heap_bytes(),
                    plain.heap_bytes() + 8 * plain.nodes().len() as u64,
                    "{clause:?} {form:?}"
                );
                assert_eq!(tail.measures.ovl, plain.heap_bytes(), "{clause:?} {form:?}");
            }
        }
        assert_eq!(std::mem::size_of::<Loc>(), 8);
    }

    /// The plain replay builds no offsets (u20 cut I-4): on [`offset_tails`] its `at` is empty, and it equals the
    /// record-by-record build of a plain [`Builder`] (ba4fc23's path) in the tail's form.
    #[test]
    fn plain_replay_builds_no_offsets() {
        for (name, x, start, bytes, chain, base) in &offset_tails() {
            let end = start + bytes.len() as u64;
            let o = Overlay::replay(
                &mut MemLog::new(*start, bytes),
                x,
                *start,
                end,
                *chain,
                *base,
                TAIL_READ_CHUNK,
            )
            .unwrap();
            assert!(o.at().is_empty(), "{name}");
            let mut b = Builder::new(*start, *base);
            for r in decode(bytes, *start, x, *chain).unwrap() {
                b.apply(&r);
            }
            let mut f = b.finish();
            f.locator = x.locator;
            assert_eq!(o, f, "{name}");
        }
    }
}
