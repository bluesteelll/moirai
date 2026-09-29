//! [F09] segments: `SegHdr` and the section table (§2), the tag registry and placement (§3), the layout classes (§4),
//! every section this chapter owns (§5–§14, §16), the runtime sections through [`crate::runtime`] (§14.5, §15), and the
//! open and full checks of §17.1 that one file allows (V-1, V-2, V-4–V-7, V-9–V-11). [`decode_container`] and
//! [`encode_container`] also serve the `hist` and `blobs` files of [F10].

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::commit::{CkimgEntry, Op, decode_ckimg_entry, encode_ckimg_entry};
use crate::lock::check_format;
use crate::prim::{Reader, Result, Writer, blake3_256, err, xxh3_64};
use crate::runtime::{self, SegKind, Table};
use crate::value::{self, AnchorRec, Creator, FieldEntry, Item, ItemBody, NONE32, NodeHdr};

/// Size of `SegHdr` ([F09 §2.1]).
pub const SEG_HDR: usize = 120;

/// `SegHdr` ([F09 §2.1]); the computed fields (`n_sections`, `total_len`, the digests) are recomputed on encode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegHdr {
    /// `FileFamily` value: 3, 4, 5, 9 (this chapter), 2, 6 ([F10]).
    pub seg_kind: u8,
    /// Tokenizer version: 0 or 1.
    pub tok_ver: u8,
    /// Row count per kind (§2.3).
    pub n_rows: u32,
    /// The number in the file's own name.
    pub file_no: u32,
    /// `seg-branch` ref id.
    pub ref_id: u32,
    /// `blobs` dictionary number.
    pub dict_no: u32,
    /// `seq` bound.
    pub base_seq: u64,
    /// First folded lsn.
    pub from_lsn: u64,
    /// Fold bound.
    pub upto_lsn: u64,
    /// Runtime fold bound.
    pub rt_upto_lsn: u64,
    /// File length (as read; recomputed on encode).
    pub total_len: u64,
    /// BLAKE3-256 over `[data_off, total_len)` (as read).
    pub seg_digest: [u8; 32],
}

/// `SecEnt` ([F09 §2.2]) without its computed `off` and `xxh3`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecEnt {
    /// Section tag.
    pub tag: u16,
    /// Bit 0 `derived-optional`.
    pub flags: u16,
    /// Item count per the layout class.
    pub count: u32,
}

/// A decoded container: header, entries and each section's bytes with its absolute offset.
#[derive(Clone, Debug)]
pub struct Container<'a> {
    /// The header.
    pub hdr: SegHdr,
    /// The entries, in table order.
    pub entries: Vec<SecEnt>,
    /// Each section's bytes and absolute offset.
    pub sections: Vec<(&'a [u8], usize)>,
}

/// Decodes `SegHdr`, the section table and the section placement with checks V-1, V-2, V-4 (against the buffer's
/// length), V-5 (header reserved fields, `tok_ver`), V-6, V-9 and P-1–P-4 ([F09 §2], §17.1).
pub fn decode_container(b: &[u8]) -> Result<Container<'_>> {
    if b.len() < SEG_HDR {
        return err(0, "a segment needs a 120-byte SegHdr [F09 §2.1]");
    }
    let mut r = Reader::new(b);
    if r.array::<4>()? != *b"MSEG" {
        return err(0, "SegHdr magic is not MSEG [F09 §2.1]");
    }
    check_format(r.u16()?, 4, "segment")?;
    let hdr_sum = xxh3_64(&b[..112]);
    if u64::from_le_bytes(b[112..120].try_into().expect("8")) != hdr_sum {
        return err(112, "SegHdr hdr_xxh3 mismatch [F09 §17.1 V-2]");
    }
    let seg_kind = r.u8()?;
    if ![2, 3, 4, 5, 6, 9].contains(&seg_kind) {
        return err(
            6,
            format!("seg_kind {seg_kind} is not a segment family [F09 §2.1]"),
        );
    }
    let tok_ver = r.u8()?;
    if tok_ver > 1 || (matches!(seg_kind, 2 | 6) && tok_ver != 0) {
        return err(
            7,
            "tok_ver outside 0-1, or non-zero in hist or blobs [F09 §17.1 V-5]",
        );
    }
    let n_rows = r.u32()?;
    let n_sections = usize::from(r.u16()?);
    r.zeros(2, "SegHdr._reserved")?;
    let file_no = r.u32()?;
    let ref_id = r.u32()?;
    let dict_no = r.u32()?;
    r.zeros(4, "SegHdr._reserved")?;
    let hdr = SegHdr {
        seg_kind,
        tok_ver,
        n_rows,
        file_no,
        ref_id,
        dict_no,
        base_seq: r.u64()?,
        from_lsn: r.u64()?,
        upto_lsn: r.u64()?,
        rt_upto_lsn: r.u64()?,
        total_len: r.u64()?,
        seg_digest: r.b32()?,
    };
    if file_no == 0 {
        return err(16, "SegHdr file_no 0 [F09 §2.1]");
    }
    check_kind_fields(&hdr)?;
    let table_sum = r.u64()?;
    if hdr.total_len != b.len() as u64 {
        return err(
            64,
            format!(
                "total_len {} differs from the file size {} [F09 §17.1 V-4]",
                hdr.total_len,
                b.len()
            ),
        );
    }
    let data_off = SEG_HDR + 32 * n_sections;
    if b.len() < data_off {
        return err(120, "the section table runs past the file [F09 §2.2]");
    }
    if xxh3_64(&b[SEG_HDR..data_off]) != table_sum {
        return err(104, "table_xxh3 mismatch [F09 §17.1 V-6]");
    }
    if blake3_256(&b[data_off..]) != hdr.seg_digest {
        return err(72, "seg_digest mismatch [F09 §17.1 V-9]");
    }
    let mut entries = Vec::with_capacity(n_sections);
    let mut sections = Vec::with_capacity(n_sections);
    let mut expect = data_off as u64;
    let mut last_tag: Option<u16> = None;
    let mut tr = Reader::with_base(&b[SEG_HDR..data_off], SEG_HDR);
    for _ in 0..n_sections {
        let at = tr.offset();
        let e = SecEnt {
            tag: tr.u16()?,
            flags: tr.u16()?,
            count: tr.u32()?,
        };
        let off = tr.u64()?;
        let len = tr.u64()?;
        let sum = tr.u64()?;
        if last_tag.is_some_and(|t| t >= e.tag) {
            return err(at, "section tags not strictly ascending [F09 §2.2 P-1]");
        }
        last_tag = Some(e.tag);
        if e.flags & 0xFFFE != 0 {
            return err(at + 2, "SecEnt flags bits 1-15 are not zero [F09 §2.2]");
        }
        let start = expect.next_multiple_of(8);
        if off != start {
            return err(
                at + 8,
                format!(
                    "section 0x{:04X} at {off}, canonical placement is {start} [F09 §2.2 P-2]",
                    e.tag
                ),
            );
        }
        if b[expect as usize..start as usize].iter().any(|&x| x != 0) {
            return err(
                expect as usize,
                "padding between sections is not zero [F09 §2.2 P-2]",
            );
        }
        let end = off.checked_add(len).filter(|&x| x <= b.len() as u64);
        let Some(end) = end else {
            return err(at + 16, "a section runs past total_len [F09 §2.2]");
        };
        let s = &b[off as usize..end as usize];
        if xxh3_64(s) != sum {
            return err(
                at + 24,
                format!("section 0x{:04X} xxh3 mismatch [F09 §17.1 V-9]", e.tag),
            );
        }
        entries.push(e);
        sections.push((s, off as usize));
        expect = end;
    }
    if expect != hdr.total_len {
        return err(
            64,
            "total_len is not the end of the last section [F09 §2.2 P-3]",
        );
    }
    Ok(Container {
        hdr,
        entries,
        sections,
    })
}

/// [F09 §2.3]: the fields a kind marks 0, and the bounds' order.
fn check_kind_fields(h: &SegHdr) -> Result<()> {
    let bad = match h.seg_kind {
        3 => h.ref_id != 0 || h.dict_no != 0 || h.from_lsn != 0 || h.rt_upto_lsn < h.upto_lsn,
        4 => {
            h.ref_id != 0 || h.dict_no != 0 || h.rt_upto_lsn < h.upto_lsn || h.from_lsn > h.upto_lsn
        }
        5 => h.dict_no != 0 || h.rt_upto_lsn != 0,
        9 => {
            h.ref_id != 0
                || h.dict_no != 0
                || h.base_seq != 0
                || h.from_lsn != 0
                || h.upto_lsn != 0
                || h.rt_upto_lsn != 0
        }
        2 => h.ref_id != 0 || h.dict_no != 0 || h.rt_upto_lsn != 0,
        _ => {
            h.ref_id != 0
                || h.base_seq != 0
                || h.from_lsn != 0
                || h.upto_lsn != 0
                || h.rt_upto_lsn != 0
        }
    };
    if bad {
        return err(
            20,
            format!(
                "SegHdr fields break §2.3 for seg_kind {} [F09 §17.1 V-5]",
                h.seg_kind
            ),
        );
    }
    Ok(())
}

/// Assembles a file from its header and `(entry, bytes)` sections in tag order: placement, checksums, digest and header
/// recomputed ([F09 §2], §17.3).
pub fn encode_container(h: &SegHdr, secs: &[(SecEnt, Vec<u8>)]) -> Vec<u8> {
    let data_off = SEG_HDR + 32 * secs.len();
    let mut data = Vec::new();
    let mut table = Writer::new();
    let mut pos = data_off as u64;
    for (e, s) in secs {
        let start = pos.next_multiple_of(8);
        data.resize(data.len() + (start - pos) as usize, 0);
        table.u16(e.tag);
        table.u16(e.flags);
        table.u32(e.count);
        table.u64(start);
        table.u64(s.len() as u64);
        table.u64(xxh3_64(s));
        data.extend_from_slice(s);
        pos = start + s.len() as u64;
    }
    let mut w = Writer::new();
    w.bytes(b"MSEG");
    w.u16(1);
    w.u8(h.seg_kind);
    w.u8(h.tok_ver);
    w.u32(h.n_rows);
    w.u16(secs.len() as u16);
    w.u16(0);
    w.u32(h.file_no);
    w.u32(h.ref_id);
    w.u32(h.dict_no);
    w.u32(0);
    w.u64(h.base_seq);
    w.u64(h.from_lsn);
    w.u64(h.upto_lsn);
    w.u64(h.rt_upto_lsn);
    w.u64(pos);
    w.bytes(&blake3_256(&data));
    w.u64(xxh3_64(table.as_slice()));
    let hs = xxh3_64(w.as_slice());
    w.u64(hs);
    w.bytes(table.as_slice());
    w.bytes(&data);
    w.into_vec()
}

/// A frozen bitset ([F09 §4.5]): chunks by `hi`, each with its low halves ascending.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Bitset {
    /// (hi, lows).
    pub chunks: Vec<(u16, Vec<u16>)>,
}

impl Bitset {
    /// Total cardinality.
    pub fn card(&self) -> u64 {
        self.chunks.iter().map(|c| c.1.len() as u64).sum()
    }

    /// Every member.
    pub fn members(&self) -> Vec<u32> {
        self.chunks
            .iter()
            .flat_map(|(hi, lo)| {
                lo.iter()
                    .map(move |l| (u32::from(*hi) << 16) | u32::from(*l))
            })
            .collect()
    }

    /// A bitset from ascending members.
    pub fn from_members(m: &[u32]) -> Bitset {
        let mut chunks: Vec<(u16, Vec<u16>)> = Vec::new();
        for &x in m {
            let (hi, lo) = ((x >> 16) as u16, (x & 0xFFFF) as u16);
            match chunks.last_mut() {
                Some((h, v)) if *h == hi => v.push(lo),
                _ => chunks.push((hi, vec![lo])),
            }
        }
        Bitset { chunks }
    }
}

/// Decodes a frozen bitset with its canonical container rules.
pub fn decode_bitset(b: &[u8], at: usize) -> Result<Bitset> {
    let mut r = Reader::with_base(b, at);
    let n = r.u32()? as usize;
    r.zeros(4, "bitset._reserved")?;
    if n > (b.len().saturating_sub(8)) / 16 {
        return err(at, "bitset chunk index runs past the section [F09 §4.5]");
    }
    let mut idx = Vec::with_capacity(n);
    for _ in 0..n {
        let e_at = r.offset();
        let hi = r.u16()?;
        r.zeros(2, "chunk._reserved")?;
        let card = r.u32()?;
        let off = r.u64()?;
        if idx.last().is_some_and(|&(h, _, _)| h >= hi) || card == 0 || card > 65_536 {
            return err(
                e_at,
                "bitset chunks unsorted, or a card outside 1-65536 [F09 §4.5]",
            );
        }
        idx.push((hi, card, off));
    }
    let mut pos = (8 + 16 * n) as u64;
    let mut chunks = Vec::with_capacity(n);
    for (hi, card, off) in idx {
        let start = pos.next_multiple_of(8);
        if off != start {
            return err(
                at,
                "bitset container not at its canonical offset [F09 §4.5]",
            );
        }
        if b.get(pos as usize..start as usize)
            .is_none_or(|p| p.iter().any(|&x| x != 0))
        {
            return err(at + pos as usize, "bitset padding is not zero [F09 §4.5]");
        }
        let mut cr = Reader::with_base(&b[start as usize..], at + start as usize);
        let lows = if card <= 4096 {
            let mut v: Vec<u16> = Vec::with_capacity(card as usize);
            for _ in 0..card {
                let l = cr.u16()?;
                if v.last().is_some_and(|&p| p >= l) {
                    return err(
                        cr.offset(),
                        "bitset array not strictly ascending [F09 §4.5]",
                    );
                }
                v.push(l);
            }
            pos = start + 2 * u64::from(card);
            v
        } else {
            let bm = cr.bytes(8192)?;
            let v: Vec<u16> = (0..65_536u32)
                .filter(|&l| bm[(l / 8) as usize] >> (l % 8) & 1 != 0)
                .map(|l| l as u16)
                .collect();
            if v.len() != card as usize {
                return err(
                    at + start as usize,
                    "bitmap popcount differs from card [F09 §4.5]",
                );
            }
            pos = start + 8192;
            v
        };
        if hi == 0 && lows.first() == Some(&0) {
            return err(at, "bitset holds #N 0 [F09 §4.5]");
        }
        chunks.push((hi, lows));
    }
    if pos != b.len() as u64 {
        return err(
            at + pos as usize,
            "bytes after the last bitset container [F09 §4.5]",
        );
    }
    Ok(Bitset { chunks })
}

