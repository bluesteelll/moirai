//! The write ([F16 §5]): phase 1 before any lock, phase 2a under the writer byte (the scan to the end of the valid log,
//! the scratch layer, idempotency, re-validation, allocation, the HLC, placement and rotation, the append), phase 2b
//! through the flush byte (the covered test, the scan and re-write under the writer byte, one flush outside it, the
//! publish, the identity check), phase 3 after the acknowledgement ([`Toy::maintain`]), and the publish itself
//! ([F16 §5.4]).

use std::collections::BTreeMap;

use moirai_vfs::{LockByte, Stamp, StoreVolume, SyncKind, Vfs, VfsErrorKind};

use crate::bugs::{Bug, Bugs};
use crate::codec::Writer;
use crate::format::{
    CommitRec, Counters, ExtentHeadRec, HEAD_GROUP, MIN_GROUP, ROTATION_RESERVE, Rec, encode_group,
    group_durable, group_len, kind,
};
use crate::head::{FLAG_QUIET, FLAG_READONLY, FLAG_RETIRED, HEAD_LEN, SLOT_LEN, Slot, SlotRead};
use crate::ops::{Alloc, Decision, Hlc, Op, build, decide, park_group};
use crate::state::{Group, State, Undo};
use crate::store::{ScanCtx, Stop, Toy, ToyError, log_name};
use crate::tap::{Note, Tap};

/// A successful operation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Done {
    /// The result is an idempotent replay of an earlier operation's ([F16] P-32, P-33).
    pub replayed: bool,
    /// The operation's result.
    pub result: u64,
    /// The first byte of the operation's own groups (0 for a replay).
    pub start: u64,
    /// E_g: the end of the operation's last group.
    pub end: u64,
    /// The chain value at `start`.
    pub chain_in: u64,
    /// The chain value at E_g, checked by the identity check.
    pub chain_out: u64,
}

/// What phase 2a appended.
#[derive(Clone, Debug)]
pub(crate) struct Appended {
    start: u64,
    end: u64,
    chain_in: u64,
    chain: u64,
    /// E of the store the group was appended to (the identity check's extent arithmetic).
    e: u64,
    durable: bool,
    /// Published at once by P-38: whether that publish covered the group.
    published: Option<bool>,
    holding_flush: bool,
    result: u64,
    /// The bytes of the append's writes, by lsn: kept only for P-47's seeded bug, which writes them again after a loss
    /// (empty with the switch off).
    written: Vec<(u64, Vec<u8>)>,
    later: Option<Op>,
}

/// How phase 2a ended.
enum Step {
    Appended(Appended),
    /// An idempotency hit: the stored result, and the pending group it lies in (end, chain, E), if pending.
    Hit(u64, Option<(u64, u64, u64)>),
    /// The view was stale under the writer byte: phase 1 runs again.
    Rerun,
}

/// How phase 2b ended.
enum Durability {
    Acked,
    Lost,
}

/// What phase 2a decided under the writer byte, from the scratch layer ([F16] P-30–P-36).
enum Planned {
    /// An idempotency hit ([F16] P-32): the stored result and the lsn of the record that stores it.
    Hit(u64, u64),
    /// The groups to append.
    Append(Box<Plan>),
}

/// The groups an append writes and what it allocated.
struct Plan {
    /// The groups, in order: the parks of P-70 first, then the operation's own.
    groups: Vec<Vec<Rec>>,
    /// How many of `groups` are parks.
    n_park: usize,
    /// The counters and HLC maxima before the groups (an extent head carries them, [F16] P-97).
    before: (Counters, u64, u64),
    /// The operation's result.
    result: u64,
    /// P-69's and P-81's seeded bugs: a group written after the acknowledgement.
    later: Option<Op>,
}

/// The facts the scratch-layer decision reads besides the scratch state.
struct PlanCtx<'a> {
    /// The newest slot under the writer byte.
    slot: &'a Slot,
    /// The groups beyond its `committed_lsn` (pending).
    pending: &'a [Group],
    /// The process's replay bound L0.
    l0: u64,
    /// Some group lies beyond L0 (published or pending).
    beyond_l0: bool,
    /// The stamp and the wall clock under the writer byte.
    now: Stamp,
    wall_ms: i64,
    /// E and `store.commit.inline-max-bytes`.
    e: u64,
    inline_max: u32,
    bugs: Bugs,
}

/// What a publish changes besides the fold ([F16] P-48, P-63, P-66, [F04 §9]).
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Change {
    /// Boot-change recovery's new `boot_id`.
    pub boot: Option<[u8; 16]>,
    /// A `config_gen` bump.
    pub bump: bool,
    /// `flags` to set (mask, value).
    pub flags: Option<(u16, u16)>,
    /// The publish follows a log flush of this holding.
    pub flushed: bool,
    /// P-38's seeded bug: `committed_lsn` is set to this end whatever the pending groups before it.
    pub force_committed: Option<u64>,
    /// P-49's decrease: the end of the valid log an appender's scan found below the published `committed_lsn`.
    pub lower_committed: Option<u64>,
}

/// The bytes a writer holds during one step.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Held {
    pub(crate) writer: bool,
    pub(crate) flush: bool,
}

/// `WriterDiag.activity` ([F03 §6.1]).
pub(crate) mod activity {
    /// Phase 2a: scan, re-validate and append.
    pub const APPEND: u8 = 1;
    /// The flush holder scans and re-writes the pending range.
    pub const FLUSH_SCAN: u8 = 2;
    /// The flush holder publishes after its flush.
    pub const FLUSH_PUBLISH: u8 = 3;
    /// A maintenance publish or the barrier's no-op publish.
    pub const MAINTENANCE: u8 = 4;
    /// Boot-change recovery.
    pub const BOOT_RECOVERY: u8 = 5;
    /// A publish that changes only fields kept in `HEAD`.
    pub const HEAD_UPDATE: u8 = 7;
}

impl<V: Vfs, T: Tap> Toy<V, T> {
    // ---- locks ----

    fn lock_err(e: moirai_vfs::LockError) -> ToyError {
        ToyError::Lock(e.to_string())
    }

    /// The bounded writer wait ([F16] P-27) and `WriterDiag` (WD-1) with the holding's `activity`. Returns whether the
    /// byte is held: P-27's seeded bug goes on without it after a timeout.
    pub(crate) fn take_writer(&mut self, activity: u8) -> Result<bool, ToyError> {
        if self.locks.holds(LockByte::Writer) {
            return self.nest(LockByte::Writer).map(|()| true);
        }
        let ok = self
            .locks
            .wait(
                &self.vfs,
                &self.root,
                LockByte::Writer,
                self.cfg.writer_wait_ms,
            )
            .map_err(Self::lock_err)?;
        if ok {
            self.writer_diag(activity);
            return Ok(true);
        }
        if self.bugs().on(Bug::P27AppendAfterWriterTimeout) {
            return Ok(false);
        }
        Err(ToyError::StoreLocked)
    }

    /// Measurement 2's injected in-lock cost ([`crate::Config::in_lock_cost_ns`]): once per append, right before its
    /// first `write_at`. The wait polls `file_size` of `HEAD`, a scheduling point of the in-memory `Vfs`, whose monotonic
    /// clock advances only at scheduling points; on the real `Vfs` each poll is one cheap system call.
    fn in_lock_cost(&self) {
        let cost = self.cfg.in_lock_cost_ns;
        if cost == 0 {
            return;
        }
        let until = self.vfs.mono_ns().saturating_add(cost);
        while self.vfs.mono_ns() < until {
            let _ = self.vfs.file_size(&self.head);
        }
    }

    /// Writes what P-69's, P-81's and P-63's seeded bugs left for later: the ref moves and pins (errors are dropped:
    /// nothing waits for them), and the `HEAD` flush that `quiet` reported success before.
    pub(crate) fn run_later(&mut self) {
        if core::mem::take(&mut self.head_flush_due) {
            self.flush_head();
        }
        for op in core::mem::take(&mut self.later) {
            let _ = self.run(&op);
        }
    }

    /// A second acquisition of `byte`, which this handle holds already. Only P-51's seeded bug makes one — maintenance
    /// run inside the flush holder's holding of the writer and flush bytes — and it is counted as nested in that
    /// holding. With the switch off it is a refusal of the lock layer, as the grant table refuses a client's second
    /// acquisition of a byte it holds ([OS/lock] contract item 3), so no path that re-acquires a held byte goes unseen.
    fn nest(&mut self, byte: LockByte) -> Result<(), ToyError> {
        if !self.bugs().on(Bug::P51MaintenanceUnderWriter) {
            return Err(ToyError::Lock(format!(
                "the handle acquires {byte:?}, which it holds already ([OS/lock] contract item 3)"
            )));
        }
        match byte {
            LockByte::Writer => self.nested.0 += 1,
            _ => self.nested.1 += 1,
        }
        Ok(())
    }

    /// The bounded flush wait ([F16] P-41).
    pub(crate) fn take_flush(&mut self) -> Result<bool, ToyError> {
        if self.locks.holds(LockByte::Flush) {
            return self.nest(LockByte::Flush).map(|()| true);
        }
        self.locks
            .wait(
                &self.vfs,
                &self.root,
                LockByte::Flush,
                self.cfg.flush_wait_ms,
            )
            .map_err(Self::lock_err)
    }

    pub(crate) fn drop_writer(&mut self) {
        if self.nested.0 > 0 {
            self.nested.0 -= 1;
            return;
        }
        self.locks.release(&self.vfs, LockByte::Writer);
    }

    pub(crate) fn drop_flush(&mut self) {
        if self.nested.1 > 0 {
            self.nested.1 -= 1;
            return;
        }
        self.locks.release(&self.vfs, LockByte::Flush);
    }

    pub(crate) fn let_go(&mut self, h: &mut Held) {
        if h.writer {
            self.drop_writer();
            h.writer = false;
        }
        if h.flush {
            self.drop_flush();
            h.flush = false;
        }
    }

    /// `WriterDiag` ([F03 §6]): one lazy write of 512 bytes right after the grant, with the client's acquisition counter
    /// and the holding's activity; a failure is not an error (WD-2).
    fn writer_diag(&mut self, activity: u8) {
        self.diag_seq += 1;
        let id = self.vfs.self_id();
        let mut w = Writer::with_capacity(512);
        w.u64(self.diag_seq).bytes(&id.to_bytes()).zeros(16);
        let cmd = b"toylog";
        w.u16(cmd.len() as u16).bytes(cmd).zeros(398 - cmd.len());
        w.u64((self.vfs.wall_ms().max(0) as u64) << 16)
            .u8(activity)
            .zeros(39);
        let sum = crate::codec::hash64(&w.buf);
        w.u64(sum);
        let data = self.locks.data(&self.vfs);
        let _ = self.vfs.write_at(data, 2048, &w.buf);
    }

    /// The stamp of now ([OS/clock §3]).
    pub(crate) fn stamp(&self) -> Stamp {
        Stamp::now(&self.vfs, &self.boot)
    }

