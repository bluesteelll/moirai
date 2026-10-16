//! The full delta of a delta checkpoint (u21 cut §3.3; u20 cut §3.9; spec Goal 1): the fold of a tail over the full base
//! ([`super::base::build_full`]) into every section [F09 §3.1] requires of a delta of its window
//! (`09-segments.md:156-206`), and the writer and reader of its bytes.
//!
//! - **[`FullBase`]** reads the full base's extra sections: each row's title and field block through its `NodeHdr`, its
//!   uid from `ALLOC`, its `DOCLEN`, the base's `FTSSTAT`, and the `REFS`, `TREES` and `FILES` snapshots.
//! - **[`FullFold`]** wraps [`delta::Fold`], the night's fold, unchanged: for each record it calls the inner `apply`, then
//!   keeps what the subset drops: each touched row's title (field 1, the probe's convention), its other fields and body
//!   hash, the bodies by hash, the deletes, the creates' uids, the `SymDefs`, each commit's ref move, op count and
//!   op-list bytes (the bytes of its frames, from [`decode_record_spans`], u21 cut D-12) and idempotency pair, the
//!   `TreeReg` epochs and the newest `FileObs` image per (`n`, `tree`). A touched base row's prior state is always the
//!   full base's, never a tail's `old` value or `Delete` image (u21 cut F-19).
//! - **[`FullFold::finish`]** gives each listed row its complete state: the base's title and fields overlaid with the
//!   window's changes; a tombstone keeps its title and drops its fields and body ([F08 §3.5],
//!   `08-data-model.md:155-169`). It sets each row's `title_off`, `fields_off` and `body_ref` into this segment's pools
//!   and `BLOBTAB`, indexes the live rows' title and abstract (field 2) with tokenizer v1 on ASCII (bodies are random
//!   bytes, so untokenized: u20 cut F-5), and sets `FTSSTAT` to the base's, less each touched base row's old
//!   contribution (from the base's `DOCLEN`), plus each listed row's new one (u21 cut D-7).
//! - **[`encode`]** seals the bodies `BLOBTAB` names into `blobs.<n>` ([`super::blobs`]; u21 cut D-6), adds that file's
//!   row to `FILES`, and writes the segment; **[`decode`]** reads both back and checks them.
//!
//! The sections, in tag order (`09-segments.md:156-206`; the F11 forms and row sizes of [`super::rt`]):
//!
//! | Tags | Written as |
//! |---|---|
//! | `IDS`, `NODE`, `CREATOR`, `TOPO`, `DEFER`, `DUE` | as [`delta::encode`], with `title_off`, `fields_off` and `body_ref` into this segment's pools (`:389-393`) |
//! | `UID` | `(uid, #N)` per listed row that is not absent, by (uid, #N) (§5.5) |
//! | `TITLE_BLOB`, `FIELDS_BLOB` | pools in row order: the title as `vstr`, the [F08 §6] field block as `vbytes` (§6.1–§6.2) |
//! | `BLOBTAB` | one `BlobRef` per distinct body hash the rows name, by hash, into `blobs.<n>`; `body_ref` 1-based (§6.3) |
//! | the CSR | as [`delta::encode`] |
//! | `EDGE_PROPS` | a fixed table of 40-B rows with no row: the probe's edges are plain (§7.2) |
//! | `TOMB` | (id, the deleting `seq`, `reason`, `replaced_by`) per listed deleted row (§8.1) |
//! | `CONFLICTS` | an `RtHdr` body, layer, 56-B rows, none (§8.2; [F11 §10]) |
//! | `BMDIR`, `BM.<i>` | as [`delta::encode`]: kind, kind-status and `deleted` (u20 cut F-4: the other predicates are not kept) |
//! | `TERMS`, `POST`, `DOCLEN`, `FTSSTAT` | [`super::text`]; `tok_ver` 1 (§12; `HOLE(F09-doclen)`'s "kept" candidate, u20 cut F-6) |
//! | `PATHIDX`, `ALIASIDX`, `ANCHORS` | variable tables with no row, 12 B each (§13.1–§13.3) |
//! | `ANCHOR_UID` | a fixed table of 28-B rows with no row (§13.4) |
//! | `GLOBIDX` | an `RtHdr` body, layer, 20-B rows, none (§13.5; [F11 §11]) |
//! | `SYMTAB` | the window's new symbols by class (§14.2) |
//! | `SCHEMAIDS` | an `RtHdr` body, layer, none (§14.3) |
//! | `FILES` | a snapshot: the base's rows and the window's `blobs.<n>` (§14.4) |
//! | `REFS` … `BACKUPS` | the forms of [F11 §1.3]: `REFS` the base's snapshot with each commit's move; `IDEM` the window's keys; `ALLOC` and `UIDX` the creates; the others no row |
//! | `TREES`, `FILEOBS`, `PENDING`, `FSINTENT`, `JOURNALCUR`, `PREFIXEV` | `TREES` the base's snapshot with the window's epochs; `FILEOBS` the newest image per key; the others no row. `FPRINT`, `DIRMAP`, `ANCESTRY`, `GITRENAMES` and `ANCHORRES` are `derived-optional` and absent (§3.3) |
//!
//! Absent, as §3.1 allows: `TOUCH` (branch segments only), `SCHEMA` (no `Schema` op), `STATS` (base only), `FPROMO`,
//! `FCOL` and `FIDX` (no promoted field), `PREV`, `VIOLATIONS` and `CKIMG` (`cs` only), `DROPPED` and `HARVEST` (no row).
//! `DOCLEN` and `FTSSTAT` carry `SecEnt.flags` bit 0, `derived-optional` ([F09 §3.3]); every other entry's flags are
//! 0. The header has `seg_kind` 4 and `tok_ver` 1 and is otherwise [`delta::Fold::finish`]'s.

use super::base::{
    ABSTRACT_FIELD, Columns, Csr, TITLE_FIELD, field_block, put_bm, put_bmdir, put_csr, put_rows,
};
use super::bitset::encode_plus_minus;
use super::blobs::{self, BLOB_REF_LEN, BlobRef, Packer};
use super::delta::{self, Delta, Fold};
use super::rt::{self, Body, FileRow, IdemEntry, MOVES_MAX, RefRow, TreeRow, form};
use super::text::{self, Doc, FtsStat, TextIndex};
use super::{
    Creator, NONE32, NodeHdr, SegView, Writer, fixed_rows, put_fixed, put_u32s, put_variable,
    seg_kind, tag, variable_rows,
};
use crate::overlay::{AllocRow, UidxRow};
use crate::tail::commit::{Op, Spans, TValue, ty};
use crate::tail::enc::{Reader, put_vbytes, uvar_len};
use crate::tail::rec::{Payload, Record};
use crate::tail::scan::{Expect, LogSource, ScanError, decode_record_spans, scan};
use std::collections::{BTreeMap, HashMap};

/// `TOMB`'s row width (`09-segments.md:536-544`).
pub const TOMB_ROW: u32 = 16;
/// `EDGE_PROPS`' row width (`09-segments.md:512-518`).
pub const EDGE_PROPS_ROW: u32 = 40;
/// `ANCHOR_UID`'s row width (`09-segments.md:892-898`).
pub const ANCHOR_UID_ROW: u32 = 28;
/// `UID`'s row width (`09-segments.md:422-426`).
pub const UID_ROW: u32 = 20;
/// `SYMTAB`'s class entry (`09-segments.md:947-955`).
pub const SYMTAB_CLASS_LEN: usize = 20;

fn e(d: crate::tail::enc::DecodeError) -> String {
    d.to_string()
}

/// The entries of an [F08 §6.1] field block, (name, typed value bytes) by name, for the value types of the probe's
/// tails: `int` and `text` ([F08 §5.1]).
pub fn parse_block(b: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let mut r = Reader::new(b);
    let n = r.uvar32().map_err(e)?;
    let mut out: Vec<(u32, Vec<u8>)> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let name = r.uvar32().map_err(e)?;
        if name == 0 || out.last().is_some_and(|l| l.0 >= name) {
            return Err(format!("field {name}: names not ascending"));
        }
        let at = r.pos();
        match r.u8().map_err(e)? {
            ty::INT => {
                r.svar64().map_err(e)?;
            }
            ty::TEXT => {
                r.vstr().map_err(e)?;
            }
            t => {
                return Err(format!(
                    "field {name}: value type {t:#04x} outside the probe's tails"
                ));
            }
        }
        out.push((name, b[at..r.pos()].to_vec()));
    }
    r.end().map_err(e)?;
    Ok(out)
}

/// The text of a typed value, if it is a `text`.
pub fn text_of(v: &[u8]) -> Option<&[u8]> {
    if v.first() != Some(&ty::TEXT) {
        return None;
    }
    Reader::new(&v[1..]).vbytes().ok()
}

/// The full base's extra sections, read in place (u21 cut §3.2).
pub struct FullBase<'c, 'a> {
    /// The base's row sections.
    pub cols: &'c Columns<'a>,
    titles: &'a [u8],
    fields: &'a [u8],
    alloc: Body<'a>,
    doclen: &'a [u8],
    /// `FTSSTAT`.
    pub ftsstat: [FtsStat; 3],
    /// `REFS`.
    pub refs: Vec<RefRow>,
    /// `REFS.aux`, `next_ref_id`.
    pub next_ref_id: u32,
    /// `TREES`.
    pub trees: Vec<TreeRow>,
    /// `FILES`.
    pub files: Vec<FileRow>,
}

impl<'c, 'a> FullBase<'c, 'a> {
    /// Reads the full base's extra sections; refuses a segment that is not a full base (`tok_ver` 1, `ALLOC` from #1
    /// over every row, `DOCLEN` of every row).
    pub fn parse(cols: &'c Columns<'a>) -> Result<FullBase<'c, 'a>, String> {
        let v = &cols.view;
        if v.header.seg_kind != seg_kind::BASE || v.header.tok_ver != text::TOK_VER {
            return Err("not a full base: seg-base with tok_ver 1".into());
        }
        let alloc = Body::parse(v.need(tag::ALLOC)?, 24)?;
        if alloc.hdr.aux != 1 || alloc.hdr.n_rows as usize != cols.rows {
            return Err("the full base's ALLOC does not cover its rows from #1".into());
        }
        let doclen = v.need(tag::DOCLEN)?;
        if doclen.len() != 6 * cols.rows {
            return Err("the full base's DOCLEN does not cover its rows".into());
        }
        let (refs, next_ref_id) = rt::decode_refs(v.need(tag::REFS)?)?;
        Ok(FullBase {
            cols,
            titles: v.need(tag::TITLE_BLOB)?,
            fields: v.need(tag::FIELDS_BLOB)?,
            alloc,
            doclen,
            ftsstat: text::decode_ftsstat(v.need(tag::FTSSTAT)?)?,
            refs,
            next_ref_id,
            trees: rt::decode_trees(v.need(tag::TREES)?)?,
            files: rt::decode_files(v.need(tag::FILES)?)?,
        })
    }