/// Encodes a frozen bitset canonically.
pub fn encode_bitset(s: &Bitset) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(s.chunks.len() as u32);
    w.u32(0);
    let mut pos = (8 + 16 * s.chunks.len()) as u64;
    let mut offs = Vec::new();
    for (_, lows) in &s.chunks {
        let start = pos.next_multiple_of(8);
        offs.push(start);
        pos = start
            + if lows.len() <= 4096 {
                2 * lows.len() as u64
            } else {
                8192
            };
    }
    for ((hi, lows), off) in s.chunks.iter().zip(&offs) {
        w.u16(*hi);
        w.u16(0);
        w.u32(lows.len() as u32);
        w.u64(*off);
    }
    for ((_, lows), off) in s.chunks.iter().zip(&offs) {
        w.zeros(*off as usize - w.len());
        if lows.len() <= 4096 {
            lows.iter().for_each(|l| w.u16(*l));
        } else {
            let mut bm = vec![0u8; 8192];
            for &l in lows {
                bm[usize::from(l) / 8] |= 1 << (l % 8);
            }
            w.bytes(&bm);
        }
    }
    w.into_vec()
}

/// A ± list ([F09 §4.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlusMinus {
    /// Added members.
    pub plus: Vec<u32>,
    /// Removed members.
    pub minus: Vec<u32>,
}

/// Decodes a ± list: each list strictly ascending, disjoint, no 0, not both empty.
pub fn decode_pm(b: &[u8], at: usize) -> Result<PlusMinus> {
    let mut r = Reader::with_base(b, at);
    let np = r.u32()? as usize;
    let nm = r.u32()? as usize;
    if np + nm == 0 || b.len() != 8 + 4 * (np + nm) {
        return err(
            at,
            "± list empty, or its length is not 8 + 4(n_plus + n_minus) [F09 §4.6]",
        );
    }
    let mut read = |n: usize| -> Result<Vec<u32>> {
        let mut v: Vec<u32> = Vec::with_capacity(n);
        for _ in 0..n {
            let x = r.u32()?;
            if x == 0 || v.last().is_some_and(|&p| p >= x) {
                return err(at, "± list not strictly ascending, or holds 0 [F09 §4.6]");
            }
            v.push(x);
        }
        Ok(v)
    };
    let plus = read(np)?;
    let minus = read(nm)?;
    let ps: BTreeSet<u32> = plus.iter().copied().collect();
    if minus.iter().any(|m| ps.contains(m)) {
        return err(at, "± list plus and minus are not disjoint [F09 §4.6]");
    }
    Ok(PlusMinus { plus, minus })
}

/// Encodes a ± list.
pub fn encode_pm(p: &PlusMinus) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(p.plus.len() as u32);
    w.u32(p.minus.len() as u32);
    p.plus.iter().chain(&p.minus).for_each(|x| w.u32(*x));
    w.into_vec()
}

/// A `BlobRef` ([F09 §6.3]), 36 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlobRef {
    /// Content address.
    pub hash: [u8; 16],
    /// `blobs.<n>`.
    pub file: u32,
    /// Payload offset.
    pub off: u64,
    /// Stored length.
    pub len: u32,
    /// Raw length.
    pub raw_len: u32,
}

/// One promoted field ([F09 §10.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fpromo {
    /// Field symbol.
    pub field_sym: u32,
    /// Slot = position.
    pub slot: u16,
    /// Element type id.
    pub vtype: u8,
    /// 1 column, 2 bitmap.
    pub index: u8,
    /// 0 scalar, 1 set.
    pub form: u8,
}

/// The promoted element width of a type id ([F09 §10.1]).
pub fn elem_w(vtype: u8) -> Option<u8> {
    Some(match vtype {
        1 => 1,
        2..=4 => 8,
        5 => 2,
        7 | 9 => 4,
        10 => 16,
        _ => return None,
    })
}

/// [F09 §10.4]: compares two promoted encodings of one type.
fn promoted_cmp(vtype: u8, a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let le = |x: &[u8]| {
        let mut v = [0u8; 8];
        v[..x.len()].copy_from_slice(x);
        u64::from_le_bytes(v)
    };
    match vtype {
        2 | 3 => (le(a) as i64).cmp(&(le(b) as i64)),
        4 => f64::from_bits(le(a)).total_cmp(&f64::from_bits(le(b))),
        10 => a.cmp(b),
        _ => le(a).cmp(&le(b)),
    }
}

fn check_promoted(vtype: u8, e: &[u8], at: usize) -> Result<()> {
    match vtype {
        1 if e[0] > 1 => err(at, "promoted bool not 00 or 01 [F09 §10.1]"),
        4 => value::check_f64(u64::from_le_bytes(e.try_into().expect("8")), at),
        9 if e == [0; 4] => err(at, "promoted ref 0 [F09 §10.1]"),
        _ => Ok(()),
    }
}

/// `FCOL.<slot>` ([F09 §10.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fcol {
    /// Field symbol.
    pub field_sym: u32,
    /// Element type id.
    pub vtype: u8,
    /// 0 scalar, 1 set.
    pub form: u8,
    /// Per row: `None` absent, else its elements (one for the scalar form).
    pub rows: Vec<Option<Vec<Vec<u8>>>>,
}

/// `FIDX.<slot>` ([F09 §10.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fidx {
    /// Field symbol.
    pub field_sym: u32,
    /// Element type id.
    pub vtype: u8,
    /// Values with their sets: frozen bitsets (form 0) or ± lists (form 1).
    pub values: Vec<(Vec<u8>, FidxBody)>,
    /// 0 frozen, 1 ± lists.
    pub form: u8,
}

/// A value's body in `FIDX`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FidxBody {
    /// Base.
    Frozen(Bitset),
    /// Upper segments.
    Pm(PlusMinus),
}

/// `STATS` ([F09 §11]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    /// (edge_kind, dir, max_degree, edges, hist).
    pub edges: Vec<(u8, u8, u32, u64, [u32; 16])>,
    /// (kind, field_sym, present, distinct).
    pub fields: Vec<(u8, u32, u32, u32)>,
}

/// One term of `TERMS` ([F09 §12.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    /// The term bytes.
    pub term: Vec<u8>,
    /// Offset of its posting list in `POST`.
    pub post_off: u64,
}

/// One posting ([F09 §12.3]): (`#N`, fmask, tfs).
pub type Posting = (u32, u8, Vec<u16>);

/// `SYMTAB` ([F09 §14.2]): per class, `first_id` and the strings of ids `first_id…`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symtab {
    /// (class, first_id, strings).
    pub classes: Vec<(u8, u32, Vec<String>)>,
}

impl Symtab {
    /// (class, id, string) triples.
    pub fn triples(&self) -> Vec<(u8, u32, String)> {
        self.classes
            .iter()
            .flat_map(|(c, f, v)| {
                v.iter()
                    .enumerate()
                    .map(move |(j, s)| (*c, f + j as u32, s.clone()))
            })
            .collect()
    }
}

/// A `PATHIDX`/`ALIASIDX` row ([F09 §13.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRow {
    /// Root symbol.
    pub root: u16,
    /// File node.
    pub id: u32,
    /// `fold_v1(path)`.
    pub fold: Vec<u8>,
    /// The exact path.
    pub path: Vec<u8>,
}

/// An `ANCHORS` row ([F09 §13.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorRow {
    /// Referrer.
    pub src: u32,
    /// File node.
    pub dst: u32,
    /// `aN`.
    pub anchor: u32,
    /// The record bytes and their decode.
    pub rec: (Vec<u8>, Box<AnchorRec>),
}

/// A decoded section ([F09 §5]–§16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sec {
    /// `IDS`.
    Ids(Vec<u32>),
    /// `NODE`.
    Node(Vec<NodeHdr>),
    /// `CREATOR`.
    Creator(Vec<Creator>),
    /// `TOPO`, `DEFER`, `DUE`, `OUT_OFF`, `OUT_DST`, `IN_OFF`, `IN_SRC`.
    U32s(Vec<u32>),
    /// `OUT_KIND`, `IN_KIND`.
    U8s(Vec<u8>),
    /// `PREV`.
    U64s(Vec<u64>),
    /// `UID` (uid, id).
    Uid(Vec<([u8; 16], u32)>),
    /// `TOUCH`, or a base `BM.<i>`.
    Bitset(Bitset),
    /// An upper `BM.<i>`.
    Pm(PlusMinus),
    /// `TITLE_BLOB`.
    Titles(Vec<String>),
    /// `FIELDS_BLOB`.
    Fields(Vec<Vec<FieldEntry>>),
    /// `BLOBTAB`.
    BlobTab(Vec<BlobRef>),
    /// `EDGE_PROPS` (edge, pflags, pinned_commit).
    EdgeProps(Vec<(u32, u8, [u8; 32])>),
    /// `TOMB` (id, tx, reason_sym, replaced_by).
    Tomb(Vec<(u32, u32, u32, u32)>),
    /// `SCHEMA` rows (bytes, item).
    Schema(Vec<(Vec<u8>, Item)>),
    /// `BMDIR` keys.
    Bmdir(Vec<[u8; 4]>),
    /// `TERMS`.
    Terms(Vec<Term>),
    /// `POST`: one list per term.
    Post(Vec<Vec<Posting>>),
    /// `DOCLEN`.
    Doclen(Vec<[u16; 3]>),
    /// `FTSSTAT` (docs, total_len) × 3.
    FtsStat([(u32, u64); 3]),
    /// `STATS`.
    Stats(Stats),
    /// `FPROMO`.
    Fpromo(Vec<Fpromo>),
    /// `FCOL.<slot>`.
    Fcol(Fcol),
    /// `FIDX.<slot>`.
    Fidx(Fidx),
    /// `PATHIDX`, `ALIASIDX`.
    Paths(Vec<PathRow>),
    /// `ANCHORS`.
    Anchors(Vec<AnchorRow>),
    /// `ANCHOR_UID` (uid, src, dst, anchor).
    AnchorUid(Vec<([u8; 16], u32, u32, u32)>),
    /// `VIOLATIONS`.
    Violations(Vec<Op>),
    /// `CKIMG`.
    Ckimg(Vec<CkimgEntry>),
    /// `SYMTAB`.
    Symtab(Symtab),
    /// An [F11 §2.1] body.
    Runtime(Box<runtime::Section>),
    /// An unknown `derived-optional` section, kept verbatim.
    Unknown(Vec<u8>),
}

/// Section placement per segment kind ([F09 §3.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pl {
    R,
    C,
    O,
    F,
}

fn placement(tag: u16, kind: u8) -> Option<Pl> {
    use Pl::{C, F, O, R};
    let col = match kind {
        3 => 0,
        4 => 1,
        5 => 2,
        _ => 3,
    };
    let row: [Pl; 4] = match tag {
        0x0001 => [F, R, R, R],
        0x0002..=0x0007 | 0x0010..=0x0012 | 0x0020..=0x0026 | 0x0030 | 0x0031 | 0x0080..=0x0084 => {
            [R, R, R, R]
        }
        0x0008 => [F, F, R, F],
        0x0032 | 0x0040 => [R, C, C, C],
        0x0050..=0x0052 | 0x0061 | 0x4000..=0x4FFF | 0x5000..=0x5FFF | 0x8000..=0xFFFE => {
            [C, C, C, C]
        }
        0x0053 => [C, C, C, F],
        0x0060 => [R, F, F, F],
        0x00C0 => [F, F, F, R],
        0x00C1 | 0x00C2 => [F, F, F, C],
        0x0100..=0x0102 | 0x0201..=0x020C => [R, R, F, F],
        0x0210..=0x021A => {
            let t = Table::from_tag(tag)?;
            if t.derived_optional() {
                [O, O, F, F]
            } else {
                [R, R, F, F]
            }
        }
        _ => return None,
    };
    Some(row[col])
}

/// Tags that must carry `derived-optional` ([F09 §3.3]).
fn must_be_derived_optional(tag: u16) -> bool {
    Table::from_tag(tag).is_some_and(|t| t.derived_optional())
}

fn seg_kind_of(kind: u8) -> SegKind {
    match kind {
        3 => SegKind::Base,
        4 => SegKind::Delta,
        5 => SegKind::Branch,
        _ => SegKind::Changeset,
    }
}