    // ---- the operation ----

    /// Runs one write operation through phases 1, 2a and 2b ([F16 §5]) and returns at its acknowledgement: the caller
    /// reports the result, then runs phase 3 ([`Toy::maintain`], [F16] P-51). A lost group is re-run with the same
    /// operation at most twice ([F16] P-47), then the operation ends `outcome_unknown`.
    pub fn run(&mut self, op: &Op) -> Result<Done, ToyError> {
        self.run_later();
        self.ensure_named();
        let mut reruns = 0;
        let mut lost = 0;
        loop {
            let (d1, hit) = self.phase1(op)?;
            if let Some(result) = hit {
                return Ok(Done {
                    replayed: true,
                    result,
                    ..Done::default()
                });
            }
            match self.phase2a(op, &d1)? {
                Step::Rerun => {
                    reruns += 1;
                    if reruns > 2 {
                        return Err(ToyError::Refused("conflict: phase 1 re-ran twice"));
                    }
                }
                Step::Hit(result, None) => {
                    return Ok(Done {
                        replayed: true,
                        result,
                        ..Done::default()
                    });
                }
                Step::Hit(result, Some((end, chain, e))) => {
                    // P-33: a hit on a pending group waits for that group's identity; its seeded bug returns at once.
                    if self.bugs().on(Bug::P33G6ReplayBeforeDurable) {
                        return Ok(Done {
                            replayed: true,
                            result,
                            ..Done::default()
                        });
                    }
                    let a = Appended {
                        start: 0,
                        end,
                        chain_in: 0,
                        chain,
                        e,
                        durable: true,
                        published: None,
                        holding_flush: false,
                        result,
                        written: Vec::new(),
                        later: None,
                    };
                    match self.phase2b(&a)? {
                        Durability::Acked => {
                            return Ok(Done {
                                replayed: true,
                                result,
                                ..Done::default()
                            });
                        }
                        Durability::Lost => lost += 1,
                    }
                }
                Step::Appended(a) => {
                    // The seeded bugs that act between the append and phase 2b: P-16's early rename, P-62's early
                    // barrier.
                    if matches!(op, Op::Intent(_)) && self.pending_ns.is_some() {
                        self.early_namespace();
                    }
                    if matches!(op, Op::Checkpoint(_))
                        && self.bugs().on(Bug::P62G12BarrierBeforeCheckpoint)
                    {
                        self.early_barrier();
                    }
                    let outcome = if let Some(covered) = a.published {
                        if covered && self.identity(&a) {
                            Durability::Acked
                        } else {
                            Durability::Lost
                        }
                    } else if !a.durable
                        && self.bugs().on(Bug::P39G13LazyStranded)
                        && !a.holding_flush
                    {
                        // P-39's seeded bug: the lazy group behind a pending durable group is reported published.
                        Durability::Acked
                    } else {
                        self.phase2b(&a)?
                    };
                    match outcome {
                        Durability::Acked => {
                            let done = Done {
                                replayed: false,
                                // An intent's result is its id: the lsn of its record ([F05 §9.15]).
                                result: if matches!(op, Op::Intent(_)) {
                                    a.start
                                } else {
                                    a.result
                                },
                                start: a.start,
                                end: a.end,
                                chain_in: a.chain_in,
                                chain_out: a.chain,
                            };
                            if let Some(later) = &a.later {
                                // P-69's and P-81's seeded bugs: the ref move or the Pin follows in a later group,
                                // after the acknowledgement.
                                self.later.push(later.clone());
                            }
                            return Ok(done);
                        }
                        Durability::Lost => {
                            if self.bugs().on(Bug::P47LgReappendOldBytes)
                                && !a.written.is_empty()
                                && let Some(done) = self.reappend(&a)
                            {
                                return Ok(done);
                            }
                            lost += 1;
                        }
                    }
                }
            }
            if lost > 2 {
                return Err(ToyError::OutcomeUnknown);
            }
        }
    }

    /// P-47's seeded bug: the lost group's old bytes go back to their old position, and phase 2b runs for them again.
    fn reappend(&mut self, a: &Appended) -> Option<Done> {
        let (s, _) = self.read_head().ok()?;
        let held = self.take_writer(activity::APPEND).ok()?;
        let ctx = ScanCtx::of(&s);
        for (lsn, b) in &a.written {
            let _ = self.write_log(&ctx, *lsn, b);
        }
        if held {
            self.drop_writer();
        }
        let again = Appended {
            holding_flush: false,
            ..a.clone()
        };
        match self.phase2b(&again) {
            Ok(Durability::Acked) => Some(Done {
                replayed: false,
                result: a.result,
                start: a.start,
                end: a.end,
                chain_in: a.chain_in,
                chain_out: a.chain,
            }),
            _ => None,
        }
    }

    /// Phase 3 ([F16] P-51): after the caller reported the acknowledgement of [`Toy::run`], and after every byte is
    /// released, the maintenance triggers of [F17 §5] — here the checkpoint trigger [`crate::Config::auto_checkpoint_bytes`].
    /// A write of the maintenance holder itself runs none: it holds the maintenance byte already (P-76), and a client
    /// never acquires a byte it holds ([OS/lock] contract item 3).
    pub fn maintain(&mut self) {
        if self.cfg.auto_checkpoint_bytes == 0 || self.locks.holds(LockByte::Maintenance) {
            return;
        }
        self.step(Note::Phase3, true);
        if let Ok((s, _)) = self.read_head()
            && s.committed_lsn.saturating_sub(s.checkpoint_lsn) >= self.cfg.auto_checkpoint_bytes
        {
            let _ = self.checkpoint();
        }
        self.step(Note::Phase3, false);
    }

    /// Phase 1 ([F16] P-25): the view up to the published `committed_lsn`, the idempotency pre-check at L0, and the
    /// candidate's decision; no lock byte is held.
    fn phase1(&mut self, op: &Op) -> Result<(Decision, Option<u64>), ToyError> {
        // P-25's seeded bug: phase 1 runs under the writer byte.
        let held = if self.bugs().on(Bug::P25PhaseOneUnderWriter) {
            self.take_writer(activity::APPEND)?
        } else {
            false
        };
        self.step(Note::Phase1, true);
        let r = (|| {
            self.refresh()?;
            let now = self.stamp();
            let bugs = self.bugs();
            let v = self
                .view
                .as_ref()
                .ok_or_else(|| ToyError::Corrupt("no view".to_owned()))?;
            if let Some((result, _)) = op.lookup(&v.state) {
                return Ok((Decision::Plain, Some(result)));
            }
            let d = decide(op, &v.state, now, bugs).map_err(ToyError::Refused)?;
            // W3 on the phase-1 encoding ([F17 §4.4], [F16] P-25).
            if let (Op::Runtime(r), Decision::Runtime { pad }) = (op, &d) {
                let e = self.cfg.extent_bytes;
                let len = crate::ops::runtime_rec(r, &v.state, *pad, bugs).len(true);
                if len > e - ROTATION_RESERVE {
                    return Err(ToyError::Refused("commit_too_large"));
                }
            }
            Ok((d, None))
        })();
        self.step(Note::Phase1, false);
        if held {
            self.drop_writer();
        }
        r
    }

    /// Phase 2a ([F16] P-27–P-39): under the writer byte.
    fn phase2a(&mut self, op: &Op, d1: &Decision) -> Result<Step, ToyError> {
        let mut held = Held {
            writer: self.take_writer(activity::APPEND)?,
            flush: false,
        };
        // The extent a rotation made ready under the flush byte of this holding (P-72 step 2).
        let mut ready: Option<u32> = None;
        loop {
            let r = self.append_once(op, d1, &mut held, &mut ready);
            match r {
                Ok(Some(step)) => {
                    // Release the writer byte (P-1, P-2); a rotation keeps the flush byte for phase 2b (P-72 step 4).
                    if held.writer {
                        self.drop_writer();
                        held.writer = false;
                    }
                    if let Step::Appended(mut a) = step {
                        a.holding_flush = held.flush;
                        return Ok(Step::Appended(a));
                    }
                    if held.flush {
                        self.drop_flush();
                    }
                    return Ok(step);
                }
                Ok(None) => {
                    // A restart at P-28 (a rotation made its extent ready, or boot-change recovery ran). An extent is
                    // ready only while the flush byte of the holding that prepared it is still held.
                    if !held.flush {
                        ready = None;
                    }
                }
                Err(e) => {
                    self.let_go(&mut held);
                    return Err(e);
                }
            }
        }
    }

