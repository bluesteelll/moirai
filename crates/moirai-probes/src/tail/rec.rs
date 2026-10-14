//! Records, groups and placement ([F05 §3], [F05 §4], `docs/spec/format/05-log.md:116-266`): `RecHdr`, the record
//! checksum, the chain trailer and the chain value at a boundary, the `SymDefs` block ([F05 §8.1], `:419-460`), the
//! `Noop` pad ([F05 §9.12], `:887-894`) and `ExtentHead` ([F05 §9.28], `:1111-1139`), the record type of the subset,
//! [`encode`], [`Appender`], which places groups by G-1 to G-5, and [`raw_group`], the one-record group of a record of
//! any kind (u21 cut §3.7: the `Checkpoint` record of [`super::ckpt`]).

use super::commit::Commit;
use super::enc::{DecodeError, Reader, put_u32, put_u64, put_uvar, put_vstr};
use super::rt::{RowBatch, TreeReg};
use super::{
    EXTENT_HEAD_GROUP_LEN, EXTENT_HEAD_PAYLOAD_LEN, Geometry, INIT_PARAMS_LEN, MIN_GROUP_LEN,
    REC_HDR_LEN, ROTATION_RESERVE, TRAILER_LEN, flag, kind,
};
use std::borrow::Cow;
use xxhash_rust::xxh3::{Xxh3, Xxh3Default, xxh3_64};

/// `RecHdr` ([F05 §3.1], `05-log.md:118-129`): `len` at 0 (4), `kind` at 4, `flags` at 5, `_reserved` at 6 (2), `lsn`
/// at 8 (8), `epoch` at 16 (8), `xxh3_64` at 24 (8); 32 bytes, little-endian.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RecHdr {
    /// The record's total length: header, payload and the trailer when `group_end` is set.
    pub len: u32,
    /// The record kind.
    pub kind: u8,
    /// The flags of [F05 §3.2].
    pub flags: u8,
    /// Reserved-zero.
    pub reserved: u16,
    /// The record's own lsn.
    pub lsn: u64,
    /// The store epoch.
    pub epoch: u64,
    /// The record checksum (§3.4).
    pub xxh3: u64,
}

impl RecHdr {
    /// The 32 bytes.
    pub fn encode(&self) -> [u8; REC_HDR_LEN] {
        let mut b = [0u8; REC_HDR_LEN];
        b[0..4].copy_from_slice(&self.len.to_le_bytes());
        b[4] = self.kind;
        b[5] = self.flags;
        b[6..8].copy_from_slice(&self.reserved.to_le_bytes());
        b[8..16].copy_from_slice(&self.lsn.to_le_bytes());
        b[16..24].copy_from_slice(&self.epoch.to_le_bytes());
        b[24..32].copy_from_slice(&self.xxh3.to_le_bytes());
        b
    }

    /// The fields of 32 bytes; every bit pattern is a header (validity is the scan's, [F05 §5.2]).
    pub fn decode(b: &[u8; REC_HDR_LEN]) -> RecHdr {
        let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().expect("8 bytes"));
        RecHdr {
            len: u32::from_le_bytes(b[0..4].try_into().expect("4 bytes")),
            kind: b[4],
            flags: b[5],
            reserved: u16::from_le_bytes([b[6], b[7]]),
            lsn: u64_at(8),
            epoch: u64_at(16),
            xxh3: u64_at(24),
        }
    }

    /// Whether `group_end` is set.
    pub const fn group_end(&self) -> bool {
        self.flags & flag::GROUP_END != 0
    }
}

/// The record checksum of [F05 §3.4] (`05-log.md:155-157`): XXH3-64, seed 0, over `[0, 24)` then `[32, len − t)` of the
/// record's bytes, `t` = 8 when `group_end` is set. `rec` is the whole record, trailer included when it has one.
pub fn record_checksum(rec: &[u8], group_end: bool) -> u64 {
    let t = if group_end { TRAILER_LEN } else { 0 };
    let mut h = Xxh3Default::new();
    h.update(&rec[..24]);
    h.update(&rec[REC_HDR_LEN..rec.len() - t]);
    h.digest()
}

/// The chain value at `HEAD.epoch_lsn`: XXH3-64 with seed 0 over the 8 little-endian bytes of the epoch ([F05 §4.3],
/// `05-log.md:182`).
pub fn epoch_seed(epoch: u64) -> u64 {
    xxh3_64(&epoch.to_le_bytes())
}

/// The chain trailer of a group ([F05 §4.2], `05-log.md:171-176`): XXH3-64 seeded with the chain value at the group's
/// boundary, over every byte of the group before the trailer. `body` is that range.
pub fn chain(body: &[u8], seed: u64) -> u64 {
    let mut h = Xxh3::with_seed(seed);
    h.update(body);
    h.digest()
}

