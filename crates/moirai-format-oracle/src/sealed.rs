//! [F10] sealed files other than graph segments: `hist` (§4) and `blobs` (§5) files, which share [F09]'s `SegHdr` and
//! section table, and `dict.<D>` (§6) and `gitmap.<n>` (§7) files with their own headers. Compressed payloads (codec
//! values other than 0) stay opaque at M0 ([PLAN §6.2] R3); a codec-0 payload is decoded and hashed.

use crate::holes;
use crate::lock::check_format;
use crate::log::{Payload, Record};
use crate::prim::{Algo, Reader, Result, Writer, blake3_128, err, xxh3_64};
use crate::segment::{Container, SecEnt, SegHdr, decode_container, encode_container};

/// A `hist` frame header ([F10 §4.3]), 88 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHdr {
    /// First record's lsn.
    pub first_lsn: u64,
    /// Last record's lsn.
    pub last_lsn: u64,
    /// First commit's `seq`, 0 if none.
    pub first_seq: u64,
    /// Last commit's `seq`, 0 if none.
    pub last_seq: u64,
    /// First commit's `append_hlc`.
    pub first_append_hlc: u64,
    /// Last commit's `append_hlc`.
    pub last_append_hlc: u64,
    /// Absolute offset of the block table.
    pub off: u64,
    /// Bytes in `HDATA`.
    pub stored_len: u32,
    /// Raw bytes.
    pub raw_len: u32,
    /// Records, ≥ 1.
    pub n_records: u32,
    /// `Commit` records.
    pub n_commits: u32,
    /// Blocks, ≥ 1.
    pub n_blocks: u16,
    /// Codec of every block.
    pub codec: u8,
    /// XXH3-64 of the raw bytes.
    pub raw_xxh3: u64,
}

/// One frame: its header, its blocks (raw length, stored payload) and, for codec 0, its records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The header.
    pub hdr: FrameHdr,
    /// (raw_len, payload) per block.
    pub blocks: Vec<(u32, Vec<u8>)>,
    /// The decoded records of a codec-0 frame.
    pub records: Option<Vec<Record>>,
}

/// A `hist` file ([F10 §4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hist {
    /// The header.
    pub hdr: SegHdr,
    /// The frames.
    pub frames: Vec<Frame>,
    /// `HCIDX` entries (id16, lsn).
    pub index: Vec<([u8; 16], u64)>,
}

fn exact_tags(c: &Container<'_>, tags: &[u16], what: &str) -> Result<()> {
    let got: Vec<u16> = c.entries.iter().map(|e| e.tag).collect();
    if got != tags || c.entries.iter().any(|e| e.flags != 0) {
        return err(
            120,
            format!(
                "a {what} file holds sections {got:04X?}, not exactly {tags:04X?} [F10 §4.3, §5.2]"
            ),
        );
    }
    Ok(())
}

