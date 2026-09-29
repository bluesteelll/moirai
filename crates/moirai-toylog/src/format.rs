//! The log's bytes as the toy writes them: the product's 32-byte `RecHdr` ([F05 §3]), groups with their chain trailer
//! ([F05 §4]), record validity ([F05 §5.2]), the extent-head record ([F05 §9.28]) and the toy's payloads.
//!
//! The header, the record checksum, the group framing, the chain, the placement constants and the extent head are the
//! product's format exactly (PLAN §6.2 R10 as amended by WP-80a: "the toy log writes the product's `RecHdr`, chained
//! groups"). The payloads of the other kinds are the toy's own compact encodings of the fields the protocol reads, under
//! the kind numbers of [F05 §7]: the commit body is [F06]'s and the rows are [F11]'s, which the toy does not need to
//! exercise [F16].

use moirai_vfs::Stamp;

use crate::bugs::{Bug, Bugs};
use crate::codec::{Reader, Short, Writer, hash64_parts, hash64_seeded};

/// The length of `RecHdr` ([F05 §3.1]).
pub const RECHDR: usize = 32;
/// The chain trailer's length ([F05 §4.2]).
pub const TRAILER: usize = 8;
/// The smallest group: one record with an empty payload and the trailer ([F05 §4.4]).
pub const MIN_GROUP: u64 = 40;
/// The length of the extent-head group, H ([F05 §4.4], §9.28).
pub const HEAD_GROUP: u64 = 138;
/// The rotation reserve R = H + 40 ([F05 §4.4]).
pub const ROTATION_RESERVE: u64 = HEAD_GROUP + MIN_GROUP;
/// The extent head's payload length ([F05 §9.28]).
pub const EXTENT_HEAD_PAYLOAD: usize = 98;

/// `RecHdr.flags` bit 0: the record's class is `lazy` ([F05 §3.2]).
pub const F_LAZY: u8 = 1 << 0;
/// `RecHdr.flags` bit 1: the record ends its group with the chain trailer.
pub const F_GROUP_END: u8 = 1 << 1;
/// `RecHdr.flags` bit 2: the payload begins with a symbol-definition block ([F05 §8.1]).
pub const F_SYMDEFS: u8 = 1 << 2;

/// Record kinds ([F05 §7]); the toy writes the ones named here.
pub mod kind {
    /// `Commit`, durable.
    pub const COMMIT: u8 = 1;
    /// `RefUpdate`, durable.
    pub const REF_UPDATE: u8 = 2;
    /// `Lease`, durable.
    pub const LEASE: u8 = 4;
    /// `Marker`, durable.
    pub const MARKER: u8 = 5;
    /// `Idem`, durable.
    pub const IDEM: u8 = 6;
    /// `Pin`, durable.
    pub const PIN: u8 = 8;
    /// `Checkpoint`, durable.
    pub const CHECKPOINT: u8 = 9;
    /// `RefTable`, durable.
    pub const REF_TABLE: u8 = 10;
    /// `Noop`, lazy: the rotation pad.
    pub const NOOP: u8 = 12;
    /// `FsIntent`, durable.
    pub const FS_INTENT: u8 = 15;
    /// `FsIntentDone`, durable.
    pub const FS_INTENT_DONE: u8 = 16;
    /// `FsIntentAborted`, durable.
    pub const FS_INTENT_ABORTED: u8 = 17;
    /// `FileObs`, lazy, `K_RT`: the toy's runtime-row batches.
    pub const FILE_OBS: u8 = 18;
    /// `ExtentHead`, durable.
    pub const EXTENT_HEAD: u8 = 28;
}

/// The durability class of a kind in the registry of [F05 §7] and §6.1.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Class {
    /// `durable`: bit 0 must be 0.
    Durable,
    /// `lazy`: bit 0 must be 1.
    Lazy,
    /// `configurable`: bit 0 as written decides.
    Configurable,
}

/// The registry class of `kind`, or `None` for a value outside the registry (0, 29–255). The toy's kind registry is the
/// one place P-5's class tag comes from: its seeded bug gives `Lease` the class `lazy`.
pub fn class_of(k: u8, bugs: Bugs) -> Option<Class> {
    Some(match k {
        1..=10 | 13 | 15..=17 | 27 | 28 => {
            if k == kind::LEASE && bugs.on(Bug::P05LeaseTaggedLazy) {
                Class::Lazy
            } else {
                Class::Durable
            }
        }
        11 | 14 => Class::Configurable,
        12 | 18..=26 => Class::Lazy,
        _ => return None,
    })
}

/// One record before it is framed: its kind, its class bit, and its payload (after the `SymDefs` block, if any).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rec {
    /// The kind.
    pub kind: u8,
    /// `lazy` (bit 0).
    pub lazy: bool,
    /// The symbol definitions of the payload's block ([F05 §8.1]): (id, text), class `text` (11).
    pub symdefs: Vec<(u32, String)>,
    /// The kind's payload.
    pub payload: Vec<u8>,
}

impl Rec {
    /// A record of `kind` tagged by its registry class (a configurable kind as `lazy`).
    pub fn new(kind: u8, payload: Vec<u8>, bugs: Bugs) -> Rec {
        let lazy = !matches!(class_of(kind, bugs), Some(Class::Durable));
        Rec {
            kind,
            lazy,
            symdefs: Vec::new(),
            payload,
        }
    }

    /// The record's encoded length when it is (`last`) or is not the last of its group.
    pub fn len(&self, last: bool) -> u64 {
        (RECHDR + self.symdefs_len() + self.payload.len() + if last { TRAILER } else { 0 }) as u64
    }

    fn symdefs_len(&self) -> usize {
        if self.symdefs.is_empty() {
            0
        } else {
            let mut w = Writer::default();
            encode_symdefs(&self.symdefs, &mut w);
            w.buf.len()
        }
    }
}

fn encode_symdefs(defs: &[(u32, String)], w: &mut Writer) {
    w.uvar(defs.len() as u64);
    for (id, text) in defs {
        w.u8(11).uvar(u64::from(*id)).vbytes(text.as_bytes());
    }
}

/// The length of a group of `recs`.
pub fn group_len(recs: &[Rec]) -> u64 {
    recs.iter()
        .enumerate()
        .map(|(i, r)| r.len(i + 1 == recs.len()))
        .sum()
}

/// Whether the group of `recs` is durable ([F05 §4.7]: some record has `lazy` = 0).
pub fn group_durable(recs: &[Rec]) -> bool {
    recs.iter().any(|r| !r.lazy)
}

