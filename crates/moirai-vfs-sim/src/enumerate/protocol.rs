//! The trace family at M0 ([F13 §1.4] "The toy vehicle", [F16 §17.2] "Where the detectors live"): generic predicates
//! of I-G4 and I-G6 over the simulator's lock, flush and namespace events and over the decoded `HEAD` slot writes, and
//! [F03 §3.1] rule 2 over the probe rounds, for any subject that declares the store protocol it follows ([`Protocol`]).
//!
//! # Inputs
//!
//! - The simulator's own events, each with the task and process that made it (FM-11: several clients of one process
//!   are told apart by their tasks): `Granted`, `Released`, `LockUnlock`, `LockWait`, `FlushStart`, `FlushEnd`,
//!   `InFlight`, `NsOp`, `Return`, `ProcEnd`, `Crash`.
//! - The world's notes that bracket the steps of the composites `create_extent` and `recycle_extent` ([F15 §5.2]):
//!   [`crate::NOTE_COMPOSITE`] and [`crate::NOTE_COMPOSITE_END`].
//! - The notes the world writes under its [`crate::Watch`]: [`crate::NOTE_CLASS`] (which nodes are slot files and log
//!   extents, and whether the store is discoverable), [`crate::NOTE_SLOT_WRITE`] (every slot write with both slots
//!   before and after it, decoded by the subject's pure [`Protocol::decode_slot`]) and [`crate::NOTE_PROBE`].
//! - Two notes the subject writes through [`crate::SimVfs::note`], in the enumerator's vocabulary: [`NOTE_PHASE`] and
//!   [`NOTE_MAINT_DECISION`].
//!
//! A holding is keyed by the lock byte and remembers the process, client and task that obtained it; a call is judged
//! against the holdings of its own task. A process's holdings end at its death (its bytes are then a dead holder's,
//! FM-8.1, and it makes no more calls), every holding at a system crash.
//!
//! # Predicates
//!
//! | Rule | Violation |
//! |---|---|
//! | P-2, I-G4 | a flush (`sync`, `sync_dir`, `sync_group`), a lock wait, a namespace change or an extent preparation (`create_extent`, `recycle_extent`) by a task that holds the writer byte |
//! | P-1 | a lock wait on a byte other than the writer and the flush byte (the grant table also panics on it) |
//! | I-G4, P-41, P-72 | a flush of a log extent by a task without the flush byte while the store is discoverable |
//! | I-G4 | a log flush starting while another task's log flush is in flight |
//! | I-G4, P-27, P-43 | a write to a log extent by a task without the writer byte while the store is discoverable, other than a step of an extent preparation |
//! | P-72, P-96 | an extent preparation of a log extent at its log name by a task without the flush byte while the store is discoverable (a spare is prepared under a `tmp/` name, P-96) |
//! | P-25, P-51 | a [`NOTE_PHASE`] note (phase 1 or 3) by a task that holds the writer or the flush byte |
//! | P-4, P-48 | a slot write by a task without the writer byte |
//! | P-12 | a slot write that is not one whole slot (4,096 bytes at offset 0 or 4,096) |
//! | P-48 | a successful slot write whose slot does not decode as valid; one into the slot that holds the newest valid state; one whose `slot_seq` is not that state's + 1 |
//! | I-G6, P-45 | a decrease of `durable_lsn`, or of any field the decoder lists as monotone (`checkpoint_lsn`, the counters, `fence`, the table pointers, `next_file_no`, `next_ref_id`, `active_log`, the HLC maxima) |
//! | I-G6, P-49 | `committed_lsn` below `durable_lsn`; a decrease of `committed_lsn` with no crash, failed flush or external act before it |
//! | P-48, P-66, U1 | a changed `boot_id`, except the first write of boot-change recovery by a Known-boot process, which writes its own boot identity over another |
//! | [F03 §3.1] rule 2, [F17 §5.3] | an automatic maintenance decision below the quiet cap ([`NOTE_MAINT_DECISION`]) whose probe round missed a quiet byte or found one `Held` or `Unknown` |
//!
//! The writer-byte rule of I-G4 covers the writes of the log protocol: a group, a pad and an extent head (P-37, P-9,
//! P-97) and the flush holder's re-write of the pending range (P-43). The zeros that `create_extent` and `recycle_extent`
//! write while a rotation makes `log.<m>` ready are not among them: [F16] P-72 step 2 makes them under the flush byte,
//! before any group lands in the extent, and P-2 forbids them under the writer byte. The simulator brackets a
//! preparation's steps by the composite's notes ([F15 §5.2]), so its writes are judged by the preparation's rules (P-2,
//! P-72) instead.
//!
//! The comparisons with the newest valid slot before a write use the file's cache. Where the writer may have read other
//! bytes they are skipped: a sector of the slot file was poisoned (FM-3.2: reads draw from K), an external actor acted
//! on it (FM-10), or one slot was fatal (the repair path of [F16] P-85 trusts neither slot). Poisoning ends at a
//! re-write (FM-3.5), so the skip ends with it: once both slots have been re-written the comparisons resume, whatever
//! failed elsewhere (a failed log flush or `sync_dir` poisons no slot, FM-3.1, FM-3.7). A slot write that a crash cut
//! is judged in the run that completes it; a cut write leaves no state to compare.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use moirai_vfs::LockByte;

use super::refusal::Facts;
use crate::trace::{
    CAPTURE_CRASHED, CAPTURE_OK, CLASS_DISCOVERABLE, CLASS_LOG, Event, EventKind, NOTE_COMPOSITE,
    NOTE_COMPOSITE_END, NOTE_PROBE, NOTE_SLOT_WRITE, SlotCapture,
};
use crate::world::CallKind;

/// The store protocol a subject follows ([F16]): what the trace predicates need to know of it.
#[derive(Clone, Debug)]
pub struct Protocol {
    /// The directory of the log extents ([F02 §6]: the store directory).
    pub log_dir: PathBuf,
    /// The prefix of a log extent's name there (`log.`).
    pub log_prefix: String,
    /// The pure decoder of one 4,096-byte slot ([F04 §3], [F04 §7]): the subject supplies it, the predicates over the
    /// decoded writes are the enumerator's ([F13 §1.4]).
    pub decode_slot: fn(&[u8]) -> SlotDecode,
}

