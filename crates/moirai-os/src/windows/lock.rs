//! `os::lock` on Windows: `LockBytes` over `LOCK` ([OS/lock]; X-F4 contract items 1–10).
//!
//! - **One table per `LOCK` per process** ([OS/lock §5.1]): a process-global registry maps the `FileIdentity` of a
//!   `LOCK` to one [`GrantTable`] (mode [`WaitMode::CallerDriven`]) with its mutex, condvar and kernel handles. Every
//!   later client of the same `LOCK` in the process joins it, whichever root value it came through; the entry's handles
//!   are closed when its last client is dropped.
//! - **User space decides first** (item 2): every acquisition goes through the table; the kernel sees at most one
//!   acquisition per byte per process, on one overlapped handle per role byte (opened lazily, reused across grants) or
//!   on a new handle per slot grant ([OS/lock §9.2]).
//! - **Kernel steps** ([OS/lock §7.1]): a try is `LockFileEx(EXCLUSIVE | FAIL_IMMEDIATELY)` at the byte (a pending
//!   completion is settled with `GetOverlappedResult(TRUE)`); a bounded wait is an overlapped `LockFileEx(EXCLUSIVE)`
//!   that its client's own thread drives with `WaitForSingleObject(event, remaining ms)` and cancels at its deadline with
//!   `CancelIoEx` + `GetOverlappedResult(TRUE)` (a grant that races the cancel is reported, never leaked); an unlock is
//!   `UnlockFile`, performed before the table's mutex is released.
//! - **Probes** ([OS/lock §8]) go through one dedicated probe handle per table (read-write, falling back to read-only):
//!   try + `UnlockFile` → `Free`, error 33 → `Held`, anything else → `Unknown`.
//! - **Identity** ([OS/lock §9]): the client's data handle is compared with a fresh query of the path, once retried;
//!   every later handle the table opens on `LOCK` is checked against the data handle's identity.
//!
//! Every handle is non-inheritable (item 7) and no lock covers a byte of `LOCK`'s data (item 1: every byte is at
//! 2^62 or above).

#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::os::windows::io::OwnedHandle;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use moirai_vfs::{
    Access, Acquired, CancelStep, ClientId, DeadlineStep, FOREIGN_CHECK_BYTE, FileIdentity, Grant,
    GrantStep, GrantTable, KernelHandle, KernelResult, LockByte, LockError, LockMode, Locks,
    OpenHint, Outcome, Owner, ProbeResult, ProbeStep, ROLE_BASE, RelPath, Step, StoreFs, TableId,
    TryStep, VfsError, VfsErrorKind, WaitMode, WaitStep,
};
use windows_sys::Wdk::Storage::FileSystem::{FILE_NON_DIRECTORY_FILE, FILE_OPEN};
use windows_sys::Win32::Foundation::{
    ERROR_IO_PENDING, ERROR_LOCK_VIOLATION, ERROR_OPERATION_ABORTED, HANDLE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    LockFileEx, UnlockFile,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, GetOverlappedResult, OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0,
};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};

use super::fs::{OsFile, OsRoot, OsVfs};
use super::proc::mono_ns;
use super::sys::{self, Domain, NtOpen, id_info, last_error, nt_open, owned, raw};

/// The name of the lock file in the store root ([F02]).
const LOCK: RelPath<'static> = RelPath::literal("LOCK");

// ---------------------------------------------------------------------------------------------------------------------
// Kernel calls

/// An `OVERLAPPED` at a stable heap address, reused by the one kernel request in flight on its role byte.
struct Ov(Box<UnsafeCell<OVERLAPPED>>);

// SAFETY: the `OVERLAPPED` has no thread affinity; it lives on the heap at a fixed address, and the grant table's
// single-flight rule (I-L2) guarantees that at most one thread issues, waits for or cancels a request through it at a
// time. Its raw `hEvent` pointer is an event handle owned by the same `RoleKernel`.
unsafe impl Send for Ov {}

impl Ov {
    fn new() -> Ov {
        Ov(Box::new(UnsafeCell::new(OVERLAPPED::default())))
    }

    fn ptr(&self) -> *mut OVERLAPPED {
        self.0.get()
    }
}

/// The kernel side of one role byte in one table: the overlapped role handle, its manual-reset event and the request
/// block ([OS/lock §7.1], §9.2).
struct RoleKernel {
    handle: OwnedHandle,
    event: OwnedHandle,
    ov: Ov,
}

