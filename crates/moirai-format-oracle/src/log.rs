//! \[F05\] the log: extents and lsns (§2), `RecHdr` (§3), groups and the chain trailer (§4), the scan with its validity
//! and end-of-log rules (§5), durability classes (§6), the record kinds (§7), the common encodings (§8) and every payload
//! (§9) except the commit body, which is [`crate::commit`]'s.
//!
//! A record that fails §5.2 is *invalid* (it ends the valid log or is corruption, §5.3); a valid record whose payload
//! breaks its kind's rules is *malformed*, which is corruption wherever it lies (§5.4). [`RecError`] keeps the two apart.

use std::collections::HashMap;
use std::rc::Rc;

use crate::commit::Commit;
use crate::head::{InitParams, SegRef, check_segment_set};
use crate::lock::{Anchor, ProcId};
use crate::prim::{Algo, Error, Oid, Reader, Result, Writer, err, xxh3_64, xxh3_64_seeded};
use crate::runtime::{
    self, Epoch, IntentItem, OsFileId, Place, Renames, Row, Stamp, Table, VolumeCaps,
};

/// Size of `RecHdr` ([F05 §3.1]).
pub const HDR: usize = 32;
/// Length of an extent-head group, H ([F05 §4.4]).
pub const H: u64 = 138;
/// The rotation reserve R = H + 40 ([F05 §4.4]).
pub const R: u64 = H + 40;
/// The smallest group ([F05 §4.4]).
pub const MIN_GROUP: u64 = 40;
/// The end of the last usable extent, `log.4294967295`, at the largest extent size 2^30: every record ends at or below
/// it, whatever `E` is ([F05 §2.2], §2.3).
pub const LSN_END_MAX: u64 = (u32::MAX as u64) << 30;

/// Durability class of a record kind ([F05 §7]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Bit 0 must be 0.
    Durable,
    /// Bit 0 must be 1.
    Lazy,
    /// Either, by the writer's configuration.
    Configurable,
}

/// The registry class of `kind`, or `None` when the kind is invalid ([F05 §7]).
pub fn kind_class(kind: u8) -> Option<Class> {
    Some(match kind {
        1..=10 | 13 | 15..=17 | 27 | 28 => Class::Durable,
        12 | 18..=26 => Class::Lazy,
        11 | 14 => Class::Configurable,
        _ => return None,
    })
}

/// The kind name ([F05 §7]).
pub fn kind_name(kind: u8) -> &'static str {
    const N: [&str; 29] = [
        "?",
        "Commit",
        "RefUpdate",
        "ClientHead",
        "Lease",
        "Marker",
        "Idem",
        "GitMap",
        "Pin",
        "Checkpoint",
        "RefTable",
        "Lazy",
        "Noop",
        "Backup",
        "SessionMark",
        "FsIntent",
        "FsIntentDone",
        "FsIntentAborted",
        "FileObs",
        "Pending",
        "FPrint",
        "JournalCursor",
        "DirMap",
        "TreeReg",
        "PrefixEv",
        "GitFacts",
        "AnchorRes",
        "Reserve",
        "ExtentHead",
    ];
    N.get(usize::from(kind)).copied().unwrap_or("?")
}

/// `RecHdr` ([F05 §3.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecHdr {
    /// Total record length.
    pub len: u32,
    /// Record kind.
    pub kind: u8,
    /// Bit 0 `lazy`, bit 1 `group_end`, bit 2 `symdefs`.
    pub flags: u8,
    /// Own position.
    pub lsn: u64,
    /// Store epoch.
    pub epoch: u64,
    /// Record checksum.
    pub xxh3_64: u64,
}

impl RecHdr {
    /// `group_end`.
    pub fn group_end(&self) -> bool {
        self.flags & 2 != 0
    }

    /// `lazy`.
    pub fn lazy(&self) -> bool {
        self.flags & 1 != 0
    }

    /// `symdefs`.
    pub fn symdefs(&self) -> bool {
        self.flags & 4 != 0
    }

    fn read(b: &[u8]) -> RecHdr {
        let u32_ = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().expect("4"));
        let u64_ = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().expect("8"));
        RecHdr {
            len: u32_(0),
            kind: b[4],
            flags: b[5],
            lsn: u64_(8),
            epoch: u64_(16),
            xxh3_64: u64_(24),
        }
    }
}

/// Why a record or group is not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecError {
    /// §5.2 or §4.6 failed: the bytes are not a valid record or group.
    Invalid(Error),
    /// §5.4: a valid record's payload breaks its kind's rules; corruption wherever it lies.
    Malformed(Error),
}

impl From<Error> for RecError {
    fn from(e: Error) -> Self {
        RecError::Malformed(e)
    }
}

fn invalid<T>(at: usize, m: impl Into<String>) -> core::result::Result<T, RecError> {
    Err(RecError::Invalid(Error {
        offset: at,
        reason: m.into(),
        rule: None,
    }))
}

/// The symbol classes of [F05 §8.1] (the same codes [F09 §14.1] uses).
pub mod sym {
    /// `actor`.
    pub const ACTOR: u8 = 1;
    /// `role`.
    pub const ROLE: u8 = 2;
    /// `session`.
    pub const SESSION: u8 = 3;
    /// `ref`.
    pub const REF: u8 = 4;
    /// `git-branch`.
    pub const GIT_BRANCH: u8 = 5;
    /// `git-worktree`.
    pub const GIT_WORKTREE: u8 = 6;
    /// `stmt`.
    pub const STMT: u8 = 7;
    /// `root`.
    pub const ROOT: u8 = 8;
    /// `reason`.
    pub const REASON: u8 = 9;
    /// `name`.
    pub const NAME: u8 = 10;
    /// `text`.
    pub const TEXT: u8 = 11;

    /// True for the `u16` classes ([F01 §8.2]).
    pub fn is_u16(class: u8) -> bool {
        class == ROLE || class == ROOT
    }
}

/// One symbol definition ([F05 §8.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymDef {
    /// Class code 1–11.
    pub class: u8,
    /// The new id.
    pub id: u32,
    /// Its string, not empty.
    pub text: String,
}

/// Decodes a `SymDefs` block with SD-1's in-block rule (consecutive ids per class) and a non-empty `text`.
pub fn decode_symdefs(r: &mut Reader<'_>) -> Result<Vec<SymDef>> {
    let at = r.offset();
    let n = usize::from(r.uvar16()?);
    if n == 0 {
        return err(at, "SymDefs n_defs is 0 [F05 §8.1]");
    }
    let mut v: Vec<SymDef> = Vec::with_capacity(n);
    for _ in 0..n {
        let d_at = r.offset();
        let class = r.u8()?;
        if !(1..=11).contains(&class) {
            return err(
                d_at,
                format!("SymDef class {class} outside 1-11 [F05 §8.1]"),
            );
        }
        let id = if sym::is_u16(class) {
            u32::from(r.uvar16()?)
        } else {
            r.uvar32()?
        };
        let t_at = r.offset();
        let text = r.vstr()?;
        if text.is_empty() {
            return err(t_at, "SymDef text is empty [F05 §8.1]");
        }
        if id == 0 {
            return err(d_at, "SymDef id 0 [F01 §8.1 S2]");
        }
        if let Some(prev) = v.iter().rev().find(|d| d.class == class) {
            // In u64: a class's previous id may be the greatest of its width ([F01 §8.1] S4), and nothing follows it.
            if u64::from(id) != u64::from(prev.id) + 1 {
                return err(
                    d_at,
                    "definitions of one class in a block are not consecutive [F05 §8.1 SD-1]",
                );
            }
            if v.iter().any(|d| d.class == class && d.text == text) {
                return err(
                    t_at,
                    "two definitions of one class with one text [F05 §8.1 SD-2]",
                );
            }
        }
        v.push(SymDef {
            class,
            id,
            text: text.to_owned(),
        });
    }
    Ok(v)
}

/// Encodes a `SymDefs` block.
pub fn encode_symdefs(v: &[SymDef], w: &mut Writer) {
    w.uvar(v.len() as u64);
    for d in v {
        w.u8(d.class);
        w.uvar(u64::from(d.id));
        w.vstr(&d.text);
    }
}

/// The strings of one symbol class defined so far ([F05 §8.1]), each held once and reachable by its text (SD-2) and by
/// its id (name resolution, §9.28 rules over `Schema` ops).
#[derive(Clone, Debug, Default)]
pub struct ClassSymbols {
    by_text: HashMap<Rc<str>, u32>,
    by_id: HashMap<u32, Rc<str>>,
}

impl ClassSymbols {
    fn insert(&mut self, text: &str, id: u32) {
        let t: Rc<str> = Rc::from(text);
        self.by_text.insert(Rc::clone(&t), id);
        self.by_id.insert(id, t);
    }

    /// The id `text` names, when defined.
    pub fn id(&self, text: &str) -> Option<u32> {
        self.by_text.get(text).copied()
    }

    /// The string of `id`, when defined.
    pub fn text(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id).map(|t| &**t)
    }
}

/// The symbol state a scan carries ([F05 §8.1], §10.5): per class, the next id and the strings defined.
#[derive(Clone, Debug, Default)]
pub struct Symbols {
    /// Per class 1–11: the next id when known (from `SYMTAB`, or the first definition met). It is a `u64`: after the
    /// greatest id of a class's width ([F01 §8.1] S4) it is 2^32 (or 65,536), which no definition can take.
    pub next: [Option<u64>; 12],
    /// Per class: the strings defined so far with their ids.
    pub strings: [ClassSymbols; 12],
    /// True when `next` came from a `SYMTAB`: every reference must then be below it (SD-3).
    pub complete: bool,
}

impl Symbols {
    /// A state whose `SYMTAB` is known: `next[c]` is one above the greatest id of class c there.
    pub fn from_symtab(defs: &[(u8, u32, String)]) -> Symbols {
        let mut s = Symbols {
            complete: true,
            ..Symbols::default()
        };
        for c in 1..=11 {
            s.next[c] = Some(1);
        }
        for (c, id, t) in defs {
            let c = usize::from(*c);
            s.next[c] = Some(s.next[c].unwrap_or(1).max(u64::from(*id) + 1));
            s.strings[c].insert(t, *id);
        }
        s
    }

    /// Applies a record's block (SD-1, SD-2 against the state).
    fn define(&mut self, defs: &[SymDef], at: usize) -> Result<()> {
        for d in defs {
            let c = usize::from(d.class);
            if let Some(n) = self.next[c]
                && u64::from(d.id) != n
            {
                return err(
                    at,
                    format!(
                        "SymDef id {} is not the next id {n} of class {} [F05 §8.1 SD-1]",
                        d.id, d.class
                    ),
                );
            }
            if self.strings[c].id(&d.text).is_some() {
                return err(
                    at,
                    "SymDef text already names another id of its class [F05 §8.1 SD-2]",
                );
            }
            self.strings[c].insert(&d.text, d.id);
            self.next[c] = Some(u64::from(d.id) + 1);
        }
        Ok(())
    }

    /// The string of symbol `id` of `class`, when the state holds it.
    pub fn text(&self, class: u8, id: u32) -> Option<&str> {
        self.strings.get(usize::from(class))?.text(id)
    }

    /// SD-3: every reference is defined.
    fn check_refs(&self, refs: &[(u8, u32)], at: usize) -> Result<()> {
        for &(c, id) in refs {
            if id == 0 {
                continue;
            }
            let known = match self.next[usize::from(c)] {
                Some(n) => u64::from(id) < n,
                None => !self.complete,
            };
            if !known {
                return err(
                    at,
                    format!("symbol {id} of class {c} is not defined [F05 §8.1 SD-3]"),
                );
            }
        }
        Ok(())
    }
}

/// `FileRefV` ([F05 §8.4]): family, ref_id (family 5), file_no.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileRefV {
    /// `FileFamily` 1–9.
    pub family: u8,
    /// Ref id for family 5.
    pub ref_id: u32,
    /// File number ≥ 1.
    pub file_no: u32,
}

impl FileRefV {
    /// Decodes a `FileRefV`.
    pub fn decode(r: &mut Reader<'_>) -> Result<Self> {
        let at = r.offset();
        let family = r.u8()?;
        if !(1..=9).contains(&family) {
            return err(
                at,
                format!("FileRefV family {family} outside 1-9 [F11 §2.5]"),
            );
        }
        let ref_id = if family == 5 { r.uvar32()? } else { 0 };
        let f_at = r.offset();
        let file_no = r.uvar32()?;
        if file_no == 0 {
            return err(f_at, "FileRefV file_no 0 [F05 §8.4]");
        }
        Ok(FileRefV {
            family,
            ref_id,
            file_no,
        })
    }

    /// Encodes a `FileRefV`.
    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.family);
        if self.family == 5 {
            w.uvar(u64::from(self.ref_id));
        }
        w.uvar(u64::from(self.file_no));
    }
}

/// `FileEntry` ([F05 §8.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// The file.
    pub file: FileRefV,
    /// Its `total_len`.
    pub total_len: u64,
    /// First 16 bytes of its recorded digest.
    pub digest: [u8; 16],
}

fn decode_file_entry(r: &mut Reader<'_>) -> Result<FileEntry> {
    Ok(FileEntry {
        file: FileRefV::decode(r)?,
        total_len: r.uvar64()?,
        digest: r.b16()?,
    })
}

fn encode_file_entry(e: &FileEntry, w: &mut Writer) {
    e.file.encode(w);
    w.uvar(e.total_len);
    w.bytes(&e.digest);
}

/// A glob list ([F05 §8.6]): (root, pattern).
pub type GlobList = Vec<(u16, String)>;

fn decode_globs(r: &mut Reader<'_>) -> Result<GlobList> {
    let n = r.count(3)?;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push((r.u16()?, r.vstr()?.to_owned()));
    }
    Ok(v)
}

fn encode_globs(g: &GlobList, w: &mut Writer) {
    w.uvar(g.len() as u64);
    for (root, p) in g {
        w.u16(*root);
        w.vstr(p);
    }
}

fn byte_range(r: &mut Reader<'_>, lo: u8, hi: u8, what: &str) -> Result<u8> {
    let at = r.offset();
    let v = r.u8()?;
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        err(at, format!("{what} {v} outside {lo}-{hi} [F05 §9]"))
    }
}

fn flag_bits(r: &mut Reader<'_>, allowed: u8, what: &str) -> Result<u8> {
    let at = r.offset();
    let v = r.u8()?;
    if v & !allowed != 0 {
        return err(at, format!("{what} has reserved bits set [F05 §9]"));
    }
    Ok(v)
}

fn anchor_of(r: &mut Reader<'_>, kinds: &[u8], what: &str) -> Result<Anchor> {
    let at = r.offset();
    let a = Anchor::decode(r)?;
    if !kinds.contains(&a.kind) {
        return err(
            at,
            format!("{what} anchor kind {} not admitted [F05 §9]", a.kind),
        );
    }
    Ok(a)
}

fn absorbed(r: &mut Reader<'_>) -> Result<Vec<(u32, u32)>> {
    let n = r.count(2)?;
    let mut v: Vec<(u32, u32)> = Vec::with_capacity(n);
    for _ in 0..n {
        let at = r.offset();
        let e = (r.uvar32()?, r.uvar32()?);
        if v.last().is_some_and(|p| p.0 >= e.0) {
            return err(
                at,
                "absorbed vector not sorted by ref id, each once [F05 §9.2]",
            );
        }
        v.push(e);
    }
    Ok(v)
}

fn put_absorbed(v: &[(u32, u32)], w: &mut Writer) {
    w.uvar(v.len() as u64);
    for (a, b) in v {
        w.uvar(u64::from(*a));
        w.uvar(u64::from(*b));
    }
}

