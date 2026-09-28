//! The in-process grant table ([OS/lock §5]): the pure state machine that decides in-process ownership of the lock
//! bytes of one `LOCK` before any kernel call (contract item 2 of X-F4).
//!
//! The table has no I/O, no clock, no thread and never blocks. Its methods take the current monotonic time where they
//! need it and return *steps* that tell the driver (the `Locks` implementation of `moirai-os` or of the simulator) which
//! kernel operation to perform. The driver calls every method under the table's mutex, performs the returned step
//! outside it, and reports the result through the next transition. One table exists per `LOCK` identity per process
//! (per *simulated* process in the simulator, [OS/lock §5.1]).
//!
//! # Transitions ([OS/lock §5.3])
//!
//! | # | Method | Effect |
//! |---|---|---|
//! | T0 | [`GrantTable::register`], [`GrantTable::unregister`] | a client joins; a client leaves, releasing every byte it holds and every queue place |
//! | T1 | [`GrantTable::begin_try`] | `Busy` without a kernel call if the byte is held, being acquired or waited for in this process; else `KernelTry` |
//! | T2 | [`GrantTable::end_try`] | the try's kernel answer |
//! | T3 | [`GrantTable::begin_wait`] | a bounded wait: `KernelTry` if nobody in the process holds, acquires or waits; else `WaitInTable`; a wait whose deadline has passed is T1 |
//! | T3b | [`GrantTable::end_wait_try`] | the wait's kernel try answer: `Granted`; on `Busy` the waiter drives a kernel wait, or ends `Busy` if its deadline has passed |
//! | T4 | [`GrantTable::kernel_granted`] | a kernel wait obtained the byte: hand it to the oldest queued client whose deadline has not passed, or release it |
//! | T5 | [`GrantTable::deadline_passed`] | a waiting client's deadline passed |
//! | T6 | [`GrantTable::kernel_cancelled`] | a caller-driven kernel wait was cancelled without a grant |
//! | T7 | [`GrantTable::release`] | a holder releases: always a kernel unlock |
//! | T8 | [`GrantTable::probe_step`] | a probe: `Held` without a kernel call if a client of this table holds the byte |
//! | T9 | [`GrantTable::holds`], [`GrantTable::holds_any_role`] | queries |
//!
//! Two transitions complete the machine where [OS/lock §5.3] leaves a gap: [`GrantTable::take_notice`], through which a
//! client that waits in the table learns what another thread's transition decided for it (a hand-off of T4, the
//! `StartWait` of T2, T3b, T5 and T7, the `NewDriver` of T6, a kernel failure), and [`GrantTable::kernel_failed`], for a
//! kernel wait that ends in an error rather than a grant or a cancellation.
//!
//! # Where the table completes or amends [OS/lock §5.3]
//!
//! Recorded as spec findings of WP-30, for WP-80a:
//!
//! 1. [`GrantTable::register`] takes the client's [`LockMode`], so that an acquisition through a `Probe`-mode client
//!    panics ([OS/lock §4]).
//! 2. [`GrantTable::release`] consumes the [`Grant`] instead of naming a byte; a grant of another client or table panics.
//! 3. `take_notice` and `kernel_failed` are added (above).
//! 4. T3's fast path gives the client its `seq` and queue place at T3, not at T3b: a client that queues while the try is
//!    in flight then ranks behind it, as `seq` is the arrival counter (I-L4).
//! 5. After T6 the byte stays `Idle` until the new driver collects its `NewDriver` notice, and only then becomes
//!    `Waiting(Caller(o))`: a client whose deadline passes before it collects is never told to cancel a wait it never
//!    issued.
//! 6. T4's `ReleaseNow` carries a `then` step, so that the queue keeps moving when every queued deadline has passed.
//! 7. A `CancelKernelWait` whose settle fails with an error other than 995 is reported through `kernel_failed`
//!    ([OS/lock §7.1] leaves that case open).
//! 8. T3 takes the current time: a wait whose deadline has already passed — always so for `acquire_within(b, 0)`, "a
//!    try" in [OS/lock §4] — runs T1 after T3's preconditions ([`WaitStep::Try`]), so a busy byte ends the request as
//!    `Busy` with no kernel wait and no waiter thread.
//! 9. The registry chooses each table's [`TableId`] ([`GrantTable::new`]), so that ids, and every trace that shows them,
//!    are deterministic in the simulator.
//! 10. T3b takes the current time: a `Busy` answer that arrives when the client's deadline has already passed (a short
//!     `acquire_within`, a preempted thread, a `StartWait` collected just before the deadline) ends the request as
//!     `Busy` ([`WaitStep::Busy`]) and starts the next queued client, instead of starting a kernel wait — in mode
//!     `WaiterThread` a thread and an OFD that the client would abandon at once ([OS/README §5.2] item 3).
//! 11. T0 on a client whose caller-driven kernel wait is still in flight — driving it, or cancelling it after T5 —
//!     returns `CancelKernelWait`: the dropping thread cancels, settles and reports T4, T6 or `kernel_failed` exactly as
//!     a timed-out driver does. T0 on a client whose kernel try is in flight returns `KernelUnlock` for the byte,
//!     whose error is ignored because the try may not have been granted.
//!
//! Beyond [OS/lock §5.3] the table also exposes [`GrantTable::holder`], [`GrantTable::kernel_state`],
//! [`GrantTable::id`], [`GrantTable::mode`], [`GrantTable::is_empty`] and [`GrantTable::client_count`], the types
//! [`KernelState`], [`Owner`], [`Driver`], [`KernelHandle`] and [`TableId`], and in `lock`, [`Grant::client`],
//! [`Grant::table`], [`LockByte::from_offset`], [`SlotIndex::get`] and [`SlotIndex::record_offset`] (additive, for the
//! drivers, the simulator's traces and the tests).
//!
//! # Driving the table
//!
//! - **Waits.** `acquire_within(b, within_ms)` reads `now = Clock::mono_ns()` and calls
//!   `begin_wait(c, b, now + within_ms × 10^6, now)`; `try_acquire(b)` calls `begin_try(c, b)`. Neither decides anything
//!   itself: `within_ms = 0` is a try because the table makes it one. The answer of a waiting client's kernel try is
//!   reported with a fresh `Clock::mono_ns()` (`end_wait_try`), so that a late `Busy` starts no kernel wait.
//! - **Notices.** A step that names another client (`StartWait`, `NewDriver`, `HandTo`, a failure) means: wake the table's
//!   waiters (a condvar broadcast). A client blocked in `WaitInTable` that wakes — notified, spuriously, or at its
//!   deadline — calls [`GrantTable::deadline_passed`] if its deadline has passed and [`GrantTable::take_notice`]
//!   otherwise, and acts on the result. A client whose driving thread is itself the reporter of `HandTo(self)` also
//!   collects the grant with `take_notice`.
//! - **Kernel unlocks.** A [`Step::KernelUnlock`] (from T0 and T4) and the unlock of a [`ReleaseStep`] (T7) are
//!   performed *before the table's mutex is released*: the table records the byte as idle at once, and on Unix an
//!   in-process try that raced an unperformed unlock on the shared role descriptor would merge with it and lose its
//!   grant. Unlocking is non-blocking on every OS. Every other step is performed outside the mutex.
//! - **Progress.** Whenever no client of the table holds a waitable byte, no kernel acquisition is in flight and clients
//!   queue for it, exactly one of them — the oldest — has been told to start (`StartWait` or `NewDriver`). A step that
//!   starts a client is returned as `then`.
//!
//! # Invariants ([OS/lock §5.4])
//!
//! I-L1 at most one holder, and `holder ≠ None ⇔ kernel = Held`; I-L2 single flight (one kernel acquisition per byte,
//! none while held); I-L3 non-reentrancy; I-L4 oldest first; I-L5 order; I-L6 exactly one outcome per request; I-L7
//! release unlocks; I-L8 probe short-circuit; I-L9 no leak on drop; I-L10 waitable set. Violations of I-L3, I-L5 and
//! I-L10, an acquisition through a `Probe`-mode client, and a grant released through another client or table are
//! programming errors: they panic, with a message that starts `grant table:`.