/// A decoded graph segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// The header.
    pub hdr: SegHdr,
    /// The entries and their sections, in tag order.
    pub sections: Vec<(SecEnt, Sec)>,
}

fn column<T>(
    b: &[u8],
    at: usize,
    w: usize,
    mut f: impl FnMut(&mut Reader<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    if !b.len().is_multiple_of(w) {
        return err(
            at,
            format!("a column length is not a multiple of {w} [F09 §4.1]"),
        );
    }
    let mut r = Reader::with_base(b, at);
    let mut v = Vec::with_capacity(b.len() / w);
    while !r.is_empty() {
        v.push(f(&mut r)?);
    }
    Ok(v)
}

fn fixed_table<'a>(b: &'a [u8], at: usize, row_w: u32) -> Result<(usize, Reader<'a>)> {
    let mut r = Reader::with_base(b, at);
    let n = r.u32()? as usize;
    let w = r.u32()?;
    if w != row_w || b.len() != 8 + n * row_w as usize {
        return err(
            at,
            format!("fixed table row_w {w} or length breaks §4.2 (row_w {row_w})"),
        );
    }
    Ok((n, r))
}

fn variable_rows(b: &[u8], at: usize) -> Result<Vec<(&[u8], usize)>> {
    let mut r = Reader::with_base(b, at);
    let n = r.u32()? as usize;
    r.zeros(4, "variable table._reserved")?;
    if n > b.len() / 4 {
        return err(at, "variable table offsets run past the section [F09 §4.3]");
    }
    let mut off = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        off.push(r.u32()? as usize);
    }
    let area = 8 + 4 * (n + 1);
    if off[0] != 0 || off.windows(2).any(|w| w[0] >= w[1]) || area + off[n] != b.len() {
        return err(
            at,
            "variable table offsets break §4.3 (off[0] = 0, increasing, off[n] = area)",
        );
    }
    Ok((0..n)
        .map(|i| (&b[area + off[i]..area + off[i + 1]], at + area + off[i]))
        .collect())
}

fn encode_variable(rows: &[Vec<u8>]) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(rows.len() as u32);
    w.u32(0);
    let mut o = 0u32;
    w.u32(0);
    for r in rows {
        o += r.len() as u32;
        w.u32(o);
    }
    rows.iter().for_each(|r| w.bytes(r));
    w.into_vec()
}

fn strictly<T: Ord>(v: &[T], at: usize, what: &str) -> Result<()> {
    if v.windows(2).any(|w| w[0] >= w[1]) {
        return err(
            at,
            format!("{what} not strictly ascending by its key [F09 §4]"),
        );
    }
    Ok(())
}

/// Decodes one section by tag ([F09 §3.1]).
fn decode_sec(tag: u16, b: &[u8], at: usize, kind: u8) -> Result<Sec> {
    Ok(match tag {
        0x0001 => {
            let v = column(b, at, 4, |r| r.u32())?;
            if v.first() == Some(&0) {
                return err(at, "IDS holds 0 [F09 §5.1]");
            }
            strictly(&v, at, "IDS")?;
            Sec::Ids(v)
        }
        0x0002 => Sec::Node(column(b, at, 60, NodeHdr::decode)?),
        0x0003 => Sec::Creator(column(b, at, 6, Creator::decode)?),
        0x0004..=0x0006 | 0x0020 | 0x0021 | 0x0023 | 0x0024 => {
            Sec::U32s(column(b, at, 4, |r| r.u32())?)
        }
        0x0022 | 0x0025 => Sec::U8s(column(b, at, 1, |r| r.u8())?),
        0x00C0 => Sec::U64s(column(b, at, 8, |r| r.u64())?),
        0x0007 => {
            let (n, mut r) = fixed_table(b, at, 20)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push((r.b16()?, r.u32()?));
            }
            strictly(&v, at, "UID")?;
            Sec::Uid(v)
        }
        0x0008 => Sec::Bitset(decode_bitset(b, at)?),
        0x0010 => {
            let mut r = Reader::with_base(b, at);
            let mut v = Vec::new();
            while !r.is_empty() {
                let t_at = r.offset();
                let t = r.vstr()?;
                if t.is_empty() || t.len() > 200 {
                    return err(t_at, "a title is not 1-200 bytes [F09 §6.1]");
                }
                value::text_rules(t, t_at, true)?;
                v.push(t.to_owned());
            }
            Sec::Titles(v)
        }
        0x0011 => {
            let mut r = Reader::with_base(b, at);
            let mut v = Vec::new();
            while !r.is_empty() {
                let e = r.vbytes()?;
                let mut er = Reader::with_base(e, r.offset() - e.len());
                v.push(value::decode_field_block(&mut er)?);
                er.finish("a field block")?;
            }
            Sec::Fields(v)
        }
        0x0012 => {
            let (n, mut r) = fixed_table(b, at, 36)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                let e_at = r.offset();
                let x = BlobRef {
                    hash: r.b16()?,
                    file: r.u32()?,
                    off: r.u64()?,
                    len: r.u32()?,
                    raw_len: r.u32()?,
                };
                if x.file == 0 || x.len == 0 || x.raw_len > 65_536 {
                    return err(
                        e_at,
                        "BlobRef file 0, len 0 or raw_len above 65536 [F09 §6.3]",
                    );
                }
                v.push(x);
            }
            if v.windows(2).any(|w| w[0].hash >= w[1].hash) {
                return err(at, "BLOBTAB not strictly ascending by hash [F09 §6.3]");
            }
            Sec::BlobTab(v)
        }
        0x0026 => {
            let (n, mut r) = fixed_table(b, at, 40)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                let e_at = r.offset();
                let edge = r.u32()?;
                let pflags = r.u8()?;
                r.zeros(3, "EDGE_PROPS._reserved")?;
                let pin = r.b32()?;
                if pflags & !3 != 0 || pflags == 0 || (pflags & 1 == 0) != (pin == [0; 32]) {
                    return err(
                        e_at,
                        "EDGE_PROPS pflags invalid, or pinned_commit disagrees with has_pin [F09 §7.2]",
                    );
                }
                v.push((edge, pflags, pin));
            }
            if v.windows(2).any(|w| w[0].0 >= w[1].0) {
                return err(at, "EDGE_PROPS not strictly ascending by edge [F09 §7.2]");
            }
            Sec::EdgeProps(v)
        }
        0x0030 => {
            let (n, mut r) = fixed_table(b, at, 16)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push((r.u32()?, r.u32()?, r.u32()?, r.u32()?));
            }
            if v.first().is_some_and(|x| x.0 == 0) || v.windows(2).any(|w| w[0].0 >= w[1].0) {
                return err(at, "TOMB ids not strictly ascending, or 0 [F09 §8.1]");
            }
            Sec::Tomb(v)
        }
        0x0032 => {
            let mut v = Vec::new();
            for (row, r_at) in variable_rows(b, at)? {
                let mut r = Reader::with_base(row, r_at);
                let it = Item::decode(&mut r)?;
                r.finish("a SCHEMA row")?;
                v.push((row.to_vec(), it));
            }
            Sec::Schema(v)
        }
        0x0040 => {
            let (n, mut r) = fixed_table(b, at, 4)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                let e_at = r.offset();
                let k: [u8; 4] = r.array()?;
                let ok = match k[0] {
                    1 => k[2] == 0 && k[3] == 0 && k[1] != 0,
                    2 => k[3] == 0 && k[1] != 0,
                    3 => k[1] == 0 && k[2] == 0 && (1..=7).contains(&k[3]),
                    _ => false,
                };
                if !ok {
                    return err(e_at, "BMDIR key invalid [F09 §9.2]");
                }
                v.push(k);
            }
            strictly(&v, at, "BMDIR")?;
            Sec::Bmdir(v)
        }
        0x0050 => Sec::Terms(decode_terms(b, at)?),
        0x0051 => return err(at, "POST is decoded against TERMS [F09 §12.3]"),
        0x0052 => Sec::Doclen(column(b, at, 6, |r| Ok([r.u16()?, r.u16()?, r.u16()?]))?),
        0x0053 => {
            if b.len() != 48 {
                return err(at, "FTSSTAT is not 48 bytes [F09 §12.4]");
            }
            let mut r = Reader::with_base(b, at);
            let mut v = [(0u32, 0u64); 3];
            for x in &mut v {
                let d = r.u32()?;
                r.zeros(4, "FTSSTAT._reserved")?;
                *x = (d, r.u64()?);
            }
            Sec::FtsStat(v)
        }
        0x0060 => {
            let mut r = Reader::with_base(b, at);
            let ne = r.u32()? as usize;
            let nf = r.u32()? as usize;
            if b.len() != 8 + 80 * ne + 16 * nf {
                return err(at, "STATS length breaks §11");
            }
            let mut edges = Vec::with_capacity(ne);
            for _ in 0..ne {
                let e_at = r.offset();
                let k = r.u8()?;
                let d = r.u8()?;
                r.zeros(2, "EdgeStat._reserved")?;
                let mx = r.u32()?;
                let es = r.u64()?;
                let mut h = [0u32; 16];
                for x in &mut h {
                    *x = r.u32()?;
                }
                if d > 1 {
                    return err(e_at, "EdgeStat dir outside 0-1 [F09 §11]");
                }
                edges.push((k, d, mx, es, h));
            }
            if edges
                .windows(2)
                .any(|w| (w[0].0, w[0].1) >= (w[1].0, w[1].1))
            {
                return err(at, "EdgeStat rows not sorted by (edge_kind, dir) [F09 §11]");
            }
            let mut fields = Vec::with_capacity(nf);
            for _ in 0..nf {
                let k = r.u8()?;
                r.zeros(3, "FieldStat._reserved")?;
                let f_at = r.offset();
                let x = (k, r.u32()?, r.u32()?, r.u32()?);
                if x.2 == 0 {
                    return err(f_at, "FieldStat present is 0 [F09 §11]");
                }
                fields.push(x);
            }
            if fields
                .windows(2)
                .any(|w| (w[0].0, w[0].1) >= (w[1].0, w[1].1))
            {
                return err(
                    at,
                    "FieldStat rows not sorted by (kind, field_sym) [F09 §11]",
                );
            }
            Sec::Stats(Stats { edges, fields })
        }
        0x0061 => {
            let (n, mut r) = fixed_table(b, at, 12)?;
            let mut v: Vec<Fpromo> = Vec::with_capacity(n);
            for i in 0..n {
                let e_at = r.offset();
                let f = Fpromo {
                    field_sym: r.u32()?,
                    slot: r.u16()?,
                    vtype: r.u8()?,
                    index: r.u8()?,
                    form: r.u8()?,
                };
                r.zeros(3, "FPROMO._reserved")?;
                // A set's element type is one of [F08 §5.2]'s; of those, int, enum, sym, ref and commitref have a
                // promoted encoding, so a set field never promotes bool, counter or f64.
                let set_elem = matches!(f.vtype, 2 | 5 | 7 | 9 | 10);
                if usize::from(f.slot) != i
                    || elem_w(f.vtype).is_none()
                    || !(1..=2).contains(&f.index)
                    || f.form > 1
                    || (f.form == 1 && !set_elem)
                    || v.last().is_some_and(|p| p.field_sym >= f.field_sym)
                {
                    return err(
                        e_at,
                        "FPROMO row invalid: slot, vtype (a set's element type for form 1), index, form or order [F09 §10.1, F08 §5.2]",
                    );
                }
                v.push(f);
            }
            Sec::Fpromo(v)
        }
        0x4000..=0x4FFF => Sec::Fcol(decode_fcol(b, at)?),
        0x5000..=0x5FFF => Sec::Fidx(decode_fidx(b, at)?),
        0x8000..=0xFFFE => {
            if kind == 3 {
                let s = decode_bitset(b, at)?;
                if s.card() == 0 {
                    return err(at, "a base BM.<i> is empty [F09 §9.2]");
                }
                Sec::Bitset(s)
            } else {
                Sec::Pm(decode_pm(b, at)?)
            }
        }
        0x0080 | 0x0081 => {
            let mut v = Vec::new();
            for (row, r_at) in variable_rows(b, at)? {
                let mut r = Reader::with_base(row, r_at);
                let x = PathRow {
                    root: r.u16()?,
                    id: r.u32()?,
                    fold: r.vbytes()?.to_vec(),
                    path: r.vbytes()?.to_vec(),
                };
                r.finish("a PATHIDX row")?;
                if x.root == 0 || x.id == 0 {
                    return err(r_at, "PATHIDX root or id 0 [F09 §13.1]");
                }
                crate::prim::utf8(&x.fold, r_at)?;
                crate::prim::utf8(&x.path, r_at)?;
                v.push(x);
            }
            let keys: Vec<_> = v.iter().map(|x| (x.root, &x.fold, &x.path, x.id)).collect();
            strictly(&keys, at, "PATHIDX")?;
            Sec::Paths(v)
        }
        0x0082 => {
            let mut v = Vec::new();
            for (row, r_at) in variable_rows(b, at)? {
                let mut r = Reader::with_base(row, r_at);
                let (src, dst, anchor) = (r.u32()?, r.u32()?, r.u32()?);
                let rb = r.vbytes()?;
                let mut rr = Reader::with_base(rb, r.offset() - rb.len());
                let rec = AnchorRec::decode(&mut rr)?;
                rr.finish("an anchor record")?;
                r.finish("an ANCHORS row")?;
                if src == 0 || dst == 0 || anchor == 0 {
                    return err(r_at, "ANCHORS src, dst or anchor 0 [F09 §13.3]");
                }
                v.push(AnchorRow {
                    src,
                    dst,
                    anchor,
                    rec: (rb.to_vec(), Box::new(rec)),
                });
            }
            let keys: Vec<_> = v.iter().map(|x| (x.src, x.dst, x.anchor)).collect();
            strictly(&keys, at, "ANCHORS")?;
            Sec::Anchors(v)
        }
        0x0083 => {
            let (n, mut r) = fixed_table(b, at, 28)?;
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push((r.b16()?, r.u32()?, r.u32()?, r.u32()?));
            }
            strictly(&v, at, "ANCHOR_UID")?;
            Sec::AnchorUid(v)
        }
        0x00C1 => {
            let mut v = Vec::new();
            for (row, r_at) in variable_rows(b, at)? {
                let mut r = Reader::with_base(row, r_at);
                let op = Op::decode(&mut r)?;
                r.finish("a VIOLATIONS row")?;
                if op.tag() != 14 {
                    return err(r_at, "a VIOLATIONS row is not a Violation op [F09 §16.4]");
                }
                v.push(op);
            }
            crate::commit::check_ops(&v, at)?;
            Sec::Violations(v)
        }
        0x00C2 => {
            let mut v: Vec<CkimgEntry> = Vec::new();
            for (row, r_at) in variable_rows(b, at)? {
                let mut r = Reader::with_base(row, r_at);
                let e = decode_ckimg_entry(&mut r)?;
                r.finish("a CKIMG row")?;
                if v.last().is_some_and(|p| p.id >= e.id) {
                    return err(r_at, "CKIMG rows not strictly ascending by id [F09 §16.4]");
                }
                v.push(e);
            }
            Sec::Ckimg(v)
        }
        0x0100 => Sec::Symtab(decode_symtab(b, at)?),
        t => match Table::from_tag(t) {
            Some(table) => Sec::Runtime(Box::new(runtime::decode_section(
                table,
                b,
                at,
                seg_kind_of(kind),
            )?)),
            None => Sec::Unknown(b.to_vec()),
        },
    })
}