fn low_high(b: u64) -> (u32, u32) {
    (b as u32, (b >> 32) as u32)
}

/// Prepares `ov` for a request at byte offset `b` with `event` (null: the file handle signals).
///
/// # Safety
///
/// `ov` must point to a live `OVERLAPPED` that no pending request uses.
unsafe fn arm(ov: *mut OVERLAPPED, event: HANDLE, b: u64) {
    let (lo, hi) = low_high(b);
    let v = OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 {
            Anonymous: OVERLAPPED_0_0 {
                Offset: lo,
                OffsetHigh: hi,
            },
        },
        hEvent: event,
    };
    // SAFETY: the caller guarantees `ov` is live and unused by any pending request.
    unsafe { ov.write(v) };
    if !event.is_null() {
        // SAFETY: `event` is a valid event handle owned by the caller's `RoleKernel`.
        unsafe { ResetEvent(event) };
    }
}

/// `LockFileEx(EXCLUSIVE | FAIL_IMMEDIATELY)` of the byte at `b` through the overlapped handle `h`: `Ok(true)` granted,
/// `Ok(false)` busy (error 33); a pending completion (997) is settled with `GetOverlappedResult(TRUE)` ([OS/lock §7.1],
/// open point 9).
///
/// # Safety
///
/// `ov` must point to a live `OVERLAPPED` that stays valid until this call returns and that no other request uses.
unsafe fn lock_try(h: HANDLE, ov: *mut OVERLAPPED, event: HANDLE, b: u64) -> Result<bool, u32> {
    // SAFETY: forwarded from the caller.
    unsafe { arm(ov, event, b) };
    // SAFETY: `h` is a valid overlapped handle; `ov` is live and armed; one byte at `b`.
    let ok = unsafe {
        LockFileEx(
            h,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            ov,
        )
    };
    if ok != 0 {
        return Ok(true);
    }
    match last_error() {
        ERROR_LOCK_VIOLATION => Ok(false),
        ERROR_IO_PENDING => {
            let mut n = 0u32;
            // SAFETY: waits for the pending request on `ov`, which stays live for the wait.
            if unsafe { GetOverlappedResult(h, ov, &mut n, 1) } != 0 {
                Ok(true)
            } else {
                match last_error() {
                    ERROR_LOCK_VIOLATION => Ok(false),
                    e => Err(e),
                }
            }
        }
        e => Err(e),
    }
}

/// `UnlockFile(h, low(b), high(b), 1, 0)`; its error is ignored (after an interrupted try the byte may not be held
/// through `h`).
fn unlock(h: HANDLE, b: u64) {
    let (lo, hi) = low_high(b);
    // SAFETY: `h` is a valid handle; unlocking a range the handle does not hold only fails.
    unsafe { UnlockFile(h, lo, hi, 1, 0) };
}

/// The lock error of a kernel code ([OS/lock §4]): 50 and 1 are `Unsupported`, everything else `Io`.
fn lock_error(code: u32, call: &'static str) -> LockError {
    match code {
        1 | 50 => LockError::Unsupported {
            os: moirai_vfs::OsCode(code as i32),
        },
        _ => LockError::Io(sys::error(code, Domain::Store, call)),
    }
}

/// Opens `LOCK` in `root` for byte locks: `NtCreateFile` relative to the root, `FILE_GENERIC_READ` (and
/// `FILE_GENERIC_WRITE` when `write`), full sharing, no synchronous-I/O option (overlapped) ([OS/lock] Appendix A).
fn open_lock_file(root: &OsRoot, name: RelPath<'_>, write: bool) -> Result<OwnedHandle, VfsError> {
    let w = sys::rel_wide(name, sys::NameCheck::Store, "NtCreateFile")?;
    let o = NtOpen {
        access: FILE_GENERIC_READ | if write { FILE_GENERIC_WRITE } else { 0 },
        disposition: FILE_OPEN,
        options: FILE_NON_DIRECTORY_FILE,
        attributes: 0,
    };
    nt_open(root.raw(), &w, o, Domain::Store)
}