/// Decodes a `hist` file with the open and full checks of [F10 §9].
pub fn decode_hist(b: &[u8]) -> Result<Hist> {
    let c = decode_container(b)?;
    if c.hdr.seg_kind != 2 {
        return err(6, "not a hist file (seg_kind 2) [F10 §4.3]");
    }
    exact_tags(&c, &[0x0300, 0x0301, 0x0302], "hist")?;
    let (fb, f_at) = c.sections[0];
    let (ib, i_at) = c.sections[1];
    let (db, d_at) = c.sections[2];
    let mut r = Reader::with_base(fb, f_at);
    let n = r.u32()? as usize;
    if r.u32()? != 88 || fb.len() != 8 + 88 * n {
        return err(
            f_at,
            "HFRAMES row_w is not 88, or its length breaks §4.2 of [F09]",
        );
    }
    let mut hdrs = Vec::with_capacity(n);
    for _ in 0..n {
        let h_at = r.offset();
        let h = FrameHdr {
            first_lsn: r.u64()?,
            last_lsn: r.u64()?,
            first_seq: r.u64()?,
            last_seq: r.u64()?,
            first_append_hlc: r.u64()?,
            last_append_hlc: r.u64()?,
            off: r.u64()?,
            stored_len: r.u32()?,
            raw_len: r.u32()?,
            n_records: r.u32()?,
            n_commits: r.u32()?,
            n_blocks: r.u16()?,
            codec: r.u8()?,
            raw_xxh3: {
                r.zeros(5, "FrameHdr._reserved")?;
                r.u64()?
            },
        };
        if h.n_records == 0
            || h.n_blocks == 0
            || h.codec > holes::CODEC_MAX
            || matches!(h.codec, 2 | 4)
        {
            return err(
                h_at,
                "FrameHdr n_records or n_blocks 0, or a codec outside the registry or with a dictionary [F10 §4.3]",
            );
        }
        hdrs.push(h);
    }
    for w in hdrs.windows(2) {
        if w[1].first_lsn <= w[0].last_lsn {
            return err(
                f_at,
                "hist frames not ascending and disjoint in lsn [F10 §4.3]",
            );
        }
    }
    let mut frames = Vec::with_capacity(n);
    let mut pos = 0usize;
    for h in hdrs {
        if h.off != (d_at + pos) as u64 {
            return err(
                d_at + pos,
                "a frame's off does not chain from the previous frame [F10 §4.3]",
            );
        }
        let mut fr = Reader::with_base(&db[pos.min(db.len())..], d_at + pos);
        let mut table = Vec::with_capacity(usize::from(h.n_blocks));
        for _ in 0..h.n_blocks {
            let (s, raw) = (fr.u32()?, fr.u32()?);
            if raw == 0 || raw > 1_048_576 {
                return err(fr.offset(), "a block raw_len outside 1-1048576 [F10 §4.3]");
            }
            table.push((s, raw));
        }
        let stored: u64 =
            8 * u64::from(h.n_blocks) + table.iter().map(|t| u64::from(t.0)).sum::<u64>();
        let raw: u64 = table.iter().map(|t| u64::from(t.1)).sum();
        if stored != u64::from(h.stored_len) || raw != u64::from(h.raw_len) {
            return err(
                d_at + pos,
                "block table does not sum to the FrameHdr lengths [F10 §4.3]",
            );
        }
        let mut blocks = Vec::with_capacity(table.len());
        for (s, raw) in &table {
            let p = fr.bytes(*s as usize)?;
            if h.codec == 0 && p.len() != *raw as usize {
                return err(
                    fr.offset(),
                    "a codec-0 block's stored length differs from its raw length [F10 §4.3]",
                );
            }
            blocks.push((*raw, p.to_vec()));
        }
        pos += stored as usize;
        let records = if h.codec == 0 {
            let raw_bytes: Vec<u8> = blocks.iter().flat_map(|(_, p)| p.iter().copied()).collect();
            if xxh3_64(&raw_bytes) != h.raw_xxh3 {
                return err(d_at, "a frame's raw_xxh3 mismatch [F10 §4.3]");
            }
            Some(decode_frame_records(&raw_bytes, &h, d_at)?)
        } else {
            None
        };
        frames.push(Frame {
            hdr: h,
            blocks,
            records,
        });
    }
    if pos != db.len() {
        return err(
            d_at + pos,
            "HDATA has bytes after the last frame [F10 §4.3]",
        );
    }
    let index = decode_hcidx(ib, i_at)?;
    let h = Hist {
        hdr: c.hdr,
        frames,
        index,
    };
    check_hist(&h, &c.entries, &c.sections)?;
    Ok(h)
}

fn decode_frame_records(raw: &[u8], h: &FrameHdr, at: usize) -> Result<Vec<Record>> {
    let mut off = 0;
    let mut recs: Vec<Record> = Vec::new();
    while off < raw.len() {
        let (rec, n) = Record::decode_detached(&raw[off..], at).map_err(|e| match e {
            crate::log::RecError::Invalid(e) | crate::log::RecError::Malformed(e) => e,
        })?;
        if recs
            .last()
            .is_some_and(|p| p.hdr.lsn + u64::from(p.hdr.len) > rec.hdr.lsn)
        {
            return err(
                at,
                "hist records not in ascending, disjoint lsn order [F10 §4.1]",
            );
        }
        off += n;
        recs.push(rec);
    }
    let commits: Vec<&crate::commit::Commit> = recs
        .iter()
        .filter_map(|r| match &r.payload {
            Payload::Commit(c) => Some(c.as_ref()),
            _ => None,
        })
        .collect();
    let want = (
        recs.first().map_or(0, |r| r.hdr.lsn),
        recs.last().map_or(0, |r| r.hdr.lsn),
        commits.first().map_or(0, |c| c.seq),
        commits.last().map_or(0, |c| c.seq),
        commits.first().map_or(0, |c| c.append_hlc()),
        commits.last().map_or(0, |c| c.append_hlc()),
        recs.len() as u32,
        commits.len() as u32,
    );
    let got = (
        h.first_lsn,
        h.last_lsn,
        h.first_seq,
        h.last_seq,
        h.first_append_hlc,
        h.last_append_hlc,
        h.n_records,
        h.n_commits,
    );
    if want != got {
        return err(
            at,
            "FrameHdr bounds or counts differ from its records [F10 §9]",
        );
    }
    Ok(recs)
}

