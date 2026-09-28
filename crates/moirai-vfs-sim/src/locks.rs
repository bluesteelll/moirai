//! The lock bytes: the simulated kernel's side ([OS/lock §7.3]) and the driver of each simulated process's
//! [`GrantTable`] ([OS/lock §5]).
//!
//! The kernel keeps a map `(LOCK node, byte offset) → holder` and a list of kernel waits. A live holder releases only by
//! its own unlock; a dead process's byte becomes a zombie released after FM-8.1's delay (or never). A freed byte goes
//! to one waiting kernel wait chosen by the adversary (item 10: fairness is not part of the contract), or to none for a
//! moment (a spurious timeout, §3.13). Each simulated process has one grant table per `LOCK` ([OS/lock §5.1]); the
//! driver below performs its steps exactly as the module documentation of `moirai_vfs::grant` prescribes, in both wait
//! modes (`CallerDriven`: the waiting task drives the kernel wait; `WaiterThread`: a simulated waiter reports the grant
//! inside the kernel).
//!
//! Every `LOCK` handle a table opens after the client's data handle — a role handle at the table's first kernel try of
//! that role, a slot handle at every slot try, the probe handle at the first kernel probe, a waiter's OFD — is opened
//! by path and identity-checked against the data handle ([OS/lock §9.1] step 4, §9.2): after a `LOCK` replacement
//! (FM-10.3) the acquisition fails with `IdentityMismatch` (a probe answers `Unknown`, [OS/lock §8]); a delete-pending
//! `LOCK` or a sharing violation fails the open as it would any open (FM-8.2, FM-8.3).

use std::collections::BTreeMap;
use std::sync::Arc;

use moirai_vfs::{
    Access, Acquired, CancelStep, ClientId, DeadlineStep, FOREIGN_CHECK_BYTE, Grant, GrantStep,
    GrantTable, KernelResult, LockByte, LockError, LockMode, Outcome, ProbeResult, ProbeStep,
    RelPath, Step, TableId, TryStep, VfsError, VfsErrorKind, WaitStep,
};

use crate::adversary::Site;
use crate::trace::EventKind;
use crate::vfs::{SimFile, SimRoot};
use crate::world::{Block, CallKind, Ctx, Shared, State, TableRec, error_code, os_code};

/// The holder of a byte in the kernel.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Holder {
    Live {
        proc: u32,
    },
    /// A dead process's byte, released at `release_at` on the boot clock, or never (FM-8.1).
    Zombie {
        proc: u32,
        release_at: Option<u64>,
    },
}

/// Who performs a kernel wait.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KDriver {
    /// `CallerDriven`: the waiting client's own task.
    Caller,
    /// `WaiterThread`: the process's simulated waiter.
    Thread,
}

/// A registered kernel wait.
#[derive(Clone, Debug)]
pub(crate) struct KWait {
    pub(crate) id: u64,
    pub(crate) node: u64,
    pub(crate) byte: u64,
    pub(crate) proc: u32,
    pub(crate) driver: KDriver,
    pub(crate) granted: bool,
}

/// The kernel's lock state.
#[derive(Clone, Debug, Default)]
pub(crate) struct KernelLocks {
    pub(crate) held: BTreeMap<(u64, u64), Holder>,
    pub(crate) waits: Vec<KWait>,
    pub(crate) next_wait: u64,
}

impl KernelLocks {
    /// The earliest zombie release.
    pub(crate) fn next_release(&self) -> Option<u64> {
        self.held
            .values()
            .filter_map(|h| match h {
                Holder::Zombie { release_at, .. } => *release_at,
                Holder::Live { .. } => None,
            })
            .min()
    }

    /// Removes and returns the zombies due at `now`.
    pub(crate) fn due_releases(&mut self, now: u64) -> Vec<(u64, u64)> {
        let due: Vec<(u64, u64)> = self
            .held
            .iter()
            .filter(|(_, h)| matches!(h, Holder::Zombie { release_at: Some(t), .. } if *t <= now))
            .map(|(&k, _)| k)
            .collect();
        for k in &due {
            self.held.remove(k);
        }
        due
    }

