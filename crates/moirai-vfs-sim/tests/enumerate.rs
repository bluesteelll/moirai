//! The crash enumerator (WP-32 acceptance): a unit case per dimension showing the adverse state reached, a correct store
//! passing the tier-`pr` enumeration, and the assertion hooks catching broken protocols.
//!
//! The stores here are the enumerator author's own validation subjects, written from the fault model and the protocol
//! outline of [80 §2.4]; they are not the toy log and carry none of its seeded bugs (S4).

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use common::{STORE, rel};
use moirai_vfs::{
    Access, Acquired, DurabilityClass, DurabilityFailure, GroupMember, LockByte, LockMode, Locks,
    OpenHint, RelPath, RootAccess, RootRole, SealedMap, SealedMaps, ShareRetry, StoreFs, SyncKind,
    VfsError, VfsErrorKind,
};
use moirai_vfs_sim::enumerate::{
    BootMode, Class, Diagnosis, Dim, Dims, EffectKey, EffectKind, EffectSet, EffectWrite,
    EnumConfig, GT1_MIN_STATES, Ledger, NOTE_ACK, PoisonPolicy, Recovered, Refusal, Refused,
    Report, Subject, Tier, Variant, enumerate,
};
use moirai_vfs_sim::{
    CallKind, Event, EventKind, SimConfig, SimFile, SimMap, SimRoot, SimVfs, SimWorld, TaskEnd,
};

// ---------------------------------------------------------------------------------------------------------------------
// Shared helpers