/// `InitParams` ([F04 §4.4], `04-head.md:147-153`): `log_extent_bytes` u64 at 0, `hist_frame_commits` u32 at 8,
/// `hist_frame_bytes` u32 at 12, `store_id` b16 at 16; 32 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct InitParams {
    /// E.
    pub log_extent_bytes: u64,
    /// P03.
    pub hist_frame_commits: u32,
    /// P04.
    pub hist_frame_bytes: u32,
    /// The store id, never all zero.
    pub store_id: [u8; 16],
}

impl InitParams {
    /// The 32 bytes.
    pub fn encode(&self) -> [u8; INIT_PARAMS_LEN] {
        let mut b = [0u8; INIT_PARAMS_LEN];
        b[0..8].copy_from_slice(&self.log_extent_bytes.to_le_bytes());
        b[8..12].copy_from_slice(&self.hist_frame_commits.to_le_bytes());
        b[12..16].copy_from_slice(&self.hist_frame_bytes.to_le_bytes());
        b[16..32].copy_from_slice(&self.store_id);
        b
    }
}

/// The `ExtentHead` payload ([F05 §9.28], `05-log.md:1117-1131`), 98 bytes: `epoch_lsn` u64, `chain_in` u64, `init` (32),
/// `project_oid_algo` u8, `hflags` u8, `commit_seq` u64, `next_id` u32, `next_anchor` u32, `fence` u64, `next_file_no`
/// u32, `next_ref_id` u32, `hlc_seq` u64, `hlc_commit` u64.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ExtentHead {
    /// `HEAD.epoch_lsn`.
    pub epoch_lsn: u64,
    /// The chain value at the record's own lsn.
    pub chain_in: u64,
    /// `HEAD.init`.
    pub init: [u8; INIT_PARAMS_LEN],
    /// `HEAD.project_oid_algo`.
    pub project_oid_algo: u8,
    /// Bit 0 `quiet`, bit 1 `readonly`; bits 2–7 reserved-zero.
    pub hflags: u8,
    /// The `seq` of the newest commit before the record.
    pub commit_seq: u64,
    /// The next `#N`.
    pub next_id: u32,
    /// The next `aN`.
    pub next_anchor: u32,
    /// The fencing token.
    pub fence: u64,
    /// The next sealed-file number.
    pub next_file_no: u32,
    /// The next ref id.
    pub next_ref_id: u32,
    /// The greatest value of the HLC sequence before the record.
    pub hlc_seq: u64,
    /// The greatest commit `hlc` before the record.
    pub hlc_commit: u64,
}

impl ExtentHead {
    /// The 98 payload bytes.
    pub fn encode(&self, out: &mut Vec<u8>) {
        put_u64(out, self.epoch_lsn);
        put_u64(out, self.chain_in);
        out.extend_from_slice(&self.init);
        out.push(self.project_oid_algo);
        out.push(self.hflags);
        put_u64(out, self.commit_seq);
        put_u32(out, self.next_id);
        put_u32(out, self.next_anchor);
        put_u64(out, self.fence);
        put_u32(out, self.next_file_no);
        put_u32(out, self.next_ref_id);
        put_u64(out, self.hlc_seq);
        put_u64(out, self.hlc_commit);
    }

    /// Decodes the 98 payload bytes; `hflags` bits 2–7 are reserved-zero.
    pub fn decode(payload: &[u8]) -> Result<ExtentHead, DecodeError> {
        let mut r = Reader::new(payload);
        if payload.len() != EXTENT_HEAD_PAYLOAD_LEN {
            return Err(r.err(format!(
                "an ExtentHead payload of {} bytes, not 98",
                payload.len()
            )));
        }
        let h = ExtentHead {
            epoch_lsn: r.u64()?,
            chain_in: r.u64()?,
            init: r.array()?,
            project_oid_algo: r.u8()?,
            hflags: r.u8()?,
            commit_seq: r.u64()?,
            next_id: r.u32()?,
            next_anchor: r.u32()?,
            fence: r.u64()?,
            next_file_no: r.u32()?,
            next_ref_id: r.u32()?,
            hlc_seq: r.u64()?,
            hlc_commit: r.u64()?,
        };
        if h.hflags & !3 != 0 {
            return Err(r.err("ExtentHead hflags bits 2-7 are reserved-zero"));
        }
        Ok(h)
    }
}

/// One definition of a `SymDefs` block ([F05 §8.1], `05-log.md:429-450`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymDef<'a> {
    /// The class, 1–12.
    pub class: u8,
    /// The new id: `uvar16` for `role` (2) and `root` (8), `uvar32` otherwise.
    pub id: u32,
    /// The string; not empty.
    pub text: Cow<'a, str>,
}

/// The symbol classes of [F05 §8.1] (`05-log.md:437-450`) the tail defines.
pub mod sym {
    /// `role`.
    pub const ROLE: u8 = 2;
    /// `session`.
    pub const SESSION: u8 = 3;
    /// `stmt`.
    pub const STMT: u8 = 7;
    /// `root`.
    pub const ROOT: u8 = 8;
}

fn sym_is_16(class: u8) -> bool {
    class == sym::ROLE || class == sym::ROOT
}

