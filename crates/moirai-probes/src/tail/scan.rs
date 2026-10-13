//! The scan of [F05 §5] (`docs/spec/format/05-log.md:268-341`): from a group boundary whose chain value it knows, it
//! reads the log through a fixed buffer, checks every record ([F05 §5.2]) and group ([F05 §4.6]), the two placement
//! defects of [F05 §5.4] and the `ExtentHead` rules of [F05 §9.28] (`:1133-1135`), and hands each valid group to its
//! caller in log order ([F05 §10.1], `:1234-1236`).
//!
//! The probe's tail lies wholly below `HEAD.durable_lsn`, so every invalid record or group is corruption, never the end
//! of the log ([F05 §5.3], `:305-309`): the scan returns an error. A kind outside the probe's subset is refused with an
//! error that says so.

use super::rec::{
    ExtentHead, Payload, RecHdr, Record, chain, decode_symdefs, lazy_kind, record_checksum,
};
use super::{
    EXTENT_HEAD_GROUP_LEN, Geometry, INIT_PARAMS_LEN, Locator, MIN_GROUP_LEN, REC_HDR_LEN,
    TRAILER_LEN, flag, kind,
};
use crate::tail::commit::Commit;
use crate::tail::enc::{DecodeError, Reader};
use crate::tail::rt::{RowBatch, TreeReg};

/// Where the scan reads the log: positional reads by lsn, each inside one extent.
pub trait LogSource {
    /// Reads up to `buf.len()` bytes at `lsn`; the caller never asks across the end of `lsn`'s extent. Returns the
    /// bytes read, fewer only at the end of the log's bytes (0 there).
    fn read_at(&mut self, lsn: u64, buf: &mut [u8]) -> Result<usize, String>;
}

/// A log held in memory: `bytes[0]` is lsn `start`. It records each read's lsn and length.
#[derive(Clone, Debug)]
pub struct MemLog<'a> {
    start: u64,
    bytes: &'a [u8],
    /// Every read: (lsn, bytes asked).
    pub reads: Vec<(u64, usize)>,
}

impl<'a> MemLog<'a> {
    /// A log whose first byte is lsn `start`.
    pub fn new(start: u64, bytes: &'a [u8]) -> MemLog<'a> {
        MemLog {
            start,
            bytes,
            reads: Vec::new(),
        }
    }
}

impl LogSource for MemLog<'_> {
    fn read_at(&mut self, lsn: u64, buf: &mut [u8]) -> Result<usize, String> {
        self.reads.push((lsn, buf.len()));
        let Some(off) = lsn.checked_sub(self.start) else {
            return Err(format!("lsn {lsn} lies before the log's start"));
        };
        let off = usize::try_from(off).map_err(|_| "offset out of range".to_string())?;
        let n = self.bytes.len().saturating_sub(off).min(buf.len());
        buf[..n]
            .copy_from_slice(&self.bytes[off.min(self.bytes.len())..off.min(self.bytes.len()) + n]);
        Ok(n)
    }
}

/// What the scan checks the log against: the extent length and the fields of the `HEAD` slot it uses
/// ([F05 §5.2] rule 6, [F05 §9.28]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Expect {
    /// E.
    pub geo: Geometry,
    /// `HEAD.epoch`.
    pub epoch: u64,
    /// `HEAD.epoch_lsn`.
    pub epoch_lsn: u64,
    /// `HEAD.init`.
    pub init: [u8; INIT_PARAMS_LEN],
    /// `HEAD.project_oid_algo`.
    pub project_oid_algo: u8,
    /// The form of item 1a's locator in the tail's op frames (u20 cut §3.1): the probe's store parameter, not a `HEAD`
    /// field. Every record is decoded in it.
    pub locator: Locator,
}

/// Why the scan stopped: the lsn of the record or group and the rule it breaks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanError {
    /// The lsn of the offending record or group.
    pub lsn: u64,
    /// The rule, or the subset feature met.
    pub what: String,
}

impl core::fmt::Display for ScanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "lsn {}: {}", self.lsn, self.what)
    }
}

impl std::error::Error for ScanError {}

fn at(lsn: u64, what: impl Into<String>) -> ScanError {
    ScanError {
        lsn,
        what: what.into(),
    }
}