    /// One pass of phase 2a from P-28; `None` restarts at P-28 with the bytes in `held`.
    fn append_once(
        &mut self,
        op: &Op,
        d1: &Decision,
        held: &mut Held,
        ready: &mut Option<u32>,
    ) -> Result<Option<Step>, ToyError> {
        let bugs = self.bugs();
        let (s, which) = self.read_head()?;
        // P-28 item 1: a boot change sends the appender to boot-change recovery, which takes the flush byte itself; the
        // bytes of this holding are released first, so an extent made ready under it is ready no more (P-72).
        if let moirai_vfs::BootIdentity::Known(b) = self.boot
            && b.0 != s.boot_id
            && !bugs.on(Bug::P60T8NoBootCheck)
        {
            self.let_go(held);
            *ready = None;
            // The handle is booted only once the recovery succeeded (see `head_for_read`).
            self.boot_recover()?;
            self.booted = true;
            held.writer = self.take_writer(activity::APPEND)?;
            return Ok(None);
        }
        // P-28 items 2 and 3.
        if s.flags & FLAG_RETIRED != 0 {
            return Err(ToyError::Retired);
        }
        if s.flags & FLAG_READONLY != 0 {
            return Err(ToyError::Refused("readonly_flag"));
        }
        let ctx = ScanCtx::of(&s);
        // P-56 before the scan from L0; a stale view re-runs phase 1.
        let Some(v) = self.view.as_ref() else {
            return Ok(Some(Step::Rerun));
        };
        let (l0, chain0, seen, base) = (v.l0, v.chain, v.slot_seq, v.base);
        if seen != s.slot_seq && !bugs.on(Bug::P56OverlayKeptAfterRefill) {
            let ok = s.committed_lsn >= l0
                && l0 >= s.checkpoint_lsn.min(base)
                && self
                    .chain_at(&ctx, l0, s.durable_lsn)
                    .is_ok_and(|c| c == chain0);
            if !ok {
                self.view = None;
                return Ok(Some(Step::Rerun));
            }
        }
        // P-29: the scan goes to the end of the valid log; its seeded bug stops at the published committed_lsn.
        let limit = bugs
            .on(Bug::P29AppendAtCommitted)
            .then_some(s.committed_lsn.max(l0));
        let sc = match self.scan(&ctx, l0, chain0, limit) {
            Ok(sc) => sc,
            Err(ToyError::ExtentMissing { .. }) => {
                // An extent the view needed was retired: phase 1 rebuilds the view from the newest set (P-59).
                self.view = None;
                return Ok(Some(Step::Rerun));
            }
            Err(e) => return Err(e),
        };
        let mut e_v = sc.end;
        let mut chain_v = sc.chain;
        match sc.stop {
            Stop::Limit => {}
            Stop::NoExtent(p) => {
                // The end of the log only at or above durable_lsn: a missing next extent below it is corruption
                // ([F05 §5.3], [F16] P-29), as for readers.
                if p < s.durable_lsn {
                    return Err(ToyError::Corrupt(format!(
                        "log.{} is missing at {p}, below durable_lsn {}; run moirai repair",
                        ctx.extent(p),
                        s.durable_lsn
                    )));
                }
            }
            Stop::Invalid(_, p) => {
                if p < s.durable_lsn {
                    return Err(ToyError::Corrupt(format!(
                        "an invalid group at {p} below durable_lsn {}; run moirai repair",
                        s.durable_lsn
                    )));
                }
            }
            Stop::ReadError(p) => {
                // P-92: a failed read stops the writer; its seeded bug treats one at or above durable_lsn as the end.
                if p < s.durable_lsn || !bugs.on(Bug::P92ReadErrorAsEnd) {
                    return Err(if p < s.durable_lsn {
                        ToyError::Corrupt(format!("unreadable log at {p} below durable_lsn"))
                    } else {
                        ToyError::IoFault(p)
                    });
                }
            }
        }
        if bugs.on(Bug::P29AppendAtCommitted) && e_v >= s.committed_lsn {
            e_v = s.committed_lsn.max(l0);
            chain_v = if e_v == sc.end {
                sc.chain
            } else {
                self.chain_at(&ctx, e_v, s.durable_lsn)?
            };
        }
        // P-49 before the append: a valid log that ends below the published committed_lsn has lost a lazy tail (a crash
        // or a failed flush, [F15] FM-3.6). The loss is published first — committed_lsn lowered to the end the scan found
        // — so that the group this process appends, and every group after it, lies beyond committed_lsn for every other
        // process: pending for appenders (P-30, P-31, P-36) and invisible to readers (P-57, I-G2) until a flush covers
        // it. Then phase 2a restarts at P-28: [F16] P-29 publishes `committed_lsn` = p under the same holding before any
        // append (spec sync 2b S2B-P-31).
        if e_v < s.committed_lsn && !bugs.on(Bug::P49CommittedAboveValidEnd) {
            let change = Change {
                lower_committed: Some(e_v),
                ..Change::default()
            };
            self.publish(&s, which, s.durable_lsn, change)?;
            return Ok(None);
        }
        // Published groups go into the view; pending ones into the scratch layer: the view with the pending groups
        // applied through an undo log and taken back after the decision (P-30; its seeded bug keeps them in the view).
        let split = sc.groups.partition_point(|g| g.end <= s.committed_lsn);
        let (published, pending) = sc.groups.split_at(split);
        let Some(mut view) = self.view.take() else {
            return Ok(Some(Step::Rerun));
        };
        for g in published {
            view.state.apply(g, bugs, false)?;
            view.l0 = g.end;
            view.chain = g.chain_out;
        }
        view.slot_seq = s.slot_seq;
        let mut undo = Undo::default();
        let mut layered = Ok(());
        for g in pending {
            if let Err(m) = view.state.apply_logged(g, bugs, true, &mut undo) {
                layered = Err(ToyError::from(m));
                break;
            }
        }
        let pctx = PlanCtx {
            slot: &s,
            pending,
            l0,
            beyond_l0: !sc.groups.is_empty(),
            now: self.stamp(),
            wall_ms: self.vfs.wall_ms(),
            e: ctx.e,
            inline_max: self.cfg.inline_max_bytes,
            bugs,
        };
        let planned = layered.and_then(|()| plan(op, d1, &view.state, &pctx));
        if bugs.on(Bug::P30G10PendingIntoOverlay) && undo_kept(&planned) {
            if let Some(g) = pending.last() {
                view.l0 = g.end;
                view.chain = g.chain_out;
            }
        } else {
            view.state.undo(undo);
        }
        self.view = Some(view);
        let plan = match planned? {
            Planned::Hit(result, lsn) => {
                let in_pending = pending.iter().find(|g| g.start <= lsn && lsn < g.end);
                return Ok(Some(Step::Hit(
                    result,
                    in_pending.map(|g| (g.end, g.chain_out, ctx.e)),
                )));
            }
            Planned::Append(p) => *p,
        };
        // Placement (P-37, G-3) and rotation (P-72).
        let groups = &plan.groups;
        let total: u64 = groups.iter().map(|g| group_len(g)).sum();
        let n = ctx.extent(e_v);
        let r = ctx.start_of(n) + ctx.e - e_v;
        let at_start = ctx.offset(e_v) == 0;
        if !at_start && r < MIN_GROUP {
            // G-4 is broken (only a defective writer leaves 1–39 bytes): no group can be placed or padded here.
            return Err(ToyError::Corrupt(format!(
                "only {r} bytes remain in log.{n} at {e_v}; no group fits and no pad is valid (G-4)"
            )));
        }
        let fits = !at_start && (total == r || total + MIN_GROUP <= r);
        let mut place = e_v;
        let mut seed = chain_v;
        let mut head_at: Option<u32> = None;
        let mut pad: Option<u64> = None;
        if !fits && !bugs.on(Bug::P09GroupAcrossExtents) {
            let m = if at_start { n } else { n + 1 };
            if *ready != Some(m) {
                if bugs.on(Bug::P72RotateUnderWriterOnly) {
                    // P-72's seeded bug: the extent is prepared under the writer byte, without the flush byte.
                    self.prepare_extent(&ctx, m)?;
                    *ready = Some(m);
                } else {
                    // P-72 steps 1–3: release the writer byte, take the flush byte, make log.<m> ready, restart.
                    // P-1's seeded bug waits for the flush byte while it still holds the writer byte.
                    if !bugs.on(Bug::P01G4WaitFlushUnderWriter) && held.writer {
                        self.drop_writer();
                        held.writer = false;
                    }
                    if !held.flush {
                        if !self.take_flush()? {
                            return Err(ToyError::StoreLocked);
                        }
                        held.flush = true;
                    }
                    self.prepare_extent(&ctx, m)?;
                    *ready = Some(m);
                    if !held.writer {
                        held.writer = self.take_writer(activity::APPEND)?;
                    }
                    return Ok(None);
                }
            }
            if !at_start {
                pad = Some(r);
            }
            head_at = Some(m);
            place = ctx.start_of(m);
        }
        // Encode and write: the pad, the extent head, then the operation's groups (P-37: seeded with the chain value at
        // E_v; its seeded bug seeds the first group with the chain value at the published committed_lsn).
        let mut written: Vec<(u64, Vec<u8>)> = Vec::new();
        if bugs.on(Bug::P37ChainSeedAtCommitted)
            && s.committed_lsn != e_v
            && s.committed_lsn >= s.epoch_lsn
        {
            seed = self.chain_at(&ctx, s.committed_lsn, s.durable_lsn)?;
        }
        if let Some(r) = pad {
            let noop = Rec::new(kind::NOOP, vec![0; (r - MIN_GROUP) as usize], bugs);
            let mut b = Vec::with_capacity(r as usize);
            seed = encode_group(&[noop], e_v, s.epoch, seed, &mut b);
            written.push((e_v, b));
        }
        let mut head_bytes = Vec::new();
        // P-97: the rotation begins the new extent with its extent head ([F05 §4.5]); its seeded bug writes none, and the
        // operation's groups start at the extent's first byte.
        if let Some(m) = head_at
            && !bugs.on(Bug::P97NoExtentHead)
        {
            let (before, hlc_seq, hlc_commit) = plan.before;
            let h = ExtentHeadRec {
                epoch_lsn: s.epoch_lsn,
                chain_in: seed,
                init: s.init,
                project_oid_algo: s.project_oid_algo,
                hflags: u8::from(s.flags & FLAG_QUIET != 0)
                    | (u8::from(s.flags & FLAG_READONLY != 0) << 1),
                counters: Counters {
                    hlc_seq,
                    hlc_commit,
                    ..before
                },
            };
            let rec = Rec::new(kind::EXTENT_HEAD, h.encode(), bugs);
            seed = encode_group(&[rec], ctx.start_of(m), s.epoch, seed, &mut head_bytes);
            place = ctx.start_of(m) + HEAD_GROUP;
        }
        let own_start = place;
        let chain_in = seed;
        let mut own = Vec::with_capacity(total as usize);
        let mut at = own_start;
        // P-52's seeded bug appends a commit's marker as a group of its own, in a write of its own after the commit's.
        let split_at = bugs
            .on(Bug::P52T10MarkerInOwnGroup)
            .then(|| groups.len().checked_sub(1))
            .flatten()
            .filter(|&k| k > plan.n_park && groups[k].iter().all(|r| r.kind == kind::MARKER));
        let mut split_bytes: Option<(u64, Vec<u8>)> = None;
        for (k, g) in groups.iter().enumerate() {
            if Some(k) == split_at {
                let mut b = Vec::new();
                seed = encode_group(g, at, s.epoch, seed, &mut b);
                at += b.len() as u64;
                split_bytes = Some((at - b.len() as u64, b));
                continue;
            }
            let before_len = own.len();
            seed = encode_group(g, at, s.epoch, seed, &mut own);
            at += (own.len() - before_len) as u64;
        }
        let end = at;
        if let Some(m) = head_at {
            let mut run = head_bytes;
            run.extend_from_slice(&own);
            written.push((ctx.start_of(m), run));
        } else {
            written.push((own_start, own));
        }
        written.extend(split_bytes);
        self.in_lock_cost();
        for (lsn, b) in &written {
            self.write_log(&ctx, *lsn, b)?;
        }
        // Only P-47's seeded bug writes the bytes again after a loss: otherwise they are dropped here, so phase 2b does
        // not keep up to P05 of group bytes alive.
        if !bugs.on(Bug::P47LgReappendOldBytes) {
            written = Vec::new();
        }
        // P-38: a lazy group is published at once only when no durable group is pending. It is covered only when the
        // publish's own scan reached its end (P-40, P-46): a read of a predecessor that a failed flush poisoned may end
        // the valid log before it ([F15] FM-3.2); then the group was lost (P-47).
        let own_durable = groups.iter().any(|g| group_durable(g));
        let mut published = None;
        if !own_durable {
            let pending_durable = pending.iter().any(Group::durable) || head_at.is_some();
            if !pending_durable || bugs.on(Bug::P38G2LazyPublishPastDurable) {
                let change = Change {
                    force_committed: pending_durable.then_some(end),
                    ..Change::default()
                };
                let next = self.publish(&s, which, s.durable_lsn, change)?;
                published = Some(next.committed_lsn >= end);
            }
        }
        Ok(Some(Step::Appended(Appended {
            start: own_start,
            end,
            chain_in,
            chain: seed,
            e: ctx.e,
            durable: own_durable,
            published,
            holding_flush: false,
            result: plan.result,
            written,
            later: plan.later,
        })))
    }