fn decode_hcidx(b: &[u8], at: usize) -> Result<Vec<([u8; 16], u64)>> {
    if b.len() < 1024 || !(b.len() - 1024).is_multiple_of(24) {
        return err(at, "HCIDX length is not 1024 + 24n [F10 §4.4]");
    }
    let mut r = Reader::with_base(b, at);
    let mut fan = [0u32; 256];
    for f in &mut fan {
        *f = r.u32()?;
    }
    let n = (b.len() - 1024) / 24;
    let mut v: Vec<([u8; 16], u64)> = Vec::with_capacity(n);
    for _ in 0..n {
        let e = (r.b16()?, r.u64()?);
        if v.last().is_some_and(|p| *p >= e) {
            return err(
                r.offset(),
                "HCIDX entries not strictly ascending by (id16, lsn) [F10 §4.4]",
            );
        }
        v.push(e);
    }
    check_fan(&fan, v.iter().map(|e| e.0[0]), at)?;
    Ok(v)
}

/// A fan-out table's rule ([F10 §4.4], §7.3): `fan[b]` counts entries whose first byte ≤ b.
fn check_fan(fan: &[u32; 256], firsts: impl Iterator<Item = u8>, at: usize) -> Result<()> {
    let mut want = [0u32; 256];
    for f in firsts {
        want[usize::from(f)] += 1;
    }
    for i in 1..256 {
        want[i] += want[i - 1];
    }
    if *fan != want {
        return err(
            at,
            "fan-out table inconsistent with the entries [F10 §4.4, §7.3]",
        );
    }
    Ok(())
}

/// The cross-section rules of a `hist` file; failures name the section (or `SegHdr` field) they are about.
fn check_hist(h: &Hist, entries: &[SecEnt], sections: &[(&[u8], usize)]) -> Result<()> {
    let n_commits: u64 = h.frames.iter().map(|f| u64::from(f.hdr.n_commits)).sum();
    if entries[0].count as usize != h.frames.len()
        || entries[2].count as usize != h.frames.len()
        || u64::from(entries[1].count) != n_commits
        || h.index.len() as u64 != n_commits
        || u64::from(h.hdr.n_rows) != n_commits
    {
        return err(
            sections[0].1,
            "hist counts: HFRAMES, HDATA frames, HCIDX commits and n_rows disagree [F10 §4.3]",
        );
    }
    if h.frames.iter().all(|f| f.records.is_some()) {
        let recs: Vec<&Record> = h
            .frames
            .iter()
            .flat_map(|f| f.records.as_ref().expect("codec 0"))
            .collect();
        let mut idx: Vec<([u8; 16], u64)> = recs
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(c) => {
                    let mut id = [0u8; 16];
                    id.copy_from_slice(&c.commit_id[..16]);
                    Some((id, r.hdr.lsn))
                }
                _ => None,
            })
            .collect();
        idx.sort();
        if idx != h.index {
            return err(
                sections[1].1,
                "HCIDX is not exactly the file's Commit records [F10 §4.4]",
            );
        }
        let max_seq = recs
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(c) => Some(c.seq),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let from = recs.first().map_or(0, |r| r.hdr.lsn);
        let upto = recs.last().map_or(0, |r| r.hdr.lsn + u64::from(r.hdr.len));
        if h.hdr.base_seq != max_seq || h.hdr.from_lsn != from || h.hdr.upto_lsn != upto {
            return err(
                32,
                "hist SegHdr base_seq, from_lsn or upto_lsn differ from its records [F09 §2.3]",
            );
        }
    }
    Ok(())
}