/// A slot as [`Protocol::decode_slot`] classifies it ([F04 §7]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlotDecode {
    /// Absent: wrong magic, checksum mismatch, format 0 (torn, never written, damaged).
    Absent,
    /// Fatal: it passes its checksum but fails check 3, 4 or 5.
    Fatal,
    /// Valid.
    Valid(SlotView),
}

/// The fields of a valid slot the predicates compare ([F04 §3.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotView {
    /// `slot_seq`.
    pub slot_seq: u64,
    /// `committed_lsn`.
    pub committed_lsn: u64,
    /// `durable_lsn`.
    pub durable_lsn: u64,
    /// `boot_id`, all 16 bytes.
    pub boot_id: [u8; 16],
    /// Every other field no publish decreases ([F04 §9.1], I-G6), by name: `checkpoint_lsn`, `commit_seq`, `next_id`,
    /// `next_anchor`, `fence`, `active_log`, the table pointers, `next_file_no`, `next_ref_id`, `hlc_seq`,
    /// `hlc_commit` — those the subject's slot has.
    pub monotone: Vec<(&'static str, u64)>,
}

/// The tag of the note a subject writes ([`crate::SimVfs::note`]) when one of its tasks runs phase 1 or phase 3 of a
/// write ([F16 §5.1], [F16 §5.5]): `b` the phase (1 or 3), `c` 0. The task then holds neither the writer nor the flush
/// byte (P-25, P-51).
pub const NOTE_PHASE: u64 = 0x454E_554D_5052_0001;

/// The tag of the note a subject writes when one of its tasks decides to run maintenance ([F17 §5.2]), after its probe
/// round and before the first record of the run: `b` the flags [`MAINT_AUTOMATIC`] and [`MAINT_BELOW_CAP`], `c` 0. An
/// automatic decision below the cap is one quiet mode defers ([F03 §3.1] rule 2, [F17 §5.3]): its round must have
/// probed all nine quiet bytes and found each `Free`.
pub const NOTE_MAINT_DECISION: u64 = 0x454E_554D_5052_0002;

/// [`NOTE_MAINT_DECISION`] flag: a trigger decided it ([F17 §5.2]), not an explicit command.
pub const MAINT_AUTOMATIC: u64 = 1;
/// [`NOTE_MAINT_DECISION`] flag: a delta checkpoint or runtime-only fold below the quiet cap ([F17 §5.3]).
pub const MAINT_BELOW_CAP: u64 = 2;

/// One client's grant: the process, the client and the task that obtained it.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Hold {
    proc: u32,
    client: u64,
    task: u32,
}

fn task_name(t: u32) -> String {
    if t == u32::MAX {
        "driver".to_owned()
    } else {
        t.to_string()
    }
}

/// Whether a call code names an extent preparation's composite ([OS/fs §4.5], [F15 §5.2]).
fn is_preparation(code: u64) -> bool {
    code == CallKind::CreateExtent as u64 || code == CallKind::RecycleExtent as u64
}

fn preparation_name(code: u64) -> &'static str {
    if code == CallKind::CreateExtent as u64 {
        "create_extent"
    } else {
        "recycle_extent"
    }
}

fn byte_name(b: u64) -> String {
    LockByte::from_offset(b).map_or_else(|| format!("{b:#x}"), |l| format!("{l:?}"))
}

/// The streaming judge of one world's trace: the avail facts always, the trace predicates when the subject declares its
/// protocol. Cloneable, so that a crash image's prefix is judged once and every crash state continues from it.
#[derive(Clone)]
pub(crate) struct TraceJudge {
    decode: Option<fn(&[u8]) -> SlotDecode>,
    /// The faults so far ([`Facts`]), and the classes of the nodes.
    pub(crate) facts: Facts,
    /// The grants held, by (`LOCK` node, byte).
    holds: BTreeMap<(u64, u64), Hold>,
    /// The log flushes in flight, by (process, task): the nodes being flushed.
    log_flushes: BTreeMap<(u32, u32), Vec<u64>>,
    /// The (process, task) pairs inside an extent preparation (`create_extent`, `recycle_extent`): between the
    /// composite's notes, their writes are the preparation's steps ([F15 §5.2]).
    preparing: BTreeSet<(u32, u32)>,
    /// The latest answer per quiet byte of each (process, task) since its last decision or phase-3 note.
    probes: BTreeMap<(u32, u32), [Option<u64>; 9]>,
    /// The violations found so far.
    pub(crate) msgs: Vec<String>,
    /// Successful whole-slot writes judged.
    pub(crate) publishes: u64,
}

impl TraceJudge {
    /// A judge for a world with `protocol` (`None`: the facts alone) whose watch has slot files if `slots_watched`.
    pub(crate) fn new(protocol: Option<&Protocol>, slots_watched: bool) -> TraceJudge {
        TraceJudge {
            decode: protocol.map(|p| p.decode_slot),
            facts: Facts::new(slots_watched),
            holds: BTreeMap::new(),
            log_flushes: BTreeMap::new(),
            preparing: BTreeSet::new(),
            probes: BTreeMap::new(),
            msgs: Vec::new(),
            publishes: 0,
        }
    }

    /// Takes in `events`, in order; `capture` resolves a [`NOTE_SLOT_WRITE`] note's capture.
    pub(crate) fn feed(
        &mut self,
        events: &[Event],
        capture: &dyn Fn(u64) -> Option<Arc<SlotCapture>>,
    ) {
        for e in events {
            let cap = if e.kind == EventKind::Note && e.a == NOTE_SLOT_WRITE {
                capture(e.c)
            } else {
                None
            };
            if self.decode.is_some() {
                self.protocol_event(e, cap.as_deref());
            }
            self.facts.feed(e, cap.as_deref());
        }
    }

    /// Whether `task` of `proc` holds `byte` (of any `LOCK`).
    fn holds(&self, proc: u32, task: u32, byte: LockByte) -> bool {
        let b = byte.offset();
        self.holds
            .iter()
            .any(|(&(_, hb), h)| hb == b && h.proc == proc && h.task == task)
    }

    fn who(proc: u32, task: u32) -> String {
        format!("process {proc} task {}", task_name(task))
    }