/// Frames `recs` as one group at lsn `p` of `epoch`, seeded with the chain value `seed` at `p` ([F05 §4.2]), appending
/// the bytes to `out`; returns the chain value at the group's end (its trailer).
pub fn encode_group(recs: &[Rec], p: u64, epoch: u64, seed: u64, out: &mut Vec<u8>) -> u64 {
    let start = out.len();
    let mut lsn = p;
    for (i, r) in recs.iter().enumerate() {
        let last = i + 1 == recs.len();
        let len = r.len(last);
        let mut flags = 0u8;
        if r.lazy {
            flags |= F_LAZY;
        }
        if last {
            flags |= F_GROUP_END;
        }
        let mut body = Writer::with_capacity(len as usize);
        if !r.symdefs.is_empty() {
            flags |= F_SYMDEFS;
            encode_symdefs(&r.symdefs, &mut body);
        }
        body.bytes(&r.payload);
        let mut hdr = [0u8; RECHDR];
        hdr[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        hdr[4] = r.kind;
        hdr[5] = flags;
        hdr[8..16].copy_from_slice(&lsn.to_le_bytes());
        hdr[16..24].copy_from_slice(&epoch.to_le_bytes());
        let sum = hash64_parts(&[&hdr[..24], &body.buf]);
        hdr[24..32].copy_from_slice(&sum.to_le_bytes());
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&body.buf);
        if last {
            let chain = hash64_seeded(&out[start..], seed);
            out.extend_from_slice(&chain.to_le_bytes());
        }
        lsn += len;
    }
    let end = out.len();
    let mut t = [0u8; 8];
    t.copy_from_slice(&out[end - TRAILER..end]);
    u64::from_le_bytes(t)
}

/// The symbol definitions of a `SymDefs` block ([F05 §8.1]): (id, text) of class `text`.
pub type SymDefs = Vec<(u32, String)>;

/// A record as the scan read it: its header fields and its body (the bytes between the header and the trailer).
///
/// The body is kept as the log holds it. A `SymDefs` block ([F05 §8.1]) is part of the payload's bytes, not of the
/// record's framing: a malformed block in a record whose checksum holds is a malformed payload, corrupt wherever it lies
/// ([F05 §5.4]), never an invalid record that ends the valid log. It is therefore decoded only when the record is
/// applied or folded ([`RecView::split`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecView {
    /// The kind.
    pub kind: u8,
    /// `lazy`.
    pub lazy: bool,
    /// Its lsn (its position).
    pub lsn: u64,
    /// `flags` bit 2: the body begins with a `SymDefs` block.
    pub has_symdefs: bool,
    /// The body: the `SymDefs` block when `has_symdefs`, then the kind's payload.
    pub body: Vec<u8>,
}

impl RecView {
    /// The symbol definitions and the kind's payload ([F05 §8.1]); `Short` for a malformed `SymDefs` block (a malformed
    /// payload, [F05 §5.4]).
    pub fn split(&self) -> Result<(SymDefs, &[u8]), Short> {
        if !self.has_symdefs {
            return Ok((Vec::new(), &self.body));
        }
        let mut r = Reader::new(&self.body);
        let defs = decode_symdefs(&mut r)?;
        let used = self.body.len() - r.rest();
        Ok((defs, &self.body[used..]))
    }

    /// The kind's payload, after the `SymDefs` block if any.
    pub fn payload(&self) -> Result<&[u8], Short> {
        Ok(self.split()?.1)
    }
}

/// Why a record is invalid ([F05 §5.2]); every reason ends the valid log at the group that holds it.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Invalid {
    /// Check 2: a length out of range, or the record does not lie inside its extent.
    Length,
    /// Check 3: a kind outside the registry.
    Kind,
    /// Check 4: reserved bits, a class tag that disagrees with the kind, a `SymDefs` flag on a `Noop` or head.
    Bits,
    /// Check 5: `lsn` differs from the position.
    Position,
    /// Check 6: another epoch.
    Epoch,
    /// Check 7: the checksum.
    Checksum,
    /// The trailer does not match the chain ([F05 §4.6]).
    Chain,
    /// The extent ends before a record carrying `group_end`.
    Unterminated,
}

/// The header fields of the record at the start of `b`, if 32 bytes are there: (`len`, `kind`, `flags`).
pub fn peek_header(b: &[u8]) -> Option<(u32, u8, u8)> {
    if b.len() < RECHDR {
        return None;
    }
    let len = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    Some((len, b[4], b[5]))
}

/// Validates the record at offset `off` of the extent buffer `ext` (the whole extent's bytes as read, `e` = its length
/// E), at lsn `p` of `epoch` ([F05 §5.2] checks 2–7); returns the record and its length.
pub fn validate_record(
    ext: &[u8],
    off: usize,
    e: u64,
    p: u64,
    epoch: u64,
    bugs: Bugs,
) -> Result<(RecView, bool, u64), Invalid> {
    let hdr = ext.get(off..off + RECHDR).ok_or(Invalid::Length)?;
    let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
    let k = hdr[4];
    let flags = hdr[5];
    let group_end = flags & F_GROUP_END != 0;
    // Check 2: 32 ≤ len, the record lies inside its extent, len ≥ 40 with group_end.
    if len < RECHDR as u64 || off as u64 + len > e || (group_end && len < MIN_GROUP) {
        return Err(Invalid::Length);
    }
    if off as u64 + len > ext.len() as u64 {
        return Err(Invalid::Length);
    }
    // Check 3: the registry.
    let class = class_of(k, bugs).ok_or(Invalid::Kind)?;
    // Check 4: reserved bits, the class tag, no SymDefs on a Noop or an extent head.
    let lazy = flags & F_LAZY != 0;
    let symdefs = flags & F_SYMDEFS != 0;
    if flags & 0xF8 != 0 || hdr[6] != 0 || hdr[7] != 0 {
        return Err(Invalid::Bits);
    }
    match class {
        Class::Durable if lazy => return Err(Invalid::Bits),
        Class::Lazy if !lazy => return Err(Invalid::Bits),
        _ => {}
    }
    if symdefs && (k == kind::NOOP || k == kind::EXTENT_HEAD) {
        return Err(Invalid::Bits);
    }
    let lsn = u64::from_le_bytes(hdr[8..16].try_into().map_err(|_| Invalid::Length)?);
    let rec_epoch = u64::from_le_bytes(hdr[16..24].try_into().map_err(|_| Invalid::Length)?);
    let sum = u64::from_le_bytes(hdr[24..32].try_into().map_err(|_| Invalid::Length)?);
    // Check 5: the position (P-54).
    if lsn != p && !bugs.on(Bug::P54T13NoPositionCheck) {
        return Err(Invalid::Position);
    }
    // Check 6: the epoch (P-55).
    if rec_epoch != epoch && !bugs.on(Bug::P55NoEpochCheck) {
        return Err(Invalid::Epoch);
    }
    let t = if group_end { TRAILER } else { 0 };
    let body = &ext[off + RECHDR..off + len as usize - t];
    // Check 7: the checksum over the header without its checksum field, then the body.
    if hash64_parts(&[&hdr[..24], body]) != sum {
        return Err(Invalid::Checksum);
    }
    // The SymDefs block is part of the payload: decoded when the record is applied (RecView::split, [F05 §5.4]).
    Ok((
        RecView {
            kind: k,
            lazy,
            lsn: p,
            has_symdefs: symdefs,
            body: body.to_vec(),
        },
        group_end,
        len,
    ))
}

