//! Measurement 14's F09 segment builder (WP-53e): the segment header `SegHdr` and its section table, the section
//! classes the probe writes, the 60-byte `NodeHdr`, a synthetic base at N nodes and the delta a checkpoint folds from
//! a tail over it.
//!
//! `[F09 §x]` cites `docs/spec/format/09-segments.md` and `[F08 §x]` `08-data-model.md`; every size, offset and code
//! below also names its section and, where the section holds one, its item id, so a reviewer can check it against the
//! chapter (R-24 Q3, as R-74 ruling 10 and R-86 ruling 9 amend it). The writer is a probe's, not the product's: no
//! frozen byte depends on it, but measurement 14 decides T1's layout from it ([F09 Holes], "T1's structure"; [F09]
//! OP-09-01).
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | this one | `SegHdr` with `total_len`, `seg_digest`, `table_xxh3` and `hdr_xxh3`; `SecEnt`; the placement P-1 to P-4; a parser that checks them; `NodeHdr` and `Creator` | [F09 §2.1]–§2.3; [F08 §3.1], [F08 §4] |
//! | [`bitset`] | the frozen bitset with per-chunk cardinality and its AND/popcount, the ± list | [F09 §4.5], [F09 §4.6] |
//! | [`base`] | the synthetic base `seg.base.1` at N: rows, cold columns, forward and reverse CSR, `BMDIR` and its frozen bitsets; the full base of the full checkpoint, with titles, fields, uids, `DOCLEN`, `FTSSTAT` and the `REFS`, `ALLOC`, `TREES` and `FILES` snapshots | [F09 §5], §7.1, §9, §16.1; [AR §8.1] |
//! | [`blobs`] | the `blobs.<n>` file a delta checkpoint seals its window's bodies into: `BLOBIDX` and `BLOBDATA`, each body `lz4` (R-40 O-1) or `none` when lz4 is not shorter; `BlobRef`, `BlobIdx`; the reader | [F10 §3.1]–§3.4, §5.1–§5.4; [F09 §6.3] |
//! | [`full`] | the full delta of a checkpoint: [`full::FullBase`] (the full base's extra sections), [`full::FullFold`] (the night's fold, wrapped), every section a delta of its window requires, and its writer and reader | [F09 §3.1] delta column, §5–§16.2; u21 cut §3.3 |
//! | [`delta`] | the delta `seg.d<K>` a checkpoint folds from a tail over the base: the touched rows with complete states and the ± lists against the base | [F09 §16.2], §5.1, §4.6; [F08 §3.5] |
//! | [`rt`] | [F11 §2.1]'s section body (`RtHdr`, rows, index region, heap); the row sizes and forms of the store-level and runtime sections; the rows of `REFS` with its name index, `IDEM` as a hash table, `ALLOC`, `UIDX`, `TREES`, `FILEOBS` and `FILES`; the bodies with no row; their readers | [F11 §1.3], §2.1–§2.5, §3, §8, §9, §12.4–§12.5; [F09 §14.3]–§14.4 |
//! | [`text`] | tokenizer v1 restricted to ASCII (u20 cut F-5), the term index of a segment's documents, `TERMS` (front-coded in blocks of 16), `POST`, `DOCLEN` and `FTSSTAT` (`HOLE(F09-doclen)`'s "kept" candidate), and their readers | [F09 §12]; [F09 Holes] |
//!
//! **The subset** (spec Goal 1: "the 60-byte `NodeHdr`, CSR, frozen bitsets, cold columns and delta"): the probe writes
//! `IDS`, `NODE`, `CREATOR`, `TOPO`, `DEFER`, `DUE`, `PRIO`, the six CSR columns, `BMDIR` and `BM.<i>` ([F09 §3.1]),
//! and none of the other sections §3.1 requires, so its segments are not valid under V-7 ([F09 §17.1]). Their bytes
//! follow the chapters; what the sections the probe omits would add is outside the hot index measurement 14 times
//! ([AR §8.1]). `NodeHdr.title_off` and `fields_off` are therefore `NONE32` and `body_ref` 0 in every row ([F09 §5.2]).
//! The T1 checkpoint is the exception: there the omitted sections are work a real delta checkpoint does, so
//! `ckpt-4096.n5e5` times a reduced delta, a lower bound, as `m14`'s "What the checkpoint omits" states (review m-4).
//!
//! **The full checkpoint** (u21 cut §3.2–§3.6; R-53 Q2) is the second exception: [`base::build_full`] adds to the
//! subset's base the sections a fold reads (titles, fields, uids, `DOCLEN`, `FTSSTAT` and the store-level snapshots),
//! and [`full`] writes every section §3.1 requires of a delta of its window, with its [`blobs`] file, for
//! `ckpt-full-4096`. Its sections' rows are the probe's (u20 cut F-4 to F-8, u21 cut F-12 and F-17, as `m14` labels
//! them); its base still carries no `TERMS`, `POST`, `STATS` or the other required sections, so it is not valid under
//! V-7 either.

pub mod base;
pub mod bitset;
pub mod blobs;
pub mod delta;
pub mod full;
pub mod rt;
pub mod text;

use crate::tail::store::{SEG_HDR_LEN, SEG_MAGIC};
use xxhash_rust::xxh3::xxh3_64;

/// `SegHdr.format`, format version 1 ([F09 §2.1]).
pub const FORMAT: u16 = 1;
/// `SecEnt`'s length ([F09 §2.2]).
pub const SEC_ENT_LEN: usize = 32;
/// Sections start at multiples of 8 (P-2, [F09 §2.2]).
pub const SECTION_ALIGN: usize = 8;
/// `NONE32` ([F09 §5.2]; [F08 §3.1]).
pub const NONE32: u32 = 0xFFFF_FFFF;
/// `NodeHdr`'s length ([F08 §3.1]).
pub const NODE_HDR_LEN: usize = 60;
/// `Creator`'s length ([F08 §4]; [F09 §5.3]).
pub const CREATOR_LEN: usize = 6;

/// `SegHdr.seg_kind`, the [F11 §2.5] `FileFamily` value ([F09 §2.1]).
pub mod seg_kind {
    /// `seg-base`.
    pub const BASE: u8 = 3;
    /// `seg-delta`.
    pub const DELTA: u8 = 4;
    /// `blobs` ([F10 §5.1]).
    pub const BLOBS: u8 = 6;
}