use core::fmt;

use crate::lock::{Grant, LockByte, LockError, LockMode, SlotIndex};

/// How kernel waits run on this OS ([OS/lock §5.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WaitMode {
    /// Windows: the kernel wait is an overlapped request driven by the thread of the waiting client ("driver").
    CallerDriven,
    /// Linux, macOS: the kernel wait runs in a waiter thread that owns its own OFD.
    WaiterThread,
}

/// A client of one table; never reused within a table ([OS/lock §5.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ClientId(u64);

impl ClientId {
    /// The raw id, for traces and diagnostics.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "client {}", self.0)
    }
}

/// The id of a table, chosen by the registry that creates it ([OS/lock §5.1]); a grant records it so that it cannot be
/// released through another table.
///
/// The registry gives distinct ids to every table whose clients could meet one another's grants: `moirai-os` draws them
/// from a process-global counter, the simulator from a counter of its simulated world, in creation order, so that a
/// seed replays with the same ids.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct TableId(u64);

impl TableId {
    /// The id `raw`.
    pub const fn new(raw: u64) -> TableId {
        TableId(raw)
    }

    /// The raw id, for traces and diagnostics.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The handle a `KernelTry` uses ([OS/lock §5.3] T1).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KernelHandle {
    /// The table's handle for this role byte, opened lazily and reused across grants.
    RoleHandle,
    /// A new handle opened for this slot grant; closed at release, or by the driver itself on busy.
    NewSlotHandle,
}

/// The handle through which the process holds a byte in the kernel ([OS/lock §5.2] `Owner`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Owner {
    /// The table's role handle.
    RoleHandle,
    /// The slot handle opened for this client's grant.
    SlotHandle(ClientId),
    /// A Unix waiter thread's own OFD (mode `WaiterThread`); closed after the unlock.
    WaiterHandle,
}

/// Who drives an outstanding kernel wait ([OS/lock §5.2] `Driver`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Driver {
    /// Mode `CallerDriven`: the thread of this client drives the overlapped request.
    Caller(ClientId),
    /// Mode `WaiterThread`: a waiter thread.
    Thread,
}

/// The kernel state of one byte for this process ([OS/lock §5.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KernelState {
    /// No kernel acquisition in flight and nothing held.
    Idle,
    /// A non-blocking kernel try by this client is in flight.
    Trying(ClientId),
    /// The process holds the byte through this handle.
    Held(Owner),
    /// A blocking kernel wait is in flight.
    Waiting(Driver),
}

/// The answer of a kernel try; errors are reported as `Err(LockError)` beside it.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KernelResult {
    /// The kernel granted the byte.
    Granted,
    /// Another process holds it.
    Busy,
}

/// The result of T1.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TryStep {
    /// Held, acquired or waited for in this process: `Busy` with no kernel call.
    Busy,
    /// Perform a kernel try through `handle`, then report T2 (`end_try`).
    KernelTry {
        /// The handle to try through.
        handle: KernelHandle,
    },
}

/// The outcome of T2 for the trying client.
#[derive(Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The client holds the byte.
    Granted(Grant),
    /// Another process holds it.
    Busy,
    /// The kernel try failed.
    Error(LockError),
}