/// Re-encodes a `hist` file.
pub fn encode_hist(h: &Hist) -> Vec<u8> {
    let data_off = crate::segment::SEG_HDR + 32 * 3;
    let hframes_len = 8 + 88 * h.frames.len();
    let hcidx_len = 1024 + 24 * h.index.len();
    let hdata_at = (data_off + hframes_len).next_multiple_of(8) + hcidx_len;
    let hdata_at = hdata_at.next_multiple_of(8);
    let mut data = Writer::new();
    let mut fw = Writer::new();
    fw.u32(h.frames.len() as u32);
    fw.u32(88);
    for f in &h.frames {
        let off = (hdata_at + data.len()) as u64;
        for (raw, p) in &f.blocks {
            data.u32(p.len() as u32);
            data.u32(*raw);
        }
        for (_, p) in &f.blocks {
            data.bytes(p);
        }
        let x = f.hdr;
        fw.u64(x.first_lsn);
        fw.u64(x.last_lsn);
        fw.u64(x.first_seq);
        fw.u64(x.last_seq);
        fw.u64(x.first_append_hlc);
        fw.u64(x.last_append_hlc);
        fw.u64(off);
        fw.u32(x.stored_len);
        fw.u32(x.raw_len);
        fw.u32(x.n_records);
        fw.u32(x.n_commits);
        fw.u16(x.n_blocks);
        fw.u8(x.codec);
        fw.zeros(5);
        fw.u64(x.raw_xxh3);
    }
    let mut iw = Writer::new();
    let mut fan = [0u32; 256];
    for (id, _) in &h.index {
        fan[usize::from(id[0])] += 1;
    }
    for i in 1..256 {
        fan[i] += fan[i - 1];
    }
    fan.iter().for_each(|f| iw.u32(*f));
    for (id, lsn) in &h.index {
        iw.bytes(id);
        iw.u64(*lsn);
    }
    let n = h.frames.len() as u32;
    let nc = h.index.len() as u32;
    encode_container(
        &h.hdr,
        &[
            (
                SecEnt {
                    tag: 0x0300,
                    flags: 0,
                    count: n,
                },
                fw.into_vec(),
            ),
            (
                SecEnt {
                    tag: 0x0301,
                    flags: 0,
                    count: nc,
                },
                iw.into_vec(),
            ),
            (
                SecEnt {
                    tag: 0x0302,
                    flags: 0,
                    count: n,
                },
                data.into_vec(),
            ),
        ],
    )
}

/// A `BlobIdx` row with its payload ([F10 §5.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    /// Content address.
    pub hash: [u8; 16],
    /// 1 body, 2 fingerprint.
    pub class: u8,
    /// Raw length.
    pub raw_len: u32,
    /// The `BlobEnc` payload: codec byte, then data.
    pub payload: Vec<u8>,
}

/// A `blobs` file ([F10 §5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blobs {
    /// The header.
    pub hdr: SegHdr,
    /// The blobs in `BLOBIDX` order.
    pub blobs: Vec<Blob>,
}

/// Decodes a `blobs` file with the checks of [F10 §9]; codec-0 payloads are hashed, others stay opaque.
pub fn decode_blobs(b: &[u8]) -> Result<Blobs> {
    let c = decode_container(b)?;
    if c.hdr.seg_kind != 6 {
        return err(6, "not a blobs file (seg_kind 6) [F10 §5.1]");
    }
    exact_tags(&c, &[0x0310, 0x0311], "blobs")?;
    let (ib, i_at) = c.sections[0];
    let (db, d_at) = c.sections[1];
    let mut r = Reader::with_base(ib, i_at);
    let n = r.u32()? as usize;
    if r.u32()? != 36 || ib.len() != 8 + 36 * n {
        return err(
            i_at,
            "BLOBIDX row_w is not 36, or its length breaks [F09 §4.2]",
        );
    }
    let mut blobs: Vec<Blob> = Vec::with_capacity(n);
    let mut pos = 0usize;
    for _ in 0..n {
        let e_at = r.offset();
        let hash = r.b16()?;
        let class = r.u8()?;
        r.zeros(3, "BlobIdx._reserved")?;
        let off = r.u64()?;
        let len = r.u32()? as usize;
        let raw_len = r.u32()?;
        if !(1..=2).contains(&class) || len == 0 {
            return err(e_at, "BlobIdx class outside 1-2, or len 0 [F10 §5.2]");
        }
        if blobs
            .last()
            .is_some_and(|p| (p.hash, p.class) >= (hash, class))
        {
            return err(
                e_at,
                "BLOBIDX not strictly ascending by (hash, class) [F10 §5.2]",
            );
        }
        if off != (d_at + pos) as u64 || pos + len > db.len() {
            return err(e_at, "a BlobIdx off does not chain in BLOBDATA [F10 §5.2]");
        }
        let payload = db[pos..pos + len].to_vec();
        pos += len;
        check_blob_enc(&payload, hash, class, raw_len, c.hdr.dict_no, e_at)?;
        blobs.push(Blob {
            hash,
            class,
            raw_len,
            payload,
        });
    }
    if pos != db.len() {
        return err(
            d_at + pos,
            "BLOBDATA has bytes no BlobIdx covers [F10 §5.2]",
        );
    }
    if c.entries[0].count as usize != n
        || c.entries[1].count as usize != n
        || c.hdr.n_rows as usize != n
    {
        return err(
            0,
            "blobs counts: BLOBIDX, BLOBDATA and n_rows disagree [F10 §5.2]",
        );
    }
    Ok(Blobs { hdr: c.hdr, blobs })
}

