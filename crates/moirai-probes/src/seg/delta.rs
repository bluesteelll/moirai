//! The delta segment `seg.d<K>` of a delta checkpoint ([F09 §16.2], `09-segments.md:1085-1089`; [AR §4.5] step 12):
//! the fold of a tail's commits over the base, as touched rows with complete states and ± lists, in the probe's subset
//! of sections.
//!
//! - **Touched rows** (`09-segments.md:378-382`): a row is listed in `IDS` iff a row-scoped section of it differs from
//!   its state in the base. The fold visits the owner of every op, both endpoints of an added or removed edge (I-P3,
//!   `:502-504`), and the old and new parent of a `Move`, whose in-lists hold the reverse of `parent` (`:499-501`), and
//!   [`Fold::finish`] drops a visited row whose state ends equal to the base's, such as an endpoint of an edge added
//!   and removed again; an owner always differs, by `rev_seq`. Each listed row carries its complete state; the other
//!   rows stay in the base.
//! - **The ops** ([F06 §7.4]–§7.5): `Create` makes a row of its kind, status and resolution with the commit's `seq` as
//!   `created_tx` and its creator (`08-data-model.md:89-91`, `:182`); `Delete` makes a tombstone (`flags` = `deleted`
//!   only, `parent` 0, counters 0, `fields_off` `NONE32`, `body_ref` 0, `topo`, `defer_until` and `due` 0, the out-list
//!   kept, `08-data-model.md:157-169`); `SetStatus` sets the status and resolution; `AddEdge` and `RemoveEdge` change
//!   the source's out-list and the destination's in-list; `Move` sets `parent`. Every op sets its owner's `rev_seq`,
//!   `updated_tx` and `last_op_lsn` (`:87-91`). `SetField` and `SetBody` change only these: the field blocks and
//!   `BLOBTAB` are outside the subset. Settles (`K_RT` records) fold into runtime sections the probe does not write.
//!   Nor does it write `TITLE_BLOB`, `UID` rows for creates, `TOMB` rows for deletes, new symbols or the store-level
//!   tables ([F09 §16.2], `09-segments.md:1085-1089`): a delta checkpoint that wrote them does more work, so the
//!   checkpoint measurement 14 times over this delta is a lower bound (`m14`, "What the checkpoint omits").
//!   [`super::full`] writes them for `ckpt-full-4096` (u21 cut §3.3): its fold wraps this one, which stays as it is,
//!   so the night's deltas keep their bytes (`night_delta_bytes_are_unchanged`).
//! - **`PRIO`** ([F09 §5.4]; R-66): a created row's is the default 2 ([F08 §4]), a deleted row keeps its own, and every
//!   other op leaves it; an absent row's is 0. A touched base row starts from the base's `PRIO`.
//! - **Derived columns** (`open_blockers`, the counters, `container`, `topo` order) are not recomputed: the probe's
//!   choice, since the base holds no precedence order to keep; a created row's `topo` is its `#N`.
//! - **± lists** (`09-segments.md:319-332`, `:591-594`): per bitset key, the ids that joined and left it relative to
//!   the base, for the overlay's families (kind and kind-status of live rows, predicate `deleted`), so the delta's lists
//!   equal [`crate::overlay::Overlay::plus_minus`] over the same tail. `BMDIR` and the lists are absent when no set
//!   changed.
//! - **The header** (`09-segments.md:119-128`): `n_rows` the `IDS` length, `from_lsn` the base's `upto_lsn`,
//!   `upto_lsn` and `rt_upto_lsn` the tail's end, `base_seq` the largest `seq` folded. `rt_upto_lsn` = `upto_lsn`
//!   because a main-set layer needs `rt_upto_lsn ≥ upto_lsn` (`:128`, `:136`), although no runtime section is folded.

use super::base::{Columns, Csr, DEFAULT_PRIORITY, put_bitsets, put_prio, put_rows, sections};
use super::bitset::{decode_plus_minus, encode_plus_minus};
use super::{Creator, FLAG_DELETED, Header, NONE32, NodeHdr, Writer, put_u32s, seg_kind, tag};
use crate::overlay::{BitsetKey, DELETED_KEY, kind_key, kind_status_key};
use crate::tail::commit::Op;
use crate::tail::rec::{Payload, Record};
use crate::tail::scan::{Expect, LogSource, decode_record, scan};
use crate::tail::synth::EKINDS;
use std::collections::BTreeMap;

/// The probe's edge code of `parent`'s reverse entries in an in-list: one above the tail's edge codes 1–4, which are
/// opaque (`tail/mod.rs`), so the two never collide.
pub const PARENT_EKIND: u8 = EKINDS + 1;