/// `RefUpdate` ([F05 §9.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefUpdate {
    /// 1 create … 5 park.
    pub reason: u8,
    /// The ref.
    pub ref_id: u32,
    /// Actor symbol.
    pub actor: u32,
    /// HLC.
    pub hlc: u64,
    /// Tip before.
    pub old: [u8; 32],
    /// Tip after.
    pub new: [u8; 32],
    /// Absorbed vector (reasons 1, 3, 4).
    pub absorbed: Option<Vec<(u32, u32)>>,
    /// `undo N` (reason 3).
    pub moves_back: Option<u32>,
    /// `op restore` seq (reason 4).
    pub restore_seq: Option<u64>,
}

/// `ClientHead` ([F05 §9.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientHead {
    /// `op` 1: the new `HEADS` row image.
    Set(Box<Row>),
    /// `op` 2: (key_kind, key, hlc).
    Remove(u8, [u8; 16], u64),
}

/// `Lease` event bodies ([F05 §9.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseEvent {
    /// 1 claim.
    Claim(Box<Claim>),
    /// 2 release with its reason: 1–7 or 9 ([F05 §9.4] field 18).
    Release(u8),
    /// 3 set: mask and the set fields.
    Set {
        /// Mask bits 0–3.
        mask: u8,
        /// Bit 0.
        files_owned: Option<GlobList>,
        /// Bit 1.
        branch: Option<u32>,
        /// Bit 2.
        bound: Option<[u8; 16]>,
        /// Bit 3.
        anchor: Option<Anchor>,
    },
    /// 4 renew.
    Renew(Stamp, u64, Anchor),
}

/// A claim's fields ([F05 §9.4] fields 5–17, 27).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    /// Task `#N`; 0 for a role lease.
    pub node: u32,
    /// 1 task, 2 role.
    pub lkind: u8,
    /// Role symbol, ≠ 0.
    pub role: u16,
    /// Holder actor symbol.
    pub holder: u32,
    /// Holder anchor.
    pub anchor: Anchor,
    /// Deadline.
    pub expires: Stamp,
    /// TTL ms.
    pub ttl_ms: u64,
    /// Run node.
    pub run: u32,
    /// Branch.
    pub branch: u32,
    /// Bound thread hash.
    pub bound: [u8; 16],
    /// Root session hash.
    pub root_session: [u8; 16],
    /// Captured globs.
    pub files_owned: GlobList,
    /// Claiming process.
    pub proc: ProcId,
    /// `lflags`.
    pub lflags: u8,
}

/// `Lease` ([F05 §9.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lease {
    /// `L-<n>`.
    pub lease_id: u64,
    /// Fencing token.
    pub token: u64,
    /// HLC.
    pub hlc: u64,
    /// The event.
    pub event: LeaseEvent,
}

/// A `MarkerEntry` ([F05 §9.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerEntry {
    /// 1 settled … 5 nonlinear.
    pub mkind: u8,
    /// `#N`.
    pub node: u32,
    /// Origin ref.
    pub ref_id: u32,
    /// Origin `ref_seq`.
    pub ref_seq: u32,
    /// Origin commit.
    pub commit: [u8; 16],
    /// Its `seq`.
    pub seq: u64,
    /// HLC.
    pub hlc: u64,
    /// 1–5.
    pub cause: u8,
    /// (holder, status, outcome) for mkind 1.
    pub settled: Option<(u32, u8, u8)>,
    /// Holder set for mkind 1, 2, 4.
    pub holders: Option<Vec<u32>>,
}

/// `Idem` ([F05 §9.6]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Idem {
    /// Key hash.
    pub key: [u8; 16],
    /// Payload hash.
    pub payload: [u8; 16],
    /// Branch.
    pub ref_id: u32,
    /// Bits 0–1.
    pub iflags: u8,
    /// Commit id16.
    pub commit: [u8; 16],
    /// `append_hlc`.
    pub append_hlc: u64,
    /// Stored result.
    pub result: Vec<u8>,
}

/// `GitMap` ([F05 §9.7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitMap {
    /// Destination number.
    pub dest: u8,
    /// Object format.
    pub algo: Algo,
    /// Bit 0 declare, bit 1 last_seen.
    pub gflags: u8,
    /// Greatest mapped seq.
    pub cursor_seq: u64,
    /// Declared name.
    pub dest_name: Option<String>,
    /// (commit id16, git oid).
    pub entries: Vec<([u8; 16], Oid)>,
    /// (ref id, last-seen oid).
    pub seen: Option<Vec<(u32, Oid)>>,
}

/// `Pin` ([F05 §9.8]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    /// 1 pin, 2 unpin.
    pub op: u8,
    /// Holder kind 1–4.
    pub holder: u8,
    /// Holding ref.
    pub ref_id: u32,
    /// Checkpoint set id.
    pub set_lsn: u64,
    /// The set's files.
    pub files: Vec<FileRefV>,
}

/// A per-ref lsn list ([F05 §9.9] `RefList`) with its entries decoded to absolute (lsn, append_hlc).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefList {
    /// The ref.
    pub ref_id: u32,
    /// Raw (dlsn, dhlc) pairs.
    pub deltas: Vec<(u64, u64)>,
}

/// A `Promotion` ([F05 §9.9]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Promotion {
    /// Promoted ref.
    pub ref_id: u32,
    /// K of the new segment.
    pub seg_file: u32,
    /// Its `total_len`.
    pub total_len: u64,
    /// Its digest16.
    pub digest: [u8; 16],
    /// New base pin; 0 = this record.
    pub base_pin: u64,
    /// Tip lsn included.
    pub tip_lsn: u64,
    /// That commit's `ref_seq`.
    pub tip_ref_seq: u32,
}

/// A `Retirement` ([F05 §9.9]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retirement {
    /// Retired extent.
    pub extent: u32,
    /// Its `hist` file.
    pub hist_file: u32,
    /// The hist file's `total_len`.
    pub total_len: u64,
    /// Its digest.
    pub digest: [u8; 16],
}

/// `Checkpoint` ([F05 §9.9]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    /// Bit table.
    pub ckflags: u16,
    /// HLC.
    pub append_hlc: u64,
    /// Allocator after the record.
    pub next_file_no: u32,
    /// Bit 0: (upto_lsn, active_log, segments).
    pub set: Option<(u64, u32, Vec<SegRef>)>,
    /// Bit 1: (window_start, ref lists).
    pub window: Option<(u64, Vec<RefList>)>,
    /// Bit 2.
    pub rt_upto_lsn: Option<u64>,
    /// Bit 3.
    pub promotions: Option<Vec<Promotion>>,
    /// Bit 4.
    pub retirements: Option<Vec<Retirement>>,
    /// Bit 5.
    pub added: Option<Vec<FileEntry>>,
    /// Bit 6.
    pub released: Option<Vec<FileRefV>>,
}

/// A `RefEntry` ([F05 §9.10]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefEntry {
    /// 1 upsert, 2 remove.
    pub op: u8,
    /// The ref.
    pub ref_id: u32,
    /// The upsert fields.
    pub upsert: Option<Box<RefUpsert>>,
}

/// The fields of an upsert `RefEntry` ([F05 §9.10] orders 3–23).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefUpsert {
    /// Ref name symbol.
    pub name: u32,
    /// 1–6.
    pub rkind: u8,
    /// Bits 0–2.
    pub eflags: u8,
    /// Tip.
    pub tip: [u8; 32],
    /// Tip lsn.
    pub tip_lsn: u64,
    /// Base pin.
    pub base_pin: u64,
    /// Fork commit.
    pub fork_commit: [u8; 32],
    /// Fork seq.
    pub fork_seq: u64,
    /// Ops since fork.
    pub ops_since_fork: u64,
    /// Overlay ops.
    pub overlay_ops: u32,
    /// Overlay bytes.
    pub overlay_bytes: u32,
    /// Next ref_seq.
    pub ref_seq_next: u32,
    /// Promoted segment K.
    pub promoted_seg: u32,
    /// Tip generation.
    pub gen_: u32,
    /// Absorbed vector.
    pub absorbed: Vec<(u32, u32)>,
    /// Tag message.
    pub message: Option<String>,
    /// Fork lsn.
    pub fork_lsn: u64,
    /// Fork ref id.
    pub fork_ref_id: u32,
    /// Trunk mark ops.
    pub trunk_mark_ops: u64,
    /// Trunk mark bytes.
    pub trunk_mark_bytes: u64,
}

/// `Lazy` ([F05 §9.11]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lazy {
    /// `sub` 1: (lease_id, token, expires, cause, hlc).
    Heartbeat(u64, u64, Stamp, u8, u64),
    /// `sub` 2.
    Cursor {
        /// Session hash.
        session_hash: [u8; 16],
        /// Agent hash.
        agent_hash: [u8; 16],
        /// 1 or 2.
        feed: u8,
        /// Pack target for feed 2.
        task: Option<u32>,
        /// Position.
        cursor_seq: u64,
        /// HLC.
        hlc: u64,
    },
}

/// A row of a runtime row batch ([F05 §8.5]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowOp {
    /// An image row: op 1 upsert or op 2 delete, and the row.
    Image(u8, Box<Row>),
    /// `FPrint`: (op, oid, fprint for an upsert).
    FPrint(u8, Oid, Option<Vec<u8>>),
    /// `JournalCursor` upsert: the 41-byte row.
    JournalUpsert(Box<Row>),
    /// `JournalCursor` delete: the `vol_key`.
    JournalDelete([u8; 16]),
}

/// `TreeReg` ([F05 §9.23]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeReg {
    /// `sub` 1.
    Register(Box<TreeRegister>),
    /// `sub` 2.
    Epoch([u8; 16], Epoch),
    /// `sub` 3: (tree, count, head, dirty_hlc).
    Dirty([u8; 16], u32, Oid, u64),
    /// `sub` 4.
    Forget([u8; 16]),
}

/// `TreeReg` `sub` 1 fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRegister {
    /// Tree key.
    pub tree: [u8; 16],
    /// OS tag.
    pub os: u8,
    /// `tflags`.
    pub tflags: u8,
    /// Canonical root path.
    pub root: String,
    /// Root directory id.
    pub root_id: OsFileId,
    /// Volume capabilities.
    pub caps: VolumeCaps,
    /// Last git HEAD.
    pub last_head: Oid,
    /// Last settle HLC.
    pub last_settle: u64,
    /// Sensitivity map.
    pub sens: Vec<(u8, String)>,
}

/// A git fact ([F05 §9.25]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fact {
    /// 1: (algo, a, b, answer).
    Ancestry(Algo, Oid, Oid, u8),
    /// 2: (algo, commit, parent, renames).
    Renames(Algo, Oid, Oid, Box<Renames>),
    /// 3: (algo, commit, committer_time, author_time).
    Time(Algo, Oid, i64, i64),
}

/// A `SchemaRes` ([F05 §9.27]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaRes {
    /// 1 kind, 3 enum value, 4 edge kind.
    pub class: u8,
    /// The key's name strings.
    pub key: Vec<String>,
    /// The reserved id.
    pub id: u16,
}

/// `Reserve` ([F05 §9.27]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reserve {
    /// The ref.
    pub ref_id: u32,
    /// HLC.
    pub hlc: u64,
    /// `cs.<n>`.
    pub cs_file: u32,
    /// `blobs.<n>`; 0 none.
    pub blobs_file: u32,
    /// First `#N`.
    pub first_id: u32,
    /// Count.
    pub n_ids: u32,
    /// First `aN`.
    pub first_anchor: u32,
    /// Count.
    pub n_anchors: u32,
    /// Schema reservations.
    pub schema: Vec<SchemaRes>,
}

/// `ExtentHead` ([F05 §9.28]), 98 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtentHead {
    /// The epoch's `epoch_lsn`.
    pub epoch_lsn: u64,
    /// Chain value at the record's lsn.
    pub chain_in: u64,
    /// `HEAD.init`.
    pub init: InitParams,
    /// `HEAD.project_oid_algo`.
    pub project_oid_algo: u8,
    /// Bit 0 quiet, bit 1 readonly.
    pub hflags: u8,
    /// Counters before the record.
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

/// Every payload ([F05 §9]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    /// 1.
    Commit(Box<Commit>),
    /// 2.
    RefUpdate(Box<RefUpdate>),
    /// 3.
    ClientHead(ClientHead),
    /// 4.
    Lease(Box<Lease>),
    /// 5.
    Marker(Vec<MarkerEntry>),
    /// 6.
    Idem(Box<Idem>),
    /// 7.
    GitMap(Box<GitMap>),
    /// 8.
    Pin(Pin),
    /// 9.
    Checkpoint(Box<Checkpoint>),
    /// 10.
    RefTable(Vec<RefEntry>),
    /// 11.
    Lazy(Lazy),
    /// 12: the number of zero bytes.
    Noop(usize),
    /// 13: (dir, committed_lsn, digest, hlc).
    Backup(String, u64, [u8; 32], u64),
    /// 14: (session_hash, agent_hash, rev, hlc, rules).
    SessionMark([u8; 16], [u8; 16], u64, u64, Vec<u32>),
    /// 15.
    FsIntent(Box<FsIntent>),
    /// 16: (intent_lsn, dflags, hlc, outcomes).
    FsIntentDone(u64, u8, u64, Vec<u8>),
    /// 17: (intent_lsn, reason, aflags, hlc).
    FsIntentAborted(u64, u8, u8, u64),
    /// 18, 19, 20, 21, 22, 24, 26: the kind and its rows.
    Rows(u8, Vec<RowOp>),
    /// 23.
    TreeReg(TreeReg),
    /// 25.
    GitFacts(Vec<Fact>),
    /// 27.
    Reserve(Box<Reserve>),
    /// 28.
    ExtentHead(Box<ExtentHead>),
}

/// `FsIntent` ([F05 §9.15]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsIntent {
    /// 1 mv, 2 rm, 3 rm --trash.
    pub op: u8,
    /// Bits 0–1.
    pub iflags: u8,
    /// Caller's branch.
    pub branch: u32,
    /// Writer tree key.
    pub tree: [u8; 16],
    /// Intent anchor, kind 2.
    pub anchor: Anchor,
    /// CLI process.
    pub proc: ProcId,
    /// HLC.
    pub hlc: u64,
    /// Items.
    pub items: Vec<IntentItem>,
}

fn row_batch(r: &mut Reader<'_>, kind: u8) -> Result<Vec<RowOp>> {
    let at = r.offset();
    let n = r.count(1)?;
    if n == 0 {
        return err(at, "row batch n_rows is 0 [F05 §8.5]");
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let b = r.vbytes()?;
        let b_at = r.offset() - b.len();
        let mut rr = Reader::with_base(b, b_at);
        let op_at = rr.offset();
        let op = rr.u8()?;
        if !(1..=2).contains(&op) {
            return err(op_at, "row op is not 1 or 2 [F05 §8.5]");
        }
        let place = if op == 1 {
            Place::Upsert
        } else {
            Place::Delete
        };
        let row = match kind {
            18 | 19 | 22 | 24 | 26 => {
                let t = match kind {
                    18 => Table::FileObs,
                    19 => Table::Pending,
                    22 => Table::DirMap,
                    24 => Table::PrefixEv,
                    _ => Table::AnchorRes,
                };
                RowOp::Image(op, Box::new(runtime::decode_image(t, &mut rr, place)?))
            }
            20 => {
                let o_at = rr.offset();
                let oid = rr.oidv()?;
                if oid == Oid::None {
                    return err(o_at, "FPrint key oid of algorithm none [F05 §9.20]");
                }
                let fp = if op == 1 {
                    Some(rr.vbytes()?.to_vec())
                } else {
                    None
                };
                RowOp::FPrint(op, oid, fp)
            }
            _ => {
                if op == 1 {
                    RowOp::JournalUpsert(Box::new(runtime::decode_image(
                        Table::JournalCur,
                        &mut rr,
                        Place::Upsert,
                    )?))
                } else {
                    RowOp::JournalDelete(rr.b16()?)
                }
            }
        };
        rr.finish("a row of a row batch")?;
        v.push(row);
    }
    Ok(v)
}