/// A step for the kernel or for another client, returned beside a transition's own result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Step {
    /// Unlock `byte` in the kernel through `owner`, before the table's mutex is released. For a `SlotHandle` the handle
    /// is then closed. After an interrupted try (T0 on a client that was trying) the byte may not be held through that
    /// handle; the unlock's error is then ignored.
    KernelUnlock {
        /// The byte.
        byte: LockByte,
        /// The handle that holds it.
        owner: Owner,
    },
    /// T0 only: cancel the caller-driven kernel wait on `byte` whose driver left, settle it, and report T4
    /// (`kernel_granted`), T6 (`kernel_cancelled`) or, for a settle error other than 995, `kernel_failed`.
    CancelKernelWait {
        /// The byte.
        byte: LockByte,
    },
    /// `client` must start: it performs a kernel try (T3's `KernelTry`) and continues with T3b. Wake the waiters; the
    /// client collects this with `take_notice`.
    StartWait {
        /// The byte.
        byte: LockByte,
        /// The oldest queued client.
        client: ClientId,
    },
    /// `client` must issue a fresh caller-driven kernel wait (T6). Wake the waiters; the client collects this with
    /// `take_notice`.
    NewDriver {
        /// The byte.
        byte: LockByte,
        /// The oldest queued client.
        client: ClientId,
    },
}

/// The result of T3, T3b and `take_notice` for the waiting client.
#[derive(Debug, Eq, PartialEq)]
pub enum WaitStep {
    /// T3 only, for a wait whose deadline has already passed (`acquire_within(b, 0)`): the request is a try, and the
    /// client follows T1's step exactly as `try_acquire` does — `Busy` ends the request as `Busy` with no kernel call;
    /// `KernelTry` is performed and reported through T2 (`end_try`).
    Try(TryStep),
    /// Perform a kernel try through `handle`, then report T3b (`end_wait_try`).
    KernelTry {
        /// The handle (always the role handle: only role bytes are waitable).
        handle: KernelHandle,
    },
    /// Block on the table's condvar until notified or the deadline; then `take_notice` or `deadline_passed`.
    WaitInTable,
    /// The client holds the byte.
    Granted(Grant),
    /// Mode `CallerDriven`: issue the overlapped kernel wait and wait for it until the deadline; report T4 on a grant,
    /// T5 on the deadline, `kernel_failed` on an error.
    DriveKernelWait,
    /// Mode `WaiterThread`: start the waiter thread (it reports T4 or `kernel_failed`), then block in the table as for
    /// `WaitInTable`. A thread that cannot be started is reported with `kernel_failed`.
    StartWaiterThread,
    /// T3b only: the kernel answered `Busy` after the client's deadline had passed. The request ends as `Busy` with no
    /// kernel wait; `then` starts the next queued client, if any.
    Busy {
        /// A step for another client.
        then: Option<Step>,
    },
    /// The request failed with `error`; `then` starts the next queued client, if any.
    Error {
        /// The failure.
        error: LockError,
        /// A step for another client.
        then: Option<Step>,
    },
}

/// The result of T4.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GrantStep {
    /// The byte now belongs to this client, which collects its grant with `take_notice` (wake the waiters; a driver
    /// that is this client collects it itself).
    HandTo(ClientId),
    /// No queued client's deadline is still open: unlock through `owner` now (before the mutex is released); `then`
    /// starts the next queued client, if any.
    ReleaseNow {
        /// The handle that holds the byte.
        owner: Owner,
        /// A step for another client.
        then: Option<Step>,
    },
}

/// The result of T5 for the waiting client.
#[derive(Debug, Eq, PartialEq)]
pub enum DeadlineStep {
    /// The grant arrived first: return `Granted`.
    AlreadyGranted(Grant),
    /// The client drives the kernel wait: cancel it, settle, report T4, T6 or (a settle error other than 995)
    /// `kernel_failed`, then return `Busy`.
    CancelKernelWait,
    /// Return `Busy`; `then` starts the next queued client, if any.
    Busy {
        /// A step for another client.
        then: Option<Step>,
    },
    /// A kernel failure was assigned to the client before its deadline: return the error.
    Error(LockError),
}

/// The result of T6.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CancelStep {
    /// The oldest queued client must issue a fresh kernel wait (it collects this with `take_notice`).
    NewDriver(ClientId),
    /// Nobody waits.
    Idle,
}

/// The result of T7: unlock `byte` through `owner` before the mutex is released; `then` starts the next queued client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseStep {
    /// The released byte.
    pub byte: LockByte,
    /// The handle that held it.
    pub owner: Owner,
    /// A step for another client.
    pub then: Option<Step>,
}

/// The result of `kernel_failed`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailStep {
    /// The client whose request the error ends (it collects the error with `take_notice`, or from `deadline_passed`).
    pub failed: Option<ClientId>,
    /// A step for another client.
    pub then: Option<Step>,
}

/// The result of T8.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProbeStep {
    /// A client of this table holds the byte: `Held`, no kernel call.
    Held,
    /// Probe in the kernel through the table's probe handle ([OS/lock §8]).
    KernelProbe,
}

/// Who holds a byte in the table.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct HeldBy {
    client: ClientId,
    /// `false` between a hand-off (T4) and the holder's collection of its grant.
    claimed: bool,
}

/// One queued client of a waitable byte; queues are ordered by `seq`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Waiter {
    client: ClientId,
    seq: u64,
    deadline_ns: u64,
}

/// The table's state of one role byte.
#[derive(Clone, Debug)]
struct RoleState {
    holder: Option<HeldBy>,
    kernel: KernelState,
    /// Only ever non-empty for the waitable bytes.
    queue: Vec<Waiter>,
}