/// One touched row's complete state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Row {
    /// `#N`.
    pub id: u32,
    /// `NODE`.
    pub node: NodeHdr,
    /// `CREATOR`.
    pub creator: Creator,
    /// `TOPO`, `DEFER`, `DUE`.
    pub cold: [u32; 3],
    /// `PRIO`, the `priority` ([F08 §4]).
    pub prio: u8,
    /// The out-list, `(kind, dst)` sorted.
    pub out: Vec<(u8, u32)>,
    /// The in-list, `(kind, src)` sorted.
    pub inn: Vec<(u8, u32)>,
}

/// A delta: its header, its rows in `IDS` order and its ± lists `(key, plus, minus)` in `BMDIR` order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Delta {
    /// The header.
    pub header: Header,
    /// The touched rows, `#N` ascending.
    pub rows: Vec<Row>,
    /// The non-empty ± lists, by key.
    pub lists: Vec<(BitsetKey, Vec<u32>, Vec<u32>)>,
}

/// The bitset keys a row is a member of: kind and kind-status while live, `deleted` once deleted, none when absent.
pub fn keys_of(h: &NodeHdr) -> Vec<BitsetKey> {
    if h.kind == 0 {
        Vec::new()
    } else if h.deleted() {
        vec![DELETED_KEY]
    } else {
        vec![kind_key(h.kind), kind_status_key(h.kind, h.status)]
    }
}

fn insert(l: &mut Vec<(u8, u32)>, e: (u8, u32)) {
    if let Err(i) = l.binary_search(&e) {
        l.insert(i, e);
    }
}

fn remove(l: &mut Vec<(u8, u32)>, e: (u8, u32)) {
    if let Ok(i) = l.binary_search(&e) {
        l.remove(i);
    }
}

/// The fold of a tail over a base.
pub struct Fold<'c, 'a> {
    base: &'c Columns<'a>,
    rows: BTreeMap<u32, Row>,
    seq: u64,
}

impl<'c, 'a> Fold<'c, 'a> {
    /// A fold over `base`.
    pub fn new(base: &'c Columns<'a>) -> Fold<'c, 'a> {
        Fold {
            base,
            rows: BTreeMap::new(),
            seq: base.view.header.base_seq,
        }
    }

    fn row(&mut self, id: u32) -> &mut Row {
        if !self.rows.contains_key(&id) {
            let r = base_row(self.base, id);
            self.rows.insert(id, r);
        }
        self.rows.get_mut(&id).expect("inserted")
    }

    /// Applies one record: a `Commit`'s ops in order; every other record kind folds into sections outside the subset.
    pub fn apply(&mut self, rec: &Record<'_>) {
        let Payload::Commit(c) = &rec.payload else {
            return;
        };
        let (seq, lsn) = (c.seq, rec.lsn);
        self.seq = self.seq.max(seq);
        for op in &c.ops {
            match op {
                Op::Create {
                    id,
                    kind,
                    c_actor,
                    c_role,
                    image,
                    ..
                } => {
                    let (status, resolution) = image.status.unwrap_or((0, 0));
                    let r = self.row(*id);
                    r.node = NodeHdr {
                        kind: *kind,
                        status,
                        resolution,
                        criticality: 2,
                        authority: 4,
                        created_tx: seq as u32,
                        title_off: NONE32,
                        fields_off: NONE32,
                        ..NodeHdr::default()
                    };
                    r.creator = Creator {
                        actor: *c_actor,
                        role: *c_role as u16,
                    };
                    r.cold = [*id, 0, 0];
                    r.prio = DEFAULT_PRIORITY;
                }
                Op::Delete { id, .. } => {
                    let old = self.row(*id).node.parent;
                    if old != 0 {
                        remove(&mut self.row(old).inn, (PARENT_EKIND, *id));
                    }
                    let r = self.row(*id);
                    r.node = NodeHdr {
                        flags: FLAG_DELETED,
                        parent: 0,
                        open_blockers: 0,
                        open_blockers_exo: 0,
                        children_total: 0,
                        children_done: 0,
                        fields_off: NONE32,
                        body_ref: 0,
                        ..r.node
                    };
                    r.cold = [0, 0, 0];
                }
                Op::SetStatus { id, new, .. } => {
                    let r = self.row(*id);
                    (r.node.status, r.node.resolution) = *new;
                }
                Op::AddEdge(e) => {
                    insert(&mut self.row(e.src).out, (e.ekind, e.dst));
                    insert(&mut self.row(e.dst).inn, (e.ekind, e.src));
                }
                Op::RemoveEdge(e) => {
                    remove(&mut self.row(e.src).out, (e.ekind, e.dst));
                    remove(&mut self.row(e.dst).inn, (e.ekind, e.src));
                }
                Op::Move { id, new_parent, .. } => {
                    let old = self.row(*id).node.parent;
                    if old != 0 {
                        remove(&mut self.row(old).inn, (PARENT_EKIND, *id));
                    }
                    if *new_parent != 0 {
                        insert(&mut self.row(*new_parent).inn, (PARENT_EKIND, *id));
                    }
                    self.row(*id).node.parent = *new_parent;
                }
                Op::SetField { .. } | Op::SetBody { .. } => {}
            }
            let r = self.row(op.owner());
            r.node.rev_seq = seq;
            r.node.updated_tx = seq as u32;
            r.node.last_op_lsn = lsn;
        }
    }