    fn protocol_event(&mut self, e: &Event, cap: Option<&SlotCapture>) {
        let (p, t) = (e.proc, e.task);
        match e.kind {
            EventKind::Granted => {
                self.holds.insert(
                    (e.a, e.b),
                    Hold {
                        proc: p,
                        client: e.c,
                        task: t,
                    },
                );
            }
            EventKind::Released => {
                if self
                    .holds
                    .get(&(e.a, e.b))
                    .is_some_and(|h| h.proc == p && h.client == e.c)
                {
                    self.holds.remove(&(e.a, e.b));
                }
            }
            EventKind::LockUnlock => {
                if self.holds.get(&(e.a, e.b)).is_some_and(|h| h.proc == p) {
                    self.holds.remove(&(e.a, e.b));
                }
            }
            EventKind::ProcEnd => {
                self.holds.retain(|_, h| h.proc != p);
                self.log_flushes.retain(|&(lp, _), _| lp != p);
                self.preparing.retain(|&(lp, _)| lp != p);
                self.probes.retain(|&(lp, _), _| lp != p);
            }
            EventKind::Crash => {
                self.holds.clear();
                self.log_flushes.clear();
                self.preparing.clear();
                self.probes.clear();
            }
            EventKind::Note if e.a == NOTE_COMPOSITE && is_preparation(e.b) => {
                if p != u32::MAX && self.holds(p, t, LockByte::Writer) {
                    self.msgs.push(format!(
                        "P-2 (I-G4): {} starts {} while holding the writer byte: a client that holds it never \
                         prepares an extent ([F16] P-2, P-72 step 2)",
                        TraceJudge::who(p, t),
                        preparation_name(e.b)
                    ));
                }
                self.preparing.insert((p, t));
            }
            EventKind::Note if e.a == NOTE_COMPOSITE_END && is_preparation(e.b) => {
                self.preparing.remove(&(p, t));
                let class = self.facts.class(e.c);
                if e.c != 0
                    && class & CLASS_LOG != 0
                    && class & CLASS_DISCOVERABLE != 0
                    && !self.holds(p, t, LockByte::Flush)
                {
                    self.msgs.push(format!(
                        "P-72: {} prepares log extent node {} by {} at its log name without the flush byte: a \
                         rotation makes log.<m> ready under the flush byte, and a spare is prepared under a tmp/ \
                         name ([F16] P-72 step 2, P-96)",
                        TraceJudge::who(p, t),
                        e.c,
                        preparation_name(e.b)
                    ));
                }
            }
            EventKind::FlushStart => self.flush_start(e),
            EventKind::FlushEnd if e.b < 2 => {
                if let Some(v) = self.log_flushes.get_mut(&(p, t))
                    && let Some(i) = v.iter().position(|&n| n == e.a)
                {
                    v.remove(i);
                }
            }
            EventKind::InFlight if e.b == 1 => {
                for ((lp, _), v) in self.log_flushes.iter_mut() {
                    if *lp == p {
                        v.retain(|&n| n != e.a);
                    }
                }
            }
            EventKind::LockWait => {
                if e.b != LockByte::Writer.offset() && e.b != LockByte::Flush.offset() {
                    self.msgs.push(format!(
                        "P-1: {} waits for the {} byte; a client waits only for the writer and the flush byte \
                         ([F16] P-1, [OS/lock §6] item 1)",
                        TraceJudge::who(p, t),
                        byte_name(e.b)
                    ));
                }
                if self.holds(p, t, LockByte::Writer) {
                    self.msgs.push(format!(
                        "P-2 (I-G4): {} waits for the {} byte while holding the writer byte ([F16] P-2)",
                        TraceJudge::who(p, t),
                        byte_name(e.b)
                    ));
                }
            }
            EventKind::NsOp if p != u32::MAX && self.holds(p, t, LockByte::Writer) => {
                self.msgs.push(format!(
                    "P-2 (I-G4): {} changes the namespace (operation {}, kind {}, node {}) while holding the writer \
                     byte ([F16] P-2)",
                    TraceJudge::who(p, t),
                    e.a,
                    e.b,
                    e.c
                ));
            }
            EventKind::Return if e.a == CallKind::Write as u64 => {
                let class = self.facts.class(e.b);
                if class & CLASS_LOG != 0
                    && class & CLASS_DISCOVERABLE != 0
                    && !self.preparing.contains(&(p, t))
                    && !self.holds(p, t, LockByte::Writer)
                {
                    self.msgs.push(format!(
                        "I-G4: {} writes log extent node {} without the writer byte ([F16] P-27, P-43)",
                        TraceJudge::who(p, t),
                        e.b
                    ));
                }
            }
            EventKind::Note if e.a == NOTE_PHASE => {
                for byte in [LockByte::Writer, LockByte::Flush] {
                    if self.holds(p, t, byte) {
                        self.msgs.push(format!(
                            "{}: {} runs phase {} while holding the {byte:?} byte ([F16] P-25, P-51)",
                            if e.b == 3 { "P-51" } else { "P-25" },
                            TraceJudge::who(p, t),
                            e.b
                        ));
                    }
                }
                if e.b == 3 {
                    self.probes.remove(&(p, t));
                }
            }
            EventKind::Note if e.a == NOTE_PROBE => {
                if let Some(LockByte::Quiet(q)) = LockByte::from_offset(e.b) {
                    self.probes.entry((p, t)).or_insert([None; 9])[usize::from(q.get())] =
                        Some(e.c);
                }
            }
            EventKind::Note if e.a == NOTE_MAINT_DECISION => {
                let round = self.probes.remove(&(p, t)).unwrap_or([None; 9]);
                let judged = e.b & MAINT_AUTOMATIC != 0 && e.b & MAINT_BELOW_CAP != 0;
                if judged {
                    let bad: Vec<String> = round
                        .iter()
                        .enumerate()
                        .filter_map(|(k, a)| match a {
                            Some(0) => None,
                            Some(1) => Some(format!("quiet byte {k} Held")),
                            Some(_) => Some(format!("quiet byte {k} Unknown")),
                            None => Some(format!("quiet byte {k} not probed")),
                        })
                        .collect();
                    if !bad.is_empty() {
                        self.msgs.push(format!(
                            "[F03 §3.1] rule 2: {} decided an automatic maintenance run below the quiet cap, but its \
                             probe round found {} ([F17 §5.3]: quiet mode defers it)",
                            TraceJudge::who(p, t),
                            bad.join(", ")
                        ));
                    }
                }
            }
            EventKind::Note if e.a == NOTE_SLOT_WRITE => {
                if let Some(c) = cap {
                    self.slot_write(c);
                }
            }
            _ => {}
        }
    }