/// The table's state of one slot that is not idle (idle slots take no memory).
#[derive(Copy, Clone, Debug)]
struct SlotState {
    slot: SlotIndex,
    holder: Option<HeldBy>,
    kernel: KernelState,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ReqKind {
    Try,
    Wait,
}

/// What another thread decided for a waiting client.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Notice {
    None,
    StartWait,
    NewDriver,
    Failed(LockError),
}

/// A client's one request in flight.
#[derive(Clone, Debug)]
struct Request {
    byte: LockByte,
    kind: ReqKind,
    notice: Notice,
}

#[derive(Clone, Debug)]
struct ClientRec {
    id: ClientId,
    mode: LockMode,
    req: Option<Request>,
}

/// Where a byte's state lives.
enum Place {
    Role(usize),
    Slot(SlotIndex),
}

/// The role bytes in the order of `GrantTable::roles`.
const ROLE_BYTES: [LockByte; 5] = [
    LockByte::Writer,
    LockByte::Leader,
    LockByte::Maintenance,
    LockByte::Quiet,
    LockByte::Flush,
];

const fn place(b: LockByte) -> Place {
    match b {
        LockByte::Writer => Place::Role(0),
        LockByte::Leader => Place::Role(1),
        LockByte::Maintenance => Place::Role(2),
        LockByte::Quiet => Place::Role(3),
        LockByte::Flush => Place::Role(4),
        LockByte::Slot(s) => Place::Slot(s),
    }
}

/// The in-process lock-ownership state machine of one `LOCK` ([OS/lock §5]).
#[derive(Debug)]
pub struct GrantTable {
    id: TableId,
    mode: WaitMode,
    next_client: u64,
    next_seq: u64,
    clients: Vec<ClientRec>,
    roles: [RoleState; 5],
    slots: Vec<SlotState>,
}

impl GrantTable {
    /// An empty table whose kernel waits run in `mode`, with the id `id` its registry chose (see [`TableId`]).
    pub fn new(mode: WaitMode, id: TableId) -> GrantTable {
        let idle = RoleState {
            holder: None,
            kernel: KernelState::Idle,
            queue: Vec::new(),
        };
        GrantTable {
            id,
            mode,
            next_client: 1,
            next_seq: 0,
            clients: Vec::new(),
            roles: [idle.clone(), idle.clone(), idle.clone(), idle.clone(), idle],
            slots: Vec::new(),
        }
    }

    /// The table's id.
    pub fn id(&self) -> TableId {
        self.id
    }

    /// The table's wait mode.
    pub fn mode(&self) -> WaitMode {
        self.mode
    }

    /// `true` when no client is registered (the registry then closes the table's role and probe handles).
    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    /// The number of registered clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// The client of this table that holds `b`, if any (including a hand-off not yet collected).
    pub fn holder(&self, b: LockByte) -> Option<ClientId> {
        match place(b) {
            Place::Role(i) => self.roles[i].holder.map(|h| h.client),
            Place::Slot(s) => self.slot(s).and_then(|st| st.holder).map(|h| h.client),
        }
    }

    /// The kernel state of `b` for this process.
    pub fn kernel_state(&self, b: LockByte) -> KernelState {
        match place(b) {
            Place::Role(i) => self.roles[i].kernel,
            Place::Slot(s) => self.slot(s).map_or(KernelState::Idle, |st| st.kernel),
        }
    }

    // ---- T0 ----

    /// T0: registers a client. `mode` is the client's `LockMode`: a `Probe`-mode client may only probe.
    pub fn register(&mut self, mode: LockMode) -> ClientId {
        let id = ClientId(self.next_client);
        self.next_client += 1;
        self.clients.push(ClientRec {
            id,
            mode,
            req: None,
        });
        id
    }

    /// T0: unregisters `c` (its `Locks::Client` was dropped). Emits `KernelUnlock` for every byte `c` holds (a hand-off
    /// not yet collected included) and for a try it left in flight (its error is then ignored), `CancelKernelWait` for a
    /// caller-driven kernel wait it was driving or had begun to cancel (the dropping thread settles it and reports T4,
    /// T6 or `kernel_failed`), and `StartWait` for the next queued client wherever `c` leaves a byte idle with clients
    /// waiting (I-L9). Panics if `c` is not registered.
    pub fn unregister(&mut self, c: ClientId) -> Vec<Step> {
        let pos = self.client_pos(c);
        self.clients.remove(pos);
        let mut steps = Vec::new();
        for (i, &byte) in ROLE_BYTES.iter().enumerate() {
            let r = &mut self.roles[i];
            r.queue.retain(|w| w.client != c);
            if r.holder.is_some_and(|h| h.client == c) {
                let KernelState::Held(owner) = r.kernel else {
                    panic!("grant table: {byte:?} has a holder but is not held in the kernel")
                };
                r.holder = None;
                r.kernel = KernelState::Idle;
                steps.push(Step::KernelUnlock { byte, owner });
            }
            match r.kernel {
                KernelState::Trying(t) if t == c => {
                    r.kernel = KernelState::Idle;
                    steps.push(Step::KernelUnlock {
                        byte,
                        owner: Owner::RoleHandle,
                    });
                }
                KernelState::Waiting(Driver::Caller(d)) if d == c => {
                    steps.push(Step::CancelKernelWait { byte });
                }
                _ => {}
            }
            if let Some(step) = self.ensure_progress(i) {
                steps.push(step);
            }
        }
        self.slots.retain(|st| {
            let held = st.holder.is_some_and(|h| h.client == c);
            let trying = st.kernel == KernelState::Trying(c);
            if held || trying {
                steps.push(Step::KernelUnlock {
                    byte: LockByte::Slot(st.slot),
                    owner: Owner::SlotHandle(c),
                });
            }
            !(held || trying)
        });
        steps
    }

    // ---- T1, T2 ----