/// One valid record of a group: its header and its payload `[32, len − t)`.
#[derive(Copy, Clone, Debug)]
pub struct RawRecord<'g> {
    /// The header.
    pub hdr: RecHdr,
    /// The payload, the `SymDefs` block included, without the trailer.
    pub payload: &'g [u8],
    /// The form its op frames are in.
    pub locator: Locator,
}

/// One valid group, as the scan hands it over.
#[derive(Copy, Clone, Debug)]
pub struct GroupView<'g> {
    /// The group's boundary.
    pub start: u64,
    /// The group's bytes.
    pub bytes: &'g [u8],
    recs: &'g [(RecHdr, u32)],
    locator: Locator,
}

impl<'g> GroupView<'g> {
    /// The group's records in order.
    pub fn records(&self) -> impl Iterator<Item = RawRecord<'g>> + 'g {
        let (bytes, locator) = (self.bytes, self.locator);
        self.recs.iter().map(move |&(hdr, off)| {
            let off = off as usize;
            let t = if hdr.group_end() { TRAILER_LEN } else { 0 };
            RawRecord {
                hdr,
                payload: &bytes[off + REC_HDR_LEN..off + hdr.len as usize - t],
                locator,
            }
        })
    }

    /// The group's end.
    pub fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }
}

/// The class rule of [F05 §6.1] and the registry of [F05 §7] for one header: `Ok` for a kind of the subset whose
/// `lazy` bit agrees with its class.
fn check_kind(h: &RecHdr) -> Result<(), String> {
    if h.kind == 0 || h.kind > kind::MAX_VALID {
        return Err(format!("record kind {} is not in the registry", h.kind));
    }
    if !matches!(
        h.kind,
        kind::COMMIT | kind::NOOP | kind::FILE_OBS | kind::TREE_REG | kind::EXTENT_HEAD
    ) {
        return Err(format!(
            "record kind {}: outside the probe's subset",
            h.kind
        ));
    }
    if (h.flags & flag::LAZY != 0) != lazy_kind(h.kind) {
        return Err(format!(
            "record kind {} with lazy = {}, against its class",
            h.kind,
            h.flags & flag::LAZY
        ));
    }
    if matches!(h.kind, kind::NOOP | kind::EXTENT_HEAD) && h.flags & flag::SYMDEFS != 0 {
        return Err("a Noop or ExtentHead with symdefs set".into());
    }
    Ok(())
}

/// The chunked reader: a window of at most one chunk over the log.
struct Window<'s, S: LogSource> {
    source: &'s mut S,
    geo: Geometry,
    to: u64,
    buf: Vec<u8>,
    lsn: u64,
    len: usize,
}

impl<S: LogSource> Window<'_, S> {
    /// Appends the bytes `[lsn, lsn + n)` to `dst`, reading through the window.
    fn fetch(&mut self, mut lsn: u64, mut n: usize, dst: &mut Vec<u8>) -> Result<(), ScanError> {
        while n > 0 {
            if !(self.lsn <= lsn && lsn < self.lsn + self.len as u64) {
                let want = (self.buf.len() as u64)
                    .min(self.geo.extent_end(lsn) - lsn)
                    .min(self.to - lsn) as usize;
                let got = self
                    .source
                    .read_at(lsn, &mut self.buf[..want])
                    .map_err(|e| at(lsn, format!("a failed read: {e}")))?;
                if got == 0 {
                    return Err(at(lsn, "the log's bytes end inside a group"));
                }
                self.lsn = lsn;
                self.len = got;
            }
            let off = (lsn - self.lsn) as usize;
            let k = n.min(self.len - off);
            dst.extend_from_slice(&self.buf[off..off + k]);
            lsn += k as u64;
            n -= k;
        }
        Ok(())
    }
}