/// Opens a `LOCK` handle of a table and checks it against the data handle's identity ([OS/lock §9.1] step 4).
fn open_checked(e: &Entry, write: bool) -> Result<OwnedHandle, LockError> {
    let h = open_lock_file(&e.root, LOCK, write).map_err(|v| match v.kind {
        VfsErrorKind::NotFound => LockError::IdentityMismatch,
        VfsErrorKind::AccessDenied => LockError::AccessDenied { os: v.os },
        _ => LockError::Io(v),
    })?;
    let info = id_info(raw(&h)).map_err(|c| lock_error(c, "GetFileInformationByHandleEx"))?;
    let id = FileIdentity {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    };
    if id != e.identity {
        return Err(LockError::IdentityMismatch);
    }
    Ok(h)
}

// ---------------------------------------------------------------------------------------------------------------------
// The registry ([OS/lock §5.1])

/// The number of role-byte offsets above [`ROLE_BASE`] ([OS/lock §2], pass 1, P1-10): `Writer` +0, `Leader` +1,
/// `Maintenance` +2, `Quiet(0)` +3, `Flush` +4, `Quiet(1)`–`Quiet(8)` +5 … +12.
const ROLE_OFFSETS: usize = 13;

/// The state of one table under its mutex.
struct State {
    table: GrantTable,
    /// One kernel side per role **byte**, indexed by [`role_index`] (offset − `ROLE_BASE`). The grant table's
    /// single-flight rule is per byte, so two bytes never share a request block: two quiet bytes tried at once by two
    /// threads each arm their own `OVERLAPPED`.
    roles: [Option<RoleKernel>; ROLE_OFFSETS],
    /// The slot handles of held slots (`Owner::SlotHandle`).
    slots: Vec<(ClientId, LockByte, OwnedHandle)>,
}

/// One registry entry: one `LOCK` identity in this process.
struct Entry {
    identity: FileIdentity,
    /// The root the table opens its handles relative to (the first client's store root).
    root: OsRoot,
    state: Mutex<State>,
    cond: Condvar,
    /// The probe handle ([OS/lock §8]), serialising the process's kernel probes of this `LOCK`.
    probe: Mutex<Option<OwnedHandle>>,
}

static REGISTRY: Mutex<Vec<Arc<Entry>>> = Mutex::new(Vec::new());
static NEXT_TABLE: AtomicU64 = AtomicU64::new(1);

fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The role-kernel index of a role byte: its offset − `ROLE_BASE` ([OS/lock §2]), so every role byte, each quiet byte
/// included, has its own handle, event and request block.
fn role_index(b: LockByte) -> usize {
    if let LockByte::Slot(_) = b {
        panic!("os::lock: slots have no role handle");
    }
    let i = b.offset() - ROLE_BASE;
    assert!(
        i < ROLE_OFFSETS as u64,
        "os::lock: role byte at ROLE_BASE + {i} is outside the role range"
    );
    i as usize
}

/// Process-wide: does any client of any table hold a role byte ([OS/lock §3] item 7)?
pub(crate) fn holds_any_role() -> bool {
    let reg = guard(&REGISTRY);
    reg.iter().any(|e| guard(&e.state).table.holds_any_role())
}

/// A lock client ([OS/lock §4]; [80 §2.1] `LockFile`): its data handle on `LOCK` and its id in the process's table
/// for that `LOCK`. Dropping it releases every grant it holds (kernel unlock) and leaves every queue.
pub struct OsLockClient {
    entry: Arc<Entry>,
    id: ClientId,
    data: OsFile,
}

impl core::fmt::Debug for OsLockClient {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OsLockClient")
            .field("client", &self.id)
            .field("table", &guard(&self.entry.state).table.id())
            .finish()
    }
}

impl Drop for OsLockClient {
    fn drop(&mut self) {
        let e = Arc::clone(&self.entry);
        let mut g = guard(&e.state);
        let steps = g.table.unregister(self.id);
        let mut cancels = Vec::new();
        for s in steps {
            match s {
                Step::CancelKernelWait { byte } => cancels.push(byte),
                s => perform(&e, &mut g, s),
            }
        }
        drop(g);
        for b in cancels {
            cancel_and_settle(&e, b);
        }
        let mut reg = guard(&REGISTRY);
        if guard(&e.state).table.is_empty() {
            reg.retain(|x| !Arc::ptr_eq(x, &e));
        }
    }
}

