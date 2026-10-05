//! \[F09\] segments: `SegHdr` and the section table (§2), the tag registry and placement (§3), the layout classes (§4),
//! every section this chapter owns (§5–§14, §16), the runtime sections through [`crate::runtime`] (§14.5, §15), and the
//! open and full checks of §17.1 that one file allows (V-1, V-2, V-4–V-7, V-9–V-11). Over a store's segment set,
//! [`SetSymbols`] resolves names through every layer's `SYMTAB` and refuses one id or one string given twice (§14.2);
//! [`check_stack`] adds V-12 over a stack (I-P3 of §7.1, main-set continuity of §2.3, the ± list preconditions of §4.6,
//! the `FPROMO` presence rule of §10.1) with the main set's `SYMTAB` ranges (§14.2) and the set-wide `SCHEMA` order
//! (§8.3, §17.1), and [`check_changeset`] the presence rule and the `SCHEMA` order for a changeset segment; `TOUCH` =
//! `IDS` is a per-file check. [`decode_container`] and [`encode_container`] also serve the `hist` and `blobs` files of \[F10\].

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::commit::{CkimgEntry, Op, decode_ckimg_entry, encode_ckimg_entry};
use crate::lock::check_format;
use crate::prim::{Error, Reader, Result, Writer, blake3_256, err, xxh3_64};
use crate::runtime::{self, SegKind, Table};
use crate::value::{self, AnchorRec, Creator, FieldEntry, Item, ItemBody, NONE32, NodeHdr};

/// Size of `SegHdr` ([F09 §2.1]).
pub const SEG_HDR: usize = 120;