    /// Drops the kernel waits of process `p`. A granted but unreported wait already holds its byte as a live holder of
    /// `p`, which the caller turns into a zombie with the process's other bytes (`held_by`).
    pub(crate) fn drop_waits_of(&mut self, p: u32) {
        self.waits.retain(|w| w.proc != p);
    }

    /// The bytes process `p` holds live.
    pub(crate) fn held_by(&self, p: u32) -> Vec<(u64, u64)> {
        self.held
            .iter()
            .filter(|(_, h)| **h == Holder::Live { proc: p })
            .map(|(&k, _)| k)
            .collect()
    }

    pub(crate) fn make_zombie(&mut self, node: u64, byte: u64, proc: u32, release_at: Option<u64>) {
        self.held
            .insert((node, byte), Holder::Zombie { proc, release_at });
    }

    pub(crate) fn clear(&mut self) {
        self.held.clear();
        self.waits.clear();
    }
}

// ---- kernel operations ----

/// Which `LOCK` handle a table opens ([OS/lock §9.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LockHandle {
    /// The role handle of a role byte, opened at the table's first kernel try of that role.
    Role(u64),
    /// A slot handle, opened at every slot try.
    Slot,
    /// The probe handle, opened at the table's first kernel probe.
    Probe,
    /// A waiter thread's own OFD (mode `WaiterThread`).
    Waiter,
}

/// Opens a `LOCK` handle of table `(proc, node)` if it is not open yet, with the identity check of [OS/lock §9.1] step
/// 4: the path `LOCK` in the table's store must still name the data handle's file.
fn open_lock_handle(st: &mut State, proc: u32, node: u64, h: LockHandle) -> Result<(), LockError> {
    let Some(rec) = st.tables.get(&(proc, node)) else {
        return Ok(());
    };
    let already = match h {
        LockHandle::Role(offset) => rec.roles_open & role_bit(offset) != 0,
        LockHandle::Probe => rec.probe_open,
        LockHandle::Slot | LockHandle::Waiter => false,
    };
    if already {
        return Ok(());
    }
    let store = rec.store;
    let os = st.cfg.os;
    let now = match st.k.ns.lookup(store, RelPath::literal("LOCK")) {
        Ok(n) if !st.k.ns.node(n).is_dir() => n,
        _ => return Err(LockError::IdentityMismatch),
    };
    if st.k.ns.node(now).delete_pending.is_some() {
        return Err(lock_io(os, VfsErrorKind::DeletePending, "NtCreateFile"));
    }
    crate::vfs::sharing_check(st, proc, now).map_err(|k| lock_io(os, k, "NtCreateFile"))?;
    if now != node {
        st.ev(EventKind::LockIdentity, None, proc, node, now, 0);
        return Err(LockError::IdentityMismatch);
    }
    let rec = st
        .tables
        .get_mut(&(proc, node))
        .expect("simulator: the table exists");
    match h {
        LockHandle::Role(offset) => rec.roles_open |= role_bit(offset),
        LockHandle::Probe => rec.probe_open = true,
        LockHandle::Slot | LockHandle::Waiter => {}
    }
    Ok(())
}

fn role_bit(offset: u64) -> u8 {
    1 << (offset - moirai_vfs::ROLE_BASE).min(7)
}

/// The handle a kernel try of `byte` uses.
fn try_handle(byte: LockByte) -> LockHandle {
    match byte {
        LockByte::Slot(_) => LockHandle::Slot,
        b => LockHandle::Role(b.offset()),
    }
}

/// A kernel try through the handle `byte` needs: the handle's open (identity-checked) and then the try.
fn checked_try(
    st: &mut State,
    proc: u32,
    node: u64,
    byte: LockByte,
) -> Result<KernelResult, LockError> {
    open_lock_handle(st, proc, node, try_handle(byte))?;
    Ok(if kernel_try(st, proc, node, byte.offset()) {
        KernelResult::Granted
    } else {
        KernelResult::Busy
    })
}