    /// Writes `bytes` at lsn `lsn` in one `write_at` on its extent (class `lazy`).
    pub(crate) fn write_log(
        &mut self,
        ctx: &ScanCtx,
        lsn: u64,
        bytes: &[u8],
    ) -> Result<(), ToyError> {
        let n = ctx.extent(lsn);
        let off = ctx.offset(lsn);
        if !self.open_extent(n)? {
            return Err(ToyError::Corrupt(format!(
                "log.{n} is missing at an append"
            )));
        }
        let r = match self.ext(n) {
            Some(f) => self.vfs.write_at(f, off, bytes),
            None => {
                return Err(ToyError::Corrupt(format!(
                    "log.{n} is missing at an append"
                )));
            }
        };
        self.io(r)
    }

    /// Makes log.<m> ready ([F16] P-72 step 2, P-8, P-96): a full-length file is taken as it is, an absent one is
    /// created, a shorter one is re-prepared in place; then, in every case, `durable+meta` on it and `durable-name` on the
    /// store directory. P-8's seeded bug skips both flushes; P-96's skips them for a spare; P-90's treats a `DiskFull`
    /// of the preparation as success (the extent then does not exist, and the append into it fails).
    ///
    /// The rotator holds the flush byte and a spare's preparer the maintenance byte, so a spare's rename onto log.<m>
    /// (P-96) can land between the check and `create_extent`: `AlreadyExists` then means another process completed the
    /// file first, and the check runs again on the file now there, which is never a refusal for it. [F16] P-72 step 2
    /// does not say so yet; the toy follows the reading proposed for it. A second `AlreadyExists` is an I/O error like
    /// any other.
    pub(crate) fn prepare_extent(&mut self, ctx: &ScanCtx, m: u32) -> Result<(), ToyError> {
        let bugs = self.bugs();
        let e = ctx.e;
        let vol: StoreVolume = self.vol;
        let mut raced = false;
        let spare = loop {
            let exists = self.open_extent(m)?;
            match self.ext(m).filter(|_| exists) {
                Some(f) => {
                    let size = self.vfs.file_size(f).map_err(ToyError::Io)?;
                    if size > e {
                        return Err(ToyError::Corrupt(format!("log.{m} is longer than E")));
                    }
                    if size < e {
                        let r = self.vfs.recycle_extent(f, e, &vol);
                        self.io(r)?;
                        break false;
                    }
                    break true;
                }
                None => {
                    let r = self
                        .vfs
                        .create_extent(&self.root, log_name(m).as_rel_path(), e, &vol);
                    match r {
                        Ok(f) => {
                            self.extents.insert(m, f);
                            break false;
                        }
                        Err(err) if err.kind == VfsErrorKind::AlreadyExists && !raced => {
                            self.forget_extent(m);
                            raced = true;
                        }
                        Err(err) => {
                            self.forget_extent(m);
                            // P-90's seeded bug: the DiskFull is treated as success.
                            self.io::<()>(Err(err))?;
                            return Ok(());
                        }
                    }
                }
            }
        };
        if bugs.on(Bug::P08AppendBeforeExtentDurable)
            || (spare && bugs.on(Bug::P96SpareWithoutFlushes))
        {
            return Ok(());
        }
        if let Some(f) = self.extents.get(&m) {
            self.sync_file(f, SyncKind::DataAndMeta);
        }
        // A full-length file may be a spare that its preparer renamed from tmp/ and died before it made the rename
        // durable: the rename has two parents, and it is durable only once both are synced ([F15] FM-2.4), so tmp/ is
        // synced too when log.<m> existed at full length ([F16] P-8, P-72 step 2, P-96; spec sync 2b S2B-P-33).
        if spare {
            self.sync_store_dir(Some(moirai_vfs::RelPath::literal("tmp")));
        }
        self.sync_store_dir(None);
        Ok(())
    }

    /// Phase 2b ([F16] P-40–P-47).
    fn phase2b(&mut self, a: &Appended) -> Result<Durability, ToyError> {
        let bugs = self.bugs();
        let (s, _) = self.read_head()?;
        if self.covered(&s, a) {
            if a.holding_flush {
                self.drop_flush();
            }
            return Ok(self.ack_or_lost(a));
        }
        let mut held = Held {
            writer: false,
            flush: a.holding_flush,
        };
        if !held.flush {
            if !self.take_flush()? {
                // P-41: a flush-byte timeout is outcome_pending; its seeded bug acknowledges it.
                if bugs.on(Bug::P41FlushTimeoutAcked) {
                    return Ok(Durability::Acked);
                }
                return Err(ToyError::OutcomePending);
            }
            held.flush = true;
        }
        let r = self.flush_pass(a, &mut held);
        match r {
            Ok(Some(outcome)) => {
                self.let_go(&mut held);
                Ok(outcome)
            }
            Ok(None) => {
                self.let_go(&mut held);
                Ok(self.ack_or_lost(a))
            }
            Err(e) => {
                self.let_go(&mut held);
                Err(e)
            }
        }
    }

    /// The covered test ([F16] P-40): a durable group by `durable_lsn`, a lazy one by `committed_lsn`.
    ///
    /// P-40's seeded bug lets `committed_lsn` stand in for `durable_lsn`. The published `committed_lsn` never passes a
    /// pending durable group (P-38, P-49), so taken literally the switch changes nothing; its toy form takes the
    /// committed position of the group's own append instead — the end of the valid log it extended, which P-49 would
    /// publish and which is at least E_g by construction — so the group goes to the identity check right after its
    /// append, without a covering flush (source bug G1 of [80 §2.4.4]: "acknowledge before a covering flush").
    fn covered(&self, s: &Slot, a: &Appended) -> bool {
        if !a.durable {
            return s.committed_lsn >= a.end;
        }
        if self.bugs().on(Bug::P40G1CoveredByCommitted) {
            return true;
        }
        s.durable_lsn >= a.end
    }

    /// The identity check ([F16] P-46): the 8 bytes at [E_g − 8, E_g) equal the remembered chain value; a failed read is a
    /// mismatch. P-46's seeded bug acknowledges by position.
    fn identity(&mut self, a: &Appended) -> bool {
        if self.bugs().on(Bug::P46G7AckByPosition) {
            return true;
        }
        let q = a.end - 8;
        let n = (q / a.e + 1) as u32;
        if !matches!(self.open_extent(n), Ok(true)) {
            return false;
        }
        let Some(f) = self.ext(n) else {
            return false;
        };
        let mut b = [0u8; 8];
        self.vfs.read_exact_at(f, q % a.e, &mut b).is_ok() && u64::from_le_bytes(b) == a.chain
    }

    fn ack_or_lost(&mut self, a: &Appended) -> Durability {
        if self.identity(a) {
            Durability::Acked
        } else {
            Durability::Lost
        }
    }

    /// The flush holder's pass, holding the flush byte: writer byte, scan and re-write, release, flush, writer byte,
    /// publish ([F16] P-42–P-45, P-48). `None`: the group is covered and goes to the identity check.
    fn flush_pass(
        &mut self,
        a: &Appended,
        held: &mut Held,
    ) -> Result<Option<Durability>, ToyError> {
        let bugs = self.bugs();
        // P-43: the scan and the re-write are under the writer byte; its seeded bug does both outside it.
        if !bugs.on(Bug::P43G8RewriteOutsideWriter) {
            held.writer = self.take_writer(activity::FLUSH_SCAN)?;
        }
        let (s, _) = self.read_head()?;
        if s.flags & FLAG_RETIRED != 0 {
            return Err(ToyError::OutcomeUnknown);
        }
        if self.covered(&s, a) {
            return Ok(None);
        }
        let ctx = ScanCtx::of(&s);
        let chain_d = self.chain_at(&ctx, s.durable_lsn, s.durable_lsn)?;
        let sc = self.scan_opt(&ctx, s.durable_lsn, chain_d, None, true)?;
        match sc.stop {
            Stop::ReadError(p) => {
                return Err(if p < s.durable_lsn {
                    ToyError::Corrupt(format!("unreadable log at {p} below durable_lsn"))
                } else {
                    ToyError::IoFault(p)
                });
            }
            Stop::Invalid(_, p) | Stop::NoExtent(p) if p < s.durable_lsn => {
                return Err(ToyError::Corrupt(format!(
                    "the log is invalid at {p} below durable_lsn; run moirai repair"
                )));
            }
            _ => {}
        }
        let e_end = sc.end;
        let own = a.end <= e_end
            && sc
                .groups
                .iter()
                .any(|g| g.end == a.end && g.chain_out == a.chain);
        if !own {
            return Ok(Some(Durability::Lost));
        }
        // P-42: re-write every byte of (durable_lsn, E] from the scan buffer; its seeded bug flushes without it.
        if !bugs.on(Bug::P42G3T5FlushWithoutRewrite) {
            for g in &sc.groups {
                self.write_log(&ctx, g.start, &g.raw)?;
            }
        }
        // P-44: one flush, outside the writer byte; P-2's seeded bug flushes under it.
        if held.writer && !bugs.on(Bug::P02FlushUnderWriter) {
            self.drop_writer();
            held.writer = false;
        }
        self.flush_range(&ctx, s.durable_lsn, e_end)?;
        // P-51's seeded bug runs phase 3 under the writer byte (below, after the publish).
        if !held.writer {
            held.writer = self.take_writer(activity::FLUSH_PUBLISH)?;
        }
        // P-48: publish from the newest slot read now; its seeded bug publishes from the slot read before the flush.
        // P-45: durable_lsn never decreases, so the flushed end E is published as max(the slot's durable_lsn, E).
        let (s2, which2) = if bugs.on(Bug::P48G11StalePublish) {
            (s.clone(), self.slot_of(&s))
        } else {
            self.read_head()?
        };
        let d = s2.durable_lsn.max(e_end);
        self.publish(
            &s2,
            which2,
            d,
            Change {
                flushed: true,
                ..Change::default()
            },
        )?;
        if bugs.on(Bug::P51MaintenanceUnderWriter) {
            self.maintain();
        }
        Ok(None)
    }

    /// Which slot of `HEAD` holds `s` now (0 or 1), by `slot_seq`.
    fn slot_of(&self, s: &Slot) -> usize {
        let mut buf = [0u8; HEAD_LEN];
        let _ = self.read_slots(&mut buf);
        for (k, chunk) in buf.chunks(SLOT_LEN).enumerate() {
            if let SlotRead::Valid(x) = Slot::read(chunk)
                && x.slot_seq == s.slot_seq
            {
                return k;
            }
        }
        0
    }