/// The smallest encoding of one `SymDefs` entry: the class byte, a one-byte id, a one-byte length and one byte of text.
const MIN_SYMDEF: usize = 4;

/// A capacity for `n` entries of at least `min` bytes each in the rest of `r`: a count taken from a payload never
/// allocates more than the payload can hold, so every decoder stays total on a corrupt payload (module [`crate::codec`]).
fn capacity(n: u64, r: &Reader<'_>, min: usize) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX).min(r.rest() / min)
}

fn decode_symdefs(r: &mut Reader<'_>) -> Result<SymDefs, Short> {
    let n = r.uvar(16)?;
    if n == 0 {
        return Err(Short);
    }
    let mut out = Vec::with_capacity(capacity(n, r, MIN_SYMDEF));
    for _ in 0..n {
        if r.u8()? != 11 {
            return Err(Short);
        }
        let id = r.uvar32()?;
        let text = r.vbytes()?;
        let text = core::str::from_utf8(text).map_err(|_| Short)?;
        if text.is_empty() {
            return Err(Short);
        }
        out.push((id, text.to_owned()));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------------
// Payloads

/// The toy's `Commit` payload (kind 1): the fields the protocol reads ([F05 §9.1]: `seq`, `ref_id`, `append_hlc`, the
/// `#N`s its creates allocate, `ref_old`) and a filler that gives the record its size.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitRec {
    /// The operation this commit carries out (the toy's commit identity; a retry carries the same value).
    pub op: u64,
    /// The digest of the commit's content.
    pub digest: u64,
    /// The commit's `seq`.
    pub seq: u64,
    /// The ref it lands on.
    pub ref_id: u32,
    /// The ref's tip before the commit (the `op` of that commit), 0 for an empty ref: the CAS of its implied ref move
    /// ([F16] P-69).
    pub ref_old: u64,
    /// The commit's `hlc` ([F16] P-36).
    pub hlc: u64,
    /// Its `append_hlc`.
    pub append_hlc: u64,
    /// The wall-clock reading the HLC rule took at append (toy field: the HLC check recomputes `hlc` from it).
    pub wall_ms: u64,
    /// Bit 0: imported (it keeps its own `hlc` and its `ref_old`).
    pub flags: u8,
    /// The nodes its creates allocate: (`#N`, uid).
    pub creates: Vec<(u32, u64)>,
    /// Filler bytes (the commit-size distribution).
    pub filler: u32,
}

/// `CommitRec::flags` bit 0: an imported commit.
pub const COMMIT_IMPORTED: u8 = 1;
/// `CommitRec::flags` bit 1 (toy): the commit implies no ref move; a separate `RefUpdate` moves its ref (written only by
/// [F16] P-69's seeded bug).
pub const COMMIT_DETACHED: u8 = 2;

impl CommitRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(64 + self.creates.len() * 12 + self.filler as usize);
        w.u64(self.op)
            .u64(self.digest)
            .uvar(self.seq)
            .uvar(u64::from(self.ref_id))
            .u64(self.ref_old)
            .u64(self.hlc)
            .u64(self.append_hlc)
            .u64(self.wall_ms)
            .u8(self.flags)
            .uvar(self.creates.len() as u64);
        for &(n, uid) in &self.creates {
            w.uvar(u64::from(n)).u64(uid);
        }
        let fill = vec![(self.op as u8) | 1; self.filler as usize];
        w.vbytes(&fill);
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<CommitRec, Short> {
        let mut r = Reader::new(b);
        let mut c = CommitRec {
            op: r.u64()?,
            digest: r.u64()?,
            seq: r.uvar(64)?,
            ref_id: r.uvar32()?,
            ref_old: r.u64()?,
            hlc: r.u64()?,
            append_hlc: r.u64()?,
            wall_ms: r.u64()?,
            flags: r.u8()?,
            ..CommitRec::default()
        };
        let n = r.uvar32()?;
        for _ in 0..n {
            let node = r.uvar32()?;
            let uid = r.u64()?;
            c.creates.push((node, uid));
        }
        c.filler = r.vbytes()?.len() as u32;
        if !r.done() || c.flags & !(COMMIT_IMPORTED | COMMIT_DETACHED) != 0 {
            return Err(Short);
        }
        Ok(c)
    }
}

/// `RefUpdate` (kind 2, [F05 §9.2]): reason 1 create, 2 delete, 5 park.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefUpdateRec {
    /// The reason.
    pub reason: u8,
    /// The ref that moves.
    pub ref_id: u32,
    /// The operation that moves it (for a park, the parked commit's op).
    pub op: u64,
    /// HLC at append.
    pub hlc: u64,
    /// The tip before (an op, 0 = none).
    pub old: u64,
    /// The tip after.
    pub new: u64,
}

/// `RefUpdate.reason` 1: create (a branch, a fork).
pub const REASON_CREATE: u8 = 1;
/// `RefUpdate.reason` 2: delete.
pub const REASON_DELETE: u8 = 2;
/// `RefUpdate.reason` 5: park ([F16] P-70).
pub const REASON_PARK: u8 = 5;
/// `RefUpdate.reason` 6 (toy): a commit's ref move written apart from it (only by P-69's seeded bug).
pub const REASON_MOVE: u8 = 6;

impl RefUpdateRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(40);
        w.u8(self.reason)
            .uvar(u64::from(self.ref_id))
            .u64(self.op)
            .u64(self.hlc)
            .u64(self.old)
            .u64(self.new);
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<RefUpdateRec, Short> {
        let mut r = Reader::new(b);
        let v = RefUpdateRec {
            reason: r.u8()?,
            ref_id: r.uvar32()?,
            op: r.u64()?,
            hlc: r.u64()?,
            old: r.u64()?,
            new: r.u64()?,
        };
        if !r.done() || !matches!(v.reason, 1 | 2 | 5 | 6) {
            return Err(Short);
        }
        Ok(v)
    }
}

/// One `RefTable` entry (kind 10, [F05 §9.10]), upsert only in the toy.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefEntry {
    /// The ref id.
    pub ref_id: u32,
    /// The ref's name (a harness key).
    pub name: u64,
    /// `rkind`: 1 work, 6 orphans.
    pub rkind: u8,
    /// `eflags` bit 0 deleted.
    pub eflags: u8,
    /// The tip (an op), 0 = empty.
    pub tip: u64,
    /// The checkpoint set the ref's view starts from (a set id), 0 = `main`'s current set.
    pub base_pin: u64,
}

/// `RefTable`: its entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefTableRec {
    /// The entries.
    pub entries: Vec<RefEntry>,
}