/// The section tags of [F09 §3.1] (its ranges [F09 §3.2]) and [F10 §5.2] that the probe names, only some of which it
/// writes; [F09 §3.3]'s `derived-optional` set, [`tag::DERIVED_OPTIONAL`]; and the predicates [`tag::listed`] and
/// [`tag::derived_optional`] that the writer's flags and [`SegView::parse`]'s check use.
pub mod tag {
    /// `IDS`, column `u32` ([F09 §3.1]).
    pub const IDS: u16 = 0x0001;
    /// `NODE`, column `NodeHdr` ([F09 §3.1]).
    pub const NODE: u16 = 0x0002;
    /// `CREATOR`, column of 6 B ([F09 §3.1]).
    pub const CREATOR: u16 = 0x0003;
    /// `TOPO`, column `u32` ([F09 §3.1]).
    pub const TOPO: u16 = 0x0004;
    /// `DEFER`, column `u32` ([F09 §3.1]).
    pub const DEFER: u16 = 0x0005;
    /// `DUE`, column `u32` ([F09 §3.1]).
    pub const DUE: u16 = 0x0006;
    /// `UID`, fixed table of 20 B ([F09 §3.1], §5.5).
    pub const UID: u16 = 0x0007;
    /// `PRIO`, column `u8`: the cold column `priority` ([F09 §3.1], [F09 §5.4]; R-66).
    pub const PRIO: u16 = 0x0009;
    /// `TITLE_BLOB`, a pool of `vstr` ([F09 §3.1], §6.1).
    pub const TITLE_BLOB: u16 = 0x0010;
    /// `FIELDS_BLOB`, a pool of `vbytes` ([F09 §3.1], §6.2).
    pub const FIELDS_BLOB: u16 = 0x0011;
    /// `BLOBTAB`, fixed table of 36-B `BlobRef` ([F09 §3.1], §6.3).
    pub const BLOBTAB: u16 = 0x0012;
    /// `OUT_OFF`, column `u32` ([F09 §3.1]).
    pub const OUT_OFF: u16 = 0x0020;
    /// `OUT_DST`, column `u32` ([F09 §3.1]).
    pub const OUT_DST: u16 = 0x0021;
    /// `OUT_KIND`, column `u8` ([F09 §3.1]).
    pub const OUT_KIND: u16 = 0x0022;
    /// `IN_OFF`, column `u32` ([F09 §3.1]).
    pub const IN_OFF: u16 = 0x0023;
    /// `IN_SRC`, column `u32` ([F09 §3.1]).
    pub const IN_SRC: u16 = 0x0024;
    /// `IN_KIND`, column `u8` ([F09 §3.1]).
    pub const IN_KIND: u16 = 0x0025;
    /// `EDGE_PROPS`, fixed table of 40 B ([F09 §3.1], §7.2).
    pub const EDGE_PROPS: u16 = 0x0026;
    /// `TOMB`, fixed table of 16 B ([F09 §3.1], §8.1).
    pub const TOMB: u16 = 0x0030;
    /// `BMDIR`, fixed table of 4-byte rows ([F09 §3.1]).
    pub const BMDIR: u16 = 0x0040;
    /// `TERMS`, the term dictionary ([F09 §3.1]).
    pub const TERMS: u16 = 0x0050;
    /// `POST`, the postings ([F09 §3.1]).
    pub const POST: u16 = 0x0051;
    /// `DOCLEN`, column of 6 B ([F09 §3.1]).
    pub const DOCLEN: u16 = 0x0052;
    /// `FTSSTAT`, fixed record of 48 B ([F09 §3.1]).
    pub const FTSSTAT: u16 = 0x0053;
    /// `CONFLICTS`, an `RtHdr` body ([F09 §3.1]; [F11 §10]).
    pub const CONFLICTS: u16 = 0x0031;
    /// `PATHIDX`, variable table ([F09 §3.1], §13.1).
    pub const PATHIDX: u16 = 0x0080;
    /// `ALIASIDX`, variable table ([F09 §3.1], §13.2).
    pub const ALIASIDX: u16 = 0x0081;
    /// `ANCHORS`, variable table ([F09 §3.1], §13.3).
    pub const ANCHORS: u16 = 0x0082;
    /// `ANCHOR_UID`, fixed table of 28 B ([F09 §3.1], §13.4).
    pub const ANCHOR_UID: u16 = 0x0083;
    /// `GLOBIDX`, an `RtHdr` body ([F09 §3.1]; [F11 §11]).
    pub const GLOBIDX: u16 = 0x0084;
    /// `SYMTAB`, the symbol table ([F09 §3.1], §14.2).
    pub const SYMTAB: u16 = 0x0100;
    /// `SCHEMAIDS`, an `RtHdr` body ([F09 §3.1], §14.3).
    pub const SCHEMAIDS: u16 = 0x0101;
    /// `FILES`, an `RtHdr` body ([F09 §3.1], §14.4).
    pub const FILES: u16 = 0x0102;
    /// `REFS` ([F09 §3.1]).
    pub const REFS: u16 = 0x0201;
    /// `PINS`.
    pub const PINS: u16 = 0x0202;
    /// `HEADS`.
    pub const HEADS: u16 = 0x0203;
    /// `LEASES`.
    pub const LEASES: u16 = 0x0204;
    /// `MARKERS`.
    pub const MARKERS: u16 = 0x0205;
    /// `MARKERS_OLD`.
    pub const MARKERS_OLD: u16 = 0x0206;
    /// `IDEM`.
    pub const IDEM: u16 = 0x0207;
    /// `ALLOC`.
    pub const ALLOC: u16 = 0x0208;
    /// `UIDX`.
    pub const UIDX: u16 = 0x0209;
    /// `CURSORS`.
    pub const CURSORS: u16 = 0x020A;
    /// `SESSMARKS`.
    pub const SESSMARKS: u16 = 0x020B;
    /// `BACKUPS`.
    pub const BACKUPS: u16 = 0x020C;
    /// `DROPPED`, conditional ([F09 §3.1]).
    pub const DROPPED: u16 = 0x020D;
    /// `HARVEST`, conditional ([F09 §3.1]).
    pub const HARVEST: u16 = 0x020E;
    /// `TREES` ([F09 §3.1]).
    pub const TREES: u16 = 0x0210;
    /// `FILEOBS`.
    pub const FILEOBS: u16 = 0x0211;
    /// `PENDING`.
    pub const PENDING: u16 = 0x0212;
    /// `FSINTENT`.
    pub const FSINTENT: u16 = 0x0213;
    /// `FPRINT`, `derived-optional` (§3.3).
    pub const FPRINT: u16 = 0x0214;
    /// `JOURNALCUR`.
    pub const JOURNALCUR: u16 = 0x0215;
    /// `DIRMAP`, `derived-optional`.
    pub const DIRMAP: u16 = 0x0216;
    /// `PREFIXEV`.
    pub const PREFIXEV: u16 = 0x0217;
    /// `ANCESTRY`, `derived-optional`.
    pub const ANCESTRY: u16 = 0x0218;
    /// `GITRENAMES`, `derived-optional`.
    pub const GITRENAMES: u16 = 0x0219;
    /// `ANCHORRES`, `derived-optional`.
    pub const ANCHORRES: u16 = 0x021A;
    /// `BLOBIDX`, a `blobs` file's index ([F09 §3.1]; [F10 §5.2]).
    pub const BLOBIDX: u16 = 0x0310;
    /// `BLOBDATA`, its payloads.
    pub const BLOBDATA: u16 = 0x0311;

    /// `BM.<i>` = `0x8000 + i`, i 0–32,766 ([F09 §3.1], [F09 §3.2]).
    pub const fn bm(i: u16) -> u16 {
        0x8000 + i
    }

    /// The sections that carry `SecEnt.flags` bit 0, `derived-optional`, in format v1: exactly [F09 §3.3]'s seven, in
    /// its order (`DIRMAP`, the four runtime caches, the two tier-2 statistics).
    pub const DERIVED_OPTIONAL: [u16; 7] = [
        DIRMAP, FPRINT, ANCESTRY, GITRENAMES, ANCHORRES, DOCLEN, FTSSTAT,
    ];