/// Performs a step that is not a cancel: a kernel unlock before the mutex is released, or a wake-up of the waiters.
fn perform(e: &Entry, g: &mut State, s: Step) {
    match s {
        Step::KernelUnlock { byte, owner } => unlock_owned(g, byte, owner),
        Step::StartWait { .. } | Step::NewDriver { .. } => e.cond.notify_all(),
        Step::CancelKernelWait { .. } => {
            unreachable!("os::lock: CancelKernelWait comes from unregister only")
        }
    }
}

fn perform_opt(e: &Entry, g: &mut State, s: Option<Step>) {
    if let Some(s) = s {
        perform(e, g, s);
    }
}

/// `UnlockFile` through the handle that owns `byte`; a slot handle is then closed.
fn unlock_owned(g: &mut State, byte: LockByte, owner: Owner) {
    match owner {
        Owner::RoleHandle => {
            if let Some(r) = &g.roles[role_index(byte)] {
                unlock(raw(&r.handle), byte.offset());
            }
        }
        Owner::SlotHandle(c) => {
            if let Some(pos) = g.slots.iter().position(|(o, b, _)| *o == c && *b == byte) {
                let (_, _, h) = g.slots.remove(pos);
                unlock(raw(&h), byte.offset());
            }
        }
        Owner::WaiterHandle => {
            unreachable!("os::lock: Windows has no waiter thread (mode CallerDriven)")
        }
    }
}

/// The role handle, event and request block of `byte`, opened on first use.
fn role_kernel(
    e: &Entry,
    g: &mut State,
    byte: LockByte,
) -> Result<(HANDLE, HANDLE, *mut OVERLAPPED), LockError> {
    let i = role_index(byte);
    if g.roles[i].is_none() {
        let handle = open_checked(e, true)?;
        // SAFETY: no security attributes, manual reset, initially unsignalled, unnamed.
        let ev = unsafe { CreateEventW(core::ptr::null(), 1, 0, core::ptr::null()) };
        let event = owned(ev).ok_or_else(|| lock_error(last_error(), "CreateEventW"))?;
        g.roles[i] = Some(RoleKernel {
            handle,
            event,
            ov: Ov::new(),
        });
    }
    let r = g.roles[i]
        .as_ref()
        .expect("os::lock: the role handle was just opened");
    Ok((raw(&r.handle), raw(&r.event), r.ov.ptr()))
}

/// A kernel try of a role byte through its role handle, outside the mutex (the table's `Trying` state makes this
/// thread the only user of the role's request block).
fn role_try<'a>(
    e: &'a Entry,
    mut g: MutexGuard<'a, State>,
    byte: LockByte,
) -> (MutexGuard<'a, State>, Result<KernelResult, LockError>) {
    let (h, ev, ov) = match role_kernel(e, &mut g, byte) {
        Ok(k) => k,
        Err(err) => return (g, Err(err)),
    };
    drop(g);
    // SAFETY: `ov` is the role's request block, alive while the entry lives (this client holds it); single flight.
    let r = unsafe { lock_try(h, ov, ev, byte.offset()) };
    let r = match r {
        Ok(true) => Ok(KernelResult::Granted),
        Ok(false) => Ok(KernelResult::Busy),
        Err(c) => Err(lock_error(c, "LockFileEx")),
    };
    (guard(&e.state), r)
}

/// A kernel try of a slot through a new slot handle ([OS/lock §7.1] row "Slot `KernelTry`"): kept at a grant,
/// closed on busy or error.
fn slot_try(e: &Entry, byte: LockByte) -> (Option<OwnedHandle>, Result<KernelResult, LockError>) {
    let h = match open_checked(e, true) {
        Ok(h) => h,
        Err(err) => return (None, Err(err)),
    };
    let mut ov = OVERLAPPED::default();
    // SAFETY: `ov` is a live local; `lock_try` settles any pending completion before it returns.
    match unsafe { lock_try(raw(&h), &mut ov, core::ptr::null_mut(), byte.offset()) } {
        Ok(true) => (Some(h), Ok(KernelResult::Granted)),
        Ok(false) => (None, Ok(KernelResult::Busy)),
        Err(c) => (None, Err(lock_error(c, "LockFileEx"))),
    }
}