impl RefTableRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(8 + self.entries.len() * 40);
        w.uvar(self.entries.len() as u64);
        for e in &self.entries {
            w.u8(1)
                .uvar(u64::from(e.ref_id))
                .u64(e.name)
                .u8(e.rkind)
                .u8(e.eflags)
                .u64(e.tip)
                .uvar(e.base_pin);
        }
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<RefTableRec, Short> {
        let mut r = Reader::new(b);
        let n = r.uvar32()?;
        if n == 0 {
            return Err(Short);
        }
        // An entry is at least 21 bytes: tag, ref id, name, rkind, eflags, tip, base pin.
        let mut entries = Vec::with_capacity(capacity(u64::from(n), &r, 21));
        for _ in 0..n {
            if r.u8()? != 1 {
                return Err(Short);
            }
            entries.push(RefEntry {
                ref_id: r.uvar32()?,
                name: r.u64()?,
                rkind: r.u8()?,
                eflags: r.u8()?,
                tip: r.u64()?,
                base_pin: r.uvar(64)?,
            });
        }
        if !r.done() {
            return Err(Short);
        }
        Ok(RefTableRec { entries })
    }
}

/// `Lease` (kind 4, [F05 §9.4]): the toy's claim and release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseRec {
    /// 1 claim, 2 release.
    pub event: u8,
    /// The lease id; a claim takes its token ([F05] open point 11).
    pub lease_id: u64,
    /// The fencing token.
    pub token: u64,
    /// HLC at append.
    pub hlc: u64,
    /// Claim: the task's uid.
    pub uid: u64,
    /// Claim: the holder (a harness value).
    pub holder: u64,
    /// Claim: the deadline.
    pub expires: Stamp,
    /// Claim: the TTL in ms.
    pub ttl_ms: u64,
    /// Claim (toy field): the claimer's stamp when it decided, which the lease check re-evaluates.
    pub decided_at: Stamp,
    /// Claim (toy field): the lease id this claim released as dead (its deadline passed), 0 = none.
    pub reclaimed: u64,
    /// Release: the reason ([F05 §9.4] field 18).
    pub reason: u8,
}

/// `Lease.event` 1.
pub const LEASE_CLAIM: u8 = 1;
/// `Lease.event` 2.
pub const LEASE_RELEASE: u8 = 2;

impl LeaseRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(128);
        w.u8(self.event)
            .uvar(self.lease_id)
            .u64(self.token)
            .u64(self.hlc);
        if self.event == LEASE_CLAIM {
            w.u64(self.uid)
                .u64(self.holder)
                .bytes(&self.expires.to_bytes())
                .uvar(self.ttl_ms)
                .bytes(&self.decided_at.to_bytes())
                .uvar(self.reclaimed);
        } else {
            w.u8(self.reason);
        }
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<LeaseRec, Short> {
        let mut r = Reader::new(b);
        let event = r.u8()?;
        let mut v = LeaseRec {
            event,
            lease_id: r.uvar(64)?,
            token: r.u64()?,
            hlc: r.u64()?,
            uid: 0,
            holder: 0,
            expires: Stamp::NEVER,
            ttl_ms: 0,
            decided_at: Stamp::NEVER,
            reclaimed: 0,
            reason: 0,
        };
        match event {
            LEASE_CLAIM => {
                v.uid = r.u64()?;
                v.holder = r.u64()?;
                v.expires = Stamp::from_bytes(&r.array()?);
                v.ttl_ms = r.uvar(64)?;
                v.decided_at = Stamp::from_bytes(&r.array()?);
                v.reclaimed = r.uvar(64)?;
            }
            LEASE_RELEASE => v.reason = r.u8()?,
            _ => return Err(Short),
        }
        if !r.done() {
            return Err(Short);
        }
        Ok(v)
    }
}

/// One `Marker` entry (kind 5, [F05 §9.5]): the toy's `settled` markers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkerEntry {
    /// `mkind`: 1 settled.
    pub mkind: u8,
    /// The task's uid (the toy's marker key).
    pub uid: u64,
    /// The origin ref.
    pub ref_id: u32,
    /// The origin commit (its op).
    pub op: u64,
    /// That commit's `seq`.
    pub seq: u64,
    /// HLC at append.
    pub hlc: u64,
    /// `status`: 1 done.
    pub status: u8,
}

/// `Marker`: its entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkerRec {
    /// The entries.
    pub entries: Vec<MarkerEntry>,
}

impl MarkerRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(8 + self.entries.len() * 48);
        w.uvar(self.entries.len() as u64);
        for e in &self.entries {
            w.u8(e.mkind)
                .u64(e.uid)
                .uvar(u64::from(e.ref_id))
                .u64(e.op)
                .uvar(e.seq)
                .u64(e.hlc)
                .u8(e.status);
        }
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<MarkerRec, Short> {
        let mut r = Reader::new(b);
        let n = r.uvar32()?;
        if n == 0 {
            return Err(Short);
        }
        // An entry is at least 28 bytes: mkind, uid, ref id, op, seq, hlc, status.
        let mut entries = Vec::with_capacity(capacity(u64::from(n), &r, 28));
        for _ in 0..n {
            entries.push(MarkerEntry {
                mkind: r.u8()?,
                uid: r.u64()?,
                ref_id: r.uvar32()?,
                op: r.u64()?,
                seq: r.uvar(64)?,
                hlc: r.u64()?,
                status: r.u8()?,
            });
        }
        if !r.done() {
            return Err(Short);
        }
        Ok(MarkerRec { entries })
    }
}

/// `Idem` (kind 6, [F05 §9.6]).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IdemRec {
    /// The idempotency key.
    pub key: u64,
    /// The payload hash.
    pub payload: u64,
    /// The branch.
    pub ref_id: u32,
    /// `iflags`: bit 1 `no_commit`.
    pub iflags: u8,
    /// The recorded commit (its op), 0 with `no_commit`.
    pub op: u64,
    /// The recorded commit's `append_hlc`, or this record's HLC.
    pub append_hlc: u64,
    /// The stored result.
    pub result: u64,
}

impl IdemRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(56);
        w.u64(self.key)
            .u64(self.payload)
            .uvar(u64::from(self.ref_id))
            .u8(self.iflags)
            .u64(self.op)
            .u64(self.append_hlc)
            .u64(self.result);
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<IdemRec, Short> {
        let mut r = Reader::new(b);
        let v = IdemRec {
            key: r.u64()?,
            payload: r.u64()?,
            ref_id: r.uvar32()?,
            iflags: r.u8()?,
            op: r.u64()?,
            append_hlc: r.u64()?,
            result: r.u64()?,
        };
        if !r.done() {
            return Err(Short);
        }
        Ok(v)
    }
}

/// `Pin` (kind 8, [F05 §9.8]).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PinRec {
    /// 1 pin, 2 unpin.
    pub op: u8,
    /// The holder kind: 1 a branch's fork base.
    pub holder: u8,
    /// The ref that holds the pin.
    pub ref_id: u32,
    /// The checkpoint set (the lsn of the `Checkpoint` record that published it; 0 = the set `init` left).
    pub set_lsn: u64,
    /// The set's files, as (family, file number).
    pub files: Vec<(u8, u32)>,
}