/// A non-blocking kernel acquisition by `proc`: granted iff nobody holds the byte.
pub(crate) fn kernel_try(st: &mut State, proc: u32, node: u64, byte: u64) -> bool {
    let granted = !st.k.locks.held.contains_key(&(node, byte));
    if granted {
        st.k.locks.held.insert((node, byte), Holder::Live { proc });
    }
    st.ev(
        EventKind::LockTry,
        None,
        proc,
        node,
        byte,
        u64::from(granted),
    );
    granted
}

/// A kernel unlock by `proc` (errors, such as an unlock of a byte a try never obtained, are ignored).
pub(crate) fn kernel_unlock(st: &mut State, proc: u32, node: u64, byte: u64) {
    if st.k.locks.held.get(&(node, byte)) == Some(&Holder::Live { proc }) {
        st.k.locks.held.remove(&(node, byte));
        st.ev(EventKind::LockUnlock, None, proc, node, byte, 0);
        kernel_freed(st, node, byte);
    }
}

impl State {
    pub(crate) fn kernel_freed(&mut self, node: u64, byte: u64) {
        kernel_freed(self, node, byte);
    }
}

/// A byte became free: offer it to one waiting kernel wait (the adversary picks which, or none for now).
pub(crate) fn kernel_freed(st: &mut State, node: u64, byte: u64) {
    if st.k.locks.held.contains_key(&(node, byte)) {
        return;
    }
    let candidates: Vec<usize> =
        st.k.locks
            .waits
            .iter()
            .enumerate()
            .filter(|(_, w)| w.node == node && w.byte == byte && !w.granted)
            .map(|(i, _)| i)
            .collect();
    if candidates.is_empty() {
        return;
    }
    let n = candidates.len() as u64;
    let pick = st.pick(Site::KernelWake, u32::MAX, node, byte, n + 1);
    if pick == n {
        return;
    }
    let i = candidates[pick as usize];
    let w = {
        let w = &mut st.k.locks.waits[i];
        w.granted = true;
        w.clone()
    };
    st.k.locks
        .held
        .insert((node, byte), Holder::Live { proc: w.proc });
    st.ev(EventKind::LockWaitGranted, None, w.proc, node, byte, 0);
    match w.driver {
        KDriver::Caller => st.wake(Block::KernelWait(w.id)),
        KDriver::Thread => waiter_granted(st, w.id),
    }
}

/// Offers every free byte that kernel waits are waiting for (after a spurious non-grant).
pub(crate) fn reoffer(st: &mut State) {
    if st.k.locks.waits.is_empty() {
        return;
    }
    let mut keys: Vec<(u64, u64)> =
        st.k.locks
            .waits
            .iter()
            .filter(|w| !w.granted)
            .map(|w| (w.node, w.byte))
            .collect();
    keys.sort_unstable();
    keys.dedup();
    for (node, byte) in keys {
        kernel_freed(st, node, byte);
    }
}

/// `WaiterThread` mode: the simulated waiter obtained the byte and reports T4 to its table.
fn waiter_granted(st: &mut State, wait: u64) {
    let Some(pos) = st.k.locks.waits.iter().position(|w| w.id == wait) else {
        return;
    };
    let w = st.k.locks.waits.remove(pos);
    let Some(lb) = LockByte::from_offset(w.byte) else {
        return;
    };
    let now = st.k.clock.mono_ns;
    let Some(rec) = st.tables.get_mut(&(w.proc, w.node)) else {
        kernel_unlock(st, w.proc, w.node, w.byte);
        return;
    };
    let gs = rec.table.kernel_granted(lb, now);
    handle_grant(st, w.proc, w.node, w.byte, gs);
}

/// Acts on T4's result.
fn handle_grant(st: &mut State, proc: u32, node: u64, byte: u64, gs: GrantStep) {
    match gs {
        GrantStep::HandTo(_) => st.wake(Block::Table(proc, node)),
        GrantStep::ReleaseNow { then, .. } => {
            kernel_unlock(st, proc, node, byte);
            if let Some(s) = then {
                perform(st, proc, node, s);
            }
        }
    }
}