    /// Whether `t` is one of [`DERIVED_OPTIONAL`].
    pub const fn derived_optional(t: u16) -> bool {
        let mut i = 0;
        while i < DERIVED_OPTIONAL.len() {
            if DERIVED_OPTIONAL[i] == t {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Whether [F09 §3.1]'s table lists `t`, as a tag or inside a listed range. Tag 0, `0x020F`, `0xFFFF` and every
    /// tag not listed are reserved.
    pub const fn listed(t: u16) -> bool {
        matches!(
            t,
            0x0001..=0x0009
                | 0x0010..=0x0012
                | 0x0020..=0x0026
                | 0x0030..=0x0032
                | 0x0040
                | 0x0050..=0x0053
                | 0x0060..=0x0061
                | 0x0080..=0x0084
                | 0x00C0..=0x00C2
                | 0x0100..=0x0102
                | 0x0201..=0x020E
                | 0x0210..=0x021A
                | 0x0300..=0x0302
                | 0x0310..=0x0311
                | 0x4000..=0x5FFF
                | 0x8000..=0xFFFE
        )
    }
}

/// The `SegHdr` fields a writer chooses ([F09 §2.1]; per kind [F09 §2.3]). `magic`, `format`, `n_sections`,
/// `total_len`, `seg_digest`, `table_xxh3` and `hdr_xxh3` follow from the content.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Header {
    /// `seg_kind` (offset 6).
    pub seg_kind: u8,
    /// `tok_ver` (offset 7): 0, no tier-2 text sections.
    pub tok_ver: u8,
    /// `n_rows` (offset 8).
    pub n_rows: u32,
    /// `file_no` (offset 16).
    pub file_no: u32,
    /// `ref_id` (offset 20).
    pub ref_id: u32,
    /// `dict_no` (offset 24).
    pub dict_no: u32,
    /// `base_seq` (offset 32).
    pub base_seq: u64,
    /// `from_lsn` (offset 40).
    pub from_lsn: u64,
    /// `upto_lsn` (offset 48).
    pub upto_lsn: u64,
    /// `rt_upto_lsn` (offset 56).
    pub rt_upto_lsn: u64,
}

/// One entry of the section table, `SecEnt` ([F09 §2.2]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SecEnt {
    /// `tag` (offset 0).
    pub tag: u16,
    /// `flags` (offset 2): bit 0 `derived-optional`, which [`Writer::push`] sets on exactly [F09 §3.3]'s seven tags
    /// ([`tag::DERIVED_OPTIONAL`]); bits 1–15 are reserved-zero ([F09 §2.2]). [`SegView::parse`] refuses bit 0 on any
    /// other tag [F09 §3.1] lists and any of bits 1–15 ([F09 §17.1] V-7).
    pub flags: u16,
    /// `count` (offset 4): the items the section holds, as its class defines them.
    pub count: u32,
    /// `off` (offset 8): the absolute offset of the section's first byte.
    pub off: u64,
    /// `len` (offset 16).
    pub len: u64,
    /// `xxh3` (offset 24): XXH3-64, seed 0, over `[off, off + len)`.
    pub xxh3: u64,
}

impl SecEnt {
    fn encode(&self, out: &mut [u8]) {
        out[0..2].copy_from_slice(&self.tag.to_le_bytes());
        out[2..4].copy_from_slice(&self.flags.to_le_bytes());
        out[4..8].copy_from_slice(&self.count.to_le_bytes());
        out[8..16].copy_from_slice(&self.off.to_le_bytes());
        out[16..24].copy_from_slice(&self.len.to_le_bytes());
        out[24..32].copy_from_slice(&self.xxh3.to_le_bytes());
    }