impl PinRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(24 + self.files.len() * 6);
        w.u8(self.op)
            .u8(self.holder)
            .uvar(u64::from(self.ref_id))
            .uvar(self.set_lsn)
            .uvar(self.files.len() as u64);
        for &(fam, no) in &self.files {
            w.u8(fam).uvar(u64::from(no));
        }
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<PinRec, Short> {
        let mut r = Reader::new(b);
        let mut v = PinRec {
            op: r.u8()?,
            holder: r.u8()?,
            ref_id: r.uvar32()?,
            set_lsn: r.uvar(64)?,
            files: Vec::new(),
        };
        let n = r.uvar32()?;
        for _ in 0..n {
            let fam = r.u8()?;
            let no = r.uvar32()?;
            v.files.push((fam, no));
        }
        if !r.done() || !matches!(v.op, 1 | 2) {
            return Err(Short);
        }
        Ok(v)
    }
}

/// A file family ([F11 §2.5] `FileFamily`).
pub mod family {
    /// `log.<n>`.
    pub const LOG: u8 = 1;
    /// `hist.<n>`.
    pub const HIST: u8 = 2;
    /// `seg.base.<G>`.
    pub const SEG_BASE: u8 = 3;
}

/// `SegRef` (29 bytes, [F04 §4.1]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SegRef {
    /// The file number.
    pub file_no: u32,
    /// 1 base.
    pub kind: u8,
    /// The lsn the segment folds the log up to.
    pub upto_lsn: u64,
    /// The first 16 bytes of the file's content digest (the toy: XXH3-128 of its content, since BLAKE3 is not among the
    /// toy's dependencies, PLAN §2.2).
    pub digest: [u8; 16],
}

impl SegRef {
    /// The 29 bytes.
    pub fn to_bytes(&self) -> [u8; 29] {
        let mut b = [0u8; 29];
        b[0..4].copy_from_slice(&self.file_no.to_le_bytes());
        b[4] = self.kind;
        b[5..13].copy_from_slice(&self.upto_lsn.to_le_bytes());
        b[13..29].copy_from_slice(&self.digest);
        b
    }

    /// Decodes 29 bytes.
    pub fn from_bytes(b: &[u8; 29]) -> SegRef {
        let mut d = [0u8; 16];
        d.copy_from_slice(&b[13..29]);
        SegRef {
            file_no: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            kind: b[4],
            upto_lsn: u64::from_le_bytes([b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12]]),
            digest: d,
        }
    }
}

/// A `Checkpoint` retirement entry ([F05 §9.9]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Retirement {
    /// The retired extent.
    pub extent: u32,
    /// The `hist` file that holds its history.
    pub hist_file: u32,
    /// Its `total_len`.
    pub total_len: u64,
    /// Its digest.
    pub digest: [u8; 16],
}

/// `Checkpoint` (kind 9, [F05 §9.9]): the toy writes set changes, retirements and releases.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckpointRec {
    /// `ckflags`.
    pub ckflags: u16,
    /// HLC at append (carried without advancing the HLC sequence, [F16] P-36).
    pub append_hlc: u64,
    /// The sealed-file allocator after this record.
    pub next_file_no: u32,
    /// Bit 0: the new `checkpoint_lsn`.
    pub upto_lsn: u64,
    /// Bit 0: the new `active_log`.
    pub active_log: u32,
    /// Bit 0: the new segment set.
    pub segments: Vec<SegRef>,
    /// Bit 4: retirements.
    pub retirements: Vec<Retirement>,
    /// Bit 6: released files (family, number).
    pub released: Vec<(u8, u32)>,
}

/// `ckflags` bit 0 `set_change`.
pub const CK_SET_CHANGE: u16 = 1 << 0;
/// `ckflags` bit 4 `retirements`.
pub const CK_RETIREMENTS: u16 = 1 << 4;
/// `ckflags` bit 6 `files_released`.
pub const CK_RELEASED: u16 = 1 << 6;

impl CheckpointRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(128);
        w.u16(self.ckflags)
            .u64(self.append_hlc)
            .uvar(u64::from(self.next_file_no));
        if self.ckflags & CK_SET_CHANGE != 0 {
            w.uvar(self.upto_lsn)
                .uvar(u64::from(self.active_log))
                .u8(self.segments.len() as u8);
            for s in &self.segments {
                w.bytes(&s.to_bytes());
            }
        }
        if self.ckflags & CK_RETIREMENTS != 0 {
            w.uvar(self.retirements.len() as u64);
            for r in &self.retirements {
                w.uvar(u64::from(r.extent))
                    .uvar(u64::from(r.hist_file))
                    .uvar(r.total_len)
                    .bytes(&r.digest);
            }
        }
        if self.ckflags & CK_RELEASED != 0 {
            w.uvar(self.released.len() as u64);
            for &(fam, no) in &self.released {
                w.u8(fam).uvar(u64::from(no));
            }
        }
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<CheckpointRec, Short> {
        let mut r = Reader::new(b);
        let mut v = CheckpointRec {
            ckflags: r.u16()?,
            append_hlc: r.u64()?,
            next_file_no: r.uvar32()?,
            ..CheckpointRec::default()
        };
        if v.ckflags & !(CK_SET_CHANGE | CK_RETIREMENTS | CK_RELEASED) != 0 {
            return Err(Short);
        }
        if v.ckflags & CK_SET_CHANGE != 0 {
            v.upto_lsn = r.uvar(64)?;
            v.active_log = r.uvar32()?;
            let n = r.u8()?;
            if n > 8 {
                return Err(Short);
            }
            for _ in 0..n {
                v.segments.push(SegRef::from_bytes(&r.array()?));
            }
        } else if v.ckflags & CK_RETIREMENTS != 0 {
            return Err(Short);
        }
        if v.ckflags & CK_RETIREMENTS != 0 {
            let n = r.uvar32()?;
            for _ in 0..n {
                v.retirements.push(Retirement {
                    extent: r.uvar32()?,
                    hist_file: r.uvar32()?,
                    total_len: r.uvar(64)?,
                    digest: r.array()?,
                });
            }
        }
        if v.ckflags & CK_RELEASED != 0 {
            let n = r.uvar32()?;
            for _ in 0..n {
                let fam = r.u8()?;
                let no = r.uvar32()?;
                v.released.push((fam, no));
            }
        }
        if !r.done() {
            return Err(Short);
        }
        Ok(v)
    }
}

/// One item of an `FsIntent` (toy form of [F05 §9.15] `IntentItem`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntentItem {
    /// The project file (a harness key; its name is derived from it).
    pub file: u64,
    /// The source directory (an index of the toy's project directories).
    pub src_dir: u8,
    /// The destination directory (`mv`), or the trash (`rm --trash`), or 0 (`rm`).
    pub dst_dir: u8,
    /// The source file's content digest at planning.
    pub oid: u64,
}

