//! The enumerator's own families at M0 ([F13 §1.4] "The toy vehicle", [F16 §17.2] "Where the detectors live"): a
//! store that follows the protocol of [F16] — group commit through the flush and writer bytes, a two-slot `HEAD`
//! published read-modify-write, boot-change recovery and plain repair — whose every run the trace predicates judge, and
//! one regression per item of WP-40's S4 disposition (E1–E13) that the enumerator now owns; and the regressions of
//! WP-40's closure check: a store that rotates through extent preparations (I-G4, [F16] P-72, P-96), and setup faults
//! that explain a refusal only through their file (avail, [F15] FM-10).
//!
//! The subjects are the enumerator author's own, written from [F16] and the fault model; they are not the toy log and
//! carry none of its seeded bugs (S4).

mod common;

use std::path::{Path, PathBuf};

use common::{STORE, rel};
use moirai_vfs::{
    Access, Acquired, Classification, ClassifyDepth, EnvGuard, LockByte, LockMode, Locks, OpenHint,
    ProbeResult, QuietIndex, RootAccess, RootRole, ShareRetry, StoreFs, StoreVolume, SyncKind,
    VfsError, VfsErrorKind,
};
use moirai_vfs_sim::enumerate::{
    Class, Dims, EffectKey, EffectKind, EffectSet, EffectWrite, EnumConfig, Ledger,
    MAINT_AUTOMATIC, MAINT_BELOW_CAP, NOTE_MAINT_DECISION, NOTE_PHASE, Protocol, Recovered,
    Refusal, Refused, Report, SlotDecode, SlotView, Subject, Tier, enumerate,
};
use moirai_vfs_sim::{CallKind, SimClient, SimFile, SimRoot, SimVfs, SimWorld, TaskEnd};

// ---------------------------------------------------------------------------------------------------------------------
// Helpers