/// Appends a `SymDefs` block.
pub fn encode_symdefs(defs: &[SymDef<'_>], out: &mut Vec<u8>) -> Result<(), String> {
    if defs.is_empty() || defs.len() > usize::from(u16::MAX) {
        return Err(format!(
            "a SymDefs block holds 1 to 65,535 definitions, not {}",
            defs.len()
        ));
    }
    put_uvar(out, defs.len() as u64);
    for d in defs {
        if !(1..=12).contains(&d.class) || d.text.is_empty() {
            return Err(format!("an invalid symbol definition {d:?}"));
        }
        if sym_is_16(d.class) && d.id > u32::from(u16::MAX) {
            return Err(format!("a class-{} id above 65,535", d.class));
        }
        out.push(d.class);
        put_uvar(out, u64::from(d.id));
        put_vstr(out, &d.text);
    }
    Ok(())
}

/// Reads a `SymDefs` block; within the block the definitions of one class have consecutive increasing ids (SD-1,
/// `05-log.md:453-455`).
pub fn decode_symdefs<'a>(r: &mut Reader<'a>) -> Result<Vec<SymDef<'a>>, DecodeError> {
    let n = r.uvar16()?;
    if n == 0 {
        return Err(r.err("a SymDefs block with no definition"));
    }
    let mut defs: Vec<SymDef<'a>> = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        let class = r.u8()?;
        if !(1..=12).contains(&class) {
            return Err(r.err(format!("symbol class {class} is not in the registry")));
        }
        let id = if sym_is_16(class) {
            u32::from(r.uvar16()?)
        } else {
            r.uvar32()?
        };
        let text = r.vstr()?;
        if text.is_empty() {
            return Err(r.err("a symbol definition with an empty string"));
        }
        if let Some(prev) = defs.iter().rev().find(|d| d.class == class)
            && prev.id.checked_add(1) != Some(id)
        {
            return Err(r.err(format!(
                "class-{class} ids {} then {id} in one block are not consecutive",
                prev.id
            )));
        }
        defs.push(SymDef {
            class,
            id,
            text: Cow::Borrowed(text),
        });
    }
    Ok(defs)
}

/// The payload of a record of the subset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Payload<'a> {
    /// `ExtentHead` (28): the first record of every extent.
    ExtentHead(ExtentHead),
    /// `Noop` (12): `len − 32 − t` zero bytes.
    Noop(u32),
    /// `Commit` (1): [F06]'s payload.
    Commit(Box<Commit<'a>>),
    /// `TreeReg` (23), sub 2: a settle epoch.
    TreeReg(TreeReg),
    /// `FileObs` (18): a row batch of upserts.
    FileObs(RowBatch<'a>),
}

/// One record of the subset, as the scan decodes it and the encoder writes it. The header's `kind`, `flags` and
/// `len` follow from it: `lazy` from the kind's class, `symdefs` from a non-empty block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record<'a> {
    /// The record's lsn.
    pub lsn: u64,
    /// Whether the record ends its group.
    pub group_end: bool,
    /// The `SymDefs` block; empty when the record has none.
    pub symdefs: Vec<SymDef<'a>>,
    /// The kind's payload.
    pub payload: Payload<'a>,
}

impl Record<'_> {
    /// The record kind.
    pub fn kind(&self) -> u8 {
        match self.payload {
            Payload::ExtentHead(_) => kind::EXTENT_HEAD,
            Payload::Noop(_) => kind::NOOP,
            Payload::Commit(_) => kind::COMMIT,
            Payload::TreeReg(_) => kind::TREE_REG,
            Payload::FileObs(_) => kind::FILE_OBS,
        }
    }

    /// The flags: `lazy` by the kind's class ([F05 §6.1], `05-log.md:349-358`), `group_end`, `symdefs`.
    pub fn flags(&self) -> u8 {
        let mut f = 0;
        if lazy_kind(self.kind()) {
            f |= flag::LAZY;
        }
        if self.group_end {
            f |= flag::GROUP_END;
        }
        if !self.symdefs.is_empty() {
            f |= flag::SYMDEFS;
        }
        f
    }

    /// Appends the payload bytes (the `SymDefs` block first).
    pub fn encode_payload(&self, out: &mut Vec<u8>) -> Result<(), String> {
        if !self.symdefs.is_empty() {
            if matches!(self.payload, Payload::Noop(_) | Payload::ExtentHead(_)) {
                return Err("a Noop or an ExtentHead carries no SymDefs block".into());
            }
            encode_symdefs(&self.symdefs, out)?;
        }
        match &self.payload {
            Payload::ExtentHead(h) => h.encode(out),
            Payload::Noop(n) => out.resize(out.len() + *n as usize, 0),
            Payload::Commit(c) => c.encode(out)?,
            Payload::TreeReg(t) => t.encode(out),
            Payload::FileObs(b) => b.encode(out)?,
        }
        Ok(())
    }

    /// The record's length: header, payload and the trailer of a group's last record.
    pub fn encoded_len(&self) -> Result<u64, String> {
        let mut p = Vec::new();
        self.encode_payload(&mut p)?;
        Ok(record_len(p.len(), self.group_end))
    }
}

