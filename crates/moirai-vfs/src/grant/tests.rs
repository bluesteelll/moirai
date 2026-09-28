//! Tests of the grant table: the property tests of [OS/lock §5.5] (PLAN WP-30 acceptance) and unit tests of the
//! remaining transitions.
//!
//! 1. Non-reentrancy (I-L3) on every byte kind — `non_reentrant_on_every_byte_kind`.
//! 2. Oldest-first grant (I-L4) — `oldest_unexpired_client_gets_the_grant`, and the interleaving model below.
//! 3. Order (I-L5, I-L10) — `lock_order_is_enforced_for_every_held_set` (exhaustive) and `lock_order_examples`.
//! 4. Single flight (I-L2) and exclusivity (I-L1) over random interleavings in both wait modes —
//!    `random_interleavings_keep_every_invariant`.
//! 5. Exactly one outcome (I-L6) under grants racing deadlines (T4 against T5), including the `CancelKernelWait` path —
//!    `random_interleavings_keep_every_invariant` (its `Settle` and `WaitEnds` operations) and the unit tests. The
//!    cancel is reached both from T5 (a driver's deadline) and from T0 (a client dropped while it tries, drives or
//!    cancels, `Drop`), and each settle races grants (T4), cancellations (T6) and settle errors (`kernel_failed`).

use std::collections::{HashMap, HashSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;

use proptest::prelude::*;

use super::*;
use crate::error::{OsCode, VfsError, VfsErrorKind};
use crate::lock::{LockByte, LockError, LockMode, SlotIndex};

/// Proptest cases per property: the `pr` tier by default, more in the `nightly` and `exit` tiers (PLAN §2.1).
fn config() -> ProptestConfig {
    crate::testing::proptest_config()
}

/// Silences the expected `grant table:` panics of the tests; every other panic still prints.
fn quiet_table_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let payload = info.payload();
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied());
            if !msg.is_some_and(|m| m.contains("grant table:")) {
                default(info);
            }
        }));
    });
}

/// Does `f` panic?
fn panics<R>(f: impl FnOnce() -> R) -> bool {
    quiet_table_panics();
    panic::catch_unwind(AssertUnwindSafe(f)).is_err()
}

const MODES: [WaitMode; 2] = [WaitMode::CallerDriven, WaitMode::WaiterThread];

/// A table with the id 1 (the id matters only to `grants_are_released_only_by_their_client_and_table`).
fn table(mode: WaitMode) -> GrantTable {
    GrantTable::new(mode, TableId::new(1))
}

fn slot(i: u16) -> LockByte {
    LockByte::Slot(SlotIndex::new(i).expect("slot index below 256"))
}

fn io_error() -> LockError {
    LockError::Io(VfsError::new(VfsErrorKind::Io, OsCode(5), "test"))
}

/// Acquires `b` for `c` through T1 and a granted kernel try.
fn take(t: &mut GrantTable, c: ClientId, b: LockByte) -> Grant {
    let handle = if b.is_role() {
        KernelHandle::RoleHandle
    } else {
        KernelHandle::NewSlotHandle
    };
    assert_eq!(t.begin_try(c, b), TryStep::KernelTry { handle });
    match t.end_try(c, b, Ok(KernelResult::Granted)) {
        (Outcome::Granted(g), None) => g,
        other => panic!("unexpected {other:?}"),
    }
}