/// [F10 §3.2] `BlobEnc` decoding rules, with codec-0 hashing and the class rules of §5.3.
pub fn check_blob_enc(
    p: &[u8],
    hash: [u8; 16],
    class: u8,
    raw_len: u32,
    dict_no: u32,
    at: usize,
) -> Result<()> {
    let codec = p[0];
    if codec > holes::CODEC_MAX {
        return err(
            at,
            format!("codec byte {codec} outside the registry [F10 §3.1]"),
        );
    }
    if matches!(codec, 2 | 4) && dict_no == 0 {
        return err(
            at,
            "a dictionary codec in a blobs file with dict_no 0 [F10 §3.5]",
        );
    }
    match class {
        1 if raw_len > 65_536 => return err(at, "a body blob above 65536 bytes [F10 §5.3]"),
        2 if codec != 0 || !(20..=276).contains(&raw_len) || !(raw_len - 20).is_multiple_of(4) => {
            return err(
                at,
                "a fingerprint blob is not codec 0 with 20 + 4n <= 276 bytes [F10 §5.3]",
            );
        }
        _ => {}
    }
    if codec == 0 {
        if p.len() - 1 != raw_len as usize {
            return err(
                at,
                "a codec-0 payload's len - 1 differs from raw_len [F10 §3.2]",
            );
        }
        if blake3_128(&p[1..]) != hash {
            return err(at, "a blob's bytes do not hash to its hash [F10 §3.2]");
        }
    }
    Ok(())
}

/// Re-encodes a `blobs` file.
pub fn encode_blobs(bl: &Blobs) -> Vec<u8> {
    let data_off = crate::segment::SEG_HDR + 64;
    let data_at = (data_off + 8 + 36 * bl.blobs.len()).next_multiple_of(8);
    let mut iw = Writer::new();
    iw.u32(bl.blobs.len() as u32);
    iw.u32(36);
    let mut dw = Writer::new();
    for b in &bl.blobs {
        iw.bytes(&b.hash);
        iw.u8(b.class);
        iw.zeros(3);
        iw.u64((data_at + dw.len()) as u64);
        iw.u32(b.payload.len() as u32);
        iw.u32(b.raw_len);
        dw.bytes(&b.payload);
    }
    let n = bl.blobs.len() as u32;
    encode_container(
        &bl.hdr,
        &[
            (
                SecEnt {
                    tag: 0x0310,
                    flags: 0,
                    count: n,
                },
                iw.into_vec(),
            ),
            (
                SecEnt {
                    tag: 0x0311,
                    flags: 0,
                    count: n,
                },
                dw.into_vec(),
            ),
        ],
    )
}

/// A `dict.<D>` file ([F10 §6]); the content's form is `HOLE(F10-dict-form)`, kept verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dict {
    /// BLAKE3-128 of the content.
    pub digest: [u8; 16],
    /// The dictionary bytes.
    pub content: Vec<u8>,
}

