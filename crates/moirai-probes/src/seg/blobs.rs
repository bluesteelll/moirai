//! The `blobs.<n>` file a delta checkpoint seals its window's bodies into ([F10 §5], `10-sealed-files.md:305-376`;
//! [F10 §5.4] "A delta checkpoint seals the bodies of its window into one new `blobs` file, compressing them here",
//! `:360`), the `BlobRef` that names a body in it ([F09 §6.3], `09-segments.md:445-475`), and the reader of the tests.
//!
//! - **The payload** ([F10 §3.2], `10-sealed-files.md:98-115`): `BlobEnc` is the codec byte, then the data. The codec is
//!   `lz4` (1, one LZ4 block, `:123`), R-40 O-1's choice for `HOLE(F10-blob-codec)`, or `none` (0, the raw bytes) when
//!   the block is not shorter than the raw bytes (§3.4, `:135`, `:141`). The fallback compares lengths, so it is
//!   deterministic.
//! - **The file** (§5.1–§5.2, `:307-338`): `SegHdr` with `seg_kind` 6, `tok_ver` 0, `n_rows` = the blobs, `dict_no` 0,
//!   `file_no` n and every other field 0 ([F09 §2.3], `09-segments.md:119-129`); exactly two sections, `BLOBIDX`
//!   (`0x0310`, a fixed table of 36-byte `BlobIdx` rows sorted by (`hash`, `class`)) and `BLOBDATA` (`0x0311`, the
//!   payloads contiguous in `BLOBIDX` order, the first at the section's first byte); every `off` is absolute.
//! - **`BlobRef`** (36 B: `hash`, `file`, `off`, `len`, `raw_len`): the same `hash`, `off`, `len` and `raw_len` as the
//!   body's `BlobIdx` row, with `file` = n (F10:336). Every body here is of class 1 `body` (§5.3, `:344`).

use super::{Header, SegView, Writer, data_off, fixed_rows, put_fixed, seg_kind, tag};
use crate::tail::commit::blake3_16;

/// `BlobRef`'s length (`09-segments.md:449-456`).
pub const BLOB_REF_LEN: usize = 36;
/// `BlobIdx`'s length (`10-sealed-files.md:323-331`).
pub const BLOB_IDX_LEN: usize = 36;

/// The codec byte ([F10 §3.1], `10-sealed-files.md:87-96`).
pub mod codec {
    /// `none`: the raw bytes.
    pub const NONE: u8 = 0;
    /// `lz4`: one LZ4 block (candidate 1, R-40 O-1).
    pub const LZ4: u8 = 1;
}

/// The blob class `body` ([F10 §5.3], `10-sealed-files.md:342-344`).
pub const CLASS_BODY: u8 = 1;

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_le(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

/// `BlobRef` ([F09 §6.3], `09-segments.md:447-456`).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, PartialOrd, Ord)]
pub struct BlobRef {
    /// `hash` (offset 0): BLAKE3-128 of the raw bytes.
    pub hash: [u8; 16],
    /// `file` (16): n of `blobs.<n>`.
    pub file: u32,
    /// `off` (20): the payload's absolute offset in the file.
    pub off: u64,
    /// `len` (28): the payload's length, the codec byte included.
    pub len: u32,
    /// `raw_len` (32).
    pub raw_len: u32,
}

impl BlobRef {
    /// The 36 bytes.
    pub fn encode(&self) -> [u8; BLOB_REF_LEN] {
        let mut b = [0u8; BLOB_REF_LEN];
        b[0..16].copy_from_slice(&self.hash);
        b[16..20].copy_from_slice(&self.file.to_le_bytes());
        b[20..28].copy_from_slice(&self.off.to_le_bytes());
        b[28..32].copy_from_slice(&self.len.to_le_bytes());
        b[32..36].copy_from_slice(&self.raw_len.to_le_bytes());
        b
    }

    /// The fields of 36 bytes.
    pub fn decode(b: &[u8]) -> BlobRef {
        BlobRef {
            hash: b[0..16].try_into().expect("16 bytes"),
            file: u32_le(b, 16),
            off: u64_le(b, 20),
            len: u32_le(b, 28),
            raw_len: u32_le(b, 32),
        }
    }
}

/// `BlobIdx` ([F10 §5.2], `10-sealed-files.md:323-331`).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BlobIdx {
    /// `hash` (offset 0).
    pub hash: [u8; 16],
    /// `class` (16).
    pub class: u8,
    /// `off` (20).
    pub off: u64,
    /// `len` (28).
    pub len: u32,
    /// `raw_len` (32).
    pub raw_len: u32,
}

