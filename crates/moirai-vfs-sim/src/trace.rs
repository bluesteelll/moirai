//! The replayable event trace ([F15 §6.4] "Determinism"; [F13 §1.4] trace predicates).
//!
//! Every adversary choice, every call's start and return, every lock, flush and namespace effect, every death, crash and
//! boot, and every harness note goes into one sequence of fixed-width [`Event`]s. The same seed and the same sequence of
//! calls, with their clients and schedule, give a byte-identical trace (WP-31 acceptance); [`Trace::bytes`] is that byte
//! form and [`Trace::digest`] a running 64-bit FNV-1a hash of it. In [`TraceMode::DigestOnly`] the events themselves are
//! not kept, so a long GT3 run holds a constant amount of trace memory and still proves replay by its digest. In
//! [`TraceMode::Full`] the events are kept in frozen shared chunks plus a short tail, so a crash image captured at every
//! scheduling point shares the trace prefix with the world instead of copying it.

use std::sync::Arc;

/// Whether the trace keeps its events.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TraceMode {
    /// Keep every event (unit tests, the enumerator's replay of one failing scenario, trace predicates).
    Full,
    /// Keep only the count and the digest (long runs).
    DigestOnly,
}

/// What an [`Event`] records. The meaning of the fields `a`, `b` and `c` is given per kind.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum EventKind {
    /// A boot began. `a` boot sequence number (1 for the first), `b` its `boot_hash`, `c` the boot clock origin.
    Boot = 1,
    /// A simulated process started. `a` pid, `b` parent process index or `u64::MAX`, `c` 1 if its boot identity is known.
    ProcStart = 2,
    /// A simulated process ended. `a` the [`crate::DeathCause`] code, `b` the number of lock bytes it still held.
    ProcEnd = 3,
    /// A task (a simulated thread) started. `a` the task number.
    TaskStart = 4,
    /// A task ended. `a` the task number, `b` 0 returned, 1 died with its process, 2 panicked, 3 aborted by deadlock.
    TaskEnd = 5,
    /// A scheduling point at a call boundary ([F15 §6.4] "Crash points"). `a` the point number, `b` the [`crate::CallKind`]
    /// code, `c` 0 at a call's start, 1 inside it (between its start and its return).
    Point = 6,
    /// A call returned. `a` the [`crate::CallKind`] code, `b` the node (file or directory) or 0, `c` 0 on success, else
    /// 1 + the error code of [`crate::error_code`].
    Return = 7,
    /// An adversary choice. `a` the [`crate::Site`] code, `b` the arity (`u64::MAX`: any value), `c` the value.
    Choice = 8,
    /// A queued fault was used instead of asking the adversary. `a` the [`crate::Site`] code, `c` the value.
    Injected = 9,
    /// Time jumped (a timer wait, a pause, a suspend, an explicit advance). `a` boot clock, `b` monotonic clock.
    Time = 10,
    /// A kernel try on a lock byte. `a` the `LOCK` node, `b` the byte offset, `c` 1 granted, 0 busy.
    LockTry = 11,
    /// A kernel unlock. `a` node, `b` byte.
    LockUnlock = 12,
    /// A dead process's byte stays held. `a` node, `b` byte, `c` release instant on the boot clock (`u64::MAX`: never).
    LockZombie = 13,
    /// A dead process's byte was released after its delay. `a` node, `b` byte.
    LockZombieFree = 14,
    /// A kernel wait was registered. `a` node, `b` byte, `c` 0 caller-driven, 1 waiter thread.
    LockWait = 15,
    /// A kernel wait obtained the byte. `a` node, `b` byte.
    LockWaitGranted = 16,
    /// A caller-driven kernel wait was cancelled. `a` node, `b` byte.
    LockWaitCancelled = 17,
    /// A client obtained a grant. `a` node, `b` byte, `c` client id.
    Granted = 18,
    /// A client released a grant. `a` node, `b` byte, `c` client id.
    Released = 19,
    /// A flush began. `a` node, `b` 0 `sync(Data)`, 1 `sync(DataAndMeta)`, 2 `sync_dir`, `c` the flush id.
    FlushStart = 20,
    /// A flush ended. `a` node, `b` as for `FlushStart`, `c` 0 succeeded, 1 failed, 2 not performed.
    FlushEnd = 21,
    /// A namespace operation took effect. `a` operation id, `b` the [`crate::NsKind`] code, `c` the node.
    NsOp = 22,
    /// A pending namespace operation became durable (FM-2.3). `a` operation id.
    NsDurable = 23,
    /// A system crash. `a` the scheduling point it happened at, `b` pending namespace operations that survived, `c` the
    /// number of files resolved.
    Crash = 24,
    /// A protocol violation the simulator detected ([F15 §3.13], OP-20). `a` the [`crate::ViolationKind`] code, `b` node.
    Violation = 25,
    /// A harness note ([F13 §1.4]: acknowledgements, publishes). `a` tag, `b` and `c` the harness's values.
    Note = 26,
    /// An external actor acted on a file (FM-10). `a` node, `b` 0 truncate, 1 write, 2 flush, 3 replace, 4 exclusive
    /// hold, 5 read error, 6 mapping fault; `c` a length or offset.
    External = 27,
    /// A process death resolved an in-flight call (§2.5). `a` node, `b` 0 write, 1 flush, 2 `sync_dir`, `c` the outcome.
    InFlight = 28,
    /// A mapped read (`SealedMap::bytes`). `a` node, `b` mapped length, `c` 0 bytes, 1 zeros beyond a truncated end,
    /// 2 a media fault (the reader dies), 3 the reader died on the truncation.
    MapRead = 29,
    /// A `fill_random` draw ([OS/README §4.6] "Simulator form": every drawn value appears in the trace). `a` the length
    /// in its low 32 bits and the number of those bytes a test scripted in its high 32 bits; `b` and `c` the drawn bytes
    /// 0–7 and 8–15 read little-endian, zero-padded (every value [OS/README §4.6]'s table names is 8 or 16 bytes). A
    /// longer draw continues in [`EventKind::RandomMore`] events.
    Random = 30,
    /// A `LOCK` handle a grant table opened named another file than the client's data handle ([OS/lock §9.1] step 4).
    /// `a` the data handle's node, `b` the node the path `LOCK` names now.
    LockIdentity = 31,
    /// The next 24 bytes of a `fill_random` draw longer than 16 bytes, after its [`EventKind::Random`] event: `a`, `b`
    /// and `c` the bytes read little-endian, zero-padded.
    RandomMore = 32,
}