    fn decode(b: &[u8]) -> SecEnt {
        SecEnt {
            tag: u16_at(b, 0),
            flags: u16_at(b, 2),
            count: u32_le(b, 4),
            off: u64_at(b, 8),
            len: u64_at(b, 16),
            xxh3: u64_at(b, 24),
        }
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().expect("2 bytes"))
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

/// The little-endian `u64` at byte `at`.
pub(crate) fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

/// Element `i` of a `u32` column ([F09 §4.1]).
pub fn u32_at(col: &[u8], i: usize) -> u32 {
    u32_le(col, 4 * i)
}

/// Appends a `u32` column.
pub fn put_u32s(out: &mut Vec<u8>, v: &[u32]) {
    out.reserve(4 * v.len());
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

/// `data_off = 120 + 32 × n_sections` ([F09 §2.2]).
pub const fn data_off(n_sections: usize) -> usize {
    SEG_HDR_LEN + SEC_ENT_LEN * n_sections
}

/// A segment written section by section in strictly ascending tag order (P-1), each section at the first multiple of
/// 8 at or after the end of the previous one with zero bytes between (P-2), ending at the last section (P-3).
pub struct Writer {
    buf: Vec<u8>,
    table: Vec<SecEnt>,
    n_sections: usize,
}

impl Writer {
    /// A writer for exactly `n_sections` sections and about `capacity` bytes.
    pub fn new(n_sections: usize, capacity: usize) -> Writer {
        let mut buf = Vec::with_capacity(capacity.max(data_off(n_sections)));
        buf.resize(data_off(n_sections), 0);
        Writer {
            buf,
            table: Vec::with_capacity(n_sections),
            n_sections,
        }
    }

    /// Appends the next section: `fill` writes its bytes at the end of the buffer it is given; `count` is its
    /// `SecEnt.count`. Its `SecEnt.flags` is bit 0 on [F09 §3.3]'s seven tags and 0 on every other.
    pub fn push(
        &mut self,
        tag: u16,
        count: u32,
        fill: impl FnOnce(&mut Vec<u8>) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.table.len() == self.n_sections {
            return Err(format!(
                "section {tag:#06x} beyond the {} declared",
                self.n_sections
            ));
        }
        if self.table.last().is_some_and(|e| e.tag >= tag) {
            return Err(format!("section {tag:#06x} out of tag order (P-1)"));
        }
        let off = self.buf.len().next_multiple_of(SECTION_ALIGN);
        self.buf.resize(off, 0);
        fill(&mut self.buf)?;
        self.table.push(SecEnt {
            tag,
            flags: u16::from(tag::derived_optional(tag)),
            count,
            off: off as u64,
            len: (self.buf.len() - off) as u64,
            xxh3: 0,
        });
        Ok(())
    }

    /// Writes the section table, `seg_digest` (BLAKE3-256 over `[data_off, total_len)`), `table_xxh3` and the header
    /// with `hdr_xxh3` ([F09 §2.1]); returns the segment's bytes.
    pub fn finish(mut self, h: &Header) -> Result<Vec<u8>, String> {
        if self.table.len() != self.n_sections {
            return Err(format!(
                "{} sections written, {} declared",
                self.table.len(),
                self.n_sections
            ));
        }
        let d = data_off(self.n_sections);
        for (i, e) in self.table.iter_mut().enumerate() {
            let (off, len) = (e.off as usize, e.len as usize);
            e.xxh3 = xxh3_64(&self.buf[off..off + len]);
            let at = SEG_HDR_LEN + SEC_ENT_LEN * i;
            e.encode(&mut self.buf[at..at + SEC_ENT_LEN]);
        }
        let digest = *blake3::hash(&self.buf[d..]).as_bytes();
        let table = xxh3_64(&self.buf[SEG_HDR_LEN..d]);
        let total = self.buf.len() as u64;
        let n = u16::try_from(self.n_sections).map_err(|_| "more than 65,535 sections")?;
        let b = &mut self.buf;
        b[0..4].copy_from_slice(&SEG_MAGIC);
        b[4..6].copy_from_slice(&FORMAT.to_le_bytes());
        b[6] = h.seg_kind;
        b[7] = h.tok_ver;
        b[8..12].copy_from_slice(&h.n_rows.to_le_bytes());
        b[12..14].copy_from_slice(&n.to_le_bytes());
        b[16..20].copy_from_slice(&h.file_no.to_le_bytes());
        b[20..24].copy_from_slice(&h.ref_id.to_le_bytes());
        b[24..28].copy_from_slice(&h.dict_no.to_le_bytes());
        b[32..40].copy_from_slice(&h.base_seq.to_le_bytes());
        b[40..48].copy_from_slice(&h.from_lsn.to_le_bytes());
        b[48..56].copy_from_slice(&h.upto_lsn.to_le_bytes());
        b[56..64].copy_from_slice(&h.rt_upto_lsn.to_le_bytes());
        b[64..72].copy_from_slice(&total.to_le_bytes());
        b[72..104].copy_from_slice(&digest);
        b[104..112].copy_from_slice(&table.to_le_bytes());
        let sum = xxh3_64(&b[..112]);
        b[112..120].copy_from_slice(&sum.to_le_bytes());
        Ok(self.buf)
    }
}

/// A parsed segment over its bytes: the open checks V-1, V-2, V-4 and V-6, the placement P-1 to P-4 and V-7's `flags`
/// ([F09 §17.1]); [`SegView::check_digests`] adds V-9.
#[derive(Clone, Debug)]
pub struct SegView<'a> {
    /// The segment's bytes.
    pub bytes: &'a [u8],
    /// The header.
    pub header: Header,
    /// The section table.
    pub table: Vec<SecEnt>,
}

impl<'a> SegView<'a> {
    /// Parses and checks `bytes`.
    pub fn parse(bytes: &'a [u8]) -> Result<SegView<'a>, String> {
        if bytes.len() < SEG_HDR_LEN {
            return Err(format!("{} bytes, shorter than SegHdr", bytes.len()));
        }
        if bytes[0..4] != SEG_MAGIC || u16_at(bytes, 4) != FORMAT {
            return Err("not a KSEG segment of format 1 (V-1)".into());
        }
        if u64_at(bytes, 112) != xxh3_64(&bytes[..112]) {
            return Err("hdr_xxh3 does not match (V-2)".into());
        }
        if u64_at(bytes, 64) != bytes.len() as u64 {
            return Err("total_len is not the length (V-4)".into());
        }
        if bytes[14..16] != [0, 0] || bytes[28..32] != [0; 4] {
            return Err("a reserved header field is not zero (V-5)".into());
        }
        let n = usize::from(u16_at(bytes, 12));
        let d = data_off(n);
        if d > bytes.len() || u64_at(bytes, 104) != xxh3_64(&bytes[SEG_HDR_LEN..d]) {
            return Err("the section table is cut or table_xxh3 does not match (V-6)".into());
        }
        let table: Vec<SecEnt> = (0..n)
            .map(|i| SecEnt::decode(&bytes[SEG_HDR_LEN + SEC_ENT_LEN * i..]))
            .collect();
        let mut end = d;
        for (i, e) in table.iter().enumerate() {
            if i > 0 && table[i - 1].tag >= e.tag {
                return Err(format!("tag {:#06x} out of order (P-1)", e.tag));
            }
            // V-7's `flags` ([F09 §3.3]): bits 1-15 reserved-zero; bit 0 on a listed tag only where §3.3 sets it,
            // accepted and never required there, and accepted on a tag §3.1 does not list (§3.3's last item).
            if e.flags & !1 != 0 {
                return Err(format!(
                    "section {:#06x} sets a reserved SecEnt.flags bit (V-7)",
                    e.tag
                ));
            }
            if e.flags & 1 != 0 && tag::listed(e.tag) && !tag::derived_optional(e.tag) {
                return Err(format!(
                    "section {:#06x} is flagged derived-optional outside F09 §3.3 (V-7)",
                    e.tag
                ));
            }
            let off = end.next_multiple_of(SECTION_ALIGN);
            if e.off != off as u64 || bytes[end..off.min(bytes.len())].iter().any(|&b| b != 0) {
                return Err(format!("section {:#06x} is not placed by P-2", e.tag));
            }
            end = off
                .checked_add(e.len as usize)
                .filter(|&x| x <= bytes.len())
                .ok_or_else(|| format!("section {:#06x} runs past the end", e.tag))?;
        }
        if end != bytes.len() {
            return Err("trailing bytes after the last section (P-3)".into());
        }
        Ok(SegView {
            bytes,
            header: Header {
                seg_kind: bytes[6],
                tok_ver: bytes[7],
                n_rows: u32_le(bytes, 8),
                file_no: u32_le(bytes, 16),
                ref_id: u32_le(bytes, 20),
                dict_no: u32_le(bytes, 24),
                base_seq: u64_at(bytes, 32),
                from_lsn: u64_at(bytes, 40),
                upto_lsn: u64_at(bytes, 48),
                rt_upto_lsn: u64_at(bytes, 56),
            },
            table,
        })
    }

    /// The table entry of `tag`.
    pub fn entry(&self, tag: u16) -> Option<&SecEnt> {
        self.table
            .binary_search_by_key(&tag, |e| e.tag)
            .ok()
            .map(|i| &self.table[i])
    }

    /// The bytes of section `tag`.
    pub fn section(&self, tag: u16) -> Option<&'a [u8]> {
        self.entry(tag)
            .map(|e| &self.bytes[e.off as usize..(e.off + e.len) as usize])
    }

    /// The bytes of section `tag`, or an error naming it.
    pub fn need(&self, tag: u16) -> Result<&'a [u8], String> {
        self.section(tag)
            .ok_or_else(|| format!("no section {tag:#06x}"))
    }

