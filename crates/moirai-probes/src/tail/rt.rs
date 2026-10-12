//! The runtime records of a settle ([F05 §8.5], [F05 §9.18], [F05 §9.23]; `docs/spec/format/05-log.md:507-521`,
//! `:974-981`, `:1006-1032`): the `TreeReg` settle epoch (sub 2) and the `FileObs` row batch whose rows are images of
//! [F11 §12.5]'s 187-byte `FILEOBS` row ([F11 §2.9], `11-runtime-tables.md:278-298`, `:889-914`). A settle writes one
//! `TreeReg` epoch and the rows that changed ([70 S8] fix (1), `70-audit-speed.md:300`).

use super::enc::{DecodeError, Reader, put_u32, put_u64, put_uvar, put_vbytes};
use std::borrow::Cow;

/// `Epoch` ([F11 §12.4], `11-runtime-tables.md:855-864`): `scope_kind` u8 at 0, `_reserved` [3] at 1, `scope_ref` u32 at
/// 4, `digest` b16 at 8, `hlc` u64 at 24.
pub const EPOCH_LEN: usize = 32;
/// The `TreeReg` sub-2 payload: `sub` (1) + `tree` (16) + `epoch` (32) ([F05 §9.23], `05-log.md:1010-1023`). F05's
/// "with the tree key before it the epoch payload is 48 bytes" (`:1028-1030`) counts the tree key and the epoch; the
/// `sub` byte, the discriminant, makes the record payload 49.
pub const TREE_REG_EPOCH_PAYLOAD_LEN: usize = 49;
/// `OsFileId` ([F11 §12.1], `11-runtime-tables.md:764-778`).
pub const OS_FILE_ID_LEN: usize = 57;
/// `FsTime` ([F11 §12.2], `:791-798`).
pub const FS_TIME_LEN: usize = 9;
/// `FileAttrs` ([F11 §12.2], `:803-805`).
pub const FILE_ATTRS_LEN: usize = 4;
/// `OidSlot` ([F11 §2.5], `:180-186`).
pub const OID_SLOT_LEN: usize = 33;
/// `HeapRef` ([F11 §2.3], `:121-126`).
pub const HEAP_REF_LEN: usize = 8;
/// The fixed part of a `FILEOBS` row ([F11 §12.5], `:895-914`).
pub const FILEOBS_ROW_LEN: usize = 187;

/// The settle epoch ([F11 §12.4]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Epoch {
    /// 0 `full-tree`, 1 `lane-owned`, 2 `partial`.
    pub scope_kind: u8,
    /// The lane's `ref_id` for `lane-owned`, else 0.
    pub scope_ref: u32,
    /// The scope digest.
    pub digest: [u8; 16],
    /// The settle's `hlc`.
    pub hlc: u64,
}

/// A `TreeReg` record of sub 2: a tree's settle epoch.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TreeReg {
    /// The tree key.
    pub tree: [u8; 16],
    /// The epoch.
    pub epoch: Epoch,
}

impl TreeReg {
    /// Appends the 49-byte payload.
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.push(2);
        out.extend_from_slice(&self.tree);
        out.push(self.epoch.scope_kind);
        out.extend_from_slice(&[0, 0, 0]);
        put_u32(out, self.epoch.scope_ref);
        out.extend_from_slice(&self.epoch.digest);
        put_u64(out, self.epoch.hlc);
    }

    /// Decodes a payload of sub 2; subs 1, 3 and 4 are outside the subset.
    pub fn decode(payload: &[u8]) -> Result<TreeReg, DecodeError> {
        let mut r = Reader::new(payload);
        match r.u8()? {
            2 => {}
            s @ (1 | 3 | 4) => {
                return Err(r.err(format!("TreeReg sub {s}: outside the probe's subset")));
            }
            s => return Err(r.err(format!("TreeReg sub {s} is invalid"))),
        }
        let tree = r.array()?;
        let scope_kind = r.u8()?;
        if scope_kind > 2 {
            return Err(r.err(format!("scope_kind {scope_kind} is invalid")));
        }
        if r.array::<3>()? != [0; 3] {
            return Err(r.err("Epoch _reserved is not zero"));
        }
        let t = TreeReg {
            tree,
            epoch: Epoch {
                scope_kind,
                scope_ref: r.u32()?,
                digest: r.array()?,
                hlc: r.u64()?,
            },
        };
        r.end()?;
        Ok(t)
    }
}