fn store(name: &str) -> PathBuf {
    Path::new(STORE).join(name)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A process with a known boot identity and its store root.
fn open(w: &SimWorld, name: &str, access: RootAccess) -> (SimVfs, SimRoot) {
    let v = w.process_with(name, None, Some(true));
    let r = v
        .open_root(Path::new(STORE), RootRole::Store, access)
        .expect("store root");
    (v, r)
}

fn read_file(v: &SimVfs, f: &SimFile, len: u64) -> Result<Vec<u8>, VfsError> {
    let mut b = vec![0u8; len as usize];
    let n = v.read_at(f, 0, &mut b)?;
    b.truncate(n);
    Ok(b)
}

/// The body of one simulated process's task.
type Body = Box<dyn FnOnce(SimVfs) + Send>;

/// Spawns one task per process body, runs them, and turns a panicked task into a panic of the workload.
fn run_tasks(w: &SimWorld, bodies: Vec<(String, Body)>) {
    let tasks: Vec<_> = bodies
        .into_iter()
        .map(|(name, body)| {
            let p = w.process_with(&name, None, Some(true));
            w.spawn(&p, body)
        })
        .collect();
    let _ = w.run();
    for t in tasks {
        if let Some(TaskEnd::Panicked(m)) = t.end() {
            panic!("a task panicked: {m}");
        }
    }
}

/// The refusal a failed open or read of the store file `name` ends in: a read error is an I/O fault ([F16] P-92), any
/// other failure leaves a file the store needs unusable ([F16] P-59).
fn read_refused(name: &str, what: &str, e: &VfsError) -> Refused {
    let reason = if e.kind == VfsErrorKind::Io {
        Refusal::IoFault
    } else {
        Refusal::Damaged(store(name))
    };
    Refused::new(reason, format!("{what} {name}: {e:?}"))
}

/// The refusal a failed write, create or namespace operation ends in (decision (f), [OS/fs §4.4.5]).
fn write_refused(what: &str, e: &VfsError) -> Refused {
    let reason = if e.kind == VfsErrorKind::DiskFull {
        Refusal::DiskFull
    } else {
        Refusal::Io
    };
    Refused::new(reason, format!("{what}: {e:?}"))
}

/// An abort after a failed write (decision (f)): the process ends without acknowledging.
fn abort(v: &SimVfs, e: &VfsError) -> ! {
    v.fail_stop(DurabilityFailure {
        class: DurabilityClass::Lazy,
        call: e.call,
        kind: e.kind,
        os: e.os,
    })
}

fn fnv(seed: u64, data: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64 ^ seed;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn pr(seeds: &[u64], dims: Dims) -> EnumConfig {
    EnumConfig {
        dims,
        ..EnumConfig::new(Tier::Pr, seeds.iter().copied())
    }
}

const NO_DIMS: Dims = Dims {
    crash_points: false,
    kills: false,
    disk_full: false,
    flush_errors: false,
    read_faults: false,
};

// ---------------------------------------------------------------------------------------------------------------------
// MiniLog: a small log store with a two-slot HEAD, run by several writer processes under the writer byte, a reader
// that observes, and an admin process that rewrites a config file and rotates backups.

/// Protocol breaks, each of which one assertion hook must catch.
#[derive(Copy, Clone, Debug, Default)]
struct Breaks {
    /// Acknowledge before the log flush (I-G1).
    ack_before_flush: bool,
    /// Record a backup, or acknowledge a config, before the directory flush that makes its name durable (FM-2.3).
    no_sync_dir: bool,
    /// Append a commit's marker in a record of its own (a group adopted without its markers, [72 M1]).
    marker_apart: bool,
    /// Skip the re-write of the unflushed range before flushing (decision (a)).
    no_rewrite: bool,
    /// Readers read past the published end (F-A1).
    reader_past_published: bool,
    /// Acknowledge although a write failed with disk-full (decision (f)).
    ack_on_disk_full: bool,
    /// The first read after a crash skips boot-change recovery and takes the older slot (a reader serving a stale view).
    stale_first_read: bool,
}

#[derive(Clone, Debug)]
struct MiniLog {
    writers: u64,
    commits: u64,
    readers: u64,
    admin: bool,
    breaks: Breaks,
}

impl MiniLog {
    fn new() -> MiniLog {
        MiniLog {
            writers: 3,
            commits: 2,
            readers: 1,
            admin: true,
            breaks: Breaks::default(),
        }
    }

    fn with(breaks: Breaks) -> MiniLog {
        MiniLog {
            breaks,
            ..MiniLog::new()
        }
    }
}

const LOG_LEN: u64 = 16 * 4096;
const HEAD_LEN: u64 = 2 * 4096;
/// Record padding, so that records straddle sectors.
const PAD: usize = 700;
const CHAIN_SEED: u64 = 0x4D4C_4F47_5345_4544;
const SLOT_SEED: u64 = 0x534C_4F54_5345_4544;
const CONFIG_SEED: u64 = 0x434F_4E46_5345_4544;
const MAGIC: u32 = 0x474F_4C4D;
const REF: EffectKey = EffectKey::new(EffectKind::Ref, 7);
const CONFIG: EffectKey = EffectKey::new(EffectKind::Other(1), 0);
const BACKUPS: [&str; 2] = ["BACKUP.0", "BACKUP.1"];
/// A sealed segment, recorded in the log with its digest.
const SEG: EffectKey = EffectKey::new(EffectKind::Other(2), 1);
const SEG_LEN: u64 = 2 * 4096;

fn seg_bytes() -> Vec<u8> {
    (0..SEG_LEN).map(|i| (i % 251) as u8).collect()
}
const TMPS: [&str; 2] = ["tmp.1", "tmp.2"];
/// The most views one reader takes.
const READER_VIEWS: usize = 24;

fn kind_code(k: EffectKind) -> u16 {
    match k {
        EffectKind::Commit => 1,
        EffectKind::Ref => 2,
        EffectKind::Marker => 3,
        EffectKind::Lease => 4,
        EffectKind::Intent => 5,
        EffectKind::GitMap => 6,
        EffectKind::Backup => 7,
        EffectKind::Idempotency => 8,
        EffectKind::Other(n) => 0x100 + n,
    }
}

fn kind_of(c: u16) -> Option<EffectKind> {
    Some(match c {
        1 => EffectKind::Commit,
        2 => EffectKind::Ref,
        3 => EffectKind::Marker,
        4 => EffectKind::Lease,
        5 => EffectKind::Intent,
        6 => EffectKind::GitMap,
        7 => EffectKind::Backup,
        8 => EffectKind::Idempotency,
        c if c >= 0x100 => EffectKind::Other(c - 0x100),
        _ => return None,
    })
}

/// A record: magic, length, op, count, the writes, padding, and the chain value over the rest seeded by the previous
/// record's chain value ([80 §2.4.3] "Chained group validity").
fn record(op: u64, writes: &[EffectWrite], prev: u64) -> Vec<u8> {
    let len = 24 + 20 * writes.len() + PAD + 8;
    let mut b = Vec::with_capacity(len);
    b.extend(MAGIC.to_le_bytes());
    b.extend((len as u32).to_le_bytes());
    b.extend(op.to_le_bytes());
    b.extend((writes.len() as u32).to_le_bytes());
    b.extend([0u8; 4]);
    for &(k, v) in writes {
        b.extend(kind_code(k.kind).to_le_bytes());
        b.push(u8::from(v.is_some()));
        b.push(0);
        b.extend(k.key.to_le_bytes());
        b.extend(v.unwrap_or(0).to_le_bytes());
    }
    b.extend(std::iter::repeat_n((op as u8) | 1, PAD));
    let c = fnv(prev, &b);
    b.extend(c.to_le_bytes());
    b
}

fn chain_at(log: &[u8], at: usize) -> u64 {
    if at == 0 {
        CHAIN_SEED
    } else {
        u64_at(log, at - 8)
    }
}

/// The valid records from `from` on, by the chain rule; returns the valid end and the records' writes.
fn scan(log: &[u8], from: usize) -> (usize, Vec<Vec<EffectWrite>>) {
    let mut at = from;
    let mut out = Vec::new();
    if at > log.len() || (at > 0 && at < 8) {
        return (at, out);
    }
    let mut prev = chain_at(log, at);
    while at + 32 <= log.len() && u32_at(log, at) == MAGIC {
        let len = u32_at(log, at + 4) as usize;
        let n = u32_at(log, at + 16) as usize;
        if len < 32 || at + len > log.len() || n > 16 || 24 + 20 * n + 8 > len {
            break;
        }
        let chain = u64_at(log, at + len - 8);
        if fnv(prev, &log[at..at + len - 8]) != chain {
            break;
        }
        let mut writes = Vec::with_capacity(n);
        for i in 0..n {
            let w = at + 24 + 20 * i;
            let Some(kind) = kind_of(u16::from_le_bytes([log[w], log[w + 1]])) else {
                return (at, out);
            };
            let key = EffectKey::new(kind, u64_at(log, w + 4));
            let v = (log[w + 2] == 1).then(|| u64_at(log, w + 12));
            writes.push((key, v));
        }
        out.push(writes);
        prev = chain;
        at += len;
    }
    (at, out)
}

fn fold(records: &[Vec<EffectWrite>], set: &mut EffectSet) {
    for r in records {
        for &(k, v) in r {
            match v {
                Some(v) => {
                    set.insert(k, v);
                }
                None => {
                    set.remove(&k);
                }
            }
        }
    }
}

/// A `HEAD` slot, spread over three sub-sectors so that a torn slot is invalid: `seq` at 0, `end` at 512, the check at
/// 1024.
fn slot_bytes(seq: u64, end: u64) -> Vec<u8> {
    let mut b = vec![0u8; 1032];
    b[0..8].copy_from_slice(&seq.to_le_bytes());
    b[512..520].copy_from_slice(&end.to_le_bytes());
    let mut both = [0u8; 16];
    both[..8].copy_from_slice(&seq.to_le_bytes());
    both[8..].copy_from_slice(&end.to_le_bytes());
    b[1024..1032].copy_from_slice(&fnv(SLOT_SEED, &both).to_le_bytes());
    b
}

/// The valid slots: `(slot, seq, end)`.
fn slots(head: &[u8]) -> Vec<(usize, u64, u64)> {
    let mut out = Vec::new();
    for s in 0..2 {
        let o = s * 4096;
        if head.len() < o + 1032 {
            continue;
        }
        let (seq, end) = (u64_at(head, o), u64_at(head, o + 512));
        let mut both = [0u8; 16];
        both[..8].copy_from_slice(&seq.to_le_bytes());
        both[8..].copy_from_slice(&end.to_le_bytes());
        if seq > 0 && u64_at(head, o + 1024) == fnv(SLOT_SEED, &both) {
            out.push((s, seq, end));
        }
    }
    out
}

fn newest_slot(head: &[u8]) -> Option<(usize, u64, u64)> {
    slots(head).into_iter().max_by_key(|s| s.1)
}

fn config_bytes(generation: u64) -> Vec<u8> {
    let mut b = generation.to_le_bytes().to_vec();
    b.extend(fnv(CONFIG_SEED, &generation.to_le_bytes()).to_le_bytes());
    b
}

fn valid_config(b: &[u8]) -> Option<u64> {
    (b.len() >= 16 && u64_at(b, 8) == fnv(CONFIG_SEED, &b[..8])).then(|| u64_at(b, 0))
}

fn op_writes(w: u64, i: u64) -> Vec<EffectWrite> {
    let op = w * 1000 + i;
    let mut v = vec![
        (
            EffectKey::new(EffectKind::Commit, op),
            Some(fnv(1, &op.to_le_bytes())),
        ),
        (
            EffectKey::new(EffectKind::Marker, op),
            Some(fnv(2, &op.to_le_bytes())),
        ),
        (REF, Some(op)),
    ];
    if i % 2 == 1 {
        v.push((EffectKey::new(EffectKind::Lease, w), Some(i + 1)));
    }
    v
}

/// A writing process's lock client and its handles on the log and `HEAD`.
struct Handles {
    client: moirai_vfs_sim::SimClient,
    log: SimFile,
    head: SimFile,
}

impl Handles {
    fn open(v: &SimVfs, root: &SimRoot) -> Handles {
        Handles {
            client: v.lock_client(root, LockMode::Acquire).expect("lock client"),
            log: v
                .open(root, rel("LOG"), Access::ReadWrite, OpenHint::Normal)
                .expect("LOG"),
            head: v
                .open(root, rel("HEAD"), Access::ReadWrite, OpenHint::Normal)
                .expect("HEAD"),
        }
    }
}

impl MiniLog {
    /// One commit under the writer byte: scan, re-write, append, flush, publish, flush `HEAD`, acknowledge. Returns
    /// whether the writer goes on.
    fn commit(
        &self,
        v: &SimVfs,
        h: &mut Handles,
        ledger: &Ledger,
        op: u64,
        writes: &[EffectWrite],
    ) -> bool {
        let b = self.breaks;
        let Handles { client, log, head } = h;
        let (log, head) = (&*log, &*head);
        let Ok(Acquired::Granted(g)) = v.acquire_within(client, LockByte::Writer, 60_000) else {
            return false;
        };
        let hb = read_file(v, head, HEAD_LEN).unwrap_or_else(|e| abort(v, &e));
        let newest = newest_slot(&hb);
        let (seq, end) = newest.map_or((0, 0), |s| (s.1, s.2 as usize));
        let lb = read_file(v, log, LOG_LEN).unwrap_or_else(|e| abort(v, &e));
        let (e, _) = scan(&lb, end.min(lb.len()));
        if !b.no_rewrite
            && e > end
            && let Err(err) = v.write_at(log, end as u64, &lb[end..e])
        {
            abort(v, &err);
        }
        let groups: Vec<Vec<EffectWrite>> = if b.marker_apart {
            let (m, rest): (Vec<EffectWrite>, Vec<EffectWrite>) = writes
                .iter()
                .partition(|(k, _)| k.kind == EffectKind::Marker);
            vec![rest, m]
        } else {
            vec![writes.to_vec()]
        };
        let mut at = e;
        let mut prev = chain_at(&lb, e);
        for gw in groups.into_iter().filter(|g| !g.is_empty()) {
            let rec = record(op, &gw, prev);
            if at + rec.len() > LOG_LEN as usize {
                v.release(client, g);
                return false;
            }
            prev = u64_at(&rec, rec.len() - 8);
            if let Err(err) = v.write_at(log, at as u64, &rec)
                && !b.ack_on_disk_full
            {
                abort(v, &err);
            }
            at += rec.len();
        }
        if b.ack_before_flush {
            ledger.ack(op);
        }
        if let Err(f) = v.sync(log, SyncKind::Data) {
            v.fail_stop(f);
        }
        let slot = newest.map_or(0, |s| 1 - s.0);
        if let Err(err) = v.write_at(head, (slot * 4096) as u64, &slot_bytes(seq + 1, at as u64))
            && !b.ack_on_disk_full
        {
            abort(v, &err);
        }
        if let Err(f) = v.sync(head, SyncKind::DataAndMeta) {
            v.fail_stop(f);
        }
        v.release(client, g);
        if !b.ack_before_flush {
            ledger.ack(op);
        }
        true
    }

    fn writer(&self, v: &SimVfs, ledger: &Ledger, w: u64) {
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .expect("root");
        let mut h = Handles::open(v, &root);
        for i in 0..self.commits {
            let op = w * 1000 + i;
            let writes = op_writes(w, i);
            ledger.begin(op, Class::Durable, &writes);
            if !self.commit(v, &mut h, ledger, op, &writes) {
                return;
            }
        }
    }

    /// The store as a reader sees it: the log up to the published end of the newest (or, broken, the oldest) valid slot.
    fn read_view(&self, v: &SimVfs, root: &SimRoot, stale: bool) -> Result<EffectSet, Refused> {
        let head = v
            .open(root, rel("HEAD"), Access::Read, OpenHint::Normal)
            .map_err(|e| read_refused("HEAD", "open", &e))?;
        let log = v
            .open(root, rel("LOG"), Access::Read, OpenHint::Normal)
            .map_err(|e| read_refused("LOG", "open", &e))?;
        let hb = read_file(v, &head, HEAD_LEN).map_err(|e| read_refused("HEAD", "read", &e))?;
        let pick = if stale {
            slots(&hb).into_iter().min_by_key(|s| s.1)
        } else {
            newest_slot(&hb)
        };
        let end = pick.map_or(0, |s| s.2 as usize);
        let lb = read_file(v, &log, LOG_LEN).map_err(|e| read_refused("LOG", "read", &e))?;
        let limit = if self.breaks.reader_past_published {
            lb.len()
        } else {
            end.min(lb.len())
        };
        let (_, recs) = scan(&lb[..limit], 0);
        let mut set = EffectSet::new();
        fold(&recs, &mut set);
        Ok(set)
    }

    /// The config: a file replaced by a rename, whose name a crash may lose.
    fn config_view(v: &SimVfs, root: &SimRoot, set: &mut EffectSet) {
        if let Ok(f) = v.open(root, rel("CONFIG"), Access::Read, OpenHint::Normal)
            && let Ok(b) = read_file(v, &f, 16)
            && let Some(generation) = valid_config(&b)
        {
            set.insert(CONFIG, generation);
        }
    }

    /// The segment, mapped after the length check ([80 §2.5] rule 4).
    fn map_segment(v: &SimVfs, root: &SimRoot) -> Result<SimMap, Refused> {
        let damaged = |e: String| {
            Refused::new(
                Refusal::Damaged(store("SEG.1")),
                format!("segment: {e} (exit 7, repair)"),
            )
        };
        let f = v
            .open(root, rel("SEG.1"), Access::Read, OpenHint::Normal)
            .map_err(|e| damaged(format!("{e:?}")))?;
        v.map_sealed(&f, SEG_LEN, rel("SEG.1"))
            .map_err(|e| damaged(format!("{e:?}")))
    }

    /// The segment the log records, read through a mapping: a damaged one refuses the store (exit 7, `repair`).
    fn check_segment(v: &SimVfs, root: &SimRoot, set: &EffectSet) -> Result<(), Refused> {
        let Some(&digest) = set.get(&SEG) else {
            return Ok(());
        };
        let m = MiniLog::map_segment(v, root)?;
        if m.len() != SEG_LEN || fnv(3, m.bytes()) != digest {
            return Err(Refused::new(
                Refusal::Damaged(store("SEG.1")),
                "segment: damaged (exit 7, repair)",
            ));
        }
        Ok(())
    }

    /// `doctor --fsck` of the segment ([80 §2.5] rule 8): it exists with another length or content than it was sealed
    /// with.
    fn segment_damaged(v: &SimVfs, root: &SimRoot) -> bool {
        let Ok(f) = v.open(root, rel("SEG.1"), Access::Read, OpenHint::Normal) else {
            return false;
        };
        v.file_size(&f).ok() != Some(SEG_LEN)
            || read_file(v, &f, SEG_LEN).map_or(true, |b| fnv(3, &b) != fnv(3, &seg_bytes()))
    }

    /// `repair --rebuild-from-log`: the segment is derived, so it is rebuilt when the log records it and removed
    /// otherwise.
    fn repair_segment(v: &SimVfs, root: &SimRoot, recorded: bool) -> Result<(), Refused> {
        v.unlink(root, rel("SEG.1"), ShareRetry::None)
            .map_err(|e| write_refused("repair: unlink", &e))?;
        if recorded {
            let f = v
                .create_new(root, rel("SEG.1"))
                .map_err(|e| write_refused("repair: create", &e))?;
            v.write_at(&f, 0, &seg_bytes())
                .map_err(|e| write_refused("repair: write", &e))?;
            if let Err(e) = v.sync(&f, SyncKind::DataAndMeta) {
                v.fail_stop(e);
            }
            v.seal(&f).map_err(|e| write_refused("repair: seal", &e))?;
        }
        if let Err(e) = v.sync_dir(root, None) {
            v.fail_stop(e);
        }
        Ok(())
    }

    /// Every backup the log records must exist with its content: the file and its name are durable before its record.
    fn check_backups(v: &SimVfs, root: &SimRoot, set: &EffectSet, findings: &mut Vec<String>) {
        for (n, name) in BACKUPS.iter().enumerate() {
            let Some(&val) = set.get(&EffectKey::new(EffectKind::Backup, n as u64)) else {
                continue;
            };
            let found = v
                .open(root, RelPath::literal(name), Access::Read, OpenHint::Normal)
                .ok()
                .and_then(|f| read_file(v, &f, 16).ok())
                .and_then(|b| valid_config(&b));
            if found != Some(val) {
                findings.push(format!(
                    "backup {n} is recorded, but its file is missing or wrong"
                ));
            }
        }
    }

    /// Up to [`READER_VIEWS`] views, each observed. The segment is mapped once, when the log first records it, and read
    /// through that mapping at every later view ([80 §2.5]): an external truncation meanwhile ends this reader at its
    /// next mapped read (FM-9.1) or shows zeros (FM-9.2), which the digest check refuses (exit 7).
    fn reader(&self, v: &SimVfs, ledger: &Ledger) {
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::Read)
            .expect("root");
        let mut seg: Option<SimMap> = None;
        let mut seg_reads = 0;
        for _ in 0..READER_VIEWS {
            let Ok(set) = self.read_view(v, &root, false) else {
                return;
            };
            if let Some(&digest) = set.get(&SEG) {
                let m = match seg.take() {
                    Some(m) => m,
                    None => match MiniLog::map_segment(v, &root) {
                        Ok(m) => m,
                        Err(_) => return,
                    },
                };
                if fnv(3, m.bytes()) != digest {
                    return;
                }
                seg = Some(m);
                seg_reads += 1;
            }
            for (k, val) in set {
                ledger.observe(k, Some(val));
            }
            if seg_reads >= 2 {
                return;
            }
        }
    }

    /// The config rewrite (a create, a replacing rename) and two backups, each made durable with its name and then
    /// recorded in the log, the older one retired by a record and an unlink.
    fn admin(&self, v: &SimVfs, ledger: &Ledger) {
        let b = self.breaks;
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .expect("root");
        let mut h = Handles::open(v, &root);
        let durable_name = |v: &SimVfs| {
            if !b.no_sync_dir
                && let Err(f) = v.sync_dir(&root, None)
            {
                v.fail_stop(f);
            }
        };
        for (i, tmp) in TMPS.iter().enumerate() {
            let generation = i as u64 + 1;
            let op = 900_000 + generation;
            ledger.begin(op, Class::Durable, &[(CONFIG, Some(generation))]);
            let f = match v.create_new(&root, RelPath::literal(tmp)) {
                Ok(f) => f,
                Err(e) => abort(v, &e),
            };
            if let Err(e) = v.write_at(&f, 0, &config_bytes(generation)) {
                abort(v, &e);
            }
            if let Err(f) = v.sync(&f, SyncKind::DataAndMeta) {
                v.fail_stop(f);
            }
            drop(f);
            if let Err(e) = v.rename_replace(
                &root,
                RelPath::literal(tmp),
                &root,
                rel("CONFIG"),
                ShareRetry::None,
            ) {
                abort(v, &e);
            }
            durable_name(v);
            ledger.ack(op);
        }
        for (n, name) in BACKUPS.iter().enumerate() {
            let n = n as u64;
            let op = 800_000 + n;
            let val = 0x100 + n;
            let writes = [(EffectKey::new(EffectKind::Backup, n), Some(val))];
            ledger.begin(op, Class::Durable, &writes);
            let f = match v.create_new(&root, RelPath::literal(name)) {
                Ok(f) => f,
                Err(e) => abort(v, &e),
            };
            if let Err(e) = v.write_at(&f, 0, &config_bytes(val)) {
                abort(v, &e);
            }
            if let Err(f) = v.sync(&f, SyncKind::DataAndMeta) {
                v.fail_stop(f);
            }
            drop(f);
            durable_name(v);
            if !self.commit(v, &mut h, ledger, op, &writes) {
                return;
            }
            if n > 0 {
                // Retire the previous backup: its record first, then its file.
                let op = 810_000 + n - 1;
                let writes = [(EffectKey::new(EffectKind::Backup, n - 1), None)];
                ledger.begin(op, Class::Durable, &writes);
                if !self.commit(v, &mut h, ledger, op, &writes) {
                    return;
                }
                if let Err(e) = v.unlink(
                    &root,
                    RelPath::literal(BACKUPS[n as usize - 1]),
                    ShareRetry::None,
                ) {
                    abort(v, &e);
                }
                durable_name(v);
            }
        }
        // A sealed segment: written, made durable with its size, sealed, named durably, then recorded.
        let data = seg_bytes();
        let writes = [(SEG, Some(fnv(3, &data)))];
        ledger.begin(700_001, Class::Durable, &writes);
        let f = match v.create_new(&root, rel("SEG.1")) {
            Ok(f) => f,
            Err(e) => abort(v, &e),
        };
        if let Err(e) = v.write_at(&f, 0, &data) {
            abort(v, &e);
        }
        if let Err(e) = v.sync(&f, SyncKind::DataAndMeta) {
            v.fail_stop(e);
        }
        if let Err(e) = v.seal(&f) {
            abort(v, &e);
        }
        drop(f);
        durable_name(v);
        self.commit(v, &mut h, ledger, 700_001, &writes);
    }

    /// Boot-change recovery ([F16] P-66, simplified): under the writer byte, scan the whole valid chain, re-write what
    /// lies beyond the published end, flush, publish, flush `HEAD`. Returns the log's effects, or `None` when the writer
    /// byte stays with a dead holder beyond the wait bound (FM-8.1 classes (b), (c)): no writer runs.
    fn boot_recover(&self, v: &SimVfs, root: &SimRoot) -> Result<Option<EffectSet>, Refused> {
        let lock = |what: &str, e: String| Refused::new(Refusal::Lock, format!("{what}: {e}"));
        let mut client = v
            .lock_client(root, LockMode::Acquire)
            .map_err(|e| lock("lock client", format!("{e:?}")))?;
        let g = match v.acquire_within(&mut client, LockByte::Writer, 700_000) {
            Ok(Acquired::Granted(g)) => g,
            Ok(Acquired::Busy) => return Ok(None),
            Err(e) => return Err(lock("acquire", format!("{e:?}"))),
        };
        let head = v
            .open(root, rel("HEAD"), Access::ReadWrite, OpenHint::Normal)
            .map_err(|e| read_refused("HEAD", "open", &e))?;
        let log = v
            .open(root, rel("LOG"), Access::ReadWrite, OpenHint::Normal)
            .map_err(|e| read_refused("LOG", "open", &e))?;
        let hb = read_file(v, &head, HEAD_LEN).map_err(|e| read_refused("HEAD", "read", &e))?;
        let newest = newest_slot(&hb);
        let (seq, end) = newest.map_or((0, 0), |s| (s.1, s.2 as usize));
        let lb = read_file(v, &log, LOG_LEN).map_err(|e| read_refused("LOG", "read", &e))?;
        let (e, recs) = scan(&lb, 0);
        if e < end {
            // After a failed flush the scan may read a shorter log than the published end ([F16] P-48).
            return Err(Refused::new(
                Refusal::Corrupt,
                format!("the valid log ends at {e}, below the published end {end}"),
            ));
        }
        if e > end {
            v.write_at(&log, end as u64, &lb[end..e])
                .map_err(|e| write_refused("re-write", &e))?;
            if let Err(f) = v.sync(&log, SyncKind::Data) {
                v.fail_stop(f);
            }
        }
        let slot = newest.map_or(0, |s| 1 - s.0);
        v.write_at(&head, (slot * 4096) as u64, &slot_bytes(seq + 1, e as u64))
            .map_err(|e| write_refused("publish", &e))?;
        if let Err(f) = v.sync(&head, SyncKind::DataAndMeta) {
            v.fail_stop(f);
        }
        v.release(&mut client, g);
        let mut set = EffectSet::new();
        fold(&recs, &mut set);
        Ok(Some(set))
    }
}

impl Subject for MiniLog {
    fn setup(&self, w: &SimWorld, _ledger: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("LOCK"), &[0u8; 36 * 1024]).expect("LOCK");
        w.put_file(&store("LOG"), &vec![0u8; LOG_LEN as usize])
            .expect("LOG");
        w.put_file(&store("HEAD"), &vec![0u8; HEAD_LEN as usize])
            .expect("HEAD");
    }

    fn workload(&self, w: &SimWorld, ledger: &Ledger) {
        let mut bodies: Vec<(String, Body)> = Vec::new();
        for i in 0..self.writers {
            let (s, l) = (self.clone(), ledger.clone());
            bodies.push((
                format!("writer{i}"),
                Box::new(move |v| s.writer(&v, &l, i + 1)),
            ));
        }
        for i in 0..self.readers {
            let (s, l) = (self.clone(), ledger.clone());
            bodies.push((format!("reader{i}"), Box::new(move |v| s.reader(&v, &l))));
        }
        if self.admin {
            let (s, l) = (self.clone(), ledger.clone());
            bodies.push(("admin".to_owned(), Box::new(move |v| s.admin(&v, &l))));
        }
        run_tasks(w, bodies);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let mut findings = Vec::new();
        // The first reader runs boot-change recovery before its first read ([F16] P-60), then reads the published view.
        let (rv, rr) = open(w, "recovery-reader", RootAccess::ReadWrite);
        let first_read = if self.breaks.stale_first_read {
            self.read_view(&rv, &rr, true)
        } else {
            self.boot_recover(&rv, &rr)
                .and_then(|_| self.read_view(&rv, &rr, false))
        }
        .and_then(|mut s| {
            MiniLog::config_view(&rv, &rr, &mut s);
            MiniLog::check_segment(&rv, &rr, &s).map(|()| s)
        });
        let (wv, wr) = open(w, "recovery-writer", RootAccess::ReadWrite);
        let (mut diagnosed, mut answered) = (Vec::new(), Vec::new());
        // Behind a byte a dead holder keeps, the writer gives up (exit busy).
        let state = self
            .boot_recover(&wv, &wr)
            .and_then(|s| {
                s.ok_or_else(|| {
                    Refused::new(Refusal::Busy, "the writer byte stays held (exit busy)")
                })
            })
            .and_then(|mut s| {
                MiniLog::config_view(&wv, &wr, &mut s);
                MiniLog::check_backups(&wv, &wr, &s, &mut findings);
                if MiniLog::segment_damaged(&wv, &wr) {
                    // `doctor --fsck` names it; a segment the log records is rebuilt by `repair --rebuild-from-log`
                    // after the writer's exit 7, an unrecorded one (left by a crash before its record) is swept.
                    let referenced = s.contains_key(&SEG);
                    diagnosed.push(Diagnosis {
                        path: store("SEG.1"),
                        referenced,
                    });
                    if referenced {
                        answered.push(Refused::new(
                            Refusal::Damaged(store("SEG.1")),
                            "segment: damaged (exit 7, repair)",
                        ));
                    }
                    MiniLog::repair_segment(&wv, &wr, referenced)?;
                }
                MiniLog::check_segment(&wv, &wr, &s).map(|()| s)
            });
        Recovered {
            first_read,
            state,
            answered,
            findings,
            diagnosed,
        }
    }

    fn slot_files(&self) -> Vec<PathBuf> {
        vec![store("HEAD")]
    }
}