/// Performs a grant-table step for process `proc` on `LOCK` node `node`.
pub(crate) fn perform(st: &mut State, proc: u32, node: u64, step: Step) {
    match step {
        Step::KernelUnlock { byte, .. } => kernel_unlock(st, proc, node, byte.offset()),
        Step::CancelKernelWait { byte } => settle_caller_wait(st, proc, node, byte),
        Step::StartWait { .. } | Step::NewDriver { .. } => st.wake(Block::Table(proc, node)),
    }
}

/// Cancels and settles a caller-driven kernel wait: T4 if it obtained the byte meanwhile, else T6.
fn settle_caller_wait(st: &mut State, proc: u32, node: u64, lb: LockByte) {
    let byte = lb.offset();
    let pos = st.k.locks.waits.iter().position(|w| {
        w.proc == proc && w.node == node && w.byte == byte && w.driver == KDriver::Caller
    });
    let granted = match pos {
        Some(p) => st.k.locks.waits.remove(p).granted,
        None => false,
    };
    let now = st.k.clock.mono_ns;
    let Some(rec) = st.tables.get_mut(&(proc, node)) else {
        if granted {
            kernel_unlock(st, proc, node, byte);
        }
        return;
    };
    if granted {
        let gs = rec.table.kernel_granted(lb, now);
        handle_grant(st, proc, node, byte, gs);
    } else {
        let cs = rec.table.kernel_cancelled(lb);
        st.ev(EventKind::LockWaitCancelled, None, proc, node, byte, 0);
        if let CancelStep::NewDriver(_) = cs {
            st.wake(Block::Table(proc, node));
        }
    }
}

/// A kernel probe through the table's probe handle: `Held` for a live or zombie holder, else `Free`; the adversary may
/// answer `Unknown` (never `Free` for a held byte), and a probe handle that cannot be opened, or names another file,
/// answers `Unknown` ([OS/lock §8] "Errors").
fn kernel_probe(st: &mut State, proc: u32, node: u64, byte: u64) -> ProbeResult {
    if open_lock_handle(st, proc, node, LockHandle::Probe).is_err() {
        return ProbeResult::Unknown;
    }
    let held = st.k.locks.held.contains_key(&(node, byte));
    if st.pick(Site::ProbeUnknown, proc, node, byte, 2) == 1 {
        return ProbeResult::Unknown;
    }
    if held {
        ProbeResult::Held
    } else {
        ProbeResult::Free
    }
}

fn table(st: &mut State, proc: u32, node: u64) -> &mut GrantTable {
    &mut st
        .tables
        .get_mut(&(proc, node))
        .expect("simulator: a live client's grant table exists")
        .table
}

// ---- the Locks sub-trait ----

/// A lock client of one simulated process ([80 §2.1] `LockFile`). Dropping it releases every grant it holds and leaves
/// every queue ([OS/lock §5.3] T0).
pub struct SimClient {
    pub(crate) sh: Arc<Shared>,
    pub(crate) proc: u32,
    pub(crate) node: u64,
    pub(crate) id: ClientId,
    pub(crate) data: SimFile,
}

impl core::fmt::Debug for SimClient {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SimClient")
            .field("proc", &self.proc)
            .field("lock_node", &self.node)
            .field("client", &self.id)
            .finish()
    }
}

impl Drop for SimClient {
    fn drop(&mut self) {
        let mut g = self.sh.lock();
        if !g.alive(self.proc) {
            return;
        }
        let key = (self.proc, self.node);
        let Some(rec) = g.tables.get_mut(&key) else {
            return;
        };
        let steps = rec.table.unregister(self.id);
        let empty = rec.table.is_empty();
        for s in steps {
            perform(&mut g, self.proc, self.node, s);
        }
        if empty {
            g.tables.remove(&key);
        }
    }
}