fn decode_terms(b: &[u8], at: usize) -> Result<Vec<Term>> {
    let mut r = Reader::with_base(b, at);
    let n = r.u32()? as usize;
    let nb = r.u32()? as usize;
    if nb != n.div_ceil(16) || nb > b.len() / 4 {
        return err(at, "TERMS n_blocks is not ceil(n_terms / 16) [F09 §12.2]");
    }
    let mut boffs = Vec::with_capacity(nb);
    for _ in 0..nb {
        boffs.push(r.u32()? as usize);
    }
    let mut v: Vec<Term> = Vec::with_capacity(n);
    for (k, &boff) in boffs.iter().enumerate() {
        if boff != r.pos() {
            return err(
                r.offset(),
                "TERMS blocks are not contiguous at their block_off [F09 §12.2]",
            );
        }
        for j in 0..16.min(n - 16 * k) {
            let t_at = r.offset();
            let term = if j == 0 {
                let len = usize::from(r.u8()?);
                if !(1..=64).contains(&len) {
                    return err(t_at, "TERMS first term length outside 1-64 [F09 §12.2]");
                }
                r.bytes(len)?.to_vec()
            } else {
                let prev = &v.last().expect("previous term").term;
                let shared = usize::from(r.u8()?);
                let sl = usize::from(r.u8()?);
                if shared > prev.len() || sl == 0 || sl > 64 - shared.min(64) {
                    return err(t_at, "TERMS shared or suffix_len out of range [F09 §12.2]");
                }
                let mut t = prev[..shared].to_vec();
                t.extend_from_slice(r.bytes(sl)?);
                let lcp = prev.iter().zip(&t).take_while(|(a, b)| a == b).count();
                if lcp != shared {
                    return err(
                        t_at,
                        "TERMS shared is not the maximal common prefix [F09 §12.2]",
                    );
                }
                t
            };
            crate::prim::utf8(&term, t_at)?;
            if v.last().is_some_and(|p| p.term >= term) {
                return err(t_at, "TERMS not strictly ascending [F09 §12.2]");
            }
            v.push(Term {
                term,
                post_off: r.uvar64()?,
            });
        }
    }
    r.finish("TERMS")?;
    Ok(v)
}

fn encode_terms(v: &[Term]) -> Vec<u8> {
    let nb = v.len().div_ceil(16);
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(nb);
    for (k, chunk) in v.chunks(16).enumerate() {
        let mut w = Writer::new();
        for (j, t) in chunk.iter().enumerate() {
            if j == 0 {
                w.u8(t.term.len() as u8);
                w.bytes(&t.term);
            } else {
                let prev = &v[16 * k + j - 1].term;
                let shared = prev.iter().zip(&t.term).take_while(|(a, b)| a == b).count();
                w.u8(shared as u8);
                w.u8((t.term.len() - shared) as u8);
                w.bytes(&t.term[shared..]);
            }
            w.uvar(t.post_off);
        }
        blocks.push(w.into_vec());
    }
    let mut w = Writer::new();
    w.u32(v.len() as u32);
    w.u32(nb as u32);
    let mut off = 8 + 4 * nb;
    for bl in &blocks {
        w.u32(off as u32);
        off += bl.len();
    }
    blocks.iter().for_each(|bl| w.bytes(bl));
    w.into_vec()
}

fn decode_post(b: &[u8], at: usize, terms: &[Term]) -> Result<Vec<Vec<Posting>>> {
    let mut r = Reader::with_base(b, at);
    let mut v = Vec::with_capacity(terms.len());
    for t in terms {
        if t.post_off != r.pos() as u64 {
            return err(
                r.offset(),
                "a TERMS post_off is not its list's offset in POST [F09 §12.2]",
            );
        }
        let n_at = r.offset();
        let n = r.count(3)?;
        if n == 0 {
            return err(n_at, "POST n_docs is 0 [F09 §12.3]");
        }
        let mut list: Vec<Posting> = Vec::with_capacity(n);
        let mut last = 0u32;
        for i in 0..n {
            let d_at = r.offset();
            let d = r.uvar32()?;
            if (i > 0 && d == 0) || (i == 0 && d == 0) {
                return err(d_at, "POST ddoc 0 [F09 §12.3]");
            }
            let id = last
                .checked_add(d)
                .map_or_else(|| err(d_at, "POST #N overflows"), Ok)?;
            last = id;
            let m_at = r.offset();
            let fmask = r.u8()?;
            if fmask & 0xF8 != 0 || fmask == 0 {
                return err(m_at, "POST fmask invalid [F09 §12.3]");
            }
            let mut tfs = Vec::new();
            for bit in 0..3 {
                if fmask & (1 << bit) != 0 {
                    let tf_at = r.offset();
                    let tf = r.uvar16()?;
                    if tf == 0 {
                        return err(tf_at, "POST tf 0 [F09 §12.3]");
                    }
                    tfs.push(tf);
                }
            }
            list.push((id, fmask, tfs));
        }
        v.push(list);
    }
    r.finish("POST")?;
    Ok(v)
}

fn encode_post(v: &[Vec<Posting>]) -> Vec<u8> {
    let mut w = Writer::new();
    for list in v {
        w.uvar(list.len() as u64);
        let mut last = 0u32;
        for (id, m, tfs) in list {
            w.uvar(u64::from(id - last));
            last = *id;
            w.u8(*m);
            tfs.iter().for_each(|t| w.uvar(u64::from(*t)));
        }
    }
    w.into_vec()
}

fn decode_fcol(b: &[u8], at: usize) -> Result<Fcol> {
    let mut r = Reader::with_base(b, at);
    let field_sym = r.u32()?;
    let n = r.u32()? as usize;
    let n_vals = r.u32()? as usize;
    let vtype = r.u8()?;
    let form = r.u8()?;
    let ew = r.u8()?;
    r.zeros(1, "FCOL._reserved")?;
    if elem_w(vtype) != Some(ew) || form > 1 || (form == 0 && n_vals != 0) {
        return err(at, "FCOL vtype, elem_w, form or n_vals invalid [F09 §10.2]");
    }
    let bm_len = n.div_ceil(8);
    let bm = r.bytes(bm_len)?.to_vec();
    let pad = (16 + bm_len).next_multiple_of(8) - (16 + bm_len);
    r.zeros(pad, "FCOL absent-bitmap padding")?;
    if !n.is_multiple_of(8) && bm[bm_len - 1] >> (n % 8) != 0 {
        return err(at + 16, "FCOL absent bitmap has bits beyond n [F09 §10.2]");
    }
    let absent = |i: usize| bm[i / 8] >> (i % 8) & 1 != 0;
    let ew = usize::from(ew);
    let mut rows = Vec::with_capacity(n);
    if form == 0 {
        for i in 0..n {
            let e_at = r.offset();
            let e = r.bytes(ew)?;
            if absent(i) {
                if e.iter().any(|&x| x != 0) {
                    return err(
                        e_at,
                        "FCOL element of an absent row is not zero [F09 §10.2]",
                    );
                }
                rows.push(None);
            } else {
                check_promoted(vtype, e, e_at)?;
                rows.push(Some(vec![e.to_vec()]));
            }
        }
    } else {
        let mut off = Vec::with_capacity(n + 1);
        for _ in 0..=n {
            off.push(r.u32()? as usize);
        }
        if off[0] != 0 || off.windows(2).any(|w| w[0] > w[1]) || off[n] != n_vals {
            return err(at, "FCOL set offsets break §10.2");
        }
        let vals_at = r.offset();
        let vals = r.bytes(n_vals * ew)?;
        for i in 0..n {
            let els: Vec<Vec<u8>> = (off[i]..off[i + 1])
                .map(|j| vals[j * ew..(j + 1) * ew].to_vec())
                .collect();
            if absent(i) != els.is_empty() {
                return err(
                    vals_at,
                    "FCOL absent bit disagrees with an empty range [F09 §10.2]",
                );
            }
            for (j, e) in els.iter().enumerate() {
                check_promoted(vtype, e, vals_at)?;
                if j > 0 && promoted_cmp(vtype, &els[j - 1], e) != std::cmp::Ordering::Less {
                    return err(
                        vals_at,
                        "FCOL set elements not unique and ascending [F09 §10.2]",
                    );
                }
            }
            rows.push(if els.is_empty() { None } else { Some(els) });
        }
    }
    r.finish("FCOL")?;
    Ok(Fcol {
        field_sym,
        vtype,
        form,
        rows,
    })
}

fn encode_fcol(c: &Fcol) -> Vec<u8> {
    let n = c.rows.len();
    let ew = usize::from(elem_w(c.vtype).expect("promoted type"));
    let n_vals: usize = if c.form == 1 {
        c.rows.iter().map(|r| r.as_ref().map_or(0, Vec::len)).sum()
    } else {
        0
    };
    let mut w = Writer::new();
    w.u32(c.field_sym);
    w.u32(n as u32);
    w.u32(n_vals as u32);
    w.u8(c.vtype);
    w.u8(c.form);
    w.u8(ew as u8);
    w.u8(0);
    let mut bm = vec![0u8; n.div_ceil(8)];
    for (i, r) in c.rows.iter().enumerate() {
        if r.is_none() {
            bm[i / 8] |= 1 << (i % 8);
        }
    }
    w.bytes(&bm);
    w.zeros(w.len().next_multiple_of(8) - w.len());
    if c.form == 0 {
        for r in &c.rows {
            match r {
                Some(v) => w.bytes(&v[0]),
                None => w.zeros(ew),
            }
        }
    } else {
        let mut o = 0u32;
        w.u32(0);
        for r in &c.rows {
            o += r.as_ref().map_or(0, Vec::len) as u32;
            w.u32(o);
        }
        for r in c.rows.iter().flatten() {
            r.iter().for_each(|e| w.bytes(e));
        }
    }
    w.into_vec()
}