/// T1's kernel try and T2 ([OS/lock §5.3]), for `try_acquire` and a wait whose deadline has already passed.
fn try_in_kernel(
    e: &Entry,
    g: MutexGuard<'_, State>,
    c: ClientId,
    byte: LockByte,
    handle: KernelHandle,
) -> Result<Acquired, LockError> {
    let (mut g, r) = match handle {
        KernelHandle::RoleHandle => role_try(e, g, byte),
        KernelHandle::NewSlotHandle => {
            drop(g);
            let (h, r) = slot_try(e, byte);
            let mut g = guard(&e.state);
            if let Some(h) = h {
                g.slots.push((c, byte, h));
            }
            (g, r)
        }
    };
    let (o, then) = g.table.end_try(c, byte, r);
    perform_opt(e, &mut g, then);
    match o {
        Outcome::Granted(grant) => Ok(Acquired::Granted(grant)),
        Outcome::Busy => Ok(Acquired::Busy),
        Outcome::Error(err) => Err(err),
    }
}

/// What a driven kernel wait ended with.
enum Driven {
    Granted,
    Timeout,
    Failed(LockError),
}

/// The caller-driven kernel wait ([OS/lock §7.1] row `DriveKernelWait`): `ResetEvent`; overlapped
/// `LockFileEx(EXCLUSIVE)`; on 997 `WaitForSingleObject(event, remaining ms of the deadline)`, then
/// `GetOverlappedResult(FALSE)`.
///
/// # Safety
///
/// `ov` must be the role's request block, unused by any other request; on `Timeout` the request is still pending and
/// must be cancelled and settled through the same block.
unsafe fn drive(
    h: HANDLE,
    ev: HANDLE,
    ov: *mut OVERLAPPED,
    byte: LockByte,
    deadline: u64,
) -> Driven {
    // SAFETY: forwarded from the caller.
    unsafe { arm(ov, ev, byte.offset()) };
    // SAFETY: `h` is the overlapped role handle; `ov` is armed with the event.
    if unsafe { LockFileEx(h, LOCKFILE_EXCLUSIVE_LOCK, 0, 1, 0, ov) } != 0 {
        return Driven::Granted;
    }
    match last_error() {
        ERROR_IO_PENDING => {}
        code => return Driven::Failed(lock_error(code, "LockFileEx")),
    }
    loop {
        let now = mono_ns();
        if now >= deadline {
            return Driven::Timeout;
        }
        let ms = (deadline - now)
            .div_ceil(1_000_000)
            .min(u64::from(u32::MAX - 1)) as u32;
        // SAFETY: `ev` is the role's event, signalled when the request completes.
        match unsafe { WaitForSingleObject(ev, ms) } {
            WAIT_OBJECT_0 => {
                let mut n = 0u32;
                // SAFETY: the request on `ov` has completed (its event is signalled); no wait.
                return if unsafe { GetOverlappedResult(h, ov, &mut n, 0) } != 0 {
                    Driven::Granted
                } else {
                    Driven::Failed(lock_error(last_error(), "GetOverlappedResult"))
                };
            }
            WAIT_TIMEOUT => {}
            _ => {
                // `WAIT_FAILED` (or any other result) leaves the request pending on `ov`. It is cancelled and settled
                // before `drive` returns, so the block is free when the table lets the next driver re-arm it, and a
                // grant that completed meanwhile is reported, never leaked through the role handle (I-L6).
                let failed = lock_error(last_error(), "WaitForSingleObject");
                let mut n = 0u32;
                // SAFETY: `ov` is the block of the pending request on `h`; cancelling it and waiting for its completion
                // (`bWait` TRUE) is the settle, after which nothing refers to `ov`.
                let granted = unsafe {
                    CancelIoEx(h, ov);
                    GetOverlappedResult(h, ov, &mut n, 1)
                } != 0;
                return if granted {
                    Driven::Granted
                } else {
                    Driven::Failed(failed)
                };
            }
        }
    }
}

/// T4 after a kernel wait obtained `byte`: hand it to the oldest unexpired waiter, or release it at once.
fn report_granted(e: &Entry, g: &mut State, byte: LockByte) {
    match g.table.kernel_granted(byte, mono_ns()) {
        GrantStep::HandTo(_) => e.cond.notify_all(),
        GrantStep::ReleaseNow { owner, then } => {
            unlock_owned(g, byte, owner);
            perform_opt(e, g, then);
        }
    }
}