    fn flush_start(&mut self, e: &Event) {
        let (p, t) = (e.proc, e.task);
        let what = match e.b {
            0 => "sync(Data)",
            1 => "sync(DataAndMeta)",
            _ => "sync_dir",
        };
        if p != u32::MAX && self.holds(p, t, LockByte::Writer) {
            self.msgs.push(format!(
                "P-2 (I-G4): {} starts a {what} of node {} while holding the writer byte ([F16] P-2, P-44)",
                TraceJudge::who(p, t),
                e.a
            ));
        }
        let class = self.facts.class(e.a);
        if e.b < 2 && class & CLASS_LOG != 0 && class & CLASS_DISCOVERABLE != 0 {
            if !self.holds(p, t, LockByte::Flush) {
                self.msgs.push(format!(
                    "I-G4: {} flushes log extent node {} without the flush byte ([F16] P-41, P-72)",
                    TraceJudge::who(p, t),
                    e.a
                ));
            }
            if let Some((&(op, ot), v)) = self
                .log_flushes
                .iter()
                .find(|&(&k, v)| k != (p, t) && !v.is_empty())
            {
                self.msgs.push(format!(
                    "I-G4: {} starts a log flush of node {} while {} flushes node {}: at most one log flush is in \
                     flight per store",
                    TraceJudge::who(p, t),
                    e.a,
                    TraceJudge::who(op, ot),
                    v[0]
                ));
            }
            self.log_flushes.entry((p, t)).or_default().push(e.a);
        }
    }