/// Whether `kind`'s class is `lazy` in the subset ([F05 §7], `05-log.md:373-400`): `Noop`, `FileObs`, `TreeReg`.
pub const fn lazy_kind(k: u8) -> bool {
    matches!(k, kind::NOOP | kind::FILE_OBS | kind::TREE_REG)
}

/// The length of a record with a payload of `payload` bytes.
pub const fn record_len(payload: usize, group_end: bool) -> u64 {
    (REC_HDR_LEN + payload + if group_end { TRAILER_LEN } else { 0 }) as u64
}

/// Appends one record at the end of `out` (whose first byte is lsn `base`), patching its checksum, and, when it ends
/// its group, the trailer over `[group_start, end − 8)` seeded with `seed`. Returns the new chain value when it ends a
/// group.
fn write_record(
    rec: &Record<'_>,
    epoch: u64,
    out: &mut Vec<u8>,
    group_start: usize,
    seed: u64,
) -> Result<Option<u64>, String> {
    let at = out.len();
    out.extend_from_slice(&[0u8; REC_HDR_LEN]);
    rec.encode_payload(out)?;
    let len = out.len() - at + if rec.group_end { TRAILER_LEN } else { 0 };
    let len32 = u32::try_from(len).map_err(|_| format!("a record of {len} bytes"))?;
    let hdr = RecHdr {
        len: len32,
        kind: rec.kind(),
        flags: rec.flags(),
        reserved: 0,
        lsn: rec.lsn,
        epoch,
        xxh3: 0,
    };
    out[at..at + REC_HDR_LEN].copy_from_slice(&hdr.encode());
    if rec.group_end {
        out.extend_from_slice(&[0u8; TRAILER_LEN]);
    }
    let sum = record_checksum(&out[at..], rec.group_end);
    out[at + 24..at + 32].copy_from_slice(&sum.to_le_bytes());
    if rec.group_end {
        let end = out.len();
        let c = chain(&out[group_start..end - TRAILER_LEN], seed);
        out[end - TRAILER_LEN..].copy_from_slice(&c.to_le_bytes());
        return Ok(Some(c));
    }
    Ok(None)
}