fn has(report: &Report, needle: &str) -> bool {
    report
        .failures
        .iter()
        .any(|f| f.messages.iter().any(|m| m.contains(needle)))
}

// ---------------------------------------------------------------------------------------------------------------------
// The correct store passes the tier-`pr` enumeration.

#[test]
fn a_correct_store_passes_every_pr_dimension_within_the_budget() {
    let cfg = EnumConfig::new(Tier::Pr, [1, 2]);
    let report = enumerate(&MiniLog::new(), &cfg);
    println!("{report}");
    report.assert_passed();
    // Crash points at every write, flush, publish, create, rename and unlink.
    for call in [
        CallKind::Write,
        CallKind::Sync,
        CallKind::SyncDir,
        CallKind::CreateNew,
        CallKind::RenameReplace,
        CallKind::Unlink,
    ] {
        assert!(
            report
                .crash_points_by_call
                .get(&call)
                .is_some_and(|&n| n > 0),
            "{call:?}"
        );
    }
    assert!(report.publish_points > 0);
    // The PR tier's states: prefixes, torn sectors, both slots, namespace subsets, bounded products; no full subsets.
    for d in [
        Dim::Pivot,
        Dim::Prefix,
        Dim::Torn,
        Dim::Slot,
        Dim::Namespace,
        Dim::Cross,
        Dim::InFlight,
    ] {
        assert!(
            report.states.get(&d).is_some_and(|&n| n > 0),
            "{d:?}: {report}"
        );
    }
    assert!(!report.states.contains_key(&Dim::Subset) && !report.states.contains_key(&Dim::Random));
    assert!(
        report.kills > 0 && report.kills_in_flush.iter().all(|&n| n > 0),
        "{report}"
    );
    for site in [
        "WriteFault",
        "FlushFault",
        "SyncDirFault",
        "CreateFault",
        "NsFault",
    ] {
        assert!(
            report.disk_full.keys().any(|s| format!("{s:?}") == site),
            "{site}"
        );
    }
    assert!(
        report.flush_errors > 0 && report.poisoned_reads > 0,
        "{report}"
    );
    for site in [
        moirai_vfs_sim::Site::ReadFault,
        moirai_vfs_sim::Site::MapFault,
    ] {
        assert!(
            report.read_faults.get(&site).is_some_and(|&n| n > 0),
            "{site:?}: {report}"
        );
    }
    // The segment truncated after the workload and while readers hold its mapping: readers refuse, the writer's
    // recovery repairs it and names it.
    assert!(
        report.truncations > 2 && report.diagnoses > 0 && report.refusals > 0,
        "{report}"
    );
    assert!(report.elapsed < std::time::Duration::from_secs(600));
}