fn decode_fidx(b: &[u8], at: usize) -> Result<Fidx> {
    let mut r = Reader::with_base(b, at);
    let field_sym = r.u32()?;
    let nv = r.u32()? as usize;
    let vtype = r.u8()?;
    let ew = r.u8()?;
    let form = r.u8()?;
    r.zeros(5, "FIDX._reserved")?;
    if elem_w(vtype) != Some(ew) || form > 1 || nv > b.len() / 32 {
        return err(
            at,
            "FIDX vtype, elem_w, form or n_values invalid [F09 §10.3]",
        );
    }
    let ew = usize::from(ew);
    let mut dir = Vec::with_capacity(nv);
    for _ in 0..nv {
        let e_at = r.offset();
        let v: [u8; 16] = r.array()?;
        let count = r.u32()?;
        r.zeros(4, "FIDX directory._reserved")?;
        let off = r.u64()?;
        if v[ew..].iter().any(|&x| x != 0) {
            return err(e_at, "FIDX value padding is not zero [F09 §10.3]");
        }
        check_promoted(vtype, &v[..ew], e_at)?;
        dir.push((v[..ew].to_vec(), count, off));
    }
    for w in dir.windows(2) {
        if promoted_cmp(vtype, &w[0].0, &w[1].0) != std::cmp::Ordering::Less {
            return err(
                at,
                "FIDX directory not strictly ascending in value order [F09 §10.3]",
            );
        }
    }
    let mut pos = (16 + 32 * nv) as u64;
    let mut values = Vec::with_capacity(nv);
    for (i, (v, count, off)) in dir.into_iter().enumerate() {
        let start = pos.next_multiple_of(8);
        if off != start
            || b.get(pos as usize..start as usize)
                .is_none_or(|p| p.iter().any(|&x| x != 0))
        {
            return err(
                at,
                "FIDX body not at its canonical offset, or padding not zero [F09 §10.3]",
            );
        }
        let end = if i + 1 < nv { None } else { Some(b.len()) };
        let body_bytes = &b[start as usize..];
        let (body, used) = if form == 0 {
            let len = bitset_len(body_bytes, at + start as usize)?;
            let s = decode_bitset(&body_bytes[..len], at + start as usize)?;
            if s.card() == 0 || s.card() != u64::from(count) {
                return err(
                    at,
                    "FIDX frozen set empty, or count differs from its cardinality [F09 §10.3]",
                );
            }
            (FidxBody::Frozen(s), len)
        } else {
            if body_bytes.len() < 8 {
                return err(at, "FIDX ± list truncated [F09 §10.3]");
            }
            let np = u32::from_le_bytes(body_bytes[..4].try_into().expect("4")) as usize;
            let nm = u32::from_le_bytes(body_bytes[4..8].try_into().expect("4")) as usize;
            let len = 8 + 4 * (np + nm);
            if len > body_bytes.len() || (np + nm) as u64 != u64::from(count) {
                return err(at, "FIDX ± list length or count invalid [F09 §10.3]");
            }
            (
                FidxBody::Pm(decode_pm(&body_bytes[..len], at + start as usize)?),
                len,
            )
        };
        pos = start + used as u64;
        if end.is_some_and(|e| pos as usize != e) {
            return err(at, "bytes after the last FIDX body [F09 §10.3]");
        }
        values.push((v, body));
    }
    if nv == 0 && pos as usize != b.len() {
        return err(at, "bytes after an empty FIDX directory [F09 §10.3]");
    }
    Ok(Fidx {
        field_sym,
        vtype,
        values,
        form,
    })
}

/// The canonical length of the frozen bitset that begins `b` ([F09 §4.5]).
fn bitset_len(b: &[u8], at: usize) -> Result<usize> {
    if b.len() < 8 {
        return err(at, "a bitset is truncated [F09 §4.5]");
    }
    let n = u32::from_le_bytes(b[..4].try_into().expect("4")) as usize;
    if b.len() < 8 + 16 * n {
        return err(at, "a bitset chunk index is truncated [F09 §4.5]");
    }
    let mut pos = 8 + 16 * n;
    for i in 0..n {
        let card =
            u32::from_le_bytes(b[8 + 16 * i + 4..8 + 16 * i + 8].try_into().expect("4")) as usize;
        pos = pos.next_multiple_of(8) + if card <= 4096 { 2 * card } else { 8192 };
    }
    if pos > b.len() {
        return err(at, "a bitset runs past its section [F09 §4.5]");
    }
    Ok(pos)
}

fn encode_fidx(x: &Fidx) -> Vec<u8> {
    let ew = elem_w(x.vtype).expect("promoted type");
    let bodies: Vec<Vec<u8>> = x
        .values
        .iter()
        .map(|(_, b)| match b {
            FidxBody::Frozen(s) => encode_bitset(s),
            FidxBody::Pm(p) => encode_pm(p),
        })
        .collect();
    let mut w = Writer::new();
    w.u32(x.field_sym);
    w.u32(x.values.len() as u32);
    w.u8(x.vtype);
    w.u8(ew);
    w.u8(x.form);
    w.zeros(5);
    let mut pos = (16 + 32 * x.values.len()) as u64;
    for ((v, b), bytes) in x.values.iter().zip(&bodies) {
        let start = pos.next_multiple_of(8);
        let mut val = v.clone();
        val.resize(16, 0);
        w.bytes(&val);
        w.u32(match b {
            FidxBody::Frozen(s) => s.card() as u32,
            FidxBody::Pm(p) => (p.plus.len() + p.minus.len()) as u32,
        });
        w.u32(0);
        w.u64(start);
        pos = start + bytes.len() as u64;
    }
    for bytes in &bodies {
        w.zeros(w.len().next_multiple_of(8) - w.len());
        w.bytes(bytes);
    }
    w.into_vec()
}

fn decode_symtab(b: &[u8], at: usize) -> Result<Symtab> {
    let mut r = Reader::with_base(b, at);
    let nc = usize::from(r.u16()?);
    r.zeros(2, "SYMTAB._reserved")?;
    let pool_len = r.u32()? as usize;
    let mut ents: Vec<(u8, u32, usize, usize, usize)> = Vec::with_capacity(nc);
    for _ in 0..nc {
        let e_at = r.offset();
        let class = r.u8()?;
        r.zeros(3, "SYMTAB class entry._reserved")?;
        let (first, n, by_id, by_str) = (
            r.u32()?,
            r.u32()? as usize,
            r.u32()? as usize,
            r.u32()? as usize,
        );
        if !(1..=11).contains(&class)
            || first == 0
            || n == 0
            || ents.last().is_some_and(|&(c, ..)| c >= class)
        {
            return err(e_at, "SYMTAB class entry invalid or unsorted [F09 §14.2]");
        }
        ents.push((class, first, n, by_id, by_str));
    }
    let arrays_start = 8 + 20 * nc;
    let total_n: usize = ents.iter().map(|e| e.2).sum();
    let pool_start = arrays_start + 8 * total_n;
    if pool_start + pool_len != b.len() {
        return err(
            at,
            "SYMTAB length breaks header + entries + arrays + pool [F09 §14.2]",
        );
    }
    let pool = &b[pool_start..];
    let mut pr = Reader::with_base(pool, at + pool_start);
    let mut expect_arr = arrays_start;
    let mut classes = Vec::with_capacity(nc);
    for (class, first, n, by_id, by_str) in ents {
        if by_id != expect_arr || by_str != expect_arr + 4 * n {
            return err(at, "SYMTAB arrays not in entry order [F09 §14.2]");
        }
        let mut ar = Reader::with_base(&b[by_id..by_id + 8 * n], at + by_id);
        let mut strings = Vec::with_capacity(n);
        for _ in 0..n {
            let o = ar.u32()? as usize;
            if o != pr.pos() {
                return err(
                    ar.offset(),
                    "SYMTAB by_id is not the pool offset in (class, id) order [F09 §14.2]",
                );
            }
            let s_at = pr.offset();
            let s = pr.vstr()?;
            if s.is_empty() {
                return err(s_at, "SYMTAB holds the empty string [F09 §14.2]");
            }
            strings.push(s.to_owned());
        }
        let mut want: Vec<u32> = (0..n as u32).map(|j| first + j).collect();
        want.sort_by(|a, b| {
            strings[(a - first) as usize]
                .as_bytes()
                .cmp(strings[(b - first) as usize].as_bytes())
        });
        for w in want.windows(2) {
            if strings[(w[0] - first) as usize] == strings[(w[1] - first) as usize] {
                return err(at, "SYMTAB holds one string twice in a class [F09 §14.2]");
            }
        }
        for want_id in want {
            if ar.u32()? != want_id {
                return err(
                    ar.offset(),
                    "SYMTAB by_str is not the ids sorted by string [F09 §14.2]",
                );
            }
        }
        expect_arr += 8 * n;
        classes.push((class, first, strings));
    }
    pr.finish("the SYMTAB pool")?;
    Ok(Symtab { classes })
}

fn encode_symtab(s: &Symtab) -> Vec<u8> {
    let nc = s.classes.len();
    let mut pool = Writer::new();
    let mut arrays = Writer::new();
    let mut ents = Writer::new();
    let mut arr_off = 8 + 20 * nc;
    for (class, first, strings) in &s.classes {
        let n = strings.len();
        ents.u8(*class);
        ents.zeros(3);
        ents.u32(*first);
        ents.u32(n as u32);
        ents.u32(arr_off as u32);
        ents.u32((arr_off + 4 * n) as u32);
        for st in strings {
            arrays.u32(pool.len() as u32);
            pool.vstr(st);
        }
        let mut ids: Vec<usize> = (0..n).collect();
        ids.sort_by(|a, b| strings[*a].as_bytes().cmp(strings[*b].as_bytes()));
        ids.iter().for_each(|j| arrays.u32(first + *j as u32));
        arr_off += 8 * n;
    }
    let mut w = Writer::new();
    w.u16(nc as u16);
    w.u16(0);
    w.u32(pool.len() as u32);
    w.bytes(ents.as_slice());
    w.bytes(arrays.as_slice());
    w.bytes(pool.as_slice());
    w.into_vec()
}

/// Encodes one section.
pub fn encode_sec(s: &Sec) -> Vec<u8> {
    let mut w = Writer::new();
    match s {
        Sec::Ids(v) | Sec::U32s(v) => v.iter().for_each(|x| w.u32(*x)),
        Sec::Node(v) => v.iter().for_each(|x| x.encode(&mut w)),
        Sec::Creator(v) => v.iter().for_each(|x| x.encode(&mut w)),
        Sec::U8s(v) => w.bytes(v),
        Sec::U64s(v) => v.iter().for_each(|x| w.u64(*x)),
        Sec::Uid(v) => {
            w.u32(v.len() as u32);
            w.u32(20);
            for (u, id) in v {
                w.bytes(u);
                w.u32(*id);
            }
        }
        Sec::Bitset(b) => return encode_bitset(b),
        Sec::Pm(p) => return encode_pm(p),
        Sec::Titles(v) => v.iter().for_each(|t| w.vstr(t)),
        Sec::Fields(v) => {
            for fb in v {
                let mut e = Writer::new();
                value::encode_field_block(fb, &mut e);
                w.vbytes(e.as_slice());
            }
        }
        Sec::BlobTab(v) => {
            w.u32(v.len() as u32);
            w.u32(36);
            for x in v {
                w.bytes(&x.hash);
                w.u32(x.file);
                w.u64(x.off);
                w.u32(x.len);
                w.u32(x.raw_len);
            }
        }
        Sec::EdgeProps(v) => {
            w.u32(v.len() as u32);
            w.u32(40);
            for (e, f, p) in v {
                w.u32(*e);
                w.u8(*f);
                w.zeros(3);
                w.bytes(p);
            }
        }
        Sec::Tomb(v) => {
            w.u32(v.len() as u32);
            w.u32(16);
            for (a, b, c, d) in v {
                w.u32(*a);
                w.u32(*b);
                w.u32(*c);
                w.u32(*d);
            }
        }
        Sec::Schema(v) => {
            return encode_variable(&v.iter().map(|(b, _)| b.clone()).collect::<Vec<_>>());
        }
        Sec::Bmdir(v) => {
            w.u32(v.len() as u32);
            w.u32(4);
            v.iter().for_each(|k| w.bytes(k));
        }
        Sec::Terms(v) => return encode_terms(v),
        Sec::Post(v) => return encode_post(v),
        Sec::Doclen(v) => v.iter().for_each(|d| d.iter().for_each(|x| w.u16(*x))),
        Sec::FtsStat(v) => {
            for (d, t) in v {
                w.u32(*d);
                w.u32(0);
                w.u64(*t);
            }
        }
        Sec::Stats(st) => {
            w.u32(st.edges.len() as u32);
            w.u32(st.fields.len() as u32);
            for (k, d, mx, es, h) in &st.edges {
                w.u8(*k);
                w.u8(*d);
                w.u16(0);
                w.u32(*mx);
                w.u64(*es);
                h.iter().for_each(|x| w.u32(*x));
            }
            for (k, f, p, d) in &st.fields {
                w.u8(*k);
                w.zeros(3);
                w.u32(*f);
                w.u32(*p);
                w.u32(*d);
            }
        }
        Sec::Fpromo(v) => {
            w.u32(v.len() as u32);
            w.u32(12);
            for f in v {
                w.u32(f.field_sym);
                w.u16(f.slot);
                w.u8(f.vtype);
                w.u8(f.index);
                w.u8(f.form);
                w.zeros(3);
            }
        }
        Sec::Fcol(c) => return encode_fcol(c),
        Sec::Fidx(x) => return encode_fidx(x),
        Sec::Paths(v) => {
            return encode_variable(
                &v.iter()
                    .map(|x| {
                        let mut r = Writer::new();
                        r.u16(x.root);
                        r.u32(x.id);
                        r.vbytes(&x.fold);
                        r.vbytes(&x.path);
                        r.into_vec()
                    })
                    .collect::<Vec<_>>(),
            );
        }
        Sec::Anchors(v) => {
            return encode_variable(
                &v.iter()
                    .map(|x| {
                        let mut r = Writer::new();
                        r.u32(x.src);
                        r.u32(x.dst);
                        r.u32(x.anchor);
                        r.vbytes(&x.rec.0);
                        r.into_vec()
                    })
                    .collect::<Vec<_>>(),
            );
        }
        Sec::AnchorUid(v) => {
            w.u32(v.len() as u32);
            w.u32(28);
            for (u, s, d, a) in v {
                w.bytes(u);
                w.u32(*s);
                w.u32(*d);
                w.u32(*a);
            }
        }
        Sec::Violations(v) => {
            return encode_variable(
                &v.iter()
                    .map(|o| {
                        let mut r = Writer::new();
                        o.encode(&mut r);
                        r.into_vec()
                    })
                    .collect::<Vec<_>>(),
            );
        }
        Sec::Ckimg(v) => {
            return encode_variable(
                &v.iter()
                    .map(|e| {
                        let mut r = Writer::new();
                        encode_ckimg_entry(e, &mut r);
                        r.into_vec()
                    })
                    .collect::<Vec<_>>(),
            );
        }
        Sec::Symtab(s) => return encode_symtab(s),
        Sec::Runtime(s) => return runtime::encode_section(s),
        Sec::Unknown(b) => return b.clone(),
    }
    w.into_vec()
}