impl BlobIdx {
    /// The 36 bytes; 17–19 are `_reserved`, zero.
    pub fn encode(&self) -> [u8; BLOB_IDX_LEN] {
        let mut b = [0u8; BLOB_IDX_LEN];
        b[0..16].copy_from_slice(&self.hash);
        b[16] = self.class;
        b[20..28].copy_from_slice(&self.off.to_le_bytes());
        b[28..32].copy_from_slice(&self.len.to_le_bytes());
        b[32..36].copy_from_slice(&self.raw_len.to_le_bytes());
        b
    }

    /// Decodes 36 bytes; refuses a non-zero `_reserved`.
    pub fn decode(b: &[u8]) -> Result<BlobIdx, String> {
        if b[17..20] != [0; 3] {
            return Err("BlobIdx _reserved is not zero".into());
        }
        Ok(BlobIdx {
            hash: b[0..16].try_into().expect("16 bytes"),
            class: b[16],
            off: u64_le(b, 20),
            len: u32_le(b, 28),
            raw_len: u32_le(b, 32),
        })
    }
}

/// The body codec with its output buffer, kept across calls so a checkpoint allocates it once.
#[derive(Clone, Debug, Default)]
pub struct Packer {
    buf: Vec<u8>,
}

impl Packer {
    /// Appends the `BlobEnc` payload of `raw` to `out` and returns its codec: `lz4` when the block is shorter than the
    /// raw bytes, else `none` with the raw bytes ([F10 §3.4], `10-sealed-files.md:135`, `:141`).
    pub fn pack(&mut self, raw: &[u8], out: &mut Vec<u8>) -> Result<u8, String> {
        let max = lz4_flex::block::get_maximum_output_size(raw.len());
        if self.buf.len() < max {
            self.buf.resize(max, 0);
        }
        let n = lz4_flex::block::compress_into(raw, &mut self.buf).map_err(|e| e.to_string())?;
        if n < raw.len() {
            out.push(codec::LZ4);
            out.extend_from_slice(&self.buf[..n]);
            Ok(codec::LZ4)
        } else {
            out.push(codec::NONE);
            out.extend_from_slice(raw);
            Ok(codec::NONE)
        }
    }
}

/// Decodes a `BlobEnc` payload ([F10 §3.2] "Decoding", `10-sealed-files.md:112-115`): refuses an unknown codec, a
/// decoded length other than `raw_len` (for codec 0, `len − 1`), and bytes whose BLAKE3-128 is not `hash`.
pub fn unpack(payload: &[u8], raw_len: u32, hash: &[u8; 16]) -> Result<Vec<u8>, String> {
    let (&c, data) = payload.split_first().ok_or("an empty payload")?;
    let raw = match c {
        codec::NONE => data.to_vec(),
        codec::LZ4 => {
            let mut out = vec![0u8; raw_len as usize];
            let n = lz4_flex::block::decompress_into(data, &mut out).map_err(|e| e.to_string())?;
            out.truncate(n);
            out
        }
        c => return Err(format!("codec {c}: not admitted")),
    };
    if raw.len() != raw_len as usize {
        return Err(format!("{} raw bytes, raw_len {raw_len}", raw.len()));
    }
    if blake3_16(&raw) != *hash {
        return Err("the decoded bytes do not hash to the blob's hash".into());
    }
    Ok(raw)
}