    /// `durable` on every extent that holds a byte of (from, to] ([F16] P-6, P-44). An extent of the range that cannot
    /// be opened ends the operation before any publish: its bytes cannot be made durable, so `durable_lsn` must not pass
    /// them (exit 7 `store_corrupt`). P-6's seeded bug flushes only the extent that holds `to`.
    pub(crate) fn flush_range(
        &mut self,
        ctx: &ScanCtx,
        from: u64,
        to: u64,
    ) -> Result<(), ToyError> {
        if to <= from {
            return Ok(());
        }
        let first = ctx.extent(from);
        let last = ctx.extent(to - 1);
        let first = if self.bugs().on(Bug::P06FlushOnlyLastExtent) {
            last
        } else {
            first
        };
        let mut r = Ok(());
        for n in first..=last {
            match self.open_extent(n) {
                Ok(true) => {
                    if let Some(f) = self.extents.get(&n) {
                        self.sync_file(f, SyncKind::Data);
                    }
                }
                Ok(false) => {
                    r = Err(ToyError::Corrupt(format!(
                        "log.{n} is missing inside the flushed range ({from}, {to}]"
                    )));
                    break;
                }
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        r
    }

    // ---- the publish ----

    /// A publish ([F16] P-48–P-50): a read-modify-write of the newest valid slot `base` (in slot `which`) under the writer
    /// byte, folding every group from the base's `durable_lsn` to the new `committed_lsn` in log order (a group the base
    /// already folded changes nothing; P-50, [F04 §9.1] step 2), with `committed_lsn` by P-49 against the flushed end `d`,
    /// written into the other slot. Returns the written slot. A slot that would fail [F04 §7] checks 4 or 5 (a fatal
    /// slot, for example `committed_lsn` below `durable_lsn` when the publisher's own scan ends before its flushed end)
    /// is never written: the publish ends with `store_corrupt` and writes nothing.
    pub(crate) fn publish(
        &mut self,
        base: &Slot,
        which: usize,
        d: u64,
        change: Change,
    ) -> Result<Slot, ToyError> {
        let bugs = self.bugs();
        let ctx = ScanCtx::of(base);
        let chain = self.chain_at(&ctx, base.durable_lsn, base.durable_lsn)?;
        let sc = self.scan(&ctx, base.durable_lsn, chain, None)?;
        if let Stop::ReadError(p) = sc.stop {
            return Err(ToyError::IoFault(p));
        }
        let committed = committed_after(base, &sc.groups, sc.end, d, &change, bugs);
        // The fold starts at the base's durable_lsn, not at its committed_lsn: bytes in (durable_lsn, committed_lsn] may
        // have been replaced since the base was published (a lazy tail lost after a crash or a failed flush and refilled,
        // [F15] FM-3.6, [72 B1]), and folding a group the base already folded changes nothing (maxima, [F05 §10.2]):
        // [F16] P-50, [F04 §9.1] step 2 (spec sync 2b S2B-P-32).
        let mut next = base.clone();
        for g in sc.groups.iter().filter(|g| g.end <= committed) {
            crate::state::fold_slot(&mut next, g, bugs, g.start >= base.committed_lsn)?;
        }
        next.slot_seq = base.slot_seq + 1;
        next.committed_lsn = committed;
        // durable_lsn: the flushed end after a flush (the caller passes max(the slot's value, E), P-45), else unchanged.
        next.durable_lsn = if change.flushed {
            d
        } else if bugs.on(Bug::P07LazyPublishAdvancesDurable) {
            // P-7's seeded bug: a lazy publish sets durable_lsn to its end.
            committed
        } else {
            base.durable_lsn
        };
        if let Some(b) = change.boot {
            next.boot_id = b;
        }
        if bugs.on(Bug::P67UnknownBootWritesBootId)
            && matches!(self.boot, moirai_vfs::BootIdentity::Unknown(_))
        {
            // P-67's seeded bug: an Unknown-boot publisher writes its own (zero) boot identity.
            next.boot_id = [0; 16];
        }
        if change.bump {
            next.config_gen = next.config_gen.wrapping_add(1);
        }
        if let Some((mask, val)) = change.flags {
            next.flags = (next.flags & !mask) | (val & mask);
        }
        // [F04 §7] checks 4 and 5 with [F16] P-48 (spec sync 2b S2B-P-29, R42): a publish never writes a fatal slot. Its
        // own scan may end below the flushed end (a read of a sector a failed flush poisoned, [F15] FM-3.2), or its fold
        // may carry a checkpoint_lsn above durable_lsn; it then writes nothing and exits 7 `store_corrupt`.
        if next.committed_lsn < next.durable_lsn || next.checkpoint_lsn > next.durable_lsn {
            return Err(ToyError::Corrupt(format!(
                "a publish would write a fatal HEAD slot: committed_lsn {}, durable_lsn {}, checkpoint_lsn {}; nothing \
                 is written",
                next.committed_lsn, next.durable_lsn, next.checkpoint_lsn
            )));
        }
        let bytes = next.to_bytes();
        if let SlotRead::Fatal(m) = Slot::read(&bytes) {
            return Err(ToyError::Corrupt(format!(
                "a publish would write a fatal HEAD slot ({m}): committed_lsn {}, durable_lsn {}, checkpoint_lsn {}; \
                 nothing is written",
                next.committed_lsn, next.durable_lsn, next.checkpoint_lsn
            )));
        }
        let target = self.target_slot(which);
        let r = self
            .vfs
            .write_at(&self.head, (target * SLOT_LEN) as u64, &bytes);
        self.io(r)?;
        Ok(next)
    }

    /// The slot a publish whose base is in slot `which` writes ([F16] P-12): the slot that does not hold the newest valid
    /// state; P-12's seeded bug overwrites it.
    fn target_slot(&self, which: usize) -> usize {
        if self.bugs().on(Bug::P12PublishOverNewestSlot) {
            which
        } else {
            1 - which
        }
    }

    /// A durable publish ([F16] P-13, [F04 §9.2]): two publishes under one holding of the writer byte (taken with
    /// `WriterDiag` activity `act`), so both slots hold the newest state, then `durable+meta` on `HEAD` outside the writer
    /// byte. The first carries `change` with the flushed end `d`. P-13's seeded bug writes one slot. The caller holds
    /// nothing; the writer byte is taken here.
    ///
    /// The second publish's base is the slot the first one wrote, not a new read of `HEAD`: no other publish can come
    /// between them under one holding of the writer byte (P-4), while a read could draw, from the other slot's sector
    /// that an earlier failed flush poisoned, an older slot with a greater `slot_seq` ([F15] FM-3.2). Read-modify-writing
    /// that one would overwrite the first publish and leave the poisoned sector unwritten; based on the first publish,
    /// the second writes the other slot, which ends its poisoning ([F04 §9.2] step 1, OP-1; WP-40 spec finding).
    ///
    /// The second publish is the no-op publish of [F04 §9.2] step 1 — the same `committed_lsn` and `durable_lsn`, so its
    /// fold changes nothing, and no changed field — so it is built from the first slot alone, with the next
    /// `slot_seq`: no scan of the log, whose re-read after a failed log flush could draw other bytes ([F15] FM-3.2) and
    /// give the second slot another `committed_lsn` or refuse halfway through the durable publish, and no longer hold of
    /// the writer byte than the two slot writes.
    pub(crate) fn durable_publish(
        &mut self,
        act: u8,
        d: Option<u64>,
        change: Change,
    ) -> Result<Slot, ToyError> {
        self.ensure_named();
        let held = self.take_writer(act)?;
        let r = (|| {
            let (s, w) = self.read_head()?;
            if s.flags & FLAG_RETIRED != 0 && change.flags.is_none() {
                return Err(ToyError::Retired);
            }
            let first = self.publish(&s, w, d.unwrap_or(s.durable_lsn), change)?;
            if self.bugs().on(Bug::P13T9SingleSlotBarrier) {
                return Ok(first);
            }
            // The first publish wrote slot `target_slot(w)`; the second, based on it, writes the slot that does not
            // hold it ([F16] P-12).
            let second = Slot {
                slot_seq: first.slot_seq + 1,
                ..first
            };
            let target = self.target_slot(self.target_slot(w));
            let r = self
                .vfs
                .write_at(&self.head, (target * SLOT_LEN) as u64, &second.to_bytes());
            self.io(r)?;
            Ok(second)
        })();
        if held {
            self.drop_writer();
        }
        let slot = r?;
        self.flush_head();
        Ok(slot)
    }

    /// `durable+meta` on `HEAD` ([F16] P-13), with the error policy.
    pub(crate) fn flush_head(&mut self) {
        self.sync_file(&self.head, SyncKind::DataAndMeta);
    }

    // ---- verbs that change a field kept in HEAD ----

    /// `config set`'s `config_gen` bump ([F16] P-20, [F04 §5.6]): an ordinary publish under the writer byte. P-4's seeded
    /// bug publishes without the writer byte.
    pub fn bump_config(&mut self) -> Result<Slot, ToyError> {
        self.run_later();
        let held = if self.bugs().on(Bug::P04BumpWithoutWriter) {
            false
        } else {
            self.take_writer(activity::HEAD_UPDATE)?
        };
        let r = (|| {
            let (s, w) = self.read_head()?;
            self.publish(
                &s,
                w,
                s.durable_lsn,
                Change {
                    bump: true,
                    ..Change::default()
                },
            )
        })();
        if held {
            self.drop_writer();
        }
        r
    }

    /// `quiet on` / `quiet off` ([F16] P-63): a durable publish whose first write carries the flag; the verb reports
    /// success only after the `HEAD` flush. P-63's seeded bug reports success before the flush: it returns after the
    /// two publishes, and the flush follows with this handle's next operation (or when the handle is dropped).
    pub fn set_quiet(&mut self, on: bool) -> Result<bool, ToyError> {
        self.run_later();
        let change = Change {
            flags: Some((FLAG_QUIET, if on { FLAG_QUIET } else { 0 })),
            ..Change::default()
        };
        if self.bugs().on(Bug::P63FlagReportedBeforeFlush) {
            let held = self.take_writer(activity::HEAD_UPDATE)?;
            let r = (|| {
                let (s, w) = self.read_head()?;
                self.publish(&s, w, s.durable_lsn, change)?;
                let (s, w) = self.read_head()?;
                self.publish(&s, w, s.durable_lsn, Change::default())
            })();
            if held {
                self.drop_writer();
            }
            r?;
            self.head_flush_due = true;
            return Ok(true);
        }
        self.durable_publish(activity::HEAD_UPDATE, None, change)?;
        Ok(true)
    }

    /// The state a scan of the valid log from the view's bound would show now, with the pending groups (a writer's
    /// scratch layer), for tools and tests.
    pub fn scratch(&mut self) -> Result<State, ToyError> {
        self.refresh()?;
        let (s, _) = self.read_head()?;
        let ctx = ScanCtx::of(&s);
        let Some(v) = self.view.as_ref() else {
            return Err(ToyError::Corrupt("no view".to_owned()));
        };
        let (l0, chain) = (v.l0, v.chain);
        let mut st = v.state.clone();
        let bugs = self.bugs();
        self.scan_each(&ctx, l0, chain, None, false, &mut |g| {
            st.apply(&g, bugs, true).map_err(ToyError::from)
        })?;
        Ok(st)
    }
}

/// Whether P-30's seeded bug keeps the scratch layer in the view: only when the layer was built (the decision itself may
/// still have refused).
fn undo_kept(planned: &Result<Planned, ToyError>) -> bool {
    !matches!(planned, Err(ToyError::Corrupt(_)))
}

/// P-49's `committed_lsn` for a publish over `base` whose scan read `groups` (ending at `end`) with the flushed end `d`:
/// the end of the valid log, stopping before the first pending durable group the flush does not cover; P-38's seeded
/// bug forces it up to an end, P-49's decrease lowers it to the valid end an appender found, and P-49's seeded bug never
/// lets it go below the base's.
pub(crate) fn committed_after(
    base: &Slot,
    groups: &[Group],
    end: u64,
    d: u64,
    change: &Change,
    bugs: Bugs,
) -> u64 {
    let mut committed = base.durable_lsn;
    for g in groups {
        if g.durable() && g.end > d {
            break;
        }
        committed = g.end;
    }
    if let Some(f) = change.force_committed {
        committed = committed.max(f.min(end));
    }
    if let Some(low) = change.lower_committed {
        committed = committed.min(low).max(base.durable_lsn);
    }
    if bugs.on(Bug::P49CommittedAboveValidEnd) {
        committed = committed.max(base.committed_lsn);
    }
    committed
}

/// Phase 2a's decision over the scratch layer ([F16] P-30–P-36, P-70): the idempotency key after the scan (P-32),
/// re-validation (P-34), allocation from the slot and the scanned groups (P-31), the HLC (P-36), the parks of failing
/// commits (P-70), the operation's groups and the final size checks (P-35, W1).
fn plan(op: &Op, d1: &Decision, scratch: &State, c: &PlanCtx<'_>) -> Result<Planned, ToyError> {
    let bugs = c.bugs;
    // P-32: the idempotency key is evaluated after the scan; its seeded bug evaluated it at L0 only (phase 1).
    if !bugs.on(Bug::P32T4IdempotencyBeforeScan)
        && let Some((result, lsn)) = op.lookup(scratch)
    {
        return Ok(Planned::Hit(result, lsn));
    }
    // P-34: re-validation by key; "the candidate stands" only when no group lies beyond L0.
    let stands = if bugs.on(Bug::P34CandidateStandsAtCommitted) {
        c.slot.committed_lsn == c.l0
    } else {
        !c.beyond_l0
    };
    // A runtime batch's content (its padding) is fixed in phase 1; every other decision is re-validated.
    let d = if stands || matches!(d1, Decision::Runtime { .. }) {
        d1.clone()
    } else {
        decide(op, scratch, c.now, bugs).map_err(ToyError::Refused)?
    };
    // P-31 allocation and P-36's HLC maxima: the newest slot's counters and what the groups the scan found beyond its
    // committed_lsn imply (pending groups included), folded as the HEAD fold folds them; no scan below committed_lsn is
    // needed.
    let mut basis = c.slot.clone();
    for g in c.pending {
        crate::state::fold_slot(&mut basis, g, bugs, true)?;
    }
    let mut alloc = Alloc::new(&c.slot.counters, &basis.counters, bugs);
    let mut hlc = Hlc::new(
        c.wall_ms,
        basis.counters.hlc_seq,
        basis.counters.hlc_commit,
        bugs,
    );
    let before = (alloc.before, hlc.seq, hlc.commit);
    // P-70: the first appender of a durable group that meets an unparked failing commit parks it first, before its own
    // group; the park's records take their ids and HLCs first, in log order (P-31, P-36). Two parks of one origin share
    // the `orphans/<R>` ref the first creates.
    let mut groups: Vec<Vec<Rec>> = Vec::new();
    if op.durable(bugs) {
        let mut orphans: BTreeMap<u32, (u32, u64)> = BTreeMap::new();
        for (&o, &r) in &scratch.unparked {
            groups.push(park_group(
                o,
                r,
                scratch,
                &mut alloc,
                &mut hlc,
                bugs,
                &mut orphans,
            ));
        }
    }
    let n_park = groups.len();
    let built = build(op, &d, scratch, &mut alloc, &mut hlc, bugs).map_err(ToyError::Refused)?;
    groups.extend(built.groups);
    // P-35: W1 and W3 on the final encoding.
    if !bugs.on(Bug::P35NoFinalSizeCheck)
        && groups.iter().any(|g| group_len(g) > c.e - ROTATION_RESERVE)
    {
        return Err(ToyError::Refused("commit_too_large"));
    }
    // W1 ([F17 §4.4]): the commit's counted quantity, `cs_bytes`, against P05 exactly; an agent verb refuses with E501.
    if let Op::Commit(_) = op
        && groups
            .iter()
            .skip(n_park)
            .flatten()
            .filter(|r| r.kind == kind::COMMIT)
            .any(|r| CommitRec::cs_bytes(&r.payload).is_ok_and(|n| n > u64::from(c.inline_max)))
    {
        return Err(ToyError::Refused(
            "E501: the inline commit exceeds store.commit.inline-max-bytes",
        ));
    }
    Ok(Planned::Append(Box::new(Plan {
        groups,
        n_park,
        before,
        result: built.result,
        later: built.later,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::RecView;

    fn g(start: u64, end: u64, durable: bool) -> Group {
        Group {
            start,
            end,
            chain_in: 0,
            chain_out: 0,
            recs: vec![RecView {
                kind: if durable {
                    kind::COMMIT
                } else {
                    kind::FILE_OBS
                },
                lazy: !durable,
                lsn: start,
                has_symdefs: false,
                body: CommitRec::default().encode(),
            }],
            raw: Vec::new(),
        }
    }

    fn base(durable: u64, committed: u64) -> Slot {
        let mut s = crate::head::tests::sample();
        s.durable_lsn = durable;
        s.committed_lsn = committed;
        s
    }

    #[test]
    fn committed_stops_before_the_first_uncovered_durable_group() {
        let b = base(300, 300);
        let gs = [g(300, 400, false), g(400, 500, true), g(500, 600, false)];
        let none = Change::default();
        // A publish without a flush covers no durable group: lazy groups before the first durable one are published.
        assert_eq!(committed_after(&b, &gs, 600, 300, &none, Bugs::NONE), 400);
        // A flush to 500 covers the durable group; the lazy group after it is published with it.
        assert_eq!(committed_after(&b, &gs, 600, 500, &none, Bugs::NONE), 600);
        // A flush that ends inside the durable group covers nothing beyond it.
        assert_eq!(committed_after(&b, &gs, 600, 450, &none, Bugs::NONE), 400);
        // No group: committed_lsn is the base's durable_lsn.
        assert_eq!(committed_after(&b, &[], 300, 300, &none, Bugs::NONE), 300);
    }

    #[test]
    fn committed_decreases_only_to_the_valid_end_and_p38_p49_bugs() {
        let b = base(300, 700);
        let gs = [g(300, 400, false)];
        // P-49's decrease: the valid log ends at 400 below the base's committed_lsn 700.
        let lower = Change {
            lower_committed: Some(400),
            ..Change::default()
        };
        assert_eq!(committed_after(&b, &gs, 400, 300, &lower, Bugs::NONE), 400);
        // Never below durable_lsn.
        let below = Change {
            lower_committed: Some(100),
            ..Change::default()
        };
        assert_eq!(committed_after(&b, &gs, 400, 300, &below, Bugs::NONE), 300);
        // P-49's seeded bug keeps the base's committed_lsn.
        assert_eq!(
            committed_after(
                &b,
                &gs,
                400,
                300,
                &lower,
                Bugs::only(Bug::P49CommittedAboveValidEnd)
            ),
            700
        );
        // P-38's seeded bug forces committed_lsn past a pending durable group (never past the scanned end).
        let b = base(300, 300);
        let gs = [g(300, 400, true), g(400, 500, false)];
        let force = Change {
            force_committed: Some(500),
            ..Change::default()
        };
        assert_eq!(committed_after(&b, &gs, 500, 300, &force, Bugs::NONE), 500);
        let force_far = Change {
            force_committed: Some(900),
            ..Change::default()
        };
        assert_eq!(
            committed_after(&b, &gs, 500, 300, &force_far, Bugs::NONE),
            500
        );
    }

    #[test]
    fn undo_is_kept_by_p30_only_when_the_layer_was_built() {
        assert!(undo_kept(&Err(ToyError::Refused("x"))));
        assert!(!undo_kept(&Err(ToyError::Corrupt("m".to_owned()))));
        assert!(undo_kept(&Ok(Planned::Hit(1, 2))));
    }
}

/// The write path on the in-memory `Vfs`, one handle at a time.
#[cfg(test)]
mod sim_tests {
    use super::*;
    use crate::config::Config;
    use crate::ops::{CommitOp, RuntimeOp};
    use crate::state::MAIN;
    use crate::testing::{STORE, open, sim_store};
    use crate::{Bug, ProcLocks};
    use moirai_vfs::{Clock, RelPath, RootAccess, RootRole, ShareRetry, StoreFs};
    use std::path::Path;
    use std::sync::{Arc, Mutex, PoisonError};

    fn commit(op: u64) -> Op {
        Op::Commit(CommitOp {
            op,
            digest: op,
            ref_name: MAIN,
            creates: vec![op << 8],
            key: Some(op),
            filler: 200,
            ..CommitOp::default()
        })
    }

    fn head_bytes(t: &Toy<moirai_vfs_sim::SimVfs>) -> Vec<u8> {
        let mut b = vec![0u8; HEAD_LEN];
        t.vfs
            .read_exact_at(&t.head, 0, &mut b)
            .unwrap_or_else(|e| panic!("{e}"));
        b
    }

    /// The virtual time one commit takes with the injected in-lock cost `cost` (the same world seed each time).
    fn commit_time(cost: u64) -> u64 {
        let mut cfg = Config::test_profile();
        cfg.in_lock_cost_ns = cost;
        let (_w, v, _) = sim_store(&cfg, 21);
        let mut t = open(&v, &cfg);
        let t0 = v.mono_ns();
        let d = t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        assert!(!d.replayed && d.end > d.start);
        v.mono_ns() - t0
    }

    #[test]
    fn the_in_lock_cost_is_spent_once_per_append_and_ends_on_the_simulator() {
        let base = commit_time(0);
        let cost = 3_000_000;
        let with = commit_time(cost);
        assert!(
            with >= base + cost,
            "the cost is spent: {with} ns with, {base} ns without"
        );
        assert!(
            with < base + 2 * cost,
            "the cost is spent once per append, not at every holding of the writer byte: {with} ns with, {base} ns without"
        );
    }

    #[test]
    fn commits_are_acknowledged_in_order_and_replayed_by_key() {
        let cfg = Config::test_profile();
        let (_w, v, _) = sim_store(&cfg, 22);
        let mut t = open(&v, &cfg);
        let a = t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        let b = t.run(&commit(2)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(b.start, a.end);
        assert_eq!(b.chain_in, a.chain_out);
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert!(s.durable_lsn >= b.end && s.committed_lsn >= b.end);
        // The same key again is an idempotent replay of the stored result, with nothing appended.
        let again = t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        assert!(again.replayed);
        assert_eq!(again.result, 1);
        assert_eq!(
            t.read_head()
                .unwrap_or_else(|e| panic!("{e}"))
                .0
                .committed_lsn,
            s.committed_lsn
        );
        // WriterDiag counts the handle's acquisitions of the writer byte ([F03 §6.1] seq).
        assert!(t.diag_seq >= 6, "{}", t.diag_seq);
    }

    #[test]
    fn a_publish_never_writes_a_fatal_slot() {
        let cfg = Config::test_profile();
        let (_w, v, _) = sim_store(&cfg, 23);
        let mut t = open(&v, &cfg);
        let (s, which) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        let before = head_bytes(&t);
        // A flushed end beyond the valid log would give durable_lsn above committed_lsn: refused, nothing written.
        let r = t.publish(
            &s,
            which,
            s.durable_lsn + 4_096,
            Change {
                flushed: true,
                ..Change::default()
            },
        );
        assert!(
            matches!(&r, Err(ToyError::Corrupt(m)) if m.contains("fatal")),
            "{r:?}"
        );
        assert_eq!(head_bytes(&t), before);
        // An ordinary publish writes the other slot with the next slot_seq.
        let next = t
            .publish(&s, which, s.durable_lsn, Change::default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(next.slot_seq, s.slot_seq + 1);
        let (now, w) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((now.slot_seq, w), (next.slot_seq, 1 - which));
    }

    #[test]
    fn a_flushed_range_over_a_missing_extent_is_an_error() {
        let cfg = Config::test_profile();
        let (_w, v, _) = sim_store(&cfg, 24);
        let mut t = open(&v, &cfg);
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        let ctx = ScanCtx::of(&s);
        assert_eq!(t.flush_range(&ctx, 0, 100), Ok(()));
        assert_eq!(t.flush_range(&ctx, 100, 100), Ok(()));
        let r = t.flush_range(&ctx, 0, 2 * ctx.e);
        assert!(
            matches!(&r, Err(ToyError::Corrupt(m)) if m.contains("log.2")),
            "{r:?}"
        );
    }

    /// [F16] P-72 step 2 with P-96: a spare's rename onto log.<m> can land between the rotator's check of log.<m> and
    /// its `create_extent`, whose exclusive create then fails `AlreadyExists`; the rotator runs the check again on the
    /// file now there, a full-length spare, and makes it ready, never refusing for it. The rename is tried after every
    /// number of the rotator's scheduling points: the preparation succeeds in each interleaving and leaves log.<m> at
    /// full length, and in at least one the create met the spare.
    #[test]
    fn a_spare_renamed_onto_the_extent_before_its_create_is_made_ready() {
        use moirai_vfs_sim::{CallKind, EventKind, Site, TaskEnd, error_code};
        let cfg = Config::test_profile();
        let e = cfg.extent_bytes;
        let store = Path::new(STORE);
        let exists = error_code(VfsErrorKind::AlreadyExists);
        let mut raced = 0;
        for k in 0..160 {
            let (w, v, _) = sim_store(&cfg, 33);
            w.put_file(&store.join("tmp").join("extent.1"), &vec![0u8; e as usize])
                .unwrap_or_else(|e| panic!("{e:?}"));
            let c = cfg.clone();
            let rotator = w.spawn(&v, move |v| {
                let mut t = open(&v, &c);
                let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
                t.prepare_extent(&ScanCtx::of(&s), 2)
            });
            let p = w.process_with("spare", None, Some(true));
            w.spawn(&p, |p| {
                let root = p
                    .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
                    .unwrap_or_else(|e| panic!("{e}"));
                // AlreadyExists when the rotator created log.2 first: the spare's preparer then deletes its temporary.
                let _ = p.rename_noreplace(
                    &root,
                    crate::store::rel("tmp/extent.1").as_rel_path(),
                    &root,
                    log_name(2).as_rel_path(),
                    ShareRetry::None,
                );
            });
            // The rotator runs k scheduling points, then the spare's process runs to its end.
            for _ in 0..k {
                w.queue_choice(Site::Schedule, 0);
            }
            for _ in 0..64 {
                w.queue_choice(Site::Schedule, 1);
            }
            w.run();
            let r = rotator.end().map(TaskEnd::unwrap);
            assert_eq!(r, Some(Ok(())), "rename after {k} points");
            assert_eq!(
                w.file_len(&store.join("log.2")),
                Some(e),
                "rename after {k} points"
            );
            raced += w
                .trace()
                .iter()
                .filter(|x| {
                    x.kind == EventKind::Return
                        && x.a == CallKind::CreateNew as u64
                        && x.c == exists
                })
                .count();
        }
        assert!(
            raced > 0,
            "no interleaving put the rename between the check and the create"
        );
    }

    /// A tap that records every note.
    #[derive(Clone, Default)]
    struct RecTap(Arc<Mutex<Vec<Note>>>);

    impl Tap for RecTap {
        fn note(&self, n: Note) {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(n);
        }
    }

    impl RecTap {
        /// How many phase-3 runs began.
        fn phase3(&self) -> usize {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .filter(|n| matches!(n, Note::Phase3(true)))
                .count()
        }
    }

    /// A lazy runtime batch of about `pad` bytes.
    fn batch(op: u64, pad: u64) -> Op {
        Op::Runtime(RuntimeOp {
            op,
            rows: vec![(op, op)],
            pad: pad as u32,
            symbols: Vec::new(),
            target_len: 0,
        })
    }

    /// [F16] P-51 (spec sync 2b S2B-P-35): a write made while the process holds the maintenance byte — its own
    /// `Checkpoint` and barrier, intent recovery's commits — runs no phase 3, which would re-acquire a byte it holds
    /// ([OS/lock §3] item 3); a writer that holds nothing runs it after the acknowledgement.
    #[test]
    fn writes_under_the_maintenance_byte_run_no_phase_3() {
        let mut cfg = Config::test_profile();
        cfg.auto_checkpoint_bytes = 1;
        let (_w, v, _) = sim_store(&cfg, 26);
        let tap = RecTap::default();
        let mut t = Toy::open(
            v.clone(),
            Path::new(STORE),
            cfg.clone(),
            tap.clone(),
            ProcLocks::new(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            t.checkpoint().unwrap_or_else(|e| panic!("{e}")).is_some(),
            "the maintenance run checkpointed"
        );
        t.recover_intents().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(tap.phase3(), 0, "the maintenance holder's writes");
        t.run(&commit(2)).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            t.locks
                .try_take(&t.vfs, LockByte::Maintenance)
                .unwrap_or_else(|e| panic!("{e}"))
        );
        t.maintain();
        assert_eq!(
            tap.phase3(),
            0,
            "phase 3 while holding the maintenance byte"
        );
        t.locks.release(&t.vfs, LockByte::Maintenance);
        let before = t
            .read_head()
            .unwrap_or_else(|e| panic!("{e}"))
            .0
            .checkpoint_lsn;
        t.maintain();
        assert_eq!(tap.phase3(), 1);
        let after = t
            .read_head()
            .unwrap_or_else(|e| panic!("{e}"))
            .0
            .checkpoint_lsn;
        assert!(after > before, "phase 3 checkpointed: {before} -> {after}");
    }

    /// [F16] P-29 with [F05 §5.3] (spec sync 2b S2B-P-31, R41): an appender whose scan finds the next extent missing
    /// below `durable_lsn` refuses the store (exit 7) instead of taking it for the end of the log.
    #[test]
    fn a_missing_extent_below_durable_lsn_stops_an_appender() {
        let cfg = Config::test_profile();
        let e = cfg.extent_bytes;
        let (_w, v, _) = sim_store(&cfg, 27);
        let mut t = open(&v, &cfg);
        let mut u = open(&v, &cfg);
        let op = commit(1);
        // t's phase 1 sees the store as `init` left it.
        let (d1, hit) = t.phase1(&op).unwrap_or_else(|e| panic!("{e}"));
        assert!(hit.is_none());
        // u fills log.1, rotates into log.2 and flushes past its first byte.
        for o in [batch(7, e / 2), commit(2), batch(8, e / 2), commit(3)] {
            u.run(&o).unwrap_or_else(|e| panic!("{e}"));
        }
        let (s, _) = u.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert!(s.durable_lsn > e, "{}", s.durable_lsn);
        // log.2 vanishes (an external act, [F15] FM-10).
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .unwrap_or_else(|e| panic!("{e}"));
        drop(u);
        v.unlink(&root, RelPath::literal("log.2"), ShareRetry::None)
            .unwrap_or_else(|e| panic!("{e}"));
        match t.phase2a(&op, &d1) {
            Err(ToyError::Corrupt(m)) => assert!(m.contains("log.2 is missing"), "{m}"),
            Err(e) => panic!("{e}"),
            Ok(_) => panic!("the appender went on over a missing extent below durable_lsn"),
        }
    }

    /// [F04 §7], [F16] P-61 and P-85 (spec sync 2b S2B-P-28): plain `repair` accepts a store whose newest slot is fatal,
    /// trusts neither slot, and rebuilds both from the extent heads.
    #[test]
    fn plain_repair_rebuilds_both_slots_over_a_fatal_slot() {
        let cfg = Config::test_profile();
        let (_w, v, _) = sim_store(&cfg, 28);
        let mut t = open(&v, &cfg);
        let d = t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        // The newest slot rewritten with a checksummed slot that fails [F04 §7] check 5 ([F15] FM-10.1).
        let (mut s, which) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        s.durable_lsn = s.committed_lsn + 8;
        t.vfs
            .write_at(&t.head, (which * SLOT_LEN) as u64, &s.to_bytes())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(t.read_head(), Err(ToyError::FatalSlot(_))));
        assert_eq!(t.repair(), Ok(Vec::new()));
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((s.committed_lsn, s.durable_lsn), (d.end, d.end));
        let other = t.other_slot().unwrap_or_else(|| panic!("two valid slots"));
        assert_eq!(other.committed_lsn, d.end);
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        assert!(st.commits.contains_key(&1));
    }

    /// [F16] P-85 step 4 as a durable publish ([F04 §9.2]): `repair` writes the two slots in two writes, the one that
    /// reads as valid first. A failed first write ([F15] FM-5.2: part of it applied) then leaves the fatal slot in place,
    /// so every process still refuses the store (P-61) and none serves the older valid slot it replaced.
    #[test]
    fn a_cut_repair_never_exposes_the_older_slot_beside_a_fatal_one() {
        use moirai_vfs_sim::{PartialWrite, Site};
        let cfg = Config::test_profile();
        let (w, v, _) = sim_store(&cfg, 33);
        let mut t = open(&v, &cfg);
        let mut op = 1;
        // Commits until the newest slot is slot 0, so that the older valid slot is slot 1.
        let (mut s, mut which) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        while which != 0 || s.committed_lsn == 0 {
            t.run(&commit(op)).unwrap_or_else(|e| panic!("{e}"));
            op += 1;
            (s, which) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        }
        s.durable_lsn = s.committed_lsn + 8;
        t.vfs
            .write_at(&t.head, 0, &s.to_bytes())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(t.read_head(), Err(ToyError::FatalSlot(_))));
        // The repair's first write to `HEAD` fails with its first 100 bytes applied.
        let head = w
            .node_at(&Path::new(STORE).join("HEAD"))
            .unwrap_or_else(|| panic!("HEAD"));
        w.queue_choice_on(None, Site::WriteFault, head, 1);
        w.queue_choice_on(
            None,
            Site::PartialWrite,
            head,
            PartialWrite::Prefix(100).to_choice(),
        );
        assert_eq!(t.repair_head(), Err(ToyError::DiskFull));
        assert!(matches!(t.read_head(), Err(ToyError::FatalSlot(_))));
        // The operator runs `repair` again, which rebuilds both slots.
        t.repair_head().unwrap_or_else(|e| panic!("{e}"));
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        assert!((1..op).all(|o| st.commits.contains_key(&o)));
    }

    /// [OS/lock] contract item 3: with every switch off, a handle's second acquisition of a byte it holds is a refusal
    /// of the lock layer; only P-51's seeded bug nests it in the holding (and its release then keeps the byte).
    #[test]
    fn a_second_acquisition_of_a_held_byte_is_refused_with_every_switch_off() {
        for bugs in [Bugs::NONE, Bugs::only(Bug::P51MaintenanceUnderWriter)] {
            let cfg = Config::test_profile().with_bugs(bugs);
            let (_w, v, _) = sim_store(&cfg, 34);
            let mut t = open(&v, &cfg);
            assert_eq!(t.take_writer(activity::APPEND), Ok(true));
            let again = t.take_writer(activity::APPEND);
            if bugs.is_none() {
                assert!(
                    matches!(&again, Err(ToyError::Lock(m)) if m.contains("holds already")),
                    "{again:?}"
                );
            } else {
                assert_eq!(again, Ok(true));
                t.drop_writer();
                assert!(
                    t.locks.holds(LockByte::Writer),
                    "the nested release keeps the byte"
                );
            }
            t.drop_writer();
            assert!(!t.locks.holds(LockByte::Writer));
        }
    }

    /// [F17 §4.4] W1: a commit's counted quantity is `cs_bytes`, its changeset part (here the count of creates, the
    /// creates and the filler with its length), compared with P05 exactly: a commit of exactly P05 is appended, one byte
    /// more is refused with E501 and appends nothing.
    #[test]
    fn w1_compares_cs_bytes_with_p05_exactly() {
        let cfg = Config::test_profile();
        let p05 = u64::from(cfg.inline_max_bytes);
        let (_w, v, _) = sim_store(&cfg, 30);
        let mut t = open(&v, &cfg);
        let sized = |op: u64, cs: u64| {
            // No creates: one byte of `n_ops`, then the filler's length and the filler.
            let mut filler = cs - 1;
            while 1 + crate::codec::uvar_len(filler) as u64 + filler > cs {
                filler -= 1;
            }
            assert_eq!(1 + crate::codec::uvar_len(filler) as u64 + filler, cs);
            Op::Commit(CommitOp {
                op,
                digest: op,
                ref_name: MAIN,
                key: Some(op),
                filler: filler as u32,
                ..CommitOp::default()
            })
        };
        let d = t.run(&sized(1, p05)).unwrap_or_else(|e| panic!("{e}"));
        assert!(!d.replayed);
        let before = t.read_head().unwrap_or_else(|e| panic!("{e}")).0;
        assert_eq!(
            t.run(&sized(2, p05 + 1)),
            Err(ToyError::Refused(
                "E501: the inline commit exceeds store.commit.inline-max-bytes"
            ))
        );
        let after = t.read_head().unwrap_or_else(|e| panic!("{e}")).0;
        assert_eq!(
            after.committed_lsn, before.committed_lsn,
            "nothing was appended"
        );
        // The counted quantity of an encoded commit.
        let rec = CommitRec {
            creates: vec![(7, 9)],
            filler: 300,
            ..CommitRec::default()
        };
        let n = CommitRec::cs_bytes(&rec.encode()).unwrap_or_else(|_| panic!("a commit payload"));
        assert_eq!(n, 1 + (1 + 8) + 2 + 300);
    }

    /// [F16] P-97: its seeded bug begins a new extent without its extent head, and the flush holder's scan, which reads
    /// the extent, refuses the store ([F05 §5.4]); with every switch off the same rotation is acknowledged.
    #[test]
    fn p97_a_rotation_without_its_extent_head_is_refused() {
        for bugs in [Bugs::NONE, Bugs::only(Bug::P97NoExtentHead)] {
            let cfg = Config::test_profile().with_bugs(bugs);
            let e = cfg.extent_bytes;
            let (_w, v, _) = sim_store(&cfg, 29);
            let mut t = open(&v, &cfg);
            for o in [batch(7, e / 2), commit(2)] {
                t.run(&o).unwrap_or_else(|e| panic!("{e}"));
            }
            let r = t.run(&batch(8, e / 2));
            if bugs.is_none() {
                assert!(r.is_ok(), "{r:?}");
            } else {
                assert!(
                    matches!(&r, Err(ToyError::Corrupt(m)) if m.contains("not its extent head")),
                    "{r:?}"
                );
            }
        }
    }

    #[test]
    fn repair_rebuilds_both_slots_from_the_log() {
        let cfg = Config::test_profile();
        let (_w, v, img) = sim_store(&cfg, 25);
        let mut t = open(&v, &cfg);
        let d = t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        // Both slots damaged: no valid slot.
        t.vfs
            .write_at(&t.head, 0, &vec![0xA5; HEAD_LEN])
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(t.read_head().map(|_| ()), Err(ToyError::NoValidSlot));
        t.repair_head().unwrap_or_else(|e| panic!("{e}"));
        let (s, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.committed_lsn, d.end);
        assert_eq!(s.durable_lsn, d.end);
        assert!(s.counters.commit_seq >= 1);
        assert_eq!(s.epoch, crate::testing::EPOCH);
        assert!(img.log.len() as u64 <= d.start);
        // The repaired store reads the commit.
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        assert!(st.commits.contains_key(&1));
    }

    /// [F04 §9.2] step 1 and OP-1 under [F15] FM-3.2: a durable publish's second publish is based on the slot its first
    /// one wrote, so the two write both slots — ending the poisoning of the newest slot's sector — even when a read of
    /// that poisoned sector would draw an older slot with a greater `slot_seq`.
    #[test]
    fn a_durable_publish_writes_both_slots_over_a_poisoned_sector() {
        use moirai_vfs_sim::Site;
        let cfg = Config::test_profile();
        let (w, v, _) = sim_store(&cfg, 32);
        let mut t = open(&v, &cfg);
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        t.durable_publish(activity::HEAD_UPDATE, None, Change::default())
            .unwrap_or_else(|e| panic!("{e}"));
        let (s, newest) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        // An unflushed write of an older state with a greater slot_seq into the newest slot's sector, then a failed
        // flush of `HEAD`: that sector's candidates are its flushed slot and the stale one (FM-3.1).
        let mut stale = s.clone();
        stale.slot_seq += 5;
        t.vfs
            .write_at(&t.head, (newest * SLOT_LEN) as u64, &stale.to_bytes())
            .unwrap_or_else(|e| panic!("{e}"));
        w.queue_choice(Site::FlushFault, 1);
        assert!(v.sync(&t.head, SyncKind::Data).is_err());
        // The first read draws the flushed slot; any later read of the poisoned sector draws the stale one.
        for k in 0..64 {
            w.queue_choice(Site::PoisonRead, u64::from(k >= 8));
        }
        let change = Change {
            flags: Some((FLAG_QUIET, FLAG_QUIET)),
            ..Change::default()
        };
        let done = t
            .durable_publish(activity::HEAD_UPDATE, None, change)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(done.slot_seq, s.slot_seq + 2);
        let (now, _) = t.read_head().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(now.slot_seq, s.slot_seq + 2);
        let other = t.other_slot().unwrap_or_else(|| panic!("two valid slots"));
        assert_eq!(other.slot_seq, s.slot_seq + 1);
        assert!(now.flags & FLAG_QUIET != 0 && other.flags & FLAG_QUIET != 0);
    }

    /// [F16] P-85 step 4 with [80 §2.3.4] decision (a) and [F15] FM-3.4, FM-3.5: `repair` of a store with no valid slot
    /// re-writes every group it scanned, with the bytes its scan validated, before its flush. Here a failed flush
    /// poisoned the sector of a group that was appended and never flushed, and the repair's scan read its newest
    /// version: after the repair the sector is definite, so no later read draws from the candidate set (FM-3.2), and the
    /// group the repaired slot covers stays readable below its `durable_lsn`.
    #[test]
    fn repair_rewrites_a_poisoned_scanned_group_before_its_flush() {
        use moirai_vfs_sim::{EventKind, Site};
        let cfg = Config::test_profile();
        let (w, v, _) = sim_store(&cfg, 31);
        let mut t = open(&v, &cfg);
        t.run(&commit(1)).unwrap_or_else(|e| panic!("{e}"));
        // Commit 2 is appended (phases 1 and 2a) and never flushed or published.
        let op = commit(2);
        let (d1, _) = t.phase1(&op).unwrap_or_else(|e| panic!("{e}"));
        t.phase2a(&op, &d1).unwrap_or_else(|e| panic!("{e}"));
        // A failed flush of log.1 poisons its dirty sector (FM-3.1): K holds it without and with commit 2's group.
        w.queue_choice(Site::FlushFault, 1);
        let log1 = t.ext(1).unwrap_or_else(|| panic!("log.1 is open"));
        assert!(v.sync(log1, SyncKind::Data).is_err());
        // Both slots damaged: no valid slot.
        t.vfs
            .write_at(&t.head, 0, &vec![0xA5; HEAD_LEN])
            .unwrap_or_else(|e| panic!("{e}"));
        // The repair's reads of the poisoned sector draw its newest member, so the scan validates commit 2's group.
        for _ in 0..256 {
            w.queue_choice(Site::PoisonRead, 1);
        }
        t.repair_head().unwrap_or_else(|e| panic!("{e}"));
        let from = w.trace().len();
        let st = t.read().unwrap_or_else(|e| panic!("{e}"));
        assert!(st.commits.contains_key(&1) && st.commits.contains_key(&2));
        let poisoned = w.trace()[from..]
            .iter()
            .filter(|e| {
                matches!(e.kind, EventKind::Choice | EventKind::Injected)
                    && e.a == Site::PoisonRead as u64
            })
            .count();
        assert_eq!(
            poisoned, 0,
            "a read after the repair drew from a poisoned sector"
        );
    }
}