fn store(name: &str) -> PathBuf {
    Path::new(STORE).join(name)
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

fn has(report: &Report, needle: &str) -> bool {
    report
        .failures
        .iter()
        .any(|f| f.messages.iter().any(|m| m.contains(needle)))
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

/// The body of one simulated task.
type Body = Box<dyn FnOnce(SimVfs) + Send>;

/// Starts one process (Known-boot) per entry with its tasks, runs them, and turns a panicked task into a panic of the
/// workload.
fn run_procs(w: &SimWorld, procs: Vec<(String, Vec<Body>)>) {
    let mut tasks = Vec::new();
    for (name, bodies) in procs {
        let p = w.process_with(&name, None, Some(true));
        for b in bodies {
            tasks.push(w.spawn(&p, b));
        }
    }
    let _ = w.run();
    for t in tasks {
        if let Some(TaskEnd::Panicked(m)) = t.end() {
            panic!("a task panicked: {m}");
        }
    }
}

fn read(v: &SimVfs, f: &SimFile, offset: u64, len: u64) -> Result<Vec<u8>, VfsError> {
    let mut b = vec![0u8; len as usize];
    let n = v.read_at(f, offset, &mut b)?;
    b.truncate(n);
    Ok(b)
}

fn read_refused(name: &str, e: &VfsError) -> Refused {
    let reason = if e.kind == VfsErrorKind::Io {
        Refusal::IoFault
    } else {
        Refusal::Damaged(store(name))
    };
    Refused::new(reason, format!("{name}: {e:?}"))
}

fn write_refused(e: &VfsError) -> Refused {
    let reason = if e.kind == VfsErrorKind::DiskFull {
        Refusal::DiskFull
    } else {
        Refusal::Io
    };
    Refused::new(reason, format!("exit 7: {e:?}"))
}

// ---------------------------------------------------------------------------------------------------------------------
// Proto: a log store that follows [F16]'s protocol.

const LOG: &str = "log.1";
const LOG_LEN: u64 = 4 * 4096;
const HEAD_LEN: u64 = 2 * 4096;
const SLOT_SEED: u64 = 0x5052_4F54_534C_4F54;
const CHAIN_SEED: u64 = 0x5052_4F54_4348_4149;
const GROUP_MAGIC: u32 = 0x544F_5250;
/// A group's length: groups straddle sectors and sub-sectors.
const GROUP_LEN: usize = 600;
const REF: EffectKey = EffectKey::new(EffectKind::Ref, 7);

/// A slot ([F04 §3] reduced): `slot_seq` at 0, `committed_lsn` at 8, `durable_lsn` at 16, `boot_id` at 24 (never
/// changed: the subject runs no boot-change recovery of `boot_id`), and a check over `[0, 4088)` at 4088.
fn slot(seq: u64, committed: u64, durable: u64) -> Vec<u8> {
    let mut b = vec![0u8; 4096];
    b[0..8].copy_from_slice(&seq.to_le_bytes());
    b[8..16].copy_from_slice(&committed.to_le_bytes());
    b[16..24].copy_from_slice(&durable.to_le_bytes());
    let c = fnv(SLOT_SEED, &b[..4088]);
    b[4088..].copy_from_slice(&c.to_le_bytes());
    b
}

/// The subject's pure slot decoder ([`Protocol::decode_slot`]): absent on a check mismatch or `slot_seq` 0, fatal
/// when `committed_lsn` < `durable_lsn` ([F04 §7] check 5).
fn decode(b: &[u8]) -> SlotDecode {
    if b.len() < 4096 || u64_at(b, 4088) != fnv(SLOT_SEED, &b[..4088]) || u64_at(b, 0) == 0 {
        return SlotDecode::Absent;
    }
    let (committed, durable) = (u64_at(b, 8), u64_at(b, 16));
    if committed < durable {
        return SlotDecode::Fatal;
    }
    let mut boot = [0u8; 16];
    boot.copy_from_slice(&b[24..40]);
    SlotDecode::Valid(SlotView {
        slot_seq: u64_at(b, 0),
        committed_lsn: committed,
        durable_lsn: durable,
        boot_id: boot,
        monotone: Vec::new(),
    })
}

/// The newest valid slot of a `HEAD` image ([F04 §8.1]), or the refusal: a fatal slot, or none valid.
fn newest(head: &[u8]) -> Result<(usize, SlotView), Refusal> {
    let mut best: Option<(usize, SlotView)> = None;
    for s in 0..2 {
        match head.get(s * 4096..(s + 1) * 4096).map(decode) {
            Some(SlotDecode::Fatal) => return Err(Refusal::FatalSlot),
            Some(SlotDecode::Valid(v))
                if best.as_ref().is_none_or(|(_, b)| v.slot_seq > b.slot_seq) =>
            {
                best = Some((s, v));
            }
            _ => {}
        }
    }
    best.ok_or(Refusal::NoValidSlot)
}

/// One group: magic, length, op, value, padding, and its chain trailer seeded by the predecessor's ([F05 §4.2]).
fn group(op: u64, prev: u64) -> Vec<u8> {
    let mut b = vec![(op as u8) | 1; GROUP_LEN];
    b[0..4].copy_from_slice(&GROUP_MAGIC.to_le_bytes());
    b[4..8].copy_from_slice(&(GROUP_LEN as u32).to_le_bytes());
    b[8..16].copy_from_slice(&op.to_le_bytes());
    let c = fnv(prev, &b[..GROUP_LEN - 8]);
    b[GROUP_LEN - 8..].copy_from_slice(&c.to_le_bytes());
    b
}

fn chain_at(log: &[u8], at: usize) -> u64 {
    if at == 0 {
        CHAIN_SEED
    } else {
        u64_at(log, at - 8)
    }
}

/// The valid groups by the chain rule ([F16] P-53): each one's end and op; and the end of the valid log.
fn scan(log: &[u8]) -> (usize, Vec<(usize, u64)>) {
    let (mut at, mut out) = (0usize, Vec::new());
    while at + GROUP_LEN <= log.len()
        && u32::from_le_bytes(log[at..at + 4].try_into().expect("4")) == GROUP_MAGIC
    {
        let end = at + GROUP_LEN;
        if fnv(chain_at(log, at), &log[at..end - 8]) != u64_at(log, end - 8) {
            break;
        }
        out.push((end, u64_at(log, at + 8)));
        at = end;
    }
    (at, out)
}

fn writes_of(op: u64) -> Vec<EffectWrite> {
    vec![
        (
            EffectKey::new(EffectKind::Commit, op),
            Some(fnv(1, &op.to_le_bytes())),
        ),
        (REF, Some(op)),
    ]
}

fn fold(groups: &[(usize, u64)], upto: usize) -> EffectSet {
    let mut set = EffectSet::new();
    for &(_, op) in groups.iter().filter(|g| g.0 <= upto) {
        for (k, v) in writes_of(op) {
            if let Some(v) = v {
                set.insert(k, v);
            }
        }
    }
    set
}

/// Protocol breaks, each of which one of the enumerator's families must catch.
#[derive(Copy, Clone, Debug, Default)]
struct Breaks {
    /// E1 (P-4): the flush holder publishes after it released the writer byte.
    publish_unlocked: bool,
    /// E2 (P-2): the flush holder flushes the log while it still holds the writer byte.
    flush_under_writer: bool,
    /// E4 (I-G4): the flush holder flushes without the flush byte.
    flush_without_byte: bool,
    /// E11 (I-G2): an appender reads its own group before any flush covers it.
    read_uncovered: bool,
    /// E9 ([F03 §3.1] rule 2): the maintenance decider probes only the first quiet byte.
    partial_probe: bool,
    /// E5 (avail): a writer refuses `store_corrupt` with nothing to explain it.
    refuse_corrupt: bool,
    /// E8 (I-G3): the recovering writer overwrites the first group's trailer.
    overwrite_acked: bool,
    /// E7 (avail): the first reader reports no valid slot.
    claim_no_slot: bool,
}

#[derive(Clone, Debug, Default)]
struct Proto {
    breaks: Breaks,
}

/// A writing task's lock client and its handles on the log and `HEAD`.
struct Handles {
    client: SimClient,
    log: SimFile,
    head: SimFile,
}

impl Handles {
    fn open(v: &SimVfs, root: &SimRoot) -> Handles {
        Handles {
            client: v.lock_client(root, LockMode::Acquire).expect("lock client"),
            log: v
                .open(root, rel(LOG), Access::ReadWrite, OpenHint::Normal)
                .expect("log"),
            head: v
                .open(root, rel("HEAD"), Access::ReadWrite, OpenHint::Normal)
                .expect("HEAD"),
        }
    }
}

fn root(v: &SimVfs, access: RootAccess) -> SimRoot {
    v.open_root(Path::new(STORE), RootRole::Store, access)
        .expect("root")
}

impl Proto {
    /// One write: phase 1, phase 2a (append under the writer byte), phase 2b (the flush holder's re-write, flush and
    /// publish), the identity check, phase 3 ([F16 §5]).
    fn commit(&self, v: &SimVfs, h: &mut Handles, l: &Ledger, who: &str, op: u64) -> bool {
        l.begin(op, Class::Durable, &writes_of(op));
        v.note(NOTE_PHASE, 1, 0);
        if self.breaks.refuse_corrupt && op % 10 == 2 {
            l.refused(who, Refused::new(Refusal::Corrupt, "store_corrupt"));
            return false;
        }
        // Phase 2a (P-27–P-37).
        let Ok(Acquired::Granted(gw)) = v.acquire_within(&mut h.client, LockByte::Writer, 60_000)
        else {
            l.refused(who, Refused::new(Refusal::Busy, "store_locked"));
            return false;
        };
        let lb = match read(v, &h.log, 0, LOG_LEN) {
            Ok(b) => b,
            Err(e) => {
                v.release(&mut h.client, gw);
                l.refused(who, read_refused(LOG, &e));
                return false;
            }
        };
        let (ev, _) = scan(&lb);
        if ev + GROUP_LEN > LOG_LEN as usize {
            v.release(&mut h.client, gw);
            return false;
        }
        let g = group(op, chain_at(&lb, ev));
        if let Err(e) = v.write_at(&h.log, ev as u64, &g) {
            v.release(&mut h.client, gw);
            l.refused(who, write_refused(&e));
            return false;
        }
        let end = ev + GROUP_LEN;
        let chain = g[GROUP_LEN - 8..].to_vec();
        if self.breaks.read_uncovered
            && let Ok(hb) = read(v, &h.head, 0, HEAD_LEN)
            && let Ok((_, s)) = newest(&hb)
        {
            for (k, val) in writes_of(op) {
                l.observe_covered(k, val, end as u64, s.durable_lsn);
            }
        }
        v.release(&mut h.client, gw);
        // Phase 2b (P-40–P-45).
        if !self.make_durable(v, h, l, who, end) {
            return false;
        }
        // P-46: acknowledgement by identity.
        match read(v, &h.log, end as u64 - 8, 8) {
            Ok(b) if b == chain => {}
            Ok(_) => {
                l.refused(
                    who,
                    Refused::new(Refusal::OutcomeUnknown, "outcome_unknown"),
                );
                return false;
            }
            Err(e) => {
                l.refused(who, read_refused(LOG, &e));
                return false;
            }
        }
        l.group_bytes(op, &store(LOG), end as u64 - 8, &chain);
        l.ack(op);
        v.note(NOTE_PHASE, 3, 0);
        true
    }

    /// Phase 2b: covered, or flushed by this process as the flush holder (P-40–P-45).
    fn make_durable(&self, v: &SimVfs, h: &mut Handles, l: &Ledger, who: &str, end: usize) -> bool {
        let b = self.breaks;
        let gf = if b.flush_without_byte {
            None
        } else {
            match v.acquire_within(&mut h.client, LockByte::Flush, 60_000) {
                Ok(Acquired::Granted(g)) => Some(g),
                _ => {
                    l.refused(who, Refused::new(Refusal::Busy, "outcome_pending"));
                    return false;
                }
            }
        };
        let release_flush = |v: &SimVfs, c: &mut SimClient, gf| {
            if let Some(g) = gf {
                v.release(c, g);
            }
        };
        let Ok(Acquired::Granted(gw)) = v.acquire_within(&mut h.client, LockByte::Writer, 60_000)
        else {
            release_flush(v, &mut h.client, gf);
            l.refused(who, Refused::new(Refusal::Busy, "store_locked"));
            return false;
        };
        let state = read(v, &h.head, 0, HEAD_LEN)
            .map_err(|e| read_refused("HEAD", &e))
            .and_then(|hb| newest(&hb).map_err(|r| Refused::new(r, "exit 7")));
        let s = match state {
            Ok((_, s)) => s,
            Err(r) => {
                v.release(&mut h.client, gw);
                release_flush(v, &mut h.client, gf);
                l.refused(who, r);
                return false;
            }
        };
        if s.durable_lsn as usize >= end {
            v.release(&mut h.client, gw);
            release_flush(v, &mut h.client, gf);
            return true;
        }
        // P-42, P-43: scan and re-write (durable_lsn, E] under the writer byte.
        let lb = match read(v, &h.log, 0, LOG_LEN) {
            Ok(b) => b,
            Err(e) => {
                v.release(&mut h.client, gw);
                release_flush(v, &mut h.client, gf);
                l.refused(who, read_refused(LOG, &e));
                return false;
            }
        };
        let (e, _) = scan(&lb);
        if e < end {
            // P-47: the group was lost before it was durable (another process's failed flush).
            v.release(&mut h.client, gw);
            release_flush(v, &mut h.client, gf);
            l.refused(
                who,
                Refused::new(Refusal::OutcomeUnknown, "outcome_unknown"),
            );
            return false;
        }
        let d = s.durable_lsn as usize;
        if let Err(err) = v.write_at(&h.log, d as u64, &lb[d..e]) {
            v.release(&mut h.client, gw);
            release_flush(v, &mut h.client, gf);
            l.refused(who, write_refused(&err));
            return false;
        }
        // P-44: one flush, outside the writer byte; an error ends the process.
        let gw = if b.flush_under_writer {
            Some(gw)
        } else {
            v.release(&mut h.client, gw);
            None
        };
        if let Err(f) = v.sync(&h.log, SyncKind::Data) {
            v.fail_stop(f);
        }
        let gw = match gw {
            Some(g) => g,
            None => match v.acquire_within(&mut h.client, LockByte::Writer, 60_000) {
                Ok(Acquired::Granted(g)) => g,
                _ => {
                    release_flush(v, &mut h.client, gf);
                    l.refused(who, Refused::new(Refusal::Busy, "store_locked"));
                    return false;
                }
            },
        };
        // P-45, P-48: the publish, a read-modify-write of the newest valid slot.
        let state = read(v, &h.head, 0, HEAD_LEN)
            .map_err(|e| read_refused("HEAD", &e))
            .and_then(|hb| newest(&hb).map_err(|r| Refused::new(r, "exit 7")));
        let (si, s2) = match state {
            Ok(x) => x,
            Err(r) => {
                v.release(&mut h.client, gw);
                release_flush(v, &mut h.client, gf);
                l.refused(who, r);
                return false;
            }
        };
        let durable = s2.durable_lsn.max(e as u64);
        let bytes = slot(s2.slot_seq + 1, durable.max(s2.committed_lsn), durable);
        let w = if b.publish_unlocked {
            v.release(&mut h.client, gw);
            v.write_at(&h.head, ((1 - si) * 4096) as u64, &bytes)
        } else {
            let w = v.write_at(&h.head, ((1 - si) * 4096) as u64, &bytes);
            v.release(&mut h.client, gw);
            w
        };
        release_flush(v, &mut h.client, gf);
        if let Err(err) = w {
            l.refused(who, write_refused(&err));
            return false;
        }
        true
    }

    fn writer(&self, v: &SimVfs, l: &Ledger, who: &str, ops: &[u64]) {
        let r = root(v, RootAccess::ReadWrite);
        let mut h = Handles::open(v, &r);
        for &op in ops {
            if !self.commit(v, &mut h, l, who, op) {
                return;
            }
        }
    }

    /// Readers take no lock byte: they read the published view up to `committed_lsn` ([F16] P-57).
    fn reader(&self, v: &SimVfs, l: &Ledger) {
        let r = root(v, RootAccess::Read);
        let (Ok(head), Ok(log)) = (
            v.open(&r, rel("HEAD"), Access::Read, OpenHint::Normal),
            v.open(&r, rel(LOG), Access::Read, OpenHint::Normal),
        ) else {
            return;
        };
        for _ in 0..4 {
            let (Ok(hb), Ok(lb)) = (read(v, &head, 0, HEAD_LEN), read(v, &log, 0, LOG_LEN)) else {
                return;
            };
            let Ok((_, s)) = newest(&hb) else {
                continue;
            };
            let (_, groups) = scan(&lb);
            for &(end, op) in groups.iter().filter(|g| g.0 as u64 <= s.committed_lsn) {
                for (k, val) in writes_of(op) {
                    l.observe_covered(k, val, end as u64, s.durable_lsn);
                }
            }
        }
    }

    /// An automatic maintenance decision below the quiet cap ([F17 §5.2]): the maintenance byte by try, then the probe
    /// round of the nine quiet bytes, stopping at the first that is not `Free` ([F03 §3.1] rule 2, [OS/lock §8]).
    fn maintainer(&self, v: &SimVfs) {
        let r = root(v, RootAccess::ReadWrite);
        let Ok(mut c) = v.lock_client(&r, LockMode::Acquire) else {
            return;
        };
        let Ok(Acquired::Granted(gm)) = v.try_acquire(&mut c, LockByte::Maintenance) else {
            return;
        };
        let n = if self.breaks.partial_probe { 1 } else { 9 };
        let quiet = (0..n).any(|k| {
            let q = QuietIndex::new(k).expect("k < 9");
            v.probe(&c, LockByte::Quiet(q)) != ProbeResult::Free
        });
        if !quiet {
            v.note(NOTE_MAINT_DECISION, MAINT_AUTOMATIC | MAINT_BELOW_CAP, 0);
        }
        v.release(&mut c, gm);
    }

    /// A requester of quiet mode holding quiet byte 4 for its run ([F03 §3.1] rule 4).
    fn quiet(&self, v: &SimVfs) {
        let r = root(v, RootAccess::ReadWrite);
        let Ok(mut c) = v.lock_client(&r, LockMode::Acquire) else {
            return;
        };
        let q = LockByte::Quiet(QuietIndex::new(4).expect("4 < 9"));
        let Ok(Acquired::Granted(g)) = v.try_acquire(&mut c, q) else {
            return;
        };
        if let Ok(f) = v.open(&r, rel(LOG), Access::Read, OpenHint::Normal) {
            for _ in 0..2 {
                let _ = read(v, &f, 0, 8);
            }
        }
        v.release(&mut c, g);
    }

    /// Boot-change recovery ([F16] P-66, without `boot_id`) under the flush and writer bytes within 2 s each, or plain
    /// repair from the log when no slot is valid ([F16] P-85: the refusal it answers goes to `answered`). Returns the
    /// recovered state.
    fn recover_store(
        &self,
        v: &SimVfs,
        r: &SimRoot,
        answered: &mut Vec<Refused>,
    ) -> Result<EffectSet, Refused> {
        let busy = |m: &str| Refused::new(Refusal::Busy, m);
        let mut c = v
            .lock_client(r, LockMode::Acquire)
            .map_err(|e| Refused::new(Refusal::Lock, format!("{e:?}")))?;
        let log = v
            .open(r, rel(LOG), Access::ReadWrite, OpenHint::Normal)
            .map_err(|e| read_refused(LOG, &e))?;
        let head = v
            .open(r, rel("HEAD"), Access::ReadWrite, OpenHint::Normal)
            .map_err(|e| read_refused("HEAD", &e))?;
        let Ok(Acquired::Granted(gf)) = v.acquire_within(&mut c, LockByte::Flush, 2_000) else {
            return Err(busy("store_locked (flush byte)"));
        };
        let Ok(Acquired::Granted(gw)) = v.acquire_within(&mut c, LockByte::Writer, 2_000) else {
            v.release(&mut c, gf);
            return Err(busy("store_locked (writer byte)"));
        };
        let hb = read(v, &head, 0, HEAD_LEN).map_err(|e| read_refused("HEAD", &e))?;
        let lb = read(v, &log, 0, LOG_LEN).map_err(|e| read_refused(LOG, &e))?;
        let (e, groups) = scan(&lb);
        let base = match newest(&hb) {
            Ok((_, s)) => Some(s),
            Err(reason) => {
                answered.push(Refused::new(
                    reason,
                    "HEAD has no valid slot; run moirai repair",
                ));
                None
            }
        };
        if let Some(s) = &base {
            if (e as u64) < s.durable_lsn {
                return Err(Refused::new(
                    Refusal::Corrupt,
                    "an invalid group below durable_lsn",
                ));
            }
            let d = s.durable_lsn as usize;
            v.write_at(&log, d as u64, &lb[d..e])
                .map_err(|err| write_refused(&err))?;
        }
        v.release(&mut c, gw);
        if let Err(f) = v.sync(&log, SyncKind::Data) {
            v.fail_stop(f);
        }
        let Ok(Acquired::Granted(gw)) = v.acquire_within(&mut c, LockByte::Writer, 2_000) else {
            v.release(&mut c, gf);
            return Err(busy("store_locked (writer byte)"));
        };
        // The durable publish (P-13): two publishes, so both slots hold the newest state; or repair's two slots.
        let (first, second) = match base {
            Some(_) => {
                let hb = read(v, &head, 0, HEAD_LEN).map_err(|e| read_refused("HEAD", &e))?;
                let (si, s2) = newest(&hb).map_err(|r| Refused::new(r, "exit 7"))?;
                let durable = s2.durable_lsn.max(e as u64);
                let committed = durable.max(s2.committed_lsn);
                (
                    (1 - si, slot(s2.slot_seq + 1, committed, durable)),
                    (si, slot(s2.slot_seq + 2, committed, durable)),
                )
            }
            None => (
                (0, slot(1, e as u64, e as u64)),
                (1, slot(2, e as u64, e as u64)),
            ),
        };
        for (s, bytes) in [first, second] {
            v.write_at(&head, (s * 4096) as u64, &bytes)
                .map_err(|err| write_refused(&err))?;
        }
        if self.breaks.overwrite_acked && e >= GROUP_LEN {
            let _ = v.write_at(&log, GROUP_LEN as u64 - 8, &[0xEE; 8]);
        }
        v.release(&mut c, gw);
        if let Err(f) = v.sync(&head, SyncKind::DataAndMeta) {
            v.fail_stop(f);
        }
        v.release(&mut c, gf);
        Ok(fold(&groups, e))
    }

    fn read_view(&self, v: &SimVfs, r: &SimRoot) -> Result<EffectSet, Refused> {
        let head = v
            .open(r, rel("HEAD"), Access::Read, OpenHint::Normal)
            .map_err(|e| read_refused("HEAD", &e))?;
        let log = v
            .open(r, rel(LOG), Access::Read, OpenHint::Normal)
            .map_err(|e| read_refused(LOG, &e))?;
        let hb = read(v, &head, 0, HEAD_LEN).map_err(|e| read_refused("HEAD", &e))?;
        let (_, s) = newest(&hb).map_err(|r| Refused::new(r, "exit 7"))?;
        let lb = read(v, &log, 0, LOG_LEN).map_err(|e| read_refused(LOG, &e))?;
        let (_, groups) = scan(&lb);
        Ok(fold(&groups, s.committed_lsn as usize))
    }
}

impl Subject for Proto {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("LOCK"), &[0u8; 36 * 1024]).expect("LOCK");
        w.put_file(&store(LOG), &vec![0u8; LOG_LEN as usize])
            .expect("log");
        let mut head = slot(1, 0, 0);
        head.extend(slot(2, 0, 0));
        w.put_file(&store("HEAD"), &head).expect("HEAD");
    }

    fn workload(&self, w: &SimWorld, l: &Ledger) {
        let task = |s: &Proto, who: &'static str, ops: Vec<u64>| -> Body {
            let (s, l) = (s.clone(), l.clone());
            Box::new(move |v: SimVfs| s.writer(&v, &l, who, &ops))
        };
        let (s1, s2, l2) = (self.clone(), self.clone(), l.clone());
        let s3 = self.clone();
        run_procs(
            w,
            vec![
                // Two clients of one process, each in its own task ([OS/lock §6] item 5): one may hold the writer byte
                // while the other flushes.
                (
                    "w1".to_owned(),
                    vec![task(self, "w1a", vec![11]), task(self, "w1b", vec![12])],
                ),
                ("w2".to_owned(), vec![task(self, "w2", vec![21, 22])]),
                (
                    "reader".to_owned(),
                    vec![Box::new(move |v: SimVfs| s1.reader(&v, &l2))],
                ),
                (
                    "maint".to_owned(),
                    vec![Box::new(move |v: SimVfs| s2.maintainer(&v))],
                ),
                (
                    "quiet".to_owned(),
                    vec![Box::new(move |v: SimVfs| s3.quiet(&v))],
                ),
            ],
        );
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let mut answered = Vec::new();
        // The first reader runs boot-change recovery before its first read ([F16] P-60).
        let rv = w.process_with("first-reader", None, Some(true));
        let rr = root(&rv, RootAccess::ReadWrite);
        let first_read = if self.breaks.claim_no_slot {
            Err(Refused::new(Refusal::NoValidSlot, "HEAD has no valid slot"))
        } else {
            self.recover_store(&rv, &rr, &mut answered)
                .and_then(|_| self.read_view(&rv, &rr))
        };
        let wv = w.process_with("writer", None, Some(true));
        let wr = root(&wv, RootAccess::ReadWrite);
        let state = self.recover_store(&wv, &wr, &mut answered);
        Recovered {
            first_read,
            state,
            answered,
            ..Recovered::same(EffectSet::new())
        }
    }

    fn slot_files(&self) -> Vec<PathBuf> {
        vec![store("HEAD")]
    }

    fn protocol(&self) -> Option<Protocol> {
        Some(Protocol {
            log_dir: PathBuf::from(STORE),
            log_prefix: "log.".to_owned(),
            decode_slot: decode,
        })
    }
}