/// Scans `[from, to)` from a group boundary `from` whose chain value is `chain_in`, reading through a buffer of
/// `chunk` bytes, and calls `on_group` with each valid group in log order. It holds one group and one chunk, never the
/// whole log. Returns the chain value at `to`, which must be a group boundary.
pub fn scan<S: LogSource>(
    source: &mut S,
    expect: &Expect,
    from: u64,
    to: u64,
    chain_in: u64,
    chunk: usize,
    on_group: &mut dyn FnMut(&GroupView<'_>) -> Result<(), ScanError>,
) -> Result<u64, ScanError> {
    let geo = expect.geo;
    let mut w = Window {
        source,
        geo,
        to,
        buf: vec![0u8; chunk.max(1)],
        lsn: 0,
        len: 0,
    };
    let mut group: Vec<u8> = Vec::new();
    let mut recs: Vec<(RecHdr, u32)> = Vec::new();
    let mut p = from;
    let mut chain_at = chain_in;
    while p < to {
        group.clear();
        recs.clear();
        let first_of_extent = geo.offset(p) == 0;
        loop {
            let q = p + group.len() as u64;
            if q + REC_HDR_LEN as u64 > to {
                return Err(at(q, "the scan's end falls inside a group"));
            }
            let off = group.len();
            w.fetch(q, REC_HDR_LEN, &mut group)?;
            let hdr = RecHdr::decode(group[off..off + REC_HDR_LEN].try_into().expect("32 bytes"));
            let len = u64::from(hdr.len);
            let bad = |what: String| Err(at(q, what));
            if len < REC_HDR_LEN as u64 || geo.offset(q) + len > geo.e() {
                return bad(format!("record length {len} (rule 2)"));
            }
            if hdr.group_end() && len < MIN_GROUP_LEN {
                return bad(format!("a group-ending record of {len} bytes (rule 2)"));
            }
            check_kind(&hdr).map_err(|e| at(q, e))?;
            if hdr.flags & flag::RESERVED != 0 || hdr.reserved != 0 {
                return bad("reserved header bits are set (rule 4)".into());
            }
            if hdr.lsn != q {
                return bad(format!("the record names lsn {} (rule 5)", hdr.lsn));
            }
            if hdr.epoch != expect.epoch {
                return bad(format!("the record's epoch {} (rule 6)", hdr.epoch));
            }
            if q + len > to {
                return Err(at(q, "the scan's end falls inside a record"));
            }
            w.fetch(
                q + REC_HDR_LEN as u64,
                len as usize - REC_HDR_LEN,
                &mut group,
            )?;
            if record_checksum(&group[off..], hdr.group_end()) != hdr.xxh3 {
                return bad("the record checksum does not match (rule 7)".into());
            }
            recs.push((hdr, off as u32));
            if hdr.group_end() {
                break;
            }
            if geo.offset(q + len) == 0 {
                return Err(at(p, "the extent ends before a record carrying group_end"));
            }
        }
        let e = p + group.len() as u64;
        let trailer = u64::from_le_bytes(
            group[group.len() - TRAILER_LEN..]
                .try_into()
                .expect("8 bytes"),
        );
        if chain(&group[..group.len() - TRAILER_LEN], chain_at) != trailer {
            return Err(at(p, "the chain trailer does not match (F05 §4.6)"));
        }
        let head_group = recs.len() == 1 && recs[0].0.kind == kind::EXTENT_HEAD;
        if first_of_extent && !head_group {
            return Err(at(
                p,
                "an extent's first group is not one ExtentHead (F05 §5.4)",
            ));
        }
        let left = geo.extent_end(e - 1) - e;
        if left > 0 && left < MIN_GROUP_LEN {
            return Err(at(
                p,
                format!("a group leaves {left} bytes in its extent (G-4, F05 §5.4)"),
            ));
        }
        let view = GroupView {
            start: p,
            bytes: &group,
            recs: &recs,
            locator: expect.locator,
        };
        for raw in view.records() {
            match raw.hdr.kind {
                kind::EXTENT_HEAD => {
                    let h = ExtentHead::decode(raw.payload)
                        .map_err(|e| at(raw.hdr.lsn, e.to_string()))?;
                    check_head(&h, raw.hdr.lsn, chain_at, expect, first_of_extent)?;
                    debug_assert_eq!(group.len() as u64, EXTENT_HEAD_GROUP_LEN);
                }
                kind::NOOP if raw.payload.iter().any(|&b| b != 0) => {
                    return Err(at(
                        raw.hdr.lsn,
                        "a Noop payload byte is not zero (F05 §9.12)",
                    ));
                }
                _ => {}
            }
        }
        on_group(&view)?;
        chain_at = trailer;
        p = e;
    }
    Ok(chain_at)
}

/// The rules of [F05 §9.28] (`05-log.md:1133-1135`).
fn check_head(
    h: &ExtentHead,
    lsn: u64,
    chain_at: u64,
    x: &Expect,
    first: bool,
) -> Result<(), ScanError> {
    if !first {
        return Err(at(
            lsn,
            "an ExtentHead that is not at an extent's first byte",
        ));
    }
    if h.chain_in != chain_at {
        return Err(at(
            lsn,
            "the ExtentHead's chain_in differs from the chain value at its lsn",
        ));
    }
    if h.epoch_lsn != x.epoch_lsn || h.init != x.init || h.project_oid_algo != x.project_oid_algo {
        return Err(at(
            lsn,
            "the ExtentHead's epoch_lsn, init or project_oid_algo differ from HEAD's",
        ));
    }
    Ok(())
}

/// Decodes one valid record's payload ([F05 §5.4]: a malformed payload of a valid record is corrupt).
pub fn decode_record<'g>(raw: &RawRecord<'g>) -> Result<Record<'g>, ScanError> {
    let lsn = raw.hdr.lsn;
    let shift = |e: DecodeError| at(lsn, e.to_string());
    let mut r = Reader::new(raw.payload);
    let symdefs = if raw.hdr.flags & flag::SYMDEFS != 0 {
        decode_symdefs(&mut r).map_err(shift)?
    } else {
        Vec::new()
    };
    let rest = &raw.payload[r.pos()..];
    let payload = match raw.hdr.kind {
        kind::EXTENT_HEAD => Payload::ExtentHead(ExtentHead::decode(rest).map_err(shift)?),
        kind::NOOP => Payload::Noop(rest.len() as u32),
        kind::COMMIT => Payload::Commit(Box::new(
            Commit::decode_in(rest, raw.locator).map_err(shift)?,
        )),
        kind::TREE_REG => Payload::TreeReg(TreeReg::decode(rest).map_err(shift)?),
        kind::FILE_OBS => Payload::FileObs(RowBatch::decode(rest).map_err(shift)?),
        k => {
            return Err(at(
                lsn,
                format!("record kind {k}: outside the probe's subset"),
            ));
        }
    };
    Ok(Record {
        lsn,
        group_end: raw.hdr.group_end(),
        symdefs,
        payload,
    })
}