    /// Whether `#id` is a row of the base.
    pub fn has(&self, id: u32) -> bool {
        id >= 1 && id as usize <= self.cols.rows
    }

    /// Row #`id`'s title, from `TITLE_BLOB` at its `title_off`; `None` without one or without a base row.
    pub fn title(&self, id: u32) -> Result<Option<&'a [u8]>, String> {
        self.pool(id, self.titles, |h| h.title_off)
    }

    /// Row #`id`'s field block, from `FIELDS_BLOB` at its `fields_off`.
    pub fn fields(&self, id: u32) -> Result<Option<&'a [u8]>, String> {
        self.pool(id, self.fields, |h| h.fields_off)
    }

    fn pool(
        &self,
        id: u32,
        pool: &'a [u8],
        at: fn(&NodeHdr) -> u32,
    ) -> Result<Option<&'a [u8]>, String> {
        if !self.has(id) {
            return Ok(None);
        }
        let off = at(&self.cols.node(id as usize - 1));
        if off == NONE32 {
            return Ok(None);
        }
        let rest = pool
            .get(off as usize..)
            .ok_or_else(|| format!("#{id}: a pool offset {off} past its pool"))?;
        Reader::new(rest).vbytes().map(Some).map_err(e)
    }

    /// Row #`id`'s uid, from `ALLOC`.
    pub fn uid(&self, id: u32) -> Option<[u8; 16]> {
        self.has(id).then(|| {
            self.alloc.row(id as usize - 1)[0..16]
                .try_into()
                .expect("16 bytes")
        })
    }

    /// Row #`id`'s `DOCLEN` entry; zero without a base row.
    pub fn doclen(&self, id: u32) -> [u16; 3] {
        if !self.has(id) {
            return [0; 3];
        }
        let b = &self.doclen[6 * (id as usize - 1)..6 * id as usize];
        [0, 2, 4].map(|k| u16::from_le_bytes([b[k], b[k + 1]]))
    }
}

/// A `TOMB` row's fields beside its `id` (`09-segments.md:538-544`).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Tomb {
    /// `tx`: the deleting commit's `seq`.
    pub tx: u32,
    /// `reason_sym`.
    pub reason: u32,
    /// `replaced_by`.
    pub replaced_by: u32,
}

/// What a listed row carries beyond the graph sections.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RowText {
    /// Its title (the text of field 1), in `TITLE_BLOB`.
    pub title: Option<Vec<u8>>,
    /// Its field block, in `FIELDS_BLOB`; `None` with no stored field.
    pub fields: Option<Vec<u8>>,
    /// Its body hash, through `body_ref` and `BLOBTAB`.
    pub body: Option<[u8; 16]>,
    /// Its uid, in `UID`; `None` for an absent row.
    pub uid: Option<[u8; 16]>,
    /// Its `TOMB` row, when deleted.
    pub tomb: Option<Tomb>,
    /// Its `DOCLEN` entry.
    pub doclen: [u16; 3],
}

/// The full delta: everything its segment holds, and the bodies its `blobs` file holds.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FullDelta {
    /// The graph sections: the header (`tok_ver` 1), the rows with this segment's pool offsets, the ± lists.
    pub graph: Delta,
    /// Each row's other row-scoped content, parallel to `graph.rows`.
    pub rows: Vec<RowText>,
    /// The bodies `BLOBTAB` names, by hash.
    pub bodies: Vec<([u8; 16], Vec<u8>)>,
    /// `TERMS` and `POST`.
    pub text: TextIndex,
    /// `FTSSTAT`.
    pub ftsstat: [FtsStat; 3],
    /// `SYMTAB`: (class, id, string) by (class, id).
    pub symbols: Vec<(u8, u32, String)>,
    /// `REFS`.
    pub refs: Vec<RefRow>,
    /// `REFS.aux`.
    pub next_ref_id: u32,
    /// `IDEM`, by key.
    pub idem: Vec<IdemEntry>,
    /// `ALLOC`: its first `#N` (0 with no row) and rows.
    pub alloc: (u32, Vec<AllocRow>),
    /// `UIDX`, by uid.
    pub uidx: Vec<UidxRow>,
    /// `TREES`.
    pub trees: Vec<TreeRow>,
    /// `FILEOBS` as row images, by (`n`, `tree`).
    pub fileobs: Vec<Vec<u8>>,
    /// `FILES` without the delta's own `blobs` file, whose row [`encode`] adds.
    pub files: Vec<FileRow>,
}

/// One commit of the window, for the `Checkpoint` record's per-ref lists ([F05 §9.9], `05-log.md:771-777`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CommitMark {
    /// The ref it lands on.
    pub ref_id: u32,
    /// Its record's lsn.
    pub lsn: u64,
    /// Its `hlc`.
    pub hlc: u64,
}

/// A fold's result: the delta and the window's commits in log order.
#[derive(Clone, Debug)]
pub struct Folded {
    /// The delta.
    pub delta: FullDelta,
    /// The commits.
    pub marks: Vec<CommitMark>,
}

/// A touched row's title, fields and body as the window leaves them.
#[derive(Clone, Debug, Default)]
struct RowState {
    title: Option<Vec<u8>>,
    fields: BTreeMap<u32, Vec<u8>>,
    body: Option<[u8; 16]>,
}

/// Sets field `name` of `s` to `v` (absent removes it); field 1 is the title, a `text`.
fn set_field(s: &mut RowState, id: u32, name: u32, v: &TValue<'_>) -> Result<(), String> {
    if name == TITLE_FIELD {
        s.title = if v.is_absent() {
            None
        } else {
            Some(
                text_of(&v.0)
                    .ok_or_else(|| {
                        format!("#{id}: field 1, the title, holds a value that is not text")
                    })?
                    .to_vec(),
            )
        };
    } else if v.is_absent() {
        s.fields.remove(&name);
    } else {
        s.fields.insert(name, v.0.to_vec());
    }
    Ok(())
}

/// The fold of a tail over the full base into the full delta.
pub struct FullFold<'f, 'c, 'a> {
    inner: Fold<'c, 'a>,
    base: &'f FullBase<'c, 'a>,
    state: HashMap<u32, RowState>,
    bodies: HashMap<[u8; 16], Vec<u8>>,
    uids: HashMap<u32, [u8; 16]>,
    tombs: HashMap<u32, Tomb>,
    symbols: Vec<(u8, u32, String)>,
    refs: Vec<RefRow>,
    idem: BTreeMap<[u8; 16], IdemEntry>,
    alloc: BTreeMap<u32, AllocRow>,
    uidx: Vec<UidxRow>,
    trees: Vec<TreeRow>,
    fileobs: BTreeMap<(u32, [u8; 16]), Vec<u8>>,
    marks: Vec<CommitMark>,
}