    /// V-9: each section's `xxh3` and `seg_digest` ([F09 §17.1]).
    pub fn check_digests(&self) -> Result<(), String> {
        for e in &self.table {
            let s = &self.bytes[e.off as usize..(e.off + e.len) as usize];
            if xxh3_64(s) != e.xxh3 {
                return Err(format!(
                    "section {:#06x}'s xxh3 does not match (V-9)",
                    e.tag
                ));
            }
        }
        let d = data_off(self.table.len());
        if blake3::hash(&self.bytes[d..]).as_bytes()[..] != self.bytes[72..104] {
            return Err("seg_digest does not match (V-9)".into());
        }
        Ok(())
    }
}

/// `NodeHdr.flags` bit 0, `deleted` ([F08 §3.2]).
pub const FLAG_DELETED: u16 = 1;

/// One row of `NODE`: [F08 §3.1]'s `NodeHdr`, 60 bytes, byte-packed, little-endian.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct NodeHdr {
    /// `kind` (offset 0); 0 = no node on this view.
    pub kind: u8,
    /// `status` (1).
    pub status: u8,
    /// `resolution` (2). Byte 3 is `_reserved`, written 0 and not read: `priority` is the cold column `PRIO`
    /// ([F08 §3.1], [F08 §4]; R-66).
    pub resolution: u8,
    /// `criticality` (4), default 2.
    pub criticality: u8,
    /// `confidence` (5), default 0.
    pub confidence: u8,
    /// `authority` (6), default 4.
    pub authority: u8,
    /// `flags` (7, `u16`).
    pub flags: u16,
    /// `rev_seq` (9, `u64`).
    pub rev_seq: u64,
    /// `parent` (17).
    pub parent: u32,
    /// `created_tx` (21).
    pub created_tx: u32,
    /// `updated_tx` (25).
    pub updated_tx: u32,
    /// `last_op_lsn` (29, `u64`).
    pub last_op_lsn: u64,
    /// `open_blockers` (37, `u16`).
    pub open_blockers: u16,
    /// `open_blockers_exo` (39).
    pub open_blockers_exo: u16,
    /// `children_total` (41).
    pub children_total: u16,
    /// `children_done` (43).
    pub children_done: u16,
    /// `title_off` (45).
    pub title_off: u32,
    /// `fields_off` (49).
    pub fields_off: u32,
    /// `body_ref` (53); bytes 57–59 are reserved-zero.
    pub body_ref: u32,
}

/// The offset of `NodeHdr.status` in a row ([F08 §3.1]).
pub const STATUS_AT: usize = 1;

impl NodeHdr {
    /// The row's 60 bytes.
    pub fn encode(&self) -> [u8; NODE_HDR_LEN] {
        let mut b = [0u8; NODE_HDR_LEN];
        b[..7].copy_from_slice(&[
            self.kind,
            self.status,
            self.resolution,
            0,
            self.criticality,
            self.confidence,
            self.authority,
        ]);
        b[7..9].copy_from_slice(&self.flags.to_le_bytes());
        b[9..17].copy_from_slice(&self.rev_seq.to_le_bytes());
        b[17..21].copy_from_slice(&self.parent.to_le_bytes());
        b[21..25].copy_from_slice(&self.created_tx.to_le_bytes());
        b[25..29].copy_from_slice(&self.updated_tx.to_le_bytes());
        b[29..37].copy_from_slice(&self.last_op_lsn.to_le_bytes());
        b[37..39].copy_from_slice(&self.open_blockers.to_le_bytes());
        b[39..41].copy_from_slice(&self.open_blockers_exo.to_le_bytes());
        b[41..43].copy_from_slice(&self.children_total.to_le_bytes());
        b[43..45].copy_from_slice(&self.children_done.to_le_bytes());
        b[45..49].copy_from_slice(&self.title_off.to_le_bytes());
        b[49..53].copy_from_slice(&self.fields_off.to_le_bytes());
        b[53..57].copy_from_slice(&self.body_ref.to_le_bytes());
        b
    }

    /// Decodes a row (the reserved bytes, byte 3 and 57–59, are not tested, as a runtime reader does not,
    /// [F09 §17.2]).
    pub fn decode(b: &[u8]) -> NodeHdr {
        NodeHdr {
            kind: b[0],
            status: b[1],
            resolution: b[2],
            criticality: b[4],
            confidence: b[5],
            authority: b[6],
            flags: u16_at(b, 7),
            rev_seq: u64_at(b, 9),
            parent: u32_le(b, 17),
            created_tx: u32_le(b, 21),
            updated_tx: u32_le(b, 25),
            last_op_lsn: u64_at(b, 29),
            open_blockers: u16_at(b, 37),
            open_blockers_exo: u16_at(b, 39),
            children_total: u16_at(b, 41),
            children_done: u16_at(b, 43),
            title_off: u32_le(b, 45),
            fields_off: u32_le(b, 49),
            body_ref: u32_le(b, 53),
        }
    }

    /// Row `i` of a `NODE` section.
    pub fn row(node: &[u8], i: usize) -> NodeHdr {
        NodeHdr::decode(&node[NODE_HDR_LEN * i..NODE_HDR_LEN * (i + 1)])
    }

    /// Whether the row is a tombstone.
    pub fn deleted(&self) -> bool {
        self.flags & FLAG_DELETED != 0
    }
}

/// One element of `CREATOR` ([F08 §4]; [F09 §5.3]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Creator {
    /// `actor` (offset 0), a symbol of class `actor`.
    pub actor: u32,
    /// `role` (offset 4, `u16`), a symbol of class `role`.
    pub role: u16,
}

impl Creator {
    /// Appends the 6 bytes.
    pub fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.actor.to_le_bytes());
        out.extend_from_slice(&self.role.to_le_bytes());
    }

    /// Element `i` of a `CREATOR` section.
    pub fn at(col: &[u8], i: usize) -> Creator {
        let b = &col[CREATOR_LEN * i..CREATOR_LEN * (i + 1)];
        Creator {
            actor: u32_le(b, 0),
            role: u16_at(b, 4),
        }
    }
}

/// Appends a fixed table ([F09 §4.2]): `n`, `row_w`, then the rows, already sorted.
pub fn put_fixed(out: &mut Vec<u8>, row_w: u32, rows: &[&[u8]]) {
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    out.extend_from_slice(&row_w.to_le_bytes());
    for r in rows {
        out.extend_from_slice(r);
    }
}

/// Appends a variable table ([F09 §4.3]): `n`, `_reserved` 0, `off[n + 1]` with `off[0]` 0,
/// then the rows, already in the table's order; with no row it is 12 bytes. An empty row is refused.
pub fn put_variable(out: &mut Vec<u8>, rows: &[&[u8]]) -> Result<(), String> {
    if rows.iter().any(|r| r.is_empty()) {
        return Err("a variable table holds no empty row".into());
    }
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    let mut off = 0u32;
    out.extend_from_slice(&off.to_le_bytes());
    for r in rows {
        off = off
            .checked_add(r.len() as u32)
            .ok_or("a variable table past 4 GiB")?;
        out.extend_from_slice(&off.to_le_bytes());
    }
    for r in rows {
        out.extend_from_slice(r);
    }
    Ok(())
}

/// The rows of a variable table; refuses a non-zero `_reserved`, `off[0]` ≠ 0, an empty row and a length other than
/// `8 + 4(n + 1) + off[n]`.
pub fn variable_rows(b: &[u8]) -> Result<Vec<&[u8]>, String> {
    if b.len() < 12 || u32_le(b, 4) != 0 || u32_le(b, 8) != 0 {
        return Err("a variable table without its header or off[0] = 0".into());
    }
    let n = u32_le(b, 0) as usize;
    let rows_at = 8 + 4 * (n + 1);
    if b.len() < rows_at {
        return Err(format!("a variable table of {n} rows cut short"));
    }
    let off = |i: usize| u32_le(b, 8 + 4 * i) as usize;
    if b.len() != rows_at + off(n) || (0..n).any(|i| off(i + 1) <= off(i)) {
        return Err(format!("a variable table of {n} rows breaks its offsets"));
    }
    Ok((0..n)
        .map(|i| &b[rows_at + off(i)..rows_at + off(i + 1)])
        .collect())
}