/// The `blobs.<file_no>` file of `bodies` (hash, raw bytes), each of class `body`: `BLOBIDX` sorted by hash, the
/// payloads contiguous in its order. Returns the file's bytes and each body's `BlobRef`, in hash order. Refuses two
/// bodies with one hash and no body.
pub fn seal(
    bodies: &[([u8; 16], &[u8])],
    file_no: u32,
    packer: &mut Packer,
) -> Result<(Vec<u8>, Vec<BlobRef>), String> {
    if bodies.is_empty() {
        return Err("a blobs file holds at least one blob".into());
    }
    let mut order: Vec<usize> = (0..bodies.len()).collect();
    order.sort_by_key(|&i| bodies[i].0);
    if order.windows(2).any(|w| bodies[w[0]].0 == bodies[w[1]].0) {
        return Err("two bodies with one hash".into());
    }
    let n = bodies.len();
    let idx_len = 8 + BLOB_IDX_LEN * n;
    let data_at = (data_off(2) + idx_len).next_multiple_of(super::SECTION_ALIGN) as u64;
    let mut data = Vec::with_capacity(bodies.iter().map(|b| b.1.len() + 1).sum());
    let mut refs = Vec::with_capacity(n);
    for &i in &order {
        let (hash, raw) = bodies[i];
        let at = data.len();
        packer.pack(raw, &mut data)?;
        refs.push(BlobRef {
            hash,
            file: file_no,
            off: data_at + at as u64,
            len: u32::try_from(data.len() - at).map_err(|_| "a payload past 4 GiB")?,
            raw_len: u32::try_from(raw.len()).map_err(|_| "a body past 4 GiB")?,
        });
    }
    let rows: Vec<[u8; BLOB_IDX_LEN]> = refs
        .iter()
        .map(|r| {
            BlobIdx {
                hash: r.hash,
                class: CLASS_BODY,
                off: r.off,
                len: r.len,
                raw_len: r.raw_len,
            }
            .encode()
        })
        .collect();
    let mut w = Writer::new(2, data_at as usize + data.len());
    w.push(tag::BLOBIDX, n as u32, |o| {
        let r: Vec<&[u8]> = rows.iter().map(|r| &r[..]).collect();
        put_fixed(o, BLOB_IDX_LEN as u32, &r);
        Ok(())
    })?;
    w.push(tag::BLOBDATA, n as u32, |o| {
        if o.len() as u64 != data_at {
            return Err(format!("BLOBDATA at {}, not {data_at}", o.len()));
        }
        o.extend_from_slice(&data);
        Ok(())
    })?;
    let bytes = w.finish(&Header {
        seg_kind: seg_kind::BLOBS,
        n_rows: n as u32,
        file_no,
        ..Header::default()
    })?;
    Ok((bytes, refs))
}

/// A `blobs` file read back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobsFile {
    /// The header.
    pub header: Header,
    /// `BLOBIDX`.
    pub idx: Vec<BlobIdx>,
    /// Each blob's raw bytes, in `BLOBIDX` order.
    pub raw: Vec<Vec<u8>>,
}