/// The bytes of `records`, which must be consecutive from lsn `start` and end a group, with chain value `chain_in` at
/// `start` ([F05 §3.3], [F05 §4.2]). The encoder does not place: pads and extent heads are records of the input. It
/// refuses a record at the wrong lsn, one that crosses its extent, and an unterminated last group. The inverse of the
/// scan's decode: `encode(decode(T)) == T`.
pub fn encode(
    records: &[Record<'_>],
    geo: Geometry,
    start: u64,
    epoch: u64,
    chain_in: u64,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut seed = chain_in;
    let mut group_start = 0;
    for rec in records {
        let lsn = start + out.len() as u64;
        if rec.lsn != lsn {
            return Err(format!(
                "a record at lsn {} where the stream is at {lsn}",
                rec.lsn
            ));
        }
        if let Some(c) = write_record(rec, epoch, &mut out, group_start, seed)? {
            seed = c;
            group_start = out.len();
        }
        let end = start + out.len() as u64;
        if geo.extent(end - 1) != geo.extent(lsn) {
            return Err(format!("the record at lsn {lsn} crosses its extent"));
        }
    }
    if group_start != out.len() {
        return Err("the last record does not end its group".into());
    }
    Ok(out)
}

/// Writes groups at the end of a log stream and places them by [F05 §4.4] G-1 to G-5 (`05-log.md:189-215`): an
/// extent's first group is its extent head, and a group that does not fit the rest of its extent goes to the next one
/// after a `Noop` pad.
#[derive(Clone, Debug)]
pub struct Appender {
    geo: Geometry,
    epoch: u64,
    start: u64,
    chain: u64,
    out: Vec<u8>,
    /// The template of the next extent head: `chain_in` is filled in when it is written; the caller keeps the counters
    /// (fields 6–13, the values of the log before the record) current.
    pub head: ExtentHead,
}

/// The records an [`Appender`] wrote before a group: a pad and an extent head, an extent head, or nothing.
pub type Prelude = Vec<Record<'static>>;

impl Appender {
    /// An appender whose stream begins at `start` with chain value `chain_in`.
    pub fn new(geo: Geometry, epoch: u64, start: u64, chain_in: u64, head: ExtentHead) -> Appender {
        Appender {
            geo,
            epoch,
            start,
            chain: chain_in,
            out: Vec::new(),
            head,
        }
    }

    /// The lsn of the next byte.
    pub fn pos(&self) -> u64 {
        self.start + self.out.len() as u64
    }

    /// The chain value at [`Appender::pos`].
    pub fn chain(&self) -> u64 {
        self.chain
    }

    /// The bytes written so far, the first at the start lsn.
    pub fn bytes(&self) -> &[u8] {
        &self.out
    }

    /// The bytes written so far.
    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }

    /// Where a group of `g` bytes encoded for lsn `at` goes when written now: `(lsn, rotates)`, G-3. The group is
    /// first sized at [`Appender::slot`]'s `at`; if it rotates, the caller re-encodes it for the returned lsn (its
    /// `prev` deltas grow with the lsn, so it still goes there).
    pub fn slot(&self, g: u64) -> Result<(u64, bool), String> {
        if g > self.geo.e() - ROTATION_RESERVE {
            return Err(format!(
                "a group of {g} bytes exceeds E − R = {} (G-2)",
                self.geo.e() - ROTATION_RESERVE
            ));
        }
        let mut p = self.pos();
        if self.geo.offset(p) == 0 {
            p += EXTENT_HEAD_GROUP_LEN;
        }
        let r = self.geo.extent_end(p) - p;
        if g == r || g + MIN_GROUP_LEN <= r {
            Ok((p, false))
        } else {
            Ok((self.geo.extent_end(p) + EXTENT_HEAD_GROUP_LEN, true))
        }
    }

    /// Where the next group starts if it fits the current extent: the current position, or past the extent head
    /// that an extent's first byte needs.
    pub fn next_lsn(&self) -> u64 {
        let p = self.pos();
        if self.geo.offset(p) == 0 {
            p + EXTENT_HEAD_GROUP_LEN
        } else {
            p
        }
    }

    /// Builds a group with `build(lsn)` for the lsn G-3 gives it and writes it: the group is built for the next lsn,
    /// sized, and rebuilt for the lsn after a rotation when it does not fit. `build` returns the group's records with
    /// their lsns (see [`sequence`]). Returns the prelude and the group.
    pub fn place<'a>(
        &mut self,
        mut build: impl FnMut(u64) -> Result<Vec<Record<'a>>, String>,
    ) -> Result<(Prelude, Vec<Record<'a>>), String> {
        let mut lsn = self.next_lsn();
        for _ in 0..3 {
            let group = build(lsn)?;
            let mut g = 0;
            for r in &group {
                g += r.encoded_len()?;
            }
            let (to, _) = self.slot(g)?;
            if to == lsn {
                let prelude = self.append(&group)?;
                return Ok((prelude, group));
            }
            lsn = to;
        }
        Err("a group's placement did not settle".into())
    }

    /// Writes the extent head whose extent begins at the current position.
    fn write_head(&mut self, prelude: &mut Prelude) -> Result<(), String> {
        let rec = Record {
            lsn: self.pos(),
            group_end: true,
            symdefs: Vec::new(),
            payload: Payload::ExtentHead(ExtentHead {
                chain_in: self.chain,
                ..self.head
            }),
        };
        let gs = self.out.len();
        if let Some(c) = write_record(&rec, self.epoch, &mut self.out, gs, self.chain)? {
            self.chain = c;
        }
        prelude.push(rec);
        Ok(())
    }

    /// Writes `group` (records with their lsns, the last ending the group), first writing the pad and extent heads G-3
    /// asks for. The group's first lsn must be the one [`Appender::slot`] gave for its length. Returns the prelude.
    pub fn append(&mut self, group: &[Record<'_>]) -> Result<Prelude, String> {
        let mut prelude = Prelude::new();
        let Some(first) = group.first() else {
            return Err("an empty group".into());
        };
        if !group.last().is_some_and(|r| r.group_end)
            || group[..group.len() - 1].iter().any(|r| r.group_end)
        {
            return Err("only a group's last record carries group_end".into());
        }
        if self.geo.offset(self.pos()) == 0 {
            self.write_head(&mut prelude)?;
        }
        if first.lsn != self.pos() {
            let r = self.geo.extent_end(self.pos()) - self.pos();
            let pad = Record {
                lsn: self.pos(),
                group_end: true,
                symdefs: Vec::new(),
                payload: Payload::Noop((r - MIN_GROUP_LEN) as u32),
            };
            let gs = self.out.len();
            if let Some(c) = write_record(&pad, self.epoch, &mut self.out, gs, self.chain)? {
                self.chain = c;
            }
            prelude.push(pad);
            self.write_head(&mut prelude)?;
        }
        if first.lsn != self.pos() {
            return Err(format!(
                "a group at lsn {} where G-3 places it at {}",
                first.lsn,
                self.pos()
            ));
        }
        let gs = self.out.len();
        let seed = self.chain;
        for rec in group {
            if rec.lsn != self.pos() {
                return Err(format!(
                    "a record at lsn {} where the group is at {}",
                    rec.lsn,
                    self.pos()
                ));
            }
            if let Some(c) = write_record(rec, self.epoch, &mut self.out, gs, seed)? {
                self.chain = c;
            }
        }
        let end = self.pos();
        if self.geo.extent(end - 1) != self.geo.extent(first.lsn) {
            return Err(format!(
                "the group at lsn {} crosses its extent (G-1)",
                first.lsn
            ));
        }
        let left = self.geo.extent_end(end - 1) - end;
        if left > 0 && left < MIN_GROUP_LEN {
            return Err(format!(
                "the group at lsn {} leaves {left} bytes in its extent (G-4)",
                first.lsn
            ));
        }
        Ok(prelude)
    }
}