/// The core edge kinds' `props` ([F08 §9.6]): 0 none, 1 pinned, 2 flagged, 3 anchor.
fn core_edge_props(k: u8) -> Option<u8> {
    Some(match k {
        2 | 3 => 2,
        11..=13 | 21 => 1,
        25 => 3,
        1 | 4..=10 | 14..=20 | 22..=24 => 0,
        _ => return None,
    })
}

fn valid_edge_kind(k: u8) -> bool {
    (1..=25).contains(&k) || (64..=254).contains(&k)
}

/// Decodes a graph segment (`seg-base`, `seg-delta`, `seg-branch`, `cs`) with every check one file allows.
pub fn decode_segment(b: &[u8]) -> Result<Segment> {
    let c = decode_container(b)?;
    let kind = c.hdr.seg_kind;
    if ![3, 4, 5, 9].contains(&kind) {
        return err(
            6,
            format!("seg_kind {kind} is not a graph segment [F09 §1]"),
        );
    }
    let mut sections: Vec<(SecEnt, Sec)> = Vec::with_capacity(c.entries.len());
    for (e, (s, at)) in c.entries.iter().zip(&c.sections) {
        let pl = placement(e.tag, kind);
        let dopt = e.flags & 1 != 0;
        match pl {
            None if !dopt => {
                return err(
                    *at,
                    format!(
                        "unknown tag 0x{:04X} without derived-optional [F09 §3.1]",
                        e.tag
                    ),
                );
            }
            Some(Pl::F) => {
                return err(
                    *at,
                    format!(
                        "tag 0x{:04X} is forbidden in seg_kind {kind} [F09 §3.1]",
                        e.tag
                    ),
                );
            }
            Some(_) if dopt != must_be_derived_optional(e.tag) => {
                return err(
                    *at,
                    format!("tag 0x{:04X} derived-optional flag breaks §3.3", e.tag),
                );
            }
            _ => {}
        }
        let sec = if pl.is_none() {
            Sec::Unknown(s.to_vec())
        } else if e.tag == 0x0051 {
            let terms = sections.iter().find_map(|(e2, s2)| match s2 {
                Sec::Terms(t) if e2.tag == 0x0050 => Some(t.clone()),
                _ => None,
            });
            let Some(terms) = terms else {
                return err(*at, "POST without TERMS [F09 §12]");
            };
            Sec::Post(decode_post(s, *at, &terms)?)
        } else {
            decode_sec(e.tag, s, *at, kind)?
        };
        sections.push((*e, sec));
    }
    let offs: BTreeMap<u16, usize> = c
        .entries
        .iter()
        .zip(&c.sections)
        .map(|(e, (_, at))| (e.tag, *at))
        .collect();
    let seg = Segment {
        hdr: c.hdr,
        sections,
    };
    check_segment(&seg, &offs)?;
    Ok(seg)
}

/// Re-encodes a graph segment.
pub fn encode_segment(s: &Segment) -> Vec<u8> {
    let secs: Vec<(SecEnt, Vec<u8>)> = s
        .sections
        .iter()
        .map(|(e, x)| (*e, encode_sec(x)))
        .collect();
    encode_container(&s.hdr, &secs)
}

impl Segment {
    fn get(&self, tag: u16) -> Option<&Sec> {
        self.sections
            .iter()
            .find(|(e, _)| e.tag == tag)
            .map(|(_, s)| s)
    }

    fn u32s(&self, tag: u16) -> &[u32] {
        match self.get(tag) {
            Some(Sec::U32s(v)) | Some(Sec::Ids(v)) => v,
            _ => &[],
        }
    }

    fn u8s(&self, tag: u16) -> &[u8] {
        match self.get(tag) {
            Some(Sec::U8s(v)) => v,
            _ => &[],
        }
    }
}

/// The `count` a section's class gives ([F09 §4]).
fn class_count(tag: u16, s: &Sec) -> Option<u64> {
    if tag == 0xFFFF {
        return None;
    }
    Some(match s {
        Sec::Ids(v) | Sec::U32s(v) => v.len() as u64,
        Sec::Node(v) => v.len() as u64,
        Sec::Creator(v) => v.len() as u64,
        Sec::U8s(v) => v.len() as u64,
        Sec::U64s(v) => v.len() as u64,
        Sec::Uid(v) => v.len() as u64,
        Sec::Bitset(b) => b.card(),
        Sec::Pm(p) => (p.plus.len() + p.minus.len()) as u64,
        Sec::Titles(v) => v.len() as u64,
        Sec::Fields(v) => v.len() as u64,
        Sec::BlobTab(v) => v.len() as u64,
        Sec::EdgeProps(v) => v.len() as u64,
        Sec::Tomb(v) => v.len() as u64,
        Sec::Schema(v) => v.len() as u64,
        Sec::Bmdir(v) => v.len() as u64,
        Sec::Terms(v) => v.len() as u64,
        Sec::Post(v) => v.len() as u64,
        Sec::Doclen(v) => v.len() as u64,
        Sec::FtsStat(_) => 3,
        Sec::Stats(st) => (st.edges.len() + st.fields.len()) as u64,
        Sec::Fpromo(v) => v.len() as u64,
        Sec::Fcol(c) => c.rows.len() as u64,
        Sec::Fidx(x) => x.values.len() as u64,
        Sec::Paths(v) => v.len() as u64,
        Sec::Anchors(v) => v.len() as u64,
        Sec::AnchorUid(v) => v.len() as u64,
        Sec::Violations(v) => v.len() as u64,
        Sec::Ckimg(v) => v.len() as u64,
        Sec::Symtab(s) => s.classes.iter().map(|c| c.2.len() as u64).sum(),
        Sec::Runtime(r) => u64::from(r.hdr.n_rows),
        Sec::Unknown(_) => return None,
    })
}