fn put_row_batch(rows: &[RowOp], w: &mut Writer) {
    w.uvar(rows.len() as u64);
    for row in rows {
        let mut b = Writer::new();
        match row {
            RowOp::Image(op, r) => {
                b.u8(*op);
                runtime::encode_image(r, &mut b);
            }
            RowOp::FPrint(op, oid, fp) => {
                b.u8(*op);
                b.oidv(oid);
                if let Some(f) = fp {
                    b.vbytes(f);
                }
            }
            RowOp::JournalUpsert(r) => {
                b.u8(1);
                runtime::encode_image(r, &mut b);
            }
            RowOp::JournalDelete(k) => {
                b.u8(2);
                b.bytes(k);
            }
        }
        w.vbytes(b.as_slice());
    }
}

impl Payload {
    /// Decodes the payload of `kind` from exactly its bytes (the `SymDefs` block already consumed).
    pub fn decode(kind: u8, r: &mut Reader<'_>) -> Result<Payload> {
        let p = match kind {
            1 => Payload::Commit(Box::new(Commit::decode(r)?)),
            2 => {
                let reason = byte_range(r, 1, 5, "RefUpdate reason")?;
                let ref_id = r.uvar32()?;
                let actor = r.uvar32()?;
                let hlc = r.u64()?;
                let o_at = r.offset();
                let old = r.b32()?;
                let new = r.b32()?;
                if (reason == 1 && old != [0; 32]) || (reason == 2 && new != [0; 32]) {
                    return err(
                        o_at,
                        "RefUpdate old non-zero on a create, or new non-zero on a delete [F05 §9.2]",
                    );
                }
                let absorbed = if matches!(reason, 1 | 3 | 4) {
                    Some(absorbed(r)?)
                } else {
                    None
                };
                let moves_back = if reason == 3 {
                    let m_at = r.offset();
                    let m = r.uvar32()?;
                    if m == 0 {
                        return err(m_at, "RefUpdate moves_back 0 [F05 §9.2]");
                    }
                    Some(m)
                } else {
                    None
                };
                let restore_seq = if reason == 4 { Some(r.uvar64()?) } else { None };
                Payload::RefUpdate(Box::new(RefUpdate {
                    reason,
                    ref_id,
                    actor,
                    hlc,
                    old,
                    new,
                    absorbed,
                    moves_back,
                    restore_seq,
                }))
            }
            3 => {
                let op = byte_range(r, 1, 2, "ClientHead op")?;
                if op == 1 {
                    Payload::ClientHead(ClientHead::Set(Box::new(runtime::decode_image(
                        Table::Heads,
                        r,
                        Place::Upsert,
                    )?)))
                } else {
                    let kk = byte_range(r, 1, 3, "ClientHead key_kind")?;
                    Payload::ClientHead(ClientHead::Remove(kk, r.b16()?, r.u64()?))
                }
            }
            4 => {
                let event = byte_range(r, 1, 4, "Lease event")?;
                let l_at = r.offset();
                let lease_id = r.uvar64()?;
                if lease_id == 0 {
                    return err(l_at, "Lease lease_id 0 [F05 §9.4]");
                }
                let token = r.u64()?;
                let hlc = r.u64()?;
                let ev = match event {
                    1 => {
                        let n_at = r.offset();
                        let node = r.uvar32()?;
                        let lkind = byte_range(r, 1, 2, "Lease lkind")?;
                        if (lkind == 1) != (node != 0) {
                            return err(
                                n_at,
                                "Lease node is 0 exactly for a role lease [F05 §9.4]",
                            );
                        }
                        let ro_at = r.offset();
                        let role = r.uvar16()?;
                        if role == 0 {
                            return err(ro_at, "Lease role 0 [F05 §9.4]");
                        }
                        let holder = r.uvar32()?;
                        let anchor = anchor_of(r, &[0, 1, 4], "Lease")?;
                        let expires = Stamp::decode(r)?;
                        let ttl_ms = r.uvar64()?;
                        let run = r.uvar32()?;
                        let branch = r.uvar32()?;
                        let bound = r.b16()?;
                        let root_session = r.b16()?;
                        let files_owned = decode_globs(r)?;
                        let proc = ProcId::decode(r)?;
                        let lflags = flag_bits(r, 1, "Lease lflags")?;
                        LeaseEvent::Claim(Box::new(Claim {
                            node,
                            lkind,
                            role,
                            holder,
                            anchor,
                            expires,
                            ttl_ms,
                            run,
                            branch,
                            bound,
                            root_session,
                            files_owned,
                            proc,
                            lflags,
                        }))
                    }
                    2 => {
                        // [F05 §9.4] field 18: reasons 1-7 and 9; 8 is not assigned, so a record with it is malformed
                        // (§5.4).
                        let at = r.offset();
                        let reason = byte_range(r, 1, 9, "Lease release reason")?;
                        if reason == 8 {
                            return err(
                                at,
                                "Lease release reason 8 is not assigned [F05 §9.4 field 18, §5.4]",
                            );
                        }
                        LeaseEvent::Release(reason)
                    }
                    3 => {
                        let m_at = r.offset();
                        let mask = r.u8()?;
                        if mask & 0xF0 != 0 || mask == 0 {
                            return err(m_at, "Lease mask reserved bits set or 0 [F05 §9.4]");
                        }
                        LeaseEvent::Set {
                            mask,
                            files_owned: if mask & 1 != 0 {
                                Some(decode_globs(r)?)
                            } else {
                                None
                            },
                            branch: if mask & 2 != 0 {
                                Some(r.uvar32()?)
                            } else {
                                None
                            },
                            bound: if mask & 4 != 0 { Some(r.b16()?) } else { None },
                            anchor: if mask & 8 != 0 {
                                Some(anchor_of(r, &[0, 1, 4], "Lease set")?)
                            } else {
                                None
                            },
                        }
                    }
                    _ => LeaseEvent::Renew(
                        Stamp::decode(r)?,
                        r.uvar64()?,
                        anchor_of(r, &[0, 1, 4], "Lease renew")?,
                    ),
                };
                Payload::Lease(Box::new(Lease {
                    lease_id,
                    token,
                    hlc,
                    event: ev,
                }))
            }
            5 => {
                let at = r.offset();
                let n = r.count(8)?;
                if n == 0 {
                    return err(at, "Marker n is 0 [F05 §9.5]");
                }
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let mkind = byte_range(r, 1, 5, "MarkerEntry mkind")?;
                    let node = r.uvar32()?;
                    let ref_id = r.uvar32()?;
                    let ref_seq = r.uvar32()?;
                    let commit = r.b16()?;
                    let seq = r.uvar64()?;
                    let hlc = r.u64()?;
                    let cause = byte_range(r, 1, 5, "MarkerEntry cause")?;
                    let settled = if mkind == 1 {
                        let holder = r.uvar32()?;
                        let status = byte_range(r, 1, 2, "MarkerEntry status")?;
                        let outcome = byte_range(r, 0, 3, "MarkerEntry outcome")?;
                        Some((holder, status, outcome))
                    } else {
                        None
                    };
                    let holders = if matches!(mkind, 1 | 2 | 4) {
                        let h_at = r.offset();
                        let c = r.count(1)?;
                        let mut hs: Vec<u32> = Vec::with_capacity(c);
                        for _ in 0..c {
                            hs.push(r.uvar32()?);
                        }
                        if c == 0 || hs.windows(2).any(|w| w[0] >= w[1]) {
                            return err(
                                h_at,
                                "MarkerEntry holders empty or not ascending and unique [F05 §9.5]",
                            );
                        }
                        Some(hs)
                    } else {
                        None
                    };
                    v.push(MarkerEntry {
                        mkind,
                        node,
                        ref_id,
                        ref_seq,
                        commit,
                        seq,
                        hlc,
                        cause,
                        settled,
                        holders,
                    });
                }
                Payload::Marker(v)
            }
            6 => {
                let key = r.b16()?;
                let payload = r.b16()?;
                let ref_id = r.uvar32()?;
                let iflags = flag_bits(r, 3, "Idem iflags")?;
                let c_at = r.offset();
                let commit = r.b16()?;
                if iflags & 2 != 0 && commit != [0; 16] {
                    return err(c_at, "Idem commit non-zero with no_commit [F05 §9.6]");
                }
                Payload::Idem(Box::new(Idem {
                    key,
                    payload,
                    ref_id,
                    iflags,
                    commit,
                    append_hlc: r.u64()?,
                    result: r.vbytes()?.to_vec(),
                }))
            }
            7 => {
                let dest = byte_range(r, 1, 255, "GitMap dest")?;
                let a_at = r.offset();
                let algo = Algo::from_byte(r.u8()?, a_at)?;
                let gflags = flag_bits(r, 3, "GitMap gflags")?;
                let cursor_seq = r.uvar64()?;
                let dest_name = if gflags & 1 != 0 {
                    Some(r.vstr()?.to_owned())
                } else {
                    None
                };
                let n = r.count(16 + algo.digest_len())?;
                let mut entries = Vec::with_capacity(n);
                for _ in 0..n {
                    entries.push((r.b16()?, r.digest(algo)?));
                }
                let seen = if gflags & 2 != 0 {
                    let n = r.count(1 + algo.digest_len())?;
                    let mut s = Vec::with_capacity(n);
                    for _ in 0..n {
                        s.push((r.uvar32()?, r.digest(algo)?));
                    }
                    Some(s)
                } else {
                    None
                };
                Payload::GitMap(Box::new(GitMap {
                    dest,
                    algo,
                    gflags,
                    cursor_seq,
                    dest_name,
                    entries,
                    seen,
                }))
            }
            8 => {
                let op = byte_range(r, 1, 2, "Pin op")?;
                let holder = byte_range(r, 1, 4, "Pin holder")?;
                let ref_id = r.uvar32()?;
                let set_lsn = r.uvar64()?;
                let n = r.count(2)?;
                let mut files = Vec::with_capacity(n);
                for _ in 0..n {
                    files.push(FileRefV::decode(r)?);
                }
                Payload::Pin(Pin {
                    op,
                    holder,
                    ref_id,
                    set_lsn,
                    files,
                })
            }
            9 => Payload::Checkpoint(Box::new(decode_checkpoint(r)?)),
            10 => {
                let at = r.offset();
                let n = r.count(2)?;
                if n == 0 {
                    return err(at, "RefTable n is 0 [F05 §9.10]");
                }
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let op = byte_range(r, 1, 2, "RefEntry op")?;
                    let ref_id = r.uvar32()?;
                    let upsert = if op == 1 {
                        let name = r.uvar32()?;
                        let rkind = byte_range(r, 1, 6, "RefEntry rkind")?;
                        let eflags = flag_bits(r, 7, "RefEntry eflags")?;
                        let t_at = r.offset();
                        let tip = r.b32()?;
                        let tip_lsn = r.uvar64()?;
                        if tip == [0; 32] && tip_lsn != 0 {
                            return err(
                                t_at,
                                "RefEntry tip_lsn non-zero with a zero tip [F05 §9.10]",
                            );
                        }
                        let base_pin = r.uvar64()?;
                        let fork_commit = r.b32()?;
                        let fork_seq = r.uvar64()?;
                        let ops_since_fork = r.uvar64()?;
                        let overlay_ops = r.uvar32()?;
                        let overlay_bytes = r.uvar32()?;
                        let ref_seq_next = r.uvar32()?;
                        let promoted_seg = r.uvar32()?;
                        let gen_ = r.uvar32()?;
                        let absorbed = absorbed(r)?;
                        let message = if eflags & 2 != 0 {
                            Some(r.vstr()?.to_owned())
                        } else {
                            None
                        };
                        let f_at = r.offset();
                        let fork_lsn = r.uvar64()?;
                        let fork_ref_id = r.uvar32()?;
                        if fork_commit == [0; 32] && (fork_lsn != 0 || fork_ref_id != 0) {
                            return err(
                                f_at,
                                "RefEntry fork_lsn or fork_ref_id without a fork [F05 §9.10]",
                            );
                        }
                        Some(Box::new(RefUpsert {
                            name,
                            rkind,
                            eflags,
                            tip,
                            tip_lsn,
                            base_pin,
                            fork_commit,
                            fork_seq,
                            ops_since_fork,
                            overlay_ops,
                            overlay_bytes,
                            ref_seq_next,
                            promoted_seg,
                            gen_,
                            absorbed,
                            message,
                            fork_lsn,
                            fork_ref_id,
                            trunk_mark_ops: r.uvar64()?,
                            trunk_mark_bytes: r.uvar64()?,
                        }))
                    } else {
                        None
                    };
                    v.push(RefEntry { op, ref_id, upsert });
                }
                Payload::RefTable(v)
            }
            11 => {
                let sub = byte_range(r, 1, 2, "Lazy sub")?;
                if sub == 1 {
                    let lease_id = r.uvar64()?;
                    let token = r.u64()?;
                    let expires = Stamp::decode(r)?;
                    let cause = byte_range(r, 1, 3, "Lazy cause")?;
                    Payload::Lazy(Lazy::Heartbeat(lease_id, token, expires, cause, r.u64()?))
                } else {
                    let session_hash = r.b16()?;
                    let agent_hash = r.b16()?;
                    let feed = byte_range(r, 1, 2, "Lazy feed")?;
                    let task = if feed == 2 {
                        let t_at = r.offset();
                        let t = r.uvar32()?;
                        if t == 0 {
                            return err(t_at, "Lazy feed-2 task 0 [F05 §9.11]");
                        }
                        Some(t)
                    } else {
                        None
                    };
                    Payload::Lazy(Lazy::Cursor {
                        session_hash,
                        agent_hash,
                        feed,
                        task,
                        cursor_seq: r.uvar64()?,
                        hlc: r.u64()?,
                    })
                }
            }
            12 => {
                let n = r.remaining();
                r.zeros(n, "Noop payload")?;
                Payload::Noop(n)
            }
            13 => Payload::Backup(r.vstr()?.to_owned(), r.uvar64()?, r.b32()?, r.u64()?),
            14 => {
                let s = r.b16()?;
                let a = r.b16()?;
                let rev = r.uvar64()?;
                let hlc = r.u64()?;
                let n = r.count(1)?;
                let mut rules = Vec::with_capacity(n);
                for _ in 0..n {
                    rules.push(r.uvar32()?);
                }
                Payload::SessionMark(s, a, rev, hlc, rules)
            }
            15 => {
                let op = byte_range(r, 1, 3, "FsIntent op")?;
                let iflags = flag_bits(r, 3, "FsIntent iflags")?;
                let branch = r.uvar32()?;
                let tree = r.b16()?;
                let anchor = anchor_of(r, &[2], "FsIntent")?;
                let proc = ProcId::decode(r)?;
                let hlc = r.u64()?;
                let n_at = r.offset();
                let n = r.count(4)?;
                if n == 0 {
                    return err(n_at, "FsIntent n is 0 [F05 §9.15]");
                }
                let items = runtime::decode_items(r, n, op, false)?;
                Payload::FsIntent(Box::new(FsIntent {
                    op,
                    iflags,
                    branch,
                    tree,
                    anchor,
                    proc,
                    hlc,
                    items,
                }))
            }
            16 => {
                let lsn = r.uvar64()?;
                let dflags = flag_bits(r, 1, "FsIntentDone dflags")?;
                let hlc = r.u64()?;
                let n = r.count(1)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(byte_range(r, 1, 5, "FsIntentDone outcome")?);
                }
                Payload::FsIntentDone(lsn, dflags, hlc, v)
            }
            17 => {
                let lsn = r.uvar64()?;
                let reason = byte_range(r, 1, 5, "FsIntentAborted reason")?;
                let aflags = flag_bits(r, 1, "FsIntentAborted aflags")?;
                Payload::FsIntentAborted(lsn, reason, aflags, r.u64()?)
            }
            18..=22 | 24 | 26 => Payload::Rows(kind, row_batch(r, kind)?),
            23 => {
                let sub = byte_range(r, 1, 4, "TreeReg sub")?;
                let tree = r.b16()?;
                Payload::TreeReg(match sub {
                    1 => {
                        let os = r.u8()?;
                        let tflags = flag_bits(r, 0b1011, "TreeReg tflags")?;
                        let root = r.vstr()?.to_owned();
                        let root_id = OsFileId::decode(r)?;
                        let caps = VolumeCaps::decode(r)?;
                        let last_head = r.oidv()?;
                        let last_settle = r.u64()?;
                        let n = r.count(2)?;
                        let mut sens: Vec<(u8, String)> = Vec::with_capacity(n);
                        for _ in 0..n {
                            let e = flag_bits(r, 3, "TreeReg SensEntry equiv")?;
                            sens.push((e, r.vstr()?.to_owned()));
                        }
                        TreeReg::Register(Box::new(TreeRegister {
                            tree,
                            os,
                            tflags,
                            root,
                            root_id,
                            caps,
                            last_head,
                            last_settle,
                            sens,
                        }))
                    }
                    2 => TreeReg::Epoch(tree, Epoch::decode(r)?),
                    3 => TreeReg::Dirty(tree, r.u32()?, r.oidv()?, r.u64()?),
                    _ => TreeReg::Forget(tree),
                })
            }
            25 => {
                let at = r.offset();
                let n = r.count(1)?;
                if n == 0 {
                    return err(at, "GitFacts n is 0 [F05 §9.25]");
                }
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let b = r.vbytes()?;
                    let mut fr = Reader::with_base(b, r.offset() - b.len());
                    let ftype = byte_range(&mut fr, 1, 3, "Fact ftype")?;
                    let a_at = fr.offset();
                    let algo = Algo::from_byte(fr.u8()?, a_at)?;
                    let a = fr.digest(algo)?;
                    let f = match ftype {
                        1 => {
                            let b2 = fr.digest(algo)?;
                            Fact::Ancestry(algo, a, b2, byte_range(&mut fr, 0, 1, "Fact answer")?)
                        }
                        2 => {
                            let b2 = fr.digest(algo)?;
                            let np = fr.count(2)?;
                            let rn = runtime::decode_renames(&mut fr, algo, np, None)?;
                            Fact::Renames(algo, a, b2, Box::new(rn))
                        }
                        _ => Fact::Time(algo, a, fr.svar64()?, fr.svar64()?),
                    };
                    fr.finish("a Fact")?;
                    v.push(f);
                }
                Payload::GitFacts(v)
            }
            27 => {
                let ref_id = r.uvar32()?;
                let hlc = r.u64()?;
                let c_at = r.offset();
                let cs_file = r.uvar32()?;
                if cs_file == 0 {
                    return err(c_at, "Reserve cs_file 0 [F05 §9.27]");
                }
                let blobs_file = r.uvar32()?;
                let i_at = r.offset();
                let first_id = r.uvar32()?;
                let n_ids = r.uvar32()?;
                let first_anchor = r.uvar32()?;
                let n_anchors = r.uvar32()?;
                if (first_id == 0) != (n_ids == 0) || (first_anchor == 0) != (n_anchors == 0) {
                    return err(
                        i_at,
                        "Reserve first id or anchor is 0 exactly when its count is 0 [F05 §9.27]",
                    );
                }
                if u64::from(first_id) + u64::from(n_ids) > u64::from(u32::MAX) + 1
                    || u64::from(first_anchor) + u64::from(n_anchors) > u64::from(u32::MAX) + 1
                {
                    return err(i_at, "Reserve range beyond 2^32 - 1 [F05 §9.27]");
                }
                let n = r.count(3)?;
                let mut schema = Vec::with_capacity(n);
                for _ in 0..n {
                    let s_at = r.offset();
                    let class = r.u8()?;
                    let keys = match class {
                        1 | 4 => 1,
                        3 => 3,
                        _ => return err(s_at, "SchemaRes class not 1, 3 or 4 [F05 §9.27]"),
                    };
                    let mut key = Vec::with_capacity(keys);
                    for _ in 0..keys {
                        key.push(r.vstr()?.to_owned());
                    }
                    schema.push(SchemaRes {
                        class,
                        key,
                        id: r.uvar16()?,
                    });
                }
                Payload::Reserve(Box::new(Reserve {
                    ref_id,
                    hlc,
                    cs_file,
                    blobs_file,
                    first_id,
                    n_ids,
                    first_anchor,
                    n_anchors,
                    schema,
                }))
            }
            28 => {
                if r.remaining() != 98 {
                    return r.fail("ExtentHead payload is not 98 bytes [F05 §9.28]");
                }
                let epoch_lsn = r.u64()?;
                let chain_in = r.u64()?;
                let init = InitParams::decode(r)?;
                let project_oid_algo = r.u8()?;
                let hflags = flag_bits(r, 3, "ExtentHead hflags")?;
                Payload::ExtentHead(Box::new(ExtentHead {
                    epoch_lsn,
                    chain_in,
                    init,
                    project_oid_algo,
                    hflags,
                    commit_seq: r.u64()?,
                    next_id: r.u32()?,
                    next_anchor: r.u32()?,
                    fence: r.u64()?,
                    next_file_no: r.u32()?,
                    next_ref_id: r.u32()?,
                    hlc_seq: r.u64()?,
                    hlc_commit: r.u64()?,
                }))
            }
            k => return r.fail(format!("record kind {k} is invalid [F05 §7]")),
        };
        r.finish(kind_name(kind))?;
        Ok(p)
    }

    /// The record kind of the payload.
    pub fn kind(&self) -> u8 {
        match self {
            Payload::Commit(_) => 1,
            Payload::RefUpdate(_) => 2,
            Payload::ClientHead(_) => 3,
            Payload::Lease(_) => 4,
            Payload::Marker(_) => 5,
            Payload::Idem(_) => 6,
            Payload::GitMap(_) => 7,
            Payload::Pin(_) => 8,
            Payload::Checkpoint(_) => 9,
            Payload::RefTable(_) => 10,
            Payload::Lazy(_) => 11,
            Payload::Noop(_) => 12,
            Payload::Backup(..) => 13,
            Payload::SessionMark(..) => 14,
            Payload::FsIntent(_) => 15,
            Payload::FsIntentDone(..) => 16,
            Payload::FsIntentAborted(..) => 17,
            Payload::Rows(k, _) => *k,
            Payload::TreeReg(_) => 23,
            Payload::GitFacts(_) => 25,
            Payload::Reserve(_) => 27,
            Payload::ExtentHead(_) => 28,
        }
    }

    /// Encodes the payload.
    pub fn encode(&self, w: &mut Writer) {
        let u = |w: &mut Writer, v: u32| w.uvar(u64::from(v));
        match self {
            Payload::Commit(c) => c.encode(w),
            Payload::RefUpdate(x) => {
                w.u8(x.reason);
                u(w, x.ref_id);
                u(w, x.actor);
                w.u64(x.hlc);
                w.bytes(&x.old);
                w.bytes(&x.new);
                if let Some(a) = &x.absorbed {
                    put_absorbed(a, w);
                }
                if let Some(m) = x.moves_back {
                    u(w, m);
                }
                if let Some(s) = x.restore_seq {
                    w.uvar(s);
                }
            }
            Payload::ClientHead(ClientHead::Set(row)) => {
                w.u8(1);
                runtime::encode_image(row, w);
            }
            Payload::ClientHead(ClientHead::Remove(k, key, hlc)) => {
                w.u8(2);
                w.u8(*k);
                w.bytes(key);
                w.u64(*hlc);
            }
            Payload::Lease(l) => {
                w.u8(match l.event {
                    LeaseEvent::Claim(_) => 1,
                    LeaseEvent::Release(_) => 2,
                    LeaseEvent::Set { .. } => 3,
                    LeaseEvent::Renew(..) => 4,
                });
                w.uvar(l.lease_id);
                w.u64(l.token);
                w.u64(l.hlc);
                match &l.event {
                    LeaseEvent::Claim(c) => {
                        u(w, c.node);
                        w.u8(c.lkind);
                        w.uvar(u64::from(c.role));
                        u(w, c.holder);
                        c.anchor.encode(w);
                        c.expires.encode(w);
                        w.uvar(c.ttl_ms);
                        u(w, c.run);
                        u(w, c.branch);
                        w.bytes(&c.bound);
                        w.bytes(&c.root_session);
                        encode_globs(&c.files_owned, w);
                        c.proc.encode(w);
                        w.u8(c.lflags);
                    }
                    LeaseEvent::Release(reason) => w.u8(*reason),
                    LeaseEvent::Set {
                        mask,
                        files_owned,
                        branch,
                        bound,
                        anchor,
                    } => {
                        w.u8(*mask);
                        if let Some(g) = files_owned {
                            encode_globs(g, w);
                        }
                        if let Some(b) = branch {
                            u(w, *b);
                        }
                        if let Some(b) = bound {
                            w.bytes(b);
                        }
                        if let Some(a) = anchor {
                            a.encode(w);
                        }
                    }
                    LeaseEvent::Renew(s, t, a) => {
                        s.encode(w);
                        w.uvar(*t);
                        a.encode(w);
                    }
                }
            }
            Payload::Marker(v) => {
                w.uvar(v.len() as u64);
                for e in v {
                    w.u8(e.mkind);
                    u(w, e.node);
                    u(w, e.ref_id);
                    u(w, e.ref_seq);
                    w.bytes(&e.commit);
                    w.uvar(e.seq);
                    w.u64(e.hlc);
                    w.u8(e.cause);
                    if let Some((h, s, o)) = e.settled {
                        u(w, h);
                        w.u8(s);
                        w.u8(o);
                    }
                    if let Some(hs) = &e.holders {
                        w.uvar(hs.len() as u64);
                        for h in hs {
                            u(w, *h);
                        }
                    }
                }
            }
            Payload::Idem(i) => {
                w.bytes(&i.key);
                w.bytes(&i.payload);
                u(w, i.ref_id);
                w.u8(i.iflags);
                w.bytes(&i.commit);
                w.u64(i.append_hlc);
                w.vbytes(&i.result);
            }
            Payload::GitMap(g) => {
                w.u8(g.dest);
                w.u8(g.algo.byte());
                w.u8(g.gflags);
                w.uvar(g.cursor_seq);
                if let Some(n) = &g.dest_name {
                    w.vstr(n);
                }
                w.uvar(g.entries.len() as u64);
                for (c, o) in &g.entries {
                    w.bytes(c);
                    w.digest(o);
                }
                if let Some(s) = &g.seen {
                    w.uvar(s.len() as u64);
                    for (r, o) in s {
                        u(w, *r);
                        w.digest(o);
                    }
                }
            }
            Payload::Pin(p) => {
                w.u8(p.op);
                w.u8(p.holder);
                u(w, p.ref_id);
                w.uvar(p.set_lsn);
                w.uvar(p.files.len() as u64);
                for f in &p.files {
                    f.encode(w);
                }
            }
            Payload::Checkpoint(c) => encode_checkpoint(c, w),
            Payload::RefTable(v) => {
                w.uvar(v.len() as u64);
                for e in v {
                    w.u8(e.op);
                    u(w, e.ref_id);
                    if let Some(x) = &e.upsert {
                        u(w, x.name);
                        w.u8(x.rkind);
                        w.u8(x.eflags);
                        w.bytes(&x.tip);
                        w.uvar(x.tip_lsn);
                        w.uvar(x.base_pin);
                        w.bytes(&x.fork_commit);
                        w.uvar(x.fork_seq);
                        w.uvar(x.ops_since_fork);
                        u(w, x.overlay_ops);
                        u(w, x.overlay_bytes);
                        u(w, x.ref_seq_next);
                        u(w, x.promoted_seg);
                        u(w, x.gen_);
                        put_absorbed(&x.absorbed, w);
                        if let Some(m) = &x.message {
                            w.vstr(m);
                        }
                        w.uvar(x.fork_lsn);
                        u(w, x.fork_ref_id);
                        w.uvar(x.trunk_mark_ops);
                        w.uvar(x.trunk_mark_bytes);
                    }
                }
            }
            Payload::Lazy(Lazy::Heartbeat(id, t, s, c, h)) => {
                w.u8(1);
                w.uvar(*id);
                w.u64(*t);
                s.encode(w);
                w.u8(*c);
                w.u64(*h);
            }
            Payload::Lazy(Lazy::Cursor {
                session_hash,
                agent_hash,
                feed,
                task,
                cursor_seq,
                hlc,
            }) => {
                w.u8(2);
                w.bytes(session_hash);
                w.bytes(agent_hash);
                w.u8(*feed);
                if let Some(t) = task {
                    u(w, *t);
                }
                w.uvar(*cursor_seq);
                w.u64(*hlc);
            }
            Payload::Noop(n) => w.zeros(*n),
            Payload::Backup(d, l, dg, h) => {
                w.vstr(d);
                w.uvar(*l);
                w.bytes(dg);
                w.u64(*h);
            }
            Payload::SessionMark(s, a, rev, h, rules) => {
                w.bytes(s);
                w.bytes(a);
                w.uvar(*rev);
                w.u64(*h);
                w.uvar(rules.len() as u64);
                for x in rules {
                    u(w, *x);
                }
            }
            Payload::FsIntent(f) => {
                w.u8(f.op);
                w.u8(f.iflags);
                u(w, f.branch);
                w.bytes(&f.tree);
                f.anchor.encode(w);
                f.proc.encode(w);
                w.u64(f.hlc);
                w.uvar(f.items.len() as u64);
                runtime::encode_items(&f.items, w);
            }
            Payload::FsIntentDone(l, d, h, o) => {
                w.uvar(*l);
                w.u8(*d);
                w.u64(*h);
                w.uvar(o.len() as u64);
                w.bytes(o);
            }
            Payload::FsIntentAborted(l, r, a, h) => {
                w.uvar(*l);
                w.u8(*r);
                w.u8(*a);
                w.u64(*h);
            }
            Payload::Rows(_, rows) => put_row_batch(rows, w),
            Payload::TreeReg(t) => match t {
                TreeReg::Register(x) => {
                    w.u8(1);
                    w.bytes(&x.tree);
                    w.u8(x.os);
                    w.u8(x.tflags);
                    w.vstr(&x.root);
                    x.root_id.encode(w);
                    x.caps.encode(w);
                    w.oidv(&x.last_head);
                    w.u64(x.last_settle);
                    w.uvar(x.sens.len() as u64);
                    for (e, p) in &x.sens {
                        w.u8(*e);
                        w.vstr(p);
                    }
                }
                TreeReg::Epoch(tree, e) => {
                    w.u8(2);
                    w.bytes(tree);
                    e.encode(w);
                }
                TreeReg::Dirty(tree, c, h, d) => {
                    w.u8(3);
                    w.bytes(tree);
                    w.u32(*c);
                    w.oidv(h);
                    w.u64(*d);
                }
                TreeReg::Forget(tree) => {
                    w.u8(4);
                    w.bytes(tree);
                }
            },
            Payload::GitFacts(v) => {
                w.uvar(v.len() as u64);
                for f in v {
                    let mut b = Writer::new();
                    match f {
                        Fact::Ancestry(al, a, b2, ans) => {
                            b.u8(1);
                            b.u8(al.byte());
                            b.digest(a);
                            b.digest(b2);
                            b.u8(*ans);
                        }
                        Fact::Renames(al, a, b2, rn) => {
                            b.u8(2);
                            b.u8(al.byte());
                            b.digest(a);
                            b.digest(b2);
                            b.uvar(rn.pairs.len() as u64);
                            runtime::encode_renames(rn, &mut b, true);
                        }
                        Fact::Time(al, a, c, t) => {
                            b.u8(3);
                            b.u8(al.byte());
                            b.digest(a);
                            b.svar(*c);
                            b.svar(*t);
                        }
                    }
                    w.vbytes(b.as_slice());
                }
            }
            Payload::Reserve(x) => {
                u(w, x.ref_id);
                w.u64(x.hlc);
                u(w, x.cs_file);
                u(w, x.blobs_file);
                u(w, x.first_id);
                u(w, x.n_ids);
                u(w, x.first_anchor);
                u(w, x.n_anchors);
                w.uvar(x.schema.len() as u64);
                for s in &x.schema {
                    w.u8(s.class);
                    for k in &s.key {
                        w.vstr(k);
                    }
                    w.uvar(u64::from(s.id));
                }
            }
            Payload::ExtentHead(h) => {
                w.u64(h.epoch_lsn);
                w.u64(h.chain_in);
                h.init.encode(w);
                w.u8(h.project_oid_algo);
                w.u8(h.hflags);
                w.u64(h.commit_seq);
                w.u32(h.next_id);
                w.u32(h.next_anchor);
                w.u64(h.fence);
                w.u32(h.next_file_no);
                w.u32(h.next_ref_id);
                w.u64(h.hlc_seq);
                w.u64(h.hlc_commit);
            }
        }
    }

    /// The symbol references of the payload as (class, id), for SD-3.
    pub fn symbol_refs(&self) -> Vec<(u8, u32)> {
        let mut v = Vec::new();
        match self {
            Payload::Commit(c) => v = c.symbol_refs(),
            Payload::RefUpdate(x) => v.push((sym::ACTOR, x.actor)),
            Payload::Lease(l) => match &l.event {
                LeaseEvent::Claim(c) => {
                    v.push((sym::ROLE, u32::from(c.role)));
                    v.push((sym::ACTOR, c.holder));
                    v.extend(
                        c.files_owned
                            .iter()
                            .map(|(r, _)| (sym::ROOT, u32::from(*r))),
                    );
                }
                LeaseEvent::Set {
                    files_owned: Some(g),
                    ..
                } => v.extend(g.iter().map(|(r, _)| (sym::ROOT, u32::from(*r)))),
                _ => {}
            },
            Payload::Marker(es) => v.extend(
                es.iter()
                    .filter_map(|e| e.settled.map(|s| (sym::ACTOR, s.0))),
            ),
            Payload::RefTable(es) => v.extend(
                es.iter()
                    .filter_map(|e| e.upsert.as_ref().map(|u| (sym::REF, u.name))),
            ),
            Payload::FsIntent(f) => {
                for it in &f.items {
                    v.push((sym::ROOT, u32::from(it.src.root)));
                    if let Some(d) = &it.dst {
                        v.push((sym::ROOT, u32::from(d.root)));
                    }
                }
            }
            Payload::ClientHead(ClientHead::Set(row)) => row.symbol_refs(&mut v),
            Payload::Rows(_, rows) => {
                for r in rows {
                    if let RowOp::Image(_, row) | RowOp::JournalUpsert(row) = r {
                        row.symbol_refs(&mut v);
                    }
                }
            }
            _ => {}
        }
        v.retain(|&(_, id)| id != 0);
        v
    }
}