/// The fields of one `FILEOBS` row ([F11 §12.5], `11-runtime-tables.md:895-914`) with an empty `var` sequence.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FileObsRow {
    /// `n`, the file node's `#F`.
    pub n: u32,
    /// The tree key.
    pub tree: [u8; 16],
    /// The recorded state ([F18 §4.2] codes: 1 `ok` …).
    pub state: u8,
    /// The resolver version.
    pub resolver_version: u16,
    /// `file_id`, an `OsFileId`.
    pub file_id: [u8; OS_FILE_ID_LEN],
    /// The raw size.
    pub size: u64,
    /// `mtime`, `ctime`, `creation`, `added`.
    pub times: [[u8; FS_TIME_LEN]; 4],
    /// `attrs`.
    pub attrs: u32,
    /// `last_oid`, an `OidSlot`.
    pub last_oid: [u8; OID_SLOT_LEN],
    /// `verified_at`.
    pub verified_at: u64,
    /// `missing_since`.
    pub missing_since: u64,
}

impl FileObsRow {
    /// The row's image ([F11 §2.9]): the 187 fixed bytes, `var` = (`off` 0, `len` 0), no slice.
    pub fn image(&self) -> [u8; FILEOBS_ROW_LEN] {
        let mut b = Vec::with_capacity(FILEOBS_ROW_LEN);
        put_u32(&mut b, self.n);
        b.extend_from_slice(&self.tree);
        b.push(self.state);
        b.push(0); // flags: no path_seen, no detail, live
        b.extend_from_slice(&self.resolver_version.to_le_bytes());
        b.push(0); // n_proposals
        b.extend_from_slice(&self.file_id);
        put_u64(&mut b, self.size);
        for t in &self.times {
            b.extend_from_slice(t);
        }
        put_u32(&mut b, self.attrs);
        b.extend_from_slice(&self.last_oid);
        put_u64(&mut b, self.verified_at);
        put_u64(&mut b, self.missing_since);
        b.extend_from_slice(&[0u8; HEAP_REF_LEN]); // var: off 0, len 0
        let mut out = [0u8; FILEOBS_ROW_LEN];
        out.copy_from_slice(&b);
        out
    }
}

/// One row of a batch ([F05 §8.5], `05-log.md:517-521`): `op` 1 (upsert), then the row image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row<'a> {
    /// 1 upsert.
    pub op: u8,
    /// The `FILEOBS` row image.
    pub image: Cow<'a, [u8]>,
}

impl Row<'_> {
    /// The row's key (`n`, `tree`), the image's first 20 bytes ([F05 §8.5], `05-log.md:515`;
    /// `11-runtime-tables.md:897-898`): a replay indexes it without decoding the value.
    pub fn key(&self) -> (u32, [u8; 16]) {
        let n = u32::from_le_bytes(self.image[0..4].try_into().expect("4 bytes"));
        let mut tree = [0u8; 16];
        tree.copy_from_slice(&self.image[4..20]);
        (n, tree)
    }
}

/// A `FileObs` row batch.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RowBatch<'a> {
    /// The rows, applied in order.
    pub rows: Vec<Row<'a>>,
}

/// Checks one `FILEOBS` upsert image of the subset: 187 bytes plus its `var` slice, `var` = (0, `len`), the reserved
/// flag bits zero, a recorded state code ([F11 §12.5], `11-runtime-tables.md:916-922`) and at most 3 proposals; rows
/// with a `var` sequence (a path seen, details, proposals, a moved-auto target) are outside the subset.
fn check_image(r: &Reader<'_>, image: &[u8]) -> Result<(), DecodeError> {
    if image.len() < FILEOBS_ROW_LEN {
        return Err(r.err(format!("a FILEOBS image of {} bytes", image.len())));
    }
    let (state, flags, n_prop) = (image[20], image[21], image[24]);
    if flags & 0x7C != 0 {
        return Err(r.err(format!("FILEOBS flags {flags:#04x} set a reserved bit")));
    }
    if flags & 0x80 != 0 {
        return Err(r.err("an upsert row with the dead bit set"));
    }
    if !matches!(state, 1 | 2 | 3 | 4 | 6 | 8 | 12) || n_prop > 3 {
        return Err(r.err(format!("FILEOBS state {state} or n_proposals {n_prop}")));
    }
    let off = u32::from_le_bytes(image[179..183].try_into().expect("4 bytes"));
    let len = u32::from_le_bytes(image[183..187].try_into().expect("4 bytes"));
    if off != 0 || len as usize != image.len() - FILEOBS_ROW_LEN {
        return Err(r.err("the image's var HeapRef is not (0, slice length) (F11 §2.9)"));
    }
    if len != 0 || flags & 3 != 0 || n_prop != 0 || state == 2 {
        return Err(r.err("a FILEOBS row with a var sequence: outside the probe's subset"));
    }
    Ok(())
}