/// `CancelKernelWait` ([OS/lock §7.1]): `CancelIoEx` + `GetOverlappedResult(TRUE)`, then T4 if the grant raced the
/// cancel, T6 on 995, `kernel_failed` on any other error.
fn cancel_and_settle(e: &Entry, byte: LockByte) {
    let (h, ov) = {
        let g = guard(&e.state);
        let r = g.roles[role_index(byte)]
            .as_ref()
            .expect("os::lock: a caller-driven wait has a role handle");
        (raw(&r.handle), r.ov.ptr())
    };
    let mut n = 0u32;
    // SAFETY: `ov` is the request block of the pending wait on `h`; cancelling and then waiting for its completion is
    // the settle; the block stays alive (the entry lives through `e`).
    let settled = unsafe {
        CancelIoEx(h, ov);
        GetOverlappedResult(h, ov, &mut n, 1)
    };
    let code = if settled != 0 { 0 } else { last_error() };
    let mut g = guard(&e.state);
    match code {
        0 => report_granted(e, &mut g, byte),
        ERROR_OPERATION_ABORTED => {
            if let CancelStep::NewDriver(_) = g.table.kernel_cancelled(byte) {
                e.cond.notify_all();
            }
        }
        c => {
            let fs = g
                .table
                .kernel_failed(byte, lock_error(c, "GetOverlappedResult"));
            perform_opt(e, &mut g, fs.then);
            e.cond.notify_all();
        }
    }
}

/// T5 and what follows: the final answer of a wait whose deadline passed.
fn after_deadline(
    e: &Entry,
    mut g: MutexGuard<'_, State>,
    c: ClientId,
    byte: LockByte,
) -> Result<Acquired, LockError> {
    match g.table.deadline_passed(c, byte) {
        DeadlineStep::AlreadyGranted(grant) => Ok(Acquired::Granted(grant)),
        DeadlineStep::CancelKernelWait => {
            drop(g);
            cancel_and_settle(e, byte);
            Ok(Acquired::Busy)
        }
        DeadlineStep::Busy { then } => {
            perform_opt(e, &mut g, then);
            Ok(Acquired::Busy)
        }
        DeadlineStep::Error(err) => Err(err),
    }
}

/// The step of a client that wakes in the table: T5 at its deadline (the final answer, as `Err`), else `take_notice`.
fn wake_step<'a>(
    e: &'a Entry,
    mut g: MutexGuard<'a, State>,
    c: ClientId,
    byte: LockByte,
    deadline: u64,
) -> Result<(MutexGuard<'a, State>, WaitStep), Result<Acquired, LockError>> {
    if mono_ns() >= deadline {
        return Err(after_deadline(e, g, c, byte));
    }
    let s = g.table.take_notice(c, byte);
    Ok((g, s))
}