fn lock_io(os: moirai_vfs::OsTag, kind: VfsErrorKind, call: &'static str) -> LockError {
    LockError::Io(VfsError::new(kind, os_code(os, kind), call))
}

/// The trace code of a lock call's result (the `c` of its `Return` event).
fn lock_code(e: &LockError) -> u64 {
    match e {
        LockError::NoLockFile => error_code(VfsErrorKind::NotFound),
        LockError::IdentityMismatch => error_code(VfsErrorKind::Stale),
        LockError::AccessDenied { .. } => error_code(VfsErrorKind::AccessDenied),
        LockError::Unsupported { .. } => error_code(VfsErrorKind::Unsupported),
        LockError::Io(v) => error_code(v.kind),
    }
}

fn ret<T>(ctx: &mut Ctx<'_>, call: CallKind, node: u64, r: &Result<T, LockError>) {
    let code = r.as_ref().map_or_else(lock_code, |_| 0);
    ctx.ret_code(call, node, code);
}

pub(crate) fn lock_client(
    sh: &Arc<Shared>,
    proc: u32,
    store: &SimRoot,
    mode: LockMode,
) -> Result<SimClient, LockError> {
    let mut ctx = Ctx::enter(sh, proc, CallKind::LockClient, store.node);
    let r = lock_client_in(&mut ctx, sh, proc, store, mode);
    let node = r.as_ref().map_or(store.node, |c| c.node);
    ret(&mut ctx, CallKind::LockClient, node, &r);
    r
}

fn lock_client_in(
    ctx: &mut Ctx<'_>,
    sh: &Arc<Shared>,
    proc: u32,
    store: &SimRoot,
    mode: LockMode,
) -> Result<SimClient, LockError> {
    let os = ctx.st().cfg.os;
    let lock = RelPath::literal("LOCK");
    for attempt in 0..2 {
        let st = ctx.st();
        let node = match st.k.ns.lookup(store.node, lock) {
            Ok(n) if !st.k.ns.node(n).is_dir() => n,
            _ => return Err(LockError::NoLockFile),
        };
        if mode == LockMode::Acquire && store.access == moirai_vfs::RootAccess::Read {
            return Err(LockError::AccessDenied {
                os: os_code(os, VfsErrorKind::AccessDenied),
            });
        }
        if st.k.ns.node(node).delete_pending.is_some() {
            return Err(lock_io(os, VfsErrorKind::DeletePending, "NtCreateFile"));
        }
        if let Err(kind) = crate::vfs::sharing_check(st, proc, node) {
            return Err(lock_io(os, kind, "NtCreateFile"));
        }
        let access = if mode == LockMode::Acquire {
            Access::ReadWrite
        } else {
            Access::Read
        };
        let h = st.open_handle(node, proc, false);
        st.k.procs[proc as usize].counters.opens += 1;
        // The identity check compares the handle with a fresh query of the path ([OS/lock §9.1]); an external actor may
        // replace `LOCK` in between (FM-10.3).
        ctx.point(CallKind::LockClient, node, 1);
        let st = ctx.st();
        let fresh = st.k.ns.lookup(store.node, lock).ok();
        if fresh != Some(node) {
            st.close_handle(h);
            if attempt == 1 {
                return Err(LockError::IdentityMismatch);
            }
            continue;
        }
        let key = (proc, node);
        if !st.tables.contains_key(&key) {
            let id = TableId::new(st.next_table);
            st.next_table += 1;
            let mode = st.wait_mode;
            st.tables.insert(
                key,
                TableRec {
                    table: GrantTable::new(mode, id),
                    store: store.node,
                    roles_open: 0,
                    probe_open: false,
                },
            );
        }
        let id = table(st, proc, node).register(mode);
        let data = SimFile {
            sh: Arc::clone(sh),
            handle: h,
            node,
            proc,
            access,
        };
        return Ok(SimClient {
            sh: Arc::clone(sh),
            proc,
            node,
            id,
            data,
        });
    }
    Err(LockError::IdentityMismatch)
}