fn decode_checkpoint(r: &mut Reader<'_>) -> Result<Checkpoint> {
    let at = r.offset();
    let ckflags = r.u16()?;
    if ckflags & 0xFC00 != 0 || ckflags & 0b110_1001 == 0 {
        return err(
            at,
            "ckflags reserved bits set, or none of bits 0, 3, 5, 6 [F05 §9.9]",
        );
    }
    let b = |i: u16| ckflags & (1 << i) != 0;
    if (b(1) || b(2) || b(4)) && !b(0) {
        return err(at, "ckflags bit 1, 2 or 4 without bit 0 [F05 §9.9]");
    }
    let append_hlc = r.u64()?;
    let next_file_no = r.uvar32()?;
    let set = if b(0) {
        let upto = r.uvar64()?;
        let active_log = r.uvar32()?;
        let n_at = r.offset();
        let n = usize::from(r.u8()?);
        if n > 8 {
            return err(n_at, "Checkpoint n_segments above 8 [F05 §9.9]");
        }
        let mut segs = Vec::with_capacity(n);
        for _ in 0..n {
            segs.push(SegRef::decode(r)?);
        }
        if let Err(m) = check_segment_set(&segs) {
            return err(n_at, format!("Checkpoint segments: {m}"));
        }
        Some((upto, active_log, segs))
    } else {
        None
    };
    let window = if b(1) {
        let w_at = r.offset();
        let ws = r.uvar64()?;
        let upto = set.as_ref().map_or(0, |s| s.0);
        if upto <= ws {
            return err(
                w_at,
                "window set without upto_lsn > window_start [F05 §9.9]",
            );
        }
        let n = r.count(2)?;
        let mut lists: Vec<RefList> = Vec::with_capacity(n);
        for _ in 0..n {
            let l_at = r.offset();
            let ref_id = r.uvar32()?;
            if lists.last().is_some_and(|p| p.ref_id >= ref_id) {
                return err(l_at, "ref lists not sorted by ref id [F05 §9.9]");
            }
            let c = r.count(2)?;
            if c == 0 {
                return err(l_at, "RefList n is 0 [F05 §9.9]");
            }
            let mut deltas = Vec::with_capacity(c);
            let mut lsn = ws;
            for j in 0..c {
                let d_at = r.offset();
                let dl = r.uvar64()?;
                let dh = r.uvar64()?;
                if j > 0 && dl == 0 {
                    return err(d_at, "RefList lsns do not increase strictly [F05 §9.9]");
                }
                lsn = lsn.checked_add(dl).filter(|l| *l < upto).map_or_else(
                    || err(d_at, "RefList lsn outside the window [F05 §9.9]"),
                    Ok,
                )?;
                deltas.push((dl, dh));
            }
            lists.push(RefList { ref_id, deltas });
        }
        Some((ws, lists))
    } else {
        None
    };
    let rt_upto_lsn = if b(2) {
        let r_at = r.offset();
        let v = r.uvar64()?;
        if v <= set.as_ref().map_or(0, |s| s.0) {
            return err(r_at, "rt_upto_lsn not above upto_lsn [F05 §9.9]");
        }
        Some(v)
    } else {
        None
    };
    let promotions = if b(3) {
        let n_at = r.offset();
        let n = r.count(4)?;
        if n == 0 {
            return err(n_at, "n_promotions is 0 [F05 §9.9]");
        }
        let mut v: Vec<Promotion> = Vec::with_capacity(n);
        for _ in 0..n {
            let p_at = r.offset();
            let p = Promotion {
                ref_id: r.uvar32()?,
                seg_file: r.uvar32()?,
                total_len: r.uvar64()?,
                digest: r.b16()?,
                base_pin: r.uvar64()?,
                tip_lsn: r.uvar64()?,
                tip_ref_seq: r.uvar32()?,
            };
            if v.last().is_some_and(|q| q.ref_id >= p.ref_id) || (p.base_pin == 0 && !b(0)) {
                return err(
                    p_at,
                    "promotions unsorted, or base_pin 0 without a set change [F05 §9.9]",
                );
            }
            v.push(p);
        }
        Some(v)
    } else {
        None
    };
    let retirements = if b(4) {
        let n_at = r.offset();
        let n = r.count(4)?;
        if n == 0 {
            return err(n_at, "n_retirements is 0 [F05 §9.9]");
        }
        let mut v: Vec<Retirement> = Vec::with_capacity(n);
        for _ in 0..n {
            let e_at = r.offset();
            let e = Retirement {
                extent: r.uvar32()?,
                hist_file: r.uvar32()?,
                total_len: r.uvar64()?,
                digest: r.b16()?,
            };
            // A run never passes the greatest extent number ([F05 §9.9]): `checked_add` refuses one that would.
            if v.last()
                .is_some_and(|q| q.extent.checked_add(1) != Some(e.extent))
                || e.extent == 0
                || e.hist_file == 0
            {
                return err(
                    e_at,
                    "retirements not increasing by one, or a zero number [F05 §9.9]",
                );
            }
            v.push(e);
        }
        Some(v)
    } else {
        None
    };
    let added = if b(5) {
        let n_at = r.offset();
        let n = r.count(4)?;
        if n == 0 {
            return err(n_at, "n_added is 0 [F05 §9.9]");
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(decode_file_entry(r)?);
        }
        Some(v)
    } else {
        None
    };
    let released = if b(6) {
        let n_at = r.offset();
        let n = r.count(2)?;
        if n == 0 {
            return err(n_at, "n_released is 0 [F05 §9.9]");
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(FileRefV::decode(r)?);
        }
        Some(v)
    } else {
        None
    };
    let c = Checkpoint {
        ckflags,
        append_hlc,
        next_file_no,
        set,
        window,
        rt_upto_lsn,
        promotions,
        retirements,
        added,
        released,
    };
    let max_named = checkpoint_files(&c).into_iter().max().unwrap_or(0);
    if next_file_no <= max_named {
        return err(
            at,
            "Checkpoint next_file_no is not above every number it names [F05 §9.9]",
        );
    }
    Ok(c)
}