    /// T1: a non-blocking acquisition. `Busy` without a kernel call if the byte is held, being acquired or waited for in
    /// this process; otherwise the client performs `KernelTry` and reports T2. Panics on a reentrant acquisition (I-L3),
    /// through a `Probe`-mode client, or while `c` has another request in flight.
    pub fn begin_try(&mut self, c: ClientId, b: LockByte) -> TryStep {
        self.check_new_request(c, b);
        self.try_step(c, b)
    }

    /// T1 after its preconditions were checked.
    fn try_step(&mut self, c: ClientId, b: LockByte) -> TryStep {
        match place(b) {
            Place::Role(i) => {
                let r = &mut self.roles[i];
                if r.holder.is_some() || r.kernel != KernelState::Idle || !r.queue.is_empty() {
                    return TryStep::Busy;
                }
                r.kernel = KernelState::Trying(c);
                self.set_request(c, b, ReqKind::Try);
                TryStep::KernelTry {
                    handle: KernelHandle::RoleHandle,
                }
            }
            Place::Slot(s) => {
                if self.slot(s).is_some() {
                    return TryStep::Busy;
                }
                self.slots.push(SlotState {
                    slot: s,
                    holder: None,
                    kernel: KernelState::Trying(c),
                });
                self.set_request(c, b, ReqKind::Try);
                TryStep::KernelTry {
                    handle: KernelHandle::NewSlotHandle,
                }
            }
        }
    }

    /// T2: the answer `r` of the kernel try that T1 asked `c` to perform. On a grant `c` holds the byte; otherwise the
    /// byte is idle again and the step starts the oldest client that queued meanwhile.
    pub fn end_try(
        &mut self,
        c: ClientId,
        b: LockByte,
        r: Result<KernelResult, LockError>,
    ) -> (Outcome, Option<Step>) {
        self.check_request(c, b, ReqKind::Try);
        assert_eq!(
            self.kernel_state(b),
            KernelState::Trying(c),
            "grant table: end_try without a try in flight"
        );
        self.finish_request(c, b, ReqKind::Try);
        match place(b) {
            Place::Role(i) => {
                let st = &mut self.roles[i];
                match r {
                    Ok(KernelResult::Granted) => {
                        st.holder = Some(HeldBy {
                            client: c,
                            claimed: true,
                        });
                        st.kernel = KernelState::Held(Owner::RoleHandle);
                        (Outcome::Granted(Grant::new(b, c, self.id)), None)
                    }
                    Ok(KernelResult::Busy) => {
                        st.kernel = KernelState::Idle;
                        (Outcome::Busy, self.ensure_progress(i))
                    }
                    Err(e) => {
                        st.kernel = KernelState::Idle;
                        (Outcome::Error(e), self.ensure_progress(i))
                    }
                }
            }
            Place::Slot(s) => {
                let pos = self.slot_pos(s);
                match r {
                    Ok(KernelResult::Granted) => {
                        self.slots[pos].holder = Some(HeldBy {
                            client: c,
                            claimed: true,
                        });
                        self.slots[pos].kernel = KernelState::Held(Owner::SlotHandle(c));
                        (Outcome::Granted(Grant::new(b, c, self.id)), None)
                    }
                    Ok(KernelResult::Busy) => {
                        self.slots.remove(pos);
                        (Outcome::Busy, None)
                    }
                    Err(e) => {
                        self.slots.remove(pos);
                        (Outcome::Error(e), None)
                    }
                }
            }
        }
    }

    // ---- T3, T3b ----