/// The one-record group of a record of `kind` with `payload`, framed at `lsn` ([F05 §3.1]–§3.4, §4.2,
/// `05-log.md:116-176`): its `RecHdr` with `group_end` (and `lazy` for a lazy kind of the subset), its checksum, and the
/// trailer, XXH3-64 over the group's bytes before it seeded with `seed`, the chain value at `lsn`. The record need not be
/// of the encoder's subset: a `Checkpoint` (kind 9, [`super::ckpt`]) is framed here, and the scan refuses it. The
/// caller places the group (G-1 to G-5).
pub fn raw_group(
    kind: u8,
    payload: &[u8],
    lsn: u64,
    epoch: u64,
    seed: u64,
) -> Result<Vec<u8>, String> {
    let len = record_len(payload.len(), true) as usize;
    let len32 = u32::try_from(len).map_err(|_| format!("a record of {len} bytes"))?;
    let hdr = RecHdr {
        len: len32,
        kind,
        flags: flag::GROUP_END | if lazy_kind(kind) { flag::LAZY } else { 0 },
        reserved: 0,
        lsn,
        epoch,
        xxh3: 0,
    };
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&hdr.encode());
    out.extend_from_slice(payload);
    out.extend_from_slice(&[0u8; TRAILER_LEN]);
    let sum = record_checksum(&out, true);
    out[24..32].copy_from_slice(&sum.to_le_bytes());
    let c = chain(&out[..len - TRAILER_LEN], seed);
    out[len - TRAILER_LEN..].copy_from_slice(&c.to_le_bytes());
    Ok(out)
}