/// `SegHdr` ([F09 §2.1]); the computed fields (`n_sections`, `total_len`, the digests) are recomputed on encode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegHdr {
    /// `FileFamily` value: 3, 4, 5, 9 (this chapter), 2, 6 (\[F10\]).
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

    /// Every member, ascending, without collecting them.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.chunks.iter().flat_map(|(hi, lo)| {
            lo.iter()
                .map(move |l| (u32::from(*hi) << 16) | u32::from(*l))
        })
    }

    /// Every member.
    pub fn members(&self) -> Vec<u32> {
        self.iter().collect()
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
    // Both lists ascend, so one merge walk finds a common member.
    let (mut i, mut j) = (0, 0);
    while i < plus.len() && j < minus.len() {
        match plus[i].cmp(&minus[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                return err(at, "± list plus and minus are not disjoint [F09 §4.6]");
            }
        }
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
    /// The decoded record, which re-encodes by [`AnchorRec::encode`] ([F08 §10.3]).
    pub rec: Box<AnchorRec>,
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
    /// `SCHEMA` rows, each re-encoded by [`Item::encode`] ([F08 §8.5]).
    Schema(Vec<Item>),
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
                v.push(it);
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
            // `slot` is the row's position and lies in 0–4,095, so the tags `0x4000 + slot` and `0x5000 + slot`
            // stay in their own ranges; refused before any tag arithmetic.
            if n > 4096 {
                return err(
                    at,
                    "FPROMO has more than 4,096 rows: slot is 0–4,095 [F09 §10.1]",
                );
            }
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
                    rec: Box::new(rec),
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
        // [F01 §8.1] S3–S4, §8.2: the ids `first … first + n − 1` lie within the class's width.
        let max = if crate::log::sym::is_u16(class) {
            u64::from(u16::MAX)
        } else {
            u64::from(u32::MAX)
        };
        if u64::from(first) + n as u64 - 1 > max {
            return err(
                e_at,
                format!(
                    "SYMTAB ids of class {class} pass the class's width {max} [F01 §8.1 S4, §8.2]"
                ),
            );
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
            return encode_variable(
                &v.iter()
                    .map(|it| {
                        let mut r = Writer::new();
                        it.encode(&mut r);
                        r.into_vec()
                    })
                    .collect::<Vec<_>>(),
            );
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
                        let mut a = Writer::new();
                        x.rec.encode(&mut a);
                        r.vbytes(a.as_slice());
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

/// A segment's rows ([F09 §2.4]): a base's are `#1…#n_rows`, an upper segment's are its `IDS` (strictly ascending, §5.1).
/// A row is found by a range test or a binary search; nothing is copied.
#[derive(Clone, Copy)]
struct Rows<'a> {
    /// `None` for a base.
    ids: Option<&'a [u32]>,
    /// The row count.
    n: usize,
}

impl Rows<'_> {
    /// The row index of `#N` `id`, when the segment has it as a row.
    fn index(&self, id: u32) -> Option<usize> {
        match self.ids {
            None => (id >= 1 && id as usize <= self.n).then(|| id as usize - 1),
            Some(ids) => ids.binary_search(&id).ok(),
        }
    }
}

/// One bit per row, for the checks that an index names each row at most once.
struct RowBits(Vec<u64>);

impl RowBits {
    fn new(n: usize) -> Self {
        RowBits(vec![0; n.div_ceil(64)])
    }

    /// Sets bit `i`; false when it was already set.
    fn insert(&mut self, i: usize) -> bool {
        let (w, b) = (i / 64, 1u64 << (i % 64));
        let fresh = self.0[w] & b == 0;
        self.0[w] |= b;
        fresh
    }
}

/// The cross-section checks of [F09 §17.1] V-7 (required sections) and V-11, and the per-kind rules of §5–§16, among
/// them §5.2's absent row: every row-scoped section holds its zero value for a row of `kind` 0.
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
    let ids = s.u32s(0x0001);
    let n = if upper {
        ids.len()
    } else {
        s.hdr.n_rows as usize
    };
    if upper && s.hdr.n_rows as usize != n {
        return fail(
            0x0001,
            "n_rows differs from the IDS length [F09 §2.3]".into(),
        );
    }
    let rows = Rows {
        ids: upper.then_some(ids),
        n,
    };
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
    // From here on `nodes[i]` is row i's header. A row is present when its kind is not 0 (§5.2), deleted when it is
    // present with the `deleted` flag (§8.1).
    let row_of = |id: u32| rows.index(id).map(|i| (i, &nodes[i]));
    let present = |id: u32| row_of(id).is_some_and(|(_, h)| h.kind != 0);
    // An index entry or a row-scoped entry of `id` (§5.1, V-11) that is not absent (§5.2).
    let held = |tag: u16, id: u32, what: &str| -> Result<()> {
        match row_of(id) {
            None => fail(
                tag,
                format!("{what} #{id} is not a row of the segment [F09 §17.1 V-11]"),
            ),
            Some((_, h)) if h.kind == 0 => fail(
                tag,
                format!("{what} #{id} is an absent row, which holds no entry [F09 §5.2]"),
            ),
            Some(_) => Ok(()),
        }
    };
    let creators: &[Creator] = match s.get(0x0003) {
        Some(Sec::Creator(v)) => {
            col_len(0x0003, v.len())?;
            v
        }
        _ => &[],
    };
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
    let (mut n_present, mut n_deleted) = (0usize, 0usize);
    for h in nodes {
        // [F09 §5.2]: an absent row is all zero (NodeHdr::decode checks it), so its offsets are 0, not NONE32, and it
        // stores no title, field block or body.
        if h.kind == 0 {
            continue;
        }
        n_present += 1;
        n_deleted += usize::from(h.flags & 1 != 0);
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
            let entry = |j: usize| (kinds[j], other[j]);
            if (a + 1..b).any(|j| entry(j - 1) >= entry(j)) {
                return fail(
                    tag,
                    format!("an {what}-list is not strictly ascending by (kind, node) [F09 §7.1]"),
                );
            }
            if (a..b)
                .map(entry)
                .any(|(k, x)| !valid_edge_kind(k) || x == 0 || (what == "out" && k == 1))
            {
                return fail(
                    tag,
                    format!("an {what}-list entry has an invalid kind or node [F09 §7.1]"),
                );
            }
            if nodes[i].kind == 0 && a != b {
                return fail(
                    tag,
                    format!("an absent row has a non-empty {what}-list [F09 §5.2]"),
                );
            }
        }
    }
    let schema_edge_props: HashMap<u8, u8> = match s.get(0x0032) {
        Some(Sec::Schema(v)) => v
            .iter()
            .filter_map(|it| match &it.body {
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
        // One entry per present row ([F09 §5.5]): as many entries as present rows, each naming a distinct one.
        let mut seen = RowBits::new(n);
        let once = v.len() == n_present
            && v.iter()
                .all(|(_, id)| row_of(*id).is_some_and(|(i, h)| h.kind != 0 && seen.insert(i)));
        if !once {
            return fail(
                0x0007,
                "UID does not hold exactly one entry per present row [F09 §5.5]".into(),
            );
        }
    }
    if let Some(Sec::Tomb(v)) = s.get(0x0030) {
        // `TOMB` ids are strictly ascending (decoded so), so as many as the deleted rows, each naming one, are exactly
        // the deleted rows ([F09 §8.1]).
        let deleted = |id: u32| row_of(id).is_some_and(|(_, h)| h.kind != 0 && h.flags & 1 != 0);
        if v.len() != n_deleted || !v.iter().all(|x| deleted(x.0)) {
            return fail(
                0x0030,
                "TOMB entries differ from the deleted rows [F09 §8.1]".into(),
            );
        }
    }
    if let Some(Sec::Anchors(v)) = s.get(0x0082) {
        for a in v {
            held(0x0082, a.src, "an ANCHORS row's src")?;
        }
        let want: Vec<([u8; 16], u32, u32, u32)> = {
            let mut w: Vec<_> = v
                .iter()
                .map(|a| (a.rec.uid, a.src, a.dst, a.anchor))
                .collect();
            w.sort_unstable();
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
        if let Some(Sec::Paths(v)) = s.get(t) {
            for p in v {
                held(t, p.id, "a PATHIDX/ALIASIDX id")?;
            }
        }
    }
    if let Some(Sec::Runtime(c)) = s.get(0x0031) {
        // Rows of schema keys (`n` = 0) are not row-scoped ([F09 §8.2]).
        for r in c.rows.iter().filter(|r| r.u("n") != 0) {
            held(0x0031, r.u("n") as u32, "a CONFLICTS row's node")?;
        }
    }
    if let Some(Sec::Runtime(g)) = s.get(0x0084) {
        for r in &g.rows {
            held(0x0084, r.u("n") as u32, "a GLOBIDX row's node")?;
        }
    }
    if kind == 5 {
        match s.get(0x0008) {
            Some(Sec::Bitset(b)) if b.iter().eq(ids.iter().copied()) => {}
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
    // [F09 §5.2]: an absent row is in no set of the view. A base's frozen bitsets hold no absent row; an upper
    // segment's ± lists add none. A `minus` entry may name one: a branch segment's absent row masks a node its pinned
    // set holds (§16.3), and its ± lists, relative to that set, take the node out of the sets it was in.
    let set_member = |tag: u16, id: u32| -> Result<()> {
        if row_of(id).is_some_and(|(_, h)| h.kind == 0) {
            return fail(
                tag,
                format!("a bitset adds #{id}, an absent row, which is in no set [F09 §5.2]"),
            );
        }
        Ok(())
    };
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
                if let Some(i) = (0..n).find(|&i| nodes[i].kind == 0 && c.rows[i].is_some()) {
                    return fail(
                        e.tag,
                        format!(
                            "an absent row (row {i}) has a promoted value; its absent bit must be set [F09 §5.2, §10.2]"
                        ),
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
                for (_, body) in &x.values {
                    match body {
                        FidxBody::Frozen(b) => b.iter().try_for_each(|id| set_member(e.tag, id))?,
                        FidxBody::Pm(p) => {
                            p.plus.iter().try_for_each(|id| set_member(e.tag, *id))?
                        }
                    }
                }
            }
            Sec::Bitset(b) if e.tag >= 0x8000 => {
                b.iter().try_for_each(|id| set_member(e.tag, id))?;
            }
            Sec::Pm(p) => p.plus.iter().try_for_each(|id| set_member(e.tag, *id))?,
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
        // [F09 §12.4]: zero for absent and deleted rows.
        if (0..n).any(|i| (nodes[i].kind == 0 || nodes[i].flags & 1 != 0) && v[i] != [0; 3]) {
            return fail(
                0x0052,
                "DOCLEN is not zero for an absent or deleted row [F09 §5.2, §12.4]".into(),
            );
        }
    }
    if let Some(Sec::Post(lists)) = s.get(0x0051)
        && lists
            .iter()
            .flatten()
            .any(|(id, _, _)| !present(*id) || row_of(*id).is_some_and(|(_, h)| h.flags & 1 != 0))
    {
        return fail(
            0x0051,
            "a posting names a row that is not a live row of the segment [F09 §12.3]".into(),
        );
    }
    // [F09 §5.2]: the zero values of an absent row's columns.
    let (topo, defer, due) = (s.u32s(0x0004), s.u32s(0x0005), s.u32s(0x0006));
    for i in (0..n).filter(|&i| nodes[i].kind == 0) {
        let zero = |c: &[u32]| c.get(i).is_none_or(|x| *x == 0);
        let creator = creators.get(i).is_none_or(|c| c.actor == 0 && c.role == 0);
        if !creator {
            return fail(
                0x0003,
                format!("CREATOR is not zero for the absent row {i} [F09 §5.2, §5.3]"),
            );
        }
        for (tag, c) in [(0x0004u16, topo), (0x0005, defer), (0x0006, due)] {
            if !zero(c) {
                return fail(
                    tag,
                    format!(
                        "column 0x{tag:04X} is not zero for the absent row {i} [F09 §5.2, §5.4]"
                    ),
                );
            }
        }
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
    if let Some(Sec::Schema(items)) = s.get(0x0032) {
        // A lone segment resolves names through its own `SYMTAB` (none in a branch or changeset segment); a store's set
        // resolves them through every layer's ([`check_stack`], [F09 §17.1]).
        let own = SetSymbols::of([s])?;
        check_schema_order(items, |id| own.get(SYM_NAME, id), at(0x0032))?;
    }
    Ok(())
}

/// Symbol class `name` ([F09 §14.1]).
const SYM_NAME: u8 = 10;

/// [F09 §8.3]: `SCHEMA` rows strictly ascending in [F08 §8.5]'s item key order, which is (class, the stored key form
/// bytewise) ([F08 §8.5] "Stored key form": names never hold `00`, so the joined form orders as its components do).
/// `name` resolves class `name`; a pair whose names do not all resolve is not compared.
fn check_schema_order<'a>(
    items: &'a [Item],
    name: impl Fn(u32) -> Option<&'a str>,
    at: usize,
) -> Result<()> {
    let keys: Vec<Option<(u8, Vec<u8>)>> = items
        .iter()
        .map(|it| it.stored_key(&name).map(|k| (it.class(), k)))
        .collect();
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

/// The symbols of a segment set ([F09 §14.2]): every `SYMTAB` of the set, merged. A check that compares names resolves
/// them here ([F09 §17.1], spec sync 2b): an upper segment's `SYMTAB` holds only the symbols its window allocated, and
/// a branch or changeset segment carries none.
///
/// The strings stay in the segments' `SYMTAB`s: per class, the set keeps disjoint ascending id ranges, each a slice of
/// one `SYMTAB`'s strings, and resolves an id by binary search.
#[derive(Clone, Debug, Default)]
pub struct SetSymbols<'a> {
    /// Per class: disjoint id ranges in ascending order, as (first id, the strings of ids first, first + 1, …).
    ranges: BTreeMap<u8, Vec<(u32, &'a [String])>>,
}

/// The string of `id` in disjoint ascending ranges.
fn range_get<'a>(ranges: &[(u32, &'a [String])], id: u32) -> Option<&'a str> {
    let i = ranges.partition_point(|r| r.0 <= id).checked_sub(1)?;
    let (first, v) = ranges[i];
    v.get((id - first) as usize).map(String::as_str)
}

impl<'a> SetSymbols<'a> {
    /// Merges the `SYMTAB`s of `segs`. A (class, id) that two of them give different strings refuses the set: a store
    /// has one id space per class across its layers ([F09 §14.2] "Ranges", [F01 §8.1]); so does a string that two ids
    /// of one class carry ([F09 §14.2] "Uniqueness"). Whether a main set's layers hold their ranges in layer order is
    /// [`check_stack`]'s.
    pub fn of(segs: impl IntoIterator<Item = &'a Segment>) -> Result<SetSymbols<'a>> {
        let mut given: BTreeMap<u8, Vec<(u32, &'a [String])>> = BTreeMap::new();
        for s in segs {
            let Some(Sec::Symtab(st)) = s.get(0x0100) else {
                continue;
            };
            for (c, first, v) in &st.classes {
                if !v.is_empty() {
                    given.entry(*c).or_default().push((*first, v.as_slice()));
                }
            }
        }
        let mut ranges = BTreeMap::new();
        for (c, mut v) in given {
            v.sort_by_key(|r| r.0);
            // Every range kept so far starts at or below `first`, so the ids they cover from `first` on are exactly
            // `first..end`: those must agree, and only the part at or above `end` is new.
            let mut kept: Vec<(u32, &'a [String])> = Vec::with_capacity(v.len());
            let mut end = 0u64;
            for (first, strs) in v {
                let r_end = u64::from(first) + strs.len() as u64;
                for id in u64::from(first)..r_end.min(end) {
                    let t = &strs[(id - u64::from(first)) as usize];
                    let old = range_get(&kept, id as u32).expect("covered by a kept range");
                    if old != t {
                        return err(
                            0,
                            format!(
                                "symbol {id} of class {c} is {old:?} in one SYMTAB of the set and {t:?} in another [F09 §14.2]"
                            ),
                        );
                    }
                }
                if r_end > end {
                    let from = end.max(u64::from(first));
                    kept.push((from as u32, &strs[(from - u64::from(first)) as usize..]));
                    end = r_end;
                }
            }
            // "Uniqueness": every id of the class is now in one kept range; sorted by string, two ids of one string
            // are neighbours.
            let mut by_text: Vec<(&str, u64)> = kept
                .iter()
                .flat_map(|(f, v)| {
                    v.iter()
                        .enumerate()
                        .map(move |(j, t)| (t.as_str(), u64::from(*f) + j as u64))
                })
                .collect();
            by_text.sort_unstable();
            if let Some(w) = by_text.windows(2).find(|w| w[0].0 == w[1].0) {
                return err(
                    0,
                    format!(
                        "the string {:?} is symbol {} and symbol {} of class {c} in the set's SYMTABs [F09 §14.2]",
                        w[0].0, w[0].1, w[1].1
                    ),
                );
            }
            ranges.insert(c, kept);
        }
        Ok(SetSymbols { ranges })
    }

    /// The string of symbol `id` of `class` ([F09 §14.1]); `None` when no `SYMTAB` of the set holds it.
    pub fn get(&self, class: u8, id: u32) -> Option<&'a str> {
        range_get(self.ranges.get(&class)?, id)
    }
}

/// The core kinds of schema version 1 by id ([F08 §9.1]).
const CORE_KINDS: [&str; 13] = [
    "task",
    "doc",
    "note",
    "rule",
    "decision",
    "question",
    "finding",
    "verdict",
    "measurement",
    "artifact",
    "run",
    "lane",
    "area",
];

/// The core fields of schema version 1 with `index ≠ none` ([F08 §9.2], §9.3) as (kind or `*`, field, `vtype`, `index`,
/// `form`) in [F09 §10.1]'s `FPROMO` terms.
const CORE_PROMOTED: [(&str, &str, u8, u8, u8); 11] = [
    ("*", "labels", value::ty::SYM, 2, 1),
    ("task", "work_kind", value::ty::ENUM, 2, 0),
    ("task", "phase_state", value::ty::ENUM, 2, 0),
    ("task", "assignee", value::ty::SYM, 2, 0),
    ("finding", "local_id", value::ty::SYM, 2, 0),
    ("finding", "severity", value::ty::ENUM, 2, 0),
    ("finding", "f_kind", value::ty::ENUM, 2, 0),
    ("finding", "round", value::ty::INT, 1, 0),
    ("verdict", "round", value::ty::INT, 1, 0),
    ("verdict", "outcome", value::ty::ENUM, 2, 0),
    ("measurement", "metric", value::ty::SYM, 2, 0),
];

/// The promoted fields of a view's effective schema ([F08 §8.1]: the core schema and the view's items): (kind or `*`,
/// field) → (`vtype`, `index`, `form`) of [F09 §10.1]. Retired items are not fields of the view (no node may use them,
/// [F08 §8.1]).
type Promoted = BTreeMap<(String, String), (u8, u8, u8)>;

/// The view's promoted fields and its kind names by kind id, from the core schema and `items` resolved through `syms`.
fn view_schema(
    items: &[Item],
    syms: &SetSymbols<'_>,
) -> core::result::Result<(Promoted, BTreeMap<u8, String>), String> {
    let name = |id: u32| -> core::result::Result<String, String> {
        if id == 0 {
            return Ok("*".into());
        }
        syms.get(SYM_NAME, id).map(str::to_owned).ok_or_else(|| {
            format!("a SCHEMA item names symbol {id}, which no SYMTAB of the set defines")
        })
    };
    let mut promoted: Promoted = CORE_PROMOTED
        .iter()
        .map(|&(k, f, vt, ix, fm)| ((k.to_owned(), f.to_owned()), (vt, ix, fm)))
        .collect();
    let mut kinds: BTreeMap<u8, String> = (1u8..)
        .zip(CORE_KINDS.iter().map(|k| (*k).to_owned()))
        .collect();
    for it in items.iter().filter(|it| it.iflags & 1 == 0) {
        match &it.body {
            ItemBody::Kind {
                name: n, kind_id, ..
            } => {
                kinds.insert(*kind_id, name(*n)?);
            }
            ItemBody::Field {
                kind,
                name: n,
                ty,
                elem,
                index,
                ..
            } if *index != 0 => {
                let set = *ty == value::ty::SET;
                promoted.insert(
                    (name(*kind)?, name(*n)?),
                    (if set { *elem } else { *ty }, *index, u8::from(set)),
                );
            }
            _ => {}
        }
    }
    Ok((promoted, kinds))
}

/// [F09 §17.1] V-12 over one stack of a view's layers, oldest first: a main set (its base, or its oldest delta when it
/// has none, then its deltas, [F04 §4.1]) or a branch segment on top of its pinned set ([F09 §16.3]), each with the
/// file name its failures name. Each layer is checked with the view's schema as of that layer, the `SCHEMA` of the
/// newest layer at or below it that carries one ([F09 §8.3] fold "full (view)"; none: the core schema alone), and with
/// the set's symbols `syms` ([F09 §17.1], spec sync 2b):
///
/// - the stack's shape ([F09 §4.7], [F04 §4.1]): at most one base, as the oldest layer, then deltas, and at most one
///   branch segment, as the top layer;
/// - main-set continuity ([F09 §2.3]): a delta's `from_lsn` is the `upto_lsn` of the layer below it, and `upto_lsn` and
///   `rt_upto_lsn` never decrease going up (a base's `from_lsn` = 0 and `rt_upto_lsn ≥ upto_lsn` in every layer are
///   checked per file, V-5); a branch segment's `from_lsn` is the `upto_lsn` of the newest layer of its pinned set
///   (§2.3, §16.3). Below the oldest layer the bound is 0: a set without a base folds the log from its start;
/// - "Ranges" ([F09 §14.2]): per symbol class, the main-set layers' `SYMTAB`s hold contiguous, disjoint id ranges in
///   layer order, the oldest layer's from 1 (below it the set folds the log from its start, §2.3);
/// - its `SCHEMA` rows are in [F08 §8.5]'s item key order, the names resolved through `syms` ([F09 §8.3]);
/// - "Presence" ([F09 §10.1]): the layer carries `FPROMO` exactly when one of its rows holds, in its field block, a value
///   for a field with `index ≠ none` of its kind (or of `*`) in that schema, or, in an upper segment, an `FIDX` is
///   present (a value's set changed, §10.3);
/// - a present `FPROMO` has one row per promoted field name, with that field's `vtype`, `index` and `form` (§10.1: "the
///   rows are the fields with `index ≠ none` in the view's schema as of this layer");
/// - the ± list preconditions ([F09 §4.6]): every set a ± list changes — a `BMDIR` key's (§9.2), or a promoted value's
///   in `FIDX` (§10.3), known by field symbol, type and value because slots are per segment (§10.1) — is folded up the
///   stack from the base's frozen bitset (empty without one, §4.7 "bitset"), and each `plus` id is not a member below
///   the layer while each `minus` id is one;
/// - I-P3 ([F09 §7.1]) in the view that each prefix of the stack forms (a prefix is the view at its top layer's bound,
///   and I-P3 holds in every visible version, \[F13\] I-P3), with every `#N`'s lists and parent from its newest layer
///   (§4.7 "row") and none for a `#N` that no layer holds as a row.
pub fn check_stack(layers: &[(&str, &Segment)], syms: &SetSymbols<'_>) -> Result<()> {
    let top = layers.len().saturating_sub(1);
    let mut schema: &[Item] = &[];
    let mut sets = ViewSets::default();
    let mut view: Vec<Adjacency<'_>> = Vec::with_capacity(layers.len());
    let mut next_sym = [1u64; 12];
    for (j, (name, seg)) in layers.iter().enumerate() {
        let named = |m: String| Error {
            offset: 0,
            reason: format!("{name}: {m}"),
            rule: None,
        };
        let below = j.checked_sub(1).map(|i| &layers[i].1.hdr);
        check_layer_place(&seg.hdr, j, top, below).map_err(named)?;
        if matches!(seg.hdr.seg_kind, 3 | 4) {
            check_symbol_ranges(seg, &mut next_sym).map_err(named)?;
        }
        if let Some(Sec::Schema(items)) = seg.get(0x0032) {
            schema = items;
        }
        check_view_layer(seg, schema, syms).map_err(named)?;
        sets.apply(seg).map_err(named)?;
        view.push(Adjacency::of(seg).map_err(named)?);
        check_ip3(&view).map_err(named)?;
    }
    Ok(())
}

/// [F09 §14.2] "Ranges" at one main-set layer: each class's ids in its `SYMTAB` start at `next[class]`, the id after the
/// layers below, which then moves past them.
fn check_symbol_ranges(seg: &Segment, next: &mut [u64; 12]) -> core::result::Result<(), String> {
    let Some(Sec::Symtab(st)) = seg.get(0x0100) else {
        return Ok(());
    };
    for (c, first, v) in &st.classes {
        let Some(want) = next.get_mut(usize::from(*c)) else {
            return Err(format!(
                "SYMTAB class {c} is not a symbol class [F09 §14.1]"
            ));
        };
        if u64::from(*first) != *want {
            return Err(format!(
                "SYMTAB ids of class {c} start at {first}, not at {want}, the id after the layers below: a main set's layers hold contiguous, disjoint ranges in layer order [F09 §14.2]"
            ));
        }
        *want = u64::from(*first) + v.len() as u64;
    }
    Ok(())
}

/// The shape of a stack ([F09 §4.7], [F04 §4.1], §16.3) at layer `j` of `0..=top`, and main-set continuity with the
/// layer below it (§2.3; 0 below the oldest layer).
fn check_layer_place(
    h: &SegHdr,
    j: usize,
    top: usize,
    below: Option<&SegHdr>,
) -> core::result::Result<(), String> {
    let placed = match h.seg_kind {
        3 => j == 0,
        4 => true,
        5 => j == top,
        _ => false,
    };
    if !placed {
        return Err(format!(
            "seg_kind {} cannot be layer {j} of a stack: at most one base, first, then deltas, and at most one branch segment, on top [F09 §4.7, §16.3, F04 §4.1]",
            h.seg_kind
        ));
    }
    let under = below.map_or(0, |b| b.upto_lsn);
    if h.seg_kind != 3 && h.from_lsn != under {
        return Err(format!(
            "from_lsn {} is not the upto_lsn {under} of the layer below [F09 §2.3, §17.1 V-12]",
            h.from_lsn
        ));
    }
    if h.seg_kind == 4
        && let Some(b) = below
        && (h.upto_lsn < b.upto_lsn || h.rt_upto_lsn < b.rt_upto_lsn)
    {
        return Err(format!(
            "upto_lsn {} or rt_upto_lsn {} is below the layer below's ({}, {}): main-set continuity [F09 §2.3, §17.1 V-12]",
            h.upto_lsn, h.rt_upto_lsn, b.upto_lsn, b.rt_upto_lsn
        ));
    }
    Ok(())
}

/// A set of the view that ± lists change ([F09 §4.6]): a `BMDIR` key (§9.2), or the rows holding one promoted value
/// (§10.3) by (`field_sym`, `vtype`, the value's promoted encoding).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SetKey {
    Bm([u8; 4]),
    Value(u32, u8, Vec<u8>),
}

impl std::fmt::Display for SetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetKey::Bm(k) => write!(f, "the BMDIR set {k:?}"),
            SetKey::Value(sym, vtype, v) => {
                write!(
                    f,
                    "the FIDX set of field symbol {sym}, vtype {vtype}, value "
                )?;
                v.iter().try_for_each(|b| write!(f, "{b:02x}"))
            }
        }
    }
}