/// The rows of a fixed table whose rows are `row_w` bytes wide.
pub fn fixed_rows(b: &[u8], row_w: u32) -> Result<Vec<&[u8]>, String> {
    if b.len() < 8 || u32_le(b, 4) != row_w {
        return Err(format!("a fixed table without its {row_w}-byte row width"));
    }
    let n = u32_le(b, 0) as usize;
    let w = row_w as usize;
    if b.len() != 8 + n * w {
        return Err(format!("a fixed table of {n} rows is {} bytes", b.len()));
    }
    Ok((0..n).map(|i| &b[8 + i * w..8 + (i + 1) * w]).collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tail::store::check_seg_header;

    /// `seg` with entry `i`'s `SecEnt.flags` set to `flags`, its `table_xxh3` and `hdr_xxh3` recomputed, so only the
    /// flags differ to a parser.
    pub(crate) fn with_flags(seg: &[u8], i: usize, flags: u16) -> Vec<u8> {
        let mut b = seg.to_vec();
        let at = SEG_HDR_LEN + SEC_ENT_LEN * i + 2;
        b[at..at + 2].copy_from_slice(&flags.to_le_bytes());
        let n = usize::from(u16::from_le_bytes([b[12], b[13]]));
        let table = xxh3_64(&b[SEG_HDR_LEN..SEG_HDR_LEN + SEC_ENT_LEN * n]);
        b[104..112].copy_from_slice(&table.to_le_bytes());
        let sum = xxh3_64(&b[..112]);
        b[112..120].copy_from_slice(&sum.to_le_bytes());
        b
    }

    fn header() -> Header {
        Header {
            seg_kind: seg_kind::DELTA,
            tok_ver: 0,
            n_rows: 0x0102_0304,
            file_no: 0x1112_1314,
            ref_id: 0,
            dict_no: 0,
            base_seq: 0x2122_2324_2526_2728,
            from_lsn: 0x3132_3334_3536_3738,
            upto_lsn: 0x4142_4344_4546_4748,
            rt_upto_lsn: 0x5152_5354_5556_5758,
        }
    }

    /// Writes sections of 3, 0, 13 and 8 bytes with tags 0x0002, 0x0003, 0x0020 and 0x8000.
    fn four() -> Vec<u8> {
        let mut w = Writer::new(4, 0);
        for (tag, len) in [(0x0002u16, 3usize), (0x0003, 0), (0x0020, 13), (0x8000, 8)] {
            w.push(tag, len as u32, |o| {
                o.extend((0..len).map(|i| 0xA0 + i as u8));
                Ok(())
            })
            .unwrap();
        }
        w.finish(&header()).unwrap()
    }

    /// `SegHdr` ([F09 §2.1]): 120 bytes, every field at its offset, `hdr_xxh3` over
    /// `[0, 112)`, `table_xxh3` over `[120, data_off)`, `seg_digest` BLAKE3-256 over `[data_off, total_len)`; u10's
    /// open-floor check reads the same header.
    #[test]
    fn seg_hdr_fields_sit_at_f09_offsets() {
        let b = four();
        let d = data_off(4);
        assert_eq!((SEG_HDR_LEN, d), (120, 120 + 4 * 32));
        assert_eq!(&b[0..4], b"KSEG");
        assert_eq!(&b[4..6], &[1, 0], "format 1");
        assert_eq!((b[6], b[7]), (4, 0), "seg_kind seg-delta, tok_ver 0");
        assert_eq!(&b[8..12], &[4, 3, 2, 1], "n_rows");
        assert_eq!(&b[12..14], &[4, 0], "n_sections");
        assert_eq!(&b[14..16], &[0, 0], "reserved");
        assert_eq!(&b[16..20], &[0x14, 0x13, 0x12, 0x11], "file_no");
        assert_eq!(&b[20..32], &[0; 12], "ref_id, dict_no, reserved");
        assert_eq!(b[32], 0x28, "base_seq");
        assert_eq!(b[40], 0x38, "from_lsn");
        assert_eq!(b[48], 0x48, "upto_lsn");
        assert_eq!(b[56], 0x58, "rt_upto_lsn");
        assert_eq!(u64_at(&b, 64), b.len() as u64, "total_len");
        assert_eq!(&b[72..104], blake3::hash(&b[d..]).as_bytes(), "seg_digest");
        assert_eq!(u64_at(&b, 104), xxh3_64(&b[120..d]), "table_xxh3");
        assert_eq!(u64_at(&b, 112), xxh3_64(&b[..112]), "hdr_xxh3");
        let h: [u8; 120] = b[..120].try_into().unwrap();
        assert_eq!(check_seg_header(&h), Ok(b.len() as u64));
        let v = SegView::parse(&b).unwrap();
        assert_eq!(v.header, header());
        v.check_digests().unwrap();
    }

    /// `SecEnt` ([F09 §2.2]): 32 bytes, little-endian, `tag` at 0 (2 B), `flags` at 2 (2 B),
    /// `count` at 4 (4 B), `off` at 8 (8 B), `len` at 16 (8 B) and `xxh3` at 24 (8 B). A hand-built entry with a
    /// distinct byte per position encodes to its literal and decodes back, and in `four()`'s segment entry 2 (tag
    /// 0x0020, 13 bytes at `data_off + 8`) holds each field at its offset; a writer and a reader that agreed on other
    /// offsets would still round-trip, so only these pins tie the entry to the chapter (review m-1).
    #[test]
    fn sec_ent_fields_sit_at_f09_offsets() {
        let e = SecEnt {
            tag: 0x0201,
            flags: 0x0403,
            count: 0x0807_0605,
            off: 0x100F_0E0D_0C0B_0A09,
            len: 0x1817_1615_1413_1211,
            xxh3: 0x201F_1E1D_1C1B_1A19,
        };
        let mut b = [0u8; SEC_ENT_LEN];
        e.encode(&mut b);
        assert_eq!(b.to_vec(), (1..=32).collect::<Vec<u8>>());
        assert_eq!(SecEnt::decode(&b), e);
        let seg = four();
        let d = data_off(4) as u64;
        let at = SEG_HDR_LEN + 2 * SEC_ENT_LEN;
        let raw = &seg[at..at + SEC_ENT_LEN];
        assert_eq!(&raw[0..2], &0x0020u16.to_le_bytes(), "tag at 0");
        assert_eq!(&raw[2..4], &[0, 0], "flags at 2");
        assert_eq!(&raw[4..8], &13u32.to_le_bytes(), "count at 4");
        assert_eq!(&raw[8..16], &(d + 8).to_le_bytes(), "off at 8");
        assert_eq!(&raw[16..24], &13u64.to_le_bytes(), "len at 16");
        let section = &seg[d as usize + 8..d as usize + 21];
        assert_eq!(&raw[24..32], &xxh3_64(section).to_le_bytes(), "xxh3 at 24");
    }

    /// The section table and placement ([F09 §2.2]): 32-byte entries sorted by tag; each section at the
    /// first multiple of 8 at or after the previous one's end, zeros between, `total_len` at the last one's end; the
    /// entry's `xxh3` covers the section. Out-of-order or surplus sections are refused, and the parser refuses a
    /// changed header, table, length or padding byte, while V-9 catches a changed section byte.
    #[test]
    fn sections_are_placed_by_p1_to_p4() {
        let b = four();
        let v = SegView::parse(&b).unwrap();
        let d = data_off(4) as u64;
        let placed: Vec<(u16, u64, u64)> = v.table.iter().map(|e| (e.tag, e.off, e.len)).collect();
        assert_eq!(
            placed,
            [
                (0x0002, d, 3),
                (0x0003, d + 8, 0),
                (0x0020, d + 8, 13),
                (0x8000, d + 24, 8)
            ]
        );
        assert_eq!(b.len() as u64, d + 32, "P-3: no trailing byte");
        assert_eq!(&b[d as usize + 3..d as usize + 8], &[0; 5], "P-2 padding");
        assert_eq!(&b[d as usize + 21..d as usize + 24], &[0; 3]);
        for e in &v.table {
            assert_eq!(e.flags, 0);
            assert_eq!(e.count as u64, e.len);
            assert_eq!(e.xxh3, xxh3_64(v.section(e.tag).unwrap()));
        }
        assert_eq!(v.section(0x0020).unwrap()[0], 0xA0);
        assert!(v.section(0x0004).is_none());
        // The writer's refusals.
        let mut w = Writer::new(1, 0);
        w.push(0x0003, 0, |_| Ok(())).unwrap();
        assert!(
            w.push(0x0004, 0, |_| Ok(()))
                .unwrap_err()
                .contains("beyond")
        );
        let mut w = Writer::new(2, 0);
        w.push(0x0003, 0, |_| Ok(())).unwrap();
        assert!(w.push(0x0003, 0, |_| Ok(())).unwrap_err().contains("P-1"));
        assert!(
            w.finish(&header())
                .unwrap_err()
                .contains("1 sections written")
        );
        // The parser's refusals: one changed byte each.
        let flip = |at: usize| {
            let mut c = b.clone();
            c[at] ^= 1;
            c
        };
        assert!(SegView::parse(&flip(0)).unwrap_err().contains("V-1"));
        assert!(SegView::parse(&flip(40)).unwrap_err().contains("V-2"));
        assert!(SegView::parse(&flip(130)).unwrap_err().contains("V-6"));
        assert!(
            SegView::parse(&b[..b.len() - 1])
                .unwrap_err()
                .contains("V-4")
        );
        let mut padded = b.clone();
        padded[d as usize + 4] = 1;
        assert!(SegView::parse(&padded).unwrap_err().contains("P-2"));
        let changed = flip(d as usize + 9);
        let v = SegView::parse(&changed).unwrap();
        assert!(v.check_digests().unwrap_err().contains("0x0020"));
    }

    /// `NodeHdr` ([F08 §3.1]): 60 bytes, each field at its offset, the reserved byte 3 (R-66: `priority` is the `PRIO`
    /// column) and bytes 57–59 zero, decoded back, and a row whose byte 3 is not zero decodes to the same header;
    /// `Creator` 6 bytes ([F08 §4]).
    #[test]
    fn node_hdr_is_60_bytes_at_f08_offsets() {
        let h = NodeHdr {
            kind: 1,
            status: 2,
            resolution: 3,
            criticality: 5,
            confidence: 6,
            authority: 7,
            flags: 0x0908,
            rev_seq: 0x1716_1514_1312_1110,
            parent: 0x2120_1918,
            created_tx: 0x2524_2322,
            updated_tx: 0x2928_2726,
            last_op_lsn: 0x3332_3130_2F2E_2D2C,
            open_blockers: 0x3534,
            open_blockers_exo: 0x3736,
            children_total: 0x3938,
            children_done: 0x3B3A,
            title_off: 0x3F3E_3D3C,
            fields_off: 0x4342_4140,
            body_ref: 0x4746_4544,
        };
        let b = h.encode();
        assert_eq!(b.len(), 60);
        let mut want: Vec<u8> = vec![1, 2, 3, 0, 5, 6, 7];
        want.extend([
            0x08, 0x09, // flags at 7
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, // rev_seq at 9
            0x18, 0x19, 0x20, 0x21, // parent at 17
            0x22, 0x23, 0x24, 0x25, // created_tx at 21
            0x26, 0x27, 0x28, 0x29, // updated_tx at 25
            0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, // last_op_lsn at 29
            0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, // the four counters at 37
            0x3C, 0x3D, 0x3E, 0x3F, // title_off at 45
            0x40, 0x41, 0x42, 0x43, // fields_off at 49
            0x44, 0x45, 0x46, 0x47, // body_ref at 53
            0, 0, 0, // reserved at 57
        ]);
        assert_eq!(b.to_vec(), want);
        assert_eq!(NodeHdr::decode(&b), h);
        let mut other = b;
        other[3] = 4;
        assert_eq!(NodeHdr::decode(&other), h, "byte 3 is not read");
        assert_eq!(b[STATUS_AT], h.status);
        let mut col = vec![0u8; NODE_HDR_LEN];
        col.extend_from_slice(&b);
        assert_eq!(NodeHdr::row(&col, 1), h);
        assert!(!h.deleted() && NodeHdr { flags: 1, ..h }.deleted());
        let mut c = Vec::new();
        Creator {
            actor: 0x0403_0201,
            role: 0x0605,
        }
        .put(&mut c);
        assert_eq!(c, [1, 2, 3, 4, 5, 6]);
        assert_eq!(
            Creator::at(&c, 0),
            Creator {
                actor: 0x0403_0201,
                role: 0x0605
            }
        );
    }

    /// `SecEnt.flags` as [F09 §3.3] and V-7 ([F09 §17.1]) give them: over a segment of `NODE` (0x0002), an unlisted tag
    /// (0x000A), `DOCLEN` (0x0052) and `FTSSTAT` (0x0053), the parser refuses bit 0 on `NODE`, any of bits 1-15 on a tag
    /// and bit 1 beside bit 0 on `DOCLEN`, and accepts bit 0 on `DOCLEN` and `FTSSTAT` (never requiring it: cleared is
    /// accepted too) and on a tag §3.1 does not list (§3.3's last item: a format-1 reader ignores such a section).
    #[test]
    fn sec_ent_flags_follow_f09_3_3() {
        let mut w = Writer::new(4, 0);
        for t in [0x0002u16, 0x000A, 0x0052, 0x0053] {
            w.push(t, 1, |o| {
                o.push(0xA5);
                Ok(())
            })
            .unwrap();
        }
        let seg = w.finish(&header()).unwrap();
        let refused = [
            (0, 1u16, "0x0002"),
            (0, 2, "0x0002"),
            (2, 3, "0x0052"),
            (3, 0x8000, "0x0053"),
            (1, 2, "0x000a"),
        ];
        for (i, flags, name) in refused {
            let b = with_flags(&seg, i, flags);
            let r = SegView::parse(&b).map(|_| ());
            assert!(
                r.as_ref()
                    .is_err_and(|e| e.contains(name) && e.contains("V-7")),
                "parse accepted flags {flags:#06x} on {name}: {r:?}"
            );
        }
        for (i, flags) in [(2, 1u16), (3, 1), (2, 0), (3, 0), (1, 1)] {
            let b = with_flags(&seg, i, flags);
            let v = SegView::parse(&b).unwrap_or_else(|e| panic!("entry {i} flags {flags}: {e}"));
            assert_eq!(v.table[i].flags, flags);
        }
    }

    /// The tags the probe writes are [F09 §3.1]'s (its runtime ranges among them; `BM.<i>` [F09 §3.2]) and
    /// [F10 §5.2]'s, and `seg_kind` [F11 §2.5]'s `FileFamily`: every constant against the chapters' tables, written out.
    #[test]
    fn section_tags_follow_f09_3_1() {
        let table: [(&str, u16, u16); 60] = [
            ("IDS", tag::IDS, 0x0001),
            ("NODE", tag::NODE, 0x0002),
            ("CREATOR", tag::CREATOR, 0x0003),
            ("TOPO", tag::TOPO, 0x0004),
            ("DEFER", tag::DEFER, 0x0005),
            ("DUE", tag::DUE, 0x0006),
            ("UID", tag::UID, 0x0007),
            ("PRIO", tag::PRIO, 0x0009),
            ("TITLE_BLOB", tag::TITLE_BLOB, 0x0010),
            ("FIELDS_BLOB", tag::FIELDS_BLOB, 0x0011),
            ("BLOBTAB", tag::BLOBTAB, 0x0012),
            ("OUT_OFF", tag::OUT_OFF, 0x0020),
            ("OUT_DST", tag::OUT_DST, 0x0021),
            ("OUT_KIND", tag::OUT_KIND, 0x0022),
            ("IN_OFF", tag::IN_OFF, 0x0023),
            ("IN_SRC", tag::IN_SRC, 0x0024),
            ("IN_KIND", tag::IN_KIND, 0x0025),
            ("EDGE_PROPS", tag::EDGE_PROPS, 0x0026),
            ("TOMB", tag::TOMB, 0x0030),
            ("CONFLICTS", tag::CONFLICTS, 0x0031),
            ("BMDIR", tag::BMDIR, 0x0040),
            ("TERMS", tag::TERMS, 0x0050),
            ("POST", tag::POST, 0x0051),
            ("DOCLEN", tag::DOCLEN, 0x0052),
            ("FTSSTAT", tag::FTSSTAT, 0x0053),
            ("PATHIDX", tag::PATHIDX, 0x0080),
            ("ALIASIDX", tag::ALIASIDX, 0x0081),
            ("ANCHORS", tag::ANCHORS, 0x0082),
            ("ANCHOR_UID", tag::ANCHOR_UID, 0x0083),
            ("GLOBIDX", tag::GLOBIDX, 0x0084),
            ("SYMTAB", tag::SYMTAB, 0x0100),
            ("SCHEMAIDS", tag::SCHEMAIDS, 0x0101),
            ("FILES", tag::FILES, 0x0102),
            ("REFS", tag::REFS, 0x0201),
            ("PINS", tag::PINS, 0x0202),
            ("HEADS", tag::HEADS, 0x0203),
            ("LEASES", tag::LEASES, 0x0204),
            ("MARKERS", tag::MARKERS, 0x0205),
            ("MARKERS_OLD", tag::MARKERS_OLD, 0x0206),
            ("IDEM", tag::IDEM, 0x0207),
            ("ALLOC", tag::ALLOC, 0x0208),
            ("UIDX", tag::UIDX, 0x0209),
            ("CURSORS", tag::CURSORS, 0x020A),
            ("SESSMARKS", tag::SESSMARKS, 0x020B),
            ("BACKUPS", tag::BACKUPS, 0x020C),
            ("DROPPED", tag::DROPPED, 0x020D),
            ("HARVEST", tag::HARVEST, 0x020E),
            ("TREES", tag::TREES, 0x0210),
            ("FILEOBS", tag::FILEOBS, 0x0211),
            ("PENDING", tag::PENDING, 0x0212),
            ("FSINTENT", tag::FSINTENT, 0x0213),
            ("FPRINT", tag::FPRINT, 0x0214),
            ("JOURNALCUR", tag::JOURNALCUR, 0x0215),
            ("DIRMAP", tag::DIRMAP, 0x0216),
            ("PREFIXEV", tag::PREFIXEV, 0x0217),
            ("ANCESTRY", tag::ANCESTRY, 0x0218),
            ("GITRENAMES", tag::GITRENAMES, 0x0219),
            ("ANCHORRES", tag::ANCHORRES, 0x021A),
            ("BLOBIDX", tag::BLOBIDX, 0x0310),
            ("BLOBDATA", tag::BLOBDATA, 0x0311),
        ];
        for (name, got, want) in table {
            assert_eq!(got, want, "{name}: {got:#06x}, F09 gives {want:#06x}");
        }
        assert_eq!((tag::bm(0), tag::bm(32_766)), (0x8000, 0xFFFE));
        // [F09 §3.1]'s listed tags and ranges, at each edge, and the reserved tags between them.
        let listed: [(u16, u16); 16] = [
            (0x0001, 0x0009),
            (0x0010, 0x0012),
            (0x0020, 0x0026),
            (0x0030, 0x0032),
            (0x0040, 0x0040),
            (0x0050, 0x0053),
            (0x0060, 0x0061),
            (0x0080, 0x0084),
            (0x00C0, 0x00C2),
            (0x0100, 0x0102),
            (0x0201, 0x020E),
            (0x0210, 0x021A),
            (0x0300, 0x0302),
            (0x0310, 0x0311),
            (0x4000, 0x5FFF),
            (0x8000, 0xFFFE),
        ];
        for (a, b) in listed {
            assert!(tag::listed(a) && tag::listed(b), "{a:#06x}..={b:#06x}");
            assert!(
                !tag::listed(a - 1) && !tag::listed(b + 1),
                "around {a:#06x}..={b:#06x}"
            );
        }
        for t in [0x0000, 0x000A, 0x020F, 0x0303, 0x6000, 0x7FFF, 0xFFFF] {
            assert!(!tag::listed(t), "{t:#06x} is reserved");
        }
        for (name, t, _) in table {
            assert!(tag::listed(t), "{name}");
        }
        // [F09 §3.3]: `derived-optional` on exactly seven tags.
        let flagged: Vec<u16> = (0..=u16::MAX)
            .filter(|&t| tag::derived_optional(t))
            .collect();
        assert_eq!(
            flagged,
            [0x0052, 0x0053, 0x0214, 0x0216, 0x0218, 0x0219, 0x021A],
            "DOCLEN, FTSSTAT, FPRINT, DIRMAP, ANCESTRY, GITRENAMES, ANCHORRES"
        );
        assert_eq!(
            (seg_kind::BASE, seg_kind::DELTA, seg_kind::BLOBS),
            (3, 4, 6)
        );
        // Variable tables ([F09 §4.3]): with no row 12 bytes; rows after `off`.
        let mut b = Vec::new();
        put_variable(&mut b, &[]).unwrap();
        assert_eq!(b, [0; 12]);
        let mut b = Vec::new();
        put_variable(&mut b, &[b"ab", b"c"]).unwrap();
        assert_eq!(
            b,
            [
                2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, b'a', b'b', b'c'
            ]
        );
        assert_eq!(variable_rows(&b).unwrap(), [&b"ab"[..], &b"c"[..]]);
        assert!(put_variable(&mut Vec::new(), &[b""]).is_err());
    }
}