fn check_client(client: &SimClient, proc: u32) {
    assert_eq!(
        client.proc, proc,
        "simulator: a lock client used through the Vfs of another process"
    );
}

fn granted(st: &mut State, proc: u32, node: u64, g: Grant) -> Result<Acquired, LockError> {
    st.ev(
        EventKind::Granted,
        None,
        proc,
        node,
        g.byte().offset(),
        g.client().get(),
    );
    Ok(Acquired::Granted(g))
}

fn outcome(st: &mut State, proc: u32, node: u64, o: Outcome) -> Result<Acquired, LockError> {
    match o {
        Outcome::Granted(g) => granted(st, proc, node, g),
        Outcome::Busy => Ok(Acquired::Busy),
        Outcome::Error(e) => Err(e),
    }
}

pub(crate) fn try_acquire(
    sh: &Arc<Shared>,
    proc: u32,
    client: &mut SimClient,
    byte: LockByte,
) -> Result<Acquired, LockError> {
    check_client(client, proc);
    let node = client.node;
    let mut ctx = Ctx::enter(sh, proc, CallKind::TryAcquire, node);
    let st = ctx.st();
    let r = match table(st, proc, node).begin_try(client.id, byte) {
        TryStep::Busy => Ok(Acquired::Busy),
        TryStep::KernelTry { .. } => try_in_kernel(st, proc, node, client.id, byte),
    };
    ret(&mut ctx, CallKind::TryAcquire, node, &r);
    r
}

/// T1's `KernelTry`, then T2.
fn try_in_kernel(
    st: &mut State,
    proc: u32,
    node: u64,
    c: ClientId,
    byte: LockByte,
) -> Result<Acquired, LockError> {
    let r = checked_try(st, proc, node, byte);
    let (o, then) = table(st, proc, node).end_try(c, byte, r);
    if let Some(s) = then {
        perform(st, proc, node, s);
    }
    outcome(st, proc, node, o)
}

/// T5 and what follows it: the final answer of a wait whose deadline passed.
fn after_deadline(
    st: &mut State,
    proc: u32,
    node: u64,
    c: ClientId,
    byte: LockByte,
) -> Result<Acquired, LockError> {
    match table(st, proc, node).deadline_passed(c, byte) {
        DeadlineStep::AlreadyGranted(g) => granted(st, proc, node, g),
        DeadlineStep::CancelKernelWait => {
            settle_caller_wait(st, proc, node, byte);
            Ok(Acquired::Busy)
        }
        DeadlineStep::Busy { then } => {
            if let Some(s) = then {
                perform(st, proc, node, s);
            }
            Ok(Acquired::Busy)
        }
        DeadlineStep::Error(e) => Err(e),
    }
}

/// Registers a kernel wait and offers the byte at once if it is free.
fn start_kernel_wait(st: &mut State, proc: u32, node: u64, byte: u64, driver: KDriver) -> u64 {
    let id = st.k.locks.next_wait;
    st.k.locks.next_wait += 1;
    st.k.locks.waits.push(KWait {
        id,
        node,
        byte,
        proc,
        driver,
        granted: false,
    });
    st.ev(
        EventKind::LockWait,
        None,
        proc,
        node,
        byte,
        u64::from(driver == KDriver::Thread),
    );
    kernel_freed(st, node, byte);
    id
}

pub(crate) fn acquire_within(
    sh: &Arc<Shared>,
    proc: u32,
    client: &mut SimClient,
    byte: LockByte,
    within_ms: u32,
) -> Result<Acquired, LockError> {
    check_client(client, proc);
    let node = client.node;
    let mut ctx = Ctx::enter(sh, proc, CallKind::AcquireWithin, node);
    let r = acquire_within_in(&mut ctx, proc, node, client.id, byte, within_ms);
    ret(&mut ctx, CallKind::AcquireWithin, node, &r);
    r
}