/// A set's body in one layer: frozen in a base, a ± list in an upper segment ([F09 §9.3], §10.3).
enum SetBody<'b> {
    Frozen(&'b Bitset),
    Pm(&'b PlusMinus),
}

/// Every non-empty set of the view as of the layers applied so far, members ascending ([F09 §4.7] "bitset").
#[derive(Default)]
struct ViewSets(BTreeMap<SetKey, Vec<u32>>);

impl ViewSets {
    /// Applies one layer's `BM.<i>` and `FIDX` sets ([F09 §9], §10.3).
    fn apply(&mut self, seg: &Segment) -> core::result::Result<(), String> {
        let base = seg.hdr.seg_kind == 3;
        let keys: &[[u8; 4]] = match seg.get(0x0040) {
            Some(Sec::Bmdir(v)) => v,
            _ => &[],
        };
        for (e, sec) in &seg.sections {
            match (e.tag, sec) {
                (0x8000..=0xFFFE, Sec::Bitset(_) | Sec::Pm(_)) => {
                    let i = usize::from(e.tag - 0x8000);
                    let key = keys
                        .get(i)
                        .ok_or_else(|| format!("BM.{i} has no BMDIR entry [F09 §9.2]"))?;
                    let body = match sec {
                        Sec::Bitset(b) => SetBody::Frozen(b),
                        Sec::Pm(p) => SetBody::Pm(p),
                        _ => unreachable!("matched above"),
                    };
                    self.change(SetKey::Bm(*key), body, base)?;
                }
                (0x5000..=0x5FFF, Sec::Fidx(x)) => {
                    for (v, b) in &x.values {
                        let body = match b {
                            FidxBody::Frozen(s) => SetBody::Frozen(s),
                            FidxBody::Pm(p) => SetBody::Pm(p),
                        };
                        self.change(SetKey::Value(x.field_sym, x.vtype, v.clone()), body, base)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// A base's frozen bitset defines its set; an upper layer's ± list changes the set below it, each `plus` id not a
    /// member there and each `minus` id one ([F09 §4.6]).
    fn change(
        &mut self,
        key: SetKey,
        body: SetBody<'_>,
        base: bool,
    ) -> core::result::Result<(), String> {
        match (body, base) {
            (SetBody::Frozen(b), true) => {
                let m = b.members();
                if !m.is_empty() {
                    self.0.insert(key, m);
                }
            }
            (SetBody::Pm(p), false) => {
                let below = self.0.remove(&key).unwrap_or_default();
                if let Some(x) = p.plus.iter().find(|x| below.binary_search(x).is_ok()) {
                    return Err(format!(
                        "a ± list of {key} adds #{x}, which is a member below the layer [F09 §4.6, §17.1 V-12]"
                    ));
                }
                if let Some(x) = p.minus.iter().find(|x| below.binary_search(x).is_err()) {
                    return Err(format!(
                        "a ± list of {key} removes #{x}, which is not a member below the layer [F09 §4.6, §17.1 V-12]"
                    ));
                }
                let mut next = Vec::with_capacity(below.len() + p.plus.len());
                let mut plus = p.plus.iter().copied().peekable();
                for x in below
                    .into_iter()
                    .filter(|x| p.minus.binary_search(x).is_err())
                {
                    while let Some(y) = plus.next_if(|&y| y < x) {
                        next.push(y);
                    }
                    next.push(x);
                }
                next.extend(plus);
                if !next.is_empty() {
                    self.0.insert(key, next);
                }
            }
            (SetBody::Frozen(_), false) => {
                return Err(format!(
                    "{key} is a frozen bitset in an upper segment [F09 §9.3, §10.3]"
                ));
            }
            (SetBody::Pm(_), true) => {
                return Err(format!("{key} is a ± list in a base [F09 §9.3, §10.3]"));
            }
        }
        Ok(())
    }
}

/// Edge id of `parent` ([F08 §8.3]), whose forward direction is `NodeHdr.parent` ([F09 §7.1]).
const PARENT: u8 = 1;

/// One direction of a layer's CSR ([F09 §7.1]): row i's list is `(kind[j], node[j])` for j in `off[i]..off[i + 1]`,
/// strictly ascending.
struct Csr<'a> {
    off: &'a [u32],
    node: &'a [u32],
    kind: &'a [u8],
}

impl Csr<'_> {
    /// Row i's list.
    fn list(&self, i: usize) -> impl Iterator<Item = (u8, u32)> + '_ {
        (self.off[i] as usize..self.off[i + 1] as usize).map(|j| (self.kind[j], self.node[j]))
    }

    /// Whether row i's list holds `(k, x)`, by binary search.
    fn has(&self, i: usize, k: u8, x: u32) -> bool {
        let (mut lo, mut hi) = (self.off[i] as usize, self.off[i + 1] as usize);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match (self.kind[mid], self.node[mid]).cmp(&(k, x)) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }
}

/// One layer's rows as I-P3 reads them ([F09 §2.4], §5.2, §7.1): their `#N`s, `NodeHdr.parent` and both CSR lists.
struct Adjacency<'a> {
    /// A base's rows are `#1…#n`; an upper segment's are its `IDS`.
    base: bool,
    ids: &'a [u32],
    nodes: &'a [NodeHdr],
    out: Csr<'a>,
    inn: Csr<'a>,
}

impl<'a> Adjacency<'a> {
    /// The layer's rows, refused when its `IDS`, `NODE` and CSR lengths disagree (a segment that did not decode, V-11).
    fn of(seg: &'a Segment) -> core::result::Result<Self, String> {
        let base = seg.hdr.seg_kind == 3;
        let ids: &[u32] = if base { &[] } else { seg.u32s(0x0001) };
        let n = if base {
            seg.hdr.n_rows as usize
        } else {
            ids.len()
        };
        let nodes: &[NodeHdr] = match seg.get(0x0002) {
            Some(Sec::Node(v)) => v,
            _ => &[],
        };
        let csr = |o: u16, x: u16, k: u16| Csr {
            off: seg.u32s(o),
            node: seg.u32s(x),
            kind: seg.u8s(k),
        };
        let (out, inn) = (csr(0x0020, 0x0021, 0x0022), csr(0x0023, 0x0024, 0x0025));
        let sound = |c: &Csr<'_>| {
            c.off.len() == n + 1
                && c.off[0] == 0
                && c.off.windows(2).all(|w| w[0] <= w[1])
                && c.off[n] as usize == c.node.len()
                && c.node.len() == c.kind.len()
        };
        if nodes.len() != n || !sound(&out) || !sound(&inn) || ids.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(
                "its rows, NODE and CSR sections do not agree [F09 §5.1, §7.1, §17.1 V-11]".into(),
            );
        }
        Ok(Adjacency {
            base,
            ids,
            nodes,
            out,
            inn,
        })
    }

    /// The row of `#N` r in this layer ([F09 §2.4]).
    fn row(&self, r: u32) -> Option<usize> {
        if self.base {
            (r >= 1 && r as usize <= self.nodes.len()).then(|| r as usize - 1)
        } else {
            self.ids.binary_search(&r).ok()
        }
    }

    /// The `#N` of row i.
    fn id(&self, i: usize) -> u32 {
        if self.base { i as u32 + 1 } else { self.ids[i] }
    }
}