fn broken(breaks: Breaks) -> Proto {
    Proto { breaks }
}

/// The trace family over a store that follows the protocol: every publish, flush, wait and probe judged in every run of
/// the PR tier's crash points, deaths, disk-full and failed-flush runs, with two clients of one process (FM-11), and no
/// false report. The workload's refusals (disk-full appends and publishes, lost groups) are judged too.
#[test]
fn a_protocol_following_store_passes_every_family() {
    let dims = Dims {
        read_faults: false,
        ..Dims::default()
    };
    let report = enumerate(&Proto::default(), &pr(&[1], dims));
    println!("{report}");
    report.assert_passed();
    assert!(report.publishes_judged > 0, "{report}");
    assert!(report.chain_checks > 0, "{report}");
    assert!(report.workload_refusals > 0, "{report}");
}

/// E1 (P-4, [F13 §1.4]): a slot write without the writer byte, which the writer reports nowhere, is caught from the
/// world's capture of the slot write.
#[test]
fn e1_a_publish_without_the_writer_byte_is_caught() {
    let report = enumerate(
        &broken(Breaks {
            publish_unlocked: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(has(&report, "trace: P-4:"), "{report}");
}

/// E2 (P-2): the flush holder's own flush under the writer byte is caught although another client of its process
/// legitimately holds the writer byte meanwhile in a correct run (the first test); E4 (I-G4): a log flush without the
/// flush byte.
#[test]
fn e2_e4_flushes_under_the_writer_byte_or_without_the_flush_byte_are_caught() {
    let report = enumerate(
        &broken(Breaks {
            flush_under_writer: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(has(&report, "trace: P-2 (I-G4)"), "{report}");
    let report = enumerate(
        &broken(Breaks {
            flush_without_byte: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(has(&report, "without the flush byte"), "{report}");
}

/// E9 ([F03 §3.1] rule 2): a decision whose probe round covered one quiet byte.
#[test]
fn e9_a_partial_quiet_probe_round_is_caught() {
    let report = enumerate(
        &broken(Breaks {
            partial_probe: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(has(&report, "trace: [F03 §3.1] rule 2"), "{report}");
}

/// E11 (I-G2): a read of a durable group no flush covered, from its position and the reader's slot.
#[test]
fn e11_a_read_of_an_uncovered_durable_group_is_caught() {
    let report = enumerate(
        &broken(Breaks {
            read_uncovered: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(has(&report, "fresh (I-G2)"), "{report}");
}

/// E5 (avail): a workload refusal that no fault of the run explains.
#[test]
fn e5_an_unexplained_workload_refusal_is_caught() {
    let report = enumerate(
        &broken(Breaks {
            refuse_corrupt: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(has(&report, "avail: w1b refused with corrupt"), "{report}");
}

/// E8 (I-G3): an acknowledged group overwritten in place in the recovered store.
#[test]
fn e8_an_overwritten_acknowledged_group_is_caught() {
    let report = enumerate(
        &broken(Breaks {
            overwrite_acked: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(has(&report, "chain (I-G3)"), "{report}");
}

/// E7 (avail): "no valid slot" with no failed slot flush and no cut slot write is not a correct answer.
#[test]
fn e7_an_unexplained_no_valid_slot_is_caught() {
    let report = enumerate(
        &broken(Breaks {
            claim_no_slot: true,
            ..Breaks::default()
        }),
        &pr(&[1], NO_DIMS),
    );
    assert!(
        has(&report, "first read: refused: no valid slot"),
        "{report}"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// E10: ns, by node identity.

const INTENT: EffectKey = EffectKey::new(EffectKind::Intent, 1);
const OPEN: u64 = 1;
const DONE: u64 = 2;

/// A file move under an intent ([40 §3.4], [F16] P-16, P-17): the intent record made durable, the rename and the
/// directory flush, then the done record. `rename_first`: the rename before the intent is durable.
struct Moves {
    rename_first: bool,
}

fn set_intent(v: &SimVfs, f: &SimFile, value: u64) {
    if let Err(e) = v.write_at(f, 0, &value.to_le_bytes()) {
        v.fail_stop(moirai_vfs::DurabilityFailure {
            class: moirai_vfs::DurabilityClass::Lazy,
            call: e.call,
            kind: e.kind,
            os: e.os,
        });
    }
    if let Err(e) = v.sync(f, SyncKind::Data) {
        v.fail_stop(e);
    }
}

fn rename(v: &SimVfs, r: &SimRoot) {
    if v.rename_noreplace(r, rel("a"), r, rel("b"), ShareRetry::None)
        .is_err()
    {
        return;
    }
    if let Err(e) = v.sync_dir(r, None) {
        v.fail_stop(e);
    }
}

impl Subject for Moves {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("I"), &[0u8; 8]).expect("I");
        w.put_file(&store("a"), &[0xA5; 100]).expect("a");
    }

    fn workload(&self, w: &SimWorld, l: &Ledger) {
        let (l, first) = (l.clone(), self.rename_first);
        let body: Body = Box::new(move |v: SimVfs| {
            let r = root(&v, RootAccess::ReadWrite);
            let f = v
                .open(&r, rel("I"), Access::ReadWrite, OpenHint::Normal)
                .expect("I");
            let (a, b) = (store("a"), store("b"));
            l.expect_names(
                &a,
                INTENT,
                &[
                    (None, std::slice::from_ref(&a)),
                    (Some(OPEN), &[a.clone(), b.clone()]),
                    (Some(DONE), std::slice::from_ref(&b)),
                ],
            );
            l.begin(1, Class::Durable, &[(INTENT, Some(OPEN))]);
            if first {
                rename(&v, &r);
            }
            set_intent(&v, &f, OPEN);
            l.ack(1);
            if !first {
                rename(&v, &r);
            }
            l.begin(2, Class::Durable, &[(INTENT, Some(DONE))]);
            set_intent(&v, &f, DONE);
            l.ack(2);
        });
        run_procs(w, vec![("mover".to_owned(), vec![body])]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let v = w.process_with("recovery", None, Some(true));
        let r = root(&v, RootAccess::Read);
        let value = v
            .open(&r, rel("I"), Access::Read, OpenHint::Normal)
            .ok()
            .and_then(|f| read(&v, &f, 0, 8).ok())
            .map_or(0, |b| u64_at(&b, 0));
        let set: EffectSet = if value == 0 {
            EffectSet::new()
        } else {
            [(INTENT, value)].into_iter().collect()
        };
        Recovered::same(set)
    }
}

/// E10 ([F16 §17.2] ns): the simulator's namespace check judges where each durable intent state expects the file, by
/// node identity; a rename before its intent is durable leaves the file at its destination with no intent.
#[test]
fn e10_a_rename_before_its_intent_is_caught_by_the_namespace_check() {
    let dims = Dims {
        crash_points: true,
        ..NO_DIMS
    };
    enumerate(
        &Moves {
            rename_first: false,
        },
        &pr(&[1], dims),
    )
    .assert_passed();
    let report = enumerate(&Moves { rename_first: true }, &pr(&[1], dims));
    assert!(has(&report, "ns: the file that was"), "{report}");
}

// ---------------------------------------------------------------------------------------------------------------------
// E13: outcome_unknown after a failed flush.

/// One flushed write; the recovering writer answers `outcome_unknown` after a failed flush (P-47's three losses), or
/// always (`always`).
struct Unknown {
    always: bool,
}

impl Subject for Unknown {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("F"), &[0u8; 4096]).expect("F");
    }

    fn workload(&self, w: &SimWorld, _l: &Ledger) {
        let body: Body = Box::new(|v: SimVfs| {
            let r = root(&v, RootAccess::ReadWrite);
            let f = v
                .open(&r, rel("F"), Access::ReadWrite, OpenHint::Normal)
                .expect("F");
            if v.write_at(&f, 0, &[7; 512]).is_ok() {
                let _ = v.sync(&f, SyncKind::Data);
            }
        });
        run_procs(w, vec![("writer".to_owned(), vec![body])]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let unknown = self.always || w.failed_flushes() > 0;
        Recovered {
            state: if unknown {
                Err(Refused::new(Refusal::OutcomeUnknown, "outcome_unknown"))
            } else {
                Ok(EffectSet::new())
            },
            ..Recovered::same(EffectSet::new())
        }
    }
}

/// E13 ([F16 §17.2] avail, P-47): the recovering writer's `outcome_unknown` is a correct answer in a run with a failed
/// flush, and a failure without one.
#[test]
fn e13_outcome_unknown_is_allowed_only_after_a_failed_flush_or_read() {
    let report = enumerate(
        &Unknown { always: false },
        &pr(
            &[1],
            Dims {
                flush_errors: true,
                ..NO_DIMS
            },
        ),
    );
    report.assert_passed();
    assert!(report.flush_errors > 0 && report.refusals > 0, "{report}");
    let report = enumerate(&Unknown { always: true }, &pr(&[1], NO_DIMS));
    assert!(
        has(&report, "recovery: refused: outcome unknown"),
        "{report}"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// I-G4 and the extent preparations (WP-40 closure): a store that rotates ([F16] P-72), re-prepares a short leftover in
// place (`recycle_extent`), prepares a spare under a `tmp/` name off the commit path (P-96) and creates an extent at its
// log name; its preparations' zeros are no log writes.

/// The extent length E of the rotating store (one group per extent, at its first byte).
const EXT_LEN: u64 = 2 * 4096;
const LOGS: [&str; 4] = ["log.1", "log.2", "log.3", "log.4"];
const SPARE_TMP: &str = "tmp/extent.1";

/// How a rotation makes `log.<n>` ready ([F16] P-72 step 2).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ready {
    /// The extent holds groups already: no rotation.
    Holds,
    /// A leftover shorter than E: `recycle_extent` in place.
    Recycle,
    /// A full-length spare (P-96): nothing written, the flushes re-issued, `tmp/` synced too.
    Spare,
    /// No file: `create_extent` at the log name.
    Create,
}

/// Preparation breaks the trace family must catch.
#[derive(Copy, Clone, Debug, Default)]
struct PrepBreaks {
    /// P-2: the rotation prepares the extent while it holds the writer byte too.
    under_writer: bool,
    /// P-72: the rotation prepares the extent at its log name without the flush byte.
    without_flush: bool,
}

#[derive(Clone, Debug, Default)]
struct Rotation {
    breaks: PrepBreaks,
}

impl Rotation {
    /// Durable commit `n` into extent `n` at its first byte, the extent made ready by `ready` first: the flush byte,
    /// the preparation and its flushes, the writer byte for the append, the flush outside it, the publish ([F16] P-72
    /// steps 1–4, P-44, P-45).
    #[allow(clippy::too_many_arguments)] // the process's handles, the volume, the ledger and the commit.
    fn commit(
        &self,
        v: &SimVfs,
        r: &SimRoot,
        c: &mut SimClient,
        head: &SimFile,
        vol: &StoreVolume,
        l: &Ledger,
        n: usize,
        ready: Ready,
    ) -> bool {
        let op = n as u64 + 1;
        l.begin(op, Class::Durable, &writes_of(op));
        let gf = if self.breaks.without_flush {
            None
        } else {
            match v.acquire_within(c, LockByte::Flush, 60_000) {
                Ok(Acquired::Granted(g)) => Some(g),
                _ => {
                    l.refused("rotator", Refused::new(Refusal::Busy, "store_locked"));
                    return false;
                }
            }
        };
        let early = if self.breaks.under_writer {
            match v.acquire_within(c, LockByte::Writer, 60_000) {
                Ok(Acquired::Granted(g)) => Some(g),
                _ => return false,
            }
        } else {
            None
        };
        let name = rel(LOGS[n]);
        let file = match ready {
            Ready::Create => v.create_extent(r, name, EXT_LEN, vol),
            _ => v
                .open(r, name, Access::ReadWrite, OpenHint::Normal)
                .and_then(|f| {
                    if ready == Ready::Recycle {
                        v.recycle_extent(&f, EXT_LEN, vol)?;
                    }
                    Ok(f)
                }),
        };
        let Ok(file) = file else {
            return false;
        };
        if ready != Ready::Holds {
            if let Err(f) = v.sync(&file, SyncKind::DataAndMeta) {
                v.fail_stop(f);
            }
            if let Err(f) = v.sync_dir(r, None) {
                v.fail_stop(f);
            }
            if ready == Ready::Spare
                && let Err(f) = v.sync_dir(r, Some(rel("tmp")))
            {
                v.fail_stop(f);
            }
        }
        let gw = match early {
            Some(g) => g,
            None => match v.acquire_within(c, LockByte::Writer, 60_000) {
                Ok(Acquired::Granted(g)) => g,
                _ => return false,
            },
        };
        let appended = v.write_at(&file, 0, &group(op, CHAIN_SEED));
        v.release(c, gw);
        if appended.is_err() {
            return false;
        }
        if let Err(f) = v.sync(&file, SyncKind::Data) {
            v.fail_stop(f);
        }
        let Ok(Acquired::Granted(gw)) = v.acquire_within(c, LockByte::Writer, 60_000) else {
            return false;
        };
        let end = n as u64 * EXT_LEN + GROUP_LEN as u64;
        let published = read(v, head, 0, HEAD_LEN)
            .ok()
            .and_then(|hb| newest(&hb).ok())
            .is_some_and(|(si, s)| {
                let bytes = slot(s.slot_seq + 1, end, end);
                v.write_at(head, ((1 - si) * 4096) as u64, &bytes).is_ok()
            });
        v.release(c, gw);
        if let Some(g) = gf {
            v.release(c, g);
        }
        if published {
            l.ack(op);
        }
        published
    }

    /// The spare of [F16] P-96: under the maintenance byte only, `create_extent` of a `tmp/` name, its flush, the rename
    /// onto `log.<n>`, then `durable-name` on `tmp/` and on the store directory.
    fn spare(&self, v: &SimVfs, r: &SimRoot, c: &mut SimClient, vol: &StoreVolume, n: usize) {
        let Ok(Acquired::Granted(gm)) = v.try_acquire(c, LockByte::Maintenance) else {
            return;
        };
        if let Ok(f) = v.create_extent(r, rel(SPARE_TMP), EXT_LEN, vol) {
            if let Err(e) = v.sync(&f, SyncKind::DataAndMeta) {
                v.fail_stop(e);
            }
            if v.rename_noreplace(r, rel(SPARE_TMP), r, rel(LOGS[n]), ShareRetry::None)
                .is_ok()
            {
                for dir in [Some(rel("tmp")), None] {
                    if let Err(e) = v.sync_dir(r, dir) {
                        v.fail_stop(e);
                    }
                }
            }
        }
        v.release(c, gm);
    }
}

impl Subject for Rotation {
    fn setup(&self, w: &SimWorld, _l: &Ledger) {
        w.mkdir_all(&store("tmp"));
        w.put_file(&store("LOCK"), &[0u8; 36 * 1024]).expect("LOCK");
        w.put_file(&store(LOGS[0]), &vec![0u8; EXT_LEN as usize])
            .expect("log.1");
        // The leftover of an interrupted preparation ([F16] P-72: shorter than E, beyond the end of the valid log).
        w.put_file(&store(LOGS[1]), &[0u8; 512]).expect("log.2");
        let mut head = slot(1, 0, 0);
        head.extend(slot(2, 0, 0));
        w.put_file(&store("HEAD"), &head).expect("HEAD");
    }

    fn workload(&self, w: &SimWorld, l: &Ledger) {
        let (s, l) = (self.clone(), l.clone());
        let body: Body = Box::new(move |v: SimVfs| {
            let r = root(&v, RootAccess::ReadWrite);
            let vol = match v.classify(&r, ClassifyDepth::Open) {
                Ok(Classification::Local(vol)) => vol,
                other => panic!("the store volume: {other:?}"),
            };
            let Ok(mut c) = v.lock_client(&r, LockMode::Acquire) else {
                return;
            };
            let Ok(head) = v.open(&r, rel("HEAD"), Access::ReadWrite, OpenHint::Normal) else {
                return;
            };
            for (n, ready) in [(0, Ready::Holds), (1, Ready::Recycle)] {
                if !s.commit(&v, &r, &mut c, &head, &vol, &l, n, ready) {
                    return;
                }
            }
            s.spare(&v, &r, &mut c, &vol, 2);
            for (n, ready) in [(2, Ready::Spare), (3, Ready::Create)] {
                if !s.commit(&v, &r, &mut c, &head, &vol, &l, n, ready) {
                    return;
                }
            }
        });
        run_procs(w, vec![("rotator".to_owned(), vec![body])]);
    }

    fn recover(&self, w: &SimWorld) -> Recovered {
        let v = w.process_with("recovery", None, Some(true));
        let r = root(&v, RootAccess::Read);
        // The valid log, extent by extent, to its first invalid group or missing extent ([F16] P-29: groups beyond the
        // published `committed_lsn` are part of it; the publish itself is not flushed).
        let state = (|| {
            let head = v
                .open(&r, rel("HEAD"), Access::Read, OpenHint::Normal)
                .map_err(|e| read_refused("HEAD", &e))?;
            let hb = read(&v, &head, 0, HEAD_LEN).map_err(|e| read_refused("HEAD", &e))?;
            let (_, s) = newest(&hb).map_err(|r| Refused::new(r, "exit 7"))?;
            let mut set = EffectSet::new();
            let mut end = 0;
            for (n, name) in LOGS.iter().enumerate() {
                let op = n as u64 + 1;
                let Ok(f) = v.open(&r, rel(name), Access::Read, OpenHint::Normal) else {
                    break;
                };
                let b = read(&v, &f, 0, GROUP_LEN as u64).map_err(|e| read_refused(name, &e))?;
                if b != group(op, CHAIN_SEED) {
                    break;
                }
                end = n as u64 * EXT_LEN + GROUP_LEN as u64;
                set.extend(
                    writes_of(op)
                        .into_iter()
                        .filter_map(|(k, v)| v.map(|v| (k, v))),
                );
            }
            if end < s.durable_lsn {
                // The first invalid group is the first byte of the extent after the last valid one.
                let bad = if end == 0 {
                    0
                } else {
                    (end / EXT_LEN) as usize + 1
                };
                return Err(
                    Refused::new(Refusal::Corrupt, "an invalid group below durable_lsn")
                        .concerning(store(LOGS[bad.min(LOGS.len() - 1)])),
                );
            }
            Ok(set)
        })();
        Recovered {
            first_read: state.clone(),
            state,
            ..Recovered::same(EffectSet::new())
        }
    }

    fn slot_files(&self) -> Vec<PathBuf> {
        vec![store("HEAD")]
    }

    fn protocol(&self) -> Option<Protocol> {
        Some(Protocol {
            log_dir: PathBuf::from(STORE),
            log_prefix: "log.".to_owned(),
            decode_slot: decode,
        })
    }
}

/// I-G4 (WP-40 closure check, item 1): a store that rotates by [F16] P-72 — re-preparing a short leftover in place, using
/// a spare that P-96 prepared under a `tmp/` name with the maintenance byte only, and creating an extent at its log
/// name, each preparation under the flush byte — passes every family in its clean run and at every crash point: the
/// zeros of `create_extent` and `recycle_extent` are no log writes, so the writer-byte rule of I-G4 (P-27, P-43) does not
/// judge them. Before the fix, each preparation's zero writes were reported as log writes without the writer byte.
#[test]
fn a_rotating_store_passes_and_its_preparations_are_no_log_writes() {
    let dims = Dims {
        crash_points: true,
        ..NO_DIMS
    };
    let report = enumerate(&Rotation::default(), &pr(&[1], dims));
    report.assert_passed();
    assert!(report.publishes_judged >= 4, "{report}");
    // The preparations' steps were crash points: the exclusive creates and every zero write.
    let calls = |k: CallKind| report.crash_points_by_call.get(&k).copied().unwrap_or(0);
    assert!(
        calls(CallKind::CreateNew) >= 2 && calls(CallKind::Write) >= 7,
        "{report}"
    );
}

/// P-2 and P-72 (WP-40 closure check, item 1): a preparation is judged by its own rules — under the writer byte it is
/// caught by P-2, at the log name without the flush byte by P-72.
#[test]
fn a_preparation_under_the_writer_byte_or_without_the_flush_byte_is_caught() {
    let report = enumerate(
        &Rotation {
            breaks: PrepBreaks {
                under_writer: true,
                ..PrepBreaks::default()
            },
        },
        &pr(&[1], NO_DIMS),
    );
    assert!(
        has(
            &report,
            "starts recycle_extent while holding the writer byte"
        ),
        "{report}"
    );
    assert!(
        has(
            &report,
            "starts create_extent while holding the writer byte"
        ),
        "{report}"
    );
    let report = enumerate(
        &Rotation {
            breaks: PrepBreaks {
                without_flush: true,
                ..PrepBreaks::default()
            },
        },
        &pr(&[1], NO_DIMS),
    );
    assert!(
        has(
            &report,
            "by recycle_extent at its log name without the flush byte"
        ),
        "{report}"
    );
    assert!(
        has(
            &report,
            "by create_extent at its log name without the flush byte"
        ),
        "{report}"
    );
    assert!(!has(&report, "writes log extent node"), "{report}");
}

// ---------------------------------------------------------------------------------------------------------------------
// Avail and setup faults (WP-40 closure): an FM-10 act before the run explains a refusal only through its file.

/// How the setup declares the stray extent it puts beside the store.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Stray {
    /// [`Ledger::setup_fault`]: damaged where the store needs it.
    Damaged,
    /// [`Ledger::setup_ignored`]: a file the protocol must ignore ([F16] P-55's extent of another epoch).
    Ignored,
}

/// A store whose setup put a fatal `HEAD` in place (declared, [F15] FM-10.1) and a stray extent `log.1` below the
/// extents it keeps; its recovery refuses the first read with a fatal slot, answers it with plain repair, and then ends
/// with `end`: a refusal concerning the stray, one that names no file, or the rebuilt state.
struct Declared {
    stray: Stray,
    end: Result<(), Refused>,
}

impl Subject for Declared {
    fn setup(&self, w: &SimWorld, l: &Ledger) {
        w.mkdir_all(Path::new(STORE));
        w.put_file(&store("HEAD"), &[0xFA; 8192]).expect("HEAD");
        w.put_file(&store("log.1"), &[0x55; 4096]).expect("log.1");
        w.put_file(&store("log.2"), &[0u8; 4096]).expect("log.2");
        l.setup_fault(&store("HEAD"));
        match self.stray {
            Stray::Damaged => l.setup_fault(&store("log.1")),
            Stray::Ignored => l.setup_ignored(&store("log.1")),
        }
    }

    fn workload(&self, _w: &SimWorld, _l: &Ledger) {}

    fn recover(&self, _w: &SimWorld) -> Recovered {
        let fatal = || {
            Refused::new(
                Refusal::FatalSlot,
                "exit 7 store_corrupt: run moirai repair",
            )
        };
        Recovered {
            first_read: Err(fatal()),
            state: self.end.clone().map(|()| EffectSet::new()),
            answered: vec![fatal()],
            ..Recovered::same(EffectSet::new())
        }
    }

    fn slot_files(&self) -> Vec<PathBuf> {
        vec![store("HEAD")]
    }
}

/// Avail (WP-40 closure check, item 2; [F16 §17.2], [F15] FM-10): the fatal `HEAD` the setup declared explains the slot
/// refusals that plain repair answers, and a stray extent declared damaged explains a refusal that concerns it; but a
/// refusal that names no file is explained by no setup fault (before the fix, any declared fault excused every corrupt
/// refusal, so P-55's missing epoch check was never seen), and a file the protocol must ignore explains nothing.
#[test]
fn a_setup_fault_explains_a_refusal_only_through_its_file() {
    let corrupt = || Refused::new(Refusal::Corrupt, "the log is invalid below durable_lsn");
    let run = |stray: Stray, end: Result<(), Refused>| {
        enumerate(&Declared { stray, end }, &pr(&[1], NO_DIMS))
    };
    // The rebuilt state: every refusal on the way is the fatal fixture's.
    let report = run(Stray::Damaged, Ok(()));
    report.assert_passed();
    assert!(report.refusals >= 2, "{report}");
    // A refusal concerning the damaged stray is detection, a correct answer (FM-10 "Crash gates").
    run(Stray::Damaged, Err(corrupt().concerning(store("log.1")))).assert_passed();
    // A refusal that names no file: no declared fault explains it.
    let report = run(Stray::Damaged, Err(corrupt()));
    assert!(
        has(&report, "recovery: refused: corrupt") && has(&report, "names no file"),
        "{report}"
    );
    // A refusal concerning another file is not explained by the stray's fault either.
    let report = run(Stray::Damaged, Err(corrupt().concerning(store("log.2"))));
    assert!(
        has(&report, "no external act or setup fault on"),
        "{report}"
    );
    // An ignored stray explains no refusal that concerns it.
    let report = run(Stray::Ignored, Err(corrupt().concerning(store("log.1"))));
    assert!(has(&report, "a file the protocol must ignore"), "{report}");
    run(Stray::Ignored, Ok(())).assert_passed();
}