/// Decodes every record of `bytes` (whose first byte is lsn `start`, a group boundary with chain value `chain_in`):
/// the scan over the whole buffer, then each record's payload, borrowed from `bytes`.
pub fn decode<'a>(
    bytes: &'a [u8],
    start: u64,
    expect: &Expect,
    chain_in: u64,
) -> Result<Vec<Record<'a>>, ScanError> {
    let mut spans: Vec<(u64, u32, bool)> = Vec::new();
    let mut src = MemLog::new(start, bytes);
    scan(
        &mut src,
        expect,
        start,
        start + bytes.len() as u64,
        chain_in,
        bytes.len().max(1),
        &mut |g| {
            spans.extend(
                g.records()
                    .map(|r| (r.hdr.lsn, r.hdr.len, r.hdr.group_end())),
            );
            Ok(())
        },
    )?;
    spans
        .into_iter()
        .map(|(lsn, len, end)| {
            let off = (lsn - start) as usize;
            let rec = &bytes[off..off + len as usize];
            let t = if end { TRAILER_LEN } else { 0 };
            decode_record(&RawRecord {
                hdr: RecHdr::decode(rec[..REC_HDR_LEN].try_into().expect("32 bytes")),
                payload: &rec[REC_HDR_LEN..rec.len() - t],
                locator: expect.locator,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tail::rec::{Appender, encode};
    use crate::tail::testdata::{self, CHAIN_IN, E, START};

    /// One group of one record with any kind, flags and payload, with a valid checksum and trailer for `seed`.
    fn raw(kind: u8, flags: u8, payload: &[u8], lsn: u64, epoch: u64, seed: u64) -> Vec<u8> {
        let len = REC_HDR_LEN + payload.len() + TRAILER_LEN;
        let hdr = RecHdr {
            len: len as u32,
            kind,
            flags: flags | flag::GROUP_END,
            reserved: 0,
            lsn,
            epoch,
            xxh3: 0,
        };
        let mut b = hdr.encode().to_vec();
        b.extend_from_slice(payload);
        b.extend_from_slice(&[0; TRAILER_LEN]);
        let sum = record_checksum(&b, true);
        b[24..32].copy_from_slice(&sum.to_le_bytes());
        let c = chain(&b[..len - TRAILER_LEN], seed);
        b[len - TRAILER_LEN..].copy_from_slice(&c.to_le_bytes());
        b
    }

    /// Extent 2's head alone, as the start of a stream; returns the bytes and the chain value after it.
    fn head_only(x: &Expect) -> (Vec<u8>, u64) {
        let mut app = Appender::new(x.geo, x.epoch, START, CHAIN_IN, testdata::head(x));
        app.place(|lsn| Ok(testdata::settle_group(lsn, 1, 1, 1, 1)))
            .unwrap();
        let b = app.bytes()[..EXTENT_HEAD_GROUP_LEN as usize].to_vec();
        let c = u64::from_le_bytes(b[130..138].try_into().unwrap());
        (b, c)
    }

    fn err(bytes: &[u8], x: &Expect) -> ScanError {
        decode(bytes, START, x, CHAIN_IN).unwrap_err()
    }

    /// A hand-built tail of every kind of the subset — extent heads, commits with a `SymDefs` block, a body and the
    /// eight ops, settles, a pad and a rotation at E = 2^16 — decodes to the records that built it and re-encodes byte
    /// for byte.
    #[test]
    fn decoder_reads_back_a_hand_built_tail() {
        let x = testdata::expect();
        let hb = testdata::hand_built();
        let recs = decode(&hb.bytes, START, &x, CHAIN_IN).unwrap();
        assert_eq!(recs, hb.records);
        assert_eq!(
            encode(&recs, x.geo, START, x.epoch, CHAIN_IN).unwrap(),
            hb.bytes
        );
        let kinds: Vec<u8> = recs.iter().map(Record::kind).collect();
        for k in [
            kind::EXTENT_HEAD,
            kind::COMMIT,
            kind::TREE_REG,
            kind::FILE_OBS,
            kind::NOOP,
        ] {
            assert!(kinds.contains(&k), "kind {k}");
        }
        assert!(
            recs.iter().any(|r| r.lsn >= 2 * E),
            "the tail crosses into extent 3"
        );
        assert_eq!(kinds.iter().filter(|&&k| k == kind::EXTENT_HEAD).count(), 2);
        assert!(
            !recs[1].symdefs.is_empty(),
            "the first commit defines its symbols"
        );
        assert_eq!(hb.commits.len(), 7);
    }

    /// Every rule of [F05 §5.2] (`05-log.md:286-296`), the group rules of §4.6 and §5.3, the placement defects of
    /// §5.4 (`:331-335`), the `ExtentHead` rules of §9.28 (`:1133-1135`) and a malformed payload refuse the tail with
    /// their reason; the unmodified tail decodes.
    #[test]
    fn scan_refuses_each_invalid_record_and_group() {
        let x = testdata::expect();
        let good = testdata::hand_built().bytes;
        assert!(decode(&good, START, &x, CHAIN_IN).is_ok());
        let at_commit = START + EXTENT_HEAD_GROUP_LEN;
        let c = EXTENT_HEAD_GROUP_LEN as usize;
        type Mutation = Box<dyn Fn(&mut Vec<u8>)>;
        let cases: Vec<(&str, Mutation)> = vec![
            (
                "record length 31",
                Box::new(move |b| b[c..c + 4].copy_from_slice(&31u32.to_le_bytes())),
            ),
            (
                "record length 65536",
                Box::new(move |b| b[c..c + 4].copy_from_slice(&65_536u32.to_le_bytes())),
            ),
            (
                "group-ending record of 36",
                Box::new(move |b| b[c..c + 4].copy_from_slice(&36u32.to_le_bytes())),
            ),
            ("not in the registry", Box::new(move |b| b[c + 4] = 0)),
            ("reserved header bits", Box::new(move |b| b[c + 5] |= 0x08)),
            ("reserved header bits", Box::new(move |b| b[c + 6] = 1)),
            (
                "against its class",
                Box::new(move |b| b[c + 5] |= flag::LAZY),
            ),
            ("rule 5", Box::new(move |b| b[c + 8] ^= 1)),
            ("rule 6", Box::new(move |b| b[c + 16] ^= 1)),
            ("rule 7", Box::new(move |b| b[c + 40] ^= 1)),
            (
                "chain trailer",
                Box::new(move |b| {
                    let len = u32::from_le_bytes(b[c..c + 4].try_into().unwrap()) as usize;
                    b[c + len - 1] ^= 1;
                }),
            ),
        ];
        for (why, mutate) in cases {
            let mut b = good.clone();
            mutate(&mut b);
            let e = err(&b, &x);
            assert!(e.what.contains(why), "{why}: {e}");
            assert_eq!(e.lsn, at_commit, "{why}");
        }
        let (head, seed) = head_only(&x);
        let geo = x.geo;
        let noop = |lsn: u64, n: u32, end: bool| Record {
            lsn,
            group_end: end,
            symdefs: Vec::new(),
            payload: Payload::Noop(n),
        };
        let head_rec = |h: ExtentHead| Record {
            lsn: START,
            group_end: true,
            symdefs: Vec::new(),
            payload: Payload::ExtentHead(h),
        };
        let h0 = ExtentHead {
            chain_in: CHAIN_IN,
            ..testdata::head(&x)
        };
        let enc = |recs: &[Record<'_>]| encode(recs, geo, START, x.epoch, CHAIN_IN).unwrap();
        let after = START + EXTENT_HEAD_GROUP_LEN;
        // The extent ends before a record carrying group_end.
        let rest = (E - EXTENT_HEAD_GROUP_LEN - 32) as u32;
        let b = enc(&[head_rec(h0), noop(after, rest, false), noop(2 * E, 0, true)]);
        assert!(
            err(&b, &x)
                .what
                .contains("before a record carrying group_end")
        );
        // An extent's first group that is not one ExtentHead.
        let b = enc(&[noop(START, 0, true)]);
        assert!(err(&b, &x).what.contains("not one ExtentHead"));
        // A group that leaves 20 bytes in its extent (G-4).
        let b = enc(&[
            head_rec(h0),
            noop(after, (E - EXTENT_HEAD_GROUP_LEN - 60) as u32, true),
        ]);
        assert!(err(&b, &x).what.contains("leaves 20 bytes"));
        // The ExtentHead's chain_in and init must match.
        let b = enc(&[head_rec(ExtentHead {
            chain_in: CHAIN_IN ^ 1,
            ..h0
        })]);
        assert!(err(&b, &x).what.contains("chain_in"));
        let mut init = h0.init;
        init[0] ^= 1;
        let b = enc(&[head_rec(ExtentHead { init, ..h0 })]);
        assert!(err(&b, &x).what.contains("init"));
        // An ExtentHead that is not at an extent's first byte.
        let mut b = head.clone();
        let mut second = Vec::new();
        ExtentHead {
            chain_in: seed,
            ..h0
        }
        .encode(&mut second);
        b.extend(raw(kind::EXTENT_HEAD, 0, &second, after, x.epoch, seed));
        assert!(err(&b, &x).what.contains("not at an extent's first byte"));
        // A Noop with a non-zero payload byte.
        let mut b = head.clone();
        b.extend(raw(kind::NOOP, flag::LAZY, &[0, 1], after, x.epoch, seed));
        assert!(err(&b, &x).what.contains("Noop payload"));
        let mut b = head.clone();
        b.extend(raw(kind::NOOP, flag::LAZY, &[0, 0], after, x.epoch, seed));
        assert!(decode(&b, START, &x, CHAIN_IN).is_ok(), "the valid form");
        // A valid record whose payload is malformed is corrupt ([F05 §5.4]): a Commit with a reserved presence bit.
        let mut b = head.clone();
        b.extend(raw(kind::COMMIT, 0, &[0, 0, 0x10, 0], after, x.epoch, seed));
        assert!(err(&b, &x).what.contains("reserved bit 20-31"));
        // The scan's end inside a record.
        let short = &good[..good.len() - 1];
        assert!(err(short, &x).what.contains("inside"));
    }

    /// The scan reads through a fixed buffer ([AR §4.7], `ARCHITECTURE-RESEARCH.md:871`): with a 64-byte chunk every
    /// read is at most 64 bytes, never across an extent, and the groups, the chain value and the records are those of
    /// a whole-buffer scan.
    #[test]
    fn scan_streams_through_a_fixed_buffer() {
        let x = testdata::expect();
        let b = testdata::hand_built().bytes;
        let end = START + b.len() as u64;
        let groups = |chunk: usize| {
            let mut src = MemLog::new(START, &b);
            let mut seen: Vec<(u64, Vec<u8>)> = Vec::new();
            let c = scan(&mut src, &x, START, end, CHAIN_IN, chunk, &mut |g| {
                seen.push((g.start, g.bytes.to_vec()));
                Ok(())
            })
            .unwrap();
            (seen, c, src.reads)
        };
        let (whole, c1, r1) = groups(b.len());
        let (small, c2, r2) = groups(64);
        assert_eq!(whole, small);
        assert_eq!(c1, c2);
        assert!(whole.len() > 8, "a non-empty tail");
        assert_eq!(whole.iter().map(|(_, g)| g.len()).sum::<usize>(), b.len());
        assert!(
            r2.iter().all(|&(_, n)| n <= 64),
            "every read fits the chunk"
        );
        assert!(r2.len() >= b.len() / 64);
        assert!(
            r1.len() <= 2,
            "one read per extent with a whole-buffer chunk"
        );
        let geo = x.geo;
        assert!(
            r2.iter()
                .all(|&(l, n)| geo.extent(l) == geo.extent(l + n as u64 - 1))
        );
    }

    /// A record kind outside the subset — valid in [F05 §7] but not one the probe writes — is refused as such; an
    /// unregistered kind as invalid; a `Noop` of the subset after the same head decodes.
    #[test]
    fn scan_refuses_kinds_outside_the_subset() {
        let x = testdata::expect();
        let (head, seed) = head_only(&x);
        let after = START + EXTENT_HEAD_GROUP_LEN;
        for k in [2u8, 9, 11, 19, 29, 30] {
            let lazy = if (18..=26).contains(&k) {
                flag::LAZY
            } else {
                0
            };
            let mut b = head.clone();
            b.extend(raw(k, lazy, &[0; 8], after, x.epoch, seed));
            let e = err(&b, &x);
            assert!(
                e.what.contains("outside the probe's subset"),
                "kind {k}: {e}"
            );
            assert_eq!(e.lsn, after);
        }
        for k in [0u8, 31, 255] {
            let mut b = head.clone();
            b.extend(raw(k, 0, &[0; 8], after, x.epoch, seed));
            assert!(err(&b, &x).what.contains("not in the registry"), "kind {k}");
        }
        let mut b = head;
        b.extend(raw(kind::NOOP, flag::LAZY, &[0; 8], after, x.epoch, seed));
        let recs = decode(&b, START, &x, CHAIN_IN).unwrap();
        assert_eq!(recs.last().map(Record::kind), Some(kind::NOOP));
    }

    /// A tail in A1 (u20 cut §3.1) decodes under its own form: the hand-built A1 tail's records come back with their
    /// locators and re-encode byte for byte, and some commit carries one; read in the night's form, the same bytes do
    /// not give its records.
    #[test]
    fn a1_tail_decodes_under_its_own_form() {
        let a1 = testdata::hand_built_in(Locator::A1);
        assert_eq!(a1.expect.locator, Locator::A1);
        let recs = decode(&a1.bytes, START, &a1.expect, CHAIN_IN).unwrap();
        assert_eq!(recs, a1.records);
        assert_eq!(
            encode(&recs, a1.expect.geo, START, a1.expect.epoch, CHAIN_IN).unwrap(),
            a1.bytes
        );
        let located: usize = recs
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(c) => Some((c.form, c.locs.len())),
                _ => None,
            })
            .map(|(form, n)| {
                assert_eq!(form, Locator::A1);
                n
            })
            .sum();
        assert!(located >= 8, "{located} locators in the hand-built A1 tail");
        let night = testdata::hand_built();
        assert_ne!(night.bytes, a1.bytes);
        let as_night = decode(&a1.bytes, START, &testdata::expect(), CHAIN_IN);
        assert!(as_night.as_ref() != Ok(&a1.records), "{as_night:?}");
    }
}