    fn slot_write(&mut self, c: &SlotCapture) {
        let who = TraceJudge::who(c.proc, c.task);
        if c.status != CAPTURE_CRASHED && !self.holds(c.proc, c.task, LockByte::Writer) {
            self.msgs.push(format!(
                "P-4: {who} writes slot file node {} without the writer byte ([F16] P-4, P-48)",
                c.node
            ));
        }
        let whole = c.offset.is_multiple_of(4096) && c.len == 4096 && c.offset / 4096 < 2;
        if !whole {
            self.msgs.push(format!(
                "P-12: {who} writes {} bytes at offset {} of slot file node {}: a publish writes one whole slot \
                 ([F04 §2], [F16] P-12)",
                c.len, c.offset, c.node
            ));
            return;
        }
        let Some(decode) = self.decode else {
            return;
        };
        if c.status != CAPTURE_OK {
            return;
        }
        self.publishes += 1;
        let w = (c.offset / 4096) as usize;
        let new = match decode(&c.after[w][..]) {
            SlotDecode::Valid(v) => v,
            other => {
                self.msgs.push(format!(
                    "P-48: {who} wrote slot {w} of node {} as {}: a publisher writes a valid slot ([F04 §7])",
                    c.node,
                    if other == SlotDecode::Fatal {
                        "a fatal slot"
                    } else {
                        "an absent slot"
                    }
                ));
                return;
            }
        };
        if new.committed_lsn < new.durable_lsn {
            self.msgs.push(format!(
                "I-G6: {who} wrote slot {w} with committed_lsn {} below durable_lsn {} ([F04 §5.4], [F16] P-48)",
                new.committed_lsn, new.durable_lsn
            ));
        }
        let before = [decode(&c.before[0][..]), decode(&c.before[1][..])];
        if c.poisoned
            || self.facts.external.contains(&c.node)
            || before.contains(&SlotDecode::Fatal)
        {
            return;
        }
        let base = match (&before[0], &before[1]) {
            (SlotDecode::Valid(a), SlotDecode::Valid(b)) => {
                if a.slot_seq >= b.slot_seq {
                    Some((0, a, a.slot_seq == b.slot_seq))
                } else {
                    Some((1, b, false))
                }
            }
            (SlotDecode::Valid(a), _) => Some((0, a, false)),
            (_, SlotDecode::Valid(b)) => Some((1, b, false)),
            _ => None,
        };
        let Some((bs, base, tie)) = base else {
            return;
        };
        let mut bad = Vec::new();
        if !tie && w == bs {
            bad.push(format!(
                "P-48: it wrote slot {w}, which holds the newest valid state (slot_seq {}) ([F04 §9.1] step 4)",
                base.slot_seq
            ));
        }
        if new.slot_seq != base.slot_seq + 1 {
            bad.push(format!(
                "P-48: slot_seq {} after the newest valid slot's {} (a publish writes S.slot_seq + 1)",
                new.slot_seq, base.slot_seq
            ));
        }
        if new.durable_lsn < base.durable_lsn {
            bad.push(format!(
                "I-G6: durable_lsn decreases from {} to {} ([F16] P-45)",
                base.durable_lsn, new.durable_lsn
            ));
        }
        for &(name, v) in &base.monotone {
            if let Some(&(_, nv)) = new.monotone.iter().find(|(n, _)| *n == name)
                && nv < v
            {
                bad.push(format!(
                    "I-G6: {name} decreases from {v} to {nv} ([F04 §9.1])"
                ));
            }
        }
        let f = &self.facts;
        if new.committed_lsn < base.committed_lsn
            && f.crashes == 0
            && f.failed_flushes == 0
            && f.external.is_empty()
        {
            bad.push(format!(
                "I-G6: committed_lsn decreases from {} to {} with no crash, failed flush or external act before it \
                 (only a lost lazy tail lowers it, [F16] P-49)",
                base.committed_lsn, new.committed_lsn
            ));
        }
        if new.boot_id != base.boot_id {
            let recovery = c
                .boot
                .is_some_and(|b| b == new.boot_id && b != base.boot_id);
            if !recovery {
                bad.push(
                    "boot_id changes, and not to the writer's own boot identity over another: only boot-change \
                     recovery by a Known-boot process changes it ([F04 §5.5], [F16] P-48, P-66, [OS/proc §5] U1)"
                        .to_owned(),
                );
            }
        }
        for b in bad {
            self.msgs
                .push(format!("{b}; {who} wrote slot {w} of node {}", c.node));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Page;
    use crate::trace::{CAPTURE_DIED, CAPTURE_FAILED, CLASS_SLOT, NOTE_CLASS};

    const LOCK: u64 = 2;
    const HEAD: u64 = 3;
    const LOG: u64 = 4;
    const W: u64 = 1 << 62;
    const F: u64 = (1 << 62) + 4;
    const M: u64 = (1 << 62) + 2;

    /// A synthetic slot: `slot_seq` at 0 (0: absent), `committed_lsn` at 8, `durable_lsn` at 16, `boot_id` at 24,
    /// `fence` at 40; byte 4095 = 0xFA marks a fatal slot.
    fn slot(seq: u64, committed: u64, durable: u64, boot: u8, fence: u64) -> Page {
        let mut b = [0u8; 4096];
        b[0..8].copy_from_slice(&seq.to_le_bytes());
        b[8..16].copy_from_slice(&committed.to_le_bytes());
        b[16..24].copy_from_slice(&durable.to_le_bytes());
        b[24..40].fill(boot);
        b[40..48].copy_from_slice(&fence.to_le_bytes());
        Arc::new(b)
    }

    fn fatal() -> Page {
        let mut b = [0u8; 4096];
        b[0] = 9;
        b[4095] = 0xFA;
        Arc::new(b)
    }

    fn decode(b: &[u8]) -> SlotDecode {
        let u = |at: usize| u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"));
        if b[4095] == 0xFA {
            return SlotDecode::Fatal;
        }
        if u(0) == 0 {
            return SlotDecode::Absent;
        }
        let mut boot = [0u8; 16];
        boot.copy_from_slice(&b[24..40]);
        SlotDecode::Valid(SlotView {
            slot_seq: u(0),
            committed_lsn: u(8),
            durable_lsn: u(16),
            boot_id: boot,
            monotone: vec![("fence", u(40))],
        })
    }

    fn proto() -> Protocol {
        Protocol {
            log_dir: PathBuf::from("/s"),
            log_prefix: "log.".to_owned(),
            decode_slot: decode,
        }
    }

    fn ev(kind: EventKind, task: u32, a: u64, b: u64, c: u64) -> Event {
        Event {
            kind,
            task,
            proc: 0,
            a,
            b,
            c,
        }
    }

    fn of(proc: u32, e: Event) -> Event {
        Event { proc, ..e }
    }

    fn classes() -> Vec<Event> {
        vec![
            ev(
                EventKind::Note,
                u32::MAX,
                NOTE_CLASS,
                HEAD,
                CLASS_SLOT | CLASS_DISCOVERABLE,
            ),
            ev(
                EventKind::Note,
                u32::MAX,
                NOTE_CLASS,
                LOG,
                CLASS_LOG | CLASS_DISCOVERABLE,
            ),
        ]
    }

    fn grant(task: u32, byte: u64) -> Event {
        ev(EventKind::Granted, task, LOCK, byte, u64::from(task) + 100)
    }

    fn release(task: u32, byte: u64) -> Event {
        ev(EventKind::Released, task, LOCK, byte, u64::from(task) + 100)
    }

    /// A judged trace: the events, with the slot-write captures their notes name.
    fn judge(events: &[Event], caps: &[SlotCapture]) -> Vec<String> {
        let p = proto();
        let mut j = TraceJudge::new(Some(&p), true);
        let caps: Vec<Arc<SlotCapture>> = caps.iter().cloned().map(Arc::new).collect();
        j.feed(events, &|i| caps.get(i as usize).cloned());
        j.msgs
    }

    fn has(m: &[String], needle: &str) -> bool {
        m.iter().any(|x| x.contains(needle))
    }

    /// A successful write by task 1 of `after` into slot `w`, over the cache `before`.
    fn write(w: u64, before: [Page; 2], after: Page) -> SlotCapture {
        let mut a = before.clone();
        a[w as usize] = after;
        SlotCapture {
            proc: 0,
            task: 1,
            node: HEAD,
            offset: w * 4096,
            len: 4096,
            status: CAPTURE_OK,
            before,
            after: a,
            poisoned: false,
            boot: None,
        }
    }

    fn note(i: u64) -> Event {
        ev(EventKind::Note, 1, NOTE_SLOT_WRITE, HEAD, i)
    }

    /// The publishes `caps` in order, each under its own holding of the writer byte by task 1.
    fn publishes(caps: &[SlotCapture]) -> Vec<String> {
        let mut events = classes();
        for i in 0..caps.len() as u64 {
            events.extend([grant(1, W), note(i), release(1, W)]);
        }
        judge(&events, caps)
    }

    #[test]
    fn a_correct_publish_sequence_passes() {
        let s1 = slot(1, 100, 100, 7, 3);
        let s2 = slot(2, 150, 120, 7, 3);
        let s3 = slot(3, 150, 150, 7, 4);
        let caps = [
            write(1, [s1.clone(), slot(0, 0, 0, 0, 0)], s2.clone()),
            write(0, [s1, s2.clone()], s3.clone()),
            write(1, [s3.clone(), s2], slot(4, 150, 150, 7, 4)),
        ];
        let m = publishes(&caps);
        assert!(m.is_empty(), "{m:?}");
    }

    /// S2B-P-41 ([F16 §17.3] P-45): the ig6 predicate on a synthetic publish sequence that lowers `durable_lsn`.
    #[test]
    fn a_publish_that_lowers_durable_lsn_is_flagged() {
        let s1 = slot(1, 200, 200, 7, 3);
        let s0 = slot(0, 0, 0, 0, 0);
        let caps = [write(1, [s1, s0], slot(2, 200, 150, 7, 3))];
        let m = publishes(&caps);
        assert!(has(&m, "durable_lsn decreases from 200 to 150"), "{m:?}");
    }

    #[test]
    fn every_slot_field_rule_is_checked() {
        let s1 = slot(5, 300, 200, 7, 9);
        let s0 = slot(4, 250, 200, 7, 9);
        // The slot that holds the newest valid state (slot A, seq 5) written over.
        let m = publishes(&[write(0, [s1.clone(), s0.clone()], slot(6, 300, 200, 7, 9))]);
        assert!(has(&m, "holds the newest valid state"), "{m:?}");
        // slot_seq not base + 1 (a stale base: P-48's "a state it read before the current holding").
        let m = publishes(&[write(1, [s1.clone(), s0.clone()], slot(5, 300, 200, 7, 9))]);
        assert!(
            has(&m, "slot_seq 5 after the newest valid slot's 5"),
            "{m:?}"
        );
        // A monotone field (fence) going back.
        let m = publishes(&[write(1, [s1.clone(), s0.clone()], slot(6, 300, 200, 7, 8))]);
        assert!(has(&m, "fence decreases from 9 to 8"), "{m:?}");
        // committed_lsn below durable_lsn.
        let m = publishes(&[write(1, [s1.clone(), s0.clone()], slot(6, 190, 200, 7, 9))]);
        assert!(has(&m, "committed_lsn 190 below durable_lsn 200"), "{m:?}");
        // committed_lsn going back with no crash or failed flush before it ...
        let lower = write(1, [s1.clone(), s0.clone()], slot(6, 250, 200, 7, 9));
        let m = publishes(std::slice::from_ref(&lower));
        assert!(has(&m, "committed_lsn decreases from 300 to 250"), "{m:?}");
        // ... is a lost lazy tail after a crash or a failed flush.
        for before in [
            ev(EventKind::Crash, u32::MAX, 1, 0, 0),
            ev(EventKind::FlushEnd, 2, LOG, 0, 1),
        ] {
            let mut events = classes();
            events.extend([before, grant(1, W), note(0), release(1, W)]);
            let m = judge(&events, std::slice::from_ref(&lower));
            assert!(m.is_empty(), "{m:?}");
        }
        // A written slot that is not valid.
        let m = publishes(&[write(1, [s1.clone(), s0.clone()], fatal())]);
        assert!(has(&m, "as a fatal slot"), "{m:?}");
        let m = publishes(&[write(1, [s1.clone(), s0.clone()], slot(0, 0, 0, 0, 0))]);
        assert!(has(&m, "as an absent slot"), "{m:?}");
        // A partial-slot write.
        let mut odd = write(1, [s1.clone(), s0.clone()], slot(6, 300, 200, 7, 9));
        odd.len = 512;
        let m = publishes(&[odd]);
        assert!(has(&m, "P-12"), "{m:?}");
    }

    #[test]
    fn boot_id_changes_only_by_a_known_boot_recovery_writing_its_own_identity() {
        let s1 = slot(5, 300, 200, 7, 9);
        let s0 = slot(4, 250, 200, 7, 9);
        let changed = write(1, [s1.clone(), s0.clone()], slot(6, 300, 200, 8, 9));
        let m = publishes(std::slice::from_ref(&changed));
        assert!(has(&m, "boot_id changes"), "{m:?}");
        // P-66's first write by a Known-boot process of boot 8.
        let mut known = changed.clone();
        known.boot = Some([8; 16]);
        assert!(publishes(&[known]).is_empty());
        // A Known-boot process of boot 9 writing boot 8: wrong identity.
        let mut other = changed;
        other.boot = Some([9; 16]);
        let m = publishes(&[other]);
        assert!(has(&m, "boot_id changes"), "{m:?}");
        // The identity compared is all 16 bytes.
        let mut half = slot(6, 300, 200, 7, 9);
        Arc::make_mut(&mut half)[39] = 8;
        let m = publishes(&[write(1, [s1, s0], half)]);
        assert!(has(&m, "boot_id changes"), "{m:?}");
    }

    /// E1 (P-4): a slot write without the writer byte, while another task holds it, with no note from the writer.
    #[test]
    fn a_slot_write_without_the_writer_byte_is_flagged() {
        let s1 = slot(1, 100, 100, 7, 3);
        let s0 = slot(0, 0, 0, 0, 0);
        let caps = [write(1, [s1, s0], slot(2, 100, 100, 7, 3))];
        let mut events = classes();
        events.extend([grant(2, W), note(0), release(2, W)]);
        let m = judge(&events, &caps);
        assert!(has(&m, "P-4: process 0 task 1 writes slot file"), "{m:?}");
        // A write a death cut is judged as the writer's too; one a crash cut is judged where it completes.
        let mut died = caps[0].clone();
        died.status = CAPTURE_DIED;
        let mut crashed = caps[0].clone();
        crashed.status = crate::trace::CAPTURE_CRASHED;
        let mut events = classes();
        events.extend([note(0), note(1)]);
        let m = judge(&events, &[died, crashed]);
        assert_eq!(
            m.iter().filter(|x| x.starts_with("P-4")).count(),
            1,
            "{m:?}"
        );
    }

    /// E3: poisoning is the slot file's own (FM-3.2), and it ends with the re-writes (FM-3.5); a failed log flush or
    /// `sync_dir` turns no comparison off.
    #[test]
    fn comparisons_are_skipped_only_while_the_slot_file_is_poisoned() {
        let s1 = slot(5, 300, 200, 7, 9);
        let s0 = slot(4, 250, 200, 7, 9);
        let mut poisoned = write(1, [s1.clone(), s0.clone()], slot(5, 300, 200, 7, 9));
        poisoned.poisoned = true;
        assert!(publishes(&[poisoned]).is_empty());
        // A failed log flush and a failed sync_dir before a stale publish: still judged.
        let stale = write(1, [s1, s0], slot(5, 300, 100, 7, 9));
        let mut events = classes();
        events.extend([
            ev(EventKind::FlushEnd, 2, LOG, 0, 1),
            ev(EventKind::FlushEnd, 2, 9, 2, 1),
            grant(1, W),
            note(0),
            release(1, W),
        ]);
        let m = judge(&events, &[stale]);
        assert!(has(&m, "durable_lsn decreases"), "{m:?}");
        assert!(has(&m, "slot_seq 5 after"), "{m:?}");
        // A fatal slot before the write (the repair path trusts neither) skips the comparisons.
        let repair = write(1, [fatal(), slot(4, 250, 200, 7, 9)], slot(1, 90, 90, 7, 0));
        assert!(publishes(&[repair]).is_empty());
        // A failed write is not compared, but its writer must hold the writer byte.
        let mut failed = write(
            1,
            [slot(1, 1, 1, 7, 0), slot(0, 0, 0, 0, 0)],
            slot(1, 1, 1, 7, 0),
        );
        failed.status = CAPTURE_FAILED;
        assert!(publishes(&[failed]).is_empty());
    }

    /// E2: two clients of one process: task 1 holds the writer byte while task 2 flushes — fine; task 1 itself
    /// flushing, waiting or renaming — a violation.
    #[test]
    fn writer_byte_rules_are_judged_per_task() {
        let mut events = classes();
        events.extend([
            grant(1, W),
            grant(2, F),
            ev(EventKind::FlushStart, 2, LOG, 0, 1),
            ev(EventKind::FlushEnd, 2, LOG, 0, 0),
        ]);
        assert!(judge(&events, &[]).is_empty(), "{:?}", judge(&events, &[]));
        events.extend([
            ev(EventKind::FlushStart, 1, 9, 2, 2),
            ev(EventKind::LockWait, 1, LOCK, F, 0),
            ev(EventKind::NsOp, 1, 7, 1, 9),
        ]);
        let m = judge(&events, &[]);
        assert!(has(&m, "task 1 starts a sync_dir"), "{m:?}");
        assert!(
            has(&m, "task 1 waits for the Flush byte while holding"),
            "{m:?}"
        );
        assert!(has(&m, "task 1 changes the namespace"), "{m:?}");
        assert!(!has(&m, "task 2"), "{m:?}");
        // Another process's task with the same number holds nothing.
        let mut events = classes();
        events.extend([
            grant(1, W),
            of(1, ev(EventKind::Granted, 1, LOCK, F, 7)),
            of(1, ev(EventKind::FlushStart, 1, LOG, 0, 1)),
        ]);
        assert!(judge(&events, &[]).is_empty(), "{:?}", judge(&events, &[]));
        // A holding ends with its release, its kernel unlock, or its process.
        let mut events = classes();
        events.extend([
            grant(1, W),
            ev(EventKind::LockUnlock, 1, LOCK, W, 0),
            ev(EventKind::FlushStart, 1, 9, 2, 2),
            grant(1, W),
            ev(EventKind::ProcEnd, u32::MAX, 1, 1, 0),
            ev(EventKind::FlushStart, 1, 9, 2, 2),
        ]);
        assert!(judge(&events, &[]).is_empty(), "{:?}", judge(&events, &[]));
    }

    /// E4: the flush byte and the one-log-flush rule, a wait on another byte, and phases 1 and 3 under the flush byte.
    #[test]
    fn flush_byte_rules() {
        // A log flush without the flush byte, alone.
        let mut events = classes();
        events.push(ev(EventKind::FlushStart, 1, LOG, 0, 1));
        let m = judge(&events, &[]);
        assert!(
            has(&m, "flushes log extent node 4 without the flush byte"),
            "{m:?}"
        );
        // Before the store is discoverable (init), no byte is needed.
        let events = [
            ev(EventKind::Note, u32::MAX, NOTE_CLASS, LOG, CLASS_LOG),
            ev(EventKind::FlushStart, 1, LOG, 0, 1),
            ev(EventKind::Return, 1, CallKind::Write as u64, LOG, 0),
        ];
        assert!(judge(&events, &[]).is_empty());
        // Two log flushes in flight by two tasks; the second has the byte through a bug elsewhere.
        let mut events = classes();
        events.extend([
            grant(1, F),
            ev(EventKind::FlushStart, 1, LOG, 0, 1),
            grant(2, F),
            ev(EventKind::FlushStart, 2, LOG, 0, 2),
        ]);
        let m = judge(&events, &[]);
        assert!(has(&m, "at most one log flush"), "{m:?}");
        // A flush that ended, or died with its process, is no longer in flight.
        let mut events = classes();
        events.extend([
            grant(1, F),
            ev(EventKind::FlushStart, 1, LOG, 0, 1),
            ev(EventKind::InFlight, u32::MAX, LOG, 1, 1),
            release(1, F),
            grant(2, F),
            ev(EventKind::FlushStart, 2, LOG, 0, 2),
        ]);
        assert!(judge(&events, &[]).is_empty(), "{:?}", judge(&events, &[]));
        // A log write without the writer byte.
        let mut events = classes();
        events.push(ev(EventKind::Return, 1, CallKind::Write as u64, LOG, 0));
        let m = judge(&events, &[]);
        assert!(
            has(&m, "writes log extent node 4 without the writer byte"),
            "{m:?}"
        );
        // A wait on the maintenance byte.
        let mut events = classes();
        events.push(ev(EventKind::LockWait, 1, LOCK, M, 0));
        let m = judge(&events, &[]);
        assert!(
            has(&m, "P-1: process 0 task 1 waits for the Maintenance byte"),
            "{m:?}"
        );
        // Phase 1 and phase 3 under the flush byte.
        let mut events = classes();
        events.extend([
            grant(1, F),
            ev(EventKind::Note, 1, NOTE_PHASE, 1, 0),
            ev(EventKind::Note, 1, NOTE_PHASE, 3, 0),
        ]);
        let m = judge(&events, &[]);
        assert!(has(&m, "P-25") && has(&m, "P-51"), "{m:?}");
        let mut events = classes();
        events.extend([grant(1, M), ev(EventKind::Note, 1, NOTE_PHASE, 3, 0)]);
        assert!(
            judge(&events, &[]).is_empty(),
            "the maintenance byte is allowed"
        );
    }

    fn start(task: u32, call: CallKind) -> Event {
        ev(EventKind::Note, task, NOTE_COMPOSITE, call as u64, 0)
    }

    fn end(task: u32, call: CallKind, node: u64) -> Event {
        ev(EventKind::Note, task, NOTE_COMPOSITE_END, call as u64, node)
    }

    fn done(task: u32, call: CallKind, node: u64) -> Event {
        ev(EventKind::Return, task, call as u64, node, 0)
    }

    /// I-G4 (WP-40 closure): the zeros of an extent preparation are no log writes ([F16] P-72 step 2, [F15 §5.2]). A
    /// rotation's `create_extent` and `recycle_extent` of a log extent under the flush byte pass, and so does a spare's
    /// preparation under a `tmp/` name with the maintenance byte only (P-96); a preparation under the writer byte (P-2)
    /// or at the log name without the flush byte (P-72) is caught; a write without the writer byte outside a
    /// preparation is still a log write without it (I-G4).
    #[test]
    fn extent_preparations_are_judged_by_their_own_rules() {
        const TMP: u64 = 9;
        let w = |task| done(task, CallKind::Write, LOG);
        // create_extent at the log name (the exclusive create, then the zero writes), then recycle_extent.
        let rotation = |task| {
            vec![
                start(task, CallKind::CreateExtent),
                ev(EventKind::Point, task, 1, CallKind::CreateNew as u64, 0),
                done(task, CallKind::CreateNew, 0),
                w(task),
                w(task),
                end(task, CallKind::CreateExtent, LOG),
                start(task, CallKind::RecycleExtent),
                w(task),
                end(task, CallKind::RecycleExtent, LOG),
            ]
        };
        let mut events = classes();
        events.push(grant(1, F));
        events.extend(rotation(1));
        events.push(release(1, F));
        // A spare: create_extent under tmp/ (no class), maintenance byte only.
        events.extend([
            grant(1, M),
            start(1, CallKind::CreateExtent),
            done(1, CallKind::Write, TMP),
            end(1, CallKind::CreateExtent, TMP),
            release(1, M),
        ]);
        let m = judge(&events, &[]);
        assert!(m.is_empty(), "{m:?}");
        // Under the writer byte (and the flush byte): P-2 at each preparation's start.
        let mut events = classes();
        events.extend([grant(1, F), grant(1, W)]);
        events.extend(rotation(1));
        let m = judge(&events, &[]);
        assert!(
            has(
                &m,
                "P-2 (I-G4): process 0 task 1 starts create_extent while holding the writer byte"
            ),
            "{m:?}"
        );
        assert!(has(&m, "starts recycle_extent while holding"), "{m:?}");
        assert_eq!(m.len(), 2, "{m:?}");
        // Without the flush byte, at the log name: P-72, once per preparation; the zeros are not I-G4 writes.
        let mut events = classes();
        events.extend(rotation(1));
        let m = judge(&events, &[]);
        assert!(
            has(
                &m,
                "P-72: process 0 task 1 prepares log extent node 4 by create_extent"
            ),
            "{m:?}"
        );
        assert!(has(&m, "by recycle_extent"), "{m:?}");
        assert_eq!(m.len(), 2, "{m:?}");
        // Before the store is discoverable (init), no byte is needed.
        let events = [
            ev(EventKind::Note, u32::MAX, NOTE_CLASS, LOG, CLASS_LOG),
            start(1, CallKind::CreateExtent),
            w(1),
            end(1, CallKind::CreateExtent, LOG),
        ];
        assert!(judge(&events, &[]).is_empty());
        // A preparation is one task's: another task's write meanwhile, and the task's own write after the `Return`, are
        // log writes judged by I-G4.
        let mut events = classes();
        events.extend([
            grant(1, F),
            start(1, CallKind::CreateExtent),
            w(2),
            w(1),
            end(1, CallKind::CreateExtent, LOG),
            w(1),
        ]);
        let m = judge(&events, &[]);
        assert!(
            has(
                &m,
                "I-G4: process 0 task 2 writes log extent node 4 without the writer byte"
            ),
            "{m:?}"
        );
        assert_eq!(
            m.iter()
                .filter(|x| x.contains("task 1 writes log extent"))
                .count(),
            1,
            "{m:?}"
        );
        // A preparation that its process's death or a system crash cut ends with it.
        for end in [
            ev(EventKind::ProcEnd, u32::MAX, 1, 0, 0),
            ev(EventKind::Crash, u32::MAX, 1, 0, 0),
        ] {
            let mut events = classes();
            events.extend([
                grant(1, F),
                start(1, CallKind::CreateExtent),
                w(1),
                end,
                w(1),
            ]);
            let m = judge(&events, &[]);
            assert_eq!(m.len(), 1, "{m:?}");
            assert!(has(&m, "task 1 writes log extent node 4"), "{m:?}");
        }
    }

    fn probe(task: u32, k: u8, answer: u64) -> Event {
        let byte = LockByte::Quiet(moirai_vfs::QuietIndex::new(k).expect("k < 9")).offset();
        ev(EventKind::Note, task, NOTE_PROBE, byte, answer)
    }

    fn decision(task: u32, flags: u64) -> Event {
        ev(EventKind::Note, task, NOTE_MAINT_DECISION, flags, 0)
    }

    /// E9 ([F03 §3.1] rule 2): the probe round of an automatic decision below the cap.
    #[test]
    fn an_automatic_decision_below_the_cap_needs_nine_free_probes() {
        let both = MAINT_AUTOMATIC | MAINT_BELOW_CAP;
        let mut full: Vec<Event> = (0..9).map(|k| probe(1, k, 0)).collect();
        full.push(decision(1, both));
        assert!(judge(&full, &[]).is_empty(), "{:?}", judge(&full, &[]));
        // A round that probed only the first quiet byte while another is held elsewhere.
        let m = judge(
            &[
                probe(1, 0, 0),
                grant(
                    2,
                    LockByte::Quiet(moirai_vfs::QuietIndex::new(4).expect("4")).offset(),
                ),
                decision(1, both),
            ],
            &[],
        );
        assert!(has(&m, "quiet byte 4 not probed"), "{m:?}");
        // A Held or Unknown answer.
        let mut held = full.clone();
        held[3] = probe(1, 3, 1);
        held[7] = probe(1, 7, 2);
        let m = judge(&held, &[]);
        assert!(
            has(&m, "quiet byte 3 Held") && has(&m, "quiet byte 7 Unknown"),
            "{m:?}"
        );
        // Not judged: an explicit run, or one at the cap.
        assert!(judge(&[probe(1, 0, 1), decision(1, MAINT_BELOW_CAP)], &[]).is_empty());
        assert!(judge(&[probe(1, 0, 1), decision(1, MAINT_AUTOMATIC)], &[]).is_empty());
        // A round is the task's own and ends with its decision: the next decision needs a round of its own.
        let mut twice = full.clone();
        twice.push(decision(1, both));
        let m = judge(&twice, &[]);
        assert!(has(&m, "quiet byte 0 not probed"), "{m:?}");
        let mut other = full;
        other.pop();
        other.push(decision(2, both));
        let m = judge(&other, &[]);
        assert!(has(&m, "process 0 task 2"), "{m:?}");
    }

    #[test]
    fn without_a_protocol_only_the_facts_are_kept() {
        let mut j = TraceJudge::new(None, true);
        let mut events = classes();
        events.extend([
            ev(EventKind::FlushStart, 1, LOG, 0, 1),
            ev(EventKind::FlushEnd, 1, LOG, 0, 1),
        ]);
        j.feed(&events, &|_| None);
        assert!(j.msgs.is_empty());
        assert_eq!(j.facts.failed_flushes, 1);
    }
}