/// The cross-section checks of [F09 §17.1] V-7 (required sections) and V-11, and the per-kind rules of §5–§16.
fn check_segment(s: &Segment, offs: &BTreeMap<u16, usize>) -> Result<()> {
    let kind = s.hdr.seg_kind;
    // A failure names the offset of the section it is about; 0 (the `SegHdr`) for an absent one.
    let at = |tag: u16| offs.get(&tag).copied().unwrap_or(0);
    let fail = |tag: u16, m: String| err(at(tag), format!("seg_kind {kind}: {m}"));
    for (e, x) in &s.sections {
        if let Some(c) = class_count(e.tag, x)
            && u64::from(e.count) != c
        {
            return fail(
                e.tag,
                format!(
                    "section 0x{:04X} count {} differs from its content {c} [F09 §2.2]",
                    e.tag, e.count
                ),
            );
        }
    }
    let tags: BTreeSet<u16> = s.sections.iter().map(|(e, _)| e.tag).collect();
    for t in (0x0001u16..=0x0060)
        .chain(0x0080..=0x0084)
        .chain([0x00C0, 0x0100, 0x0101, 0x0102])
        .chain(0x0201..=0x021A)
    {
        if placement(t, kind) == Some(Pl::R) && !tags.contains(&t) {
            return fail(
                0,
                format!("required section 0x{t:04X} is absent [F09 §17.1 V-7]"),
            );
        }
    }
    let upper = kind != 3;
    let rows: Vec<u32> = if upper {
        s.u32s(0x0001).to_vec()
    } else {
        (1..=s.hdr.n_rows).collect()
    };
    let n = rows.len();
    if upper && s.hdr.n_rows as usize != n {
        return fail(
            0x0001,
            "n_rows differs from the IDS length [F09 §2.3]".into(),
        );
    }
    let row_set: BTreeSet<u32> = rows.iter().copied().collect();
    let Some(Sec::Node(nodes)) = s.get(0x0002) else {
        unreachable!("NODE is required")
    };
    let col_len = |tag: u16, len: usize| -> Result<()> {
        if len != n {
            return err(
                at(tag),
                format!("column 0x{tag:04X} has {len} elements, not {n} [F09 §17.1 V-11]"),
            );
        }
        Ok(())
    };
    col_len(0x0002, nodes.len())?;
    if let Some(Sec::Creator(v)) = s.get(0x0003) {
        col_len(0x0003, v.len())?;
    }
    for t in [0x0004, 0x0005, 0x0006] {
        col_len(t, s.u32s(t).len())?;
    }
    if kind == 9 {
        col_len(
            0x00C0,
            match s.get(0x00C0) {
                Some(Sec::U64s(v)) => v.len(),
                _ => 0,
            },
        )?;
        for h in nodes.iter().filter(|h| h.kind != 0) {
            if h.rev_seq != 0 || h.updated_tx != 0 || h.last_op_lsn != u64::MAX {
                return fail(
                    0x0002,
                    "a changeset NODE row lacks the values fixed at append [F09 §16.4]".into(),
                );
            }
        }
    }
    let mut next_title = 0u64;
    let mut next_field = 0u64;
    let titles: &[String] = match s.get(0x0010) {
        Some(Sec::Titles(v)) => v,
        _ => &[],
    };
    let fields: &[Vec<FieldEntry>] = match s.get(0x0011) {
        Some(Sec::Fields(v)) => v,
        _ => &[],
    };
    let (mut ti, mut fi) = (0usize, 0usize);
    let blobtab_n = match s.get(0x0012) {
        Some(Sec::BlobTab(v)) => v.len(),
        _ => 0,
    };
    let deleted: BTreeSet<u32> = rows
        .iter()
        .zip(nodes)
        .filter(|(_, h)| h.flags & 1 != 0 && h.kind != 0)
        .map(|(r, _)| *r)
        .collect();
    let present: BTreeSet<u32> = rows
        .iter()
        .zip(nodes)
        .filter(|(_, h)| h.kind != 0)
        .map(|(r, _)| *r)
        .collect();
    for h in nodes {
        // [F09 §5.2]: an absent row is all zero (NodeHdr::decode checks it), so its offsets are 0, not NONE32, and it
        // stores no title, field block or body.
        if h.kind == 0 {
            continue;
        }
        if h.title_off != NONE32 {
            if u64::from(h.title_off) != next_title || ti >= titles.len() {
                return fail(
                    0x0010,
                    "a title_off is not the next TITLE_BLOB entry in row order [F09 §6.1]".into(),
                );
            }
            let mut w = Writer::new();
            w.vstr(&titles[ti]);
            next_title += w.len() as u64;
            ti += 1;
        }
        if h.fields_off != NONE32 {
            if u64::from(h.fields_off) != next_field || fi >= fields.len() {
                return fail(
                    0x0011,
                    "a fields_off is not the next FIELDS_BLOB entry in row order [F09 §6.2]".into(),
                );
            }
            let mut e = Writer::new();
            value::encode_field_block(&fields[fi], &mut e);
            let mut w = Writer::new();
            w.vbytes(e.as_slice());
            next_field += w.len() as u64;
            fi += 1;
        }
        if h.body_ref as usize > blobtab_n {
            return fail(
                0x0002,
                "a body_ref is beyond BLOBTAB [F09 §17.1 V-11]".into(),
            );
        }
    }
    if ti != titles.len() || fi != fields.len() {
        return fail(
            0x0010,
            "TITLE_BLOB or FIELDS_BLOB holds entries no row names [F09 §6]".into(),
        );
    }
    let (out_off, out_dst, out_kind) = (s.u32s(0x0020), s.u32s(0x0021), s.u8s(0x0022));
    let (in_off, in_src, in_kind) = (s.u32s(0x0023), s.u32s(0x0024), s.u8s(0x0025));
    for (off, other, kinds, what, tag) in [
        (out_off, out_dst, out_kind, "out", 0x0020u16),
        (in_off, in_src, in_kind, "in", 0x0023),
    ] {
        if off.len() != n + 1 || off[0] != 0 || off.windows(2).any(|w| w[0] > w[1]) {
            return fail(tag, format!("{what} offsets break §7.1"));
        }
        let e = off[n] as usize;
        if other.len() != e || kinds.len() != e {
            return fail(tag, format!("{what} arrays are not e long [F09 §7.1]"));
        }
        for i in 0..n {
            let (a, b) = (off[i] as usize, off[i + 1] as usize);
            let list: Vec<(u8, u32)> = (a..b).map(|j| (kinds[j], other[j])).collect();
            if list.windows(2).any(|w| w[0] >= w[1]) {
                return fail(
                    tag,
                    format!("an {what}-list is not strictly ascending by (kind, node) [F09 §7.1]"),
                );
            }
            if list
                .iter()
                .any(|&(k, x)| !valid_edge_kind(k) || x == 0 || (what == "out" && k == 1))
            {
                return fail(
                    tag,
                    format!("an {what}-list entry has an invalid kind or node [F09 §7.1]"),
                );
            }
        }
    }
    let schema_edge_props: HashMap<u8, u8> = match s.get(0x0032) {
        Some(Sec::Schema(v)) => v
            .iter()
            .filter_map(|(_, it)| match &it.body {
                ItemBody::EdgeKind(e) => Some((e.edge_id, e.props)),
                _ => None,
            })
            .collect(),
        _ => HashMap::new(),
    };
    if let Some(Sec::EdgeProps(v)) = s.get(0x0026) {
        for (edge, pflags, _) in v {
            let Some(&k) = out_kind.get(*edge as usize) else {
                return fail(0x0026, "EDGE_PROPS edge beyond e_out [F09 §7.2]".into());
            };
            let props = core_edge_props(k)
                .or_else(|| schema_edge_props.get(&k).copied())
                .unwrap_or(0);
            let ok = match props {
                1 => *pflags == 1,
                2 => *pflags == 2,
                _ => false,
            };
            if !ok {
                return fail(
                    0x0026,
                    format!("EDGE_PROPS pflags {pflags} not admitted by edge kind {k} [F08 §10.2]"),
                );
            }
        }
    }
    if let Some(Sec::Uid(v)) = s.get(0x0007) {
        let ids: BTreeSet<u32> = v.iter().map(|x| x.1).collect();
        if ids != present || ids.len() != v.len() {
            return fail(
                0x0007,
                "UID does not hold exactly one entry per present row [F09 §5.5]".into(),
            );
        }
    }
    if let Some(Sec::Tomb(v)) = s.get(0x0030) {
        let ids: BTreeSet<u32> = v.iter().map(|x| x.0).collect();
        if ids != deleted {
            return fail(
                0x0030,
                "TOMB entries differ from the deleted rows [F09 §8.1]".into(),
            );
        }
    }
    if let Some(Sec::Anchors(v)) = s.get(0x0082) {
        if v.iter().any(|a| !row_set.contains(&a.src)) {
            return fail(
                0x0082,
                "an ANCHORS row's src is not a row of the segment [F09 §17.1 V-11]".into(),
            );
        }
        let want: Vec<([u8; 16], u32, u32, u32)> = {
            let mut w: Vec<_> = v
                .iter()
                .map(|a| (a.rec.1.uid, a.src, a.dst, a.anchor))
                .collect();
            w.sort();
            w
        };
        match s.get(0x0083) {
            Some(Sec::AnchorUid(u)) if *u == want => {}
            _ => {
                return fail(
                    0x0083,
                    "ANCHOR_UID is not one entry per ANCHORS row [F09 §13.4]".into(),
                );
            }
        }
    }
    for t in [0x0080u16, 0x0081] {
        if let Some(Sec::Paths(v)) = s.get(t)
            && v.iter().any(|p| !row_set.contains(&p.id))
        {
            return fail(
                t,
                "a PATHIDX/ALIASIDX id is not a row of the segment [F09 §17.1 V-11]".into(),
            );
        }
    }
    if let Some(Sec::Runtime(c)) = s.get(0x0031)
        && c.rows
            .iter()
            .any(|r| r.u("n") != 0 && !row_set.contains(&(r.u("n") as u32)))
    {
        return fail(
            0x0031,
            "a CONFLICTS row names a node that is not a row of the segment [F09 §8.2]".into(),
        );
    }
    if let Some(Sec::Runtime(g)) = s.get(0x0084)
        && g.rows.iter().any(|r| !row_set.contains(&(r.u("n") as u32)))
    {
        return fail(
            0x0084,
            "a GLOBIDX row names a node that is not a row of the segment [F09 §13.5]".into(),
        );
    }
    if kind == 5 {
        match s.get(0x0008) {
            Some(Sec::Bitset(b)) if b.members() == rows => {}
            _ => return fail(0x0008, "TOUCH differs from IDS [F09 §16.3]".into()),
        }
    }
    let bm_tags: Vec<u16> = tags.iter().copied().filter(|t| *t >= 0x8000).collect();
    match s.get(0x0040) {
        Some(Sec::Bmdir(d)) => {
            let want: Vec<u16> = (0..d.len() as u16).map(|i| 0x8000 + i).collect();
            if bm_tags != want || (upper && d.is_empty()) {
                return fail(0x0040, "BM.<i> tags differ from the BMDIR entries, or an empty BMDIR in an upper segment [F09 §9.2]".into());
            }
        }
        _ if !bm_tags.is_empty() => {
            return fail(
                bm_tags[0],
                "BM.<i> sections without BMDIR [F09 §9.2]".into(),
            );
        }
        _ => {}
    }
    let fpromo: &[Fpromo] = match s.get(0x0061) {
        Some(Sec::Fpromo(v)) => v,
        _ => &[],
    };
    let fcol_tags: Vec<u16> = tags
        .iter()
        .copied()
        .filter(|t| (0x4000..0x5000).contains(t))
        .collect();
    let fidx_tags: Vec<u16> = tags
        .iter()
        .copied()
        .filter(|t| (0x5000..0x6000).contains(t))
        .collect();
    if s.get(0x0061).is_some() && fcol_tags.is_empty() && fidx_tags.is_empty() {
        return fail(0x0061, "FPROMO without any FCOL or FIDX [F09 §10.1]".into());
    }
    let want_fcol: Vec<u16> = fpromo
        .iter()
        .filter(|f| f.index == 1 || (f.index == 2 && f.form == 0))
        .map(|f| 0x4000 + f.slot)
        .collect();
    if fcol_tags != want_fcol {
        return fail(
            0x0061,
            "FCOL sections differ from what FPROMO implies [F09 §10.1]".into(),
        );
    }
    for t in &fidx_tags {
        let slot = t - 0x5000;
        if fpromo.get(usize::from(slot)).is_none_or(|f| f.index != 2) {
            return fail(
                *t,
                "an FIDX section for a slot FPROMO does not index as a bitmap [F09 §10.1]".into(),
            );
        }
    }
    if !upper {
        let want_fidx: Vec<u16> = fpromo
            .iter()
            .filter(|f| f.index == 2)
            .map(|f| 0x5000 + f.slot)
            .collect();
        if fidx_tags != want_fidx {
            return fail(
                0x0061,
                "a base lacks an FIDX its FPROMO implies [F09 §10.1]".into(),
            );
        }
    }
    for (e, x) in &s.sections {
        match x {
            Sec::Fcol(c) => {
                let f = fpromo[usize::from(e.tag - 0x4000)];
                if c.field_sym != f.field_sym
                    || c.vtype != f.vtype
                    || c.form != f.form
                    || c.rows.len() != n
                {
                    return fail(
                        e.tag,
                        "an FCOL header differs from its FPROMO row or row count [F09 §10.2]"
                            .into(),
                    );
                }
            }
            Sec::Fidx(x) => {
                let f = fpromo[usize::from(e.tag - 0x5000)];
                if x.field_sym != f.field_sym || x.vtype != f.vtype || x.form != u8::from(upper) {
                    return fail(
                        e.tag,
                        "an FIDX header differs from its FPROMO row or segment kind [F09 §10.3]"
                            .into(),
                    );
                }
            }
            _ => {}
        }
    }
    let fts = tags.contains(&0x0050);
    if (s.hdr.tok_ver == 1) != fts || tags.contains(&0x0051) != fts {
        return fail(
            0x0050,
            "TERMS and POST present exactly when tok_ver is 1 [F09 §12]".into(),
        );
    }
    if tags.contains(&0x0052) && !fts || tags.contains(&0x0053) && !fts {
        return fail(0x0052, "DOCLEN or FTSSTAT without TERMS [F09 §12.4]".into());
    }
    if kind != 9 && fts && tags.contains(&0x0052) != tags.contains(&0x0053) {
        return fail(
            0x0052,
            "DOCLEN and FTSSTAT are kept or dropped together (HOLE(F09-doclen)) [F09 §12.4]".into(),
        );
    }
    if let Some(Sec::Doclen(v)) = s.get(0x0052) {
        col_len(0x0052, v.len())?;
    }
    if let Some(Sec::Post(lists)) = s.get(0x0051)
        && lists.iter().flatten().any(|(id, _, _)| {
            !row_set.contains(id) || deleted.contains(id) || !present.contains(id)
        })
    {
        return fail(
            0x0051,
            "a posting names a row that is not a live row of the segment [F09 §12.3]".into(),
        );
    }
    for t in [0x00C1u16, 0x00C2] {
        let empty = match s.get(t) {
            Some(Sec::Violations(v)) => v.is_empty(),
            Some(Sec::Ckimg(v)) => v.is_empty(),
            _ => false,
        };
        if empty {
            return fail(
                t,
                format!("section 0x{t:04X} is present with no row [F09 §16.4]"),
            );
        }
    }
    if let (Some(Sec::Schema(items)), Some(Sec::Symtab(st))) = (s.get(0x0032), s.get(0x0100)) {
        check_schema_order(items, st, at(0x0032))?;
    }
    Ok(())
}