/// Every sealed-file number a `Checkpoint` names ([F04 §5.13], [F05 §10.2]).
pub fn checkpoint_files(c: &Checkpoint) -> Vec<u32> {
    let mut v = Vec::new();
    if let Some((_, _, segs)) = &c.set {
        v.extend(segs.iter().map(|s| s.file_no));
    }
    if let Some(p) = &c.promotions {
        v.extend(p.iter().map(|x| x.seg_file));
    }
    if let Some(p) = &c.retirements {
        v.extend(p.iter().map(|x| x.hist_file));
    }
    if let Some(p) = &c.added {
        v.extend(
            p.iter()
                .filter(|e| e.file.family != 1)
                .map(|e| e.file.file_no),
        );
    }
    if let Some(p) = &c.released {
        v.extend(p.iter().filter(|e| e.family != 1).map(|e| e.file_no));
    }
    v
}

fn encode_checkpoint(c: &Checkpoint, w: &mut Writer) {
    let u = |w: &mut Writer, v: u32| w.uvar(u64::from(v));
    w.u16(c.ckflags);
    w.u64(c.append_hlc);
    u(w, c.next_file_no);
    if let Some((upto, al, segs)) = &c.set {
        w.uvar(*upto);
        u(w, *al);
        w.u8(segs.len() as u8);
        for s in segs {
            s.encode(w);
        }
    }
    if let Some((ws, lists)) = &c.window {
        w.uvar(*ws);
        w.uvar(lists.len() as u64);
        for l in lists {
            u(w, l.ref_id);
            w.uvar(l.deltas.len() as u64);
            for (a, b) in &l.deltas {
                w.uvar(*a);
                w.uvar(*b);
            }
        }
    }
    if let Some(v) = c.rt_upto_lsn {
        w.uvar(v);
    }
    if let Some(ps) = &c.promotions {
        w.uvar(ps.len() as u64);
        for p in ps {
            u(w, p.ref_id);
            u(w, p.seg_file);
            w.uvar(p.total_len);
            w.bytes(&p.digest);
            w.uvar(p.base_pin);
            w.uvar(p.tip_lsn);
            u(w, p.tip_ref_seq);
        }
    }
    if let Some(rs) = &c.retirements {
        w.uvar(rs.len() as u64);
        for e in rs {
            u(w, e.extent);
            u(w, e.hist_file);
            w.uvar(e.total_len);
            w.bytes(&e.digest);
        }
    }
    if let Some(a) = &c.added {
        w.uvar(a.len() as u64);
        for e in a {
            encode_file_entry(e, w);
        }
    }
    if let Some(a) = &c.released {
        w.uvar(a.len() as u64);
        for e in a {
            e.encode(w);
        }
    }
}

/// A decoded record ([F05 §3.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The header as read.
    pub hdr: RecHdr,
    /// The `SymDefs` block.
    pub symdefs: Option<Vec<SymDef>>,
    /// The payload.
    pub payload: Payload,
    /// The chain trailer when `group_end` is set.
    pub trailer: Option<u64>,
}

/// The context a record is validated in ([F05 §5.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ctx {
    /// `E`.
    pub e: u64,
    /// `HEAD.epoch` of the slot in use.
    pub epoch: u64,
}

/// [F05 §3.4]: XXH3-64 over bytes `[0, 24)` and `[32, len − t)`.
pub fn record_checksum(rec: &[u8], t: usize) -> u64 {
    let mut h = xxhash_rust::xxh3::Xxh3::new();
    h.update(&rec[..24]);
    h.update(&rec[32..rec.len() - t]);
    h.digest()
}

impl Record {
    /// Validates the record at log position `p` whose bytes begin `b` ([F05 §5.2] checks 2–7), then decodes its
    /// payload (§5.4). `b` is the rest of the extent from `p`.
    pub fn decode_at(
        b: &[u8],
        p: u64,
        ctx: Ctx,
        base: usize,
    ) -> core::result::Result<(Record, usize), RecError> {
        if b.len() < HDR {
            return invalid(
                base,
                "fewer than 32 bytes left in the extent for a RecHdr [F05 §5.2]",
            );
        }
        let h = RecHdr::read(b);
        let o = p & (ctx.e - 1);
        let len = u64::from(h.len);
        if len < 32 || o + len > ctx.e || (h.group_end() && len < 40) || len as usize > b.len() {
            return invalid(base, format!("record length {} breaks §5.2 check 2", h.len));
        }
        let Some(class) = kind_class(h.kind) else {
            return invalid(
                base + 4,
                format!("record kind {} is invalid [F05 §5.2]", h.kind),
            );
        };
        let reserved = u16::from_le_bytes([b[6], b[7]]);
        if h.flags & 0xF8 != 0 || reserved != 0 {
            return invalid(
                base + 5,
                "RecHdr reserved bits or bytes are not zero [F05 §5.2]",
            );
        }
        let lazy_ok = match class {
            Class::Durable => !h.lazy(),
            Class::Lazy => h.lazy(),
            Class::Configurable => true,
        };
        if !lazy_ok || (matches!(h.kind, 12 | 28) && h.symdefs()) {
            return invalid(
                base + 5,
                "RecHdr lazy bit disagrees with the kind, or symdefs on Noop/ExtentHead [F05 §5.2]",
            );
        }
        if h.lsn != p {
            return invalid(
                base + 8,
                format!("RecHdr lsn {} is not its position {p} [F05 §5.2]", h.lsn),
            );
        }
        if h.epoch != ctx.epoch {
            return invalid(base + 16, "RecHdr epoch differs from the slot's [F05 §5.2]");
        }
        let rec = &b[..len as usize];
        let t = if h.group_end() { 8 } else { 0 };
        if record_checksum(rec, t) != h.xxh3_64 {
            return invalid(base + 24, "record checksum mismatch [F05 §5.2]");
        }
        let mut r = Reader::with_base(&rec[HDR..rec.len() - t], base + HDR);
        let symdefs = if h.symdefs() {
            Some(decode_symdefs(&mut r)?)
        } else {
            None
        };
        let payload = Payload::decode(h.kind, &mut r)?;
        let trailer = if h.group_end() {
            Some(u64::from_le_bytes(
                rec[rec.len() - 8..].try_into().expect("8"),
            ))
        } else {
            None
        };
        Ok((
            Record {
                hdr: h,
                symdefs,
                payload,
                trailer,
            },
            len as usize,
        ))
    }