/// One trace record: a kind, the task and process it concerns (`u32::MAX`: none, for example the driver thread or an
/// external actor) and three values whose meaning [`EventKind`] gives.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Event {
    /// What happened.
    pub kind: EventKind,
    /// The task, or `u32::MAX`.
    pub task: u32,
    /// The process, or `u32::MAX`.
    pub proc: u32,
    /// First value.
    pub a: u64,
    /// Second value.
    pub b: u64,
    /// Third value.
    pub c: u64,
}

/// The width of one encoded event: kind, task, process and three values, little-endian.
pub const EVENT_LEN: usize = 1 + 4 + 4 + 8 * 3;

impl Event {
    /// The event's fixed-width little-endian encoding.
    pub fn encode(&self) -> [u8; EVENT_LEN] {
        let mut b = [0u8; EVENT_LEN];
        b[0] = self.kind as u8;
        b[1..5].copy_from_slice(&self.task.to_le_bytes());
        b[5..9].copy_from_slice(&self.proc.to_le_bytes());
        b[9..17].copy_from_slice(&self.a.to_le_bytes());
        b[17..25].copy_from_slice(&self.b.to_le_bytes());
        b[25..33].copy_from_slice(&self.c.to_le_bytes());
        b
    }
}

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// Up to 8 bytes read little-endian, zero-padded (the values of [`EventKind::Random`] and [`EventKind::RandomMore`]).
pub(crate) fn le_padded(bytes: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    let n = bytes.len().min(8);
    w[..n].copy_from_slice(&bytes[..n]);
    u64::from_le_bytes(w)
}