/// `FsIntent` (kind 15).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntentRec {
    /// 1 `mv`, 2 `rm`, 3 `rm --trash`.
    pub op: u8,
    /// The caller's branch.
    pub branch: u32,
    /// The intent anchor, kind 2 ([F03 §10]).
    pub anchor: [u8; 32],
    /// The CLI's `ProcId`, diagnostics only.
    pub proc: [u8; 32],
    /// HLC at append.
    pub hlc: u64,
    /// The harness's key of the operation (toy field).
    pub key: u64,
    /// The items.
    pub items: Vec<IntentItem>,
}

/// `FsIntent.op` 1.
pub const INTENT_MV: u8 = 1;
/// `FsIntent.op` 2.
pub const INTENT_RM: u8 = 2;
/// `FsIntent.op` 3.
pub const INTENT_RM_TRASH: u8 = 3;

impl IntentRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(128);
        w.u8(self.op)
            .u8(0)
            .uvar(u64::from(self.branch))
            .bytes(&self.anchor)
            .bytes(&self.proc)
            .u64(self.hlc)
            .u64(self.key)
            .uvar(self.items.len() as u64);
        for it in &self.items {
            w.u8(0)
                .u64(it.file)
                .u8(it.src_dir)
                .u8(it.dst_dir)
                .u64(it.oid);
        }
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<IntentRec, Short> {
        let mut r = Reader::new(b);
        let op = r.u8()?;
        if !matches!(op, 1..=3) || r.u8()? != 0 {
            return Err(Short);
        }
        let mut v = IntentRec {
            op,
            branch: r.uvar32()?,
            anchor: r.array()?,
            proc: r.array()?,
            hlc: r.u64()?,
            key: r.u64()?,
            items: Vec::new(),
        };
        let n = r.uvar32()?;
        if n == 0 {
            return Err(Short);
        }
        for _ in 0..n {
            if r.u8()? != 0 {
                return Err(Short);
            }
            v.items.push(IntentItem {
                file: r.u64()?,
                src_dir: r.u8()?,
                dst_dir: r.u8()?,
                oid: r.u64()?,
            });
        }
        if !r.done() {
            return Err(Short);
        }
        Ok(v)
    }
}

/// `FsIntentDone` (kind 16).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntentDoneRec {
    /// The intent id (its record's lsn).
    pub intent_lsn: u64,
    /// Bit 0 `recovered`.
    pub dflags: u8,
    /// HLC at append.
    pub hlc: u64,
    /// Per item: 1 done, 2–5 failed.
    pub outcomes: Vec<u8>,
}

impl IntentDoneRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(32);
        w.uvar(self.intent_lsn)
            .u8(self.dflags)
            .u64(self.hlc)
            .uvar(self.outcomes.len() as u64)
            .bytes(&self.outcomes);
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<IntentDoneRec, Short> {
        let mut r = Reader::new(b);
        let intent_lsn = r.uvar(64)?;
        let dflags = r.u8()?;
        let hlc = r.u64()?;
        let n = r.uvar32()? as usize;
        let outcomes = r.bytes(n)?.to_vec();
        if !r.done() || dflags > 1 || outcomes.iter().any(|&o| !(1..=5).contains(&o)) {
            return Err(Short);
        }
        Ok(IntentDoneRec {
            intent_lsn,
            dflags,
            hlc,
            outcomes,
        })
    }
}

/// `FsIntentAborted` (kind 17).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntentAbortRec {
    /// The intent id.
    pub intent_lsn: u64,
    /// The abort reason (1–5).
    pub reason: u8,
    /// Bit 0 `recovered`.
    pub aflags: u8,
    /// HLC at append.
    pub hlc: u64,
}

impl IntentAbortRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(24);
        w.uvar(self.intent_lsn)
            .u8(self.reason)
            .u8(self.aflags)
            .u64(self.hlc);
        w.buf
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<IntentAbortRec, Short> {
        let mut r = Reader::new(b);
        let v = IntentAbortRec {
            intent_lsn: r.uvar(64)?,
            reason: r.u8()?,
            aflags: r.u8()?,
            hlc: r.u64()?,
        };
        if !r.done() || !(1..=5).contains(&v.reason) || v.aflags > 1 {
            return Err(Short);
        }
        Ok(v)
    }
}

/// The toy's runtime-row batch (`FileObs`, kind 18, lazy, `K_RT`; row batches of [F05 §8.5]): upserts of (key, value)
/// rows, each row framed by its length; the last row may carry padding (the batch's size).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimeRec {
    /// The rows: (key, value, padding bytes).
    pub rows: Vec<(u64, u64, u32)>,
}

impl RuntimeRec {
    /// The payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let pad: usize = self.rows.iter().map(|r| r.2 as usize).sum();
        let mut w = Writer::with_capacity(8 + self.rows.len() * 24 + pad);
        w.uvar(self.rows.len() as u64);
        for &(k, v, p) in &self.rows {
            let mut row = Writer::with_capacity(17 + p as usize);
            row.u8(1).u64(k).u64(v).zeros(p as usize);
            w.vbytes(&row.buf);
        }
        w.buf
    }

    /// The payload's length without encoding it.
    pub fn encoded_len(&self) -> usize {
        let mut n = crate::codec::uvar_len(self.rows.len() as u64);
        for &(_, _, p) in &self.rows {
            let row = 17 + p as usize;
            n += crate::codec::uvar_len(row as u64) + row;
        }
        n
    }

    /// Decodes a payload.
    pub fn decode(b: &[u8]) -> Result<RuntimeRec, Short> {
        let mut r = Reader::new(b);
        let n = r.uvar32()?;
        if n == 0 {
            return Err(Short);
        }
        // A row is at least 18 bytes: its length prefix, the tag, the key and the value.
        let mut rows = Vec::with_capacity(capacity(u64::from(n), &r, 18));
        for _ in 0..n {
            let row = r.vbytes()?;
            let mut rr = Reader::new(row);
            if rr.u8()? != 1 {
                return Err(Short);
            }
            let k = rr.u64()?;
            let v = rr.u64()?;
            let p = rr.rest() as u32;
            if rr.bytes(p as usize)?.iter().any(|&x| x != 0) {
                return Err(Short);
            }
            rows.push((k, v, p));
        }
        if !r.done() {
            return Err(Short);
        }
        Ok(RuntimeRec { rows })
    }
}

/// `InitParams` (32 bytes, [F04 §4.4]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct InitParams {
    /// `store.log-extent-bytes`, E.
    pub log_extent_bytes: u64,
    /// `store.hist-frame-commits`.
    pub hist_frame_commits: u32,
    /// `store.hist-frame-bytes`.
    pub hist_frame_bytes: u32,
    /// The store id.
    pub store_id: [u8; 16],
}