    /// Decodes a record kept outside the log ([F10 §4.1]: a `hist` frame keeps records byte for byte), with every check
    /// of [F05 §5.2] that does not need the extent, the position or the slot: length, kind, header bits, checksum; then
    /// the payload (§5.4). Without `E`, the record must fit an extent of the largest size, 2^30, and end by the end of
    /// the last usable extent at that size, `(2^32 − 1)·2^30` ([F05 §2.2], §2.3).
    pub fn decode_detached(
        b: &[u8],
        base: usize,
    ) -> core::result::Result<(Record, usize), RecError> {
        if b.len() < HDR {
            return invalid(base, "fewer than 32 bytes left for a RecHdr [F05 §3.1]");
        }
        let h = RecHdr::read(b);
        let ctx = Ctx {
            e: 1 << 30,
            epoch: h.epoch,
        };
        let o = h.lsn & (ctx.e - 1);
        if o + u64::from(h.len) > ctx.e {
            return invalid(base, "a record longer than the largest extent [F05 §2.2]");
        }
        if h.lsn
            .checked_add(u64::from(h.len))
            .is_none_or(|end| end > LSN_END_MAX)
        {
            return invalid(
                base,
                "a record past the last usable extent, log.4294967295 [F05 §2.3]",
            );
        }
        Record::decode_at(b, h.lsn, ctx, base)
    }

    /// Re-encodes the record: `len` and `xxh3_64` recomputed, `lsn`, `epoch` and flags kept, the trailer as decoded
    /// (a group re-encode recomputes it, [`encode_group`]).
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Writer::new();
        if let Some(d) = &self.symdefs {
            encode_symdefs(d, &mut body);
        }
        self.payload.encode(&mut body);
        let t = if self.trailer.is_some() { 8 } else { 0 };
        let len = HDR + body.len() + t;
        let mut w = Writer::new();
        w.u32(len as u32);
        w.u8(self.payload.kind());
        w.u8(self.hdr.flags);
        w.u16(0);
        w.u64(self.hdr.lsn);
        w.u64(self.hdr.epoch);
        w.u64(0);
        w.bytes(body.as_slice());
        if let Some(tr) = self.trailer {
            w.u64(tr);
        }
        let sum = record_checksum(w.as_slice(), t);
        w.as_mut_slice()[24..32].copy_from_slice(&sum.to_le_bytes());
        w.into_vec()
    }
}

/// [F05 §4.3]: the chain value at `epoch_lsn` is XXH3-64 of the epoch's 8 little-endian bytes.
pub fn epoch_seed(epoch: u64) -> u64 {
    xxh3_64(&epoch.to_le_bytes())
}

/// A valid group ([F05 §4.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Its boundary p.
    pub lsn: u64,
    /// Its records.
    pub records: Vec<Record>,
    /// Its length.
    pub len: u64,
}

/// Decodes one group at boundary `p` from `b` (the rest of its extent), with the chain value `seed` at `p`
/// ([F05 §4.2], §4.6).
pub fn decode_group(
    b: &[u8],
    p: u64,
    seed: u64,
    ctx: Ctx,
    base: usize,
) -> core::result::Result<Group, RecError> {
    let mut off = 0usize;
    let mut records = Vec::new();
    loop {
        if off >= b.len() {
            return invalid(
                base + off,
                "the extent ends before a record carrying group_end [F05 §5.3]",
            );
        }
        let (rec, n) = Record::decode_at(&b[off..], p + off as u64, ctx, base + off)?;
        off += n;
        let end = rec.hdr.group_end();
        records.push(rec);
        if end {
            break;
        }
    }
    let want = xxh3_64_seeded(&b[..off - 8], seed);
    let got = u64::from_le_bytes(b[off - 8..off].try_into().expect("8"));
    if want != got {
        return invalid(base + off - 8, "chain trailer mismatch [F05 §4.2]");
    }
    Ok(Group {
        lsn: p,
        records,
        len: off as u64,
    })
}

/// Re-encodes a group with the chain value `seed` at its boundary, recomputing every checksum and the trailer.
pub fn encode_group(g: &Group, seed: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, rec) in g.records.iter().enumerate() {
        let mut b = rec.encode();
        if i + 1 == g.records.len() {
            let n = b.len();
            let sum = xxh3_64_seeded(&[out.as_slice(), &b[..n - 8]].concat(), seed);
            b[n - 8..].copy_from_slice(&sum.to_le_bytes());
        }
        out.extend_from_slice(&b);
    }
    out
}

/// Why a scan stopped ([F05 §5.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The end of the valid log at or above `durable_lsn`.
    End(Error),
    /// Corruption: an invalid group below `durable_lsn`, or anywhere a malformed payload or one of the two placement
    /// defects of a valid group ([F05 §5.4]).
    Corrupt(Error),
    /// The extents ran out exactly at a boundary with nothing more to read.
    Clean,
}

/// The result of a scan ([F05 §5.5]).
#[derive(Clone, Debug)]
pub struct Scan {
    /// The valid groups.
    pub groups: Vec<Group>,
    /// End of the valid log.
    pub end: u64,
    /// The chain value there.
    pub chain: u64,
    /// Why the scan stopped.
    pub stop: Stop,
}

/// The inputs of a scan ([F05 §5.1]).
#[derive(Clone, Debug)]
pub struct ScanCtx {
    /// `E` and the slot's epoch.
    pub ctx: Ctx,
    /// `HEAD.epoch_lsn`.
    pub epoch_lsn: u64,
    /// `HEAD.durable_lsn`.
    pub durable_lsn: u64,
    /// `HEAD.init`, compared with every `ExtentHead`.
    pub init: InitParams,
    /// `HEAD.project_oid_algo`.
    pub project_oid_algo: u8,
}

/// Scans from boundary `start` with chain value `chain` over the extents `ext(n)` (the bytes of `log.<n>`, or `None`
/// when absent), applying §4.5, §4.6, §5.2–§5.4 and SD-1–SD-3 with `symbols`. Error offsets are within the extent
/// file the failure lies in.
///
/// An extent file shorter than `E` where the valid log reaches its first byte ends the log as a missing extent does:
/// at or above `durable_lsn` it is the leftover of an interrupted preparation that the next rotation re-prepares
/// ([F05 §2.2] with \[F16\] P-8), below it corruption. Any other extent of another length is corrupt (§2.2).
///
/// The two placement defects a reader checks (§4.7) are corrupt wherever they lie, above `durable_lsn` included (§5.4,
/// second paragraph): a boundary that leaves 1–39 bytes in its extent, which a valid group ending there breaks G-4 with
/// (§4.4: no group fits there), and a valid group at an extent's first byte that is not its one `ExtentHead` record
/// (§4.5). Only a defective writer produces a checksummed, chained group like that, so the scan stops with
/// [`Stop::Corrupt`] instead of classifying the position by `durable_lsn` as §5.3 does an invalid group.
pub fn scan<'a>(
    ext: impl Fn(u32) -> Option<&'a [u8]>,
    sc: &ScanCtx,
    start: u64,
    mut chain: u64,
    symbols: &mut Symbols,
) -> Scan {
    let e = sc.ctx.e;
    let mut p = start;
    let mut groups = Vec::new();
    let stop = loop {
        let n = u32::try_from((p / e) + 1).unwrap_or(u32::MAX);
        let o = (p % e) as usize;
        let Some(file) = ext(n) else {
            break if o == 0 && p >= sc.durable_lsn {
                Stop::Clean
            } else {
                classify(
                    p,
                    sc,
                    Error {
                        offset: 0,
                        reason: format!("log.{n} is missing [F05 §5.2 check 1]"),
                        rule: None,
                    },
                )
            };
        };
        if o == 0 && (file.len() as u64) < e {
            // [F05 §2.2] read with [F16] P-8: the valid log ends at this extent's first byte, and a file shorter than E
            // here is the leftover of a preparation that a death, crash or DiskFull interrupted ([F15] FM-5.4), which
            // the next rotation re-prepares in place; below durable_lsn it is corruption, like a missing extent.
            break classify(
                p,
                sc,
                Error {
                    offset: 0,
                    reason: format!(
                        "log.{n} is {} bytes, shorter than E = {e}, where the valid log reaches its first byte: an interrupted preparation [F05 §2.2, F16 P-8]",
                        file.len()
                    ),
                    rule: None,
                },
            );
        }
        if file.len() as u64 != e {
            break Stop::Corrupt(Error {
                offset: 0,
                reason: format!("log.{n} is {} bytes, not E = {e} [F05 §2.2]", file.len()),
                rule: None,
            });
        }
        if o != 0 && e - (o as u64) < MIN_GROUP {
            break Stop::Corrupt(Error {
                offset: o,
                reason: "a group boundary leaves 1-39 bytes in its extent, where no group fits: corrupt wherever it lies [F05 §4.4 G-4, §4.7, §5.4]"
                    .into(),
                rule: None,
            });
        }
        let seed = if p == sc.epoch_lsn {
            epoch_seed(sc.ctx.epoch)
        } else {
            chain
        };
        match decode_group(&file[o..], p, seed, sc.ctx, o) {
            Ok(g) => {
                let is_head = g.records.len() == 1 && g.records[0].hdr.kind == 28;
                if o == 0 && !is_head {
                    break Stop::Corrupt(Error {
                        offset: 0,
                        reason: format!(
                            "the first group of log.{n} is not one ExtentHead record: corrupt wherever it lies [F05 §4.5, §4.7, §5.4]"
                        ),
                        rule: None,
                    });
                }
                if let Err(m) = check_group_rules(&g, sc, seed, symbols) {
                    break Stop::Corrupt(m);
                }
                let end = o + g.len as usize;
                chain = u64::from_le_bytes(file[end - 8..end].try_into().expect("8"));
                p += g.len;
                groups.push(g);
            }
            Err(RecError::Malformed(m)) => break Stop::Corrupt(m),
            Err(RecError::Invalid(m)) => break classify(p, sc, m),
        }
    };
    Scan {
        groups,
        end: p,
        chain,
        stop,
    }
}

fn classify(p: u64, sc: &ScanCtx, m: Error) -> Stop {
    if p >= sc.durable_lsn {
        Stop::End(m)
    } else {
        Stop::Corrupt(m)
    }
}