    /// The delta numbered `file_no` that folds up to `upto_lsn`: the visited rows whose state differs from the base's
    /// (F09 §5.1) and the ± lists against the base. The ± keys need the base's `NodeHdr` alone; a row's lists are
    /// decoded from the base only when its `NodeHdr` equals the base's, which no owner's does.
    pub fn finish(self, file_no: u32, upto_lsn: u64) -> Delta {
        let base = self.base;
        let mut lists: BTreeMap<BitsetKey, (Vec<u32>, Vec<u32>)> = BTreeMap::new();
        let mut rows: Vec<Row> = Vec::with_capacity(self.rows.len());
        for (id, r) in self.rows {
            let node = base_node(base, id);
            if r.node == node && r == base_row(base, id) {
                continue;
            }
            let (before, after) = (keys_of(&node), keys_of(&r.node));
            for k in after.iter().filter(|k| !before.contains(k)) {
                lists.entry(*k).or_default().0.push(id);
            }
            for k in before.iter().filter(|k| !after.contains(k)) {
                lists.entry(*k).or_default().1.push(id);
            }
            rows.push(r);
        }
        Delta {
            header: Header {
                seg_kind: seg_kind::DELTA,
                n_rows: rows.len() as u32,
                file_no,
                base_seq: self.seq,
                from_lsn: base.view.header.upto_lsn,
                upto_lsn,
                // F09 requires rt_upto_lsn >= upto_lsn in a main-set layer (09-segments.md:128, :136); the probe folds
                // no runtime section (the module docs).
                rt_upto_lsn: upto_lsn,
                ..Header::default()
            },
            rows,
            lists: lists.into_iter().map(|(k, (p, m))| (k, p, m)).collect(),
        }
    }
}

/// The base's `NodeHdr` of `id`, or an absent row's.
fn base_node(base: &Columns<'_>, id: u32) -> NodeHdr {
    if id >= 1 && id as usize <= base.rows {
        base.node(id as usize - 1)
    } else {
        NodeHdr::default()
    }
}

/// The base's state of `id`: its row, or an absent row.
fn base_row(base: &Columns<'_>, id: u32) -> Row {
    if id >= 1 && id as usize <= base.rows {
        let i = id as usize - 1;
        Row {
            id,
            node: base.node(i),
            creator: base.creator(i),
            cold: base.cold(i),
            prio: base.prio(i),
            out: base.out_list(i),
            inn: base.in_list(i),
        }
    } else {
        Row {
            id,
            ..Row::default()
        }
    }
}