/// The waiting client `c` performs a started kernel try that the kernel grants.
fn started_try_granted(t: &mut GrantTable, c: ClientId, b: LockByte, deadline: u64) -> Grant {
    assert_eq!(
        t.take_notice(c, b),
        WaitStep::KernelTry {
            handle: KernelHandle::RoleHandle
        }
    );
    match t.end_wait_try(c, b, Ok(KernelResult::Granted), deadline, 0) {
        WaitStep::Granted(g) => g,
        other => panic!("unexpected {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Required property 1: non-reentrancy (I-L3)

proptest! {
    #![proptest_config(config())]

    #[test]
    fn non_reentrant_on_every_byte_kind(slot_i in 0u16..256, mode_i in 0usize..2) {
        let mode = MODES[mode_i];
        for b in [LockByte::Writer, LockByte::Leader, LockByte::Maintenance, LockByte::Quiet, LockByte::Flush, slot(slot_i)] {
            let mut t = table(mode);
            let c = t.register(LockMode::Acquire);
            let g = take(&mut t, c, b);
            prop_assert!(panics(|| t.begin_try(c, b)), "second try of {:?}", b);
            prop_assert!(panics(|| t.begin_wait(c, b, 10, 0)), "wait for held {:?}", b);
            // The refused calls changed nothing.
            prop_assert!(t.holds(c, b));
            prop_assert_eq!(t.kernel_state(b), KernelState::Held(if b.is_role() { Owner::RoleHandle } else { Owner::SlotHandle(c) }));
            let step = t.release(c, g);
            prop_assert_eq!(step.byte, b);
            prop_assert_eq!(t.kernel_state(b), KernelState::Idle);
            // Once released, the same client may take it again.
            let g = take(&mut t, c, b);
            let _ = t.release(c, g);
        }
        // A byte obtained through the table's queue is held just the same.
        for b in [LockByte::Writer, LockByte::Flush] {
            let mut t = table(mode);
            let h = t.register(LockMode::Acquire);
            let c = t.register(LockMode::Acquire);
            let g = take(&mut t, h, b);
            prop_assert_eq!(t.begin_wait(c, b, 100, 0), WaitStep::WaitInTable);
            let r = t.release(h, g);
            prop_assert_eq!(r.then, Some(Step::StartWait { byte: b, client: c }));
            let g2 = started_try_granted(&mut t, c, b, 100);
            prop_assert!(panics(|| t.begin_try(c, b)));
            prop_assert!(panics(|| t.begin_wait(c, b, 200, 0)));
            let _ = t.release(c, g2);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Required property 2: oldest-first grant (I-L4)

proptest! {
    #![proptest_config(config())]

    #[test]
    fn oldest_unexpired_client_gets_the_grant(
        mode_i in 0usize..2,
        byte_i in 0usize..2,
        deadlines in proptest::collection::vec(1u64..1_000, 1..8),
        now in 0u64..1_100,
    ) {
        let mode = MODES[mode_i];
        let b = [LockByte::Writer, LockByte::Flush][byte_i];
        let mut t = table(mode);
        let clients: Vec<ClientId> = deadlines.iter().map(|_| t.register(LockMode::Acquire)).collect();
        // The first waiter tries in the kernel, finds the byte busy and drives (or starts) the kernel wait.
        prop_assert_eq!(t.begin_wait(clients[0], b, deadlines[0], 0), WaitStep::KernelTry { handle: KernelHandle::RoleHandle });
        let driving = t.end_wait_try(clients[0], b, Ok(KernelResult::Busy), deadlines[0], 0);
        prop_assert_eq!(driving, match mode {
            WaitMode::CallerDriven => WaitStep::DriveKernelWait,
            WaitMode::WaiterThread => WaitStep::StartWaiterThread,
        });
        for (k, &c) in clients.iter().enumerate().skip(1) {
            prop_assert_eq!(t.begin_wait(c, b, deadlines[k], 0), WaitStep::WaitInTable);
        }
        let expected = deadlines.iter().position(|&d| d > now).map(|k| clients[k]);
        match t.kernel_granted(b, now) {
            GrantStep::HandTo(o) => {
                prop_assert_eq!(Some(o), expected);
                prop_assert!(matches!(t.take_notice(o, b), WaitStep::Granted(_)));
                prop_assert_eq!(t.holder(b), Some(o));
            }
            GrantStep::ReleaseNow { owner, then } => {
                prop_assert_eq!(expected, None);
                prop_assert_eq!(owner, match mode {
                    WaitMode::CallerDriven => Owner::RoleHandle,
                    WaitMode::WaiterThread => Owner::WaiterHandle,
                });
                // Every client's deadline passed, and the oldest is started so that the queue never stalls.
                prop_assert_eq!(then, Some(Step::StartWait { byte: b, client: clients[0] }));
            }
        }
        check_table(&t);
    }

    /// After a release (T7) the oldest queued client — whatever its deadline — is started; its try then wins.
    #[test]
    fn release_starts_the_oldest_waiter(mode_i in 0usize..2, n in 1usize..6) {
        let b = LockByte::Writer;
        let mut t = table(MODES[mode_i]);
        let h = t.register(LockMode::Acquire);
        let g = take(&mut t, h, b);
        let clients: Vec<ClientId> = (0..n).map(|_| t.register(LockMode::Acquire)).collect();
        for (k, &c) in clients.iter().enumerate() {
            prop_assert_eq!(t.begin_wait(c, b, 50 + k as u64, 0), WaitStep::WaitInTable);
        }
        let r = t.release(h, g);
        prop_assert_eq!(r.then, Some(Step::StartWait { byte: b, client: clients[0] }));
        // A younger client that wakes has nothing to do.
        if n > 1 {
            prop_assert_eq!(t.take_notice(clients[1], b), WaitStep::WaitInTable);
        }
        let g0 = started_try_granted(&mut t, clients[0], b, 50);
        prop_assert_eq!(t.holder(b), Some(clients[0]));
        let r = t.release(clients[0], g0);
        if n > 1 {
            prop_assert_eq!(r.then, Some(Step::StartWait { byte: b, client: clients[1] }));
        } else {
            prop_assert_eq!(r.then, None);
        }
        check_table(&t);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Required property 3: order (I-L5, I-L10)

#[test]
fn lock_order_is_enforced_for_every_held_set() {
    let roles = [
        LockByte::Leader,
        LockByte::Maintenance,
        LockByte::Quiet,
        LockByte::Flush,
        LockByte::Writer,
    ];
    let targets = [
        LockByte::Writer,
        LockByte::Flush,
        LockByte::Leader,
        LockByte::Maintenance,
        LockByte::Quiet,
        slot(9),
        slot(3),
    ];
    for mode in MODES {
        for mask in 0u32..64 {
            for target in targets {
                let mut t = table(mode);
                let c = t.register(LockMode::Acquire);
                let mut held = Vec::new();
                for (k, &b) in roles.iter().enumerate() {
                    if mask & (1 << k) != 0 {
                        let _ = take(&mut t, c, b);
                        held.push(b);
                    }
                }
                if mask & 32 != 0 {
                    let _ = take(&mut t, c, slot(3));
                    held.push(slot(3));
                }
                let max_rank = held.iter().filter_map(|b| b.rank()).max();
                let allowed = target.waitable()
                    && !held.contains(&target)
                    && max_rank.is_none_or(|m| Some(m) < target.rank());
                let panicked = panics(|| t.begin_wait(c, target, 1_000, 0));
                assert_eq!(
                    panicked, !allowed,
                    "mode {mode:?}, held {held:?}, wait for {target:?}"
                );
                if !panicked {
                    check_table(&t);
                }
            }
        }
    }
}

#[test]
fn lock_order_examples() {
    let ok = |held: &[LockByte], target: LockByte| {
        let mut t = table(WaitMode::CallerDriven);
        let c = t.register(LockMode::Acquire);
        for &b in held {
            let _ = take(&mut t, c, b);
        }
        !panics(|| t.begin_wait(c, target, 5, 0))
    };
    assert!(ok(&[LockByte::Flush], LockByte::Writer));
    assert!(ok(&[LockByte::Maintenance], LockByte::Flush));
    assert!(ok(&[slot(0)], LockByte::Writer));
    assert!(ok(
        &[LockByte::Maintenance, LockByte::Flush],
        LockByte::Writer
    ));
    assert!(ok(&[LockByte::Quiet], LockByte::Flush));
    assert!(!ok(&[LockByte::Writer], LockByte::Flush));
    assert!(!ok(&[LockByte::Writer], LockByte::Writer));
    assert!(!ok(&[], LockByte::Maintenance));
    assert!(!ok(&[], LockByte::Leader));
    assert!(!ok(&[], LockByte::Quiet));
    assert!(!ok(&[], slot(1)));
}

// ---------------------------------------------------------------------------------------------------------------------
// Unit tests of the remaining transitions and programming errors

#[test]
fn probe_mode_clients_cannot_acquire() {
    let mut t = table(WaitMode::WaiterThread);
    let p = t.register(LockMode::Probe);
    assert!(panics(|| t.begin_try(p, LockByte::Maintenance)));
    assert!(panics(|| t.begin_wait(p, LockByte::Writer, 9, 0)));
    assert_eq!(t.probe_step(LockByte::Writer), ProbeStep::KernelProbe);
}

#[test]
fn grants_are_released_only_by_their_client_and_table() {
    let mut t = table(WaitMode::CallerDriven);
    let mut other = GrantTable::new(WaitMode::CallerDriven, TableId::new(2));
    assert_ne!(t.id(), other.id());
    let a = t.register(LockMode::Acquire);
    let b = t.register(LockMode::Acquire);
    let g = take(&mut t, a, LockByte::Flush);
    assert!(panics(|| t.release(b, g)));
    let g = take(&mut t, b, LockByte::Leader);
    let x = other.register(LockMode::Acquire);
    assert!(panics(|| other.release(x, g)));
}

#[test]
fn a_second_request_in_flight_panics() {
    let mut t = table(WaitMode::CallerDriven);
    let c = t.register(LockMode::Acquire);
    assert_eq!(
        t.begin_try(c, LockByte::Maintenance),
        TryStep::KernelTry {
            handle: KernelHandle::RoleHandle
        }
    );
    assert!(panics(|| t.begin_try(c, LockByte::Leader)));
    assert!(panics(|| t.begin_wait(c, LockByte::Writer, 5, 0)));
    assert!(panics(|| t.end_try(
        c,
        LockByte::Leader,
        Ok(KernelResult::Busy)
    )));
}

/// `acquire_within(b, 0)` is a try ([OS/lock §4]): T3 with a deadline that has passed runs T1 after T3's preconditions.
#[test]
fn a_wait_whose_deadline_has_passed_is_a_try() {
    for mode in MODES {
        let mut t = table(mode);
        let a = t.register(LockMode::Acquire);
        let c = t.register(LockMode::Acquire);
        // An idle byte: a kernel try, reported through T2.
        assert_eq!(
            t.begin_wait(c, LockByte::Writer, 7, 7),
            WaitStep::Try(TryStep::KernelTry {
                handle: KernelHandle::RoleHandle
            })
        );
        let (out, then) = t.end_try(c, LockByte::Writer, Ok(KernelResult::Granted));
        let Outcome::Granted(g) = out else {
            panic!("unexpected {out:?}")
        };
        assert_eq!(then, None);
        // Held by another client: `Busy` with no kernel call, no queue place and no kernel wait.
        assert_eq!(
            t.begin_wait(a, LockByte::Writer, 5, 9),
            WaitStep::Try(TryStep::Busy)
        );
        assert!(t.roles[0].queue.is_empty());
        assert_eq!(
            t.kernel_state(LockByte::Writer),
            KernelState::Held(Owner::RoleHandle)
        );
        check_table(&t);
        // The preconditions hold whatever the deadline: reentrancy, the waitable set and the order.
        assert!(panics(|| t.begin_wait(c, LockByte::Writer, 0, 0)));
        assert!(panics(|| t.begin_wait(a, LockByte::Maintenance, 0, 0)));
        assert!(panics(|| t.begin_wait(c, LockByte::Flush, 3, 3)));
        let _ = t.release(c, g);
        // A zero wait while another client queues is `Busy`: it never overtakes the queue.
        let h = take(&mut t, a, LockByte::Flush);
        assert_eq!(
            t.begin_wait(c, LockByte::Flush, 50, 0),
            WaitStep::WaitInTable
        );
        let d = t.register(LockMode::Acquire);
        let r = t.release(a, h);
        assert_eq!(
            r.then,
            Some(Step::StartWait {
                byte: LockByte::Flush,
                client: c
            })
        );
        assert_eq!(
            t.begin_wait(d, LockByte::Flush, 1, 1),
            WaitStep::Try(TryStep::Busy)
        );
        let g = started_try_granted(&mut t, c, LockByte::Flush, 50);
        let _ = t.release(c, g);
        check_table(&t);
    }
}

/// T3b: a `Busy` answer that arrives once the deadline has passed ends the request as `Busy` with no kernel wait (no
/// driver, no waiter thread) and starts the next queued client; a late grant is still returned.
#[test]
fn a_busy_answer_after_the_deadline_starts_no_kernel_wait() {
    for mode in MODES {
        let b = LockByte::Writer;
        let mut t = table(mode);
        let c = t.register(LockMode::Acquire);
        let w = t.register(LockMode::Acquire);
        assert_eq!(
            t.begin_wait(c, b, 10, 0),
            WaitStep::KernelTry {
                handle: KernelHandle::RoleHandle
            }
        );
        assert_eq!(t.begin_wait(w, b, 50, 1), WaitStep::WaitInTable);
        // The answer arrives at the deadline: no kernel wait; w is started.
        assert_eq!(
            t.end_wait_try(c, b, Ok(KernelResult::Busy), 10, 10),
            WaitStep::Busy {
                then: Some(Step::StartWait { byte: b, client: w })
            }
        );
        assert_eq!(t.kernel_state(b), KernelState::Idle);
        check_table(&t);
        // c may ask again at once and queues behind w.
        assert_eq!(t.begin_wait(c, b, 90, 11), WaitStep::WaitInTable);
        // w's started try is late too: the start passes to c.
        assert_eq!(
            t.take_notice(w, b),
            WaitStep::KernelTry {
                handle: KernelHandle::RoleHandle
            }
        );
        assert_eq!(
            t.end_wait_try(w, b, Ok(KernelResult::Busy), 50, 70),
            WaitStep::Busy {
                then: Some(Step::StartWait { byte: b, client: c })
            }
        );
        check_table(&t);
        // A grant that arrives after the deadline is returned, never dropped (contract item 5).
        assert_eq!(
            t.take_notice(c, b),
            WaitStep::KernelTry {
                handle: KernelHandle::RoleHandle
            }
        );
        let g = match t.end_wait_try(c, b, Ok(KernelResult::Granted), 90, 95) {
            WaitStep::Granted(g) => g,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(t.holder(b), Some(c));
        let r = t.release(c, g);
        assert_eq!(r.then, None);
        // Nobody queued: a late busy answer starts nobody.
        assert!(matches!(
            t.begin_wait(w, b, 100, 99),
            WaitStep::KernelTry { .. }
        ));
        assert_eq!(
            t.end_wait_try(w, b, Ok(KernelResult::Busy), 100, 100),
            WaitStep::Busy { then: None }
        );
        assert!(t.roles[0].queue.is_empty());
        check_table(&t);
    }
}

#[test]
fn probes_short_circuit_on_held_bytes() {
    let mut t = table(WaitMode::WaiterThread);
    let c = t.register(LockMode::Acquire);
    let p = t.register(LockMode::Probe);
    for b in [LockByte::Writer, LockByte::Quiet, slot(77)] {
        assert_eq!(t.probe_step(b), ProbeStep::KernelProbe);
        let g = take(&mut t, c, b);
        assert_eq!(t.probe_step(b), ProbeStep::Held);
        assert!(t.holds(c, b) && !t.holds(p, b));
        let _ = t.release(c, g);
    }
    assert!(!t.holds_any_role());
    let g = take(&mut t, c, slot(1));
    assert!(!t.holds_any_role(), "a slot is not a role byte");
    let q = take(&mut t, c, LockByte::Quiet);
    assert!(t.holds_any_role());
    let _ = (t.release(c, g), t.release(c, q));
}

#[test]
fn tries_are_busy_without_a_kernel_call_while_the_process_holds_acquires_or_waits() {
    let mut t = table(WaitMode::CallerDriven);
    let a = t.register(LockMode::Acquire);
    let b = t.register(LockMode::Acquire);
    // Held.
    let g = take(&mut t, a, LockByte::Writer);
    assert_eq!(t.begin_try(b, LockByte::Writer), TryStep::Busy);
    let _ = t.release(a, g);
    // Being acquired.
    assert!(matches!(
        t.begin_try(a, LockByte::Writer),
        TryStep::KernelTry { .. }
    ));
    assert_eq!(t.begin_try(b, LockByte::Writer), TryStep::Busy);
    assert_eq!(
        t.end_try(a, LockByte::Writer, Ok(KernelResult::Busy)),
        (Outcome::Busy, None)
    );
    // Waited for.
    assert!(matches!(
        t.begin_wait(a, LockByte::Writer, 10, 0),
        WaitStep::KernelTry { .. }
    ));
    assert_eq!(
        t.end_wait_try(a, LockByte::Writer, Ok(KernelResult::Busy), 10, 0),
        WaitStep::DriveKernelWait
    );
    assert_eq!(t.begin_try(b, LockByte::Writer), TryStep::Busy);
    // Slots: one grant per slot per process.
    let s = take(&mut t, b, slot(4));
    assert_eq!(
        t.begin_try(b, slot(5)),
        TryStep::KernelTry {
            handle: KernelHandle::NewSlotHandle
        }
    );
    assert_eq!(
        t.end_try(b, slot(5), Err(io_error())),
        (Outcome::Error(io_error()), None)
    );
    let r = t.release(b, s);
    assert_eq!(
        (r.byte, r.owner, r.then),
        (slot(4), Owner::SlotHandle(b), None)
    );
    check_table(&t);
}

#[test]
fn a_try_that_ends_while_others_queue_starts_the_oldest() {
    let mut t = table(WaitMode::WaiterThread);
    let a = t.register(LockMode::Acquire);
    let w1 = t.register(LockMode::Acquire);
    let w2 = t.register(LockMode::Acquire);
    assert!(matches!(
        t.begin_try(a, LockByte::Flush),
        TryStep::KernelTry { .. }
    ));
    assert_eq!(
        t.begin_wait(w1, LockByte::Flush, 30, 0),
        WaitStep::WaitInTable
    );
    assert_eq!(
        t.begin_wait(w2, LockByte::Flush, 40, 0),
        WaitStep::WaitInTable
    );
    let (out, then) = t.end_try(a, LockByte::Flush, Ok(KernelResult::Busy));
    assert_eq!(out, Outcome::Busy);
    assert_eq!(
        then,
        Some(Step::StartWait {
            byte: LockByte::Flush,
            client: w1
        })
    );
    check_table(&t);
    // w1's deadline passes before it acts: the start passes to w2.
    assert_eq!(
        t.deadline_passed(w1, LockByte::Flush),
        DeadlineStep::Busy {
            then: Some(Step::StartWait {
                byte: LockByte::Flush,
                client: w2
            })
        }
    );
    assert_eq!(
        t.take_notice(w2, LockByte::Flush),
        WaitStep::KernelTry {
            handle: KernelHandle::RoleHandle
        }
    );
    assert_eq!(
        t.end_wait_try(w2, LockByte::Flush, Ok(KernelResult::Busy), 40, 0),
        WaitStep::StartWaiterThread
    );
    check_table(&t);
    // The waiter thread fails: the error ends w2's request.
    let f = t.kernel_failed(LockByte::Flush, io_error());
    assert_eq!(
        f,
        FailStep {
            failed: Some(w2),
            then: None
        }
    );
    assert_eq!(
        t.take_notice(w2, LockByte::Flush),
        WaitStep::Error {
            error: io_error(),
            then: None
        }
    );
    assert_eq!(t.kernel_state(LockByte::Flush), KernelState::Idle);
    check_table(&t);
}

#[test]
fn caller_driven_cancel_paths() {
    let b = LockByte::Writer;
    let mut t = table(WaitMode::CallerDriven);
    let d = t.register(LockMode::Acquire);
    let w = t.register(LockMode::Acquire);
    assert!(matches!(
        t.begin_wait(d, b, 10, 0),
        WaitStep::KernelTry { .. }
    ));
    assert_eq!(
        t.end_wait_try(d, b, Ok(KernelResult::Busy), 10, 0),
        WaitStep::DriveKernelWait
    );
    assert_eq!(t.begin_wait(w, b, 100, 0), WaitStep::WaitInTable);
    // The driver times out: it cancels; the cancel completes with error 995 (T6): w drives next.
    assert_eq!(t.deadline_passed(d, b), DeadlineStep::CancelKernelWait);
    assert_eq!(t.kernel_cancelled(b), CancelStep::NewDriver(w));
    check_table(&t);
    assert_eq!(t.take_notice(w, b), WaitStep::DriveKernelWait);
    assert_eq!(t.kernel_state(b), KernelState::Waiting(Driver::Caller(w)));
    // A new waiter queues behind w; w times out and its cancel races a grant (T4 after T5): the grant goes to the new
    // waiter, never back to w.
    let n = t.register(LockMode::Acquire);
    assert_eq!(t.begin_wait(n, b, 200, 0), WaitStep::WaitInTable);
    assert_eq!(t.deadline_passed(w, b), DeadlineStep::CancelKernelWait);
    assert_eq!(t.kernel_granted(b, 101), GrantStep::HandTo(n));
    assert!(matches!(
        t.deadline_passed(n, b),
        DeadlineStep::AlreadyGranted(_)
    ));
    assert_eq!(t.holder(b), Some(n));
    check_table(&t);
}

#[test]
fn a_grant_to_a_driver_whose_deadline_passed_goes_to_the_next_waiter() {
    let b = LockByte::Flush;
    let mut t = table(WaitMode::CallerDriven);
    let d = t.register(LockMode::Acquire);
    let w = t.register(LockMode::Acquire);
    assert!(matches!(
        t.begin_wait(d, b, 10, 0),
        WaitStep::KernelTry { .. }
    ));
    assert_eq!(
        t.end_wait_try(d, b, Ok(KernelResult::Busy), 10, 0),
        WaitStep::DriveKernelWait
    );
    assert_eq!(t.begin_wait(w, b, 50, 0), WaitStep::WaitInTable);
    // The kernel grants at time 10, the driver's deadline: the grant goes to w; the driver returns Busy.
    assert_eq!(t.kernel_granted(b, 10), GrantStep::HandTo(w));
    assert_eq!(t.deadline_passed(d, b), DeadlineStep::Busy { then: None });
    assert!(matches!(t.take_notice(w, b), WaitStep::Granted(_)));
    check_table(&t);
}

#[test]
fn unregister_releases_everything_and_passes_the_turn() {
    // Holding, queued and slots.
    let mut t = table(WaitMode::WaiterThread);
    let a = t.register(LockMode::Acquire);
    let w = t.register(LockMode::Acquire);
    let _g = take(&mut t, a, LockByte::Writer);
    let _s = take(&mut t, a, slot(8));
    let _m = take(&mut t, a, LockByte::Maintenance);
    assert_eq!(
        t.begin_wait(w, LockByte::Writer, 90, 0),
        WaitStep::WaitInTable
    );
    let steps = t.unregister(a);
    assert_eq!(
        steps,
        vec![
            Step::KernelUnlock {
                byte: LockByte::Writer,
                owner: Owner::RoleHandle
            },
            Step::StartWait {
                byte: LockByte::Writer,
                client: w
            },
            Step::KernelUnlock {
                byte: LockByte::Maintenance,
                owner: Owner::RoleHandle
            },
            Step::KernelUnlock {
                byte: slot(8),
                owner: Owner::SlotHandle(a)
            },
        ]
    );
    assert!(!t.holds_any_role() && t.holder(slot(8)).is_none());
    assert!(panics(|| t.unregister(a)));
    check_table(&t);

    // A client with a start notice pending leaves: the next waiter is started.
    let x = t.register(LockMode::Acquire);
    assert_eq!(
        t.begin_wait(x, LockByte::Writer, 95, 0),
        WaitStep::WaitInTable
    );
    assert_eq!(
        t.unregister(w),
        vec![Step::StartWait {
            byte: LockByte::Writer,
            client: x
        }]
    );
    check_table(&t);

    // A client that left a try in flight: the byte is unlocked whatever the try did.
    let mut t = table(WaitMode::CallerDriven);
    let c = t.register(LockMode::Acquire);
    assert!(matches!(
        t.begin_try(c, LockByte::Flush),
        TryStep::KernelTry { .. }
    ));
    assert_eq!(
        t.unregister(c),
        vec![Step::KernelUnlock {
            byte: LockByte::Flush,
            owner: Owner::RoleHandle
        }]
    );
    assert_eq!(t.kernel_state(LockByte::Flush), KernelState::Idle);
    assert!(t.is_empty());

    // A driver that leaves: its wait is cancelled; the settle hands over or passes the drive on.
    let d = t.register(LockMode::Acquire);
    let v = t.register(LockMode::Acquire);
    assert!(matches!(
        t.begin_wait(d, LockByte::Writer, 10, 0),
        WaitStep::KernelTry { .. }
    ));
    assert_eq!(
        t.end_wait_try(d, LockByte::Writer, Ok(KernelResult::Busy), 10, 0),
        WaitStep::DriveKernelWait
    );
    assert_eq!(
        t.begin_wait(v, LockByte::Writer, 20, 0),
        WaitStep::WaitInTable
    );
    assert_eq!(
        t.unregister(d),
        vec![Step::CancelKernelWait {
            byte: LockByte::Writer
        }]
    );
    assert_eq!(
        t.kernel_cancelled(LockByte::Writer),
        CancelStep::NewDriver(v)
    );
    assert_eq!(
        t.take_notice(v, LockByte::Writer),
        WaitStep::DriveKernelWait
    );
    check_table(&t);

    // A holder by hand-off that leaves before collecting its grant.
    assert_eq!(t.kernel_granted(LockByte::Writer, 15), GrantStep::HandTo(v));
    assert_eq!(
        t.unregister(v),
        vec![Step::KernelUnlock {
            byte: LockByte::Writer,
            owner: Owner::RoleHandle
        }]
    );
    assert!(t.is_empty() && t.kernel_state(LockByte::Writer) == KernelState::Idle);
}

#[test]
fn an_abandoned_waiter_thread_releases_its_grant_or_serves_a_newcomer() {
    let b = LockByte::Writer;
    let mut t = table(WaitMode::WaiterThread);
    let c = t.register(LockMode::Acquire);
    assert!(matches!(
        t.begin_wait(c, b, 10, 0),
        WaitStep::KernelTry { .. }
    ));
    assert_eq!(
        t.end_wait_try(c, b, Ok(KernelResult::Busy), 10, 0),
        WaitStep::StartWaiterThread
    );
    assert_eq!(t.deadline_passed(c, b), DeadlineStep::Busy { then: None });
    assert_eq!(t.kernel_state(b), KernelState::Waiting(Driver::Thread));
    // Abandoned: its grant is released at once.
    assert_eq!(
        t.kernel_granted(b, 11),
        GrantStep::ReleaseNow {
            owner: Owner::WaiterHandle,
            then: None
        }
    );
    // Abandoned again, then a newcomer queues on the still-parked waiter and receives its grant.
    assert!(matches!(
        t.begin_wait(c, b, 20, 0),
        WaitStep::KernelTry { .. }
    ));
    assert_eq!(
        t.end_wait_try(c, b, Ok(KernelResult::Busy), 20, 12),
        WaitStep::StartWaiterThread
    );
    assert_eq!(t.deadline_passed(c, b), DeadlineStep::Busy { then: None });
    let n = t.register(LockMode::Acquire);
    assert_eq!(t.begin_wait(n, b, 40, 0), WaitStep::WaitInTable);
    assert_eq!(t.kernel_granted(b, 25), GrantStep::HandTo(n));
    let g = match t.take_notice(n, b) {
        WaitStep::Granted(g) => g,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(t.release(n, g).owner, Owner::WaiterHandle);
    check_table(&t);
}

// ---------------------------------------------------------------------------------------------------------------------
// Invariant check over the table's internals ([OS/lock §5.4])

fn check_table(t: &GrantTable) {
    let rec = |c: ClientId| t.clients.iter().find(|r| r.id == c);
    for (i, st) in t.roles.iter().enumerate() {
        let b = ROLE_BYTES[i];
        // I-L1: one holder, and a holder iff held in the kernel.
        assert_eq!(
            st.holder.is_some(),
            matches!(st.kernel, KernelState::Held(_)),
            "I-L1 on {b:?}: {st:?}"
        );
        match st.kernel {
            KernelState::Held(Owner::SlotHandle(_)) => panic!("{b:?} held through a slot handle"),
            KernelState::Held(Owner::WaiterHandle) | KernelState::Waiting(Driver::Thread) => {
                assert_eq!(t.mode, WaitMode::WaiterThread, "{b:?}: {st:?}")
            }
            KernelState::Waiting(Driver::Caller(_)) => {
                assert_eq!(t.mode, WaitMode::CallerDriven, "{b:?}: {st:?}")
            }
            KernelState::Trying(c) => {
                let q = rec(c)
                    .and_then(|r| r.req.as_ref())
                    .expect("a trying client has a request");
                assert_eq!(q.byte, b);
                if q.kind == ReqKind::Wait {
                    assert!(
                        st.queue.iter().any(|w| w.client == c),
                        "a waiting client that tries keeps its place"
                    );
                }
            }
            _ => {}
        }
        if !b.waitable() {
            assert!(st.queue.is_empty(), "only waitable bytes queue");
        }
        // The queue: strictly ordered by arrival; every entry a registered client waiting for this byte.
        assert!(
            st.queue.windows(2).all(|w| w[0].seq < w[1].seq),
            "queue order on {b:?}"
        );
        for w in &st.queue {
            let q = rec(w.client)
                .and_then(|r| r.req.as_ref())
                .expect("a queued client is registered with a request");
            assert!(
                q.byte == b && q.kind == ReqKind::Wait,
                "queued client's request"
            );
            assert!(
                !matches!(q.notice, Notice::Failed(_)),
                "a failed client is not queued"
            );
        }
        // A hand-off not yet collected: the holder still has its wait request and is no longer queued.
        if let Some(h) = st.holder.filter(|h| !h.claimed) {
            let q = rec(h.client)
                .and_then(|r| r.req.as_ref())
                .expect("an uncollected hand-off has a request");
            assert!(q.byte == b && q.kind == ReqKind::Wait);
            assert!(st.queue.iter().all(|w| w.client != h.client));
        }
        // I-L2 single flight, and the progress rule: at most one started client, the oldest, and only while nothing is
        // held or in flight; exactly one while clients queue for an idle byte.
        let started: Vec<usize> = st
            .queue
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                rec(w.client)
                    .and_then(|r| r.req.as_ref())
                    .is_some_and(|q| matches!(q.notice, Notice::StartWait | Notice::NewDriver))
            })
            .map(|(k, _)| k)
            .collect();
        assert!(started.len() <= 1, "I-L2 on {b:?}: several started clients");
        if let Some(&k) = started.first() {
            assert_eq!(k, 0, "the started client is the oldest");
            assert!(
                st.holder.is_none() && st.kernel == KernelState::Idle,
                "a start while {b:?} is busy"
            );
        }
        if st.holder.is_none() && st.kernel == KernelState::Idle && !st.queue.is_empty() {
            assert_eq!(
                started.len(),
                1,
                "progress on {b:?}: clients queue for an idle byte and nobody was started"
            );
        }
    }
    let mut seen = HashSet::new();
    for s in &t.slots {
        assert!(seen.insert(s.slot), "one entry per slot");
        assert_ne!(s.kernel, KernelState::Idle, "idle slots take no memory");
        match (s.holder, s.kernel) {
            (Some(h), KernelState::Held(Owner::SlotHandle(o))) => {
                assert!(h.claimed && h.client == o)
            }
            (None, KernelState::Trying(c)) => {
                let q = rec(c)
                    .and_then(|r| r.req.as_ref())
                    .expect("a trying client has a request");
                assert!(q.byte == LockByte::Slot(s.slot) && q.kind == ReqKind::Try);
            }
            other => panic!("slot {} in state {other:?}", s.slot.get()),
        }
    }
    // Every request in flight is visible in the byte's state.
    for r in &t.clients {
        let Some(q) = &r.req else { continue };
        match (place(q.byte), q.kind) {
            (Place::Role(i), ReqKind::Try) => {
                assert_eq!(t.roles[i].kernel, KernelState::Trying(r.id))
            }
            (Place::Slot(s), ReqKind::Try) => {
                assert_eq!(t.kernel_state(LockByte::Slot(s)), KernelState::Trying(r.id))
            }
            (Place::Role(i), ReqKind::Wait) => {
                let st = &t.roles[i];
                let queued = st.queue.iter().any(|w| w.client == r.id);
                let handed = st.holder
                    == Some(HeldBy {
                        client: r.id,
                        claimed: false,
                    });
                let failed = matches!(q.notice, Notice::Failed(_));
                assert_eq!(
                    u8::from(queued) + u8::from(handed) + u8::from(failed),
                    1,
                    "{} request state",
                    r.id
                );
            }
            (Place::Slot(_), ReqKind::Wait) => panic!("a wait for a slot"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Required properties 4 and 5: random interleavings in both wait modes
//
// A model of one process drives the table the way a `Locks` implementation does, against a simulated kernel in which
// other processes take and drop bytes. After every operation it checks the table's invariants and the kernel-level
// facts: the process holds a byte in the kernel iff the table says `Held` (I-L1, I-L7), never together with another
// process; every hand-off goes to the oldest queued client whose deadline is open (I-L4) and every start to the oldest
// queued client; every request ends in exactly one outcome (I-L6), including grants that race deadlines and the
// caller-driven cancel. At the end the model drains every request and checks that the table is empty again (I-L9).

#[derive(Copy, Clone, Debug)]
enum Op {
    Try(u8, u8),
    Wait(u8, u8, u8),
    Answer(u8, u8),
    WaitEnds(u8, u8),
    Wake(u8),
    Timeout(u8),
    Settle(u8, u8),
    Release(u8, u8),
    Advance(u8),
    Foreign(u8),
    Probe(u8),
    Drop(u8, u8),
}

/// One operation from four raw choices; `k` picks the kind with the weights below.
fn op_from(k: u8, a: u8, b: u8, c: u8) -> Op {
    match k % 16 {
        0 | 1 => Op::Try(a, b),
        2..=4 => Op::Wait(a, b, c),
        5 | 6 => Op::Answer(a, b),
        7 => Op::WaitEnds(a, b),
        8 | 9 => Op::Wake(a),
        10 => Op::Timeout(a),
        11 => Op::Settle(a, b),
        12 => Op::Release(a, b),
        13 => Op::Advance(a),
        14 => [Op::Foreign(a), Op::Probe(a)][usize::from(b % 2)],
        _ => Op::Drop(a, b),
    }
}

fn op() -> impl Strategy<Value = Op> {
    (0u8..16, any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(k, a, b, c)| op_from(k, a, b, c))
}

fn all_bytes() -> [LockByte; 7] {
    [
        LockByte::Writer,
        LockByte::Leader,
        LockByte::Maintenance,
        LockByte::Quiet,
        LockByte::Flush,
        slot(0),
        slot(200),
    ]
}

fn byte_index(b: LockByte) -> usize {
    all_bytes()
        .iter()
        .position(|&x| x == b)
        .expect("a modelled byte")
}

#[derive(Debug)]
enum SeatState {
    Idle,
    /// A kernel try in flight; `wait` = (deadline, arrival) for the try of a wait.
    Trying {
        b: LockByte,
        wait: Option<(u64, u64)>,
    },
    /// Blocked in the table.
    InTable {
        b: LockByte,
        deadline: u64,
        seq: u64,
        handed: bool,
        failed: bool,
    },
    /// `CallerDriven`: driving the kernel wait.
    Driving {
        b: LockByte,
        deadline: u64,
        seq: u64,
    },
    /// `CallerDriven`: its deadline passed and it is cancelling its kernel wait.
    Cancelling {
        b: LockByte,
    },
}

struct Seat {
    id: ClientId,
    st: SeatState,
    grants: Vec<Grant>,
    req: Option<u64>,
}

struct Model {
    t: GrantTable,
    now: u64,
    seats: Vec<Seat>,
    ours: [bool; 7],
    foreign: [bool; 7],
    next_req: u64,
    next_seq: u64,
    outcomes: HashMap<u64, u32>,
    abandoned: HashSet<u64>,
    /// Bytes whose caller-driven kernel wait was left by a dropped driver (T0's `CancelKernelWait`): the dropping thread
    /// has cancelled it and settles it at a later `Settle`.
    droppers: Vec<LockByte>,
    /// How often each transition path was taken (see `interleavings_reach_every_path`).
    hits: HashMap<&'static str, u32>,
}

impl Model {
    fn hit(&mut self, path: &'static str) {
        *self.hits.entry(path).or_insert(0) += 1;
    }

    fn new(mode: WaitMode, n: usize) -> Model {
        let mut t = table(mode);
        let seats = (0..n)
            .map(|_| Seat {
                id: t.register(LockMode::Acquire),
                st: SeatState::Idle,
                grants: Vec::new(),
                req: None,
            })
            .collect();
        Model {
            t,
            now: 0,
            seats,
            ours: [false; 7],
            foreign: [false; 7],
            next_req: 0,
            next_seq: 0,
            outcomes: HashMap::new(),
            abandoned: HashSet::new(),
            droppers: Vec::new(),
            hits: HashMap::new(),
        }
    }

    fn mode(&self) -> WaitMode {
        self.t.mode()
    }

    fn seat_of(&self, c: ClientId) -> usize {
        self.seats
            .iter()
            .position(|s| s.id == c)
            .expect("a seated client")
    }

    fn new_req(&mut self, s: usize) {
        self.seats[s].req = Some(self.next_req);
        self.next_req += 1;
    }

    /// Records the one outcome of seat `s`'s request (I-L6).
    fn outcome(&mut self, s: usize) {
        let r = self.seats[s]
            .req
            .take()
            .expect("an outcome for a request in flight");
        let n = self.outcomes.entry(r).or_insert(0);
        *n += 1;
        assert_eq!(*n, 1, "request {r} has more than one outcome");
        self.seats[s].st = SeatState::Idle;
    }

    fn grant_to(&mut self, s: usize, g: Grant) {
        assert_eq!(g.client(), self.seats[s].id);
        self.seats[s].grants.push(g);
        self.outcome(s);
    }

    fn kernel_grant(&mut self, b: LockByte) {
        let i = byte_index(b);
        assert!(
            !self.ours[i] && !self.foreign[i],
            "kernel exclusivity on {b:?}"
        );
        self.ours[i] = true;
    }

    fn kernel_unlock(&mut self, b: LockByte) {
        let i = byte_index(b);
        assert!(
            self.ours[i],
            "unlock of {b:?}, which this process does not hold"
        );
        self.ours[i] = false;
    }

    /// The seats queued for `b` in the model's view, oldest first: (arrival, deadline, seat).
    fn queued(&self, b: LockByte) -> Vec<(u64, u64, usize)> {
        let mut v: Vec<(u64, u64, usize)> = self
            .seats
            .iter()
            .enumerate()
            .filter_map(|(k, s)| match s.st {
                SeatState::InTable {
                    b: x,
                    deadline,
                    seq,
                    handed: false,
                    failed: false,
                } if x == b => Some((seq, deadline, k)),
                SeatState::Driving {
                    b: x,
                    deadline,
                    seq,
                } if x == b => Some((seq, deadline, k)),
                SeatState::Trying {
                    b: x,
                    wait: Some((deadline, seq)),
                } if x == b => Some((seq, deadline, k)),
                _ => None,
            })
            .collect();
        v.sort_unstable();
        v
    }

    /// Applies a step for another client, checking that a start goes to the oldest queued client.
    fn apply(&mut self, step: Option<Step>) {
        match step {
            None => {}
            Some(Step::StartWait { byte, client } | Step::NewDriver { byte, client }) => {
                let oldest = self.queued(byte).first().map(|&(_, _, k)| self.seats[k].id);
                assert_eq!(
                    oldest,
                    Some(client),
                    "a start goes to the oldest queued client of {byte:?}"
                );
            }
            Some(Step::KernelUnlock { byte, .. }) => self.kernel_unlock(byte),
            Some(Step::CancelKernelWait { byte }) => panic!("unexpected cancel of {byte:?}"),
        }
    }

    /// Checks a hand-off against the model's queue: the oldest client whose deadline is still open (I-L4).
    fn expect_hand_off(&self, b: LockByte, to: Option<ClientId>) {
        let expected = self
            .queued(b)
            .iter()
            .find(|&&(_, d, _)| d > self.now)
            .map(|&(_, _, k)| self.seats[k].id);
        assert_eq!(to, expected, "I-L4: hand-off of {b:?} at {}", self.now);
    }

    fn run(&mut self, op: Op) {
        let n = self.seats.len();
        match op {
            Op::Try(a, bs) => self.op_try(usize::from(a) % n, all_bytes()[usize::from(bs) % 7]),
            Op::Wait(a, bs, within) => {
                let b = [LockByte::Writer, LockByte::Flush][usize::from(bs % 2)];
                self.op_wait(usize::from(a) % n, b, u64::from(within % 40));
            }
            Op::Answer(a, coin) => self.op_answer(usize::from(a) % n, coin),
            Op::WaitEnds(bs, coin) => self.op_wait_ends(
                [LockByte::Writer, LockByte::Flush][usize::from(bs % 2)],
                coin,
            ),
            Op::Wake(a) => self.op_wake(usize::from(a) % n),
            Op::Timeout(a) => self.op_timeout(usize::from(a) % n),
            Op::Settle(a, coin) => self.op_settle(usize::from(a) % n, coin),
            Op::Release(a, k) => self.op_release(usize::from(a) % n, usize::from(k)),
            Op::Advance(ms) => self.now += u64::from(ms % 20),
            Op::Foreign(bs) => {
                let i = usize::from(bs) % 7;
                if self.foreign[i] {
                    self.foreign[i] = false;
                } else if !self.ours[i] {
                    self.foreign[i] = true;
                }
            }
            Op::Probe(bs) => {
                let b = all_bytes()[usize::from(bs) % 7];
                let held = self.t.probe_step(b) == ProbeStep::Held;
                assert_eq!(held, self.t.holder(b).is_some(), "I-L8");
                assert!(!held || self.ours[byte_index(b)]);
            }
            Op::Drop(a, coin) => self.op_drop(usize::from(a) % n, coin),
        }
        self.check();
    }

    fn op_try(&mut self, s: usize, b: LockByte) {
        if !matches!(self.seats[s].st, SeatState::Idle)
            || self.seats[s].grants.iter().any(|g| g.byte() == b)
        {
            return;
        }
        self.new_req(s);
        let c = self.seats[s].id;
        let busy_expected = self.t.holder(b).is_some()
            || self.t.kernel_state(b) != KernelState::Idle
            || !self.queued(b).is_empty()
            || self
                .seats
                .iter()
                .any(|x| matches!(x.st, SeatState::InTable { b: y, handed: true, .. } if y == b));
        match self.t.begin_try(c, b) {
            TryStep::Busy => {
                assert!(busy_expected, "T1 Busy without cause on {b:?}");
                self.hit("try-busy");
                self.outcome(s);
            }
            TryStep::KernelTry { handle } => {
                assert!(
                    !busy_expected,
                    "T1 lets a second in-process acquisition of {b:?} reach the kernel"
                );
                self.hit("try-kernel");
                let expected = if b.is_role() {
                    KernelHandle::RoleHandle
                } else {
                    KernelHandle::NewSlotHandle
                };
                assert_eq!(handle, expected);
                self.seats[s].st = SeatState::Trying { b, wait: None };
            }
        }
    }

    fn op_wait(&mut self, s: usize, b: LockByte, within: u64) {
        let seat = &self.seats[s];
        let rank = b.rank().expect("a waitable byte has a rank");
        let order_ok = seat
            .grants
            .iter()
            .all(|g| g.byte().rank().is_none_or(|r| r < rank));
        if !matches!(seat.st, SeatState::Idle) || !order_ok {
            return;
        }
        self.new_req(s);
        let deadline = self.now + within;
        let c = self.seats[s].id;
        let fast = self.t.holder(b).is_none()
            && self.t.kernel_state(b) == KernelState::Idle
            && self.queued(b).is_empty();
        let step = self.t.begin_wait(c, b, deadline, self.now);
        if within == 0 {
            // `acquire_within(b, 0)`: the deadline has already passed, so the request is T1.
            match step {
                WaitStep::Try(TryStep::Busy) => {
                    assert!(!fast, "a zero wait is Busy although {b:?} is idle");
                    self.hit("wait-zero-busy");
                    self.outcome(s);
                }
                WaitStep::Try(TryStep::KernelTry {
                    handle: KernelHandle::RoleHandle,
                }) => {
                    assert!(fast, "a zero wait reaches the kernel while {b:?} is busy");
                    self.hit("wait-zero-kernel");
                    self.seats[s].st = SeatState::Trying { b, wait: None };
                }
                other => panic!("T3 with a passed deadline returned {other:?}"),
            }
            return;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        match step {
            WaitStep::KernelTry {
                handle: KernelHandle::RoleHandle,
            } => {
                assert!(fast, "T3 fast path while {b:?} is busy");
                self.hit("wait-fast");
                self.seats[s].st = SeatState::Trying {
                    b,
                    wait: Some((deadline, seq)),
                };
            }
            WaitStep::WaitInTable => {
                assert!(!fast, "T3 queues although {b:?} is idle");
                self.hit("wait-queued");
                self.seats[s].st = SeatState::InTable {
                    b,
                    deadline,
                    seq,
                    handed: false,
                    failed: false,
                };
            }
            other => panic!("T3 returned {other:?}"),
        }
    }

    fn op_answer(&mut self, s: usize, coin: u8) {
        let SeatState::Trying { b, wait } = self.seats[s].st else {
            return;
        };
        let i = byte_index(b);
        let r = if coin.is_multiple_of(11) {
            Err(io_error())
        } else if self.foreign[i] || coin.is_multiple_of(5) {
            Ok(KernelResult::Busy)
        } else {
            self.kernel_grant(b);
            Ok(KernelResult::Granted)
        };
        let c = self.seats[s].id;
        match wait {
            None => {
                let (out, then) = self.t.end_try(c, b, r);
                match out {
                    Outcome::Granted(g) => {
                        self.hit("try-granted");
                        self.grant_to(s, g);
                    }
                    Outcome::Busy => self.outcome(s),
                    Outcome::Error(_) => {
                        self.hit("try-error");
                        self.outcome(s);
                    }
                }
                self.apply(then);
            }
            Some((deadline, seq)) => match self.t.end_wait_try(c, b, r, deadline, self.now) {
                WaitStep::Granted(g) => {
                    self.hit("wait-try-granted");
                    self.grant_to(s, g);
                }
                WaitStep::Busy { then } => {
                    assert!(
                        deadline <= self.now,
                        "T3b ends a wait as Busy before its deadline"
                    );
                    self.hit("wait-try-late-busy");
                    self.outcome(s);
                    self.apply(then);
                }
                WaitStep::DriveKernelWait => {
                    assert_eq!(self.mode(), WaitMode::CallerDriven);
                    assert!(
                        deadline > self.now,
                        "T3b starts a kernel wait after the deadline"
                    );
                    self.hit("drive");
                    self.seats[s].st = SeatState::Driving { b, deadline, seq };
                }
                WaitStep::StartWaiterThread => {
                    assert_eq!(self.mode(), WaitMode::WaiterThread);
                    assert!(
                        deadline > self.now,
                        "T3b starts a waiter thread after the deadline"
                    );
                    self.hit("waiter-thread");
                    self.seats[s].st = SeatState::InTable {
                        b,
                        deadline,
                        seq,
                        handed: false,
                        failed: false,
                    };
                }
                WaitStep::Error { then, .. } => {
                    self.hit("wait-try-error");
                    self.outcome(s);
                    self.apply(then);
                }
                other => panic!("T3b returned {other:?}"),
            },
        }
    }

    /// The outstanding kernel wait on `b` ends: a grant if no other process holds `b`, or an error.
    fn op_wait_ends(&mut self, b: LockByte, coin: u8) {
        let KernelState::Waiting(driver) = self.t.kernel_state(b) else {
            return;
        };
        let driver_seat = match driver {
            Driver::Caller(c) => {
                // A cancelling driver, or the thread of a dropped one, settles through `Settle`.
                let Some(k) = self.seats.iter().position(|s| s.id == c) else {
                    return;
                };
                if !matches!(self.seats[k].st, SeatState::Driving { .. }) {
                    return;
                }
                Some(k)
            }
            Driver::Thread => None,
        };
        if coin.is_multiple_of(7) {
            let f = self.t.kernel_failed(b, io_error());
            self.hit(if f.failed.is_some() {
                "kernel-failed"
            } else {
                "kernel-failed-abandoned"
            });
            let oldest = self.queued(b).first().map(|&(_, _, k)| k);
            assert_eq!(
                f.failed.map(|c| self.seat_of(c)),
                driver_seat.or(oldest),
                "the failure ends the served request"
            );
            if let Some(k) = f.failed.map(|c| self.seat_of(c)) {
                match driver_seat {
                    Some(d) => {
                        assert_eq!(k, d);
                        let c = self.seats[d].id;
                        assert!(matches!(
                            self.t.take_notice(c, b),
                            WaitStep::Error { then: None, .. }
                        ));
                        self.outcome(d);
                    }
                    None => {
                        let SeatState::InTable { failed, .. } = &mut self.seats[k].st else {
                            panic!("a failure for a seat that does not wait in the table")
                        };
                        *failed = true;
                    }
                }
            }
            self.apply(f.then);
            return;
        }
        if self.foreign[byte_index(b)] {
            return;
        }
        self.kernel_grant(b);
        self.hand_off(b, driver_seat);
    }

    /// Reports T4 for a granted kernel wait and follows the step, as the reporter (a driver seat or a waiter thread).
    fn hand_off(&mut self, b: LockByte, reporter: Option<usize>) {
        let step = self.t.kernel_granted(b, self.now);
        match step {
            GrantStep::HandTo(o) => {
                self.expect_hand_off(b, Some(o));
                let k = self.seat_of(o);
                if reporter == Some(k) {
                    self.hit("hand-to-driver");
                    match self.t.take_notice(o, b) {
                        WaitStep::Granted(g) => self.grant_to(k, g),
                        other => panic!("the driver's own hand-off returned {other:?}"),
                    }
                } else {
                    let SeatState::InTable { handed, .. } = &mut self.seats[k].st else {
                        panic!(
                            "a hand-off to a seat that does not wait in the table: {:?}",
                            self.seats[k].st
                        )
                    };
                    *handed = true;
                    self.hit("hand-to-waiter");
                    if let Some(d) = reporter {
                        // A driver that is not the oldest open client: its own deadline has passed.
                        self.driver_returns_busy(d, b);
                    }
                }
            }
            GrantStep::ReleaseNow { owner, then } => {
                self.expect_hand_off(b, None);
                self.hit("release-now");
                let expected = match self.mode() {
                    WaitMode::CallerDriven => Owner::RoleHandle,
                    WaitMode::WaiterThread => Owner::WaiterHandle,
                };
                assert_eq!(owner, expected);
                self.kernel_unlock(b);
                self.apply(then);
                if let Some(d) = reporter {
                    self.driver_returns_busy(d, b);
                }
            }
        }
    }

    /// A driver whose kernel wait was granted to another client, or released, returns `Busy` through T5.
    fn driver_returns_busy(&mut self, d: usize, b: LockByte) {
        let SeatState::Driving { deadline, .. } = self.seats[d].st else {
            panic!("not a driver")
        };
        assert!(
            deadline <= self.now,
            "a driver with an open deadline lost the grant"
        );
        let c = self.seats[d].id;
        match self.t.deadline_passed(c, b) {
            DeadlineStep::Busy { then } => {
                self.hit("driver-busy");
                self.outcome(d);
                self.apply(then);
            }
            other => panic!("the driver's T5 returned {other:?}"),
        }
    }

    fn op_wake(&mut self, s: usize) {
        let SeatState::InTable {
            b,
            deadline,
            seq,
            handed,
            failed,
        } = self.seats[s].st
        else {
            return;
        };
        let c = self.seats[s].id;
        if self.now >= deadline {
            match self.t.deadline_passed(c, b) {
                DeadlineStep::AlreadyGranted(g) => {
                    assert!(handed);
                    self.hit("already-granted");
                    self.grant_to(s, g);
                }
                DeadlineStep::Error(_) => {
                    assert!(failed);
                    self.hit("deadline-error");
                    self.outcome(s);
                }
                DeadlineStep::Busy { then } => {
                    assert!(!handed && !failed);
                    self.hit(if then.is_some() {
                        "deadline-passes-start-on"
                    } else {
                        "deadline-busy"
                    });
                    self.outcome(s);
                    self.apply(then);
                }
                DeadlineStep::CancelKernelWait => panic!("a table waiter was told to cancel"),
            }
            return;
        }
        match self.t.take_notice(c, b) {
            WaitStep::WaitInTable => assert!(!handed && !failed),
            WaitStep::Granted(g) => {
                assert!(handed);
                self.hit("notice-granted");
                self.grant_to(s, g);
            }
            WaitStep::KernelTry {
                handle: KernelHandle::RoleHandle,
            } => {
                self.hit("notice-start");
                self.seats[s].st = SeatState::Trying {
                    b,
                    wait: Some((deadline, seq)),
                };
            }
            WaitStep::DriveKernelWait => {
                assert_eq!(self.mode(), WaitMode::CallerDriven);
                self.hit("notice-new-driver");
                self.seats[s].st = SeatState::Driving { b, deadline, seq };
            }
            WaitStep::Error { then, .. } => {
                assert!(failed);
                self.hit("notice-error");
                self.outcome(s);
                self.apply(then);
            }
            other => panic!("take_notice returned {other:?}"),
        }
    }

    fn op_timeout(&mut self, s: usize) {
        match self.seats[s].st {
            SeatState::InTable { deadline, .. } => {
                self.now = self.now.max(deadline);
                self.op_wake(s);
            }
            SeatState::Driving { b, deadline, .. } => {
                self.now = self.now.max(deadline);
                let c = self.seats[s].id;
                assert_eq!(self.t.deadline_passed(c, b), DeadlineStep::CancelKernelWait);
                self.hit("cancel");
                self.seats[s].st = SeatState::Cancelling { b };
            }
            _ => {}
        }
    }

    /// A cancelling driver settles its cancelled wait and returns `Busy`; if seat `s` is not cancelling, the thread of a
    /// dropped driver settles the wait it left instead.
    fn op_settle(&mut self, s: usize, coin: u8) {
        if let SeatState::Cancelling { b } = self.seats[s].st {
            assert_eq!(
                self.t.kernel_state(b),
                KernelState::Waiting(Driver::Caller(self.seats[s].id))
            );
            self.settle(b, coin);
            self.outcome(s);
        } else if !self.droppers.is_empty() {
            self.settle_dropped(s % self.droppers.len(), coin);
        }
    }

    /// The thread of a dropped driver settles the kernel wait `droppers[k]` it cancelled at T0.
    fn settle_dropped(&mut self, k: usize, coin: u8) {
        let b = self.droppers.swap_remove(k);
        let KernelState::Waiting(Driver::Caller(d)) = self.t.kernel_state(b) else {
            panic!("a dropped driver's wait on {b:?} is not in flight")
        };
        assert!(
            self.seats.iter().all(|s| s.id != d),
            "the dropped driver is unregistered"
        );
        self.hit("drop-settle");
        self.settle(b, coin);
    }

    /// Settles a cancelled caller-driven wait on `b` whose driver no longer queues (its deadline passed, or it was
    /// dropped): the grant raced the cancel (T4), the cancel won (T6, error 995), or the settle failed with another error
    /// (`kernel_failed`, which then ends nobody's request).
    fn settle(&mut self, b: LockByte, coin: u8) {
        match coin % 3 {
            0 if !self.foreign[byte_index(b)] => {
                self.hit("settle-raced");
                self.kernel_grant(b);
                self.hand_off(b, None);
            }
            2 => {
                let f = self.t.kernel_failed(b, io_error());
                assert_eq!(
                    f.failed, None,
                    "a settle error after the driver stopped queueing ends no request"
                );
                self.hit("settle-failed");
                self.apply(f.then);
            }
            _ => match self.t.kernel_cancelled(b) {
                CancelStep::NewDriver(o) => {
                    let oldest = self.queued(b).first().map(|&(_, _, k)| self.seats[k].id);
                    assert_eq!(
                        oldest,
                        Some(o),
                        "T6 passes the drive to the oldest queued client"
                    );
                    self.hit("settle-new-driver");
                }
                CancelStep::Idle => {
                    assert!(self.queued(b).is_empty());
                    self.hit("settle-idle");
                }
            },
        }
    }

    fn op_release(&mut self, s: usize, k: usize) {
        if !matches!(self.seats[s].st, SeatState::Idle) || self.seats[s].grants.is_empty() {
            return;
        }
        let held = self.seats[s].grants.len();
        let g = self.seats[s].grants.swap_remove(k % held);
        let (b, c) = (g.byte(), self.seats[s].id);
        let step = self.t.release(c, g);
        assert_eq!(step.byte, b);
        let expected_owner = match (b.is_role(), self.mode()) {
            (false, _) => vec![Owner::SlotHandle(c)],
            (true, WaitMode::CallerDriven) => vec![Owner::RoleHandle],
            (true, WaitMode::WaiterThread) => vec![Owner::RoleHandle, Owner::WaiterHandle],
        };
        assert!(
            expected_owner.contains(&step.owner),
            "release owner {:?}",
            step.owner
        );
        self.hit(if step.then.is_some() {
            "release-starts-waiter"
        } else {
            "release"
        });
        self.kernel_unlock(b);
        self.apply(step.then);
    }

    /// T0 in any state: the client is dropped (its thread unwinds) whatever it was doing. A try in flight may or may not
    /// have been granted in the kernel (`coin`); the unlock T0 emits for it clears the byte either way, its error ignored.
    /// A caller-driven kernel wait that the client drove, or had begun to cancel, is cancelled by the dropping thread,
    /// which settles it at a later `Settle`.
    fn op_drop(&mut self, s: usize, coin: u8) {
        let c = self.seats[s].id;
        let (tried, driven) = match self.seats[s].st {
            SeatState::Idle | SeatState::InTable { .. } => (None, None),
            SeatState::Trying { b, .. } => (Some(b), None),
            SeatState::Driving { b, .. } | SeatState::Cancelling { b } => (None, Some(b)),
        };
        if let Some(b) = tried {
            self.hit("drop-trying");
            if coin.is_multiple_of(2) && !self.foreign[byte_index(b)] {
                self.kernel_grant(b);
            }
        }
        self.seats[s].st = SeatState::Idle;
        if let Some(r) = self.seats[s].req.take() {
            self.abandoned.insert(r);
        }
        self.seats[s].grants.clear();
        self.hit("drop");
        let mut cancelled = None;
        for step in self.t.unregister(c) {
            match step {
                Step::KernelUnlock { byte, .. } if Some(byte) == tried => {
                    // The unlock after an interrupted try: it fails harmlessly if the try was not granted.
                    self.ours[byte_index(byte)] = false;
                }
                Step::CancelKernelWait { byte } => {
                    assert_eq!(
                        Some(byte),
                        driven,
                        "T0 cancels only the kernel wait the client drove"
                    );
                    assert!(cancelled.replace(byte).is_none(), "one cancel per driver");
                    self.droppers.push(byte);
                }
                step => self.apply(Some(step)),
            }
        }
        assert_eq!(cancelled, driven, "T0 of a driver cancels its kernel wait");
        if driven.is_some() {
            self.hit("drop-driver");
        }
        self.seats[s].id = self.t.register(LockMode::Acquire);
    }

    fn check(&self) {
        check_table(&self.t);
        for (i, b) in all_bytes().into_iter().enumerate() {
            assert!(
                !(self.ours[i] && self.foreign[i]),
                "kernel exclusivity on {b:?}"
            );
            assert_eq!(
                self.ours[i],
                matches!(self.t.kernel_state(b), KernelState::Held(_)),
                "I-L1/I-L7 on {b:?}"
            );
            // The table's holder is the seat that holds a grant for it or was handed it.
            let holder = self.seats.iter().find(|s| {
                s.grants.iter().any(|g| g.byte() == b)
                    || matches!(s.st, SeatState::InTable { b: x, handed: true, .. } if x == b)
            });
            assert_eq!(self.t.holder(b), holder.map(|s| s.id), "holder of {b:?}");
        }
        for b in [LockByte::Writer, LockByte::Flush] {
            let table: Vec<ClientId> = self.t.roles[byte_index(b)]
                .queue
                .iter()
                .map(|w| w.client)
                .collect();
            let model: Vec<ClientId> = self
                .queued(b)
                .iter()
                .map(|&(_, _, k)| self.seats[k].id)
                .collect();
            assert_eq!(table, model, "the queue of {b:?}");
        }
    }

    /// Ends every request: answers tries, lets every deadline pass, settles cancels, releases every grant, lets other
    /// processes go and completes abandoned waiter threads, until nothing is in flight.
    fn drain(&mut self) {
        for _ in 0..64 {
            self.foreign = [false; 7];
            self.now = self.now.max(
                self.seats
                    .iter()
                    .filter_map(|s| deadline_of(&s.st))
                    .max()
                    .unwrap_or(0),
            );
            for s in 0..self.seats.len() {
                match self.seats[s].st {
                    SeatState::Trying { .. } => self.run(Op::Answer(s as u8, 1)),
                    SeatState::InTable { .. } => self.run(Op::Wake(s as u8)),
                    SeatState::Driving { .. } => self.run(Op::Timeout(s as u8)),
                    SeatState::Cancelling { .. } => self.run(Op::Settle(s as u8, 1)),
                    SeatState::Idle => {
                        while !self.seats[s].grants.is_empty() {
                            self.run(Op::Release(s as u8, 0));
                        }
                    }
                }
            }
            for b in [LockByte::Writer, LockByte::Flush] {
                if self.t.kernel_state(b) == KernelState::Waiting(Driver::Thread) {
                    self.run(Op::WaitEnds(if b == LockByte::Writer { 0 } else { 1 }, 1));
                }
            }
            while !self.droppers.is_empty() {
                self.settle_dropped(0, 1);
                self.check();
            }
            let quiet = self.droppers.is_empty()
                && self
                    .seats
                    .iter()
                    .all(|s| matches!(s.st, SeatState::Idle) && s.grants.is_empty())
                && all_bytes()
                    .into_iter()
                    .all(|b| self.t.kernel_state(b) == KernelState::Idle);
            if quiet {
                break;
            }
        }
        // I-L6 and I-L9: every request ended exactly once; nothing is held, queued or in flight.
        for r in 0..self.next_req {
            if !self.abandoned.contains(&r) {
                assert_eq!(
                    self.outcomes.get(&r),
                    Some(&1),
                    "request {r} did not end exactly once"
                );
            }
        }
        for b in all_bytes() {
            assert_eq!(
                self.t.kernel_state(b),
                KernelState::Idle,
                "{b:?} still busy after the drain"
            );
            assert_eq!(self.t.holder(b), None);
        }
        assert!(self.t.roles.iter().all(|r| r.queue.is_empty()) && self.t.slots.is_empty());
        assert!(!self.t.holds_any_role());
    }
}

fn deadline_of(st: &SeatState) -> Option<u64> {
    match *st {
        SeatState::InTable { deadline, .. } | SeatState::Driving { deadline, .. } => Some(deadline),
        SeatState::Trying {
            wait: Some((deadline, _)),
            ..
        } => Some(deadline),
        _ => None,
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn random_interleavings_keep_every_invariant(
        mode_i in 0usize..2,
        seats in 2usize..5,
        ops in proptest::collection::vec(op(), 0..160),
    ) {
        let mut m = Model::new(MODES[mode_i], seats);
        for op in ops {
            m.run(op);
        }
        m.drain();
    }
}

/// The interleaving model reaches every transition path of the table in both wait modes, so the property above is not
/// vacuous: a fixed-seed run of the same operation mix, with each path's count printed under `--nocapture`.
#[test]
fn interleavings_reach_every_path() {
    let common = [
        "try-busy",
        "try-kernel",
        "try-granted",
        "try-error",
        "wait-fast",
        "wait-queued",
        "wait-zero-busy",
        "wait-zero-kernel",
        "wait-try-granted",
        "wait-try-error",
        "hand-to-waiter",
        "release-now",
        "already-granted",
        "deadline-busy",
        "deadline-passes-start-on",
        "notice-granted",
        "notice-start",
        "release",
        "release-starts-waiter",
        "drop",
        "drop-trying",
        "kernel-failed",
        "wait-try-late-busy",
    ];
    let caller = [
        "drive",
        "hand-to-driver",
        "driver-busy",
        "cancel",
        "settle-raced",
        "settle-new-driver",
        "settle-idle",
        "settle-failed",
        "notice-new-driver",
        "drop-driver",
        "drop-settle",
    ];
    let thread = [
        "waiter-thread",
        "notice-error",
        "deadline-error",
        "kernel-failed-abandoned",
    ];
    for mode in MODES {
        let mut hits: HashMap<&'static str, u32> = HashMap::new();
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for round in 0..400 {
            let mut m = Model::new(mode, 2 + round % 3);
            for _ in 0..160 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let [k, a, b, c, ..] = x.to_le_bytes();
                m.run(op_from(k, a, b, c));
            }
            m.drain();
            for (path, n) in m.hits {
                *hits.entry(path).or_insert(0) += n;
            }
        }
        let mut sorted: Vec<_> = hits.iter().collect();
        sorted.sort();
        println!("{mode:?}: {sorted:?}");
        let specific: &[&str] = match mode {
            WaitMode::CallerDriven => &caller,
            WaitMode::WaiterThread => &thread,
        };
        for path in common.iter().chain(specific) {
            assert!(
                hits.get(path).copied().unwrap_or(0) > 0,
                "{mode:?}: path {path} never taken"
            );
        }
    }
}