/// The nightly and exit tiers (`MOIRAI_TEST_TIER`) run the correct store through every dimension at full depth and
/// report their state counts; the PR tier skips this test (the one above is its acceptance).
#[test]
fn the_nightly_tier_passes_and_reports_its_state_counts() {
    let tier = Tier::from_env();
    if tier == Tier::Pr {
        return;
    }
    let report = enumerate(&MiniLog::new(), &EnumConfig::new(tier, [1, 2]));
    println!("{report}");
    // The enumeration itself fails below GT1's 10⁵ crash states and without a release of class (b) and (c) per seed.
    report.assert_passed();
    assert!(report.states_total() >= GT1_MIN_STATES, "{report}");
    assert!(
        report.release_classes[1] > 0 && report.release_classes[2] > 0,
        "{report}"
    );
    assert!(
        report.kills_of_holders > 0 && report.crash_points_after_kills > 0,
        "{report}"
    );
    assert!(
        report
            .poison_mixed
            .get(&Dim::Random)
            .is_some_and(|&n| n > 0),
        "{report}"
    );
    for d in [
        Dim::Pivot,
        Dim::Prefix,
        Dim::Subset,
        Dim::Torn,
        Dim::Slot,
        Dim::Namespace,
        Dim::Cross,
        Dim::InFlight,
        Dim::Random,
    ] {
        assert!(
            report.states.get(&d).is_some_and(|&n| n > 0),
            "{d:?}: {report}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The assertion hooks catch broken protocols.

#[test]
fn acknowledged_durable_effects_are_checked_after_every_crash() {
    let only = Dims {
        crash_points: true,
        ..NO_DIMS
    };
    let r = enumerate(
        &MiniLog::with(Breaks {
            ack_before_flush: true,
            ..Breaks::default()
        }),
        &pr(&[1], only),
    );
    assert!(!r.passed());
    assert!(has(&r, "acknowledged or observed"), "{r}");
}

#[test]
fn markers_are_all_or_nothing_with_their_commit() {
    let only = Dims {
        crash_points: true,
        ..NO_DIMS
    };
    let r = enumerate(
        &MiniLog::with(Breaks {
            marker_apart: true,
            ..Breaks::default()
        }),
        &pr(&[1], only),
    );
    assert!(has(&r, "partly applied"), "{r}");
}

#[test]
fn namespace_durability_is_checked() {
    let only = Dims {
        crash_points: true,
        ..NO_DIMS
    };
    let r = enumerate(
        &MiniLog::with(Breaks {
            no_sync_dir: true,
            ..Breaks::default()
        }),
        &pr(&[1], only),
    );
    assert!(
        has(&r, "backup 0 is recorded") || has(&r, "backup 1 is recorded"),
        "{r}"
    );
    assert!(has(&r, "Other(1)"), "the config rename: {r}");
}

#[test]
fn read_freshness_is_checked_before_and_after_the_crash() {
    let only = Dims {
        crash_points: true,
        ..NO_DIMS
    };
    // Before: a reader that saw an unflushed group, which a crash then lost.
    let broken = MiniLog {
        readers: 2,
        ..MiniLog::with(Breaks {
            reader_past_published: true,
            ..Breaks::default()
        })
    };
    let r = enumerate(&broken, &pr(&[1, 2, 3, 4], only));
    // The value seen marks its pending durable group done; a crash that loses it loses an observed effect.
    assert!(has(&r, "acknowledged or observed"), "{r}");
    // After: a first read that serves an older view.
    let r = enumerate(
        &MiniLog::with(Breaks {
            stale_first_read: true,
            ..Breaks::default()
        }),
        &pr(&[1], only),
    );
    assert!(has(&r, "first read"), "{r}");
}

#[test]
fn an_acknowledgement_after_disk_full_is_caught() {
    let only = Dims {
        disk_full: true,
        ..NO_DIMS
    };
    let r = enumerate(
        &MiniLog::with(Breaks {
            ack_on_disk_full: true,
            ..Breaks::default()
        }),
        &pr(&[1], only),
    );
    assert!(!r.passed(), "{r}");
    assert!(
        r.failures
            .iter()
            .any(|f| matches!(f.variant, Variant::DiskFull { .. }))
    );
}

#[test]
fn flush_error_more_commits_crash_catches_a_missing_rewrite() {
    let only = Dims {
        flush_errors: true,
        ..NO_DIMS
    };
    let broken = MiniLog {
        commits: 3,
        ..MiniLog::with(Breaks {
            no_rewrite: true,
            ..Breaks::default()
        })
    };
    // The pages keep the failed flush's bytes until they are evicted ([80 §2.3.4], ext4 and XFS): the next writer validates
    // the lost group and appends behind it; without the re-write, a crash (or the next full scan) loses both.
    let mut cfg = pr(&[1], only);
    cfg.limits.poison_policies = vec![PoisonPolicy::Evict];
    let r = enumerate(&broken, &cfg);
    assert!(!r.passed(), "{r}");
    assert!(
        r.failures
            .iter()
            .any(|f| matches!(f.variant, Variant::FlushError { .. }))
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Probes: one subject per dimension family, whose recovery or workload records what it saw, so that each test can show
// the adverse state reached.

type Seen = Arc<Mutex<BTreeSet<String>>>;

/// A subject made of closures; what they record goes to a shared set.
struct Probe {
    files: Vec<(&'static str, Vec<u8>)>,
    workload: Arc<dyn Fn(SimVfs) + Send + Sync>,
    recover: Box<dyn Fn(&SimWorld) + Send + Sync>,
    slots: Vec<PathBuf>,
}

impl Subject for Probe {
    fn setup(&self, w: &SimWorld, _ledger: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        for (name, data) in &self.files {
            w.put_file(&store(name), data).expect("setup file");
        }
    }

    fn workload(&self, w: &SimWorld, _ledger: &Ledger) {
        let body = Arc::clone(&self.workload);
        run_tasks(w, vec![("probe".to_owned(), Box::new(move |v| body(v)))]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        (self.recover)(w);
        // A probe's `doctor --fsck`: every file shorter than it was made (an external truncation, FM-10.2).
        let diagnosed = self
            .files
            .iter()
            .filter(|(n, d)| w.file_len(&store(n)).is_some_and(|l| l < d.len() as u64))
            .map(|(n, _)| Diagnosis {
                path: store(n),
                referenced: false,
            })
            .collect();
        Recovered {
            diagnosed,
            ..Recovered::same(EffectSet::new())
        }
    }

    fn slot_files(&self) -> Vec<PathBuf> {
        self.slots.clone()
    }
}

fn note(seen: &Seen, s: impl Into<String>) {
    lock(seen).insert(s.into());
}

/// The first byte of every sub-sector of `sector`, as hex.
fn subs(b: &[u8], sector: usize) -> String {
    (0..8)
        .map(|j| {
            let at = sector * 4096 + j * 512;
            b.get(at)
                .map_or_else(|| "--".to_owned(), |x| format!("{x:02x}"))
        })
        .collect()
}

fn rw(v: &SimVfs, root: &SimRoot, name: &'static str) -> SimFile {
    v.open(root, rel(name), Access::ReadWrite, OpenHint::Normal)
        .expect("open")
}

fn root_rw(v: &SimVfs) -> SimRoot {
    v.open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
        .expect("root")
}

fn content(w: &SimWorld, name: &'static str) -> Option<Vec<u8>> {
    let (v, r) = open(w, "probe-recovery", RootAccess::Read);
    let f = v.open(&r, rel(name), Access::Read, OpenHint::Normal).ok()?;
    read_file(&v, &f, 1 << 20).ok()
}

/// The nightly tier's limits for a probe: one seed, and no minimum of crash states (a probe is far smaller than a
/// store; GT1's minimum is the store's).
fn nightly_only(dims: Dims) -> EnumConfig {
    let mut cfg = EnumConfig::new(Tier::Nightly, [1]);
    cfg.dims = dims;
    cfg.limits.min_states = 0;
    cfg
}

/// Sector subsets and intermediate versions (FM-1.1, FM-1.5), one torn sector (FM-1.2), both slots of a `HEAD`-like
/// file ({old, new, torn}²), bounded cross-file products, sizes of H(f) (FM-2.2), namespace operations lost in any
/// subset (FM-2.3) and writes in flight (§2.5).
#[test]
fn crash_states_reach_every_file_slot_and_namespace_state() {
    let seen: Seen = Arc::default();
    let s2 = Arc::clone(&seen);
    let probe = Probe {
        files: vec![
            ("A", vec![0x11; 3 * 4096]),
            ("B", vec![0x11; 2 * 4096]),
            ("H", vec![0x00; 2 * 4096]),
        ],
        workload: Arc::new(|v: SimVfs| {
            let r = root_rw(&v);
            let (a, b, h) = (rw(&v, &r, "A"), rw(&v, &r, "B"), rw(&v, &r, "H"));
            v.write_at(&a, 0, &[0x22; 3 * 4096]).expect("A");
            v.write_at(&a, 4096, &[0x33; 4096])
                .expect("A sector 1 again");
            v.write_at(&b, 0, &[0x44; 2 * 4096]).expect("B");
            v.write_at(&b, 2 * 4096, &[0x45; 100]).expect("B grows");
            v.write_at(&h, 0, &[0x55; 4096]).expect("slot 0");
            v.write_at(&h, 4096, &[0x66; 4096]).expect("slot 1");
            for n in ["n1", "n2", "n3"] {
                v.create_new(&r, RelPath::literal(n)).expect("create");
            }
        }),
        recover: Box::new(move |w| {
            let a = content(w, "A").unwrap_or_default();
            let b = content(w, "B").unwrap_or_default();
            let h = content(w, "H").unwrap_or_default();
            let names: String = ["n1", "n2", "n3"]
                .iter()
                .map(|n| if w.exists(&store(n)) { '1' } else { '0' })
                .collect();
            note(
                &s2,
                format!(
                    "A={}|{}|{} B={}:{}|{} H={}|{} N={names}",
                    subs(&a, 0),
                    subs(&a, 1),
                    subs(&a, 2),
                    b.len(),
                    subs(&b, 0),
                    subs(&b, 1),
                    subs(&h, 0),
                    subs(&h, 1)
                ),
            );
        }),
        slots: vec![store("H")],
    };
    let mut cfg = nightly_only(Dims {
        crash_points: true,
        ..NO_DIMS
    });
    cfg.limits.plans.random_per_point = 4;
    let report = enumerate(&probe, &cfg);
    report.assert_passed();
    let states = lock(&seen).clone();
    let field = |s: &str, key: &str| -> String {
        s.split(' ')
            .find_map(|f| f.strip_prefix(key))
            .expect("field")
            .to_owned()
    };
    let sector = |s: &str, key: &str, i: usize| {
        field(s, key)
            .split(['|', ':'])
            .nth(i)
            .expect("sector")
            .to_owned()
    };
    let whole = |x: &str, b: u8| x == format!("{b:02x}").repeat(8);
    let torn = |x: &str| x.len() == 16 && (0..8).any(|j| x[2 * j..2 * j + 2] != x[..2]);
    // A later write survives while an earlier one is lost (FM-1.5).
    assert!(
        states
            .iter()
            .any(|s| whole(&sector(s, "A=", 0), 0x11) && whole(&sector(s, "A=", 2), 0x22))
    );
    // An intermediate version (FM-1.1): sector 1 was written 0x22, then 0x33.
    assert!(states.iter().any(|s| whole(&sector(s, "A=", 1), 0x22)));
    // One torn sector (FM-1.2).
    assert!(
        states
            .iter()
            .any(|s| (0..3).any(|i| torn(&sector(s, "A=", i))))
    );
    // Both slots: {old, new, torn}² less (torn, torn), which FM-1.2 excludes for two dirty sectors of one file.
    let class = |x: &str, new: u8| {
        if whole(x, 0) {
            "old"
        } else if whole(x, new) {
            "new"
        } else {
            "torn"
        }
    };
    let slots: BTreeSet<(&str, &str)> = states
        .iter()
        .map(|s| {
            (
                class(&sector(s, "H=", 0), 0x55),
                class(&sector(s, "H=", 1), 0x66),
            )
        })
        .collect();
    assert_eq!(slots.len(), 8, "{slots:?}");
    // Cross-file products: A new while B old, and A old while B new.
    assert!(
        states
            .iter()
            .any(|s| whole(&sector(s, "A=", 0), 0x22) && whole(&sector(s, "B=", 1), 0x11))
    );
    assert!(
        states
            .iter()
            .any(|s| whole(&sector(s, "A=", 0), 0x11) && whole(&sector(s, "B=", 1), 0x44))
    );
    // Sizes of H(f): B at its durable size and grown.
    let sizes: BTreeSet<String> = states.iter().map(|s| sector(s, "B=", 0)).collect();
    assert!(
        sizes.contains("8192") && sizes.contains("8292"),
        "{sizes:?}"
    );
    // Namespace operations lost in any subset: a later create survives an earlier lost one, and the reverse.
    let names: BTreeSet<String> = states.iter().map(|s| field(s, "N=")).collect();
    assert!(
        names.contains("001") && names.contains("100") && names.contains("010"),
        "{names:?}"
    );
    for d in [
        Dim::Subset,
        Dim::Torn,
        Dim::Slot,
        Dim::Cross,
        Dim::Size,
        Dim::Namespace,
        Dim::InFlight,
        Dim::Random,
    ] {
        assert!(
            report.states.get(&d).is_some_and(|&n| n > 0),
            "{d:?}: {report}"
        );
    }
}

/// Every subset of ≤ 12 dirty sectors, and ≥ 10⁴ random states beyond; the nightly tier reports its state counts.
#[test]
fn the_nightly_tier_enumerates_every_subset_and_samples_beyond_twelve() {
    let probe = Probe {
        files: vec![("S", vec![0x11; 12 * 4096]), ("R", vec![0x11; 13 * 4096])],
        workload: Arc::new(|v: SimVfs| {
            let r = root_rw(&v);
            v.write_at(&rw(&v, &r, "S"), 0, &[0x22; 12 * 4096])
                .expect("S");
            v.write_at(&rw(&v, &r, "R"), 0, &[0x22; 13 * 4096])
                .expect("R");
        }),
        recover: Box::new(|_| {}),
        slots: Vec::new(),
    };
    let report = enumerate(
        &probe,
        &nightly_only(Dims {
            crash_points: true,
            ..NO_DIMS
        }),
    );
    let shown = report.to_string();
    println!("{shown}");
    report.assert_passed();
    // At one crash point: every subset of S's 12 sectors against both pivots (the prefixes among them are counted as
    // prefixes), and at least 10⁴ random states for R's 13.
    let at_one = |d: Dim| report.point_max.get(&d).copied().unwrap_or(0);
    assert!(
        at_one(Dim::Subset) + at_one(Dim::Prefix) >= 2 * 4096,
        "{shown}"
    );
    assert!(at_one(Dim::Random) >= 10_000, "{shown}");
    assert!(report.point_max_total >= 2 * 4096 + 10_000, "{shown}");
    assert!(shown.contains("crash states") && shown.contains("Random") && shown.contains("Subset"));
}

/// Disk-full at every write, flush, create and namespace operation (FM-5), and FM-5.4's empty file.
#[test]
fn disk_full_is_injected_at_every_write_flush_create_and_namespace_operation() {
    let seen: Seen = Arc::default();
    let (s1, s2) = (Arc::clone(&seen), Arc::clone(&seen));
    let probe = Probe {
        files: Vec::new(),
        workload: Arc::new(move |v: SimVfs| {
            let r = root_rw(&v);
            let failed = |what: &str, e: VfsErrorKind| note(&s1, format!("{what} {e:?}"));
            let f = match v.create_new(&r, rel("x")) {
                Ok(f) => f,
                Err(e) => return failed("create", e.kind),
            };
            if let Err(e) = v.write_at(&f, 0, &[7; 5000]) {
                return failed("write", e.kind);
            }
            if let Err(e) = v.sync(&f, SyncKind::DataAndMeta) {
                return failed("flush", e.kind);
            }
            if let Err(e) = v.sync_dir(&r, None) {
                return failed("sync_dir", e.kind);
            }
            drop(f);
            if let Err(e) = v.rename_noreplace(&r, rel("x"), &r, rel("y"), ShareRetry::None) {
                return failed("rename", e.kind);
            }
            if let Err(e) = v.unlink(&r, rel("y"), ShareRetry::None) {
                failed("unlink", e.kind);
            }
        }),
        recover: Box::new(move |w| {
            if content(w, "x").is_some_and(|c| c.is_empty()) {
                note(&s2, "an empty file left by a failed create");
            }
        }),
        slots: Vec::new(),
    };
    let report = enumerate(
        &probe,
        &pr(
            &[1],
            Dims {
                disk_full: true,
                ..NO_DIMS
            },
        ),
    );
    report.assert_passed();
    let seen = lock(&seen).clone();
    for s in [
        "create DiskFull",
        "write DiskFull",
        "flush DiskFull",
        "sync_dir DiskFull",
        "rename DiskFull",
        "unlink DiskFull",
        "an empty file left by a failed create",
    ] {
        assert!(seen.contains(s), "{s}: {seen:?}");
    }
}

/// The run state the flush-failure probe's tasks share.
#[derive(Default)]
struct Group {
    /// Set by the writer whose flush failed: (writer, its last round).
    failed: Option<(usize, u8)>,
    /// A flush succeeded after the failure.
    ok_after: bool,
    /// Writers that appended after the failure.
    after: BTreeSet<usize>,
}

/// A subject whose workload and recovery are closures over shared state.
struct Holders<W, R> {
    workload: W,
    recover: R,
}

impl<W: Fn(&SimWorld), R: Fn(&SimWorld)> Subject for Holders<W, R> {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("G"), &[0u8; 4096]).expect("G");
    }

    fn workload(&self, w: &SimWorld, _l: &Ledger) {
        (self.workload)(w);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        (self.recover)(w);
        Recovered::same(EffectSet::new())
    }
}

/// "Flush error, more commits, crash", and several pending groups and flush holders with reverted, invalidated or
/// evicted pages while appends continue (FM-3, FM-11.3, [80 §2.4.4]): four writers share one sector, each flushing every
/// other round; one flush fails and its writer dies; the three others go on appending, reading and flushing.
#[test]
fn failed_flushes_poison_pages_while_three_live_writers_append_and_flush() {
    const WRITERS: usize = 4;
    let seen: Seen = Arc::default();
    let group: Arc<Mutex<Group>> = Arc::default();
    let (s1, s2, g1, g2) = (
        Arc::clone(&seen),
        Arc::clone(&seen),
        Arc::clone(&group),
        Arc::clone(&group),
    );
    let workload = move |w: &SimWorld| {
        *lock(&g1) = Group::default();
        let bodies: Vec<(String, Body)> = (0..WRITERS)
            .map(|i| {
                let (seen, group) = (Arc::clone(&s1), Arc::clone(&g1));
                let body: Body = Box::new(move |v: SimVfs| {
                    let r = root_rw(&v);
                    let f = rw(&v, &r, "G");
                    let mut last = [0u8; WRITERS];
                    for round in 1..=4u8 {
                        let mine = ((i as u8) << 4) | round;
                        v.write_at(&f, (i * 512) as u64, &[mine; 512])
                            .expect("write");
                        let failed = lock(&group).failed;
                        if failed.is_some() {
                            lock(&group).after.insert(i);
                        }
                        let mut buf = vec![0u8; 4096];
                        v.read_at(&f, 0, &mut buf).expect("read");
                        for (j, seen_last) in last.iter_mut().enumerate() {
                            let now = buf[j * 512] & 0x0F;
                            if now < *seen_last && failed.is_some() {
                                note(
                                    &seen,
                                    "a read went back to an older round after the failed flush",
                                );
                                if lock(&group).ok_after {
                                    note(
                                        &seen,
                                        "a later successful flush did not end the poisoning",
                                    );
                                }
                            }
                            if now != *seen_last
                                && *seen_last != 0
                                && failed.is_some_and(|(d, _)| d == j)
                            {
                                note(&seen, "the dead writer's bytes changed between reads");
                            }
                            *seen_last = now;
                        }
                        if round % 2 == 0 {
                            match v.sync(&f, SyncKind::Data) {
                                Ok(()) => {
                                    let mut g = lock(&group);
                                    if g.failed.is_some() {
                                        g.ok_after = true;
                                    }
                                }
                                Err(e) => {
                                    lock(&group).failed = Some((i, round));
                                    v.fail_stop(e);
                                }
                            }
                        }
                    }
                });
                (format!("writer{i}"), body)
            })
            .collect();
        run_tasks(w, bodies);
        let g = lock(&g1);
        if g.failed.is_some() && g.after.len() >= 3 {
            note(&s1, "three live writers appended after the failed flush");
        }
    };
    let probe = Holders {
        workload,
        recover: move |w: &SimWorld| {
            let Some((dead, round)) = lock(&g2).failed else {
                return;
            };
            let Some(c) = content(w, "G") else {
                return;
            };
            let dead_lost = (c[dead * 512] & 0x0F) < round;
            let later_kept = (0..WRITERS).any(|j| j != dead && (c[j * 512] & 0x0F) > round);
            if c.len() >= 4096 && dead_lost && later_kept {
                note(
                    &s2,
                    "after the crash the dead writer's group is lost while later appends survive",
                );
            }
        },
    };
    let report = enumerate(
        &probe,
        &pr(
            &[1, 2],
            Dims {
                flush_errors: true,
                ..NO_DIMS
            },
        ),
    );
    report.assert_passed();
    assert!(
        report.flush_errors > 0 && report.poisoned_reads > 0,
        "{report}"
    );
    let seen = lock(&seen).clone();
    for s in [
        "three live writers appended after the failed flush",
        "a read went back to an older round after the failed flush",
        "a later successful flush did not end the poisoning",
        "the dead writer's bytes changed between reads",
        "after the crash the dead writer's group is lost while later appends survive",
    ] {
        assert!(seen.contains(s), "{s}: {seen:?}");
    }
}

/// Process death at every point, inside a flush with each of its three outcomes (§2.5, FM-11.2): a flush that failed at
/// its holder's death leaves reads that change or mix (FM-3.2).
#[test]
fn deaths_inside_a_flush_take_each_outcome() {
    let seen: Seen = Arc::default();
    let s2 = Arc::clone(&seen);
    let probe = Probe {
        files: vec![("K", vec![0u8; 4096])],
        workload: Arc::new(|v: SimVfs| {
            let r = root_rw(&v);
            let f = rw(&v, &r, "K");
            for b in [0xA1u8, 0xA2] {
                v.write_at(&f, 0, &[b; 4096]).expect("write");
                if let Err(e) = v.sync(&f, SyncKind::Data) {
                    v.fail_stop(e);
                }
            }
        }),
        recover: Box::new(move |w| {
            let (Some(a), Some(b)) = (content(w, "K"), content(w, "K")) else {
                return;
            };
            let mixed = |c: &[u8]| (0..8).any(|j| c[j * 512] != c[0]);
            note(
                &s2,
                if a != b || mixed(&a) {
                    "poisoned: reads differ or mix"
                } else {
                    "stable"
                },
            );
        }),
        slots: Vec::new(),
    };
    let report = enumerate(
        &probe,
        &pr(
            &[1, 2, 3],
            Dims {
                kills: true,
                ..NO_DIMS
            },
        ),
    );
    report.assert_passed();
    assert!(report.kills_in_flush.iter().all(|&n| n > 0), "{report}");
    let seen = lock(&seen).clone();
    assert!(
        seen.contains("poisoned: reads differ or mix") && seen.contains("stable"),
        "{seen:?}"
    );
}

/// The time budget fails an enumeration that exceeds it rather than letting it pass with fewer states.
#[test]
fn an_exceeded_budget_fails_the_report() {
    let mut cfg = EnumConfig::new(Tier::Pr, [1]);
    cfg.limits.budget = Some(std::time::Duration::ZERO);
    let r = enumerate(&MiniLog::new(), &cfg);
    assert!(!r.passed() && r.incomplete.is_some(), "{r}");
}

/// Read errors, transient and persistent (FM-12), a media fault under a mapping that ends the reader (FM-9.1), and an
/// external truncation of a sealed file (FM-10.2); a refusal is a correct answer after a persistent error or a
/// truncation, a wrong answer never is.
#[test]
fn read_faults_mapping_faults_and_truncations_are_injected() {
    let seen: Seen = Arc::default();
    let run: Arc<Mutex<(bool, bool)>> = Arc::default();
    let (s1, s2, r1, r2) = (
        Arc::clone(&seen),
        Arc::clone(&seen),
        Arc::clone(&run),
        Arc::clone(&run),
    );
    let probe = Probe {
        files: vec![("Z", vec![0x5A; 4096])],
        workload: Arc::new(move |v: SimVfs| {
            *lock(&r1) = (false, false);
            let r = root_rw(&v);
            let f = rw(&v, &r, "Z");
            v.seal(&f).expect("seal");
            let mut buf = vec![0u8; 4096];
            if let Err(e) = v.read_at(&f, 0, &mut buf) {
                note(&s1, format!("read error {:?}", e.kind));
                note(
                    &s1,
                    if v.read_at(&f, 0, &mut buf).is_ok() {
                        "transient: the next read succeeded"
                    } else {
                        "persistent: the next read failed too"
                    },
                );
                return;
            }
            // A file truncated meanwhile fails the length check: the reader exits 7 ([80 §2.5] rule 4).
            let Ok(m) = v.map_sealed(&f, 4096, rel("Z")) else {
                return;
            };
            lock(&r1).0 = true;
            let ok = m.bytes().iter().all(|&b| b == 0x5A);
            lock(&r1).1 = ok;
        }),
        recover: Box::new(move |w| {
            let (mapping, mapped) = *lock(&r2);
            if mapping && !mapped {
                note(&s2, "the process died at its mapped read");
            }
            match content(w, "Z") {
                Some(c) if c.len() < 4096 => note(&s2, "a truncated sealed file"),
                None => note(&s2, "an unreadable file after a persistent error"),
                Some(_) => {}
            }
        }),
        slots: Vec::new(),
    };
    // The probe's recovery answers with an empty state whatever it reads; the refusals are the MiniLog's.
    let report = enumerate(
        &probe,
        &pr(
            &[1],
            Dims {
                read_faults: true,
                ..NO_DIMS
            },
        ),
    );
    report.assert_passed();
    assert!(
        report.read_faults.len() == 2 && report.truncations > 2 && report.diagnoses > 0,
        "after the workload and at the points of calls on the file: {report}"
    );
    let seen = lock(&seen).clone();
    for s in [
        "read error Io",
        "transient: the next read succeeded",
        "persistent: the next read failed too",
        "the process died at its mapped read",
        "a truncated sealed file",
        "an unreadable file after a persistent error",
    ] {
        assert!(seen.contains(s), "{s}: {seen:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Review pass 1: sync_group members, busy flush holders and lock waiters, crash points after a holder's death, release
// classes, poisoned random mixes, trace predicates, truncation while readers run, lazy effects after a failed flush.

fn class_of(c: &[u8], old: u8, new: u8) -> &'static str {
    if c.iter().all(|&x| x == old) {
        "old"
    } else if c.iter().all(|&x| x == new) {
        "new"
    } else {
        "mixed"
    }
}

/// A death inside a `sync_group` resolves each member separately ([F15 §2.5], FM-11.2): after a crash at the run's end
/// (the baseline pivot), a member whose flush succeeded holds its new bytes while another, failed (its pages reverted)
/// or not performed, holds its old ones.
#[test]
fn deaths_inside_a_sync_group_resolve_each_member_separately() {
    let seen: Seen = Arc::default();
    let s2 = Arc::clone(&seen);
    let probe = Probe {
        files: vec![("A", vec![0x11; 4096]), ("B", vec![0x11; 4096])],
        workload: Arc::new(|v: SimVfs| {
            let r = root_rw(&v);
            let (a, b) = (rw(&v, &r, "A"), rw(&v, &r, "B"));
            v.write_at(&a, 0, &[0xA1; 4096]).expect("A");
            v.write_at(&b, 0, &[0xB1; 4096]).expect("B");
            let members = [
                GroupMember::File {
                    file: &a,
                    kind: SyncKind::Data,
                },
                GroupMember::File {
                    file: &b,
                    kind: SyncKind::Data,
                },
            ];
            if let Err(e) = v.sync_group(&members) {
                v.fail_stop(e);
            }
        }),
        recover: Box::new(move |w| {
            let a = content(w, "A").unwrap_or_default();
            let b = content(w, "B").unwrap_or_default();
            note(
                &s2,
                format!(
                    "A {} B {}",
                    class_of(&a, 0x11, 0xA1),
                    class_of(&b, 0x11, 0xB1)
                ),
            );
        }),
        slots: Vec::new(),
    };
    let mut cfg = pr(
        &[1],
        Dims {
            kills: true,
            ..NO_DIMS
        },
    );
    // Reverted pages: a failed member reads as old, so only mixed outcome vectors split the members.
    cfg.limits.poison_policies = vec![PoisonPolicy::Revert];
    let report = enumerate(&probe, &cfg);
    report.assert_passed();
    assert!(report.kills_mixed_group > 0, "{report}");
    let seen = lock(&seen).clone();
    assert!(
        seen.contains("A new B old") && seen.contains("A old B new"),
        "{seen:?}"
    );
}

/// What one run of [`Adopt`] did, shared with its recovery.
#[derive(Default)]
struct AdoptRun {
    /// Writer 1 is inside its flush (set before the call, cleared at its return).
    w1_in_flush: bool,
    /// Writer 1's flush returned.
    w1_flushed: bool,
    /// Writer 2 appended while writer 1 was inside its flush.
    w2_during: bool,
    /// The scheduling point at which writer 2's flush returned.
    w2_flushed_at: Option<u64>,
    /// The scheduling points at the workload's end.
    end: u64,
}

/// Two writers append to one log file under the writer byte and flush outside it ([80 §2.4.3]): writer 1 appends
/// sector 0, writer 2 a sub-sector of sector 1 and reads it back before its own flush. The recovery takes the writer
/// byte within a 2 s bound and refuses when it cannot (a dead holder's byte, FM-8.1).
struct Adopt {
    seen: Seen,
    run: Arc<Mutex<AdoptRun>>,
}

impl Adopt {
    fn new() -> Adopt {
        Adopt {
            seen: Arc::default(),
            run: Arc::default(),
        }
    }

    fn append(v: &SimVfs, client: &mut moirai_vfs_sim::SimClient, f: &SimFile, at: u64, b: &[u8]) {
        let Ok(Acquired::Granted(g)) = v.acquire_within(client, LockByte::Writer, 60_000) else {
            return;
        };
        if let Err(e) = v.write_at(f, at, b) {
            abort(v, &e);
        }
        v.release(client, g);
    }
}

impl Subject for Adopt {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("LOCK"), &[0u8; 36 * 1024]).expect("LOCK");
        w.put_file(&store("L"), &[0u8; 2 * 4096]).expect("L");
    }

    fn workload(&self, w: &SimWorld, _l: &Ledger) {
        *lock(&self.run) = AdoptRun::default();
        let (r1, r2, seen, w2) = (
            Arc::clone(&self.run),
            Arc::clone(&self.run),
            Arc::clone(&self.seen),
            w.clone(),
        );
        let one: Body = Box::new(move |v: SimVfs| {
            let root = root_rw(&v);
            let mut client = v.lock_client(&root, LockMode::Acquire).expect("client");
            let f = rw(&v, &root, "L");
            Adopt::append(&v, &mut client, &f, 0, &[0xA1; 4096]);
            lock(&r1).w1_in_flush = true;
            if let Err(e) = v.sync(&f, SyncKind::Data) {
                v.fail_stop(e);
            }
            let mut g = lock(&r1);
            g.w1_in_flush = false;
            g.w1_flushed = true;
        });
        let two: Body = Box::new(move |v: SimVfs| {
            let root = root_rw(&v);
            let mut client = v.lock_client(&root, LockMode::Acquire).expect("client");
            let f = rw(&v, &root, "L");
            Adopt::append(&v, &mut client, &f, 4096, &[0xB2; 512]);
            let during = lock(&r2).w1_in_flush;
            lock(&r2).w2_during = during;
            for _ in 0..4 {
                let mut b = [0u8; 512];
                if v.read_at(&f, 4096, &mut b).is_ok() && b[0] != 0xB2 && during {
                    note(
                        &seen,
                        "another writer's append went back after a flush holder died with a failed flush",
                    );
                }
            }
            if let Err(e) = v.sync(&f, SyncKind::Data) {
                v.fail_stop(e);
            }
            lock(&r2).w2_flushed_at = Some(w2.points());
            // The publish after the covering flush.
            Adopt::append(&v, &mut client, &f, 4096 + 512, &[0xC3; 512]);
        });
        run_tasks(w, vec![("w1".to_owned(), one), ("w2".to_owned(), two)]);
        lock(&self.run).end = w.points();
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let at = w.points();
        {
            let g = lock(&self.run);
            let holder_died = g.w1_in_flush || !g.w1_flushed;
            if w.boot().0 > 1
                && holder_died
                && at < g.end
                && g.w2_flushed_at.is_some_and(|p| p < at)
            {
                note(
                    &self.seen,
                    "a crash after the dead holder's successor flushed, before the run's end",
                );
            }
        }
        let (v, r) = open(w, "recovery", RootAccess::ReadWrite);
        let busy = match v.lock_client(&r, LockMode::Acquire) {
            Ok(mut c) => match v.acquire_within(&mut c, LockByte::Writer, 2_000) {
                Ok(Acquired::Granted(g)) => {
                    v.release(&mut c, g);
                    false
                }
                _ => true,
            },
            Err(_) => true,
        };
        if busy {
            note(&self.seen, "the recovering writer found the byte held");
        }
        // Behind a byte a dead holder keeps beyond every bound, the first reader's boot-change recovery (or the repair
        // its exit 7 calls for) gives up too ([F16] P-60, P-66, P-85; E12).
        let waits = || {
            if busy {
                Err(Refused::new(
                    Refusal::Busy,
                    "the writer byte stays held (exit busy)",
                ))
            } else {
                Ok(EffectSet::new())
            }
        };
        Recovered {
            first_read: waits(),
            state: waits(),
            ..Recovered::same(EffectSet::new())
        }
    }
}

/// The nightly tier's process deaths ([80 §2.4.4], FM-11.2, FM-8.1): a flush holder killed at the other writer's points
/// while it is inside its flush, so that a failed outcome poisons an append made during the flush; lock waiters killed
/// in their wait; crash points after a flush holder's death while its successor flushes; release classes (b) and (c)
/// forced, a recovering writer behind a byte never released allowed to give up, and a seed without them failed.
#[test]
fn nightly_deaths_reach_busy_holders_later_crash_points_and_every_release_class() {
    let only = Dims {
        kills: true,
        ..NO_DIMS
    };
    let mut cfg = nightly_only(only);
    cfg.seeds = vec![1, 2, 3, 4];
    cfg.limits.poison_policies = vec![PoisonPolicy::Revert];
    let subject = Adopt::new();
    let report = enumerate(&subject, &cfg);
    report.assert_passed();
    assert!(
        report.kills_of_holders > 0
            && report.kills_with_crash_points > 0
            && report.crash_points_after_kills > 0,
        "{report}"
    );
    assert!(
        report.release_classes[1] > 0 && report.release_classes[2] > 0 && report.refusals > 0,
        "{report}"
    );
    let seen = lock(&subject.seen).clone();
    for s in [
        "another writer's append went back after a flush holder died with a failed flush",
        "a crash after the dead holder's successor flushed, before the run's end",
        "the recovering writer found the byte held",
    ] {
        assert!(seen.contains(s), "{s}: {seen:?}");
    }
    // Without classes (b) and (c) the nightly tier fails the seed ([F15 §3.8]).
    let mut cfg = nightly_only(only);
    cfg.limits.kill_release_classes = vec![0];
    cfg.limits.kill_crash_points = false;
    let report = enumerate(&Adopt::new(), &cfg);
    assert!(has(&report, "FM-8.1"), "{report}");
}

/// "Flush error, more commits, crash" in the nightly tier ([F15 §6.4]): the crash states after a failed flush are the
/// full ones, and their random states sample poisoned sub-sector mixes (FM-3.3).
#[test]
fn nightly_failed_flushes_are_followed_by_random_poisoned_mixes() {
    let probe = Probe {
        files: vec![("G", vec![0x11; 3 * 4096])],
        workload: Arc::new(|v: SimVfs| {
            let r = root_rw(&v);
            let f = rw(&v, &r, "G");
            v.write_at(&f, 0, &[0x22; 3 * 4096]).expect("write");
            if v.sync(&f, SyncKind::Data).is_err() {
                // Another process's writer goes on appending after the failure (FM-11.3).
                return;
            }
            v.write_at(&f, 4096, &[0x33; 4096]).expect("more");
        }),
        recover: Box::new(|_| {}),
        slots: Vec::new(),
    };
    let mut cfg = nightly_only(Dims {
        flush_errors: true,
        ..NO_DIMS
    });
    cfg.limits.plans.random_per_point = 8;
    let report = enumerate(&probe, &cfg);
    report.assert_passed();
    assert!(
        report.flush_errors == 5 && report.states.get(&Dim::Random).is_some_and(|&n| n > 0),
        "{report}"
    );
    assert!(
        report
            .poison_mixed
            .get(&Dim::Random)
            .is_some_and(|&n| n > 0),
        "{report}"
    );
}

/// A subject whose trace predicate is I-G4's lock-order rule ([F13 §1.4]: no flush while holding the writer byte).
struct Traced {
    /// Flush under the writer byte (the violation).
    flush_under_writer: bool,
    /// Traces checked, those with a system crash, and those with an acknowledgement note.
    counts: Arc<Mutex<(u64, u64, u64)>>,
}

impl Subject for Traced {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("LOCK"), &[0u8; 36 * 1024]).expect("LOCK");
        w.put_file(&store("L"), &[0u8; 4096]).expect("L");
    }

    fn workload(&self, w: &SimWorld, ledger: &Ledger) {
        let (l, under) = (ledger.clone(), self.flush_under_writer);
        let body: Body = Box::new(move |v: SimVfs| {
            let root = root_rw(&v);
            let mut client = v.lock_client(&root, LockMode::Acquire).expect("client");
            let f = rw(&v, &root, "L");
            l.begin(1, Class::Durable, &[(REF, Some(1))]);
            let Ok(Acquired::Granted(g)) = v.acquire_within(&mut client, LockByte::Writer, 60_000)
            else {
                return;
            };
            v.write_at(&f, 0, &[1; 16]).expect("write");
            if under && let Err(e) = v.sync(&f, SyncKind::Data) {
                v.fail_stop(e);
            }
            v.release(&mut client, g);
            if !under && let Err(e) = v.sync(&f, SyncKind::Data) {
                v.fail_stop(e);
            }
            l.ack(1);
        });
        run_tasks(w, vec![("writer".to_owned(), body)]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let (v, r) = open(w, "recovery", RootAccess::Read);
        let set: EffectSet = v
            .open(&r, rel("L"), Access::Read, OpenHint::Normal)
            .ok()
            .and_then(|f| read_file(&v, &f, 1).ok())
            .filter(|b| b.first() == Some(&1))
            .map(|_| [(REF, 1)].into_iter().collect())
            .unwrap_or_default();
        Recovered::same(set)
    }

    fn checks_trace(&self) -> bool {
        true
    }

    fn check_trace(&self, events: &[Event]) -> Vec<String> {
        let writer = LockByte::Writer.offset();
        let mut holding: BTreeSet<u32> = BTreeSet::new();
        let mut out = Vec::new();
        {
            let mut c = lock(&self.counts);
            c.0 += 1;
            c.1 += u64::from(events.iter().any(|e| e.kind == EventKind::Crash));
            c.2 += u64::from(
                events
                    .iter()
                    .any(|e| e.kind == EventKind::Note && e.a == NOTE_ACK),
            );
        }
        for e in events {
            match e.kind {
                EventKind::Granted if e.b == writer => {
                    holding.insert(e.proc);
                }
                EventKind::Released if e.b == writer => {
                    holding.remove(&e.proc);
                }
                EventKind::Crash => holding.clear(),
                EventKind::FlushStart if holding.contains(&e.proc) => {
                    out.push(format!(
                        "I-G4: process {} flushes node {} under the writer byte",
                        e.proc, e.a
                    ));
                }
                _ => {}
            }
        }
        out
    }
}

/// The trace hook ([F13 §1.4], OP-13-02): the subject's trace predicates run after every recovery over the whole
/// replayable trace — the workload with the ledger's acknowledgement notes, the crash, the recovery — and a violation
/// fails the enumeration.
#[test]
fn trace_predicates_run_over_every_recovered_worlds_trace() {
    let only = Dims {
        crash_points: true,
        kills: true,
        ..NO_DIMS
    };
    let good = Traced {
        flush_under_writer: false,
        counts: Arc::default(),
    };
    let report = enumerate(&good, &pr(&[1], only));
    report.assert_passed();
    let (checked, crashed, acked) = *lock(&good.counts);
    assert_eq!(checked, report.recoveries, "{report}");
    assert!(crashed > 0 && acked > 0, "{checked} {crashed} {acked}");
    let bad = Traced {
        flush_under_writer: true,
        counts: Arc::default(),
    };
    let report = enumerate(&bad, &pr(&[1], only));
    assert!(has(&report, "trace: I-G4"), "{report}");
}

/// How [`Sealed`]'s recovery answers a truncated sealed file.
#[derive(Copy, Clone)]
struct SealedRecovery {
    /// `doctor --fsck` names the file.
    diagnose: bool,
    /// `repair --rebuild-from-log` restores it (it is derived).
    repair: bool,
    /// The diagnosis also names `other`, a file the state references that nothing touched (E6: a diagnosis with no
    /// cause).
    blame_other: bool,
}

/// A sealed file `Z` that a reader maps once and reads through its mapping several times, with other calls between;
/// an acknowledged effect the store holds from the start. Its recovery refuses a truncated `Z` at the first read (exit
/// 7) and, as configured, names it and repairs it.
struct Sealed {
    how: SealedRecovery,
    seen: Seen,
    reading: Arc<Mutex<(bool, bool)>>,
}

const KEPT: EffectKey = EffectKey::new(EffectKind::Commit, 5);

impl Sealed {
    fn short(v: &SimVfs, r: &SimRoot) -> bool {
        v.open(r, rel("Z"), Access::Read, OpenHint::Normal)
            .ok()
            .and_then(|f| v.file_size(&f).ok())
            .is_some_and(|n| n < 4096)
    }
}

impl Subject for Sealed {
    fn setup(&self, w: &SimWorld, ledger: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("Z"), &[0x5A; 4096]).expect("Z");
        w.put_file(&store("other"), &[0; 512]).expect("other");
        ledger.begin(5, Class::Durable, &[(KEPT, Some(55))]);
        ledger.ack(5);
    }

    fn workload(&self, w: &SimWorld, _l: &Ledger) {
        *lock(&self.reading) = (false, false);
        let (seen, reading) = (Arc::clone(&self.seen), Arc::clone(&self.reading));
        let body: Body = Box::new(move |v: SimVfs| {
            let r = root_rw(&v);
            let f = rw(&v, &r, "Z");
            v.seal(&f).expect("seal");
            drop(f);
            let other = rw(&v, &r, "other");
            let z = v
                .open(&r, rel("Z"), Access::Read, OpenHint::Normal)
                .expect("Z");
            let Ok(m) = v.map_sealed(&z, 4096, rel("Z")) else {
                note(&seen, "the mapping refused a truncated file (exit 7)");
                return;
            };
            lock(&reading).0 = true;
            for _ in 0..3 {
                let mut b = [0u8; 16];
                let _ = v.read_at(&other, 0, &mut b);
                if m.bytes().iter().any(|&x| x != 0x5A) {
                    note(&seen, "the mapped read saw zeros; the reader exits 7");
                    return;
                }
            }
            lock(&reading).1 = true;
        });
        run_tasks(w, vec![("reader".to_owned(), body)]);
        let (mapped, done) = *lock(&self.reading);
        if mapped && !done {
            note(&self.seen, "the reader died at its mapped read");
        }
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let kept: EffectSet = [(KEPT, 55)].into_iter().collect();
        let (v, r) = open(w, "recovery", RootAccess::ReadWrite);
        if !Sealed::short(&v, &r) {
            return Recovered::same(kept);
        }
        note(&self.seen, "the recovery found the sealed file truncated");
        let damaged = |m: &str| Refused::new(Refusal::Damaged(store("Z")), m);
        let (state, answered) = if self.how.repair {
            let z = rw_unsealed(&v, &r);
            (
                z.map(|()| kept.clone()),
                vec![damaged("Z is damaged: exit 7, run repair")],
            )
        } else {
            (Err(damaged("Z is damaged: exit 7, run repair")), Vec::new())
        };
        Recovered {
            first_read: Err(damaged("Z is damaged: exit 7")),
            state,
            answered,
            findings: Vec::new(),
            diagnosed: [
                (self.how.diagnose, store("Z")),
                (self.how.blame_other, store("other")),
            ]
            .into_iter()
            .filter(|(named, _)| *named)
            .map(|(_, p)| Diagnosis::referenced(p))
            .collect(),
        }
    }
}

/// `repair` of `Z`: removed and rebuilt from its source, sealed again.
fn rw_unsealed(v: &SimVfs, r: &SimRoot) -> Result<(), Refused> {
    v.unlink(r, rel("Z"), ShareRetry::None)
        .map_err(|e| write_refused("repair: unlink", &e))?;
    let f = v
        .create_new(r, rel("Z"))
        .map_err(|e| write_refused("repair: create", &e))?;
    v.write_at(&f, 0, &[0x5A; 4096])
        .map_err(|e| write_refused("repair: write", &e))?;
    if let Err(e) = v.sync(&f, SyncKind::DataAndMeta) {
        v.fail_stop(e);
    }
    v.seal(&f).map_err(|e| write_refused("repair: seal", &e))?;
    if let Err(e) = v.sync_dir(r, None) {
        v.fail_stop(e);
    }
    Ok(())
}

/// External truncation of a sealed file while its reader holds a mapping ([80 §2.5] rule 8, FM-10.2, FM-9): the reader
/// dies at its mapped read or sees zeros and exits 7; the first read after may refuse, but the writer's recovery (with
/// `repair`) must keep every acknowledged commit, and its diagnosis must name the file.
#[test]
fn a_sealed_file_truncated_while_read_is_refused_repaired_and_named() {
    let only = Dims {
        read_faults: true,
        ..NO_DIMS
    };
    let good = Sealed {
        how: SealedRecovery {
            diagnose: true,
            repair: true,
            blame_other: false,
        },
        seen: Arc::default(),
        reading: Arc::default(),
    };
    let report = enumerate(&good, &pr(&[1, 2], only));
    report.assert_passed();
    assert!(
        report.truncations > 2 && report.diagnoses > 0 && report.refusals > 0,
        "{report}"
    );
    let seen = lock(&good.seen).clone();
    assert!(
        seen.contains("the reader died at its mapped read")
            || seen.contains("the mapped read saw zeros; the reader exits 7"),
        "{seen:?}"
    );
    assert!(
        seen.contains("the recovery found the sealed file truncated"),
        "{seen:?}"
    );
    // A diagnosis that does not name the file fails.
    let unnamed = Sealed {
        how: SealedRecovery {
            diagnose: false,
            repair: true,
            blame_other: false,
        },
        seen: Arc::default(),
        reading: Arc::default(),
    };
    let report = enumerate(&unnamed, &pr(&[1], only));
    assert!(has(&report, "diagnosis: the sealed file"), "{report}");
    // A writer that refuses instead of repairing fails: only the readers may refuse.
    let refusing = Sealed {
        how: SealedRecovery {
            diagnose: true,
            repair: false,
            blame_other: false,
        },
        seen: Arc::default(),
        reading: Arc::default(),
    };
    let report = enumerate(&refusing, &pr(&[1], only));
    assert!(has(&report, "recovery: refused"), "{report}");
    // E6 ([F15] G-13): a diagnosis that also names `other`, which the state references and no fault touched, fails;
    // the truncated `Z`, rebuilt by the repair as a new file, is still known by its path.
    let blaming = Sealed {
        how: SealedRecovery {
            diagnose: true,
            repair: true,
            blame_other: true,
        },
        seen: Arc::default(),
        reading: Arc::default(),
    };
    let report = enumerate(&blaming, &pr(&[1], only));
    assert!(
        has(&report, "diagnosis: the recovery names /sim/store")
            && report
                .failures
                .iter()
                .flat_map(|f| f.messages.iter())
                .filter(|m| m.starts_with("diagnosis: the recovery names"))
                .all(|m| m.contains("other")),
        "{report}"
    );
}

/// One file sector: a first operation publishes sub-sector 0 without a flush (lazy, or, broken, a durable one that is
/// never acknowledged), then a durable group appends sub-sector 1 and flushes. The recovery's first read and its writer
/// read the file separately, so a poisoned sector may show its newest bytes to the first and its oldest to the second
/// (FM-3.2).
struct LazyTail {
    class: Class,
}

const LAZY: EffectKey = EffectKey::new(EffectKind::Commit, 1);

impl LazyTail {
    fn read(w: &SimWorld, who: &str) -> EffectSet {
        let (v, r) = open(w, who, RootAccess::Read);
        let mut b = [0u8; 1];
        let seen = v
            .open(&r, rel("F"), Access::Read, OpenHint::Normal)
            .ok()
            .is_some_and(|f| v.read_at(&f, 0, &mut b).is_ok() && b[0] == 7);
        if seen {
            [(LAZY, 7)].into_iter().collect()
        } else {
            EffectSet::new()
        }
    }
}

impl Subject for LazyTail {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("F"), &[0u8; 4096]).expect("F");
    }

    fn workload(&self, w: &SimWorld, ledger: &Ledger) {
        let (l, class) = (ledger.clone(), self.class);
        let body: Body = Box::new(move |v: SimVfs| {
            let r = root_rw(&v);
            let f = rw(&v, &r, "F");
            l.begin(1, class, &[(LAZY, Some(7))]);
            v.write_at(&f, 0, &[7; 512]).expect("publish");
            if class == Class::Lazy {
                l.ack(1);
            }
            l.begin(2, Class::Durable, &[(REF, Some(9))]);
            v.write_at(&f, 512, &[9; 512]).expect("append");
            if let Err(e) = v.sync(&f, SyncKind::Data) {
                v.fail_stop(e);
            }
        });
        run_tasks(w, vec![("writer".to_owned(), body)]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        Recovered {
            first_read: Ok(LazyTail::read(w, "first-reader")),
            state: Ok(LazyTail::read(w, "writer")),
            findings: Vec::new(),
            answered: Vec::new(),
            diagnosed: Vec::new(),
        }
    }
}

/// Read freshness after a failed flush with no crash ([F15 FM-3.2, FM-3.6]; review pass 1): an acknowledged lazy record
/// that the first reader sees and the recovering writer no longer does (pages evicted after the failure) is no
/// violation; a pending durable value seen and then dropped is.
#[test]
fn a_lazy_record_may_vanish_between_reads_after_a_failed_flush_but_a_durable_one_may_not() {
    let mut cfg = pr(
        &[1],
        Dims {
            flush_errors: true,
            ..NO_DIMS
        },
    );
    cfg.limits.poison_policies = vec![PoisonPolicy::Evict];
    let report = enumerate(&LazyTail { class: Class::Lazy }, &cfg);
    report.assert_passed();
    assert!(
        report.flush_errors > 0 && report.poisoned_reads > 0,
        "{report}"
    );
    let report = enumerate(
        &LazyTail {
            class: Class::Durable,
        },
        &cfg,
    );
    assert!(has(&report, "read freshness"), "{report}");
    assert!(
        report
            .failures
            .iter()
            .any(|f| f.crash.is_none() && matches!(f.variant, Variant::FlushError { .. })),
        "the death-only check after the failed flush: {report}"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Spec sync 2b: the second way to leave no valid slot (S2B-P-27), fields kept only in `HEAD` (S2B-P-14) and
// Unknown-boot readers (S2B-P-18).

/// A whole-sector `HEAD` slot like [F04 §3]'s: `seq` at 0 and 2048, `seq`'s low byte elsewhere, and a check over
/// bytes [0, 4088) at 4088, so that a slot torn between two versions, or partly written, fails validation.
fn sector_slot(seq: u64) -> Vec<u8> {
    let mut b = vec![seq.to_le_bytes()[0]; 4096];
    b[0..8].copy_from_slice(&seq.to_le_bytes());
    b[2048..2056].copy_from_slice(&seq.to_le_bytes());
    let check = fnv(SLOT_SEED, &b[..4088]);
    b[4088..].copy_from_slice(&check.to_le_bytes());
    b
}

/// The valid whole-sector slots of a two-slot `HEAD`: their `seq`.
fn sector_slots(head: &[u8]) -> Vec<u64> {
    (0..2)
        .filter_map(|s| head.get(s * 4096..(s + 1) * 4096))
        .filter(|b| u64_at(b, 4088) == fnv(SLOT_SEED, &b[..4088]) && u64_at(b, 0) > 0)
        .map(|b| u64_at(b, 0))
        .collect()
}

/// A two-slot `HEAD` whose durable publish ([F04 §9.2]) writes slot B, then slot A, then flushes; both slots are valid
/// and durable before it. Its recovery notes every state that has no valid slot, with the failed flushes behind it.
fn durable_publish_probe(seen: &Seen) -> Probe {
    let mut head = sector_slot(2);
    head.extend(sector_slot(1));
    let s = Arc::clone(seen);
    Probe {
        files: vec![("H", head)],
        workload: Arc::new(|v: SimVfs| {
            let r = root_rw(&v);
            let h = rw(&v, &r, "H");
            // Each publish writes the slot that does not hold the newest valid state: B (seq 3) over seq 1, then A
            // (seq 4) over seq 2.
            for (at, seq) in [(4096, 3), (0, 4)] {
                if let Err(e) = v.write_at(&h, at, &sector_slot(seq)) {
                    abort(&v, &e);
                }
            }
            if let Err(e) = v.sync(&h, SyncKind::DataAndMeta) {
                v.fail_stop(e);
            }
        }),
        recover: Box::new(move |w| {
            let h = content(w, "H").unwrap_or_default();
            if sector_slots(&h).is_empty() {
                note(
                    &s,
                    format!("no valid slot, failed flushes {}", w.failed_flushes()),
                );
            }
        }),
        slots: vec![store("H")],
    }
}

/// [F04 §8.1] "Both slots absent", [F15 §6.4] (spec sync 2b S2B-P-27): a durable publish's second slot write that fails
/// with `DiskFull` (FM-5.2) or is cut by its writer's death (§2.5) leaves slot A any mix of bytes; a crash that tears
/// slot B, which the first write left dirty, then leaves no valid slot although no flush failed. The PR tier reaches the
/// state at the end of each such run (the slot states over the pivots).
#[test]
fn a_cut_slot_write_and_a_torn_dirty_slot_leave_no_valid_slot_without_a_failed_flush() {
    for dims in [
        Dims {
            disk_full: true,
            ..NO_DIMS
        },
        Dims {
            kills: true,
            ..NO_DIMS
        },
    ] {
        let seen: Seen = Arc::default();
        let report = enumerate(&durable_publish_probe(&seen), &pr(&[1], dims));
        report.assert_passed();
        let seen = lock(&seen).clone();
        assert!(
            seen.contains("no valid slot, failed flushes 0"),
            "{dims:?}: {seen:?}\n{report}"
        );
        assert!(report.slot_fault_runs > 0, "{report}");
        assert!(
            report.states.get(&Dim::Slot).is_some_and(|&n| n > 0),
            "{report}"
        );
    }
}

/// A durable publish of a flag kept only in `HEAD` ([F04 §6]), which its writer reads back before the `HEAD` flush and
/// acknowledges after it; `head`: reported by [`Ledger::begin_kept_in_head`], else as an ordinary durable operation.
struct HeadFlag {
    head: bool,
}

const FLAG: EffectKey = EffectKey::new(EffectKind::Other(3), 1);

impl HeadFlag {
    fn read(w: &SimWorld, who: &str) -> EffectSet {
        let (v, r) = open(w, who, RootAccess::Read);
        let mut b = [0u8; 1];
        let set = v
            .open(&r, rel("H"), Access::Read, OpenHint::Normal)
            .ok()
            .is_some_and(|f| v.read_at(&f, 0, &mut b).is_ok() && b[0] == 1);
        if set {
            [(FLAG, 1)].into_iter().collect()
        } else {
            EffectSet::new()
        }
    }
}

impl Subject for HeadFlag {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("H"), &[0u8; 4096]).expect("H");
    }

    fn workload(&self, w: &SimWorld, ledger: &Ledger) {
        let (l, head) = (ledger.clone(), self.head);
        let body: Body = Box::new(move |v: SimVfs| {
            let r = root_rw(&v);
            let h = rw(&v, &r, "H");
            if head {
                l.begin_kept_in_head(1, &[(FLAG, Some(1))]);
            } else {
                l.begin(1, Class::Durable, &[(FLAG, Some(1))]);
            }
            if let Err(e) = v.write_at(&h, 0, &[1; 512]) {
                abort(&v, &e);
            }
            // A reader sees the published flag before the HEAD flush.
            let mut b = [0u8; 1];
            if v.read_at(&h, 0, &mut b).is_ok() && b[0] == 1 {
                l.observe(FLAG, Some(1));
            }
            if let Err(e) = v.sync(&h, SyncKind::DataAndMeta) {
                v.fail_stop(e);
            }
            l.ack(1);
        });
        run_tasks(w, vec![("writer".to_owned(), body)]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        Recovered {
            first_read: Ok(HeadFlag::read(w, "first-reader")),
            state: Ok(HeadFlag::read(w, "writer")),
            findings: Vec::new(),
            answered: Vec::new(),
            diagnosed: Vec::new(),
        }
    }
}

/// [F15 §3.3] FM-3.6 "Fields kept only in `HEAD`" (spec sync 2b S2B-P-14): after the `HEAD` flush failed, the
/// unacknowledged flag that a reader saw may read back old, and a crash may leave either value; the same history
/// reported as an ordinary durable operation fails, since a seen durable record must survive (I-G2).
#[test]
fn an_unacknowledged_head_only_flag_may_vanish_after_its_head_flush_failed() {
    let mut cfg = pr(
        &[1],
        Dims {
            flush_errors: true,
            ..NO_DIMS
        },
    );
    cfg.limits.poison_policies = vec![PoisonPolicy::Evict, PoisonPolicy::Revert];
    let report = enumerate(&HeadFlag { head: true }, &cfg);
    report.assert_passed();
    assert!(
        report.flush_errors > 0 && report.poisoned_reads > 0,
        "{report}"
    );
    let report = enumerate(&HeadFlag { head: false }, &cfg);
    assert!(has(&report, "op 1"), "{report}");
    // The clean run's crash states: before the acknowledgement a crash may revert the unflushed flag that the reader
    // saw (FM-1.1) when it is kept only in HEAD.
    let crash_only = pr(
        &[1],
        Dims {
            crash_points: true,
            ..NO_DIMS
        },
    );
    enumerate(&HeadFlag { head: true }, &crash_only).assert_passed();
    let report = enumerate(&HeadFlag { head: false }, &crash_only);
    assert!(has(&report, "op 1"), "{report}");
}

/// A store with one acknowledged durable record whose recovery's first reader always shows the empty store: a reader
/// that lags.
struct Lagging {
    boot: BootMode,
}

impl Subject for Lagging {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("F"), &[0u8; 4096]).expect("F");
    }

    fn workload(&self, w: &SimWorld, ledger: &Ledger) {
        let l = ledger.clone();
        let body: Body = Box::new(move |v: SimVfs| {
            let r = root_rw(&v);
            let f = rw(&v, &r, "F");
            l.begin(1, Class::Durable, &[(REF, Some(7))]);
            if let Err(e) = v.write_at(&f, 0, &[7; 512]) {
                abort(&v, &e);
            }
            if let Err(e) = v.sync(&f, SyncKind::Data) {
                v.fail_stop(e);
            }
            l.ack(1);
        });
        run_tasks(w, vec![("writer".to_owned(), body)]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        self.recover_with_boot(w).0
    }

    fn recover_with_boot(&self, w: &SimWorld) -> (Recovered, BootMode) {
        let (v, r) = open(w, "writer", RootAccess::Read);
        let mut b = [0u8; 1];
        let kept = v
            .open(&r, rel("F"), Access::Read, OpenHint::Normal)
            .ok()
            .is_some_and(|f| v.read_at(&f, 0, &mut b).is_ok() && b[0] == 7);
        let state: EffectSet = if kept {
            [(REF, 7)].into_iter().collect()
        } else {
            EffectSet::new()
        };
        let rec = Recovered {
            first_read: Ok(EffectSet::new()),
            state: Ok(state),
            findings: Vec::new(),
            answered: Vec::new(),
            diagnosed: Vec::new(),
        };
        (rec, self.boot)
    }
}

/// [F13 §3.8] "I-G2's post-crash clause, read precisely", "Boot mode" (spec sync 2b S2B-P-18): after a crash, an
/// Unknown-boot reader's first read is judged for consistency only ([`Subject::recover_with_boot`]), a Known-boot
/// reader's for completeness too.
#[test]
fn an_unknown_boot_readers_lagging_first_read_passes_and_a_known_boot_readers_fails() {
    let cfg = pr(
        &[1],
        Dims {
            crash_points: true,
            ..NO_DIMS
        },
    );
    let report = enumerate(
        &Lagging {
            boot: BootMode::Unknown,
        },
        &cfg,
    );
    report.assert_passed();
    assert!(report.unknown_boot_reads > 0, "{report}");
    let report = enumerate(
        &Lagging {
            boot: BootMode::Known,
        },
        &cfg,
    );
    assert!(has(&report, "first read"), "{report}");
    assert_eq!(report.unknown_boot_reads, 0);
    // The default hook answers Known.
    let probe = Probe {
        files: Vec::new(),
        workload: Arc::new(|_| {}),
        recover: Box::new(|_| {}),
        slots: Vec::new(),
    };
    let world = SimWorld::new(SimConfig::new(1));
    assert_eq!(probe.recover_with_boot(&world).1, BootMode::Known);
}