/// Gives `records` consecutive lsns from `lsn`.
pub fn sequence(lsn: u64, records: &mut [Record<'_>]) -> Result<(), String> {
    let mut at = lsn;
    for r in records.iter_mut() {
        r.lsn = at;
        at += r.encoded_len()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tail::testdata::{self, CHAIN_IN, E, START};
    use crate::tail::{Geometry, kind};

    /// `RecHdr` ([F05 §3.1], `05-log.md:118-129`): 32 bytes, `len` at 0, `kind` at 4, `flags` at 5, `_reserved` at 6,
    /// `lsn` at 8, `epoch` at 16, `xxh3_64` at 24, all little-endian.
    #[test]
    fn rechdr_is_32_bytes_as_f05_lays_it_out() {
        let h = RecHdr {
            len: 0x0403_0201,
            kind: 0x05,
            flags: 0x06,
            reserved: 0x0807,
            lsn: 0x100F_0E0D_0C0B_0A09,
            epoch: 0x1817_1615_1413_1211,
            xxh3: 0x201F_1E1D_1C1B_1A19,
        };
        let b = h.encode();
        assert_eq!(b.len(), REC_HDR_LEN);
        assert_eq!(REC_HDR_LEN, 32);
        let expected: Vec<u8> = (1..=32).collect();
        assert_eq!(
            b.to_vec(),
            expected,
            "every field at its offset, little-endian"
        );
        assert_eq!(RecHdr::decode(&b), h);
        assert!(h.group_end(), "flags 0x06 set bit 1");
        assert!(
            !RecHdr {
                flags: flag::LAZY | flag::SYMDEFS,
                ..h
            }
            .group_end()
        );
        // Flag bits 0, 1, 2 ([F05 §3.2], `05-log.md:131-139`).
        assert_eq!(
            (flag::LAZY, flag::GROUP_END, flag::SYMDEFS, flag::RESERVED),
            (1, 2, 4, 0xF8)
        );
    }

    /// `n(L)` and `o(L)` ([F05 §2.3], `05-log.md:60-61`): extent n holds `[(n − 1)·E, n·E)`; lsn 0 is `log.1`'s first
    /// byte. E is a power of two in [2^16, 2^30] (`05-log.md:47-50`).
    #[test]
    fn lsn_maps_to_extent_and_offset() {
        let g = Geometry::new(1 << 16).unwrap();
        assert_eq!((g.extent(0), g.offset(0)), (1, 0));
        assert_eq!((g.extent(65_535), g.offset(65_535)), (1, 65_535));
        assert_eq!((g.extent(65_536), g.offset(65_536)), (2, 0));
        assert_eq!((g.extent(3 * 65_536 + 7), g.offset(3 * 65_536 + 7)), (4, 7));
        assert_eq!(g.extent_start(3), 2 * 65_536);
        assert_eq!(g.extent_end(65_536 + 9), 2 * 65_536);
        let p = Geometry::new(64 << 20).unwrap();
        assert_eq!((p.extent(5 << 26), p.offset((5 << 26) + 3)), (6, 3));
        assert!(Geometry::new(1 << 15).is_none() && Geometry::new(3 << 16).is_none());
        assert!(Geometry::new(1 << 31).is_none() && Geometry::new(1 << 30).is_some());
        // `K_RT` is kinds 18–26 (`05-log.md:390-398`).
        assert!(crate::tail::in_k_rt(kind::FILE_OBS) && crate::tail::in_k_rt(kind::TREE_REG));
        assert!(!crate::tail::in_k_rt(kind::COMMIT) && !crate::tail::in_k_rt(kind::NOOP));
    }

    /// One `Noop` record of `n` payload bytes at lsn 0, epoch 9.
    fn noop(n: u32, group_end: bool) -> Vec<u8> {
        let r = Record {
            lsn: 0,
            group_end,
            symdefs: Vec::new(),
            payload: Payload::Noop(n),
        };
        let mut out = Vec::new();
        write_record(&r, 9, &mut out, 0, 0).unwrap();
        out
    }

    /// The checksum ([F05 §3.4], `05-log.md:155-157`) covers `[0, 24)` and the payload: a payload or header byte moves
    /// it; the trailer and the checksum field do not.
    #[test]
    fn record_checksum_covers_header_and_payload_only() {
        let rec = noop(16, true);
        assert_eq!(rec.len(), 32 + 16 + 8);
        let sum = u64::from_le_bytes(rec[24..32].try_into().unwrap());
        assert_eq!(record_checksum(&rec, true), sum);
        let mut expect = Vec::new();
        expect.extend_from_slice(&rec[..24]);
        expect.extend_from_slice(&rec[32..48]);
        assert_eq!(
            sum,
            xxh3_64(&expect),
            "XXH3-64, seed 0, over [0, 24) ++ [32, len − 8)"
        );
        let mut t = rec.clone();
        t[50] ^= 1;
        assert_eq!(record_checksum(&t, true), sum, "the trailer is outside");
        let mut c = rec.clone();
        c[25] ^= 1;
        assert_eq!(
            record_checksum(&c, true),
            sum,
            "the checksum field is outside"
        );
        let mut p = rec.clone();
        p[40] ^= 1;
        assert_ne!(record_checksum(&p, true), sum, "a payload byte moves it");
        let mut h = rec.clone();
        h[9] ^= 1;
        assert_ne!(record_checksum(&h, true), sum, "a header byte moves it");
        // Without group_end the payload runs to len.
        let open = noop(16, false);
        assert_eq!(open.len(), 48);
        assert_eq!(
            record_checksum(&open, false),
            u64::from_le_bytes(open[24..32].try_into().unwrap())
        );
    }

    /// The chain ([F05 §4.2]–§4.3, `05-log.md:169-185`): the epoch seed is XXH3-64 of the epoch's 8 bytes; a trailer is
    /// XXH3-64 over the group's bytes before it, seeded with the chain value at its boundary; the next group's seed is
    /// that trailer.
    #[test]
    fn chain_seeds_at_the_boundary_and_links_groups() {
        assert_eq!(
            epoch_seed(0x0102_0304_0506_0708),
            xxh3_64(&[8, 7, 6, 5, 4, 3, 2, 1])
        );
        let x = testdata::expect();
        let hb = testdata::hand_built();
        let b = &hb.bytes;
        // The first group is the extent head at START: 138 bytes seeded with CHAIN_IN.
        let t1 = u64::from_le_bytes(b[130..138].try_into().unwrap());
        assert_eq!(t1, chain(&b[..130], CHAIN_IN));
        assert_eq!(
            t1,
            xxhash_rust::xxh3::xxh3_64_with_seed(&b[..130], CHAIN_IN)
        );
        // The second group (the first commit) is seeded with the first group's trailer.
        let len2 = u32::from_le_bytes(b[138..142].try_into().unwrap()) as usize;
        let t2 = u64::from_le_bytes(b[138 + len2 - 8..138 + len2].try_into().unwrap());
        assert_eq!(t2, chain(&b[138..138 + len2 - 8], t1));
        assert_ne!(
            t2,
            chain(&b[138..138 + len2 - 8], 0),
            "a group validates only behind its predecessor"
        );
        // The whole tail re-encodes from the decoded records with the same seed, and not with another.
        let recs = crate::tail::scan::decode(b, START, &x, CHAIN_IN).unwrap();
        assert_eq!(encode(&recs, x.geo, START, x.epoch, CHAIN_IN).unwrap(), *b);
        assert_ne!(
            encode(&recs, x.geo, START, x.epoch, CHAIN_IN ^ 1).unwrap(),
            *b
        );
    }

    /// H = 32 + 98 + 8 = 138 ([F05 §9.28], `05-log.md:1113-1114`) and R = H + 40 = 178 (`:191`).
    #[test]
    fn extent_head_group_is_138_bytes() {
        let x = testdata::expect();
        let h = testdata::head(&x);
        let mut p = Vec::new();
        h.encode(&mut p);
        assert_eq!(p.len(), EXTENT_HEAD_PAYLOAD_LEN);
        assert_eq!(ExtentHead::decode(&p), Ok(h));
        let rec = Record {
            lsn: START,
            group_end: true,
            symdefs: Vec::new(),
            payload: Payload::ExtentHead(h),
        };
        assert_eq!(rec.encoded_len(), Ok(EXTENT_HEAD_GROUP_LEN));
        assert_eq!(EXTENT_HEAD_GROUP_LEN, 138);
        assert_eq!(ROTATION_RESERVE, 178);
        // Payload offsets: init at 16 (its log_extent_bytes first), project_oid_algo at 48, hflags at 49, hlc_commit
        // at 90.
        assert_eq!(&p[16..24], &E.to_le_bytes());
        assert_eq!(&p[48..50], &[1, 0]);
        assert_eq!(&p[90..98], &(1u64 << 40).to_le_bytes());
        let mut bad = p.clone();
        bad[49] = 4;
        assert!(
            ExtentHead::decode(&bad)
                .unwrap_err()
                .what
                .contains("hflags")
        );
        // An extent head never carries a SymDefs block.
        let with_defs = Record {
            symdefs: testdata::symdefs(),
            ..rec
        };
        assert!(with_defs.encoded_len().is_err());
    }

    /// [`raw_group`] frames one record as the encoder does ([F05 §3.1]–§3.4, §4.2): a `Noop` group of 16 zero bytes at
    /// [`START`] after the chain value [`CHAIN_IN`] is the encoder's group byte for byte, `lazy` and `group_end` set; a
    /// durable kind (a `Checkpoint`, 9) gets `group_end` alone; another seed gives another trailer.
    #[test]
    fn raw_group_equals_the_encoders_group() {
        let x = testdata::expect();
        let rec = Record {
            lsn: START,
            group_end: true,
            symdefs: Vec::new(),
            payload: Payload::Noop(16),
        };
        let want = encode(&[rec], x.geo, START, x.epoch, CHAIN_IN).unwrap();
        let got = raw_group(kind::NOOP, &[0; 16], START, x.epoch, CHAIN_IN).unwrap();
        assert_eq!(got, want, "the encoder's Noop group");
        assert_eq!(got.len(), 32 + 16 + 8);
        assert_eq!(got[5], flag::LAZY | flag::GROUP_END);
        let other = raw_group(kind::NOOP, &[0; 16], START, x.epoch, CHAIN_IN ^ 1).unwrap();
        assert_ne!(
            other[56 - 8..],
            got[56 - 8..],
            "the trailer depends on the seed"
        );
        let ck = raw_group(9, &[1, 2, 3], START, x.epoch, CHAIN_IN).unwrap();
        let h = RecHdr::decode(ck[..32].try_into().unwrap());
        assert_eq!(
            (h.len, h.kind, h.flags, h.lsn, h.epoch),
            (43, 9, flag::GROUP_END, START, x.epoch)
        );
        assert_eq!(h.xxh3, record_checksum(&ck, true));
        assert_eq!(&ck[35..], &chain(&ck[..35], CHAIN_IN).to_le_bytes());
    }

    /// G-3 (`05-log.md:196-210`): a group that does not fit the rest of its extent goes to the next one after exactly
    /// one `Noop` pad of length r and the next extent's head; every group leaves 0 or ≥ 40 bytes (G-4).
    #[test]
    fn rotation_pads_with_one_noop_group() {
        let x = testdata::expect();
        let mut app = Appender::new(x.geo, x.epoch, START, CHAIN_IN, testdata::head(&x));
        let msg = "p".repeat(20_000);
        let mut placed = Vec::new();
        for seq in 1..=4u64 {
            let (pre, g) = app
                .place(|lsn| {
                    let c = testdata::commit(seq, 7, Vec::new(), &msg);
                    Ok(testdata::commit_group(lsn, c, Vec::new()))
                })
                .unwrap();
            placed.push((pre, g[0].lsn));
        }
        // The first group is preceded by extent 2's head; the next two fit; the fourth rotates.
        assert_eq!(placed[0].0.len(), 1);
        assert_eq!(placed[0].1, START + EXTENT_HEAD_GROUP_LEN);
        assert!(placed[1].0.is_empty() && placed[2].0.is_empty());
        let (pre, lsn) = &placed[3];
        assert_eq!(pre.len(), 2, "one pad, one extent head");
        let Payload::Noop(zeros) = pre[0].payload else {
            panic!("a pad")
        };
        let r = 2 * E - pre[0].lsn;
        assert_eq!(u64::from(zeros) + MIN_GROUP_LEN, r, "the pad's len is r");
        assert!(pre[0].group_end && r >= MIN_GROUP_LEN);
        assert!(matches!(pre[1].payload, Payload::ExtentHead(h) if h.chain_in != 0));
        assert_eq!(pre[1].lsn, 2 * E);
        assert_eq!(*lsn, 2 * E + EXTENT_HEAD_GROUP_LEN);
        // The rotated stream scans, and its pad and head decode as written.
        let recs = crate::tail::scan::decode(app.bytes(), START, &x, CHAIN_IN).unwrap();
        assert!(recs.contains(&pre[0]) && recs.contains(&pre[1]));
        // A group longer than E − R is refused (G-2).
        assert!(
            app.slot(E - ROTATION_RESERVE + 1)
                .unwrap_err()
                .contains("G-2")
        );
        assert!(app.slot(E - ROTATION_RESERVE).is_ok());
    }
}