impl InitParams {
    /// The 32 bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0..8].copy_from_slice(&self.log_extent_bytes.to_le_bytes());
        b[8..12].copy_from_slice(&self.hist_frame_commits.to_le_bytes());
        b[12..16].copy_from_slice(&self.hist_frame_bytes.to_le_bytes());
        b[16..32].copy_from_slice(&self.store_id);
        b
    }

    /// Decodes 32 bytes.
    pub fn from_bytes(b: &[u8; 32]) -> InitParams {
        let mut id = [0u8; 16];
        id.copy_from_slice(&b[16..32]);
        InitParams {
            log_extent_bytes: u64::from_le_bytes(b[0..8].try_into().unwrap_or([0; 8])),
            hist_frame_commits: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            hist_frame_bytes: u32::from_le_bytes([b[12], b[13], b[14], b[15]]),
            store_id: id,
        }
    }

    /// IP-2 of [F17 §2.2]: the ranges of P01, P03, P04 and a non-zero store id.
    pub fn valid(&self) -> bool {
        let e = self.log_extent_bytes;
        e.is_power_of_two()
            && (1 << 16..=1 << 30).contains(&e)
            && (1..=65_536).contains(&self.hist_frame_commits)
            && (4_096..=1 << 20).contains(&self.hist_frame_bytes)
            && self.store_id != [0; 16]
    }
}

/// The log-derived counters an extent head carries and the `HEAD` fold maintains ([F05 §9.28], [F04 §5.7]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Counters {
    /// `commit_seq`.
    pub commit_seq: u64,
    /// `next_id`.
    pub next_id: u32,
    /// `next_anchor`.
    pub next_anchor: u32,
    /// `fence`.
    pub fence: u64,
    /// `next_file_no`.
    pub next_file_no: u32,
    /// `next_ref_id`.
    pub next_ref_id: u32,
    /// `hlc_seq`.
    pub hlc_seq: u64,
    /// `hlc_commit`.
    pub hlc_commit: u64,
}

impl Counters {
    /// The counters of an empty store ([F16] P-88 step 5).
    pub const EMPTY: Counters = Counters {
        commit_seq: 0,
        next_id: 1,
        next_anchor: 1,
        fence: 0,
        next_file_no: 1,
        next_ref_id: 0,
        hlc_seq: 0,
        hlc_commit: 0,
    };

    /// The field-wise maximum.
    pub fn max(self, o: Counters) -> Counters {
        Counters {
            commit_seq: self.commit_seq.max(o.commit_seq),
            next_id: self.next_id.max(o.next_id),
            next_anchor: self.next_anchor.max(o.next_anchor),
            fence: self.fence.max(o.fence),
            next_file_no: self.next_file_no.max(o.next_file_no),
            next_ref_id: self.next_ref_id.max(o.next_ref_id),
            hlc_seq: self.hlc_seq.max(o.hlc_seq),
            hlc_commit: self.hlc_commit.max(o.hlc_commit),
        }
    }
}

/// `ExtentHead` (kind 28, [F05 §9.28]): the product's 98-byte payload.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ExtentHeadRec {
    /// `HEAD.epoch_lsn` of the record's epoch.
    pub epoch_lsn: u64,
    /// The chain value at the record's own lsn.
    pub chain_in: u64,
    /// `HEAD.init`.
    pub init: InitParams,
    /// `HEAD.project_oid_algo`.
    pub project_oid_algo: u8,
    /// Bit 0 quiet, bit 1 readonly.
    pub hflags: u8,
    /// The counters of the log before the record.
    pub counters: Counters,
}

impl ExtentHeadRec {
    /// The 98 payload bytes.
    pub fn encode(&self) -> Vec<u8> {
        let c = &self.counters;
        let mut w = Writer::with_capacity(EXTENT_HEAD_PAYLOAD);
        w.u64(self.epoch_lsn)
            .u64(self.chain_in)
            .bytes(&self.init.to_bytes())
            .u8(self.project_oid_algo)
            .u8(self.hflags)
            .u64(c.commit_seq)
            .u32(c.next_id)
            .u32(c.next_anchor)
            .u64(c.fence)
            .u32(c.next_file_no)
            .u32(c.next_ref_id)
            .u64(c.hlc_seq)
            .u64(c.hlc_commit);
        debug_assert_eq!(w.buf.len(), EXTENT_HEAD_PAYLOAD);
        w.buf
    }