/// I-P3 of [F09 §7.1] in the view that `view` (oldest layer first) forms: `(s, k, d)` is in s's out-list iff
/// `(d, k, s)` is in d's in-list, for every kind but `parent`, for which `NodeHdr.parent` of s is d iff `(parent, s)`
/// is in d's in-list. Every `#N`'s state is its newest layer's (§4.7 "row").
fn check_ip3(view: &[Adjacency<'_>]) -> core::result::Result<(), String> {
    let newest = |r: u32| view.iter().rev().find_map(|l| l.row(r).map(|i| (l, i)));
    let has_in = |d: u32, k: u8, s: u32| newest(d).is_some_and(|(l, i)| l.inn.has(i, k, s));
    let has_out = |s: u32, k: u8, d: u32| newest(s).is_some_and(|(l, i)| l.out.has(i, k, d));
    let parent_of = |s: u32| newest(s).map_or(0, |(l, i)| l.nodes[i].parent);
    for (li, l) in view.iter().enumerate() {
        for i in 0..l.nodes.len() {
            let s = l.id(i);
            if view[li + 1..].iter().any(|u| u.row(s).is_some()) {
                continue;
            }
            let p = l.nodes[i].parent;
            if p != 0 && !has_in(p, PARENT, s) {
                return Err(format!(
                    "I-P3: NodeHdr.parent of #{s} is #{p}, but (parent, #{s}) is not in the in-list of #{p} [F09 §7.1, §17.1 V-12]"
                ));
            }
            for (k, d) in l.out.list(i) {
                if !has_in(d, k, s) {
                    return Err(format!(
                        "I-P3: (#{s}, {k}, #{d}) is in the out-list of #{s}, but (#{d}, {k}, #{s}) is not in the in-list of #{d} [F09 §7.1, §17.1 V-12]"
                    ));
                }
            }
            for (k, x) in l.inn.list(i) {
                let held = if k == PARENT {
                    parent_of(x) == s
                } else {
                    has_out(x, k, s)
                };
                if !held {
                    return Err(format!(
                        "I-P3: (#{s}, {k}, #{x}) is in the in-list of #{s}, but #{x} does not hold the edge to #{s} ({}) [F09 §7.1, §17.1 V-12]",
                        if k == PARENT {
                            "its NodeHdr.parent differs"
                        } else {
                            "not in its out-list"
                        }
                    ));
                }
            }
        }
    }
    Ok(())
}

/// A changeset segment `cs.<n>` ([F09 §16.4]) is no layer of a stack: it holds the rows its bulk commit touches, over
/// the state of the commit's ref. When it carries `SCHEMA` (the commit changed the schema, §8.3), that section is the
/// view's schema as of the changeset, and [`check_stack`]'s rules apply with it; otherwise the schema is the ref's
/// state before the commit, which no segment holds, and only the per-file checks apply.
pub fn check_changeset(name: &str, seg: &Segment, syms: &SetSymbols<'_>) -> Result<()> {
    match seg.get(0x0032) {
        Some(Sec::Schema(items)) => {
            check_view_layer(seg, items, syms).or_else(|m| err(0, format!("{name}: {m}")))
        }
        _ => Ok(()),
    }
}

/// The rules of [`check_stack`] over one layer with the view's schema `schema`.
fn check_view_layer(
    seg: &Segment,
    schema: &[Item],
    syms: &SetSymbols<'_>,
) -> core::result::Result<(), String> {
    let sym = |id: u32| syms.get(SYM_NAME, id);
    if let Some(Sec::Schema(items)) = seg.get(0x0032) {
        check_schema_order(items, sym, 0).map_err(|e| e.reason)?;
    }
    let (promoted, kinds) = view_schema(schema, syms)?;
    let nodes: &[NodeHdr] = match seg.get(0x0002) {
        Some(Sec::Node(v)) => v,
        _ => &[],
    };
    let mut blocks = match seg.get(0x0011) {
        Some(Sec::Fields(v)) => v.iter(),
        _ => [].iter(),
    };
    let mut holds = false;
    for h in nodes.iter().filter(|h| h.kind != 0) {
        let block = if h.fields_off != NONE32 {
            blocks.next().map(Vec::as_slice).unwrap_or(&[])
        } else {
            &[]
        };
        if block.is_empty() {
            continue;
        }
        let kind = kinds.get(&h.kind).ok_or_else(|| {
            format!(
                "a row of kind id {} names no kind of the view's schema [F08 §8.3]",
                h.kind
            )
        })?;
        for e in block {
            let f = sym(e.field_sym).ok_or_else(|| {
                format!(
                    "a field block names symbol {}, which no SYMTAB of the set defines",
                    e.field_sym
                )
            })?;
            let key = |k: &str| (k.to_owned(), f.to_owned());
            if promoted.contains_key(&key(kind)) || promoted.contains_key(&key("*")) {
                holds = true;
            }
        }
    }
    let upper = seg.hdr.seg_kind != 3;
    let fidx = seg
        .sections
        .iter()
        .any(|(e, _)| (0x5000..0x6000).contains(&e.tag));
    let fpromo = match seg.get(0x0061) {
        Some(Sec::Fpromo(v)) => Some(v),
        _ => None,
    };
    let want = holds || (upper && fidx);
    if fpromo.is_some() != want {
        return Err(format!(
            "FPROMO is {}, yet {} [F09 §10.1 Presence, §17.1 V-12]",
            if fpromo.is_some() {
                "present"
            } else {
                "absent"
            },
            if want {
                "a row holds a promoted field's value or an FIDX is present"
            } else {
                "no row holds a value of a field with index != none in the view's schema"
            }
        ));
    }
    if let Some(rows) = fpromo {
        let mut by_name: BTreeMap<&str, BTreeSet<(u8, u8, u8)>> = BTreeMap::new();
        for ((_, f), v) in &promoted {
            by_name.entry(f.as_str()).or_default().insert(*v);
        }
        let mut got: BTreeMap<&str, (u8, u8, u8)> = BTreeMap::new();
        for r in rows {
            let f = sym(r.field_sym).ok_or_else(|| {
                format!(
                    "an FPROMO row names symbol {}, which no SYMTAB of the set defines",
                    r.field_sym
                )
            })?;
            got.insert(f, (r.vtype, r.index, r.form));
        }
        let agree = got.len() == by_name.len()
            && got.iter().all(|(f, v)| {
                by_name
                    .get(f)
                    .is_some_and(|s| s.len() == 1 && s.contains(v))
            });
        if !agree {
            return Err(format!(
                "FPROMO rows {got:?} are not the view's promoted fields {by_name:?} [F09 §10.1, §17.1 V-12]"
            ));
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

    /// Recomputes every checksum and digest of a segment file whose section placement is unchanged ([F09 §2.1], §2.2).
    pub(crate) fn reseal(b: &mut [u8]) {
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

    /// [F01 §8.1] S4, §8.2, [F09 §14.2]: a `SYMTAB` class entry's ids lie within the class's width, 65,535 for `role`
    /// and `root` and 2^32 - 1 for the others; an entry that would pass it is refused (built by hand: the encoder
    /// writes only valid tables).
    #[test]
    fn symtab_ids_within_the_width() {
        let table = |class: u8, first: u32, n: u32| {
            let strings: Vec<String> = (0..n).map(|j| format!("s{j}")).collect();
            let mut pool = Writer::new();
            let mut by_id = Writer::new();
            for t in &strings {
                by_id.u32(pool.len() as u32);
                pool.vstr(t);
            }
            let mut w = Writer::new();
            w.u16(1);
            w.u16(0);
            w.u32(pool.len() as u32);
            w.u8(class);
            w.zeros(3);
            w.u32(first);
            w.u32(n);
            w.u32(28);
            w.u32(28 + 4 * n);
            w.bytes(by_id.as_slice());
            // The strings `s0`, `s1`, … sort as their ids do for n <= 10.
            (0..n).for_each(|j| w.u32(first.wrapping_add(j)));
            w.bytes(pool.as_slice());
            w.into_vec()
        };
        for (class, first, n) in [
            (2u8, 65_535u32, 1u32),
            (8, 65_534, 2),
            (1, u32::MAX, 1),
            (11, u32::MAX - 2, 3),
        ] {
            let st = decode_symtab(&table(class, first, n), 0)
                .unwrap_or_else(|e| panic!("class {class} first {first} n {n}: {e}"));
            assert_eq!(encode_symtab(&st), table(class, first, n));
        }
        for (class, first, n) in [
            (2u8, 65_535u32, 2u32),
            (8, 65_536, 1),
            (1, u32::MAX, 2),
            (11, u32::MAX - 1, 3),
        ] {
            let e = decode_symtab(&table(class, first, n), 0).unwrap_err();
            assert!(
                e.reason.contains("width"),
                "class {class} first {first} n {n}: {e}"
            );
        }
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

    /// [F09 §10.1]: `slot` is 0–4,095, so an `FPROMO` of 4,096 rows decodes and one of 4,097 is refused before any
    /// `FCOL`/`FIDX` tag is formed from a slot.
    #[test]
    fn fpromo_slot_range() {
        let table = |n: u32| {
            let mut w = Writer::new();
            w.u32(n);
            w.u32(12);
            for i in 0..n {
                w.u32(i + 1);
                w.u16(i as u16);
                w.u8(2);
                w.u8(1);
                w.u8(0);
                w.zeros(3);
            }
            w.into_vec()
        };
        assert!(decode_sec(0x0061, &table(4096), 0, 3).is_ok());
        let e = decode_sec(0x0061, &table(4097), 0, 3).unwrap_err();
        assert!(e.reason.contains("4,096 rows"), "{e}");
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

    /// `s` with section `tag` replaced by (or given) `sec`, in tag order.
    fn with(mut s: Segment, tag: u16, sec: Sec) -> Segment {
        s.sections.retain(|(e, _)| e.tag != tag);
        s.sections.push((ent(tag, 0), sec));
        s.sections.sort_by_key(|x| x.0.tag);
        s
    }

    /// Base segment `s` as a delta over `below` ([F09 §2.3], §16.2): kind 4, the bounds just above `below`'s, `IDS` =
    /// its rows `#1…#n`, no set changed (no `BMDIR`, no `BM.<i>`) and no symbol allocated (an empty `SYMTAB`, §14.2).
    fn as_delta(s: Segment, below: &Segment) -> Segment {
        let n = s.hdr.n_rows;
        let s = with(s, 0x0100, Sec::Symtab(Symtab { classes: vec![] }));
        let mut d = with(s, 0x0001, Sec::Ids((1..=n).collect()));
        d.sections
            .retain(|(e, _)| e.tag != 0x0040 && e.tag < 0x8000);
        d.hdr.seg_kind = 4;
        d.hdr.from_lsn = below.hdr.upto_lsn;
        d.hdr.upto_lsn = below.hdr.upto_lsn + 100;
        d.hdr.rt_upto_lsn = d.hdr.upto_lsn.max(below.hdr.rt_upto_lsn);
        d
    }

    /// `s` with its CSR replaced ([F09 §7.1]): per row, its out-list and its in-list as (kind, node).
    fn with_csr(mut s: Segment, out: &[&[(u8, u32)]], inn: &[&[(u8, u32)]]) -> Segment {
        for (lists, tags) in [
            (out, [0x0020u16, 0x0021, 0x0022]),
            (inn, [0x0023, 0x0024, 0x0025]),
        ] {
            let mut off = vec![0u32];
            let (mut node, mut kind) = (Vec::new(), Vec::new());
            for l in lists {
                for &(k, x) in *l {
                    kind.push(k);
                    node.push(x);
                }
                off.push(node.len() as u32);
            }
            s = with(s, tags[0], Sec::U32s(off));
            s = with(s, tags[1], Sec::U32s(node));
            s = with(s, tags[2], Sec::U8s(kind));
        }
        s
    }

    /// `s` with `f` applied to its `NODE` rows.
    fn map_nodes(mut s: Segment, f: impl Fn(&mut Vec<NodeHdr>)) -> Segment {
        for (_, x) in &mut s.sections {
            if let Sec::Node(v) = x {
                f(v);
            }
        }
        s
    }

    /// [F09 §4.7], §2.3, §16.3, §17.1 V-12: a stack is at most one base, first, then deltas, with at most one branch
    /// segment on top; a delta's `from_lsn` is the `upto_lsn` below it (0 below the oldest layer), `upto_lsn` and
    /// `rt_upto_lsn` never decrease, and a branch segment's `from_lsn` is its pinned set's newest `upto_lsn`.
    #[test]
    fn stack_shape_and_continuity() {
        let base = small_base();
        let syms = SetSymbols::of([&base]).unwrap();
        let d1 = as_delta(small_base(), &base);
        check_stack(&[("base", &base), ("d1", &d1)], &syms).unwrap();
        let d2 = as_delta(small_base(), &d1);
        check_stack(&[("base", &base), ("d1", &d1), ("d2", &d2)], &syms).unwrap();
        let mut gap = d1.clone();
        gap.hdr.from_lsn -= 1;
        let e = check_stack(&[("base", &base), ("d1", &gap)], &syms).unwrap_err();
        assert!(e.reason.starts_with("d1: from_lsn 999"), "{e}");
        let mut high = base.clone();
        high.hdr.rt_upto_lsn = 5000;
        let e = check_stack(&[("base", &high), ("d1", &d1)], &syms).unwrap_err();
        assert!(e.reason.contains("main-set continuity"), "{e}");
        let mut back = d2.clone();
        back.hdr.upto_lsn = d1.hdr.upto_lsn - 1;
        back.hdr.rt_upto_lsn = d1.hdr.rt_upto_lsn;
        let e = check_stack(&[("base", &base), ("d1", &d1), ("d2", &back)], &syms).unwrap_err();
        assert!(e.reason.contains("main-set continuity"), "{e}");
        // A set without a base: its oldest delta folds the log from lsn 0.
        let mut lone = d1.clone();
        lone.hdr.from_lsn = 0;
        check_stack(&[("d1", &lone)], &syms).unwrap();
        assert!(check_stack(&[("d1", &d1)], &syms).is_err());
        // A branch segment over its pinned set; its upto_lsn is its tip's and may lie below from_lsn.
        let mut b = d1.clone();
        b.hdr.seg_kind = 5;
        b.hdr.upto_lsn = 400;
        b.hdr.rt_upto_lsn = 0;
        check_stack(&[("base", &base), ("b", &b)], &syms).unwrap();
        let mut off = b.clone();
        off.hdr.from_lsn = 7;
        assert!(check_stack(&[("base", &base), ("b", &off)], &syms).is_err());
        // Shapes that are no stack.
        let mut cs = d1.clone();
        cs.hdr.seg_kind = 9;
        for layers in [
            vec![("d1", &lone), ("base", &base)],
            vec![("base", &base), ("b", &b), ("d1", &d1)],
            vec![("base", &base), ("cs", &cs)],
        ] {
            let e = check_stack(&layers, &syms).unwrap_err();
            assert!(e.reason.contains("cannot be layer"), "{e}");
        }
    }

    /// [F09 §4.6], §9, §17.1 V-12: `small_base` holds the tasks #1 and #2 in the frozen `BMDIR` set (1, 1, 0, 0); a
    /// delta may remove #2 and a later one add it back, but never add a member or remove a non-member; a set without a
    /// frozen bitset in the base is empty.
    #[test]
    fn stack_plus_minus_preconditions() {
        let base = small_base();
        let syms = SetSymbols::of([&base]).unwrap();
        let change = |below: &Segment, key: [u8; 4], plus: Vec<u32>, minus: Vec<u32>| {
            let d = with(as_delta(small_base(), below), 0x0040, Sec::Bmdir(vec![key]));
            with(d, 0x8000, Sec::Pm(PlusMinus { plus, minus }))
        };
        let tasks = [1, 1, 0, 0];
        let d1 = change(&base, tasks, vec![], vec![2]);
        let d2 = change(&d1, tasks, vec![2], vec![]);
        check_stack(&[("base", &base), ("d1", &d1), ("d2", &d2)], &syms).unwrap();
        let again = change(&d1, tasks, vec![], vec![2]);
        let e = check_stack(&[("base", &base), ("d1", &d1), ("d2", &again)], &syms).unwrap_err();
        assert!(
            e.reason.starts_with("d2: ") && e.reason.contains("removes #2"),
            "{e}"
        );
        let member = change(&base, tasks, vec![1], vec![]);
        let e = check_stack(&[("base", &base), ("d1", &member)], &syms).unwrap_err();
        assert!(e.reason.contains("adds #1"), "{e}");
        let deleted = [3, 0, 0, 3];
        let add = change(&base, deleted, vec![2], vec![]);
        check_stack(&[("base", &base), ("d1", &add)], &syms).unwrap();
        let remove = change(&base, deleted, vec![], vec![2]);
        let e = check_stack(&[("base", &base), ("d1", &remove)], &syms).unwrap_err();
        assert!(e.reason.contains("removes #2"), "{e}");
    }

    /// [F09 §4.6], §10.3: `FIDX` sets fold by field symbol, type and value across layers whatever their slots; a frozen
    /// body belongs in a base and a ± list in an upper segment.
    #[test]
    fn view_sets_fold_fidx_values() {
        let fidx = |kind: u8, slot: u16, values: Vec<(u16, FidxBody)>| Segment {
            hdr: SegHdr {
                seg_kind: kind,
                ..small_base().hdr
            },
            sections: vec![(
                ent(0x5000 + slot, 0),
                Sec::Fidx(Fidx {
                    field_sym: 3,
                    vtype: 5,
                    form: u8::from(kind != 3),
                    values: values
                        .into_iter()
                        .map(|(v, b)| (v.to_le_bytes().to_vec(), b))
                        .collect(),
                }),
            )],
        };
        let frozen = |m: &[u32]| FidxBody::Frozen(Bitset::from_members(m));
        let pm = |plus: Vec<u32>, minus: Vec<u32>| FidxBody::Pm(PlusMinus { plus, minus });
        let mut sets = ViewSets::default();
        sets.apply(&fidx(3, 0, vec![(1, frozen(&[4, 9])), (2, frozen(&[5]))]))
            .unwrap();
        sets.apply(&fidx(
            4,
            3,
            vec![(1, pm(vec![5], vec![9])), (2, pm(vec![], vec![5]))],
        ))
        .unwrap();
        let key = |v: u16| SetKey::Value(3, 5, v.to_le_bytes().to_vec());
        assert_eq!(sets.0.get(&key(1)), Some(&vec![4, 5]));
        assert_eq!(sets.0.get(&key(2)), None, "an emptied set is dropped");
        let e = sets
            .apply(&fidx(4, 1, vec![(1, pm(vec![4], vec![]))]))
            .unwrap_err();
        assert!(e.contains("adds #4"), "{e}");
        let mut other = ViewSets::default();
        assert!(other.apply(&fidx(4, 0, vec![(1, frozen(&[4]))])).is_err());
        assert!(
            other
                .apply(&fidx(3, 0, vec![(1, pm(vec![4], vec![]))]))
                .is_err()
        );
    }

    /// [F09 §7.1] I-P3, §17.1 V-12: `small_base`'s one `blocks` edge #1 → #2 is in both lists; either half alone, a
    /// `NodeHdr.parent` without its in-list entry (or the reverse), an edge to a node no layer holds, and a delta that
    /// changes one endpoint only are refused.
    #[test]
    fn stack_ip3() {
        let base = small_base();
        let syms = SetSymbols::of([&base]).unwrap();
        check_stack(&[("base", &base)], &syms).unwrap();
        let fails = |layers: &[(&str, &Segment)], want: &str| {
            let e = check_stack(layers, &syms).unwrap_err();
            assert!(e.reason.contains("I-P3") && e.reason.contains(want), "{e}");
        };
        let no_in = with_csr(small_base(), &[&[(2, 2)], &[]], &[&[], &[]]);
        fails(&[("base", &no_in)], "not in the in-list of #2");
        let no_out = with_csr(small_base(), &[&[], &[]], &[&[], &[(2, 1)]]);
        fails(&[("base", &no_out)], "not in its out-list");
        let dangling = with_csr(small_base(), &[&[(2, 2), (2, 5)], &[]], &[&[], &[(2, 1)]]);
        fails(&[("base", &dangling)], "in-list of #5");
        // parent: NodeHdr.parent of #1 is #2 exactly when (parent, #1) is in #2's in-list.
        let child = map_nodes(small_base(), |v| v[0].parent = 2);
        fails(&[("base", &child)], "NodeHdr.parent of #1 is #2");
        let both = with_csr(child, &[&[(2, 2)], &[]], &[&[], &[(PARENT, 1), (2, 1)]]);
        check_stack(&[("base", &both)], &syms).unwrap();
        let orphan = with_csr(
            small_base(),
            &[&[(2, 2)], &[]],
            &[&[], &[(PARENT, 1), (2, 1)]],
        );
        fails(&[("base", &orphan)], "NodeHdr.parent differs");
        // A delta that drops the edge at #1 alone leaves #2's in-list entry in the base: refused in its view.
        let mut one = with(as_delta(small_base(), &base), 0x0001, Sec::Ids(vec![1]));
        one.hdr.n_rows = 1;
        let one = with_csr(map_nodes(one, |v| v.truncate(1)), &[&[]], &[&[]]);
        let e = check_stack(&[("base", &base), ("d", &one)], &syms).unwrap_err();
        assert!(e.reason.starts_with("d: I-P3"), "{e}");
        let both_ends = with_csr(as_delta(small_base(), &base), &[&[], &[]], &[&[], &[]]);
        check_stack(&[("base", &base), ("d", &both_ends)], &syms).unwrap();
        // A lone delta holds both endpoints of its edge.
        let mut lone = as_delta(small_base(), &base);
        lone.hdr.from_lsn = 0;
        check_stack(&[("d", &lone)], &syms).unwrap();
    }

    /// [F09 §14.2]: the set's `SYMTAB` ranges merge per class, overlaps included; one id with two strings refuses the
    /// set.
    #[test]
    fn set_symbols_merge_ranges() {
        let seg = |first: u32, v: &[&str]| {
            with(
                small_base(),
                0x0100,
                Sec::Symtab(Symtab {
                    classes: vec![(10, first, v.iter().map(|s| (*s).to_owned()).collect())],
                }),
            )
        };
        let (a, b, c) = (
            seg(1, &["a", "b", "c", "d"]),
            seg(3, &["c", "d", "e"]),
            seg(2, &["b"]),
        );
        let s = SetSymbols::of([&b, &a, &c]).unwrap();
        let got: Vec<Option<&str>> = (0..7).map(|id| s.get(10, id)).collect();
        assert_eq!(
            got,
            [
                None,
                Some("a"),
                Some("b"),
                Some("c"),
                Some("d"),
                Some("e"),
                None
            ]
        );
        assert_eq!(s.get(1, 1), None);
        let clash = seg(4, &["x"]);
        let e = SetSymbols::of([&a, &b, &clash]).unwrap_err();
        assert!(e.reason.contains("symbol 4 of class 10"), "{e}");
    }

    /// [F09 §14.2] "Uniqueness": a string is at most one id of its class across the set's `SYMTAB`s, overlapping
    /// ranges included.
    #[test]
    fn set_symbols_unique_strings() {
        let seg = |first: u32, v: &[&str]| {
            with(
                small_base(),
                0x0100,
                Sec::Symtab(Symtab {
                    classes: vec![(10, first, v.iter().map(|s| (*s).to_owned()).collect())],
                }),
            )
        };
        let (a, b) = (seg(1, &["a", "b"]), seg(2, &["b", "c"]));
        SetSymbols::of([&a, &b]).unwrap();
        let again = seg(3, &["a"]);
        let e = SetSymbols::of([&a, &again]).unwrap_err();
        assert!(
            e.reason
                .contains("\"a\" is symbol 1 and symbol 3 of class 10"),
            "{e}"
        );
        // Another class may hold the same string.
        let other = with(
            small_base(),
            0x0100,
            Sec::Symtab(Symtab {
                classes: vec![(10, 1, vec!["a".into()]), (11, 1, vec!["a".into()])],
            }),
        );
        SetSymbols::of([&other]).unwrap();
    }

    /// [F09 §14.2] "Ranges", §17.1 V-12: a main set's layers hold contiguous, disjoint per-class id ranges in layer
    /// order, the oldest from 1; a gap, an overlap and a set without a base whose oldest delta starts above 1 are
    /// refused; a branch segment carries no `SYMTAB`.
    #[test]
    fn stack_symbol_ranges() {
        let base = small_base();
        let syms = SetSymbols::of([&base]).unwrap();
        let symtab = |first: u32, v: &[&str]| {
            Sec::Symtab(Symtab {
                classes: vec![(10, first, v.iter().map(|s| (*s).to_owned()).collect())],
            })
        };
        let d1 = with(
            as_delta(small_base(), &base),
            0x0100,
            symtab(2, &["x", "y"]),
        );
        check_stack(&[("base", &base), ("d1", &d1)], &syms).unwrap();
        let d2 = with(as_delta(small_base(), &d1), 0x0100, symtab(4, &["z"]));
        check_stack(&[("base", &base), ("d1", &d1), ("d2", &d2)], &syms).unwrap();
        // d1 allocates nothing: d2 continues from the base.
        let quiet = as_delta(small_base(), &base);
        let next = with(as_delta(small_base(), &quiet), 0x0100, symtab(2, &["x"]));
        check_stack(&[("base", &base), ("d1", &quiet), ("d2", &next)], &syms).unwrap();
        for (first, what) in [(3, "a gap"), (1, "an overlap")] {
            let d = with(as_delta(small_base(), &base), 0x0100, symtab(first, &["x"]));
            let e = check_stack(&[("base", &base), ("d1", &d)], &syms).unwrap_err();
            assert!(
                e.reason.starts_with("d1: SYMTAB ids of class 10 start at"),
                "{what}: {e}"
            );
        }
        let mut lone = with(as_delta(small_base(), &base), 0x0100, symtab(2, &["x"]));
        lone.hdr.from_lsn = 0;
        assert!(check_stack(&[("d1", &lone)], &syms).is_err());
    }

    /// Every `SecEnt.count` of `s` set to its content's ([F09 §4]).
    fn recount(mut s: Segment) -> Segment {
        for (e, x) in &mut s.sections {
            if let Some(c) = class_count(e.tag, x) {
                e.count = c as u32;
            }
        }
        s
    }

    /// The per-file checks of `s` ([F09 §17.1] V-11, §5–§16) on its decoded form.
    fn per_file(s: &Segment) -> Result<()> {
        check_segment(&recount(s.clone()), &BTreeMap::new())
    }

    /// `small_base` with a third row, #3, absent ([F09 §5.2]): an all-zero `NODE` row, zero column elements, empty
    /// adjacency lists and no entry anywhere else.
    fn with_absent() -> Segment {
        let mut s = small_base();
        s.hdr.n_rows = 3;
        for (e, x) in &mut s.sections {
            match (e.tag, x) {
                (0x0002, Sec::Node(v)) => v.push(NodeHdr::default()),
                (0x0003, Sec::Creator(v)) => v.push(Creator { actor: 0, role: 0 }),
                (0x0004..=0x0006, Sec::U32s(v)) => v.push(0),
                (0x0020 | 0x0023, Sec::U32s(v)) => v.push(*v.last().expect("n + 1 offsets")),
                _ => {}
            }
        }
        recount(s)
    }

    /// `s` with section `tag`'s content changed by `f`.
    fn edit(mut s: Segment, tag: u16, f: impl FnOnce(&mut Sec)) -> Segment {
        if let Some((_, x)) = s.sections.iter_mut().find(|(e, _)| e.tag == tag) {
            f(x);
        }
        s
    }

    /// A promoted `enum` field, symbol 1, indexed as a bitmap in slot 0 ([F09 §10.1]-§10.3): #1 holds value 1.
    fn with_promoted(s: Segment) -> Segment {
        let s = with(
            s,
            0x0061,
            Sec::Fpromo(vec![Fpromo {
                field_sym: 1,
                slot: 0,
                vtype: 5,
                index: 2,
                form: 0,
            }]),
        );
        let s = with(
            s,
            0x4000,
            Sec::Fcol(Fcol {
                field_sym: 1,
                vtype: 5,
                form: 0,
                rows: vec![Some(vec![vec![1, 0]]), None, None],
            }),
        );
        with(
            s,
            0x5000,
            Sec::Fidx(Fidx {
                field_sym: 1,
                vtype: 5,
                form: 0,
                values: vec![(vec![1, 0], FidxBody::Frozen(Bitset::from_members(&[1])))],
            }),
        )
    }

    /// One `CONFLICTS` row (a status conflict) of node `n` ([F11 §10]).
    fn conflict_row(n: u32) -> runtime::Row {
        use runtime::{FV, Slice};
        let side = || FV::Slice(Slice::KVal(Box::new(crate::commit::KVal::Status(None))));
        runtime::Row {
            table: Table::Conflicts,
            vals: vec![
                FV::U(u64::from(n)),
                FV::U(2),
                FV::U(0),
                FV::Bytes(vec![0; 2]),
                FV::Bytes(vec![0; 16]),
                FV::Slice(Slice::CKey(crate::commit::CKey::Status(n))),
                side(),
                side(),
                side(),
            ],
        }
    }

    /// One `GLOBIDX` row of node `n` ([F11 §11]).
    fn glob_row(n: u32) -> runtime::Row {
        use runtime::{FV, Slice};
        runtime::Row {
            table: Table::GlobIdx,
            vals: vec![
                FV::U(u64::from(n)),
                FV::U(1),
                FV::U(5),
                FV::Slice(Slice::Text("docs/*.md".into())),
            ],
        }
    }

    /// `sec` (a runtime section of `rt_empty`'s form) holding `rows`.
    fn rt_with(sec: Sec, rows: Vec<runtime::Row>) -> Sec {
        let Sec::Runtime(mut r) = sec else {
            unreachable!("a runtime section")
        };
        r.hdr.n_rows = rows.len() as u32;
        r.rows = rows;
        Sec::Runtime(r)
    }

    /// [F09 §5.2]: an absent row holds the zero value of every row-scoped section: zero `CREATOR`, `TOPO`, `DEFER` and
    /// `DUE` elements, empty adjacency lists, no `PATHIDX`, `ALIASIDX`, `ANCHORS`, `CONFLICTS` or `GLOBIDX` entry, a
    /// set absent bit in every `FCOL`, zero `DOCLEN`, and no membership in a base's bitsets or `FIDX` sets.
    #[test]
    fn absent_row_holds_zero_values() {
        let s = with_absent();
        per_file(&s).unwrap();
        let refused = |s: Segment, what: &str| {
            let e = per_file(&s).unwrap_err();
            assert!(e.reason.contains("absent"), "{what}: {e}");
        };
        refused(
            edit(s.clone(), 0x0003, |x| {
                if let Sec::Creator(v) = x {
                    v[2].actor = 5;
                }
            }),
            "CREATOR",
        );
        for t in [0x0004u16, 0x0005, 0x0006] {
            refused(
                edit(s.clone(), t, |x| {
                    if let Sec::U32s(v) = x {
                        v[2] = 77;
                    }
                }),
                "TOPO, DEFER, DUE",
            );
        }
        refused(
            with_csr(
                s.clone(),
                &[&[(2, 2)], &[], &[(2, 1)]],
                &[&[(2, 3)], &[(2, 1)], &[]],
            ),
            "out-list",
        );
        refused(
            with_csr(
                s.clone(),
                &[&[(2, 2)], &[], &[]],
                &[&[], &[(2, 1)], &[(4, 1)]],
            ),
            "in-list",
        );
        let path = PathRow {
            root: 1,
            id: 3,
            fold: b"a.md".to_vec(),
            path: b"a.md".to_vec(),
        };
        for t in [0x0080u16, 0x0081] {
            refused(
                with(s.clone(), t, Sec::Paths(vec![path.clone()])),
                "PATHIDX, ALIASIDX",
            );
        }
        let anchor = AnchorRow {
            src: 3,
            dst: 1,
            anchor: 1,
            rec: Box::new(file_anchor(4)),
        };
        refused(
            with(s.clone(), 0x0082, Sec::Anchors(vec![anchor])),
            "ANCHORS",
        );
        let conflicts = rt_with(rt_empty(Table::Conflicts, 1), vec![conflict_row(3)]);
        refused(with(s.clone(), 0x0031, conflicts), "CONFLICTS");
        let ok = rt_with(rt_empty(Table::Conflicts, 1), vec![conflict_row(1)]);
        per_file(&with(s.clone(), 0x0031, ok)).unwrap();
        let globs = rt_with(rt_empty(Table::GlobIdx, 1), vec![glob_row(3)]);
        refused(with(s.clone(), 0x0084, globs), "GLOBIDX");
        refused(
            with(
                s.clone(),
                0x8000,
                Sec::Bitset(Bitset::from_members(&[1, 2, 3])),
            ),
            "BM.<i>",
        );
        let p = with_promoted(s.clone());
        per_file(&p).unwrap();
        refused(
            edit(p.clone(), 0x4000, |x| {
                if let Sec::Fcol(c) = x {
                    c.rows[2] = Some(vec![vec![1, 0]]);
                }
            }),
            "FCOL",
        );
        refused(
            edit(p, 0x5000, |x| {
                if let Sec::Fidx(f) = x {
                    f.values[0].1 = FidxBody::Frozen(Bitset::from_members(&[1, 3]));
                }
            }),
            "FIDX",
        );
        // Full text with DOCLEN: zero for the absent row #3 and for the deleted row #2 ([F09 §12.4]).
        let mut fts = with(s.clone(), 0x0050, Sec::Terms(vec![]));
        fts = with(fts, 0x0051, Sec::Post(vec![]));
        fts = with(fts, 0x0052, Sec::Doclen(vec![[3, 0, 9], [0; 3], [0; 3]]));
        fts = with(fts, 0x0053, Sec::FtsStat([(1, 3), (0, 0), (1, 9)]));
        fts.hdr.tok_ver = 1;
        per_file(&fts).unwrap();
        for row in [1usize, 2] {
            let bad = edit(fts.clone(), 0x0052, |x| {
                if let Sec::Doclen(v) = x {
                    v[row] = [1, 0, 0];
                }
            });
            let e = per_file(&bad).unwrap_err();
            assert!(e.reason.contains("DOCLEN"), "row {row}: {e}");
        }
    }

    /// [F09 §5.2], §4.6, §16.3: an upper segment's ± lists never add an absent row; a `minus` entry may name one (a
    /// branch segment's absent row takes a node of its pinned set out of the sets it was in).
    #[test]
    fn absent_row_in_plus_minus_lists() {
        let mut d = with(
            as_delta(with_absent(), &small_base()),
            0x0040,
            Sec::Bmdir(vec![[1, 1, 0, 0]]),
        );
        d.hdr.n_rows = 3;
        let pm = |plus: Vec<u32>, minus: Vec<u32>| {
            with(d.clone(), 0x8000, Sec::Pm(PlusMinus { plus, minus }))
        };
        per_file(&pm(vec![], vec![3])).unwrap();
        let e = per_file(&pm(vec![3], vec![])).unwrap_err();
        assert!(e.reason.contains("absent"), "{e}");
    }

    /// A `file` anchor record ([F08 §10.3]): no hint, quote, window or blob.
    fn file_anchor(uid: u8) -> AnchorRec {
        AnchorRec {
            aflags: 0,
            uid: [uid; 16],
            kind: 1,
            mode: 1,
            watch: 1,
            resolver: 1,
            captured: [5; 16],
            pred: None,
            hint: None,
            scope: None,
            quote: None,
            end: None,
            occurrence: None,
            window: None,
            span_hash: None,
            blob: crate::prim::Oid::None,
            git: None,
            marker: None,
        }
    }

    /// [F08 §8.5], §10.3, E3: `SCHEMA` rows and `ANCHORS` records re-encode from their decoded values, so a re-encode
    /// tests the item and record encodings instead of copying the input: a changed value changes the bytes. A row whose
    /// bytes are more than its item's or record's encoding is refused.
    #[test]
    fn schema_and_anchor_rows_encode_from_their_values() {
        let it = int_field(3, 1, 0, 0);
        let mut iw = Writer::new();
        it.encode(&mut iw);
        let schema = Sec::Schema(vec![it.clone()]);
        let b = encode_sec(&schema);
        assert_eq!(b, encode_variable(&[iw.as_slice().to_vec()]));
        assert_eq!(decode_sec(0x0032, &b, 0, 3).unwrap(), schema);
        let mut retired = it.clone();
        retired.iflags = 1;
        let b2 = encode_sec(&Sec::Schema(vec![retired.clone()]));
        assert_ne!(b2, b);
        assert_eq!(
            decode_sec(0x0032, &b2, 0, 3).unwrap(),
            Sec::Schema(vec![retired])
        );
        let long = encode_variable(&[[iw.as_slice(), &[0]].concat()]);
        assert!(decode_sec(0x0032, &long, 0, 3).is_err());

        let row = AnchorRow {
            src: 1,
            dst: 2,
            anchor: 7,
            rec: Box::new(file_anchor(9)),
        };
        let anchors = Sec::Anchors(vec![row.clone()]);
        let b = encode_sec(&anchors);
        assert_eq!(decode_sec(0x0082, &b, 0, 3).unwrap(), anchors);
        let mut pinned = row.clone();
        pinned.rec.mode = 2;
        let b2 = encode_sec(&Sec::Anchors(vec![pinned.clone()]));
        assert_ne!(b2, b);
        assert_eq!(
            decode_sec(0x0082, &b2, 0, 3).unwrap(),
            Sec::Anchors(vec![pinned])
        );
        let mut rw = Writer::new();
        file_anchor(9).encode(&mut rw);
        let mut long = Writer::new();
        long.u32(1);
        long.u32(2);
        long.u32(7);
        long.vbytes(&[rw.as_slice(), &[0]].concat());
        assert!(decode_sec(0x0082, &encode_variable(&[long.into_vec()]), 0, 3).is_err());
    }

    /// A project field item `kind`.`name` of type `int` with `index`.
    fn int_field(kind: u32, name: u32, index: u8, iflags: u8) -> Item {
        Item {
            iflags,
            body: ItemBody::Field {
                kind,
                name,
                ty: value::ty::INT,
                elem: 0,
                class: 0,
                storage: 4,
                decl: 30,
                optional: true,
                index,
                coerce: 0,
                cflags: 0,
                default: None,
                range: None,
            },
        }
    }

    /// [F09 §10.1] "Presence", §17.1 V-12: `FPROMO` exactly when a row holds a value of a field its kind (or `*`)
    /// promotes in the view's schema as of the layer, or an upper segment's `FIDX` shows a changed set; its rows are the
    /// view's promoted fields; the schema is the newest `SCHEMA` at or below the layer; retired items promote nothing.
    #[test]
    fn view_presence_rule() {
        // Symbols 1 `estimate` (the field small_base's row #1, a task, holds), 2–11 the core promoted fields, 12 `task`.
        let names = [
            "estimate",
            "labels",
            "work_kind",
            "phase_state",
            "assignee",
            "local_id",
            "severity",
            "f_kind",
            "round",
            "outcome",
            "metric",
            "task",
        ];
        let symtab = Sec::Symtab(Symtab {
            classes: vec![(10, 1, names.iter().map(|s| (*s).to_owned()).collect())],
        });
        let base = with(small_base(), 0x0100, symtab);
        let syms = SetSymbols::of([&base]).unwrap();
        check_stack(&[("base", &base)], &syms).unwrap();
        // A project item promotes task.estimate: row #1 now holds a promoted value, so FPROMO must be present.
        let promoting = with(
            base.clone(),
            0x0032,
            Sec::Schema(vec![int_field(12, 1, 1, 0)]),
        );
        let e = check_stack(&[("base", &promoting)], &syms).unwrap_err();
        assert!(e.reason.contains("Presence"), "{e}");
        let retired = with(
            base.clone(),
            0x0032,
            Sec::Schema(vec![int_field(12, 1, 1, 1)]),
        );
        check_stack(&[("base", &retired)], &syms).unwrap();
        let other_kind = with(
            base.clone(),
            0x0032,
            Sec::Schema(vec![int_field(3, 1, 1, 0)]),
        );
        check_stack(&[("base", &other_kind)], &syms).unwrap();
        let row = |sym: u32, vtype: u8, index: u8, form: u8| Fpromo {
            field_sym: sym,
            slot: 0,
            vtype,
            index,
            form,
        };
        let core: Vec<Fpromo> = [
            (2, 7, 2, 1),
            (3, 5, 2, 0),
            (4, 5, 2, 0),
            (5, 7, 2, 0),
            (6, 7, 2, 0),
            (7, 5, 2, 0),
            (8, 5, 2, 0),
            (9, 2, 1, 0),
            (10, 5, 2, 0),
            (11, 7, 2, 0),
        ]
        .map(|(s, v, i, f)| row(s, v, i, f))
        .to_vec();
        let mut all = vec![row(1, 2, 1, 0)];
        all.extend(core.iter().copied());
        let full = with(promoting.clone(), 0x0061, Sec::Fpromo(all.clone()));
        check_stack(&[("base", &full)], &syms).unwrap();
        let short = with(promoting.clone(), 0x0061, Sec::Fpromo(core.clone()));
        assert!(check_stack(&[("base", &short)], &syms).is_err());
        let mut wrong = all.clone();
        wrong[0].index = 2;
        let wrong = with(promoting.clone(), 0x0061, Sec::Fpromo(wrong));
        assert!(check_stack(&[("base", &wrong)], &syms).is_err());
        // FPROMO where no row holds a promoted value: a base never, an upper segment only beside an FIDX.
        let spare = with(base.clone(), 0x0061, Sec::Fpromo(core.clone()));
        assert!(check_stack(&[("base", &spare)], &syms).is_err());
        let upper = as_delta(spare.clone(), &base);
        let e = check_stack(&[("base", &base), ("d", &upper)], &syms).unwrap_err();
        assert!(e.reason.contains("Presence"), "{e}");
        let changed = with(
            upper,
            0x5001,
            Sec::Fidx(Fidx {
                field_sym: 3,
                vtype: 5,
                form: 1,
                values: vec![],
            }),
        );
        check_stack(&[("base", &base), ("d", &changed)], &syms).unwrap();
        // An upper layer without SCHEMA takes the promoting schema below it.
        let mut delta = as_delta(base.clone(), &full);
        delta.sections.retain(|(e, _)| e.tag != 0x0032);
        let e = check_stack(&[("base", &full), ("d", &delta)], &syms).unwrap_err();
        assert!(
            e.reason.starts_with("d: ") && e.reason.contains("Presence"),
            "{e}"
        );
    }

    /// [F09 §17.1] (spec sync 2b): names resolve through every `SYMTAB` of the set. A branch segment without a `SYMTAB`
    /// whose `SCHEMA` rows are out of order passes alone (nothing resolves) and fails in its set; two `SYMTAB`s that give
    /// one id two strings refuse the set.
    #[test]
    fn set_symbols_resolve_names() {
        let symtab = |v: &[&str]| {
            Sec::Symtab(Symtab {
                classes: vec![(10, 1, v.iter().map(|s| (*s).to_owned()).collect())],
            })
        };
        let base = with(small_base(), 0x0100, symtab(&["estimate", "round", "task"]));
        let mut branch = as_delta(
            with(
                base.clone(),
                0x0032,
                Sec::Schema(vec![int_field(3, 2, 0, 0), int_field(3, 1, 0, 0)]),
            ),
            &base,
        );
        branch.hdr.seg_kind = 5;
        branch.sections.retain(|(e, _)| e.tag != 0x0100);
        let lone = SetSymbols::of([&branch]).unwrap();
        assert_eq!(lone.get(10, 1), None);
        let Some(Sec::Schema(items)) = branch.get(0x0032) else {
            unreachable!("set above")
        };
        check_schema_order(items, |id| lone.get(10, id), 0).unwrap();
        let set = SetSymbols::of([&base, &branch]).unwrap();
        assert!(check_schema_order(items, |id| set.get(10, id), 0).is_err());
        assert_eq!(set.get(10, 3), Some("task"));
        let e = check_stack(&[("base", &base), ("b", &branch)], &set).unwrap_err();
        assert!(e.reason.contains("item key order"), "{e}");
        let clash = with(small_base(), 0x0100, symtab(&["other"]));
        assert!(SetSymbols::of([&base, &clash]).is_err());
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

        /// [F01 §8.1] S4, [F09 §14.2]: `SYMTAB` class entries at and around the greatest id of each class's width
        /// never panic; one decodes exactly when its ids fit the width.
        #[test]
        fn symtab_near_the_width_never_panics(
            class in 1u8..=11,
            first in prop_oneof![
                Just(u32::MAX),
                Just(u32::MAX - 1),
                Just(65_535u32),
                Just(65_534),
                Just(65_536),
                1u32..4,
            ],
            n in 1u32..4,
        ) {
            let mut pool = Writer::new();
            let mut by_id = Writer::new();
            for j in 0..n {
                by_id.u32(pool.len() as u32);
                pool.vstr(&format!("s{j}"));
            }
            let mut w = Writer::new();
            w.u16(1);
            w.u16(0);
            w.u32(pool.len() as u32);
            w.u8(class);
            w.zeros(3);
            w.u32(first);
            w.u32(n);
            w.u32(28);
            w.u32(28 + 4 * n);
            w.bytes(by_id.as_slice());
            (0..n).for_each(|j| w.u32(first.wrapping_add(j)));
            w.bytes(pool.as_slice());
            let max = if crate::log::sym::is_u16(class) { 65_535 } else { u64::from(u32::MAX) };
            let fits = u64::from(first) + u64::from(n) - 1 <= max;
            prop_assert_eq!(decode_symtab(w.as_slice(), 0).is_ok(), fits);
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
            super::tests::reseal(&mut b);
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
}