impl RowBatch<'_> {
    /// Appends the batch: `n_rows` uvar32 (≥ 1), then each row as `vbytes` of `op` and the image.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), String> {
        if self.rows.is_empty() {
            return Err("a row batch holds at least one row".into());
        }
        put_uvar(out, self.rows.len() as u64);
        let mut row = Vec::new();
        for r in &self.rows {
            if r.op != 1 {
                return Err(format!("row op {}: outside the probe's subset", r.op));
            }
            row.clear();
            row.push(r.op);
            row.extend_from_slice(&r.image);
            put_vbytes(out, &row);
        }
        Ok(())
    }

    /// Decodes a `FileObs` payload; each row's bytes are consumed exactly ([F05 §8.5]).
    pub fn decode(payload: &[u8]) -> Result<RowBatch<'_>, DecodeError> {
        let mut r = Reader::new(payload);
        let n = r.uvar32()?;
        if n == 0 {
            return Err(r.err("a row batch with no row"));
        }
        let mut rows = Vec::with_capacity(n.min(1 << 12) as usize);
        for _ in 0..n {
            let row = r.vbytes()?;
            let Some((&op, image)) = row.split_first() else {
                return Err(r.err("an empty row"));
            };
            match op {
                1 => {}
                2 => return Err(r.err("a delete row: outside the probe's subset")),
                _ => return Err(r.err(format!("row op {op} is invalid"))),
            }
            check_image(&r, image)?;
            rows.push(Row {
                op,
                image: Cow::Borrowed(image),
            });
        }
        r.end()?;
        Ok(RowBatch { rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_reg() -> TreeReg {
        TreeReg {
            tree: [0xA1; 16],
            epoch: Epoch {
                scope_kind: 1,
                scope_ref: 0x0403_0201,
                digest: [0xD5; 16],
                hlc: 0x0C0B_0A09_0807_0605,
            },
        }
    }

    /// The `TreeReg` sub-2 payload ([F05 §9.23], `05-log.md:1010-1023`): `sub` u8, `tree` b16, then the 32-byte
    /// `Epoch` of [F11 §12.4] (`11-runtime-tables.md:855-864`): 49 bytes. F05's "48 bytes" (`:1028-1030`) counts the
    /// tree key and the epoch, without the `sub` discriminant.
    #[test]
    fn treereg_epoch_payload_is_49_bytes() {
        let t = tree_reg();
        let mut out = Vec::new();
        t.encode(&mut out);
        assert_eq!(out.len(), TREE_REG_EPOCH_PAYLOAD_LEN);
        assert_eq!(TREE_REG_EPOCH_PAYLOAD_LEN, 1 + 16 + EPOCH_LEN);
        assert_eq!(16 + EPOCH_LEN, 48, "F05's tree key plus epoch");
        assert_eq!(out[0], 2);
        assert_eq!(&out[1..17], &[0xA1; 16]);
        // Epoch: scope_kind at 0, _reserved [3] at 1, scope_ref at 4, digest at 8, hlc at 24.
        let e = &out[17..];
        assert_eq!(e[0], 1);
        assert_eq!(&e[1..4], &[0, 0, 0]);
        assert_eq!(&e[4..8], &[1, 2, 3, 4]);
        assert_eq!(&e[8..24], &[0xD5; 16]);
        assert_eq!(&e[24..32], &[5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(TreeReg::decode(&out), Ok(t));
        let mut sub1 = out.clone();
        sub1[0] = 1;
        assert!(
            TreeReg::decode(&sub1)
                .unwrap_err()
                .what
                .contains("outside the probe's subset")
        );
        let mut kind3 = out.clone();
        kind3[17] = 3;
        assert!(
            TreeReg::decode(&kind3)
                .unwrap_err()
                .what
                .contains("scope_kind 3")
        );
        let mut res = out;
        res[18] = 1;
        assert!(
            TreeReg::decode(&res)
                .unwrap_err()
                .what
                .contains("_reserved")
        );
    }

    fn row(n: u32) -> FileObsRow {
        FileObsRow {
            n,
            tree: [0xB2; 16],
            state: 1,
            resolver_version: 0x0201,
            file_id: [0x0F; OS_FILE_ID_LEN],
            size: 0x1817_1615_1413_1211,
            times: [
                [0x21; FS_TIME_LEN],
                [0x22; FS_TIME_LEN],
                [0x23; FS_TIME_LEN],
                [0x24; FS_TIME_LEN],
            ],
            attrs: 0x0000_0041,
            last_oid: [0x33; OID_SLOT_LEN],
            verified_at: 0x4444_4444_4444_4444,
            missing_since: 0x5555_5555_5555_5555,
        }
    }

    /// The `FILEOBS` row ([F11 §12.5], `11-runtime-tables.md:895-914`): `OsFileId` 57, `FsTime` 9, `FileAttrs` 4,
    /// `OidSlot` 33 and `HeapRef` 8 (`:180-193`) make 187 bytes; the image with an empty `var` is the 187 fixed bytes
    /// with `var` = (0, 0) ([F11 §2.9], `:282-286`).
    #[test]
    fn fileobs_row_image_is_187_bytes() {
        assert_eq!(
            (
                OS_FILE_ID_LEN,
                FS_TIME_LEN,
                FILE_ATTRS_LEN,
                OID_SLOT_LEN,
                HEAP_REF_LEN
            ),
            (57, 9, 4, 33, 8)
        );
        let img = row(0x0403_0201).image();
        assert_eq!(img.len(), FILEOBS_ROW_LEN);
        assert_eq!(FILEOBS_ROW_LEN, 187);
        assert_eq!(&img[0..4], &[1, 2, 3, 4], "n at 0");
        assert_eq!(&img[4..20], &[0xB2; 16], "tree at 4");
        assert_eq!((img[20], img[21]), (1, 0), "state at 20, flags at 21");
        assert_eq!(&img[22..24], &[1, 2], "resolver_version at 22");
        assert_eq!(img[24], 0, "n_proposals at 24");
        assert_eq!(&img[25..82], &[0x0F; 57], "file_id at 25");
        assert_eq!(
            &img[82..90],
            &[0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18],
            "size at 82"
        );
        assert_eq!(&img[90..99], &[0x21; 9], "mtime at 90");
        assert_eq!(&img[99..108], &[0x22; 9], "ctime at 99");
        assert_eq!(&img[108..117], &[0x23; 9], "creation at 108");
        assert_eq!(&img[117..126], &[0x24; 9], "added at 117");
        assert_eq!(&img[126..130], &[0x41, 0, 0, 0], "attrs at 126");
        assert_eq!(&img[130..163], &[0x33; 33], "last_oid at 130");
        assert_eq!(&img[163..171], &[0x44; 8], "verified_at at 163");
        assert_eq!(&img[171..179], &[0x55; 8], "missing_since at 171");
        assert_eq!(&img[179..187], &[0; 8], "var at 179: off 0, len 0");
        let r = Row {
            op: 1,
            image: Cow::Owned(img.to_vec()),
        };
        assert_eq!(r.key(), (0x0403_0201, [0xB2; 16]));
    }

    /// A row batch ([F05 §8.5], `05-log.md:512-521`): `n_rows` uvar32, then each row as `vbytes` of `op` and the
    /// image, consumed exactly; a delete row and a row with a `var` sequence are outside the subset.
    #[test]
    fn row_batch_frames_each_row() {
        let b = RowBatch {
            rows: [7u32, 9]
                .iter()
                .map(|&n| Row {
                    op: 1,
                    image: Cow::Owned(row(n).image().to_vec()),
                })
                .collect(),
        };
        let mut out = Vec::new();
        b.encode(&mut out).unwrap();
        // n_rows 02, then per row: length 188 as uvar32 (BC 01), op 01, 187 bytes.
        assert_eq!(out.len(), 1 + 2 * (2 + 188));
        assert_eq!(&out[..4], &[0x02, 0xBC, 0x01, 0x01]);
        assert_eq!(&out[4..8], &7u32.to_le_bytes());
        assert_eq!(&out[191..195], &[0xBC, 0x01, 0x01, 0x09]);
        assert_eq!(RowBatch::decode(&out), Ok(b.clone()));
        let mut delete = out.clone();
        delete[3] = 2;
        assert!(
            RowBatch::decode(&delete)
                .unwrap_err()
                .what
                .contains("delete row")
        );
        let mut detail = out.clone();
        detail[3 + 1 + 21] = 2; // flags bit 1: has_detail
        assert!(
            RowBatch::decode(&detail)
                .unwrap_err()
                .what
                .contains("outside the probe's subset")
        );
        let mut bad_var = out.clone();
        bad_var[3 + 1 + 183] = 1; // var.len 1 with no slice
        assert!(
            RowBatch::decode(&bad_var)
                .unwrap_err()
                .what
                .contains("HeapRef")
        );
        let mut extra = out;
        extra.push(0);
        assert!(
            RowBatch::decode(&extra)
                .unwrap_err()
                .what
                .contains("left after")
        );
        assert!(RowBatch::default().encode(&mut Vec::new()).is_err());
    }
}