/// Decodes a `dict` file: magic, `total_len`, reserved bytes and `digest` ([F10 §9]).
pub fn decode_dict(b: &[u8]) -> Result<Dict> {
    let mut r = Reader::new(b);
    if b.len() < 32 || r.array::<4>()? != *b"MDIC" {
        return err(0, "dict header magic is not MDIC [F10 §6.1]");
    }
    if r.u64()? != b.len() as u64 {
        return err(4, "dict total_len differs from the file size [F10 §6.1]");
    }
    let digest = r.b16()?;
    r.zeros(4, "DictHdr._reserved")?;
    if blake3_128(&b[32..]) != digest {
        return err(12, "dict digest mismatch [F10 §6.1]");
    }
    Ok(Dict {
        digest,
        content: b[32..].to_vec(),
    })
}

/// Re-encodes a `dict` file.
pub fn encode_dict(d: &Dict) -> Vec<u8> {
    let mut w = Writer::new();
    w.bytes(b"MDIC");
    w.u64(32 + d.content.len() as u64);
    w.bytes(&blake3_128(&d.content));
    w.zeros(4);
    w.bytes(&d.content);
    w.into_vec()
}

/// A `gitmap.<n>` page ([F10 §7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gitmap {
    /// Destination number.
    pub dest: u8,
    /// Object format.
    pub algo: Algo,
    /// The number in the file's name.
    pub file_no: u32,
    /// (id16, git oid digest).
    pub entries: Vec<([u8; 16], Vec<u8>)>,
}

/// Decodes a `gitmap` page with the checks of [F10 §9].
pub fn decode_gitmap(b: &[u8]) -> Result<Gitmap> {
    if b.len() < 1072 {
        return err(0, "a gitmap page is at least 1072 bytes [F10 §7.2]");
    }
    let mut r = Reader::new(b);
    if r.array::<4>()? != *b"MGMP" {
        return err(0, "gitmap magic is not MGMP [F10 §7.2]");
    }
    check_format(r.u16()?, 4, "gitmap")?;
    if xxh3_64(&b[..40]) != u64::from_le_bytes(b[40..48].try_into().expect("8")) {
        return err(40, "gitmap hdr_xxh3 mismatch [F10 §7.2]");
    }
    let dest = r.u8()?;
    let algo = Algo::from_byte(r.u8()?, 7)?;
    let n = r.u32()? as usize;
    let file_no = r.u32()?;
    let total = r.u64()?;
    let digest = r.b16()?;
    let w = 16 + algo.digest_len();
    if dest == 0 || file_no == 0 || total != (1072 + n * w) as u64 || total != b.len() as u64 {
        return err(
            6,
            "gitmap dest 0, file_no 0, or total_len not 1072 + n × w and the file size [F10 §7.2]",
        );
    }
    if blake3_128(&b[48..]) != digest {
        return err(24, "gitmap digest mismatch [F10 §7.2]");
    }
    let mut fr = Reader::with_base(&b[48..1072], 48);
    let mut fan = [0u32; 256];
    for f in &mut fan {
        *f = fr.u32()?;
    }
    let mut er = Reader::with_base(&b[1072..], 1072);
    let mut entries: Vec<([u8; 16], Vec<u8>)> = Vec::with_capacity(n);
    for _ in 0..n {
        let id = er.b16()?;
        if entries.last().is_some_and(|p| p.0 >= id) {
            return err(
                er.offset(),
                "gitmap entries not strictly ascending by id16 [F10 §7.3]",
            );
        }
        entries.push((id, er.bytes(algo.digest_len())?.to_vec()));
    }
    check_fan(&fan, entries.iter().map(|e| e.0[0]), 48)?;
    Ok(Gitmap {
        dest,
        algo,
        file_no,
        entries,
    })
}