impl OsVfs {
    /// The bounded wait of [OS/lock §4, §7.1], driving the table from T3 to one outcome.
    fn wait_for(
        &self,
        client: &mut OsLockClient,
        byte: LockByte,
        within_ms: u32,
    ) -> Result<Acquired, LockError> {
        let e = &*client.entry;
        let c = client.id;
        let now = mono_ns();
        let deadline = now.saturating_add(u64::from(within_ms) * 1_000_000);
        let mut g = guard(&e.state);
        let mut step = g.table.begin_wait(c, byte, deadline, now);
        loop {
            step = match step {
                WaitStep::Try(TryStep::Busy) => return Ok(Acquired::Busy),
                WaitStep::Try(TryStep::KernelTry { handle }) => {
                    return try_in_kernel(e, g, c, byte, handle);
                }
                WaitStep::KernelTry { .. } => {
                    let (g2, r) = role_try(e, g, byte);
                    g = g2;
                    g.table.end_wait_try(c, byte, r, deadline, mono_ns())
                }
                WaitStep::Granted(grant) => return Ok(Acquired::Granted(grant)),
                WaitStep::Busy { then } => {
                    perform_opt(e, &mut g, then);
                    return Ok(Acquired::Busy);
                }
                WaitStep::Error { error, then } => {
                    perform_opt(e, &mut g, then);
                    return Err(error);
                }
                WaitStep::WaitInTable => {
                    let now = mono_ns();
                    if now >= deadline {
                        return after_deadline(e, g, c, byte);
                    }
                    let (g2, _) = e
                        .cond
                        .wait_timeout(g, Duration::from_nanos(deadline - now))
                        .unwrap_or_else(PoisonError::into_inner);
                    match wake_step(e, g2, c, byte, deadline) {
                        Ok((g3, s)) => {
                            g = g3;
                            s
                        }
                        Err(done) => return done,
                    }
                }
                WaitStep::StartWaiterThread => {
                    unreachable!("os::lock: Windows tables run in mode CallerDriven")
                }
                WaitStep::DriveKernelWait => {
                    let (h, ev, ov) = match role_kernel(e, &mut g, byte) {
                        Ok(k) => k,
                        Err(err) => {
                            let fs = g.table.kernel_failed(byte, err);
                            perform_opt(e, &mut g, fs.then);
                            e.cond.notify_all();
                            match wake_step(e, g, c, byte, deadline) {
                                Ok((g3, s)) => {
                                    g = g3;
                                    step = s;
                                    continue;
                                }
                                Err(done) => return done,
                            }
                        }
                    };
                    drop(g);
                    // SAFETY: `ov` is the role's request block; the table's `Waiting(Caller(c))` makes this thread its
                    // only user until the wait is reported; a timed-out request is cancelled and settled below.
                    let r = unsafe { drive(h, ev, ov, byte, deadline) };
                    g = guard(&e.state);
                    match r {
                        Driven::Granted => match g.table.kernel_granted(byte, mono_ns()) {
                            GrantStep::HandTo(o) => {
                                e.cond.notify_all();
                                if o == c {
                                    g.table.take_notice(c, byte)
                                } else {
                                    match wake_step(e, g, c, byte, deadline) {
                                        Ok((g3, s)) => {
                                            g = g3;
                                            s
                                        }
                                        Err(done) => return done,
                                    }
                                }
                            }
                            GrantStep::ReleaseNow { owner, then } => {
                                unlock_owned(&mut g, byte, owner);
                                perform_opt(e, &mut g, then);
                                match wake_step(e, g, c, byte, deadline) {
                                    Ok((g3, s)) => {
                                        g = g3;
                                        s
                                    }
                                    Err(done) => return done,
                                }
                            }
                        },
                        Driven::Timeout => return after_deadline(e, g, c, byte),
                        Driven::Failed(err) => {
                            let fs = g.table.kernel_failed(byte, err);
                            perform_opt(e, &mut g, fs.then);
                            e.cond.notify_all();
                            match wake_step(e, g, c, byte, deadline) {
                                Ok((g3, s)) => {
                                    g = g3;
                                    s
                                }
                                Err(done) => return done,
                            }
                        }
                    }
                }
            };
        }
    }

    /// A kernel probe of the byte at offset `b` through the table's probe handle ([OS/lock §8]).
    fn kernel_probe(e: &Entry, b: u64) -> ProbeResult {
        let mut p = guard(&e.probe);
        if p.is_none() {
            let h = match open_checked(e, true) {
                Ok(h) => h,
                Err(LockError::AccessDenied { .. }) => match open_checked(e, false) {
                    Ok(h) => h,
                    Err(_) => return ProbeResult::Unknown,
                },
                Err(_) => return ProbeResult::Unknown,
            };
            *p = Some(h);
        }
        let h = raw(p
            .as_ref()
            .expect("os::lock: the probe handle was just opened"));
        let mut ov = OVERLAPPED::default();
        // SAFETY: `ov` is a live local; `lock_try` settles any pending completion before it returns.
        match unsafe { lock_try(h, &mut ov, core::ptr::null_mut(), b) } {
            Ok(true) => {
                unlock(h, b);
                ProbeResult::Free
            }
            Ok(false) => ProbeResult::Held,
            Err(_) => ProbeResult::Unknown,
        }
    }
}

impl Locks for OsVfs {
    type Client = OsLockClient;