/// Reads a `blobs` file and checks it ([F10 §5], V-9): `seg_kind` 6, `tok_ver` 0 and the `blobs` column's zero fields;
/// exactly `BLOBIDX` and `BLOBDATA`, each with `count` = `n_rows`; rows strictly ascending by (`hash`, `class`), of
/// class 1; payloads contiguous from `BLOBDATA`'s first byte to its last; each payload decoding to its raw bytes and
/// hash.
pub fn decode(bytes: &[u8]) -> Result<BlobsFile, String> {
    let v = SegView::parse(bytes)?;
    v.check_digests()?;
    let h = v.header;
    if h.seg_kind != seg_kind::BLOBS
        || h.tok_ver != 0
        || h.dict_no != 0
        || h.ref_id != 0
        || (h.base_seq, h.from_lsn, h.upto_lsn, h.rt_upto_lsn) != (0, 0, 0, 0)
    {
        return Err(format!("not a blobs header: {h:?}"));
    }
    let tags: Vec<(u16, u32)> = v.table.iter().map(|e| (e.tag, e.count)).collect();
    if tags != [(tag::BLOBIDX, h.n_rows), (tag::BLOBDATA, h.n_rows)] {
        return Err(format!("blobs sections {tags:?}"));
    }
    let idx = fixed_rows(v.need(tag::BLOBIDX)?, BLOB_IDX_LEN as u32)?
        .into_iter()
        .map(BlobIdx::decode)
        .collect::<Result<Vec<_>, _>>()?;
    let data = v.entry(tag::BLOBDATA).expect("listed");
    let mut at = data.off;
    let mut raw = Vec::with_capacity(idx.len());
    for (i, r) in idx.iter().enumerate() {
        if i > 0 && (idx[i - 1].hash, idx[i - 1].class) >= (r.hash, r.class) {
            return Err(format!("BLOBIDX row {i} is not after row {}", i - 1));
        }
        if r.class != CLASS_BODY {
            return Err(format!("BLOBIDX row {i}: class {}", r.class));
        }
        if r.off != at {
            return Err(format!(
                "BLOBIDX row {i}: off {} where the payloads reach {at} (no gap)",
                r.off
            ));
        }
        let p = bytes
            .get(r.off as usize..r.off as usize + r.len as usize)
            .ok_or_else(|| format!("BLOBIDX row {i} runs past the file"))?;
        raw.push(unpack(p, r.raw_len, &r.hash)?);
        at += u64::from(r.len);
    }
    if at != data.off + data.len {
        return Err(format!(
            "BLOBDATA holds {} bytes, its payloads {}",
            data.len,
            at - data.off
        ));
    }
    Ok(BlobsFile {
        header: h,
        idx,
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tail::store::SEG_HDR_LEN;

    /// Three bodies: one that compresses (300 × `0x5B`, `tail::testdata`'s), 1,020 random bytes (the generator's mix
    /// body, which does not), and two bytes.
    fn three() -> Vec<([u8; 16], Vec<u8>)> {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let random: Vec<u8> = (0..1_020)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        [vec![0x5B; 300], random, b"hi".to_vec()]
            .into_iter()
            .map(|b| (blake3_16(&b), b))
            .collect()
    }

    fn sealed(bodies: &[([u8; 16], Vec<u8>)]) -> (Vec<u8>, Vec<BlobRef>) {
        let refs: Vec<([u8; 16], &[u8])> = bodies.iter().map(|(h, b)| (*h, b.as_slice())).collect();
        seal(&refs, 15, &mut Packer::default()).unwrap()
    }

    /// The `blobs` file ([F10 §5.1]–§5.2, `10-sealed-files.md:307-338`): `SegHdr` with `seg_kind` 6, `tok_ver` 0,
    /// `n_rows` 3, `file_no` 15 and every other field 0; exactly `BLOBIDX` (`0x0310`, 8 + 3 × 36 bytes) and `BLOBDATA`
    /// (`0x0311`), each `count` 3; rows sorted by hash, class 1, reserved zero; no gap: the first `off` is `BLOBDATA`'s
    /// first byte and each next `off` is the previous `off` + `len`, the last payload ending the section; each payload
    /// decodes to its raw bytes and hash; each `BlobRef` repeats its row with `file` 15. A gap is refused.
    #[test]
    fn blobs_file_follows_f10_5() {
        let bodies = three();
        let (b, refs) = sealed(&bodies);
        let v = SegView::parse(&b).unwrap();
        v.check_digests().unwrap();
        let rows = fixed_rows(v.need(tag::BLOBIDX).unwrap(), 36).unwrap();
        let idx: Vec<BlobIdx> = rows.iter().map(|r| BlobIdx::decode(r).unwrap()).collect();
        assert_eq!(idx.len(), 3);
        assert_eq!(
            idx[0].off, v.table[1].off,
            "the first payload at BLOBDATA's first byte"
        );
        for i in 1..3 {
            assert_eq!(
                idx[i].off,
                idx[i - 1].off + u64::from(idx[i - 1].len),
                "off[{i}] ≠ off[{}] + len[{}]: a gap",
                i - 1,
                i - 1
            );
        }
        assert_eq!(idx[2].off + u64::from(idx[2].len), b.len() as u64);
        assert!(
            idx.windows(2).all(|w| w[0].hash < w[1].hash),
            "sorted by hash"
        );
        assert!(rows.iter().all(|r| r[16] == 1 && r[17..20] == [0; 3]));
        assert_eq!((b[6], b[7]), (6, 0), "seg_kind blobs, tok_ver 0");
        assert_eq!(
            v.header,
            Header {
                seg_kind: 6,
                n_rows: 3,
                file_no: 15,
                ..Header::default()
            }
        );
        let placed: Vec<(u16, u32, u64)> =
            v.table.iter().map(|e| (e.tag, e.count, e.len)).collect();
        let data_len: u64 = refs.iter().map(|r| u64::from(r.len)).sum();
        assert_eq!(placed, [(0x0310, 3, 8 + 3 * 36), (0x0311, 3, data_len)]);
        assert_eq!(v.table[0].off, (SEG_HDR_LEN + 2 * 32) as u64);
        for (r, i) in refs.iter().zip(&idx) {
            assert_eq!(
                (r.hash, r.file, r.off, r.len, r.raw_len),
                (i.hash, 15, i.off, i.len, i.raw_len)
            );
        }
        let back = decode(&b).unwrap();
        assert_eq!(back.idx, idx);
        let mut want: Vec<&([u8; 16], Vec<u8>)> = bodies.iter().collect();
        want.sort_by_key(|x| x.0);
        assert_eq!(
            back.raw,
            want.iter().map(|x| x.1.clone()).collect::<Vec<_>>()
        );
        // The same blobs with one zero byte after the first payload (a gap), every digest right, are refused.
        let data = v.need(tag::BLOBDATA).unwrap();
        let first = idx[0].len as usize;
        let gapped: Vec<u8> = [&data[..first], &[0], &data[first..]].concat();
        let mut w = Writer::new(2, 0);
        w.push(tag::BLOBIDX, 3, |o| {
            let moved: Vec<[u8; 36]> = idx
                .iter()
                .enumerate()
                .map(|(k, r)| {
                    BlobIdx {
                        off: r.off + u64::from(k > 0),
                        ..*r
                    }
                    .encode()
                })
                .collect();
            put_fixed(o, 36, &moved.iter().map(|r| &r[..]).collect::<Vec<_>>());
            Ok(())
        })
        .unwrap();
        w.push(tag::BLOBDATA, 3, |o| {
            o.extend_from_slice(&gapped);
            Ok(())
        })
        .unwrap();
        let gap = w.finish(&v.header).unwrap();
        SegView::parse(&gap).unwrap().check_digests().unwrap();
        assert!(
            decode(&gap).unwrap_err().contains("no gap"),
            "{:?}",
            decode(&gap)
        );
        assert!(seal(&[], 15, &mut Packer::default()).is_err());
    }

    /// The codec ([F10 §3.4], `10-sealed-files.md:135`, `:141`; R-40 O-1): a body that lz4 shortens is codec 1, one LZ4
    /// block that decodes to it; a body that lz4 does not shorten (random bytes, two bytes) is codec 0 with its raw
    /// bytes, `len − 1` = `raw_len`; an unknown codec, a wrong length or a wrong hash is refused.
    #[test]
    fn codec_falls_back_to_none_when_lz4_is_not_shorter() {
        let mut p = Packer::default();
        let bodies = three();
        let mut out = Vec::new();
        assert_eq!(
            p.pack(&bodies[0].1, &mut out),
            Ok(codec::LZ4),
            "300 equal bytes compress"
        );
        assert!(out.len() < 1 + 300, "{} bytes", out.len());
        assert_eq!(&out[1..], &lz4_flex::block::compress(&bodies[0].1)[..]);
        assert_eq!(unpack(&out, 300, &bodies[0].0), Ok(bodies[0].1.clone()));
        for (h, raw) in &bodies[1..] {
            let mut out = Vec::new();
            assert_eq!(
                p.pack(raw, &mut out),
                Ok(codec::NONE),
                "{} bytes",
                raw.len()
            );
            assert!(lz4_flex::block::compress(raw).len() >= raw.len());
            assert_eq!((out[0], &out[1..]), (0, &raw[..]));
            assert_eq!(unpack(&out, raw.len() as u32, h), Ok(raw.clone()));
            assert!(
                unpack(&out, raw.len() as u32 + 1, h).is_err(),
                "len − 1 ≠ raw_len"
            );
        }
        assert!(
            unpack(&[2, 0], 1, &bodies[2].0).is_err(),
            "codec 2 is not admitted"
        );
        assert!(unpack(&out, 300, &bodies[1].0).is_err(), "another hash");
    }

    /// `BlobRef` ([F09 §6.3], `09-segments.md:449-456`) and `BlobIdx` ([F10 §5.2], `10-sealed-files.md:323-331`), 36
    /// bytes each, little-endian: `hash` at 0, `file` (or `class` and 3 reserved bytes) at 16, `off` at 20, `len` at 28,
    /// `raw_len` at 32.
    #[test]
    fn blob_ref_fields_sit_at_f09_6_3_offsets() {
        let r = BlobRef {
            hash: core::array::from_fn(|i| i as u8 + 1),
            file: 0x1411_1213,
            off: 0x1C1B_1A19_1817_1615,
            len: 0x201F_1E1D,
            raw_len: 0x2423_2221,
        };
        let mut want: Vec<u8> = (1..=16).collect();
        want.extend([0x13, 0x12, 0x11, 0x14]);
        want.extend(0x15..=0x24);
        assert_eq!(r.encode().to_vec(), want, "BlobRef");
        assert_eq!(BlobRef::decode(&want), r);
        let i = BlobIdx {
            hash: r.hash,
            class: 1,
            off: r.off,
            len: r.len,
            raw_len: r.raw_len,
        };
        let mut want_i = want.clone();
        want_i[16..20].copy_from_slice(&[1, 0, 0, 0]);
        assert_eq!(i.encode().to_vec(), want_i, "BlobIdx");
        assert_eq!(BlobIdx::decode(&want_i), Ok(i));
        want_i[18] = 1;
        assert!(BlobIdx::decode(&want_i).is_err());
    }
}