    /// T3: a bounded wait until `deadline_ns`, at monotonic time `now_ns` (both `Clock::mono_ns()`). Panics unless `b` is
    /// waitable (I-L10), `c` does not hold `b` (I-L3), `c` holds no byte of equal or higher rank (I-L5), `c` is in
    /// `Acquire` mode and has no other request in flight — whatever the deadline.
    ///
    /// If the deadline has already passed (`deadline_ns <= now_ns`; always so for `acquire_within(b, 0)`, which
    /// [OS/lock §4] defines as a try), the request is T1: [`WaitStep::Try`] with T1's step, and the client neither queues
    /// nor starts a kernel wait. Otherwise, if no client of the process holds, acquires or waits for the byte, `c` tries in
    /// the kernel first (`KernelTry`, then T3b); else it queues (`WaitInTable`).
    pub fn begin_wait(
        &mut self,
        c: ClientId,
        b: LockByte,
        deadline_ns: u64,
        now_ns: u64,
    ) -> WaitStep {
        self.check_new_request(c, b);
        let (Place::Role(i), Some(rank)) = (place(b), b.rank()) else {
            panic!("grant table: {b:?} is not waitable (only Writer and Flush are)")
        };
        assert!(
            b.waitable(),
            "grant table: {b:?} is not waitable (only Writer and Flush are)"
        );
        if let Some(held) = self.held_at_or_above(c, rank) {
            panic!(
                "grant table: {c} holds {held:?} and may not wait for {b:?} (lock order slot < leader < maintenance < flush < writer)"
            )
        }
        if deadline_ns <= now_ns {
            return WaitStep::Try(self.try_step(c, b));
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let r = &mut self.roles[i];
        let fast = r.holder.is_none() && r.kernel == KernelState::Idle && r.queue.is_empty();
        r.queue.push(Waiter {
            client: c,
            seq,
            deadline_ns,
        });
        if fast {
            r.kernel = KernelState::Trying(c);
        }
        self.set_request(c, b, ReqKind::Wait);
        if fast {
            WaitStep::KernelTry {
                handle: KernelHandle::RoleHandle,
            }
        } else {
            WaitStep::WaitInTable
        }
    }

    /// T3b: the answer `r` of the kernel try of a waiting client (after T3's `KernelTry` or a `StartWait`), reported at
    /// monotonic time `now_ns`. On `Busy`, if the deadline is still open (`deadline_ns > now_ns`), the client keeps its
    /// place and deadline and becomes the kernel wait's driver (`CallerDriven`) or starts the waiter thread
    /// (`WaiterThread`); if it has passed, the request ends as [`WaitStep::Busy`] with no kernel wait. `deadline_ns` must
    /// be the deadline `c` waits with.
    pub fn end_wait_try(
        &mut self,
        c: ClientId,
        b: LockByte,
        r: Result<KernelResult, LockError>,
        deadline_ns: u64,
        now_ns: u64,
    ) -> WaitStep {
        let i = self.waiting_role(c, b);
        assert!(
            matches!(self.request(c).map(|q| &q.notice), Some(Notice::None)),
            "grant table: end_wait_try with a notice pending"
        );
        let st = &mut self.roles[i];
        assert_eq!(
            st.kernel,
            KernelState::Trying(c),
            "grant table: end_wait_try without a try in flight"
        );
        let pos = queue_pos(&st.queue, c);
        assert_eq!(
            st.queue[pos].deadline_ns, deadline_ns,
            "grant table: end_wait_try with another deadline"
        );
        match r {
            Ok(KernelResult::Granted) => {
                st.queue.remove(pos);
                st.holder = Some(HeldBy {
                    client: c,
                    claimed: true,
                });
                st.kernel = KernelState::Held(Owner::RoleHandle);
                self.finish_request(c, b, ReqKind::Wait);
                WaitStep::Granted(Grant::new(b, c, self.id))
            }
            Ok(KernelResult::Busy) if deadline_ns <= now_ns => {
                st.queue.remove(pos);
                st.kernel = KernelState::Idle;
                self.finish_request(c, b, ReqKind::Wait);
                let then = self.ensure_progress(i);
                WaitStep::Busy { then }
            }
            Ok(KernelResult::Busy) => match self.mode {
                WaitMode::CallerDriven => {
                    st.kernel = KernelState::Waiting(Driver::Caller(c));
                    WaitStep::DriveKernelWait
                }
                WaitMode::WaiterThread => {
                    st.kernel = KernelState::Waiting(Driver::Thread);
                    WaitStep::StartWaiterThread
                }
            },
            Err(error) => {
                st.queue.remove(pos);
                st.kernel = KernelState::Idle;
                self.finish_request(c, b, ReqKind::Wait);
                let then = self.ensure_progress(i);
                WaitStep::Error { error, then }
            }
        }
    }

    /// For a client that waits in the table and woke before its deadline: collects what another thread decided for it.
    /// `Granted` after a hand-off (T4); `KernelTry` after a `StartWait` (then T3b); `DriveKernelWait` after a
    /// `NewDriver` (T6); `Error` after a kernel failure assigned to it; `WaitInTable` when nothing happened (a spurious
    /// wake-up): wait again.
    pub fn take_notice(&mut self, c: ClientId, b: LockByte) -> WaitStep {
        let i = self.waiting_role(c, b);
        if let Some(grant) = self.claim_hand_off(c, b, i) {
            return WaitStep::Granted(grant);
        }
        let notice = self.request_mut(c).map_or(Notice::None, |q| {
            core::mem::replace(&mut q.notice, Notice::None)
        });
        let st = &mut self.roles[i];
        match notice {
            Notice::None => WaitStep::WaitInTable,
            Notice::StartWait => {
                assert!(
                    st.holder.is_none() && st.kernel == KernelState::Idle,
                    "grant table: StartWait on a busy byte"
                );
                st.kernel = KernelState::Trying(c);
                WaitStep::KernelTry {
                    handle: KernelHandle::RoleHandle,
                }
            }
            Notice::NewDriver => {
                assert!(
                    st.holder.is_none() && st.kernel == KernelState::Idle,
                    "grant table: NewDriver on a busy byte"
                );
                st.kernel = KernelState::Waiting(Driver::Caller(c));
                WaitStep::DriveKernelWait
            }
            Notice::Failed(error) => {
                self.finish_request(c, b, ReqKind::Wait);
                WaitStep::Error { error, then: None }
            }
        }
    }

    // ---- T4, T5, T6 and kernel failures ----

    /// T4: the outstanding kernel wait on `b` obtained the byte at monotonic time `now_ns`. The grant goes to the oldest
    /// queued client whose deadline is later than `now_ns` (I-L4), which collects it with `take_notice`; if there is none
    /// the byte is released at once (an abandoned waiter's grant, or a grant that raced every deadline).
    pub fn kernel_granted(&mut self, b: LockByte, now_ns: u64) -> GrantStep {
        let i = self.role_index(b, "kernel_granted");
        let st = &mut self.roles[i];
        let owner = match st.kernel {
            KernelState::Waiting(Driver::Caller(_)) => Owner::RoleHandle,
            KernelState::Waiting(Driver::Thread) => Owner::WaiterHandle,
            other => panic!(
                "grant table: kernel_granted on {b:?} without a kernel wait (kernel state {other:?})"
            ),
        };
        match st.queue.iter().position(|w| w.deadline_ns > now_ns) {
            Some(pos) => {
                let o = st.queue.remove(pos).client;
                st.holder = Some(HeldBy {
                    client: o,
                    claimed: false,
                });
                st.kernel = KernelState::Held(owner);
                GrantStep::HandTo(o)
            }
            None => {
                st.kernel = KernelState::Idle;
                let then = self.ensure_progress(i);
                GrantStep::ReleaseNow { owner, then }
            }
        }
    }

    /// Completes T4–T6: the outstanding kernel wait on `b` ended with `error` (an overlapped request, its settle after
    /// `CancelKernelWait` with an error other than 995, or `F_OFD_SETLKW` failed; or a waiter thread could not be
    /// started). The error ends the request of the client the wait served, which collects it with `take_notice` or
    /// `deadline_passed`:
    ///
    /// - `WaiterThread`: the oldest queued client; nobody if the waiter was abandoned.
    /// - `CallerDriven`: the driver if it still queues; else nobody — a driver that settles after T5 (or after leaving
    ///   through T0) has already been told `Busy`.
    ///
    /// Either way the byte becomes idle and `then` starts the oldest queued client, if any.
    pub fn kernel_failed(&mut self, b: LockByte, error: LockError) -> FailStep {
        let i = self.role_index(b, "kernel_failed");
        let st = &mut self.roles[i];
        let KernelState::Waiting(driver) = st.kernel else {
            panic!("grant table: kernel_failed on {b:?} without a kernel wait")
        };
        st.kernel = KernelState::Idle;
        let pos = match driver {
            Driver::Caller(d) => st.queue.iter().position(|w| w.client == d),
            Driver::Thread => (!st.queue.is_empty()).then_some(0),
        };
        let failed = pos.map(|p| st.queue.remove(p).client);
        if let Some(f) = failed {
            let q = self
                .request_mut(f)
                .expect("grant table: a queued client has a request");
            q.notice = Notice::Failed(error);
        }
        let then = self.ensure_progress(i);
        FailStep { failed, then }
    }

    /// T5: the deadline of `c`, which waits for `b`, passed. `AlreadyGranted` if a hand-off reached it first; the kernel
    /// failure assigned to it, if one was; `CancelKernelWait` if `c` drives the kernel wait (cancel, settle, report T4, T6
    /// or `kernel_failed`, then return `Busy`); otherwise `Busy`, and a start notice `c` had not acted on passes to the next client. In
    /// mode `WaiterThread` an emptied queue leaves the waiter abandoned: T4 then releases the byte at once.
    pub fn deadline_passed(&mut self, c: ClientId, b: LockByte) -> DeadlineStep {
        let i = self.waiting_role(c, b);
        assert_ne!(
            self.roles[i].kernel,
            KernelState::Trying(c),
            "grant table: deadline_passed while the client's try is in flight"
        );
        if let Some(grant) = self.claim_hand_off(c, b, i) {
            return DeadlineStep::AlreadyGranted(grant);
        }
        let notice = self.request_mut(c).map_or(Notice::None, |q| {
            core::mem::replace(&mut q.notice, Notice::None)
        });
        self.finish_request(c, b, ReqKind::Wait);
        if let Notice::Failed(e) = notice {
            return DeadlineStep::Error(e);
        }
        let st = &mut self.roles[i];
        let pos = queue_pos(&st.queue, c);
        st.queue.remove(pos);
        if st.kernel == KernelState::Waiting(Driver::Caller(c)) {
            return DeadlineStep::CancelKernelWait;
        }
        let then = self.ensure_progress(i);
        DeadlineStep::Busy { then }
    }

    /// T6: the caller-driven kernel wait on `b` was cancelled before a grant (error 995). The oldest queued client, if
    /// any, becomes the new driver.
    pub fn kernel_cancelled(&mut self, b: LockByte) -> CancelStep {
        let i = self.role_index(b, "kernel_cancelled");
        let st = &mut self.roles[i];
        assert!(
            matches!(st.kernel, KernelState::Waiting(Driver::Caller(_))),
            "grant table: kernel_cancelled on {b:?} without a caller-driven kernel wait"
        );
        st.kernel = KernelState::Idle;
        match st.queue.first().map(|w| w.client) {
            Some(o) => {
                if let Some(q) = self.request_mut(o) {
                    q.notice = Notice::NewDriver;
                }
                CancelStep::NewDriver(o)
            }
            None => CancelStep::Idle,
        }
    }

    // ---- T7, T8, T9 ----

    /// T7: `c` releases `grant`. Always a kernel unlock (I-L7), performed before the mutex is released; the next queued
    /// client competes through a fresh kernel acquisition. Panics if the grant belongs to another client or table.
    pub fn release(&mut self, c: ClientId, grant: Grant) -> ReleaseStep {
        assert_eq!(
            grant.table(),
            self.id,
            "grant table: a grant released through another table"
        );
        assert_eq!(
            grant.client(),
            c,
            "grant table: a grant released through another client"
        );
        let byte = grant.byte();
        match place(byte) {
            Place::Role(i) => {
                let st = &mut self.roles[i];
                assert_eq!(
                    st.holder,
                    Some(HeldBy {
                        client: c,
                        claimed: true
                    }),
                    "grant table: release of a byte not held"
                );
                let KernelState::Held(owner) = st.kernel else {
                    panic!("grant table: {byte:?} has a holder but is not held in the kernel")
                };
                st.holder = None;
                st.kernel = KernelState::Idle;
                let then = self.ensure_progress(i);
                ReleaseStep { byte, owner, then }
            }
            Place::Slot(s) => {
                let pos = self.slot_pos(s);
                let st = self.slots[pos];
                assert_eq!(
                    st.holder,
                    Some(HeldBy {
                        client: c,
                        claimed: true
                    }),
                    "grant table: release of a slot not held"
                );
                let KernelState::Held(owner) = st.kernel else {
                    panic!("grant table: {byte:?} has a holder but is not held in the kernel")
                };
                self.slots.remove(pos);
                ReleaseStep {
                    byte,
                    owner,
                    then: None,
                }
            }
        }
    }

    /// T8: `Held` without a kernel call if a client of this table holds `b` (I-L8); otherwise a kernel probe.
    pub fn probe_step(&self, b: LockByte) -> ProbeStep {
        if self.holder(b).is_some() {
            ProbeStep::Held
        } else {
            ProbeStep::KernelProbe
        }
    }

    /// T9: does `c` hold `b`?
    pub fn holds(&self, c: ClientId, b: LockByte) -> bool {
        self.holder(b) == Some(c)
    }

    /// T9: does any client of this table hold a role byte (every byte except a slot)?
    pub fn holds_any_role(&self) -> bool {
        self.roles.iter().any(|r| r.holder.is_some())
    }

    // ---- helpers ----

    /// Starts the oldest queued client of role byte `i` if the byte is idle, clients queue for it, and none has been
    /// started yet (the progress rule of the module documentation).
    fn ensure_progress(&mut self, i: usize) -> Option<Step> {
        let st = &self.roles[i];
        if st.holder.is_some() || st.kernel != KernelState::Idle {
            return None;
        }
        let first = st.queue.first()?.client;
        let q = self
            .request_mut(first)
            .expect("grant table: a queued client has a request");
        if matches!(q.notice, Notice::StartWait | Notice::NewDriver) {
            return None;
        }
        q.notice = Notice::StartWait;
        Some(Step::StartWait {
            byte: ROLE_BYTES[i],
            client: first,
        })
    }

    /// Collects a hand-off to `c` of role byte `i`, if one is pending.
    fn claim_hand_off(&mut self, c: ClientId, b: LockByte, i: usize) -> Option<Grant> {
        let st = &mut self.roles[i];
        if st.holder
            != Some(HeldBy {
                client: c,
                claimed: false,
            })
        {
            return None;
        }
        st.holder = Some(HeldBy {
            client: c,
            claimed: true,
        });
        self.finish_request(c, b, ReqKind::Wait);
        Some(Grant::new(b, c, self.id))
    }

    /// The panicking preconditions of every new request (T1, T3).
    fn check_new_request(&self, c: ClientId, b: LockByte) {
        let rec = &self.clients[self.client_pos(c)];
        assert!(
            rec.mode == LockMode::Acquire,
            "grant table: {c} is in Probe mode and may not acquire {b:?}"
        );
        assert!(
            rec.req.is_none(),
            "grant table: {c} already has a request in flight"
        );
        assert!(
            !self.holds(c, b),
            "grant table: {c} already holds {b:?} (reentrant acquisition, contract item 3)"
        );
    }

    /// The first byte of rank `≥ rank` that `c` holds, if any.
    fn held_at_or_above(&self, c: ClientId, rank: u8) -> Option<LockByte> {
        let role = ROLE_BYTES.iter().zip(&self.roles).find_map(|(&b, st)| {
            let mine = st.holder.is_some_and(|h| h.client == c);
            (mine && b.rank().is_some_and(|r| r >= rank)).then_some(b)
        });
        role.or_else(|| {
            self.slots
                .iter()
                .find(|st| {
                    st.holder.is_some_and(|h| h.client == c)
                        && LockByte::Slot(st.slot).rank() >= Some(rank)
                })
                .map(|st| LockByte::Slot(st.slot))
        })
    }

    fn set_request(&mut self, c: ClientId, byte: LockByte, kind: ReqKind) {
        let pos = self.client_pos(c);
        self.clients[pos].req = Some(Request {
            byte,
            kind,
            notice: Notice::None,
        });
    }

    /// Checks that `c`'s request in flight is `kind` on `b`.
    fn check_request(&self, c: ClientId, b: LockByte, kind: ReqKind) {
        match self.request(c) {
            Some(q) if q.byte == b && q.kind == kind => {}
            other => panic!(
                "grant table: {c} has no {kind:?} request for {b:?} in flight (found {other:?})"
            ),
        }
    }

    /// Ends `c`'s request, checking that it is `kind` on `b`.
    fn finish_request(&mut self, c: ClientId, b: LockByte, kind: ReqKind) {
        self.check_request(c, b, kind);
        let pos = self.client_pos(c);
        self.clients[pos].req = None;
    }

    /// The role index of `b`, checking that `c` waits for it.
    fn waiting_role(&self, c: ClientId, b: LockByte) -> usize {
        match self.request(c) {
            Some(q) if q.byte == b && q.kind == ReqKind::Wait => {}
            other => panic!("grant table: {c} does not wait for {b:?} (request {other:?})"),
        }
        self.role_index(b, "a wait")
    }

    fn role_index(&self, b: LockByte, what: &str) -> usize {
        match place(b) {
            Place::Role(i) => i,
            Place::Slot(_) => panic!("grant table: {what} on {b:?}: slots are never waited for"),
        }
    }

    fn client_pos(&self, c: ClientId) -> usize {
        self.clients
            .iter()
            .position(|r| r.id == c)
            .unwrap_or_else(|| panic!("grant table: {c} is not registered"))
    }

    fn request(&self, c: ClientId) -> Option<&Request> {
        self.clients
            .iter()
            .find(|r| r.id == c)
            .and_then(|r| r.req.as_ref())
    }

    fn request_mut(&mut self, c: ClientId) -> Option<&mut Request> {
        self.clients
            .iter_mut()
            .find(|r| r.id == c)
            .and_then(|r| r.req.as_mut())
    }

    fn slot(&self, s: SlotIndex) -> Option<&SlotState> {
        self.slots.iter().find(|st| st.slot == s)
    }

    fn slot_pos(&self, s: SlotIndex) -> usize {
        self.slots
            .iter()
            .position(|st| st.slot == s)
            .unwrap_or_else(|| panic!("grant table: slot {} is idle", s.get()))
    }
}

fn queue_pos(queue: &[Waiter], c: ClientId) -> usize {
    queue
        .iter()
        .position(|w| w.client == c)
        .unwrap_or_else(|| panic!("grant table: {c} is not queued"))
}

#[cfg(test)]
mod tests;