fn acquire_within_in(
    ctx: &mut Ctx<'_>,
    proc: u32,
    node: u64,
    c: ClientId,
    byte: LockByte,
    within_ms: u32,
) -> Result<Acquired, LockError> {
    let st = ctx.st();
    let now = st.k.clock.mono_ns;
    let deadline = now.saturating_add(u64::from(within_ms) * 1_000_000);
    let mut step = table(st, proc, node).begin_wait(c, byte, deadline, now);
    loop {
        let st = ctx.st();
        match step {
            WaitStep::Try(TryStep::Busy) => return Ok(Acquired::Busy),
            WaitStep::Try(TryStep::KernelTry { .. }) => {
                return try_in_kernel(st, proc, node, c, byte);
            }
            WaitStep::KernelTry { .. } => {
                let r = checked_try(st, proc, node, byte);
                let now = st.k.clock.mono_ns;
                step = table(st, proc, node).end_wait_try(c, byte, r, deadline, now);
            }
            WaitStep::Granted(g) => return granted(st, proc, node, g),
            WaitStep::Busy { then } => {
                if let Some(s) = then {
                    perform(st, proc, node, s);
                }
                return Ok(Acquired::Busy);
            }
            WaitStep::Error { error, then } => {
                if let Some(s) = then {
                    perform(st, proc, node, s);
                }
                return Err(error);
            }
            WaitStep::WaitInTable => {
                let until = st.boot_at_mono(deadline);
                ctx.block(Block::Table(proc, node), Some(until));
                step = match wake_step(ctx.st(), proc, node, c, byte, deadline) {
                    Ok(s) => s,
                    Err(done) => return done,
                };
            }
            WaitStep::StartWaiterThread => {
                // The waiter opens its own OFD on `LOCK`; an open that fails is the thread's failure ([OS/lock §7.2]).
                match open_lock_handle(st, proc, node, LockHandle::Waiter) {
                    Ok(()) => {
                        start_kernel_wait(st, proc, node, byte.offset(), KDriver::Thread);
                    }
                    Err(e) => {
                        let fs = table(st, proc, node).kernel_failed(byte, e);
                        if let Some(s) = fs.then {
                            perform(st, proc, node, s);
                        }
                        if fs.failed.is_some_and(|f| f != c) {
                            st.wake(Block::Table(proc, node));
                        }
                    }
                }
                step = match wake_step(st, proc, node, c, byte, deadline) {
                    Ok(s) => s,
                    Err(done) => return done,
                };
            }
            WaitStep::DriveKernelWait => {
                let id = start_kernel_wait(st, proc, node, byte.offset(), KDriver::Caller);
                loop {
                    let st = ctx.st();
                    let granted_now = st.k.locks.waits.iter().any(|w| w.id == id && w.granted);
                    if granted_now {
                        let pos =
                            st.k.locks
                                .waits
                                .iter()
                                .position(|w| w.id == id)
                                .expect("simulator: a granted wait is registered");
                        st.k.locks.waits.remove(pos);
                        let now = st.k.clock.mono_ns;
                        match table(st, proc, node).kernel_granted(byte, now) {
                            GrantStep::HandTo(o) => {
                                st.wake(Block::Table(proc, node));
                                step = if o == c {
                                    table(st, proc, node).take_notice(c, byte)
                                } else {
                                    match wake_step(st, proc, node, c, byte, deadline) {
                                        Ok(s) => s,
                                        Err(done) => return done,
                                    }
                                };
                            }
                            GrantStep::ReleaseNow { then, .. } => {
                                kernel_unlock(st, proc, node, byte.offset());
                                if let Some(s) = then {
                                    perform(st, proc, node, s);
                                }
                                step = match wake_step(st, proc, node, c, byte, deadline) {
                                    Ok(s) => s,
                                    Err(done) => return done,
                                };
                            }
                        }
                        break;
                    }
                    if st.k.clock.mono_ns >= deadline {
                        return after_deadline(st, proc, node, c, byte);
                    }
                    let until = st.boot_at_mono(deadline);
                    ctx.block(Block::KernelWait(id), Some(until));
                }
            }
        }
    }
}