/// Events per frozen chunk.
const CHUNK: usize = 4096;

/// The event sequence of one simulated world.
#[derive(Clone, Debug)]
pub(crate) struct Trace {
    mode: TraceMode,
    /// Full chunks, shared by every clone.
    frozen: Vec<Arc<[Event]>>,
    /// The events after the last full chunk (fewer than `CHUNK`).
    tail: Vec<Event>,
    count: u64,
    digest: u64,
}

impl Trace {
    pub(crate) fn new(mode: TraceMode) -> Trace {
        Trace {
            mode,
            frozen: Vec::new(),
            tail: Vec::new(),
            count: 0,
            digest: FNV_OFFSET,
        }
    }

    pub(crate) fn push(&mut self, e: Event) {
        for byte in e.encode() {
            self.digest ^= u64::from(byte);
            self.digest = self.digest.wrapping_mul(FNV_PRIME);
        }
        self.count += 1;
        if self.mode == TraceMode::Full {
            self.tail.push(e);
            if self.tail.len() == CHUNK {
                let chunk: Arc<[Event]> = core::mem::take(&mut self.tail).into();
                self.frozen.push(chunk);
            }
        }
    }

    /// The kept events, in order.
    pub(crate) fn events(&self) -> Vec<Event> {
        let mut out = Vec::with_capacity(self.frozen.len() * CHUNK + self.tail.len());
        for c in &self.frozen {
            out.extend_from_slice(c);
        }
        out.extend_from_slice(&self.tail);
        out
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn digest(&self) -> u64 {
        self.digest
    }

    /// The byte form of the kept events (empty in `DigestOnly` mode).
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let n = self.frozen.len() * CHUNK + self.tail.len();
        let mut out = Vec::with_capacity(n * EVENT_LEN);
        for e in self.frozen.iter().flat_map(|c| c.iter()).chain(&self.tail) {
            out.extend_from_slice(&e.encode());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_follows_the_bytes_in_both_modes() {
        let e = Event {
            kind: EventKind::Note,
            task: 1,
            proc: 2,
            a: 3,
            b: 4,
            c: 5,
        };
        let mut full = Trace::new(TraceMode::Full);
        let mut lean = Trace::new(TraceMode::DigestOnly);
        for _ in 0..3 {
            full.push(e);
            lean.push(e);
        }
        assert_eq!(full.digest(), lean.digest());
        assert_eq!(full.count(), 3);
        assert_eq!(full.bytes().len(), 3 * EVENT_LEN);
        assert!(lean.bytes().is_empty());
        let enc = e.encode();
        assert_eq!(enc[0], EventKind::Note as u8);
        assert_eq!(&enc[9..17], &3u64.to_le_bytes());
    }

    #[test]
    fn clones_share_the_frozen_prefix() {
        let mut t = Trace::new(TraceMode::Full);
        for i in 0..(CHUNK as u64 * 2 + 5) {
            t.push(Event {
                kind: EventKind::Note,
                task: 0,
                proc: 0,
                a: i,
                b: 0,
                c: 0,
            });
        }
        let img = t.clone();
        assert!(Arc::ptr_eq(&t.frozen[1], &img.frozen[1]));
        assert_eq!(img.events().len(), CHUNK * 2 + 5);
        assert_eq!(img.events()[CHUNK].a, CHUNK as u64);
        assert_eq!(img.bytes().len(), (CHUNK * 2 + 5) * EVENT_LEN);
        assert_eq!(le_padded(&[1, 2]), 0x0201);
        assert_eq!(le_padded(&[0xFF; 9]), u64::MAX);
    }
}