/// [F05 §9.28] and §8.1 rules over a valid group; errors name the record's offset in its extent.
fn check_group_rules(
    g: &Group,
    sc: &ScanCtx,
    seed: u64,
    symbols: &mut Symbols,
) -> core::result::Result<(), Error> {
    let e = sc.ctx.e;
    for rec in &g.records {
        let at = (rec.hdr.lsn % e) as usize;
        if let Payload::ExtentHead(h) = &rec.payload
            && (rec.hdr.lsn % e != 0
                || h.chain_in != seed
                || h.epoch_lsn != sc.epoch_lsn
                || h.init != sc.init
                || h.project_oid_algo != sc.project_oid_algo
                || rec.hdr.len as u64 != H)
        {
            return err(
                at,
                format!("ExtentHead at lsn {} breaks §9.28", rec.hdr.lsn),
            );
        }
        if let Some(d) = &rec.symdefs {
            symbols.define(d, at + HDR)?;
        }
        symbols.check_refs(&rec.payload.symbol_refs(), at + HDR)?;
        if let Payload::Commit(c) = &rec.payload {
            for op in &c.ops {
                op.check_schema_key(|id| symbols.text(sym::NAME, id), at + HDR)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::head::InitParams;

    pub(crate) const E: u64 = 1 << 16;
    pub(crate) const EPOCH: u64 = 0x1234_5678_9ABC_DEF1;

    pub(crate) fn init() -> InitParams {
        InitParams {
            log_extent_bytes: E,
            hist_frame_commits: 4,
            hist_frame_bytes: 4096,
            store_id: [0xA5; 16],
        }
    }

    pub(crate) fn rec(
        kind: u8,
        flags: u8,
        lsn: u64,
        payload: Payload,
        symdefs: Option<Vec<SymDef>>,
    ) -> Record {
        Record {
            hdr: RecHdr {
                len: 0,
                kind,
                flags: flags | if symdefs.is_some() { 4 } else { 0 },
                lsn,
                epoch: EPOCH,
                xxh3_64: 0,
            },
            symdefs,
            payload,
            trailer: if flags & 2 != 0 { Some(0) } else { None },
        }
    }

    pub(crate) fn head(lsn: u64, chain_in: u64) -> Record {
        rec(
            28,
            2,
            lsn,
            Payload::ExtentHead(Box::new(ExtentHead {
                epoch_lsn: 0,
                chain_in,
                init: init(),
                project_oid_algo: 1,
                hflags: 0,
                commit_seq: 0,
                next_id: 1,
                next_anchor: 1,
                fence: 0,
                next_file_no: 1,
                next_ref_id: 0,
                hlc_seq: 0,
                hlc_commit: 0,
            })),
            None,
        )
    }

    fn one(g: Vec<Record>, lsn: u64) -> Group {
        Group {
            lsn,
            records: g,
            len: 0,
        }
    }

    /// [F05 §4.5], §9.28: the epoch-start group is exactly H = 138 bytes; its trailer seeds with XXH3-64(epoch).
    #[test]
    fn epoch_start_group() {
        let g = one(vec![head(0, epoch_seed(EPOCH))], 0);
        let b = encode_group(&g, epoch_seed(EPOCH));
        assert_eq!(b.len() as u64, H);
        let ctx = Ctx { e: E, epoch: EPOCH };
        let back = decode_group(&b, 0, epoch_seed(EPOCH), ctx, 0).unwrap();
        assert_eq!(back.len, H);
        assert_eq!(encode_group(&back, epoch_seed(EPOCH)), b);
        let mut bad = b.clone();
        bad[137] ^= 1;
        assert!(matches!(
            decode_group(&bad, 0, epoch_seed(EPOCH), ctx, 0),
            Err(RecError::Invalid(_))
        ));
    }

    /// [F05 §5.2]: position, epoch, kind and lazy-bit rules make a record invalid.
    #[test]
    fn record_validity() {
        let ctx = Ctx { e: E, epoch: EPOCH };
        let r = rec(12, 3, 200, Payload::Noop(8), None);
        let b = r.encode();
        assert_eq!(b.len(), 48);
        assert!(Record::decode_at(&b, 200, ctx, 0).is_ok());
        assert!(
            matches!(
                Record::decode_at(&b, 208, ctx, 0),
                Err(RecError::Invalid(_))
            ),
            "lsn != position"
        );
        let other = Ctx { e: E, epoch: 7 };
        assert!(
            matches!(
                Record::decode_at(&b, 200, other, 0),
                Err(RecError::Invalid(_))
            ),
            "epoch"
        );
        let durable_noop = rec(12, 2, 200, Payload::Noop(0), None).encode();
        assert!(
            matches!(
                Record::decode_at(&durable_noop, 200, ctx, 0),
                Err(RecError::Invalid(_))
            ),
            "lazy bit"
        );
        let mut nonzero = b.clone();
        nonzero[35] = 1;
        let t = 8;
        let sum = record_checksum(&nonzero, t);
        nonzero[24..32].copy_from_slice(&sum.to_le_bytes());
        assert!(
            matches!(
                Record::decode_at(&nonzero, 200, ctx, 0),
                Err(RecError::Malformed(_))
            ),
            "Noop payload"
        );
    }

    /// A small log: extent head, a ref group with a symbol definition, then the end of the log; a scan yields both
    /// groups and stops at the first zero byte ([F05 §5.3] end of log above `durable_lsn`).
    #[test]
    fn scan_small_log() {
        let mut log = vec![0u8; E as usize];
        let g0 = one(vec![head(0, epoch_seed(EPOCH))], 0);
        let b0 = encode_group(&g0, epoch_seed(EPOCH));
        log[..b0.len()].copy_from_slice(&b0);
        let seed1 = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
        let ru = rec(
            2,
            0,
            138,
            Payload::RefUpdate(Box::new(RefUpdate {
                reason: 1,
                ref_id: 0,
                actor: 1,
                hlc: 5 << 16,
                old: [0; 32],
                new: [0; 32],
                absorbed: Some(vec![]),
                moves_back: None,
                restore_seq: None,
            })),
            Some(vec![SymDef {
                class: sym::ACTOR,
                id: 1,
                text: "owner".into(),
            }]),
        );
        let ru_len = ru.encode().len() as u64;
        let rt = rec(
            10,
            2,
            138 + ru_len,
            Payload::RefTable(vec![RefEntry {
                op: 1,
                ref_id: 0,
                upsert: Some(Box::new(RefUpsert {
                    name: 1,
                    rkind: 1,
                    eflags: 0,
                    tip: [0; 32],
                    tip_lsn: 0,
                    base_pin: 0,
                    fork_commit: [0; 32],
                    fork_seq: 0,
                    ops_since_fork: 0,
                    overlay_ops: 0,
                    overlay_bytes: 0,
                    ref_seq_next: 1,
                    promoted_seg: 0,
                    gen_: 0,
                    absorbed: vec![],
                    message: None,
                    fork_lsn: 0,
                    fork_ref_id: 0,
                    trunk_mark_ops: 0,
                    trunk_mark_bytes: 0,
                })),
            }]),
            Some(vec![SymDef {
                class: sym::REF,
                id: 1,
                text: "main".into(),
            }]),
        );
        let g1 = one(vec![ru, rt], 138);
        let b1 = encode_group(&g1, seed1);
        log[138..138 + b1.len()].copy_from_slice(&b1);
        let sc = ScanCtx {
            ctx: Ctx { e: E, epoch: EPOCH },
            epoch_lsn: 0,
            durable_lsn: 138 + b1.len() as u64,
            init: init(),
            project_oid_algo: 1,
        };
        let mut syms = Symbols::from_symtab(&[]);
        let s = scan(|n| (n == 1).then_some(log.as_slice()), &sc, 0, 0, &mut syms);
        assert_eq!(s.groups.len(), 2, "{:?}", s.stop);
        assert_eq!(s.end, 138 + b1.len() as u64);
        assert!(matches!(s.stop, Stop::End(_)));
        assert_eq!(encode_group(&s.groups[1], seed1), b1);
        let mut syms = Symbols::from_symtab(&[]);
        let mut sc2 = sc.clone();
        sc2.durable_lsn = 1 << 15;
        let s2 = scan(
            |n| (n == 1).then_some(log.as_slice()),
            &sc2,
            0,
            0,
            &mut syms,
        );
        assert!(
            matches!(s2.stop, Stop::Corrupt(_)),
            "an invalid group below durable_lsn is corruption"
        );
    }

    /// A log of the extent head and one group holding a `Commit` whose `SetField` writes `Value::Sym(text)`; the record
    /// defines ref 1, actor 1, name 1 and text 1 itself. Returns the extent and the commit group's end.
    fn log_with_sym_value(text: u32) -> (Vec<u8>, u64) {
        use crate::commit::{Op, tests::base_commit};
        use crate::value::Value;
        let mut log = vec![0u8; E as usize];
        let b0 = encode_group(&one(vec![head(0, epoch_seed(EPOCH))], 0), epoch_seed(EPOCH));
        log[..b0.len()].copy_from_slice(&b0);
        let seed = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
        let mut c = base_commit();
        c.presence = 1 << 12;
        c.ops = vec![Op::SetField {
            id: 1,
            prev: 0,
            name: 1,
            old: Value::Absent,
            new: Value::Sym(text),
        }];
        let def = |class, text: &str| SymDef {
            class,
            id: 1,
            text: text.into(),
        };
        let defs = vec![
            def(sym::ACTOR, "owner"),
            def(sym::REF, "main"),
            def(sym::NAME, "tag"),
            def(sym::TEXT, "blue"),
        ];
        let g = one(
            vec![rec(1, 2, H, Payload::Commit(Box::new(c)), Some(defs))],
            H,
        );
        let b = encode_group(&g, seed);
        log[H as usize..H as usize + b.len()].copy_from_slice(&b);
        (log, H + b.len() as u64)
    }

    /// [F05 §8.1] SD-3 over stored values: a `sym` value no record defines makes its `Commit` malformed, which is
    /// corruption wherever it lies (§5.4), here at `durable_lsn`; the same record with a defined `sym` is adopted.
    #[test]
    fn undefined_symbol_in_a_value_is_corrupt() {
        let sc = ScanCtx {
            ctx: Ctx { e: E, epoch: EPOCH },
            epoch_lsn: 0,
            durable_lsn: H,
            init: init(),
            project_oid_algo: 1,
        };
        let (good, end) = log_with_sym_value(1);
        let mut syms = Symbols::from_symtab(&[]);
        let s = scan(
            |n| (n == 1).then_some(good.as_slice()),
            &sc,
            0,
            0,
            &mut syms,
        );
        assert_eq!((s.groups.len(), s.end), (2, end), "{:?}", s.stop);
        let (bad, _) = log_with_sym_value(2);
        let mut syms = Symbols::from_symtab(&[]);
        let s = scan(|n| (n == 1).then_some(bad.as_slice()), &sc, 0, 0, &mut syms);
        match &s.stop {
            Stop::Corrupt(e) => assert!(e.reason.contains("SD-3"), "{e}"),
            other => panic!("an undefined sym value ended the scan as {other:?}"),
        }
        assert_eq!(s.end, H);
    }

    /// [F05 §4.4] G-3: a pad group fills the extent exactly; the next extent begins with its head carrying the pad's
    /// trailer as `chain_in` (§2.7).
    #[test]
    fn rotation_pad() {
        let mut ext1 = vec![0u8; E as usize];
        let b0 = encode_group(&one(vec![head(0, epoch_seed(EPOCH))], 0), epoch_seed(EPOCH));
        ext1[..b0.len()].copy_from_slice(&b0);
        let seed = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
        let r = E - H;
        let pad = one(
            vec![rec(12, 3, H, Payload::Noop((r - 40) as usize), None)],
            H,
        );
        let bp = encode_group(&pad, seed);
        assert_eq!(bp.len() as u64, r);
        ext1[H as usize..].copy_from_slice(&bp);
        let chain2 = u64::from_le_bytes(bp[bp.len() - 8..].try_into().unwrap());
        let mut ext2 = vec![0u8; E as usize];
        let bh = encode_group(&one(vec![head(E, chain2)], E), chain2);
        ext2[..bh.len()].copy_from_slice(&bh);
        let sc = ScanCtx {
            ctx: Ctx { e: E, epoch: EPOCH },
            epoch_lsn: 0,
            durable_lsn: E + H,
            init: init(),
            project_oid_algo: 1,
        };
        let mut syms = Symbols::default();
        let s = scan(
            |n| match n {
                1 => Some(ext1.as_slice()),
                2 => Some(ext2.as_slice()),
                _ => None,
            },
            &sc,
            0,
            0,
            &mut syms,
        );
        assert_eq!(s.groups.len(), 3, "{:?}", s.stop);
        assert_eq!(s.end, E + H);
    }

    /// [F05 §2.2] with \[F16\] P-8 and \[F15\] FM-5.4: when the valid log ends exactly at the end of extent 1 (a pad group
    /// fills it, §4.4 G-3), a `log.2` shorter than E is an interrupted preparation: the end of the valid log at and above
    /// `durable_lsn` (as a missing `log.2` is), corruption below it. A `log.2` longer than E is corrupt wherever it lies.
    #[test]
    fn short_extent_at_the_end_of_the_valid_log() {
        let mut ext1 = vec![0u8; E as usize];
        let b0 = encode_group(&one(vec![head(0, epoch_seed(EPOCH))], 0), epoch_seed(EPOCH));
        ext1[..b0.len()].copy_from_slice(&b0);
        let seed = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
        let pad = one(
            vec![rec(12, 3, H, Payload::Noop((E - H - 40) as usize), None)],
            H,
        );
        ext1[H as usize..].copy_from_slice(&encode_group(&pad, seed));
        let scan_with = |ext2: Option<&[u8]>, durable: u64| {
            let sc = ScanCtx {
                ctx: Ctx { e: E, epoch: EPOCH },
                epoch_lsn: 0,
                durable_lsn: durable,
                init: init(),
                project_oid_algo: 1,
            };
            let mut syms = Symbols::default();
            scan(
                |n| match n {
                    1 => Some(ext1.as_slice()),
                    2 => ext2,
                    _ => None,
                },
                &sc,
                0,
                0,
                &mut syms,
            )
        };
        for short in [&[][..], &[0u8; 4096][..]] {
            for durable in [H, E] {
                let s = scan_with(Some(short), durable);
                assert!(
                    matches!(&s.stop, Stop::End(e) if e.reason.contains("P-8")) && s.end == E,
                    "{} bytes, durable_lsn {durable}: {:?}",
                    short.len(),
                    s.stop
                );
            }
            let s = scan_with(Some(short), E + H);
            assert!(
                matches!(s.stop, Stop::Corrupt(_)) && s.end == E,
                "{} bytes below durable_lsn: {:?}",
                short.len(),
                s.stop
            );
        }
        let long = vec![0u8; E as usize + 8];
        for durable in [H, E, E + H] {
            let s = scan_with(Some(&long), durable);
            assert!(
                matches!(&s.stop, Stop::Corrupt(e) if e.reason.contains("not E")),
                "durable_lsn {durable}: {:?}",
                s.stop
            );
        }
        assert!(matches!(scan_with(None, E).stop, Stop::Clean));
        assert!(matches!(scan_with(None, E + H).stop, Stop::Corrupt(_)));
    }

    /// [F05 §5.4] second paragraph, §4.7 (R51): the two placement defects of a valid group — a group that leaves 20
    /// bytes in its extent (§4.4 G-4), and an extent whose first group is a `Noop` group (§4.5) — are corruption at, above
    /// and below `durable_lsn` alike.
    #[test]
    fn misplaced_groups_are_corrupt_wherever_they_lie() {
        let scan_at = |exts: &[Vec<u8>], durable: u64| {
            let sc = ScanCtx {
                ctx: Ctx { e: E, epoch: EPOCH },
                epoch_lsn: 0,
                durable_lsn: durable,
                init: init(),
                project_oid_algo: 1,
            };
            let mut syms = Symbols::default();
            scan(
                |n| exts.get(n as usize - 1).map(Vec::as_slice),
                &sc,
                0,
                0,
                &mut syms,
            )
        };
        let b0 = encode_group(&one(vec![head(0, epoch_seed(EPOCH))], 0), epoch_seed(EPOCH));
        let seed = u64::from_le_bytes(b0[b0.len() - 8..].try_into().unwrap());
        // G-4: one lazy Noop group that stops 20 bytes before the end of the extent.
        let mut short = vec![0u8; E as usize];
        short[..b0.len()].copy_from_slice(&b0);
        let len = E - H - 20;
        let g = encode_group(
            &one(
                vec![rec(12, 3, H, Payload::Noop((len - 40) as usize), None)],
                H,
            ),
            seed,
        );
        short[H as usize..(H + len) as usize].copy_from_slice(&g);
        for durable in [H, E - 20, E] {
            let s = scan_at(std::slice::from_ref(&short), durable);
            assert!(
                matches!(&s.stop, Stop::Corrupt(e) if e.reason.contains("§5.4")) && s.end == E - 20,
                "durable_lsn {durable}: {:?}",
                s.stop
            );
        }
        // §4.5: extent 2 opens with a validly chained Noop group instead of its ExtentHead.
        let mut ext1 = vec![0u8; E as usize];
        ext1[..b0.len()].copy_from_slice(&b0);
        let pad = encode_group(
            &one(
                vec![rec(12, 3, H, Payload::Noop((E - H - 40) as usize), None)],
                H,
            ),
            seed,
        );
        ext1[H as usize..].copy_from_slice(&pad);
        let chain2 = u64::from_le_bytes(pad[pad.len() - 8..].try_into().unwrap());
        let mut ext2 = vec![0u8; E as usize];
        let noop = encode_group(&one(vec![rec(12, 3, E, Payload::Noop(8), None)], E), chain2);
        ext2[..noop.len()].copy_from_slice(&noop);
        let exts = [ext1, ext2];
        for durable in [H, E, E + 100] {
            let s = scan_at(&exts, durable);
            assert!(
                matches!(&s.stop, Stop::Corrupt(e) if e.reason.contains("§5.4")) && s.end == E,
                "durable_lsn {durable}: {:?}",
                s.stop
            );
        }
    }

    /// [F05 §9.4] field 18: a release carries reason 1–7 or 9; reason 8 is not assigned and makes the record malformed
    /// (§5.4), as 0 and 10 are outside the enumeration.
    #[test]
    fn lease_release_reasons() {
        let bytes = |reason: u8| {
            let mut w = Writer::new();
            Payload::Lease(Box::new(Lease {
                lease_id: 7,
                token: 7,
                hlc: 2,
                event: LeaseEvent::Release(reason),
            }))
            .encode(&mut w);
            w.into_vec()
        };
        for reason in (1..=7).chain([9]) {
            payload_rt(
                4,
                Payload::Lease(Box::new(Lease {
                    lease_id: 7,
                    token: 7,
                    hlc: 2,
                    event: LeaseEvent::Release(reason),
                })),
            );
        }
        for reason in [0, 8, 10] {
            let e = Payload::decode(4, &mut Reader::new(&bytes(reason))).unwrap_err();
            assert!(e.reason.contains("reason"), "{reason}: {e}");
        }
    }

    fn payload_rt(kind: u8, p: Payload) {
        let mut w = Writer::new();
        p.encode(&mut w);
        let mut r = Reader::new(w.as_slice());
        let back =
            Payload::decode(kind, &mut r).unwrap_or_else(|e| panic!("{}: {e}", kind_name(kind)));
        assert_eq!(back, p, "{}", kind_name(kind));
    }

    /// Every payload kind other than `Commit` and the row batches round-trips through its sequence table ([F05 §9]).
    #[test]
    fn payload_round_trips() {
        let anchor0 = Anchor::default();
        let proc = ProcId::default();
        payload_rt(
            4,
            Payload::Lease(Box::new(Lease {
                lease_id: 7,
                token: 7,
                hlc: 1,
                event: LeaseEvent::Claim(Box::new(Claim {
                    node: 12,
                    lkind: 1,
                    role: 2,
                    holder: 3,
                    anchor: anchor0,
                    expires: Stamp {
                        wall: 10,
                        boot_hash: 1,
                        mono: 2,
                    },
                    ttl_ms: 900_000,
                    run: 0,
                    branch: 0,
                    bound: [0; 16],
                    root_session: [0; 16],
                    files_owned: vec![(1, "crates/**".into())],
                    proc,
                    lflags: 0,
                })),
            })),
        );
        payload_rt(
            4,
            Payload::Lease(Box::new(Lease {
                lease_id: 7,
                token: 7,
                hlc: 2,
                event: LeaseEvent::Set {
                    mask: 0b1010,
                    files_owned: None,
                    branch: Some(4),
                    bound: None,
                    anchor: Some(anchor0),
                },
            })),
        );
        payload_rt(
            5,
            Payload::Marker(vec![MarkerEntry {
                mkind: 1,
                node: 40,
                ref_id: 3,
                ref_seq: 17,
                commit: [1; 16],
                seq: 4470,
                hlc: 9,
                cause: 1,
                settled: Some((0, 1, 0)),
                holders: Some(vec![3]),
            }]),
        );
        payload_rt(
            6,
            Payload::Idem(Box::new(Idem {
                key: [1; 16],
                payload: [2; 16],
                ref_id: 0,
                iflags: 2,
                commit: [0; 16],
                append_hlc: 5,
                result: b"{}".to_vec(),
            })),
        );
        payload_rt(
            7,
            Payload::GitMap(Box::new(GitMap {
                dest: 1,
                algo: Algo::Sha1,
                gflags: 3,
                cursor_seq: 9,
                dest_name: Some("origin".into()),
                entries: vec![([1; 16], Oid::Sha1([2; 20]))],
                seen: Some(vec![(0, Oid::Sha1([3; 20]))]),
            })),
        );
        payload_rt(
            8,
            Payload::Pin(Pin {
                op: 1,
                holder: 1,
                ref_id: 2,
                set_lsn: 500,
                files: vec![
                    FileRefV {
                        family: 3,
                        ref_id: 0,
                        file_no: 4,
                    },
                    FileRefV {
                        family: 5,
                        ref_id: 2,
                        file_no: 1,
                    },
                ],
            }),
        );
        payload_rt(
            9,
            Payload::Checkpoint(Box::new(Checkpoint {
                ckflags: 0b110_0011,
                append_hlc: 3,
                next_file_no: 9,
                set: Some((
                    1000,
                    1,
                    vec![SegRef {
                        file_no: 8,
                        kind: 1,
                        upto_lsn: 1000,
                        blake3_16: [1; 16],
                    }],
                )),
                window: Some((
                    138,
                    vec![RefList {
                        ref_id: 0,
                        deltas: vec![(62, 5), (100, 1)],
                    }],
                )),
                rt_upto_lsn: None,
                promotions: None,
                retirements: None,
                added: Some(vec![FileEntry {
                    file: FileRefV {
                        family: 6,
                        ref_id: 0,
                        file_no: 7,
                    },
                    total_len: 4096,
                    digest: [2; 16],
                }]),
                released: Some(vec![FileRefV {
                    family: 9,
                    ref_id: 0,
                    file_no: 3,
                }]),
            })),
        );
        payload_rt(
            11,
            Payload::Lazy(Lazy::Cursor {
                session_hash: [1; 16],
                agent_hash: [0; 16],
                feed: 2,
                task: Some(12),
                cursor_seq: 44,
                hlc: 1,
            }),
        );
        payload_rt(13, Payload::Backup("D:/bk".into(), 900, [3; 32], 4));
        payload_rt(14, Payload::SessionMark([1; 16], [2; 16], 5, 6, vec![3, 9]));
        payload_rt(16, Payload::FsIntentDone(700, 1, 5, vec![1, 3]));
        payload_rt(17, Payload::FsIntentAborted(700, 4, 1, 5));
        payload_rt(
            23,
            Payload::TreeReg(TreeReg::Epoch(
                [4; 16],
                Epoch {
                    scope_kind: 1,
                    scope_ref: 3,
                    digest: [5; 16],
                    hlc: 6,
                },
            )),
        );
        payload_rt(
            25,
            Payload::GitFacts(vec![
                Fact::Ancestry(Algo::Sha1, Oid::Sha1([1; 20]), Oid::Sha1([2; 20]), 1),
                Fact::Renames(
                    Algo::Sha1,
                    Oid::Sha1([1; 20]),
                    Oid::Sha1([2; 20]),
                    Box::new(Renames {
                        pairs: vec![(b"a".to_vec(), b"b".to_vec())],
                        groups: vec![(
                            Oid::Sha1([9; 20]),
                            vec![b"c".to_vec()],
                            vec![b"d".to_vec(), b"e".to_vec()],
                        )],
                    }),
                ),
                Fact::Time(Algo::Sha256, Oid::Sha256([1; 32]), -5, 7),
            ]),
        );
        payload_rt(
            27,
            Payload::Reserve(Box::new(Reserve {
                ref_id: 1,
                hlc: 2,
                cs_file: 3,
                blobs_file: 0,
                first_id: 10,
                n_ids: 5,
                first_anchor: 0,
                n_anchors: 0,
                schema: vec![SchemaRes {
                    class: 3,
                    key: vec!["*".into(), "labels".into(), "x".into()],
                    id: 64,
                }],
            })),
        );
        payload_rt(21, Payload::Rows(21, vec![RowOp::JournalDelete([7; 16])]));
        payload_rt(
            20,
            Payload::Rows(
                20,
                vec![RowOp::FPrint(1, Oid::Sha1([1; 20]), Some(vec![1, 2, 3]))],
            ),
        );
    }

    /// [F05 §9.9]: `next_file_no` must exceed every number named; a RefList lsn outside the window is refused.
    #[test]
    fn checkpoint_refusals() {
        let mut c = Checkpoint {
            ckflags: 1,
            append_hlc: 0,
            next_file_no: 8,
            set: Some((
                0,
                1,
                vec![SegRef {
                    file_no: 8,
                    kind: 1,
                    upto_lsn: 0,
                    blake3_16: [0; 16],
                }],
            )),
            window: None,
            rt_upto_lsn: None,
            promotions: None,
            retirements: None,
            added: None,
            released: None,
        };
        let mut w = Writer::new();
        encode_checkpoint(&c, &mut w);
        assert!(Payload::decode(9, &mut Reader::new(w.as_slice())).is_err());
        c.next_file_no = 9;
        let mut w = Writer::new();
        encode_checkpoint(&c, &mut w);
        assert!(Payload::decode(9, &mut Reader::new(w.as_slice())).is_ok());
    }

    /// A `Checkpoint` that publishes a set and retires the extents `extents` (bits 0 and 4, [F05 §9.9]).
    pub(crate) fn retiring(extents: &[u32]) -> Vec<u8> {
        let c = Checkpoint {
            ckflags: 0b1_0001,
            append_hlc: 0,
            next_file_no: 20,
            set: Some((
                0,
                1,
                vec![SegRef {
                    file_no: 8,
                    kind: 1,
                    upto_lsn: 0,
                    blake3_16: [0; 16],
                }],
            )),
            window: None,
            rt_upto_lsn: None,
            promotions: None,
            retirements: Some(
                extents
                    .iter()
                    .enumerate()
                    .map(|(i, &extent)| Retirement {
                        extent,
                        hist_file: 10 + i as u32,
                        total_len: 0,
                        digest: [0; 16],
                    })
                    .collect(),
            ),
            added: None,
            released: None,
        };
        let mut w = Writer::new();
        encode_checkpoint(&c, &mut w);
        w.into_vec()
    }

    /// [F05 §9.9]: retired extents increase by one; a run never passes the greatest extent number, so one that would is
    /// refused rather than wrapping.
    #[test]
    fn retirements_stop_at_the_greatest_extent() {
        let dec = |e: &[u32]| Payload::decode(9, &mut Reader::new(&retiring(e)));
        assert!(dec(&[u32::MAX - 1, u32::MAX]).is_ok());
        assert!(dec(&[u32::MAX]).is_ok());
        for bad in [
            &[u32::MAX, 0][..],
            &[u32::MAX, 1],
            &[u32::MAX, u32::MAX],
            &[3, 5],
        ] {
            let e = dec(bad).unwrap_err();
            assert!(e.reason.contains("increasing by one"), "{bad:?}: {e}");
        }
    }

    /// [F01 §8.1] S4, [F05 §8.1] SD-1: a class's greatest id (2^32 − 1, or 65,535 for `role` and `root`) may be defined;
    /// nothing follows it, and a later definition or a reference above it is refused, never wrapped.
    #[test]
    fn symbols_reach_the_greatest_id() {
        let def = |class: u8, id: u32, text: &str| SymDef {
            class,
            id,
            text: text.into(),
        };
        let block = |v: &[SymDef]| {
            let mut w = Writer::new();
            encode_symdefs(v, &mut w);
            decode_symdefs(&mut Reader::new(w.as_slice()))
        };
        // Within a block: the greatest id, then one more of the class.
        assert!(block(&[def(sym::ACTOR, u32::MAX, "a")]).is_ok());
        let e = block(&[def(sym::ACTOR, u32::MAX, "a"), def(sym::ACTOR, 0, "b")]).unwrap_err();
        assert!(
            e.reason.contains("SD-1") || e.reason.contains("id 0"),
            "{e}"
        );
        let e = block(&[def(sym::ACTOR, u32::MAX, "a"), def(sym::ACTOR, 1, "b")]).unwrap_err();
        assert!(e.reason.contains("SD-1"), "{e}");
        assert!(block(&[def(sym::ROLE, 65_534, "r"), def(sym::ROLE, 65_535, "s")]).is_ok());
        // Against a state: SYMTAB ends at the greatest id; the next id is 2^32, which no definition can take.
        let mut s = Symbols::from_symtab(&[
            (sym::ACTOR, u32::MAX, "a".into()),
            (sym::ROLE, 65_535, "r".into()),
        ]);
        assert_eq!(s.next[usize::from(sym::ACTOR)], Some(1 << 32));
        assert_eq!(s.next[usize::from(sym::ROLE)], Some(65_536));
        assert!(
            s.check_refs(&[(sym::ACTOR, u32::MAX), (sym::ROLE, 65_535)], 0)
                .is_ok()
        );
        let e = s.define(&[def(sym::ACTOR, 0, "b")], 0).unwrap_err();
        assert!(e.reason.contains("SD-1"), "{e}");
        let e = s.define(&[def(sym::ACTOR, 1, "b")], 0).unwrap_err();
        assert!(e.reason.contains("SD-1"), "{e}");
        let mut t = Symbols::from_symtab(&[(sym::ACTOR, u32::MAX - 1, "a".into())]);
        t.define(&[def(sym::ACTOR, u32::MAX, "b")], 0).unwrap();
        assert_eq!(t.next[usize::from(sym::ACTOR)], Some(1 << 32));
        assert_eq!(t.text(sym::ACTOR, u32::MAX), Some("b"));
    }
}

#[cfg(test)]
mod props {
    use super::tests::{E, EPOCH, head, rec};
    use super::*;
    use proptest::prelude::*;

    fn two_groups() -> (Vec<u8>, u64) {
        let g0 = Group {
            lsn: 0,
            records: vec![head(0, epoch_seed(EPOCH))],
            len: 0,
        };
        let mut b = encode_group(&g0, epoch_seed(EPOCH));
        let seed = u64::from_le_bytes(b[b.len() - 8..].try_into().unwrap());
        let g1 = Group {
            lsn: H,
            records: vec![
                rec(12, 1, H, Payload::Noop(8), None),
                rec(12, 3, H + 40, Payload::Noop(16), None),
            ],
            len: 0,
        };
        b.extend_from_slice(&encode_group(&g1, seed));
        (b, seed)
    }

    proptest! {
        /// [F05 §3], §4.5: a change to any single byte of a group makes it refused, never decoded as something else.
        #[test]
        fn torn_group_is_refused(k in 0usize..1_000, x in 1..=255u8) {
            let ctx = Ctx { e: E, epoch: EPOCH };
            let (b, seed) = two_groups();
            let g1 = &b[H as usize..];
            let back = decode_group(g1, H, seed, ctx, H as usize).unwrap();
            let again = encode_group(&back, seed);
            prop_assert_eq!(again.as_slice(), g1);
            let i = k % g1.len();
            let mut bad = g1.to_vec();
            bad[i] ^= x;
            prop_assert!(decode_group(&bad, H, seed, ctx, H as usize).is_err());
            let i = k % H as usize;
            let mut bad = b[..H as usize].to_vec();
            bad[i] ^= x;
            prop_assert!(decode_group(&bad, 0, epoch_seed(EPOCH), ctx, 0).is_err());
        }

        /// [F05 §5.2], §5.4: a record with a valid header and checksum over an arbitrary payload reaches the payload
        /// decoder; whatever it accepts re-encodes to the record's bytes, and a refusal is always the malformed kind
        /// (the header and checksum hold, so the record is valid).
        #[test]
        fn sealed_record_decode_is_canonical(
            kind in 1u8..=28,
            symdefs in any::<bool>(),
            payload in proptest::collection::vec(any::<u8>(), 0..160),
        ) {
            let lazy = matches!(kind_class(kind), Some(Class::Lazy));
            let symdefs = symdefs && !matches!(kind, 12 | 28);
            let flags = 2 | u8::from(lazy) | if symdefs { 4 } else { 0 };
            let len = HDR + payload.len() + 8;
            let mut b = Writer::new();
            b.u32(len as u32);
            b.u8(kind);
            b.u8(flags);
            b.u16(0);
            b.u64(4096);
            b.u64(EPOCH);
            b.u64(0);
            b.bytes(&payload);
            b.u64(0);
            let mut b = b.into_vec();
            let sum = record_checksum(&b, 8);
            b[24..32].copy_from_slice(&sum.to_le_bytes());
            match Record::decode_detached(&b, 0) {
                Ok((r, n)) => {
                    prop_assert_eq!(n, b.len());
                    prop_assert_eq!(r.encode(), b);
                }
                Err(e) => prop_assert!(matches!(e, RecError::Malformed(_)), "{:?}", e),
            }
        }

        /// [F01 §8.1] S4, [F05 §8.1]: definitions and references at and around the greatest id of each class's width
        /// never panic: a `SymDefs` block decodes or is refused, and a state from `SYMTAB` takes the block or refuses it.
        #[test]
        fn symbols_near_the_width_never_panic(
            defs in proptest::collection::vec(
                (
                    1u8..=11,
                    prop_oneof![
                        Just(u32::MAX),
                        Just(u32::MAX - 1),
                        Just(65_535u32),
                        Just(65_536),
                        Just(1),
                        any::<u32>(),
                    ],
                    0u8..4,
                ),
                1..6,
            ),
            table in proptest::collection::vec((1u8..=11, any::<u32>()), 0..4),
        ) {
            let v: Vec<SymDef> = defs
                .iter()
                .map(|&(class, id, t)| SymDef { class, id, text: format!("s{t}") })
                .collect();
            let mut w = Writer::new();
            encode_symdefs(&v, &mut w);
            let block = decode_symdefs(&mut Reader::new(w.as_slice()));
            let triples: Vec<(u8, u32, String)> = table
                .iter()
                .enumerate()
                .map(|(i, &(c, id))| (c, id.max(1), format!("t{i}")))
                .collect();
            let mut s = Symbols::from_symtab(&triples);
            if let Ok(b) = block {
                let refs: Vec<(u8, u32)> = b.iter().map(|d| (d.class, d.id)).collect();
                if s.define(&b, 0).is_ok() {
                    prop_assert!(s.check_refs(&refs, 0).is_ok());
                }
            }
        }

        /// [F05 §9.9]: retirement runs at and around the greatest extent number never panic; a run decodes exactly when
        /// each number is one above the previous.
        #[test]
        fn retirements_near_the_width_never_panic(
            a in prop_oneof![Just(u32::MAX), Just(u32::MAX - 1), 1u32..4],
            b in prop_oneof![Just(u32::MAX), Just(0u32), Just(1), any::<u32>()],
        ) {
            let r = Payload::decode(9, &mut Reader::new(&super::tests::retiring(&[a, b])));
            prop_assert_eq!(r.is_ok(), a.checked_add(1) == Some(b));
        }

        /// [F05 §9]: every payload decoder over arbitrary bytes never panics, and an accepted payload re-encodes to
        /// exactly its bytes.
        #[test]
        fn payload_decode_is_canonical(
            kind in 1u8..=28,
            b in proptest::collection::vec(any::<u8>(), 0..200),
        ) {
            let mut r = Reader::new(&b);
            if let Ok(p) = Payload::decode(kind, &mut r) {
                let mut w = Writer::new();
                p.encode(&mut w);
                prop_assert_eq!(w.as_slice(), &b[..]);
            }
        }
    }
}