/// What a client blocked in the table does when it wakes: T5 if its deadline has passed (the final answer, as `Err`),
/// else `take_notice`.
fn wake_step(
    st: &mut State,
    proc: u32,
    node: u64,
    c: ClientId,
    byte: LockByte,
    deadline: u64,
) -> Result<WaitStep, Result<Acquired, LockError>> {
    if st.k.clock.mono_ns >= deadline {
        return Err(after_deadline(st, proc, node, c, byte));
    }
    Ok(table(st, proc, node).take_notice(c, byte))
}

pub(crate) fn release(sh: &Arc<Shared>, proc: u32, client: &mut SimClient, grant: Grant) {
    check_client(client, proc);
    let node = client.node;
    let mut ctx = Ctx::enter(sh, proc, CallKind::Release, node);
    let st = ctx.st();
    let byte = grant.byte().offset();
    let rs = table(st, proc, node).release(client.id, grant);
    st.ev(EventKind::Released, None, proc, node, byte, client.id.get());
    kernel_unlock(st, proc, node, byte);
    if let Some(s) = rs.then {
        perform(st, proc, node, s);
    }
    ctx.ret_code(CallKind::Release, node, 0);
}

/// The `Return` code of a probe: 0 `Free`, 1 `Held`, 2 `Unknown`.
fn probe_code(p: ProbeResult) -> u64 {
    match p {
        ProbeResult::Free => 0,
        ProbeResult::Held => 1,
        ProbeResult::Unknown => 2,
    }
}

pub(crate) fn probe(
    sh: &Arc<Shared>,
    proc: u32,
    client: &SimClient,
    byte: LockByte,
) -> ProbeResult {
    check_client(client, proc);
    let node = client.node;
    let mut ctx = Ctx::enter(sh, proc, CallKind::Probe, node);
    let st = ctx.st();
    let p = match table(st, proc, node).probe_step(byte) {
        ProbeStep::Held => ProbeResult::Held,
        ProbeStep::KernelProbe => kernel_probe(st, proc, node, byte.offset()),
    };
    ctx.ret_code(CallKind::Probe, node, probe_code(p));
    p
}

pub(crate) fn holds(sh: &Arc<Shared>, proc: u32, client: &SimClient, byte: LockByte) -> bool {
    check_client(client, proc);
    let mut ctx = Ctx::quiet(sh, proc);
    let st = ctx.st();
    st.tables
        .get(&(proc, client.node))
        .is_some_and(|r| r.table.holds(client.id, byte))
}

pub(crate) fn holds_any_role(st: &State, proc: u32) -> bool {
    st.tables
        .iter()
        .any(|(&(p, _), r)| p == proc && r.table.holds_any_role())
}

pub(crate) fn holds_writer_or_flush(st: &State, proc: u32) -> bool {
    st.tables.iter().any(|(&(p, _), r)| {
        p == proc
            && (r.table.holder(LockByte::Writer).is_some()
                || r.table.holder(LockByte::Flush).is_some())
    })
}

pub(crate) fn foreign_lock_check(sh: &Arc<Shared>, proc: u32, client: &SimClient) -> ProbeResult {
    check_client(client, proc);
    let node = client.node;
    let mut ctx = Ctx::enter(sh, proc, CallKind::ForeignCheck, node);
    let p = kernel_probe(ctx.st(), proc, node, FOREIGN_CHECK_BYTE);
    ctx.ret_code(CallKind::ForeignCheck, node, probe_code(p));
    p
}

/// The `init` probe's lock step ([OS/env §5] step 4): try the byte 2^62 on a probe file and release it. The simulated
/// kernel always supports byte-range locks, so the probe never refuses.
pub(crate) fn probe_file_lock(st: &mut State, proc: u32, node: u64) {
    let byte = moirai_vfs::ROLE_BASE;
    if kernel_try(st, proc, node, byte) {
        kernel_unlock(st, proc, node, byte);
    }
}