impl<'f, 'c, 'a> FullFold<'f, 'c, 'a> {
    /// A fold over `base`.
    pub fn new(base: &'f FullBase<'c, 'a>) -> FullFold<'f, 'c, 'a> {
        FullFold {
            inner: Fold::new(base.cols),
            base,
            state: HashMap::new(),
            bodies: HashMap::new(),
            uids: HashMap::new(),
            tombs: HashMap::new(),
            symbols: Vec::new(),
            refs: base.refs.clone(),
            idem: BTreeMap::new(),
            alloc: BTreeMap::new(),
            uidx: Vec::new(),
            trees: base.trees.clone(),
            fileobs: BTreeMap::new(),
            marks: Vec::new(),
        }
    }

    /// The window's state of `#id`, loaded from the full base when first touched.
    fn load(&mut self, id: u32) -> Result<&mut RowState, String> {
        if !self.state.contains_key(&id) {
            let s = RowState {
                title: self.base.title(id)?.map(<[u8]>::to_vec),
                fields: match self.base.fields(id)? {
                    Some(b) => parse_block(b)?.into_iter().collect(),
                    None => BTreeMap::new(),
                },
                body: None,
            };
            self.state.insert(id, s);
        }
        Ok(self.state.get_mut(&id).expect("inserted"))
    }

    /// Applies one record, with `spans`, its commit's frames (u21 cut D-12).
    pub fn apply(&mut self, rec: &Record<'_>, spans: &Spans) -> Result<(), String> {
        for d in &rec.symdefs {
            self.symbols.push((d.class, d.id, d.text.to_string()));
        }
        match &rec.payload {
            Payload::Commit(c) => {
                self.inner.apply(rec);
                for b in &c.bodies {
                    self.bodies.entry(b.hash).or_insert_with(|| b.data.to_vec());
                }
                for op in &c.ops {
                    match op {
                        Op::Create { id, uid, image, .. } => {
                            let mut s = RowState {
                                body: image.body,
                                ..RowState::default()
                            };
                            for (name, v) in &image.fields {
                                set_field(&mut s, *id, *name, v)?;
                            }
                            self.state.insert(*id, s);
                            self.uids.insert(*id, *uid);
                            self.alloc.insert(
                                *id,
                                AllocRow {
                                    uid: *uid,
                                    ref_id: c.ref_id,
                                    create_seq: c.seq as u32,
                                },
                            );
                            self.uidx.push(UidxRow { uid: *uid, n: *id });
                        }
                        Op::Delete {
                            id,
                            reason,
                            replaced_by,
                            ..
                        } => {
                            let s = self.load(*id)?;
                            s.fields.clear();
                            s.body = None;
                            self.tombs.insert(
                                *id,
                                Tomb {
                                    tx: c.seq as u32,
                                    reason: *reason,
                                    replaced_by: *replaced_by,
                                },
                            );
                        }
                        Op::SetField { id, name, new, .. } => {
                            let s = self.load(*id)?;
                            set_field(s, *id, *name, new)?;
                        }
                        Op::SetBody { id, new, .. } => self.load(*id)?.body = *new,
                        _ => {}
                    }
                }
                // [F11 §3.6] and §3.9: the commit lands on its ref.
                let r = self
                    .refs
                    .iter_mut()
                    .find(|r| r.ref_id == c.ref_id)
                    .ok_or_else(|| {
                        format!("a commit on ref {} that REFS does not hold", c.ref_id)
                    })?;
                let ops = c.ops.len() as u64;
                let bytes: u64 = spans.iter().map(|s| u64::from(s.2)).sum();
                let sat = |x: u32, d: u64| (u64::from(x) + d).min(u64::from(u32::MAX)) as u32;
                r.tip = c.commit_id[..16].try_into().expect("16 bytes");
                r.tip_lsn = rec.lsn;
                r.gen_no = c.gen_no;
                r.ref_seq_next = c.ref_seq.saturating_add(1);
                r.ops_total = r.ops_total.saturating_add(ops);
                r.bytes_total = r.bytes_total.saturating_add(bytes);
                r.ops_since_fork = sat(r.ops_since_fork, ops);
                r.overlay_ops = sat(r.overlay_ops, ops);
                r.overlay_bytes = sat(r.overlay_bytes, bytes);
                match r.absorbed.binary_search_by_key(&c.ref_id, |a| a.0) {
                    Ok(i) => r.absorbed[i].1 = c.ref_seq,
                    Err(i) => r.absorbed.insert(i, (c.ref_id, c.ref_seq)),
                }
                r.moves.insert(0, rec.lsn);
                r.moves.truncate(MOVES_MAX);
                if let Some((key, payload)) = c.idem {
                    self.idem.entry(key).or_insert(IdemEntry {
                        key,
                        payload,
                        ref_id: c.ref_id,
                        branch_sym: c.ref_sym,
                        ref_seq: c.ref_seq,
                        flags: 1,
                        append_hlc: c.hlc,
                        origin_lsn: rec.lsn,
                    });
                }
                self.marks.push(CommitMark {
                    ref_id: c.ref_id,
                    lsn: rec.lsn,
                    hlc: c.hlc,
                });
            }
            Payload::TreeReg(t) => {
                let row = self
                    .trees
                    .iter_mut()
                    .find(|r| r.key == t.tree)
                    .ok_or_else(|| {
                        format!(
                            "a TreeReg of tree {:02x} that TREES does not hold",
                            t.tree[0]
                        )
                    })?;
                let ep = t.epoch;
                match row
                    .epochs
                    .binary_search_by_key(&(ep.scope_kind, ep.scope_ref), |x| {
                        (x.scope_kind, x.scope_ref)
                    }) {
                    Ok(i) => row.epochs[i] = ep,
                    Err(i) => row.epochs.insert(i, ep),
                }
                row.last_settle_hlc = row.last_settle_hlc.max(ep.hlc);
            }
            Payload::FileObs(b) => {
                for row in &b.rows {
                    self.fileobs.insert(row.key(), row.image.to_vec());
                }
            }
            Payload::ExtentHead(_) | Payload::Noop(_) => {}
        }
        Ok(())
    }

    /// The full delta numbered `file_no` that folds up to `upto_lsn`.
    pub fn finish(self, file_no: u32, upto_lsn: u64) -> Result<Folded, String> {
        let base = self.base;
        let mut graph = self.inner.finish(file_no, upto_lsn);
        graph.header.tok_ver = text::TOK_VER;
        let mut rows = Vec::with_capacity(graph.rows.len());
        for r in &graph.rows {
            let (absent, deleted) = (r.node.kind == 0, r.node.deleted());
            let mut t = RowText::default();
            if !absent {
                let (title, fields, body) = match self.state.get(&r.id) {
                    Some(s) => {
                        let f: Vec<(u32, Vec<u8>)> =
                            s.fields.iter().map(|(n, v)| (*n, v.clone())).collect();
                        let block = (!f.is_empty()).then(|| field_block(&f));
                        (s.title.clone(), block, s.body)
                    }
                    None => (
                        base.title(r.id)?.map(<[u8]>::to_vec),
                        base.fields(r.id)?.map(<[u8]>::to_vec),
                        None,
                    ),
                };
                t.title = title;
                if !deleted {
                    (t.fields, t.body) = (fields, body);
                }
                t.uid = Some(
                    self.uids
                        .get(&r.id)
                        .copied()
                        .or_else(|| base.uid(r.id))
                        .ok_or_else(|| format!("#{}: no uid", r.id))?,
                );
            }
            if deleted {
                t.tomb =
                    Some(*self.tombs.get(&r.id).ok_or_else(|| {
                        format!("#{}: deleted with no Delete in the window", r.id)
                    })?);
            }
            rows.push(t);
        }
        // The pools' offsets and BLOBTAB's 1-based indexes (F09 §5.2, §6).
        let mut hashes: Vec<[u8; 16]> = rows.iter().filter_map(|t| t.body).collect();
        hashes.sort_unstable();
        hashes.dedup();
        let (mut toff, mut foff) = (0u64, 0u64);
        for (r, t) in graph.rows.iter_mut().zip(&rows) {
            let place = |s: &Option<Vec<u8>>, off: &mut u64| -> Result<u32, String> {
                Ok(match s {
                    Some(b) => {
                        let o = u32::try_from(*off).map_err(|_| "a pool past 4 GiB")?;
                        *off += (uvar_len(b.len() as u64) + b.len()) as u64;
                        o
                    }
                    None => NONE32,
                })
            };
            r.node.title_off = place(&t.title, &mut toff)?;
            r.node.fields_off = place(&t.fields, &mut foff)?;
            r.node.body_ref = match t.body {
                Some(h) => 1 + hashes.binary_search(&h).expect("listed") as u32,
                None => 0,
            };
        }
        let bodies = hashes
            .iter()
            .map(|h| {
                self.bodies.get(h).map(|b| (*h, b.clone())).ok_or_else(|| {
                    format!("body {:02x?}: no commit of the window carries it", &h[..4])
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        // The text sections (F09 §12): the live rows' title and abstract, bodies untokenized (u20 cut F-5).
        let abstracts: Vec<Option<Vec<u8>>> = rows
            .iter()
            .map(|t| -> Result<Option<Vec<u8>>, String> {
                let Some(b) = &t.fields else {
                    return Ok(None);
                };
                Ok(parse_block(b)?
                    .into_iter()
                    .find(|f| f.0 == ABSTRACT_FIELD)
                    .and_then(|f| text_of(&f.1).map(<[u8]>::to_vec)))
            })
            .collect::<Result<_, _>>()?;
        for ((r, t), a) in graph.rows.iter().zip(rows.iter_mut()).zip(&abstracts) {
            if r.node.kind == 0 || r.node.deleted() {
                continue;
            }
            let dl = text::doclen(&Doc {
                id: r.id,
                fields: [
                    t.title.as_deref().unwrap_or(&[]),
                    a.as_deref().unwrap_or(&[]),
                    &[],
                ],
            })?;
            t.doclen = dl;
        }
        let docs: Vec<Doc<'_>> = graph
            .rows
            .iter()
            .zip(&rows)
            .zip(&abstracts)
            .filter(|((r, _), _)| r.node.kind != 0 && !r.node.deleted())
            .map(|((r, t), a)| Doc {
                id: r.id,
                fields: [
                    t.title.as_deref().unwrap_or(&[]),
                    a.as_deref().unwrap_or(&[]),
                    &[],
                ],
            })
            .collect();
        let index = TextIndex::of(&docs)?;
        drop(docs);
        let mut ftsstat = base.ftsstat;
        for (r, t) in graph.rows.iter().zip(&rows) {
            let old = base.doclen(r.id);
            for f in 0..3 {
                let s = &mut ftsstat[f];
                if old[f] > 0 {
                    s.docs = s.docs.checked_sub(1).ok_or("FTSSTAT docs below 0")?;
                    s.total_len = s
                        .total_len
                        .checked_sub(u64::from(old[f]))
                        .ok_or("FTSSTAT total_len below 0")?;
                }
                if t.doclen[f] > 0 {
                    s.docs += 1;
                    s.total_len += u64::from(t.doclen[f]);
                }
            }
        }
        // SYMTAB: the window's symbols by class, each class's ids contiguous (F09 §14.2 "Ranges").
        let mut symbols = self.symbols;
        symbols.sort_by_key(|s| (s.0, s.1));
        if symbols
            .windows(2)
            .any(|w| w[0].0 == w[1].0 && w[0].1 + 1 != w[1].1)
        {
            return Err("the window's symbols of one class are not contiguous".into());
        }
        // ALLOC: the creates' contiguous range (F11 §9.1).
        let alloc = match self.alloc.keys().next() {
            None => (0, Vec::new()),
            Some(&first) => {
                if self
                    .alloc
                    .keys()
                    .enumerate()
                    .any(|(i, &id)| id != first + i as u32)
                {
                    return Err("the window's creates leave a gap in ALLOC".into());
                }
                (first, self.alloc.into_values().collect())
            }
        };
        let mut uidx = self.uidx;
        uidx.sort_unstable_by_key(|r| r.uid);
        Ok(Folded {
            delta: FullDelta {
                graph,
                rows,
                bodies,
                text: index,
                ftsstat,
                symbols,
                refs: self.refs,
                next_ref_id: base.next_ref_id,
                idem: self.idem.into_values().collect(),
                alloc,
                uidx,
                trees: self.trees,
                fileobs: self.fileobs.into_values().collect(),
                files: base.files.clone(),
            },
            marks: self.marks,
        })
    }
}

/// Folds `[from, to)` of `source` (a group boundary with chain value `chain_in`) over the full base, reading through a
/// buffer of `chunk` bytes, into the full delta numbered `file_no`. Each record is decoded with its frames
/// ([`decode_record_spans`]) for its commit's op-list bytes.
#[allow(clippy::too_many_arguments)]
pub fn fold<S: LogSource>(
    source: &mut S,
    expect: &Expect,
    from: u64,
    to: u64,
    chain_in: u64,
    chunk: usize,
    base: &FullBase<'_, '_>,
    file_no: u32,
) -> Result<Folded, String> {
    let mut f = FullFold::new(base);
    scan(source, expect, from, to, chain_in, chunk, &mut |g| {
        for raw in g.records() {
            let (rec, spans) = decode_record_spans(&raw)?;
            f.apply(&rec, &spans).map_err(|what| ScanError {
                lsn: raw.hdr.lsn,
                what,
            })?;
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    f.finish(file_no, to)
}

/// Appends `SYMTAB` (§14.2, `09-segments.md:933-966`): the header, one 20-byte entry per class, each class's `by_id`
/// (pool offsets) and `by_str` (the ids by string) arrays, then the pool of `vstr`s in (class, id) order. `syms` are by
/// (class, id), each class's ids contiguous.
pub fn put_symtab(out: &mut Vec<u8>, syms: &[(u8, u32, String)]) -> Result<(), String> {
    let base = out.len();
    let mut classes: Vec<(u8, usize, usize)> = Vec::new();
    for (i, s) in syms.iter().enumerate() {
        match classes.last_mut() {
            Some(c) if c.0 == s.0 => c.2 += 1,
            _ => classes.push((s.0, i, 1)),
        }
    }
    let nc = u16::try_from(classes.len()).map_err(|_| "classes")?;
    let mut pool = Vec::new();
    let mut pool_at = Vec::with_capacity(syms.len());
    for s in syms {
        pool_at.push(pool.len() as u32);
        put_vbytes(&mut pool, s.2.as_bytes());
    }
    out.extend_from_slice(&nc.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(pool.len() as u32).to_le_bytes());
    let mut arr = 8 + SYMTAB_CLASS_LEN * classes.len();
    for &(class, first, n) in &classes {
        out.push(class);
        out.extend_from_slice(&[0; 3]);
        out.extend_from_slice(&syms[first].1.to_le_bytes());
        out.extend_from_slice(&(n as u32).to_le_bytes());
        out.extend_from_slice(&(arr as u32).to_le_bytes());
        out.extend_from_slice(&((arr + 4 * n) as u32).to_le_bytes());
        arr += 8 * n;
    }
    for &(_, first, n) in &classes {
        let ids = &syms[first..first + n];
        put_u32s(out, &pool_at[first..first + n]);
        let mut by_str: Vec<&(u8, u32, String)> = ids.iter().collect();
        by_str.sort_by(|a, b| a.2.as_bytes().cmp(b.2.as_bytes()));
        let v: Vec<u32> = by_str.iter().map(|s| s.1).collect();
        put_u32s(out, &v);
    }
    debug_assert_eq!(out.len() - base, arr);
    out.extend_from_slice(&pool);
    Ok(())
}

/// Reads `SYMTAB` back: (class, id, string) by (class, id); refuses arrays out of place and a `by_str` that is not the
/// ids by string.
pub fn decode_symtab(b: &[u8]) -> Result<Vec<(u8, u32, String)>, String> {
    if b.len() < 8 || b[2..4] != [0, 0] {
        return Err("SYMTAB without its header".into());
    }
    let nc = usize::from(u16::from_le_bytes([b[0], b[1]]));
    let pool_len = u32::from_le_bytes(b[4..8].try_into().expect("4 bytes")) as usize;
    let pool = b
        .len()
        .checked_sub(pool_len)
        .map(|at| &b[at..])
        .ok_or("SYMTAB's pool runs past it")?;
    let u32_at = |at: usize| -> Result<u32, String> {
        b.get(at..at + 4)
            .map(|x| u32::from_le_bytes(x.try_into().expect("4 bytes")))
            .ok_or_else(|| format!("SYMTAB cut at {at}"))
    };
    let mut out = Vec::new();
    let mut arr = 8 + SYMTAB_CLASS_LEN * nc;
    for c in 0..nc {
        let at = 8 + SYMTAB_CLASS_LEN * c;
        let class = b[at];
        let (first, n) = (u32_at(at + 4)?, u32_at(at + 8)? as usize);
        if u32_at(at + 12)? as usize != arr || u32_at(at + 16)? as usize != arr + 4 * n || n == 0 {
            return Err(format!(
                "SYMTAB class {class}'s arrays are not where they follow"
            ));
        }
        let mut strs = Vec::with_capacity(n);
        for j in 0..n {
            let off = u32_at(arr + 4 * j)? as usize;
            let s = Reader::new(pool.get(off..).ok_or("a pool offset past the pool")?)
                .vstr()
                .map_err(e)?;
            strs.push((first + j as u32, s.to_string()));
        }
        let mut sorted = strs.clone();
        sorted.sort_by(|x, y| x.1.as_bytes().cmp(y.1.as_bytes()));
        for (j, s) in sorted.iter().enumerate() {
            if u32_at(arr + 4 * n + 4 * j)? != s.0 {
                return Err(format!(
                    "SYMTAB class {class}'s by_str is not its ids by string"
                ));
            }
        }
        out.extend(strs.into_iter().map(|(id, s)| (class, id, s)));
        arr += 8 * n;
    }
    if arr + pool_len != b.len() {
        return Err("SYMTAB's arrays and pool do not fill it".into());
    }
    Ok(out)
}

/// The full checkpoint's two files: the segment and, when the window carries a body, its `blobs` file with its `FILES`
/// row.
#[derive(Clone, Debug)]
pub struct Encoded {
    /// `seg.d<K>`.
    pub seg: Vec<u8>,
    /// `blobs.<n>` and its row.
    pub blobs: Option<(Vec<u8>, FileRow)>,
}

/// The sections of a full delta with `lists` ± lists: 49, and `BMDIR` and the lists when a set changed.
pub const fn full_delta_sections(lists: usize) -> usize {
    49 + if lists > 0 { 1 + lists } else { 0 }
}

/// The tables of [F11 §1.3] a delta carries that hold no row in the probe's window, in tag order, with their form.
const EMPTY_STORE: [(u16, u8); 11] = [
    (tag::PINS, form::SNAPSHOT),
    (tag::HEADS, form::SNAPSHOT),
    (tag::LEASES, form::SNAPSHOT),
    (tag::MARKERS, form::SNAPSHOT),
    (tag::MARKERS_OLD, form::LAYER),
    (tag::CURSORS, form::LAYER),
    (tag::SESSMARKS, form::LAYER),
    (tag::BACKUPS, form::SNAPSHOT),
    (tag::PENDING, form::LAYER),
    (tag::FSINTENT, form::SNAPSHOT),
    (tag::JOURNALCUR, form::SNAPSHOT),
];

/// Writes `d` (u21 cut §3.3): the window's bodies sealed into `blobs.<blobs_no>` first, so `BLOBTAB` and `FILES` name
/// it, then the segment.
pub fn encode(d: &FullDelta, blobs_no: u32, packer: &mut Packer) -> Result<Encoded, String> {
    let rows = &d.graph.rows;
    let n = rows.len();
    let (refs, blobs) = if d.bodies.is_empty() {
        (Vec::new(), None)
    } else {
        let b: Vec<([u8; 16], &[u8])> = d.bodies.iter().map(|(h, x)| (*h, x.as_slice())).collect();
        let (bytes, refs) = blobs::seal(&b, blobs_no, packer)?;
        let row = FileRow {
            family: seg_kind::BLOBS,
            file_no: blobs_no,
            total_len: bytes.len() as u64,
            digest16: bytes[72..88].try_into().expect("16 bytes"),
            ..FileRow::default()
        };
        (refs, Some((bytes, row)))
    };
    let mut files = d.files.clone();
    if let Some((_, row)) = &blobs {
        files.push(*row);
        files.sort_by_key(FileRow::key);
    }
    let k = d.graph.lists.len();
    let mut w = Writer::new(full_delta_sections(k), 300 * n + 4_096);
    w.push(tag::IDS, n as u32, |o| {
        let ids: Vec<u32> = rows.iter().map(|r| r.id).collect();
        put_u32s(o, &ids);
        Ok(())
    })?;
    let nodes: Vec<NodeHdr> = rows.iter().map(|r| r.node).collect();
    let creators: Vec<Creator> = rows.iter().map(|r| r.creator).collect();
    let col = |j: usize| -> Vec<u32> { rows.iter().map(|r| r.cold[j]).collect() };
    put_rows(&mut w, &nodes, &creators, [&col(0), &col(1), &col(2)])?;
    let mut uids: Vec<([u8; 16], u32)> = rows
        .iter()
        .zip(&d.rows)
        .filter_map(|(r, t)| t.uid.map(|u| (u, r.id)))
        .collect();
    uids.sort_unstable();
    w.push(tag::UID, uids.len() as u32, |o| {
        let b: Vec<[u8; 20]> = uids
            .iter()
            .map(|(u, id)| {
                let mut x = [0u8; 20];
                x[..16].copy_from_slice(u);
                x[16..].copy_from_slice(&id.to_le_bytes());
                x
            })
            .collect();
        put_fixed(o, UID_ROW, &b.iter().map(|x| &x[..]).collect::<Vec<_>>());
        Ok(())
    })?;
    for (t, pick) in [
        (
            tag::TITLE_BLOB,
            (|r: &RowText| r.title.as_deref()) as fn(&RowText) -> Option<&[u8]>,
        ),
        (tag::FIELDS_BLOB, |r: &RowText| r.fields.as_deref()),
    ] {
        let count = d.rows.iter().filter(|r| pick(r).is_some()).count() as u32;
        w.push(t, count, |o| {
            for s in d.rows.iter().filter_map(pick) {
                put_vbytes(o, s);
            }
            Ok(())
        })?;
    }
    w.push(tag::BLOBTAB, refs.len() as u32, |o| {
        let b: Vec<[u8; BLOB_REF_LEN]> = refs.iter().map(BlobRef::encode).collect();
        put_fixed(
            o,
            BLOB_REF_LEN as u32,
            &b.iter().map(|x| &x[..]).collect::<Vec<_>>(),
        );
        Ok(())
    })?;
    put_csr(
        &mut w,
        &Csr::of_lists(rows.iter().map(|r| r.out.as_slice())),
        [tag::OUT_OFF, tag::OUT_DST, tag::OUT_KIND],
    )?;
    put_csr(
        &mut w,
        &Csr::of_lists(rows.iter().map(|r| r.inn.as_slice())),
        [tag::IN_OFF, tag::IN_SRC, tag::IN_KIND],
    )?;
    w.push(tag::EDGE_PROPS, 0, |o| {
        put_fixed(o, EDGE_PROPS_ROW, &[]);
        Ok(())
    })?;
    let tombs: Vec<[u8; 16]> = rows
        .iter()
        .zip(&d.rows)
        .filter_map(|(r, t)| {
            t.tomb.map(|x| {
                let mut b = [0u8; 16];
                b[0..4].copy_from_slice(&r.id.to_le_bytes());
                b[4..8].copy_from_slice(&x.tx.to_le_bytes());
                b[8..12].copy_from_slice(&x.reason.to_le_bytes());
                b[12..16].copy_from_slice(&x.replaced_by.to_le_bytes());
                b
            })
        })
        .collect();
    w.push(tag::TOMB, tombs.len() as u32, |o| {
        put_fixed(
            o,
            TOMB_ROW,
            &tombs.iter().map(|x| &x[..]).collect::<Vec<_>>(),
        );
        Ok(())
    })?;
    w.push(tag::CONFLICTS, 0, |o| {
        rt::put_empty(o, tag::CONFLICTS, form::LAYER)
    })?;
    let dir: Vec<([u8; 4], u32)> = d
        .graph
        .lists
        .iter()
        .map(|(key, p, m)| (*key, (p.len() + m.len()) as u32))
        .collect();
    if k > 0 {
        put_bmdir(&mut w, &dir)?;
    }
    let mut post = Vec::new();
    let offs = text::encode_post(&d.text, &mut post)?;
    w.push(tag::TERMS, d.text.n_terms(), |o| {
        text::encode_terms(&d.text, &offs, o)
    })?;
    w.push(tag::POST, d.text.n_terms(), |o| {
        o.extend_from_slice(&post);
        Ok(())
    })?;
    w.push(tag::DOCLEN, n as u32, |o| {
        let dl: Vec<[u16; 3]> = d.rows.iter().map(|r| r.doclen).collect();
        text::put_doclen(o, &dl);
        Ok(())
    })?;
    w.push(tag::FTSSTAT, 3, |o| {
        text::put_ftsstat(o, &d.ftsstat);
        Ok(())
    })?;
    for t in [tag::PATHIDX, tag::ALIASIDX, tag::ANCHORS] {
        w.push(t, 0, |o| put_variable(o, &[]))?;
    }
    w.push(tag::ANCHOR_UID, 0, |o| {
        put_fixed(o, ANCHOR_UID_ROW, &[]);
        Ok(())
    })?;
    w.push(tag::GLOBIDX, 0, |o| {
        rt::put_empty(o, tag::GLOBIDX, form::LAYER)
    })?;
    w.push(tag::SYMTAB, d.symbols.len() as u32, |o| {
        put_symtab(o, &d.symbols)
    })?;
    w.push(tag::SCHEMAIDS, 0, |o| {
        rt::put_empty(o, tag::SCHEMAIDS, form::LAYER)
    })?;
    w.push(tag::FILES, files.len() as u32, |o| rt::put_files(o, &files))?;
    w.push(tag::REFS, d.refs.len() as u32, |o| {
        rt::put_refs(o, form::SNAPSHOT, &d.refs, d.next_ref_id)
    })?;
    let empty = |w: &mut Writer, t: u16, f: u8| w.push(t, 0, |o| rt::put_empty(o, t, f));
    for &(t, f) in &EMPTY_STORE[..5] {
        empty(&mut w, t, f)?;
    }
    w.push(tag::IDEM, rt::idem_capacity(d.idem.len()) as u32, |o| {
        rt::put_idem(o, &d.idem)
    })?;
    w.push(tag::ALLOC, d.alloc.1.len() as u32, |o| {
        rt::put_alloc(o, form::LAYER, d.alloc.0, &d.alloc.1)
    })?;
    w.push(tag::UIDX, d.uidx.len() as u32, |o| {
        rt::put_uidx(o, form::LAYER, &d.uidx)
    })?;
    for &(t, f) in &EMPTY_STORE[5..8] {
        empty(&mut w, t, f)?;
    }
    w.push(tag::TREES, d.trees.len() as u32, |o| {
        rt::put_trees(o, &d.trees)
    })?;
    w.push(tag::FILEOBS, d.fileobs.len() as u32, |o| {
        rt::put_fileobs(o, &d.fileobs.iter().map(Vec::as_slice).collect::<Vec<_>>())
    })?;
    for &(t, f) in &EMPTY_STORE[8..] {
        empty(&mut w, t, f)?;
    }
    w.push(tag::PREFIXEV, 0, |o| {
        rt::put_empty(o, tag::PREFIXEV, form::SNAPSHOT)
    })?;
    if k > 0 {
        put_bm(&mut w, &dir, |i, o| {
            encode_plus_minus(&d.graph.lists[i].1, &d.graph.lists[i].2, o).map(|_| ())
        })?;
    }
    let seg = w.finish(&d.graph.header)?;
    Ok(Encoded { seg, blobs })
}

/// Reads an F11 body that must hold no row, in `form`.
fn need_empty(v: &SegView<'_>, t: u16, f: u8) -> Result<(), String> {
    let rs = rt::row_size(t).ok_or("no F11 body")?;
    let b = Body::parse(v.need(t)?, rs)?;
    if b.hdr.n_rows != 0
        || b.hdr.form != f
        || b.hdr.aux != 0
        || !b.index.is_empty()
        || !b.heap.is_empty()
    {
        return Err(format!("section {t:#06x} holds a row or another form"));
    }
    Ok(())
}

/// The entries of a pool in row order, each at the row's offset: `NONE32` means none; refuses an entry not at the
/// running offset or a pool with bytes after the last entry.
fn pool_entries(pool: &[u8], offs: &[u32]) -> Result<Vec<Option<Vec<u8>>>, String> {
    let mut at = 0usize;
    let mut out = Vec::with_capacity(offs.len());
    for &o in offs {
        if o == NONE32 {
            out.push(None);
            continue;
        }
        if o as usize != at {
            return Err(format!("a pool entry at {o} where the pool reaches {at}"));
        }
        let mut r = Reader::new(&pool[at..]);
        let s = r.vbytes().map_err(e)?;
        at += r.pos();
        out.push(Some(s.to_vec()));
    }
    if at != pool.len() {
        return Err(format!(
            "a pool of {} bytes whose entries end at {at}",
            pool.len()
        ));
    }
    Ok(out)
}

/// Reads a full delta back from its segment and, when `BLOBTAB` names a body, its `blobs` file, and checks both
/// (V-9; the pools' canonical order; `BLOBTAB` against `BLOBIDX`; `FILES`' row of the `blobs` file against the file;
/// every table with no row empty in its form).
pub fn decode(seg: &[u8], blobs: Option<&[u8]>) -> Result<FullDelta, String> {
    let v = SegView::parse(seg)?;
    v.check_digests()?;
    if v.header.seg_kind != seg_kind::DELTA || v.header.tok_ver != text::TOK_VER {
        return Err("not a seg-delta of tok_ver 1".into());
    }
    let graph = delta::decode(seg)?;
    let n = graph.rows.len();
    let uid_rows = fixed_rows(v.need(tag::UID)?, UID_ROW)?;
    let mut uids: HashMap<u32, [u8; 16]> = HashMap::new();
    for (i, r) in uid_rows.iter().enumerate() {
        if i > 0 && uid_rows[i - 1] >= *r {
            return Err("UID rows are not strictly ascending".into());
        }
        let id = u32::from_le_bytes(r[16..20].try_into().expect("4 bytes"));
        uids.insert(id, r[..16].try_into().expect("16 bytes"));
    }
    let titles = pool_entries(
        v.need(tag::TITLE_BLOB)?,
        &graph
            .rows
            .iter()
            .map(|r| r.node.title_off)
            .collect::<Vec<_>>(),
    )?;
    let fields = pool_entries(
        v.need(tag::FIELDS_BLOB)?,
        &graph
            .rows
            .iter()
            .map(|r| r.node.fields_off)
            .collect::<Vec<_>>(),
    )?;
    let refs: Vec<BlobRef> = fixed_rows(v.need(tag::BLOBTAB)?, BLOB_REF_LEN as u32)?
        .into_iter()
        .map(BlobRef::decode)
        .collect();
    if refs.windows(2).any(|w| w[0].hash >= w[1].hash) {
        return Err("BLOBTAB is not strictly ascending by hash".into());
    }
    let mut files = rt::decode_files(v.need(tag::FILES)?)?;
    let bodies = match (refs.is_empty(), blobs) {
        (true, _) => Vec::new(),
        (false, None) => return Err("BLOBTAB names bodies and no blobs file is given".into()),
        (false, Some(bytes)) => {
            let f = blobs::decode(bytes)?;
            let no = f.header.file_no;
            let same = refs.len() == f.idx.len()
                && refs.iter().zip(&f.idx).all(|(r, i)| {
                    (r.hash, r.file, r.off, r.len, r.raw_len)
                        == (i.hash, no, i.off, i.len, i.raw_len)
                });
            if !same {
                return Err("BLOBTAB does not match the blobs file's BLOBIDX".into());
            }
            let pos = files
                .iter()
                .position(|r| r.key() == (seg_kind::BLOBS, 0, no))
                .ok_or("FILES does not name the blobs file")?;
            let row = files.remove(pos);
            if row.total_len != bytes.len() as u64 || row.digest16[..] != bytes[72..88] {
                return Err("FILES' row of the blobs file does not match it".into());
            }
            refs.iter().map(|r| r.hash).zip(f.raw).collect()
        }
    };
    let mut tombs: HashMap<u32, Tomb> = HashMap::new();
    let tomb_rows = fixed_rows(v.need(tag::TOMB)?, TOMB_ROW)?;
    for r in &tomb_rows {
        let x = |at: usize| u32::from_le_bytes(r[at..at + 4].try_into().expect("4 bytes"));
        tombs.insert(
            x(0),
            Tomb {
                tx: x(4),
                reason: x(8),
                replaced_by: x(12),
            },
        );
    }
    let doclen = text::decode_doclen(v.need(tag::DOCLEN)?)?;
    if doclen.len() != n || tombs.len() != tomb_rows.len() || uids.len() != uid_rows.len() {
        return Err("DOCLEN, TOMB or UID does not fit the rows".into());
    }
    let mut rows = Vec::with_capacity(n);
    for (i, r) in graph.rows.iter().enumerate() {
        let body = match r.node.body_ref {
            0 => None,
            k => Some(
                refs.get(k as usize - 1)
                    .ok_or_else(|| format!("#{}: body_ref {k} past BLOBTAB", r.id))?
                    .hash,
            ),
        };
        rows.push(RowText {
            title: titles[i].clone(),
            fields: fields[i].clone(),
            body,
            uid: uids.get(&r.id).copied(),
            tomb: tombs.get(&r.id).copied(),
            doclen: doclen[i],
        });
    }
    if fixed_rows(v.need(tag::EDGE_PROPS)?, EDGE_PROPS_ROW)?.len()
        + fixed_rows(v.need(tag::ANCHOR_UID)?, ANCHOR_UID_ROW)?.len()
        != 0
    {
        return Err("EDGE_PROPS or ANCHOR_UID holds a row".into());
    }
    for t in [tag::PATHIDX, tag::ALIASIDX, tag::ANCHORS] {
        if !variable_rows(v.need(t)?)?.is_empty() {
            return Err(format!("section {t:#06x} holds a row"));
        }
    }
    for (t, f) in [
        (tag::CONFLICTS, form::LAYER),
        (tag::GLOBIDX, form::LAYER),
        (tag::SCHEMAIDS, form::LAYER),
        (tag::PREFIXEV, form::SNAPSHOT),
    ]
    .into_iter()
    .chain(EMPTY_STORE)
    {
        need_empty(&v, t, f)?;
    }
    let (refs_rows, next_ref_id) = rt::decode_refs(v.need(tag::REFS)?)?;
    let mut idem: Vec<IdemEntry> = rt::decode_idem(v.need(tag::IDEM)?)?
        .into_iter()
        .map(|x| x.1)
        .collect();
    idem.sort_by_key(|x| x.key);
    Ok(FullDelta {
        graph,
        rows,
        bodies,
        text: text::decode_index(v.need(tag::TERMS)?, v.need(tag::POST)?)?,
        ftsstat: text::decode_ftsstat(v.need(tag::FTSSTAT)?)?,
        symbols: decode_symtab(v.need(tag::SYMTAB)?)?,
        refs: refs_rows,
        next_ref_id,
        idem,
        alloc: rt::decode_alloc(v.need(tag::ALLOC)?)?,
        uidx: rt::decode_uidx(v.need(tag::UIDX)?)?,
        trees: rt::decode_trees(v.need(tag::TREES)?)?,
        fileobs: rt::decode_fileobs(v.need(tag::FILEOBS)?)?,
        files,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::seg::Header;
    use crate::seg::base::{Spec, Vocab, full_files, full_refs, full_trees, full_uid, text_value};
    use crate::seg::text::tokenize;
    use crate::tail::TAIL_READ_CHUNK;
    use crate::tail::scan::MemLog;
    use crate::tail::synth::{Clause, Tail, generate, tests::test_target};
    use crate::tail::testdata;
    use std::collections::BTreeSet;

    /// A test-profile tail of `ops` ops over N = `n`, records kept, and its full base.
    pub(crate) struct Case {
        /// The tail.
        pub(crate) tail: Tail,
        /// The full base's bytes.
        pub(crate) base: Vec<u8>,
    }

    pub(crate) fn case(n: u32, ops: u64, seed: u64) -> Case {
        let mut t = test_target(Clause::Ops, n, seed);
        t.threshold = ops;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, true).unwrap();
        let base = base::build_full(&spec_of(&tail)).unwrap();
        Case { tail, base }
    }

    fn spec_of(t: &Tail) -> Spec {
        Spec {
            n: t.base.n,
            upto_lsn: t.start,
            base_seq: 999,
        }
    }

    use crate::seg::base;

    /// The fold of `t` from memory over `fb` into `seg.d14`.
    pub(crate) fn fold_mem(t: &Tail, fb: &FullBase<'_, '_>) -> Folded {
        fold(
            &mut MemLog::new(t.start, &t.bytes),
            &t.expect,
            t.start,
            t.end,
            t.chain_in,
            TAIL_READ_CHUNK,
            fb,
            14,
        )
        .unwrap()
    }

    fn files_of(d: &FullDelta) -> Encoded {
        encode(d, 15, &mut Packer::default()).unwrap()
    }

    fn read_back(enc: &Encoded) -> FullDelta {
        decode(&enc.seg, enc.blobs.as_ref().map(|b| b.0.as_slice())).unwrap()
    }

    /// The hand-built tail (`tail::testdata`, base N = 50) and its full base.
    fn hand() -> (testdata::HandBuilt, Vec<u8>) {
        let hb = testdata::hand_built();
        let b = base::build_full(&Spec {
            n: 50,
            upto_lsn: testdata::START,
            base_seq: 100,
        })
        .unwrap();
        (hb, b)
    }

    fn fold_hand(hb: &testdata::HandBuilt, fb: &FullBase<'_, '_>) -> Folded {
        let end = testdata::START + hb.bytes.len() as u64;
        fold(
            &mut MemLog::new(testdata::START, &hb.bytes),
            &hb.expect,
            testdata::START,
            end,
            testdata::CHAIN_IN,
            TAIL_READ_CHUNK,
            fb,
            14,
        )
        .unwrap()
    }

    /// The section table of a full delta (u21 cut §3.3; [F09 §3.1], `09-segments.md:156-206`): exactly `IDS` …
    /// `DUE`, `UID`, `TITLE_BLOB`, `FIELDS_BLOB`, `BLOBTAB`, the CSR, `EDGE_PROPS`, `TOMB`, `CONFLICTS`, `BMDIR`, `TERMS`,
    /// `POST`, `DOCLEN`, `FTSSTAT`, `PATHIDX` … `GLOBIDX`, `SYMTAB`, `SCHEMAIDS`, `FILES`, `REFS` … `BACKUPS`, `TREES`,
    /// `FILEOBS`, `PENDING`, `FSINTENT`, `JOURNALCUR`, `PREFIXEV` and the ± lists, in tag order; none of `TOUCH`,
    /// `SCHEMA`, `STATS`, `FPROMO`, `FCOL`, `FIDX`, `PREV`, `VIOLATIONS`, `CKIMG`, `DROPPED`, `HARVEST` and the five
    /// `derived-optional` runtime sections; `flags` bit 0 on `DOCLEN` and `FTSSTAT` and 0 on every other entry ([F09 §3.3]);
    /// `seg_kind` 4, `tok_ver` 1, `file_no` 14; each F11 body in its
    /// delta form and row size with `count` its `n_rows`; the empty tables' exact bytes.
    #[test]
    fn full_delta_carries_every_section_a_delta_requires() {
        let c = case(200, 400, 1);
        let cols = Columns::parse(&c.base).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let d = fold_mem(&c.tail, &fb).delta;
        let k = d.graph.lists.len() as u16;
        assert!(
            k > 0 && !d.bodies.is_empty(),
            "lists {k}, bodies {}",
            d.bodies.len()
        );
        let enc = files_of(&d);
        let v = SegView::parse(&enc.seg).unwrap();
        let tags: Vec<u16> = v.table.iter().map(|e| e.tag).collect();
        let mut want: Vec<u16> = (0x0001..=0x0007).collect();
        want.extend([0x0010, 0x0011, 0x0012]);
        want.extend(0x0020..=0x0026);
        want.extend([0x0030, 0x0031, 0x0040]);
        want.extend(0x0050..=0x0053);
        want.extend(0x0080..=0x0084);
        want.extend([0x0100, 0x0101, 0x0102]);
        want.extend(0x0201..=0x020C);
        want.extend([0x0210, 0x0211, 0x0212, 0x0213, 0x0215, 0x0217]);
        want.extend((0..k).map(|i| 0x8000 + i));
        let missing: Vec<String> = want
            .iter()
            .filter(|t| !tags.contains(t))
            .map(|t| format!("{t:#06x}"))
            .collect();
        assert!(missing.is_empty(), "missing tags {missing:?}");
        assert_eq!(tags, want);
        assert_eq!(tags.len(), full_delta_sections(k as usize));
        for t in [
            0x0008, 0x0032, 0x0060, 0x0061, 0x00C0, 0x00C1, 0x00C2, 0x020D, 0x020E, 0x0214, 0x0216,
            0x0218, 0x0219, 0x021A,
        ] {
            assert!(v.entry(t).is_none(), "{t:#06x} present");
        }
        assert!(
            tags.iter().all(|t| !(0x4000..0x6000).contains(t)),
            "no FCOL or FIDX"
        );
        for e in &v.table {
            let want = u16::from(e.tag == 0x0052 || e.tag == 0x0053);
            assert_eq!(e.flags, want, "{:#06x} flags {}", e.tag, e.flags);
        }
        assert_eq!(
            (v.header.seg_kind, v.header.tok_ver, v.header.file_no),
            (4, 1, 14)
        );
        let mut bodies = 0;
        for e in &v.table {
            if let Some(rs) = rt::row_size(e.tag) {
                let b = Body::parse(v.section(e.tag).unwrap(), rs).unwrap();
                assert_eq!(
                    (b.hdr.form, e.count),
                    (rt::delta_form(e.tag).unwrap(), b.hdr.n_rows),
                    "{:#06x}",
                    e.tag
                );
                bodies += 1;
            }
        }
        assert_eq!(bodies, 22, "the F11 bodies");
        assert_eq!(
            v.section(tag::EDGE_PROPS).unwrap(),
            &[0, 0, 0, 0, 40, 0, 0, 0]
        );
        assert_eq!(
            v.section(tag::ANCHOR_UID).unwrap(),
            &[0, 0, 0, 0, 28, 0, 0, 0]
        );
        for t in [tag::PATHIDX, tag::ALIASIDX, tag::ANCHORS] {
            assert_eq!(v.section(t).unwrap(), &[0; 12], "{t:#06x}");
        }
        assert_eq!(v.entry(tag::FTSSTAT).unwrap().count, 3);
        assert_eq!(
            v.entry(tag::DOCLEN).unwrap().count as usize,
            d.graph.rows.len()
        );
    }

    /// The reader takes `DOCLEN`'s and `FTSSTAT`'s `derived-optional` bit as [F09 §3.3] gives it, accepted and never
    /// required: the full delta of `case(200, 400, 1)` decodes equal to its fold with both entries' bit 0 set and with it
    /// cleared; with bit 0 on `NODE` it is refused (V-7).
    #[test]
    fn decode_accepts_doclen_and_ftsstat_with_or_without_bit_0() {
        use crate::seg::tests::with_flags;
        let c = case(200, 400, 1);
        let cols = Columns::parse(&c.base).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let d = fold_mem(&c.tail, &fb).delta;
        let enc = files_of(&d);
        let blobs = enc.blobs.as_ref().map(|b| b.0.as_slice());
        let at = |t: u16| {
            SegView::parse(&enc.seg)
                .unwrap()
                .table
                .iter()
                .position(|e| e.tag == t)
                .unwrap()
        };
        let (dl, fs, node) = (at(0x0052), at(0x0053), at(0x0002));
        for flags in [1u16, 0] {
            let seg = with_flags(&with_flags(&enc.seg, dl, flags), fs, flags);
            let got = decode(&seg, blobs).unwrap_or_else(|e| panic!("flags {flags}: {e}"));
            assert_eq!(got, d, "DOCLEN and FTSSTAT with flags {flags}");
        }
        let r = decode(&with_flags(&enc.seg, node, 1), blobs);
        assert!(r.is_err(), "bit 0 on NODE is accepted");
    }

    /// A full delta reads back equal to its fold, from its segment and its `blobs` file, with every digest right (V-9)
    /// and the reader's own checks: for three generated tails and the hand-built one.
    #[test]
    fn full_delta_reads_back_equal_to_its_fold() {
        for seed in 1..=3 {
            let c = case(200, 400, seed);
            let cols = Columns::parse(&c.base).unwrap();
            let fb = FullBase::parse(&cols).unwrap();
            let d = fold_mem(&c.tail, &fb).delta;
            let enc = files_of(&d);
            assert_eq!(read_back(&enc), d, "seed {seed}");
            SegView::parse(&enc.seg).unwrap().check_digests().unwrap();
            assert!(enc.blobs.is_some(), "seed {seed}: a window with bodies");
        }
        let (hb, b) = hand();
        let cols = Columns::parse(&b).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let d = fold_hand(&hb, &fb).delta;
        assert_eq!(read_back(&files_of(&d)), d, "the hand-built tail");
        assert!(
            decode(&files_of(&d).seg, None).is_err(),
            "BLOBTAB names a body and no blobs file is given"
        );
    }

    /// The graph sections equal the subset's fold (u21 cut §3.3: `FullFold` wraps `delta::Fold` unchanged): over the
    /// same full base, the segment's `IDS`, `NODE` without its three offsets, `CREATOR`, the cold columns, the CSR and the
    /// ± lists read back equal to [`delta::fold`]'s, and so does the header but `tok_ver`.
    #[test]
    fn graph_sections_equal_the_subset_fold() {
        for seed in 1..=3 {
            let c = case(200, 400, seed);
            let t = &c.tail;
            let cols = Columns::parse(&c.base).unwrap();
            let fb = FullBase::parse(&cols).unwrap();
            let full = read_back(&files_of(&fold_mem(t, &fb).delta)).graph;
            let plain = delta::fold(
                &mut MemLog::new(t.start, &t.bytes),
                &t.expect,
                t.start,
                t.end,
                t.chain_in,
                TAIL_READ_CHUNK,
                &cols,
                14,
            )
            .unwrap();
            let mask = |d: &Delta| -> Vec<delta::Row> {
                d.rows
                    .iter()
                    .map(|r| delta::Row {
                        node: NodeHdr {
                            title_off: 0,
                            fields_off: 0,
                            body_ref: 0,
                            ..r.node
                        },
                        ..r.clone()
                    })
                    .collect()
            };
            assert_eq!(mask(&full), mask(&plain), "seed {seed}: rows");
            assert_eq!(full.lists, plain.lists, "seed {seed}: ± lists");
            assert_eq!(
                Header {
                    tok_ver: 0,
                    ..full.header
                },
                plain.header
            );
        }
    }

    /// One node of the naive replay of the text, fields, bodies and uids.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Naive {
        pub(crate) live: bool,
        pub(crate) deleted: bool,
        pub(crate) title: Option<Vec<u8>>,
        pub(crate) fields: BTreeMap<u32, Vec<u8>>,
        pub(crate) body: Option<[u8; 16]>,
        pub(crate) uid: Option<[u8; 16]>,
    }

    /// The naive replay of `records` over a full base of `n` rows: each touched node's state, from the base's generator
    /// ([`Vocab`]), and the nodes an op that changes text, a field or a body touched.
    pub(crate) fn naive(n: u32, records: &[Record<'_>]) -> (BTreeMap<u32, Naive>, BTreeSet<u32>) {
        let vocab = Vocab::new();
        let start = |id: u32| -> Naive {
            if id > n {
                return Naive::default();
            }
            Naive {
                live: true,
                title: Some(vocab.title(id)),
                fields: vocab
                    .fields(id)
                    .into_iter()
                    .map(|(k, t)| (k, text_value(&t)))
                    .collect(),
                uid: Some(full_uid(id)),
                ..Naive::default()
            }
        };
        let mut m: BTreeMap<u32, Naive> = BTreeMap::new();
        let mut touched = BTreeSet::new();
        let set = |x: &mut Naive, name: u32, v: &TValue<'_>| {
            if name == 1 {
                x.title = (!v.is_absent()).then(|| text_of(&v.0).unwrap().to_vec());
            } else if v.is_absent() {
                x.fields.remove(&name);
            } else {
                x.fields.insert(name, v.0.to_vec());
            }
        };
        for r in records {
            let Payload::Commit(c) = &r.payload else {
                continue;
            };
            for op in &c.ops {
                let id = op.owner();
                let x = m.entry(id).or_insert_with(|| start(id));
                match op {
                    Op::Create { uid, image, .. } => {
                        *x = Naive {
                            live: true,
                            uid: Some(*uid),
                            body: image.body,
                            ..Naive::default()
                        };
                        for (k, v) in &image.fields {
                            set(x, *k, v);
                        }
                    }
                    Op::Delete { .. } => {
                        x.deleted = true;
                        x.fields.clear();
                        x.body = None;
                    }
                    Op::SetField { name, new, .. } => set(x, *name, new),
                    Op::SetBody { new, .. } => x.body = *new,
                    _ => continue,
                }
                touched.insert(id);
            }
        }
        (m, touched)
    }

    /// Every listed row carries its complete state, against a naive replay (u21 cut §3.3): a live row its title (field 1),
    /// its other fields as a block by name, its body hash and its uid (the create's, or the base's); a tombstone keeps its
    /// title and uid and has no field or body; an absent row has none; each row's offsets point at its own entries.
    /// Every node whose text, fields or body the window changed is listed. Over three tails there are creates,
    /// tombstones, bodies and base rows whose title comes from the base.
    #[test]
    fn touched_rows_carry_title_fields_body_and_uid() {
        let (mut creates, mut tombs, mut bodies, mut from_base) = (0, 0, 0, 0);
        for seed in 1..=3 {
            let c = case(300, 400, seed);
            let cols = Columns::parse(&c.base).unwrap();
            let fb = FullBase::parse(&cols).unwrap();
            let d = read_back(&files_of(&fold_mem(&c.tail, &fb).delta));
            let (m, touched) = naive(300, &c.tail.records);
            let listed: BTreeSet<u32> = d.graph.rows.iter().map(|r| r.id).collect();
            assert!(touched.is_subset(&listed), "seed {seed}");
            let vocab = Vocab::new();
            for (r, t) in d.graph.rows.iter().zip(&d.rows) {
                let x = m.get(&r.id).cloned().unwrap_or_else(|| Naive {
                    live: r.id <= 300,
                    title: (r.id <= 300).then(|| vocab.title(r.id)),
                    fields: if r.id <= 300 {
                        vocab
                            .fields(r.id)
                            .into_iter()
                            .map(|(k, s)| (k, text_value(&s)))
                            .collect()
                    } else {
                        BTreeMap::new()
                    },
                    uid: (r.id <= 300).then(|| full_uid(r.id)),
                    ..Naive::default()
                });
                let block = (!x.fields.is_empty() && !x.deleted).then(|| {
                    base::field_block(
                        &x.fields
                            .iter()
                            .map(|(k, v)| (*k, v.clone()))
                            .collect::<Vec<_>>(),
                    )
                });
                assert_eq!(t.title, x.title, "#{} title", r.id);
                assert_eq!(t.fields, block, "#{} fields", r.id);
                assert_eq!(t.body, x.body.filter(|_| !x.deleted), "#{} body", r.id);
                assert_eq!(t.uid, x.uid, "#{} uid", r.id);
                assert_eq!(t.tomb.is_some(), x.deleted, "#{} tomb", r.id);
                if x.deleted {
                    assert!(
                        t.title.is_some() || r.id > 300,
                        "#{}: a tombstone keeps its title",
                        r.id
                    );
                    assert_eq!((r.node.fields_off, r.node.body_ref), (NONE32, 0));
                    tombs += 1;
                }
                creates += usize::from(r.id > 300 && x.live);
                bodies += usize::from(t.body.is_some());
                from_base += usize::from(r.id <= 300 && !m.contains_key(&r.id));
            }
        }
        assert!(
            creates > 0 && tombs > 0 && bodies > 0 && from_base > 0,
            "creates {creates}, tombstones {tombs}, bodies {bodies}, base-only rows {from_base}"
        );
    }

    /// `TOMB` follows the deletes ([F09 §8.1], `09-segments.md:534-547`): on the hand-built tail, whose `Delete` of #9 in
    /// the commit of `seq` 103 has `reason` 3 and `replaced_by` 52, `TOMB` is the one row (9, 103, 3, 52), so a swap of
    /// `reason` and `replaced_by` shows; #9 is a tombstone with its base title and uid, no field and no body.
    #[test]
    fn tomb_rows_follow_the_deletes() {
        let (hb, b) = hand();
        let cols = Columns::parse(&b).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let enc = files_of(&fold_hand(&hb, &fb).delta);
        let v = SegView::parse(&enc.seg).unwrap();
        let mut want = vec![1, 0, 0, 0, 16, 0, 0, 0];
        for x in [9u32, 103, 3, 52] {
            want.extend(x.to_le_bytes());
        }
        assert_eq!(v.need(tag::TOMB).unwrap(), &want[..]);
        let d = read_back(&enc);
        let i = d.graph.rows.iter().position(|r| r.id == 9).unwrap();
        let (r, t) = (&d.graph.rows[i], &d.rows[i]);
        assert!(r.node.deleted());
        assert_eq!(t.title, Some(Vocab::new().title(9)));
        assert_eq!(
            (t.fields.as_ref(), t.body, t.uid),
            (None, None, Some(full_uid(9)))
        );
        assert_eq!(
            t.tomb,
            Some(Tomb {
                tx: 103,
                reason: 3,
                replaced_by: 52
            })
        );
    }

    /// The text sections index the touched rows ([F09 §12]; u21 cut D-7): `TERMS` and `POST` are the tokenizer's index
    /// of the listed live rows' titles and abstracts (field 2) from a naive replay, `DOCLEN` their counts, and `FTSSTAT`
    /// equals a naive recount over the whole view (every base row the window did not touch from the base's generator,
    /// every touched or created row from the replay, deleted rows not counted).
    #[test]
    fn text_sections_index_the_touched_rows() {
        let vocab = Vocab::new();
        for seed in 1..=3 {
            let n = 300;
            let c = case(n, 400, seed);
            let cols = Columns::parse(&c.base).unwrap();
            let fb = FullBase::parse(&cols).unwrap();
            let d = read_back(&files_of(&fold_mem(&c.tail, &fb).delta));
            let (m, _) = naive(n, &c.tail.records);
            let abstract_of = |x: &Naive| -> Vec<u8> {
                x.fields
                    .get(&2)
                    .and_then(|v| text_of(v).map(<[u8]>::to_vec))
                    .unwrap_or_default()
            };
            let count = |s: &[u8]| tokenize(s, |_| {}).unwrap();
            let mut stat = [FtsStat::default(); 3];
            let mut add = |t: u32, a: u32| {
                for (f, k) in [(0, t), (1, a)] {
                    if k > 0 {
                        stat[f].docs += 1;
                        stat[f].total_len += u64::from(k);
                    }
                }
            };
            let max = m.keys().copied().max().unwrap_or(n).max(n);
            let mut docs: Vec<(u32, Vec<u8>, Vec<u8>)> = Vec::new();
            let listed: BTreeSet<u32> = d.graph.rows.iter().map(|r| r.id).collect();
            for id in 1..=max {
                match m.get(&id) {
                    Some(x) if x.live && !x.deleted => {
                        let (t, a) = (x.title.clone().unwrap_or_default(), abstract_of(x));
                        add(count(&t), count(&a));
                        docs.push((id, t, a));
                    }
                    Some(_) => {}
                    None if id <= n => {
                        let a = text_of(&text_value(&vocab.fields(id)[0].1))
                            .unwrap()
                            .to_vec();
                        add(count(&vocab.title(id)), count(&a));
                        if listed.contains(&id) {
                            docs.push((id, vocab.title(id), a));
                        }
                    }
                    None => {}
                }
            }
            assert_eq!(
                d.ftsstat, stat,
                "seed {seed}: FTSSTAT against the whole view"
            );
            assert_ne!(
                d.ftsstat, fb.ftsstat,
                "seed {seed}: the window changes the view's text"
            );
            let listed_docs: Vec<Doc<'_>> = docs
                .iter()
                .filter(|x| listed.contains(&x.0))
                .map(|(id, t, a)| Doc {
                    id: *id,
                    fields: [t, a, b""],
                })
                .collect();
            assert_eq!(
                d.text,
                TextIndex::of(&listed_docs).unwrap(),
                "seed {seed}: TERMS, POST"
            );
            for (r, t) in d.graph.rows.iter().zip(&d.rows) {
                let want = listed_docs
                    .iter()
                    .find(|x| x.id == r.id)
                    .map(|x| text::doclen(x).unwrap())
                    .unwrap_or([0; 3]);
                assert_eq!(t.doclen, want, "seed {seed} #{} DOCLEN", r.id);
            }
        }
    }

    /// `SYMTAB` holds the window's new symbols ([F09 §14.2], `09-segments.md:933-966`): the overlay's symbols by (class,
    /// id), here `session` 17 and `stmt` 9 (`tail/synth.rs`), in the layout of §14.2: the header, a 20-byte entry per
    /// class with `first_id`, `n` and its arrays' offsets, `by_id` (pool offsets) and `by_str`, then the `vstr` pool.
    #[test]
    fn symtab_holds_the_windows_new_symbols() {
        let c = case(200, 400, 2);
        let cols = Columns::parse(&c.base).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let enc = files_of(&fold_mem(&c.tail, &fb).delta);
        let o = c.tail.overlay().unwrap();
        let mut want: Vec<(u8, u32, String)> = o
            .symbols()
            .iter()
            .map(|s| (s.class, s.id, s.text.to_string()))
            .collect();
        want.sort_by_key(|s| (s.0, s.1));
        assert_eq!(
            want,
            [
                (3, 17, "probe:session".to_string()),
                (7, 9, "tx.edit".to_string())
            ]
        );
        assert_eq!(read_back(&enc).symbols, want);
        let v = SegView::parse(&enc.seg).unwrap();
        let s = v.section(tag::SYMTAB).unwrap();
        assert_eq!(v.entry(tag::SYMTAB).unwrap().count, 2);
        assert_eq!(
            &s[0..8],
            &[2, 0, 0, 0, 22, 0, 0, 0],
            "n_classes 2, pool_len 14 + 8"
        );
        assert_eq!(
            &s[8..28],
            &[
                3, 0, 0, 0, 17, 0, 0, 0, 1, 0, 0, 0, 48, 0, 0, 0, 52, 0, 0, 0
            ]
        );
        assert_eq!(
            &s[28..48],
            &[7, 0, 0, 0, 9, 0, 0, 0, 1, 0, 0, 0, 56, 0, 0, 0, 60, 0, 0, 0]
        );
        assert_eq!(
            &s[48..64],
            &[0, 0, 0, 0, 17, 0, 0, 0, 14, 0, 0, 0, 9, 0, 0, 0]
        );
        assert_eq!(&s[64..], b"\x0dprobe:session\x07tx.edit");
    }

    /// `REFS` is the base's snapshot with `main` moved by the window ([F11 §3.6], §3.9): its `tip`, `tip_lsn` and `gen`
    /// the last commit's, `ref_seq_next` its `ref_seq` + 1, the totals and counters raised by each commit's ops and
    /// op-list bytes (its frames), `absorbed` [(0, the last `ref_seq`)], `moves` the window's lsns newest first before the
    /// base's, at most 32; the 49 lanes as in the base; `aux` 50.
    #[test]
    fn refs_snapshot_holds_every_ref_with_main_at_the_windows_tip() {
        let c = case(200, 400, 3);
        let cols = Columns::parse(&c.base).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let d = read_back(&files_of(&fold_mem(&c.tail, &fb).delta));
        let base = full_refs(&spec_of(&c.tail));
        assert_eq!(d.next_ref_id, 50);
        assert_eq!(d.refs.len(), 50);
        assert_eq!(d.refs[1..], base[1..], "the lanes");
        let mut main = base[0].clone();
        let (mut ops, mut bytes, mut lsns) = (0u64, 0u64, Vec::new());
        for r in &c.tail.records {
            if let Payload::Commit(cm) = &r.payload {
                ops += cm.ops.len() as u64;
                bytes += cm
                    .op_spans()
                    .unwrap()
                    .iter()
                    .map(|s| u64::from(s.2))
                    .sum::<u64>();
                lsns.push(r.lsn);
                main.tip = cm.commit_id[..16].try_into().unwrap();
                main.tip_lsn = r.lsn;
                main.gen_no = cm.gen_no;
                main.ref_seq_next = cm.ref_seq + 1;
                main.absorbed = vec![(0, cm.ref_seq)];
            }
        }
        assert!(lsns.len() > 2, "{} commits", lsns.len());
        main.ops_total += ops;
        main.bytes_total += bytes;
        main.ops_since_fork += ops as u32;
        main.overlay_ops += ops as u32;
        main.overlay_bytes += bytes as u32;
        let mut moves: Vec<u64> = lsns.iter().rev().copied().collect();
        moves.extend(&base[0].moves);
        moves.truncate(32);
        main.moves = moves;
        assert_eq!(d.refs[0], main);
    }

    /// `IDEM`, `ALLOC` and `UIDX` follow the window ([F11 §8]–§9): `ALLOC` and `UIDX` equal the overlay's
    /// ([`crate::overlay::Overlay::alloc`], `uidx`, u10's naive-checked tables); `IDEM` holds one entry per commit, found
    /// by its key, bound to `main` with the commit's `ref_seq`, `hlc` and lsn and no inline result.
    #[test]
    fn idem_alloc_and_uidx_layers_follow_the_window() {
        let c = case(200, 400, 1);
        let cols = Columns::parse(&c.base).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let enc = files_of(&fold_mem(&c.tail, &fb).delta);
        let d = read_back(&enc);
        let o = c.tail.overlay().unwrap();
        let (first, rows) = o.alloc();
        assert!(first > 200 && !rows.is_empty());
        assert_eq!(d.alloc, (first, rows.to_vec()));
        assert_eq!(d.uidx, o.uidx());
        let v = SegView::parse(&enc.seg).unwrap();
        let body = Body::parse(v.section(tag::IDEM).unwrap(), 72).unwrap();
        let mut n = 0;
        for r in &c.tail.records {
            let Payload::Commit(cm) = &r.payload else {
                continue;
            };
            let (key, payload) = cm.idem.unwrap();
            assert_eq!(
                rt::idem_lookup(&body, &key),
                Some(IdemEntry {
                    key,
                    payload,
                    ref_id: 0,
                    branch_sym: cm.ref_sym,
                    ref_seq: cm.ref_seq,
                    flags: 1,
                    append_hlc: cm.hlc,
                    origin_lsn: r.lsn,
                })
            );
            n += 1;
        }
        assert_eq!((body.hdr.aux, d.idem.len()), (n, n as usize));
    }

    /// `TREES`, `FILEOBS` and `FILES` follow the window ([F11 §12.4]–§12.5; [F09 §14.4]): each tree's epochs are the
    /// base's with each `TreeReg` epoch replacing its scope's, `last_settle_hlc` raised to the newest, and each newest
    /// epoch is the one the overlay indexes ([`crate::overlay::Overlay::epochs`]); `FILEOBS` holds the image of each row
    /// the overlay indexes ([`crate::overlay::Overlay::files`]), by (`n`, `tree`); `FILES` is the base's 12 rows and
    /// `blobs.15`'s, with its length and digest.
    #[test]
    fn trees_fileobs_and_files_follow_the_window() {
        let c = case(200, 400, 2);
        let cols = Columns::parse(&c.base).unwrap();
        let fb = FullBase::parse(&cols).unwrap();
        let enc = files_of(&fold_mem(&c.tail, &fb).delta);
        let d = read_back(&enc);
        let o = c.tail.overlay().unwrap();
        let at = |lsn: u64| c.tail.records.iter().find(|r| r.lsn == lsn).unwrap();
        let mut trees = full_trees();
        for r in &c.tail.records {
            if let Payload::TreeReg(t) = &r.payload {
                let row = trees.iter_mut().find(|x| x.key == t.tree).unwrap();
                row.epochs = vec![t.epoch];
                row.last_settle_hlc = row.last_settle_hlc.max(t.epoch.hlc);
            }
        }
        assert_eq!(d.trees, trees);
        assert!(!o.epochs().is_empty());
        for ep in o.epochs() {
            let Payload::TreeReg(t) = &at(ep.lsn).payload else {
                panic!("an epoch's record");
            };
            let row = d.trees.iter().find(|x| x.key == ep.tree).unwrap();
            assert!(row.epochs.contains(&t.epoch));
        }
        let images: Vec<Vec<u8>> = o
            .files()
            .iter()
            .map(|f| {
                let Payload::FileObs(b) = &at(f.lsn).payload else {
                    panic!("a FileObs record");
                };
                b.rows[f.row as usize].image.to_vec()
            })
            .collect();
        assert!(!images.is_empty());
        assert_eq!(d.fileobs, images);
        let (blobs, row) = enc.blobs.as_ref().unwrap();
        assert_eq!(
            (row.family, row.file_no, row.total_len, &row.digest16[..]),
            (6, 15, blobs.len() as u64, &blobs[72..88])
        );
        assert_eq!(d.files, full_files(&spec_of(&c.tail)), "the base's 12");
        let v = SegView::parse(&enc.seg).unwrap();
        let mut all = full_files(&spec_of(&c.tail));
        all.push(*row);
        assert_eq!(
            rt::decode_files(v.section(tag::FILES).unwrap()).unwrap(),
            all
        );
    }
}