/// [F08 §8.5] item key order of `SCHEMA` (class, then the key's name strings), using the segment's own `SYMTAB` for
/// class `name`; a pair whose names do not all resolve there is not compared (its symbols live in older layers).
fn check_schema_order(items: &[(Vec<u8>, Item)], st: &Symtab, at: usize) -> Result<()> {
    let names: BTreeMap<u32, &str> = st
        .classes
        .iter()
        .filter(|c| c.0 == 10)
        .flat_map(|(_, f, v)| {
            v.iter()
                .enumerate()
                .map(move |(j, s)| (f + j as u32, s.as_str()))
        })
        .collect();
    let key = |it: &Item| -> Option<(u8, Vec<Vec<u8>>)> {
        let nm = |id: u32| -> Option<Vec<u8>> {
            if id == 0 {
                Some(b"*".to_vec())
            } else {
                names.get(&id).map(|s| s.as_bytes().to_vec())
            }
        };
        let parts = match &it.body {
            ItemBody::Kind { name, .. } | ItemBody::Query { name, .. } => vec![nm(*name)?],
            ItemBody::Field { kind, name, .. } => vec![nm(*kind)?, nm(*name)?],
            ItemBody::EnumValue {
                kind, field, name, ..
            } => vec![nm(*kind)?, nm(*field)?, nm(*name)?],
            ItemBody::EdgeKind(e) => vec![nm(e.name)?],
        };
        Some((it.class(), parts))
    };
    let keys: Vec<Option<(u8, Vec<Vec<u8>>)>> = items.iter().map(|(_, it)| key(it)).collect();
    for w in keys.windows(2) {
        if let (Some(a), Some(b)) = (&w[0], &w[1])
            && a >= b
        {
            return err(
                at,
                "SCHEMA rows not strictly ascending in item key order [F09 §8.3]",
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn ent(tag: u16, count: u32) -> SecEnt {
        SecEnt {
            tag,
            flags: 0,
            count,
        }
    }

    fn rt_empty(t: Table, form: u8) -> Sec {
        // [F11 §8]: an empty IDEM still has the minimum capacity of 16 zero slots.
        let rows: Vec<runtime::Row> = if t == Table::Idem {
            let zero = [0u8; 72];
            let row =
                runtime::decode_image(Table::Idem, &mut Reader::new(&zero), runtime::Place::Upsert)
                    .unwrap();
            vec![row; 16]
        } else {
            vec![]
        };
        Sec::Runtime(Box::new(runtime::Section {
            table: t,
            hdr: runtime::RtHdr {
                n_rows: rows.len() as u32,
                row_size: t.row_size() as u16,
                form,
                aux: if t == Table::Alloc { 1 } else { 0 },
                index_len: 0,
                heap_len: 0,
            },
            rows,
            index: vec![],
        }))
    }

    /// A base segment with one task row (#1), a title, a field block, a body in `BLOBTAB`, one `blocks` edge to a
    /// deleted #2 and every required section ([F09 §3.1]).
    pub(crate) fn small_base() -> Segment {
        let task = NodeHdr {
            kind: 1,
            priority: 2,
            criticality: 2,
            authority: 4,
            rev_seq: 3,
            created_tx: 1,
            updated_tx: 3,
            last_op_lsn: 500,
            title_off: 0,
            fields_off: 0,
            body_ref: 1,
            ..NodeHdr::default()
        };
        let tomb = NodeHdr {
            kind: 1,
            status: 4,
            flags: 1,
            priority: 2,
            criticality: 2,
            authority: 4,
            rev_seq: 4,
            created_tx: 2,
            updated_tx: 4,
            last_op_lsn: 600,
            title_off: 6,
            fields_off: NONE32,
            ..NodeHdr::default()
        };
        let fb = vec![FieldEntry {
            field_sym: 1,
            value: value::Value::Int(3),
        }];
        let secs: Vec<(SecEnt, Sec)> = vec![
            (ent(0x0002, 2), Sec::Node(vec![task, tomb])),
            (
                ent(0x0003, 2),
                Sec::Creator(vec![Creator { actor: 1, role: 0 }; 2]),
            ),
            (ent(0x0004, 2), Sec::U32s(vec![1, 0])),
            (ent(0x0005, 2), Sec::U32s(vec![0, 0])),
            (ent(0x0006, 2), Sec::U32s(vec![0, 0])),
            (ent(0x0007, 2), Sec::Uid(vec![([1; 16], 1), ([2; 16], 2)])),
            (
                ent(0x0010, 2),
                Sec::Titles(vec!["first".into(), "gone".into()]),
            ),
            (ent(0x0011, 1), Sec::Fields(vec![fb])),
            (
                ent(0x0012, 1),
                Sec::BlobTab(vec![BlobRef {
                    hash: [9; 16],
                    file: 2,
                    off: 120,
                    len: 7,
                    raw_len: 6,
                }]),
            ),
            (ent(0x0020, 3), Sec::U32s(vec![0, 1, 1])),
            (ent(0x0021, 1), Sec::U32s(vec![2])),
            (ent(0x0022, 1), Sec::U8s(vec![2])),
            (ent(0x0023, 3), Sec::U32s(vec![0, 0, 1])),
            (ent(0x0024, 1), Sec::U32s(vec![1])),
            (ent(0x0025, 1), Sec::U8s(vec![2])),
            (ent(0x0026, 0), Sec::EdgeProps(vec![])),
            (ent(0x0030, 1), Sec::Tomb(vec![(2, 4, 0, 0)])),
            (ent(0x0031, 0), rt_empty(Table::Conflicts, 1)),
            (ent(0x0032, 0), Sec::Schema(vec![])),
            (ent(0x0040, 1), Sec::Bmdir(vec![[1, 1, 0, 0]])),
            (
                ent(0x0060, 0),
                Sec::Stats(Stats {
                    edges: vec![],
                    fields: vec![],
                }),
            ),
            (ent(0x0080, 0), Sec::Paths(vec![])),
            (ent(0x0081, 0), Sec::Paths(vec![])),
            (ent(0x0082, 0), Sec::Anchors(vec![])),
            (ent(0x0083, 0), Sec::AnchorUid(vec![])),
            (ent(0x0084, 0), rt_empty(Table::GlobIdx, 1)),
            (
                ent(0x0100, 1),
                Sec::Symtab(Symtab {
                    classes: vec![(10, 1, vec!["estimate".into()])],
                }),
            ),
            (ent(0x0101, 0), rt_empty(Table::SchemaIds, 1)),
            (ent(0x0102, 0), rt_empty(Table::Files, 1)),
        ];
        let mut secs = secs;
        for t in runtime::TABLES {
            if (0x0201..=0x021A).contains(&t.tag()) && !t.derived_optional() {
                secs.push((
                    ent(t.tag(), if t == Table::Idem { 16 } else { 0 }),
                    rt_empty(t, 1),
                ));
            }
        }
        secs.push((ent(0x8000, 2), Sec::Bitset(Bitset::from_members(&[1, 2]))));
        Segment {
            hdr: SegHdr {
                seg_kind: 3,
                tok_ver: 0,
                n_rows: 2,
                file_no: 5,
                ref_id: 0,
                dict_no: 0,
                base_seq: 4,
                from_lsn: 0,
                upto_lsn: 1000,
                rt_upto_lsn: 1000,
                total_len: 0,
                seg_digest: [0; 32],
            },
            sections: secs,
        }
    }

    /// [F09 §2], §17.3: a base segment encodes canonically and decodes back; its header digest fields follow.
    #[test]
    fn base_segment_round_trip() {
        let s = small_base();
        let b = encode_segment(&s);
        let d = decode_segment(&b).unwrap();
        assert_eq!(encode_segment(&d), b);
        assert_eq!(d.hdr.total_len, b.len() as u64);
        assert_eq!(&b[..4], b"MSEG");
        let mut bad = b.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(decode_segment(&bad).is_err(), "seg_digest / section xxh3");
    }

    /// [F09 §17.1] V-7, V-11: a missing required section and a TOMB that misses a deleted row are refused.
    #[test]
    fn segment_refusals() {
        let mut s = small_base();
        s.sections.retain(|(e, _)| e.tag != 0x0060);
        assert!(
            decode_segment(&encode_segment(&s)).is_err(),
            "STATS required in a base"
        );
        let mut s = small_base();
        for (e, x) in &mut s.sections {
            if e.tag == 0x0030 {
                *x = Sec::Tomb(vec![]);
                e.count = 0;
            }
        }
        assert!(
            decode_segment(&encode_segment(&s)).is_err(),
            "TOMB = deleted rows"
        );
    }

    /// [F09 §4.5]: array versus bitmap containers are chosen by cardinality; #N 0 never appears.
    #[test]
    fn bitset_forms() {
        let small = Bitset::from_members(&[1, 5, 70_000]);
        let b = encode_bitset(&small);
        assert_eq!(decode_bitset(&b, 0).unwrap(), small);
        let big: Vec<u32> = (1..=5000).collect();
        let bs = Bitset::from_members(&big);
        let b = encode_bitset(&bs);
        assert_eq!(b.len(), 8 + 16 + 8192);
        assert_eq!(decode_bitset(&b, 0).unwrap(), bs);
        let zero = encode_bitset(&Bitset::from_members(&[0, 3]));
        assert!(decode_bitset(&zero, 0).is_err());
    }

    /// [F09 §12.2], §12.3: front coding with maximal `shared`, and postings addressed by `post_off`.
    #[test]
    fn terms_and_postings() {
        let words = ["alpha", "alphabet", "beta"];
        let lists: Vec<Vec<Posting>> = vec![
            vec![(1, 1, vec![2])],
            vec![(1, 4, vec![1]), (3, 5, vec![1, 2])],
            vec![(2, 2, vec![1])],
        ];
        let post = encode_post(&lists);
        let mut offs = Vec::new();
        let mut r = 0usize;
        for l in &lists {
            offs.push(r as u64);
            r += encode_post(std::slice::from_ref(l)).len();
        }
        let terms: Vec<Term> = words
            .iter()
            .zip(&offs)
            .map(|(w, o)| Term {
                term: w.as_bytes().to_vec(),
                post_off: *o,
            })
            .collect();
        let tb = encode_terms(&terms);
        let back = decode_terms(&tb, 0).unwrap();
        assert_eq!(back, terms);
        assert_eq!(decode_post(&post, 0, &back).unwrap(), lists);
    }

    /// [F09 §14.2]: `SYMTAB` arrays and pool in (class, id) order; `by_str` sorted by string.
    #[test]
    fn symtab_round_trip() {
        let st = Symtab {
            classes: vec![
                (1, 1, vec!["zed".into(), "amy".into()]),
                (4, 1, vec!["main".into()]),
            ],
        };
        let b = encode_symtab(&st);
        assert_eq!(decode_symtab(&b, 0).unwrap(), st);
        let dup = Symtab {
            classes: vec![(1, 1, vec!["a".into(), "a".into()])],
        };
        assert!(decode_symtab(&encode_symtab(&dup), 0).is_err());
    }

    /// [F09 §10.2], §10.3: a set-form `FCOL` and a frozen `FIDX` round-trip.
    #[test]
    fn promoted_sections() {
        let c = Fcol {
            field_sym: 5,
            vtype: 7,
            form: 1,
            rows: vec![
                Some(vec![
                    7u32.to_le_bytes().to_vec(),
                    12u32.to_le_bytes().to_vec(),
                ]),
                None,
            ],
        };
        let b = encode_fcol(&c);
        assert_eq!(decode_fcol(&b, 0).unwrap(), c);
        let x = Fidx {
            field_sym: 5,
            vtype: 7,
            form: 0,
            values: vec![
                (
                    7u32.to_le_bytes().to_vec(),
                    FidxBody::Frozen(Bitset::from_members(&[1])),
                ),
                (
                    12u32.to_le_bytes().to_vec(),
                    FidxBody::Frozen(Bitset::from_members(&[1, 9])),
                ),
            ],
        };
        let b = encode_fidx(&x);
        assert_eq!(decode_fidx(&b, 0).unwrap(), x);
    }

    /// [F09 §10.1], [F08 §5.2]: an `FPROMO` set row promotes a set element type; bool, counter and f64 are refused.
    #[test]
    fn fpromo_set_element_types() {
        let row = |vtype: u8, form: u8| {
            let mut w = Writer::new();
            w.u32(1);
            w.u32(12);
            w.u32(5);
            w.u16(0);
            w.u8(vtype);
            w.u8(2);
            w.u8(form);
            w.zeros(3);
            w.into_vec()
        };
        for vtype in [2, 5, 7, 9, 10] {
            assert!(
                decode_sec(0x0061, &row(vtype, 1), 0, 3).is_ok(),
                "set of {vtype}"
            );
        }
        for vtype in [1, 3, 4] {
            assert!(
                decode_sec(0x0061, &row(vtype, 1), 0, 3).is_err(),
                "set of {vtype}"
            );
            assert!(
                decode_sec(0x0061, &row(vtype, 0), 0, 3).is_ok(),
                "scalar {vtype}"
            );
        }
    }

    /// Checks name the section they are about: a `TOMB` that misses a deleted row fails at `TOMB`'s offset.
    #[test]
    fn failures_name_their_section() {
        let mut s = small_base();
        for (e, x) in &mut s.sections {
            if e.tag == 0x0030 {
                *x = Sec::Tomb(vec![]);
                e.count = 0;
            }
        }
        let b = encode_segment(&s);
        let c = decode_container(&b).unwrap();
        let tomb_at = c
            .entries
            .iter()
            .zip(&c.sections)
            .find(|(e, _)| e.tag == 0x0030)
            .map(|(_, (_, at))| *at)
            .unwrap();
        assert_eq!(decode_segment(&b).unwrap_err().offset, tomb_at);
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    /// Member sets that mix sparse chunks (array containers) with dense runs (bitmap containers, > 4,096 members).
    fn members() -> impl Strategy<Value = Vec<u32>> {
        proptest::collection::vec((0u32..4, any::<u16>(), 0u32..6_000, 1u32..3), 0..6).prop_map(
            |runs| {
                let mut s = std::collections::BTreeSet::new();
                for (hi, lo, n, step) in runs {
                    let base = hi << 16;
                    let mut l = u32::from(lo);
                    for _ in 0..n {
                        if l > 0xFFFF {
                            break;
                        }
                        s.insert(base | l);
                        l += step;
                    }
                }
                s.remove(&0);
                s.into_iter().collect()
            },
        )
    }

    proptest! {
        /// [F09 §4.5]: a frozen bitset round-trips through its canonical encoding, whichever containers it uses.
        #[test]
        fn bitset_round_trip(m in members()) {
            let s = Bitset::from_members(&m);
            let b = encode_bitset(&s);
            let back = decode_bitset(&b, 0).unwrap();
            prop_assert_eq!(back.members(), m.clone());
            prop_assert_eq!(back.card(), m.len() as u64);
            prop_assert_eq!(encode_bitset(&back), b);
        }

        /// [F09 §3], §17.1: decoding arbitrary bytes as a segment never panics (the checksum paths).
        #[test]
        fn segment_decode_never_panics(tail in proptest::collection::vec(any::<u8>(), 0..400)) {
            let mut b = b"MSEG".to_vec();
            b.extend_from_slice(&tail);
            if let Ok(s) = decode_segment(&b) {
                prop_assert_eq!(encode_segment(&s), b);
            }
        }

        /// [F09 §17.1] with damage past the checksums: a valid base segment with up to three bytes of its section
        /// data changed, then resealed (every section `xxh3`, `table_xxh3`, `seg_digest` and `hdr_xxh3` recomputed),
        /// reaches the section decoders and the cross-section checks (damage to the zero padding between sections is refused
        /// by the container, [F09 §2.2] P-2); it is refused or read canonically.
        #[test]
        fn damaged_segment_is_canonical_or_refused(
            edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..4),
        ) {
            let mut b = encode_segment(&super::tests::small_base());
            prop_assert!(decode_segment(&b).is_ok());
            let data_off = SEG_HDR + 32 * usize::from(u16::from_le_bytes([b[12], b[13]]));
            let n = b.len() - data_off;
            for (at, v) in edits {
                b[data_off + at % n] = v;
            }
            reseal(&mut b);
            if let Ok(s) = decode_segment(&b) {
                prop_assert_eq!(encode_segment(&s), b);
            }
        }

        /// [F09 §5]–§16: each section of the base segment with up to three bytes changed, decoded alone, is refused
        /// or re-encodes to its bytes.
        #[test]
        fn damaged_section_is_canonical_or_refused(
            which in any::<usize>(),
            edits in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..4),
        ) {
            let s = super::tests::small_base();
            let (e, sec) = &s.sections[which % s.sections.len()];
            let mut b = encode_sec(sec);
            if !b.is_empty() {
                let n = b.len();
                for (at, v) in edits {
                    b[at % n] = v;
                }
            }
            if let Ok(back) = decode_sec(e.tag, &b, 0, 3) {
                prop_assert_eq!(encode_sec(&back), b);
            }
        }
    }

    /// Recomputes every checksum and digest of a segment file whose section placement is unchanged ([F09 §2.1], §2.2).
    fn reseal(b: &mut [u8]) {
        let n = usize::from(u16::from_le_bytes([b[12], b[13]]));
        let data_off = SEG_HDR + 32 * n;
        for j in 0..n {
            let e = SEG_HDR + 32 * j;
            let off = u64::from_le_bytes(b[e + 8..e + 16].try_into().unwrap()) as usize;
            let len = u64::from_le_bytes(b[e + 16..e + 24].try_into().unwrap()) as usize;
            let sum = xxh3_64(&b[off..off + len]);
            b[e + 24..e + 32].copy_from_slice(&sum.to_le_bytes());
        }
        let t = xxh3_64(&b[SEG_HDR..data_off]);
        b[104..112].copy_from_slice(&t.to_le_bytes());
        let d = blake3_256(&b[data_off..]);
        b[72..104].copy_from_slice(&d);
        let h = xxh3_64(&b[..112]);
        b[112..120].copy_from_slice(&h.to_le_bytes());
    }
}