    fn lock_client(&self, store: &OsRoot, mode: LockMode) -> Result<OsLockClient, LockError> {
        let access = match mode {
            LockMode::Acquire => Access::ReadWrite,
            LockMode::Probe => Access::Read,
        };
        for attempt in 0..2 {
            let data = match self.open(store, LOCK, access, OpenHint::Normal) {
                Ok(f) => f,
                Err(v) => {
                    return Err(match v.kind {
                        VfsErrorKind::NotFound => LockError::NoLockFile,
                        VfsErrorKind::AccessDenied if mode == LockMode::Acquire => {
                            LockError::AccessDenied { os: v.os }
                        }
                        _ => LockError::Io(v),
                    });
                }
            };
            let id = self.identity(&data).map_err(LockError::Io)?;
            let fresh = match self.path_identity(store, LOCK) {
                Ok(f) => Some(f),
                Err(v) if v.kind == VfsErrorKind::NotFound => None,
                Err(v) => return Err(LockError::Io(v)),
            };
            if fresh != Some(id) {
                drop(data);
                if attempt == 1 {
                    return Err(LockError::IdentityMismatch);
                }
                continue;
            }
            let mut reg = guard(&REGISTRY);
            let entry = match reg.iter().find(|x| x.identity == id) {
                Some(x) => Arc::clone(x),
                None => {
                    let x = Arc::new(Entry {
                        identity: id,
                        root: store.clone(),
                        state: Mutex::new(State {
                            table: GrantTable::new(
                                WaitMode::CallerDriven,
                                TableId::new(NEXT_TABLE.fetch_add(1, Ordering::Relaxed)),
                            ),
                            roles: [const { None }; ROLE_OFFSETS],
                            slots: Vec::new(),
                        }),
                        cond: Condvar::new(),
                        probe: Mutex::new(None),
                    });
                    reg.push(Arc::clone(&x));
                    x
                }
            };
            let cid = guard(&entry.state).table.register(mode);
            drop(reg);
            return Ok(OsLockClient {
                entry,
                id: cid,
                data,
            });
        }
        Err(LockError::IdentityMismatch)
    }

    fn lock_data<'a>(&self, client: &'a OsLockClient) -> &'a OsFile {
        &client.data
    }

    fn try_acquire(
        &self,
        client: &mut OsLockClient,
        byte: LockByte,
    ) -> Result<Acquired, LockError> {
        let e = &*client.entry;
        let c = client.id;
        let mut g = guard(&e.state);
        let step = g.table.begin_try(c, byte);
        match step {
            TryStep::Busy => Ok(Acquired::Busy),
            TryStep::KernelTry { handle } => try_in_kernel(e, g, c, byte, handle),
        }
    }

    fn acquire_within(
        &self,
        client: &mut OsLockClient,
        byte: LockByte,
        within_ms: u32,
    ) -> Result<Acquired, LockError> {
        self.wait_for(client, byte, within_ms)
    }

    fn release(&self, client: &mut OsLockClient, grant: Grant) {
        let e = &*client.entry;
        let mut g = guard(&e.state);
        let rs = g.table.release(client.id, grant);
        unlock_owned(&mut g, rs.byte, rs.owner);
        perform_opt(e, &mut g, rs.then);
    }

    fn probe(&self, client: &OsLockClient, byte: LockByte) -> ProbeResult {
        let e = &*client.entry;
        let step = guard(&e.state).table.probe_step(byte);
        match step {
            ProbeStep::Held => ProbeResult::Held,
            ProbeStep::KernelProbe => OsVfs::kernel_probe(e, byte.offset()),
        }
    }

    fn holds(&self, client: &OsLockClient, byte: LockByte) -> bool {
        guard(&client.entry.state).table.holds(client.id, byte)
    }

    fn holds_any_role(&self) -> bool {
        holds_any_role()
    }

    fn foreign_lock_check(&self, client: &OsLockClient) -> ProbeResult {
        OsVfs::kernel_probe(&client.entry, FOREIGN_CHECK_BYTE)
    }
}

/// The `init` probe's lock step ([OS/env §5] step 4): opens `rel` under `root` as a lock handle ([OS/lock §9.2] flags),
/// tries the byte 2^62, releases it and closes the handle. `Ok(false)` if another process holds the byte ("busy" is not
/// a refusal); `Err` for an open or kernel error.
pub(crate) fn probe_byte_locks(root: &OsRoot, rel: RelPath<'_>) -> Result<bool, VfsError> {
    let h = open_lock_file(root, rel, true)?;
    let mut ov = OVERLAPPED::default();
    let b = ROLE_BASE;
    // SAFETY: `ov` is a live local; `lock_try` settles any pending completion before it returns.
    match unsafe { lock_try(raw(&h), &mut ov, core::ptr::null_mut(), b) } {
        Ok(true) => {
            unlock(raw(&h), b);
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(c) => Err(sys::error(c, Domain::Store, "LockFileEx")),
    }
}