    /// Decodes the 98 bytes.
    pub fn decode(b: &[u8]) -> Result<ExtentHeadRec, Short> {
        if b.len() != EXTENT_HEAD_PAYLOAD {
            return Err(Short);
        }
        let mut r = Reader::new(b);
        let v = ExtentHeadRec {
            epoch_lsn: r.u64()?,
            chain_in: r.u64()?,
            init: InitParams::from_bytes(&r.array()?),
            project_oid_algo: r.u8()?,
            hflags: r.u8()?,
            counters: Counters {
                commit_seq: r.u64()?,
                next_id: r.u32()?,
                next_anchor: r.u32()?,
                fence: r.u64()?,
                next_file_no: r.u32()?,
                next_ref_id: r.u32()?,
                hlc_seq: r.u64()?,
                hlc_commit: r.u64()?,
            },
        };
        if v.hflags & !0b11 != 0 {
            return Err(Short);
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn buf_at(recs: &[Rec], p: u64, epoch: u64, seed: u64) -> (Vec<u8>, u64) {
        let mut out = Vec::new();
        let c = encode_group(recs, p, epoch, seed, &mut out);
        (out, c)
    }

    #[test]
    fn the_extent_head_group_is_138_bytes() {
        let h = ExtentHeadRec::default();
        let r = Rec::new(kind::EXTENT_HEAD, h.encode(), Bugs::NONE);
        assert_eq!(group_len(&[r]), HEAD_GROUP);
        assert_eq!(ROTATION_RESERVE, 178);
        assert_eq!(
            Rec::new(kind::NOOP, Vec::new(), Bugs::NONE).len(true),
            MIN_GROUP
        );
    }

    #[test]
    fn a_group_validates_at_its_own_position_epoch_and_seed() {
        let recs = [
            Rec::new(kind::COMMIT, CommitRec::default().encode(), Bugs::NONE),
            Rec::new(kind::IDEM, IdemRec::default().encode(), Bugs::NONE),
        ];
        let e = 1u64 << 16;
        let p = 1000;
        let (b, chain) = buf_at(&recs, p, 7, 42);
        let mut ext = vec![0u8; e as usize];
        ext[p as usize..p as usize + b.len()].copy_from_slice(&b);
        let (r0, end0, l0) = validate_record(&ext, p as usize, e, p, 7, Bugs::NONE).unwrap();
        assert!(!end0 && r0.kind == kind::COMMIT && !r0.lazy);
        let (r1, end1, l1) =
            validate_record(&ext, (p + l0) as usize, e, p + l0, 7, Bugs::NONE).unwrap();
        assert!(end1 && r1.kind == kind::IDEM);
        assert_eq!(l0 + l1, b.len() as u64);
        // The trailer is the seeded hash of the group's bytes before it.
        let t = hash64_seeded(&b[..b.len() - 8], 42);
        assert_eq!(t, chain);
        // Position, epoch and checksum checks.
        assert_eq!(
            validate_record(&ext, p as usize, e, p + 1, 7, Bugs::NONE).map(|_| ()),
            Err(Invalid::Position)
        );
        assert!(
            validate_record(
                &ext,
                p as usize,
                e,
                p + 1,
                7,
                Bugs::only(Bug::P54T13NoPositionCheck)
            )
            .is_ok()
        );
        assert_eq!(
            validate_record(&ext, p as usize, e, p, 8, Bugs::NONE).map(|_| ()),
            Err(Invalid::Epoch)
        );
        assert!(
            validate_record(&ext, p as usize, e, p, 8, Bugs::only(Bug::P55NoEpochCheck)).is_ok()
        );
        ext[p as usize + 40] ^= 1;
        assert_eq!(
            validate_record(&ext, p as usize, e, p, 7, Bugs::NONE).map(|_| ()),
            Err(Invalid::Checksum)
        );
    }

    #[test]
    fn the_class_tag_must_agree_with_the_kind() {
        let lease = LeaseRec {
            event: LEASE_RELEASE,
            lease_id: 1,
            token: 1,
            hlc: 0,
            uid: 0,
            holder: 0,
            expires: Stamp::NEVER,
            ttl_ms: 0,
            decided_at: Stamp::NEVER,
            reclaimed: 0,
            reason: 1,
        };
        let e = 1u64 << 16;
        let r = Rec::new(kind::LEASE, lease.encode(), Bugs::NONE);
        assert!(!r.lazy);
        let mut forged = r.clone();
        forged.lazy = true;
        let (b, _) = buf_at(&[forged], 0, 1, 1);
        let mut ext = vec![0u8; e as usize];
        ext[..b.len()].copy_from_slice(&b);
        assert_eq!(
            validate_record(&ext, 0, e, 0, 1, Bugs::NONE).map(|_| ()),
            Err(Invalid::Bits)
        );
        // Under P-5's seeded bug the registry itself says lazy, and the lazy tag validates.
        let bug = Bugs::only(Bug::P05LeaseTaggedLazy);
        assert!(Rec::new(kind::LEASE, lease.encode(), bug).lazy);
        assert!(validate_record(&ext, 0, e, 0, 1, bug).is_ok());
    }

    #[test]
    fn a_record_must_lie_inside_its_extent() {
        let e = 1u64 << 16;
        let r = Rec::new(kind::NOOP, vec![0; 100], Bugs::NONE);
        let p = e - 60;
        let (b, _) = buf_at(&[r], p, 1, 1);
        let mut ext = vec![0u8; e as usize + 200];
        ext[p as usize..p as usize + b.len()].copy_from_slice(&b);
        assert_eq!(
            validate_record(&ext, p as usize, e, p, 1, Bugs::NONE).map(|_| ()),
            Err(Invalid::Length)
        );
    }

    #[test]
    fn payloads_round_trip() {
        let c = CommitRec {
            op: 5,
            digest: 6,
            seq: 7,
            ref_id: 3,
            ref_old: 4,
            hlc: 1 << 40,
            append_hlc: 1 << 40,
            wall_ms: 99,
            flags: COMMIT_IMPORTED,
            creates: vec![(1, 11), (300, 12)],
            filler: 17,
        };
        assert_eq!(CommitRec::decode(&c.encode()), Ok(c.clone()));
        let ck = CheckpointRec {
            ckflags: CK_SET_CHANGE | CK_RETIREMENTS | CK_RELEASED,
            append_hlc: 1,
            next_file_no: 9,
            upto_lsn: 1 << 17,
            active_log: 2,
            segments: vec![SegRef {
                file_no: 8,
                kind: 1,
                upto_lsn: 1 << 17,
                digest: [3; 16],
            }],
            retirements: vec![Retirement {
                extent: 1,
                hist_file: 7,
                total_len: 100,
                digest: [4; 16],
            }],
            released: vec![(family::SEG_BASE, 5)],
        };
        assert_eq!(CheckpointRec::decode(&ck.encode()), Ok(ck));
        let rt = RuntimeRec {
            rows: vec![(1, 2, 0), (3, 4, 1000)],
        };
        assert_eq!(rt.encoded_len(), rt.encode().len());
        assert_eq!(RuntimeRec::decode(&rt.encode()), Ok(rt));
        let h = ExtentHeadRec {
            epoch_lsn: 0,
            chain_in: 77,
            init: InitParams {
                log_extent_bytes: 1 << 16,
                hist_frame_commits: 4,
                hist_frame_bytes: 4096,
                store_id: [9; 16],
            },
            project_oid_algo: 1,
            hflags: 1,
            counters: Counters::EMPTY,
        };
        assert_eq!(h.encode().len(), EXTENT_HEAD_PAYLOAD);
        assert_eq!(ExtentHeadRec::decode(&h.encode()), Ok(h));
        assert!(h.init.valid());
        let i = IntentRec {
            op: INTENT_MV,
            branch: 0,
            anchor: [2; 32],
            proc: [1; 32],
            hlc: 5,
            key: 8,
            items: vec![IntentItem {
                file: 1,
                src_dir: 1,
                dst_dir: 2,
                oid: 3,
            }],
        };
        assert_eq!(IntentRec::decode(&i.encode()), Ok(i));
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        #[test]
        fn validation_never_panics_on_garbage(ext in proptest::collection::vec(any::<u8>(), 0..512), off in 0usize..512) {
            let _ = validate_record(&ext, off.min(ext.len()), 1 << 16, 0, 1, Bugs::NONE);
        }

        #[test]
        fn groups_validate_record_by_record(sizes in proptest::collection::vec(0usize..300, 1..6), p in 0u64..1000, seed in any::<u64>()) {
            let recs: Vec<Rec> = sizes.iter().map(|&n| Rec::new(kind::NOOP, vec![0; n], Bugs::NONE)).collect();
            let (b, _) = buf_at(&recs, p, 3, seed);
            prop_assert_eq!(b.len() as u64, group_len(&recs));
            let mut ext = vec![0u8; 1 << 16];
            ext[p as usize..p as usize + b.len()].copy_from_slice(&b);
            let mut at = p;
            for (i, _) in recs.iter().enumerate() {
                let (_, end, len) = validate_record(&ext, at as usize, 1 << 16, at, 3, Bugs::NONE).unwrap();
                prop_assert_eq!(end, i + 1 == recs.len());
                at += len;
            }
            prop_assert_eq!(at, p + b.len() as u64);
        }
    }
}