/// Folds `[from, to)` of `source` (a group boundary with chain value `chain_in`) over `base`, reading through a
/// buffer of `chunk` bytes, into the delta numbered `file_no`.
#[allow(clippy::too_many_arguments)]
pub fn fold<S: LogSource>(
    source: &mut S,
    expect: &Expect,
    from: u64,
    to: u64,
    chain_in: u64,
    chunk: usize,
    base: &Columns<'_>,
    file_no: u32,
) -> Result<Delta, String> {
    let mut f = Fold::new(base);
    scan(source, expect, from, to, chain_in, chunk, &mut |g| {
        for raw in g.records() {
            f.apply(&decode_record(&raw)?);
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(f.finish(file_no, to))
}

/// The delta's bytes: `IDS`, the row columns, `PRIO`, both CSR directions, then `BMDIR` and the ± lists when a set
/// changed.
pub fn encode(d: &Delta) -> Result<Vec<u8>, String> {
    let n = d.rows.len();
    let k = d.lists.len();
    let count = sections(k, true) - usize::from(k == 0);
    let mut w = Writer::new(count, 140 * n + 16 * k);
    w.push(tag::IDS, n as u32, |o| {
        let ids: Vec<u32> = d.rows.iter().map(|r| r.id).collect();
        put_u32s(o, &ids);
        Ok(())
    })?;
    let nodes: Vec<NodeHdr> = d.rows.iter().map(|r| r.node).collect();
    let creators: Vec<Creator> = d.rows.iter().map(|r| r.creator).collect();
    let cold: Vec<[u32; 3]> = d.rows.iter().map(|r| r.cold).collect();
    let col = |j: usize| -> Vec<u32> { cold.iter().map(|c| c[j]).collect() };
    put_rows(&mut w, &nodes, &creators, [&col(0), &col(1), &col(2)])?;
    let prios: Vec<u8> = d.rows.iter().map(|r| r.prio).collect();
    put_prio(&mut w, &prios)?;
    for (lists, tags) in [
        (
            Csr::of_lists(d.rows.iter().map(|r| r.out.as_slice())),
            [tag::OUT_OFF, tag::OUT_DST, tag::OUT_KIND],
        ),
        (
            Csr::of_lists(d.rows.iter().map(|r| r.inn.as_slice())),
            [tag::IN_OFF, tag::IN_SRC, tag::IN_KIND],
        ),
    ] {
        let e = lists.ids.len() as u32;
        w.push(tags[0], lists.off.len() as u32, |o| {
            put_u32s(o, &lists.off);
            Ok(())
        })?;
        w.push(tags[1], e, |o| {
            put_u32s(o, &lists.ids);
            Ok(())
        })?;
        w.push(tags[2], e, |o| {
            o.extend_from_slice(&lists.kinds);
            Ok(())
        })?;
    }
    if k > 0 {
        let dir: Vec<(BitsetKey, u32)> = d
            .lists
            .iter()
            .map(|(key, p, m)| (*key, (p.len() + m.len()) as u32))
            .collect();
        put_bitsets(&mut w, &dir, |i, o| {
            encode_plus_minus(&d.lists[i].1, &d.lists[i].2, o).map(|_| ())
        })?;
    }
    w.finish(&d.header)
}

/// Reads a delta back from its bytes.
pub fn decode(bytes: &[u8]) -> Result<Delta, String> {
    let c = Columns::parse(bytes)?;
    if c.view.header.seg_kind != seg_kind::DELTA || c.ids.is_empty() && c.rows > 0 {
        return Err("not a seg-delta with IDS".into());
    }
    let rows = (0..c.rows)
        .map(|i| Row {
            id: c.id(i),
            node: c.node(i),
            creator: c.creator(i),
            cold: c.cold(i),
            prio: c.prio(i),
            out: c.out_list(i),
            inn: c.in_list(i),
        })
        .collect();
    let mut lists = Vec::with_capacity(c.keys.len());
    for (i, key) in c.keys.iter().enumerate() {
        let (p, m) = decode_plus_minus(c.bitset(i)?)?;
        lists.push((*key, p, m));
    }
    Ok(Delta {
        header: c.view.header,
        rows,
        lists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::{Key, Probe, Value};
    use crate::seg::base::{self, tests::spec};
    use crate::seg::{NODE_HDR_LEN, SegView};
    use crate::tail::TAIL_READ_CHUNK;
    use crate::tail::scan::MemLog;
    use crate::tail::synth::{Clause, Tail, generate, tests::test_target};
    use std::collections::BTreeSet;

    /// A tail of `ops` ops over a base of `n` nodes, records kept.
    fn tail(n: u32, ops: u64, seed: u64) -> Tail {
        let mut t = test_target(Clause::Ops, n, seed);
        t.threshold = ops;
        t.rt_bound = Some(u64::MAX);
        generate(&t, true).unwrap()
    }

    fn base_for(t: &Tail) -> Vec<u8> {
        base::build(&base::Spec {
            n: t.base.n,
            upto_lsn: t.start,
            base_seq: 999,
        })
        .unwrap()
    }

    fn fold_of(t: &Tail, c: &Columns<'_>) -> Delta {
        fold(
            &mut MemLog::new(t.start, &t.bytes),
            &t.expect,
            t.start,
            t.end,
            t.chain_in,
            TAIL_READ_CHUNK,
            c,
            1,
        )
        .unwrap()
    }

    /// A delta's bytes read back to the delta that was encoded (spec gate 1, "a re-read delta equal to its input"),
    /// with the header of `09-segments.md:119-128`, `IDS` strictly ascending, every column sized by the rows and every
    /// digest right; a delta with no set change carries no `BMDIR`.
    #[test]
    fn delta_reads_back_equal_to_its_input() {
        for seed in 1..=4 {
            let t = tail(200, 400, seed);
            let b = base_for(&t);
            let c = Columns::parse(&b).unwrap();
            let d = fold_of(&t, &c);
            assert!(d.rows.len() > 50, "seed {seed}: {} rows", d.rows.len());
            assert!(!d.lists.is_empty());
            let bytes = encode(&d).unwrap();
            assert_eq!(decode(&bytes).unwrap(), d, "seed {seed}");
            let v = SegView::parse(&bytes).unwrap();
            v.check_digests().unwrap();
            let h = v.header;
            assert_eq!(
                (h.seg_kind, h.file_no, h.n_rows),
                (4, 1, d.rows.len() as u32)
            );
            assert_eq!(
                (h.from_lsn, h.upto_lsn, h.rt_upto_lsn),
                (t.start, t.end, t.end)
            );
            assert!(h.base_seq >= 1_000, "the largest seq folded");
            let ids: Vec<u32> = d.rows.iter().map(|r| r.id).collect();
            assert!(ids.windows(2).all(|w| w[0] < w[1]) && ids[0] > 0);
            assert_eq!(
                v.entry(tag::NODE).unwrap().len,
                (NODE_HDR_LEN * ids.len()) as u64
            );
            assert_eq!(v.entry(tag::IDS).unwrap().len, 4 * ids.len() as u64);
            assert_eq!(v.table.len(), sections(d.lists.len(), true));
        }
        let quiet = Delta {
            header: Header {
                seg_kind: seg_kind::DELTA,
                n_rows: 1,
                ..Header::default()
            },
            rows: vec![Row {
                id: 7,
                ..Row::default()
            }],
            lists: Vec::new(),
        };
        let bytes = encode(&quiet).unwrap();
        assert!(SegView::parse(&bytes).unwrap().entry(tag::BMDIR).is_none());
        assert_eq!(decode(&bytes).unwrap().rows, quiet.rows);
    }

    /// The delta's ± lists equal the overlay's over the same tail and base ([F09 §4.6]; u10's naive-checked
    /// `plus_minus`), and each `plus` id is outside the base's set while each `minus` id is in it.
    #[test]
    fn delta_plus_minus_equal_the_overlay() {
        for seed in 1..=6 {
            let t = tail(150, 300, seed);
            let b = base_for(&t);
            let c = Columns::parse(&b).unwrap();
            let d = fold_of(&t, &c);
            let o = t.overlay().unwrap();
            let want: Vec<(BitsetKey, Vec<u32>, Vec<u32>)> = o
                .plus_minus()
                .iter()
                .map(|p| (p.key, p.plus.to_vec(), p.minus.to_vec()))
                .collect();
            assert_eq!(d.lists, want, "seed {seed}");
            for (key, plus, minus) in &d.lists {
                if let Ok(f) = c.frozen(*key) {
                    let m = f.members();
                    assert!(plus.iter().all(|x| m.binary_search(x).is_err()));
                    assert!(minus.iter().all(|x| m.binary_search(x).is_ok()));
                } else {
                    assert!(minus.is_empty(), "{key:?} is empty in the base");
                }
            }
        }
    }

    /// Naive per-node state: the whole `NodeHdr`, `CREATOR`, the cold columns (`topo`, `defer_until`, `due`) and
    /// `priority` as F08 gives them, and the out- and in-sets.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct Naive {
        hdr: NodeHdr,
        creator: Creator,
        cold: [u32; 3],
        prio: u8,
        out: BTreeSet<(u8, u32)>,
        inn: BTreeSet<(u8, u32)>,
    }

    /// The naive replay: each touched node's state, from the base's generator.
    struct World {
        s: base::Spec,
        naive: BTreeMap<u32, Naive>,
        touched: BTreeSet<u32>,
    }

    impl World {
        /// The base's state of `id` from its generator ([`base::Spec`]), or an absent row (all zero, F08 §3.3).
        fn start(&self, id: u32) -> Naive {
            if id > self.s.n {
                return Naive::default();
            }
            let s = self.s;
            Naive {
                hdr: s.row(id),
                creator: s.creator(id),
                cold: [s.topo(id), s.defer(id), s.due(id)],
                prio: 2,
                out: s.out_edges(id).into_iter().collect(),
                inn: (1..=s.n)
                    .flat_map(|src| s.out_edges(src).into_iter().map(move |e| (src, e)))
                    .filter(|(_, (_, dst))| *dst == id)
                    .map(|(src, (k, _))| (k, src))
                    .collect(),
            }
        }

        fn get(&mut self, id: u32) -> &mut Naive {
            self.touched.insert(id);
            if !self.naive.contains_key(&id) {
                let v = self.start(id);
                self.naive.insert(id, v);
            }
            self.naive.get_mut(&id).unwrap()
        }

        fn set_parent(&mut self, id: u32, new: u32) {
            let old = self.get(id).hdr.parent;
            if old != 0 {
                self.get(old).inn.remove(&(PARENT_EKIND, id));
            }
            if new != 0 {
                self.get(new).inn.insert((PARENT_EKIND, id));
            }
            self.get(id).hdr.parent = new;
        }

        /// Applies one op of the commit `seq` whose record is at `lsn`.
        fn apply(&mut self, op: &Op<'_>, seq: u64, lsn: u64) {
            let o = op.owner();
            match op {
                Op::Create {
                    kind,
                    c_actor,
                    c_role,
                    image,
                    ..
                } => {
                    let (status, resolution) = image.status.unwrap();
                    let v = self.get(o);
                    // F08 §3.1: the header defaults, `created_tx` the creating commit, no title, field or body in
                    // the probe's subset; §4: `CREATOR` is the commit's actor and role; the probe's `topo` is #N.
                    v.hdr = NodeHdr {
                        kind: *kind,
                        status,
                        resolution,
                        criticality: 2,
                        confidence: 0,
                        authority: 4,
                        created_tx: seq as u32,
                        title_off: NONE32,
                        fields_off: NONE32,
                        ..NodeHdr::default()
                    };
                    v.creator = Creator {
                        actor: *c_actor,
                        role: *c_role as u16,
                    };
                    v.cold = [o, 0, 0];
                    v.prio = 2;
                }
                Op::Delete { .. } => {
                    self.set_parent(o, 0);
                    let v = self.get(o);
                    // F08 §3.5 (`08-data-model.md:157-169`): the enumerations, `created_tx` and the title kept;
                    // `flags` = deleted only, `parent` 0, the counters 0, no field block or body; `CREATOR` kept,
                    // `topo`, `defer_until` and `due` 0.
                    v.hdr = NodeHdr {
                        flags: FLAG_DELETED,
                        parent: 0,
                        open_blockers: 0,
                        open_blockers_exo: 0,
                        children_total: 0,
                        children_done: 0,
                        fields_off: NONE32,
                        body_ref: 0,
                        ..v.hdr
                    };
                    v.cold = [0, 0, 0];
                }
                Op::SetStatus { new, .. } => {
                    let v = self.get(o);
                    (v.hdr.status, v.hdr.resolution) = *new;
                }
                Op::AddEdge(e) => {
                    self.get(e.src).out.insert((e.ekind, e.dst));
                    self.get(e.dst).inn.insert((e.ekind, e.src));
                }
                Op::RemoveEdge(e) => {
                    self.get(e.src).out.remove(&(e.ekind, e.dst));
                    self.get(e.dst).inn.remove(&(e.ekind, e.src));
                }
                Op::Move { new_parent, .. } => self.set_parent(o, *new_parent),
                Op::SetField { .. } | Op::SetBody { .. } => {}
            }
            // F08 §3.1: `rev_seq`, `updated_tx` and `last_op_lsn` name the owner's last op (the probe stamps them on
            // every op of the owner, `SetField` and edges included).
            let v = self.get(o);
            v.hdr.rev_seq = seq;
            v.hdr.updated_tx = seq as u32;
            v.hdr.last_op_lsn = lsn;
        }
    }

    /// The delta's rows follow the tail, against a naive replay with sets: `IDS` is exactly the visited rows (the op
    /// owners, the edge endpoints, the old and new parents) whose naive state differs from the base's (F09 §5.1,
    /// review m-2), every owner among them, and over the four seeds some visited row ends equal to its base and is not
    /// listed; each row's whole `NodeHdr`, `CREATOR` and cold columns, its out-list and
    /// its in-list are the naive ones (review m-3: a created row's creator, `created_tx`, resolution and defaults,
    /// every owner's `rev_seq` and `updated_tx`, F08 §3.5's tombstone with its kept creator); a tombstone has F08
    /// §3.5's fields; each owner's `last_op_lsn` is the overlay's node index entry; and I-P3 holds across the base and
    /// the delta.
    #[test]
    fn delta_rows_follow_the_tail() {
        let (mut created, mut tombs, mut unchanged) = (0, 0, Vec::new());
        for seed in 1..=4 {
            let t = tail(120, 400, seed);
            let s = spec(t.base.n);
            let b = base_for(&t);
            let c = Columns::parse(&b).unwrap();
            let d = fold_of(&t, &c);
            let mut w = World {
                s,
                naive: BTreeMap::new(),
                touched: BTreeSet::new(),
            };
            let mut owners = BTreeSet::new();
            for r in &t.records {
                let Payload::Commit(cm) = &r.payload else {
                    continue;
                };
                for op in &cm.ops {
                    w.apply(op, cm.seq, r.lsn);
                    owners.insert(op.owner());
                }
            }
            let changed: BTreeSet<u32> = w
                .touched
                .iter()
                .copied()
                .filter(|&id| w.naive[&id] != w.start(id))
                .collect();
            unchanged.push(w.touched.len() - changed.len());
            let naive = w.naive;
            let ids: BTreeSet<u32> = d.rows.iter().map(|r| r.id).collect();
            assert_eq!(ids, changed, "seed {seed}: IDS");
            assert!(owners.is_subset(&ids), "seed {seed}: every owner is listed");
            let o = t.overlay().unwrap();
            let lsn_of: BTreeMap<u32, u64> = o.nodes().iter().map(|e| (e.n, e.lsn)).collect();
            for r in &d.rows {
                let v = &naive[&r.id];
                let h = r.node;
                assert_eq!(h, v.hdr, "#{} NodeHdr", r.id);
                assert_eq!(r.creator, v.creator, "#{} CREATOR", r.id);
                assert_eq!(r.cold, v.cold, "#{} topo, defer_until, due", r.id);
                assert_eq!(r.prio, v.prio, "#{} PRIO", r.id);
                assert_eq!(
                    r.out,
                    v.out.iter().copied().collect::<Vec<_>>(),
                    "#{} out",
                    r.id
                );
                assert_eq!(
                    r.inn,
                    v.inn.iter().copied().collect::<Vec<_>>(),
                    "#{} in",
                    r.id
                );
                if let Some(&l) = lsn_of.get(&r.id) {
                    assert_eq!((h.last_op_lsn, v.hdr.last_op_lsn), (l, l), "#{}", r.id);
                }
                if h.deleted() {
                    assert_eq!(
                        (h.flags, h.fields_off, h.body_ref, h.children_total),
                        (1, NONE32, 0, 0)
                    );
                    assert_eq!(r.cold, [0, 0, 0]);
                    tombs += 1;
                }
                if r.id > s.n {
                    created += 1;
                }
                if let Ok(Probe::Tail(Value::Status(st, _))) =
                    o.get(&mut MemLog::new(t.start, &t.bytes), r.id, &Key::Status)
                {
                    assert_eq!(h.status, st, "#{} status as the overlay probes it", r.id);
                }
            }
            // I-P3 over the view: base rows below, delta rows above.
            let view_in = |id: u32| -> Vec<(u8, u32)> {
                match d.rows.binary_search_by_key(&id, |r| r.id) {
                    Ok(i) => d.rows[i].inn.clone(),
                    Err(_) => c.in_list(id as usize - 1),
                }
            };
            for r in &d.rows {
                for &(k, dst) in &r.out {
                    assert!(view_in(dst).contains(&(k, r.id)), "#{} -{k}-> #{dst}", r.id);
                }
            }
        }
        assert!(
            created > 0 && tombs > 0,
            "created {created}, tombstones {tombs}"
        );
        assert!(
            unchanged.iter().sum::<usize>() > 0,
            "visited rows equal to their base, per seed: {unchanged:?}"
        );
    }

    /// `PRIO` through a fold over a base with its own priorities (RF-3; [F09 §5.4], R-66): over [`base::build_with`] at
    /// [`base::ready_priority`] and `tail(200, 400, seed)` for seeds 1-4, the encoded delta's `PRIO` (0x0009) holds one
    /// byte per listed row; a created row (#N > N) holds 2, and every other listed row its base priority, deleted or
    /// not. Over the seeds some live touched base row's priority is not 2, some deleted base row's is neither 0 nor 2,
    /// and some row is created, so a fold that took a touched row's priority as 2, or zeroed it on a delete, is caught.
    #[test]
    fn prio_follows_the_fold_over_a_base_with_its_own_priorities() {
        let (mut kept, mut deleted, mut created) = (0, 0, 0);
        for seed in 1..=4 {
            let t = tail(200, 400, seed);
            let b = base::build_with(
                &base::Spec {
                    n: t.base.n,
                    upto_lsn: t.start,
                    base_seq: 999,
                },
                base::ready_priority,
            )
            .unwrap();
            let c = Columns::parse(&b).unwrap();
            let bytes = encode(&fold_of(&t, &c)).unwrap();
            let rows = decode(&bytes).unwrap().rows;
            let v = SegView::parse(&bytes).unwrap();
            let prio = v.need(0x0009).unwrap();
            assert_eq!(prio.len(), rows.len(), "seed {seed}: one byte per row");
            for (r, &p) in rows.iter().zip(prio) {
                if r.id > t.base.n {
                    assert_eq!(p, 2, "seed {seed}: created #{}", r.id);
                    created += 1;
                    continue;
                }
                let want = base::ready_priority(r.id);
                assert_eq!(
                    p,
                    want,
                    "seed {seed}: #{} (deleted {})",
                    r.id,
                    r.node.deleted()
                );
                if r.node.deleted() && want != 0 && want != 2 {
                    deleted += 1;
                }
                if !r.node.deleted() && want != 2 {
                    kept += 1;
                }
            }
        }
        assert!(
            kept > 0 && deleted > 0 && created > 0,
            "kept {kept}, deleted {deleted}, created {created}"
        );
    }

    /// The night's delta of `tail(200, 400, 1)` over its base, before `PRIO` (recorded on 4036cbf; annex E).
    const NIGHT_DELTA_PRE: [(u16, u32, u64, u64); 33] = [
        (0x0001, 212, 848, 0xe4e3_c1f9_a022_23fe),
        (0x0002, 212, 12_720, 0x3b28_95a2_6a11_da57),
        (0x0003, 212, 1_272, 0x4e8d_0746_f76d_05d1),
        (0x0004, 212, 848, 0xf788_d291_b693_7a42),
        (0x0005, 212, 848, 0xce2a_99d4_65f4_9083),
        (0x0006, 212, 848, 0x28bd_ac54_d9ea_8885),
        (0x0020, 213, 852, 0xb67d_c3f1_451b_9d6a),
        (0x0021, 591, 2_364, 0x55a1_bfd3_c0fe_d6b6),
        (0x0022, 591, 591, 0xcefd_fb5c_de46_f53f),
        (0x0023, 213, 852, 0x980c_f165_72f1_4ca9),
        (0x0024, 607, 2_428, 0x66e4_a43d_0b14_b2b0),
        (0x0025, 607, 607, 0x86f4_589b_2e24_02b2),
        (0x0040, 20, 88, 0xc1a6_584a_03f8_8669),
        (0x8000, 26, 112, 0x1641_a3cf_9d8f_1e37),
        (0x8001, 8, 40, 0xa70e_b94f_0f65_1997),
        (0x8002, 5, 28, 0xf226_362f_9607_5956),
        (0x8003, 9, 44, 0x89f6_e600_48b0_e290),
        (0x8004, 36, 152, 0xeb96_a007_2786_9462),
        (0x8005, 33, 140, 0x63dd_b087_f696_b9a8),
        (0x8006, 42, 176, 0xb816_84c0_f5ab_7290),
        (0x8007, 41, 172, 0xd743_b901_e0ef_4e1c),
        (0x8008, 4, 24, 0x966b_f213_c287_27e5),
        (0x8009, 1, 12, 0x84e2_e03f_939f_b210),
        (0x800a, 2, 16, 0x4c08_2093_40c7_96ee),
        (0x800b, 1, 12, 0x4f3b_464a_757f_5b18),
        (0x800c, 2, 16, 0x1007_d814_1426_0614),
        (0x800d, 1, 12, 0x2073_58c6_1e0e_7736),
        (0x800e, 2, 16, 0xffa9_d9d0_eb0f_d9f2),
        (0x800f, 2, 16, 0xb648_d6d8_4c5d_e49b),
        (0x8010, 1, 12, 0x7abf_950d_bc65_d557),
        (0x8011, 4, 24, 0x7e4a_300d_74dc_9cd0),
        (0x8012, 2, 16, 0x6004_be62_bc09_f562),
        (0x8013, 23, 100, 0x307f_d018_9fc1_890e),
    ];

    /// Ruling 8's derivation of the night-delta pin: the encoded fold of `tail(200, 400, 1)` over its base follows from
    /// its pre-`PRIO` bytes ([`base::tests::follows_from_pre_prio`]): a created row's `PRIO` is 2 and every other
    /// listed row's its base row's, the old byte 3 each.
    #[test]
    fn night_delta_bytes_follow_from_the_pre_prio_bytes() {
        let t = tail(200, 400, 1);
        let b = base_for(&t);
        let c = Columns::parse(&b).unwrap();
        let bytes = encode(&fold_of(&t, &c)).unwrap();
        base::tests::follows_from_pre_prio(
            "the night's delta",
            &bytes,
            &NIGHT_DELTA_PRE,
            0x8541_8bea_82ad_4917,
            0xef15_937d_159a_8b03,
            212,
        );
    }

    /// The night's delta keeps its bytes (u21 cut §6, the identity gate's quick form): the XXH3-64 of [`encode`] of the
    /// fold of the test tail `tail(200, 400, 1)` over its base. Pinned at 8916d63, before the full delta wrapped
    /// [`Fold`]; re-pinned when `PRIO` moved `priority` out of byte 3 (R-66), with the new bytes derived from the old ones
    /// by `night_delta_bytes_follow_from_the_pre_prio_bytes` (R-86 ruling 8).
    #[test]
    fn night_delta_bytes_are_unchanged() {
        let t = tail(200, 400, 1);
        let b = base_for(&t);
        let c = Columns::parse(&b).unwrap();
        let bytes = encode(&fold_of(&t, &c)).unwrap();
        assert_eq!(xxhash_rust::xxh3::xxh3_64(&bytes), 0x4139_da21_1b45_710e);
    }
}