/// Re-encodes a `gitmap` page.
pub fn encode_gitmap(g: &Gitmap) -> Vec<u8> {
    let mut body = Writer::new();
    let mut fan = [0u32; 256];
    for (id, _) in &g.entries {
        fan[usize::from(id[0])] += 1;
    }
    for i in 1..256 {
        fan[i] += fan[i - 1];
    }
    fan.iter().for_each(|f| body.u32(*f));
    for (id, d) in &g.entries {
        body.bytes(id);
        body.bytes(d);
    }
    let mut w = Writer::new();
    w.bytes(b"MGMP");
    w.u16(1);
    w.u8(g.dest);
    w.u8(g.algo.byte());
    w.u32(g.entries.len() as u32);
    w.u32(g.file_no);
    w.u64(48 + body.len() as u64);
    w.bytes(&blake3_128(body.as_slice()));
    let hs = xxh3_64(w.as_slice());
    w.u64(hs);
    w.bytes(body.as_slice());
    w.into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::tests::{EPOCH, head, rec};
    use crate::log::{Group, Payload, encode_group, epoch_seed};

    fn seghdr(kind: u8) -> SegHdr {
        SegHdr {
            seg_kind: kind,
            tok_ver: 0,
            n_rows: 0,
            file_no: 3,
            ref_id: 0,
            dict_no: 0,
            base_seq: 0,
            from_lsn: 0,
            upto_lsn: 0,
            rt_upto_lsn: 0,
            total_len: 0,
            seg_digest: [0; 32],
        }
    }

    /// [F10 §5.2]: a `blobs` file with a body and a fingerprint round-trips; a wrong hash is refused.
    #[test]
    fn blobs_round_trip() {
        let body = b"hello\n".to_vec();
        let fp = vec![7u8; 24];
        let mut blobs = vec![
            Blob {
                hash: blake3_128(&body),
                class: 1,
                raw_len: 6,
                payload: [&[0u8][..], &body].concat(),
            },
            Blob {
                hash: blake3_128(&fp),
                class: 2,
                raw_len: 24,
                payload: [&[0u8][..], &fp].concat(),
            },
        ];
        blobs.sort_by_key(|a| (a.hash, a.class));
        let mut h = seghdr(6);
        h.n_rows = 2;
        let f = Blobs { hdr: h, blobs };
        let b = encode_blobs(&f);
        let d = decode_blobs(&b).unwrap();
        assert_eq!(encode_blobs(&d), b);
        let mut bad = f.clone();
        bad.blobs[0].hash[0] ^= 1;
        bad.blobs.sort_by_key(|a| (a.hash, a.class));
        assert!(decode_blobs(&encode_blobs(&bad)).is_err());
    }

    /// [F10 §4]: a `hist` file holding one frame with an extent head and a pad-free group, codec 0.
    #[test]
    fn hist_round_trip() {
        let g = Group {
            lsn: 0,
            records: vec![head(0, epoch_seed(EPOCH))],
            len: 0,
        };
        let raw = encode_group(&g, epoch_seed(EPOCH));
        let noop = rec(12, 3, 138, Payload::Noop(0), None).encode();
        let raw_all = [raw.clone(), noop].concat();
        let fh = FrameHdr {
            first_lsn: 0,
            last_lsn: 138,
            first_seq: 0,
            last_seq: 0,
            first_append_hlc: 0,
            last_append_hlc: 0,
            off: 0,
            stored_len: 8 + raw_all.len() as u32,
            raw_len: raw_all.len() as u32,
            n_records: 2,
            n_commits: 0,
            n_blocks: 1,
            codec: 0,
            raw_xxh3: xxh3_64(&raw_all),
        };
        let mut h = seghdr(2);
        h.upto_lsn = 138 + 40;
        let hist = Hist {
            hdr: h,
            frames: vec![Frame {
                hdr: fh,
                blocks: vec![(raw_all.len() as u32, raw_all.clone())],
                records: None,
            }],
            index: vec![],
        };
        let b = encode_hist(&hist);
        let d = decode_hist(&b).unwrap();
        assert_eq!(d.frames[0].records.as_ref().unwrap().len(), 2);
        assert_eq!(encode_hist(&d), b);
    }

    /// [F10 §6], §7: `dict` and `gitmap` headers round-trip; a bad fan-out is refused.
    #[test]
    fn dict_and_gitmap() {
        let d = Dict {
            digest: blake3_128(b"abc"),
            content: b"abc".to_vec(),
        };
        let b = encode_dict(&d);
        assert_eq!(b.len(), 35);
        assert_eq!(decode_dict(&b).unwrap(), d);
        let g = Gitmap {
            dest: 1,
            algo: Algo::Sha1,
            file_no: 4,
            entries: vec![([1; 16], vec![2; 20]), ([9; 16], vec![3; 20])],
        };
        let b = encode_gitmap(&g);
        assert_eq!(b.len(), 1072 + 2 * 36);
        assert_eq!(decode_gitmap(&b).unwrap(), g);
        let mut bad = b.clone();
        bad[48] = 1;
        assert!(decode_gitmap(&bad).is_err());
    }
}
