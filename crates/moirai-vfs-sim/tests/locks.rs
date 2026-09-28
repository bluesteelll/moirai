//! The lock bytes: the real grant table per simulated process ([OS/lock §5.1]), several clients in one process, both
//! wait modes, and release after a death (FM-8.1, FM-11.1).

mod common;

use std::path::Path;

use common::*;
use moirai_vfs::{
    Acquired, LockByte, LockError, LockMode, Locks, ProbeResult, RootAccess, RootRole, ShareRetry,
    StoreFs, VfsErrorKind, WaitMode,
};
use moirai_vfs_sim::{DeathPlan, EventKind, SimConfig, SimWorld, Site};

fn world_in(mode: WaitMode, seed: u64) -> SimWorld {
    let mut cfg = SimConfig::new(seed);
    cfg.wait_mode = Some(mode);
    let w = SimWorld::new(cfg);
    w.mkdir_all(Path::new(STORE));
    let (d, r) = proc(&w, "init");
    durable_file(&d, &r, "LOCK", &[0u8; 36 * 1024]);
    w
}

const MODES: [WaitMode; 2] = [WaitMode::CallerDriven, WaitMode::WaiterThread];

#[test]
fn two_clients_of_one_process_conflict_before_any_kernel_call() {
    for mode in MODES {
        let w = world_in(mode, 20);
        assert_eq!(w.wait_mode(), mode);
        let (v, r) = proc(&w, "p");
        let mut c1 = v.lock_client(&r, LockMode::Acquire).unwrap();
        let mut c2 = v.lock_client(&r, LockMode::Acquire).unwrap();
        let Acquired::Granted(g) = v.try_acquire(&mut c1, LockByte::Writer).unwrap() else {
            panic!("free at first");
        };
        let kernel_tries = |w: &SimWorld| {
            w.trace()
                .iter()
                .filter(|e| e.kind == EventKind::LockTry)
                .count()
        };
        let before = kernel_tries(&w);
        assert_eq!(
            v.try_acquire(&mut c2, LockByte::Writer).unwrap(),
            Acquired::Busy
        );
        assert_eq!(
            v.acquire_within(&mut c2, LockByte::Writer, 0).unwrap(),
            Acquired::Busy
        );
        assert_eq!(
            kernel_tries(&w),
            before,
            "decided in the grant table ([OS/lock §3] item 2)"
        );
        assert_eq!(v.probe(&c2, LockByte::Writer), ProbeResult::Held);
        assert!(v.holds(&c1, LockByte::Writer) && !v.holds(&c2, LockByte::Writer));
        assert!(v.holds_any_role());
        // A bounded in-process wait times out on the virtual clock.
        assert_eq!(
            v.acquire_within(&mut c2, LockByte::Writer, 50).unwrap(),
            Acquired::Busy
        );
        v.release(&mut c1, g);
        let Acquired::Granted(g2) = v.try_acquire(&mut c2, LockByte::Writer).unwrap() else {
            panic!("released");
        };
        v.release(&mut c2, g2);
        assert!(!v.holds_any_role());
    }
}

#[test]
fn an_in_process_waiter_gets_the_byte_when_the_holder_releases() {
    for mode in MODES {
        for seed in 0..8 {
            let w = world_in(mode, seed);
            let p = w.process_with("p", None, Some(true));
            let body = |v: moirai_vfs_sim::SimVfs| {
                let root = v
                    .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
                    .unwrap();
                let mut c = v.lock_client(&root, LockMode::Acquire).unwrap();
                let Acquired::Granted(g) =
                    v.acquire_within(&mut c, LockByte::Writer, 60_000).unwrap()
                else {
                    return false;
                };
                // Hold it across a few calls, so the other client queues.
                for _ in 0..3 {
                    v.file_size(v.lock_data(&c)).unwrap();
                }
                v.release(&mut c, g);
                true
            };
            let t1 = w.spawn(&p, body);
            let t2 = w.spawn(&p, body);
            assert!(!w.run().deadlock);
            assert!(t1.end().unwrap().unwrap(), "{mode:?} seed {seed}");
            assert!(t2.end().unwrap().unwrap(), "{mode:?} seed {seed}");
        }
    }
}

#[test]
fn a_waiter_in_another_process_gets_a_dead_holders_byte_after_the_measured_delay() {
    for mode in MODES {
        let w = world_in(mode, 21);
        let (a, ra) = proc(&w, "a");
        let mut ca = a.lock_client(&ra, LockMode::Acquire).unwrap();
        assert!(matches!(
            a.try_acquire(&mut ca, LockByte::Flush).unwrap(),
            Acquired::Granted(_)
        ));
        // Class (a): the measured distribution (≤ 32 ms by the prior evidence, [F15] HOLE(F15-lock-release)).
        w.kill(
            &a,
            DeathPlan {
                release_class: Some(0),
                ..DeathPlan::default()
            },
        );
        let (b, rb) = proc(&w, "b");
        let mut cb = b.lock_client(&rb, LockMode::Acquire).unwrap();
        let t0 = b.mono_ns_now();
        let Acquired::Granted(g) = b.acquire_within(&mut cb, LockByte::Flush, 2_000).unwrap()
        else {
            panic!("{mode:?}: the delay is below the wait bound");
        };
        assert!(b.mono_ns_now() - t0 <= 40_000_000);
        b.release(&mut cb, g);
    }
}

#[test]
fn a_wait_that_breaks_the_lock_order_is_a_programming_error() {
    let w = world_in(WaitMode::CallerDriven, 22);
    let (v, r) = proc(&w, "p");
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let Acquired::Granted(_g) = v.try_acquire(&mut c, LockByte::Writer).unwrap() else {
        panic!("free");
    };
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = v.acquire_within(&mut c, LockByte::Flush, 10);
    }));
    let msg = r.unwrap_err();
    let text = msg
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| msg.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap();
    assert!(text.starts_with("grant table:"), "{text}");
}

trait MonoNow {
    fn mono_ns_now(&self) -> u64;
}

impl MonoNow for moirai_vfs_sim::SimVfs {
    fn mono_ns_now(&self) -> u64 {
        moirai_vfs::Clock::mono_ns(self)
    }
}

/// [OS/lock §9.1] step 4 (review regression): every later `LOCK` handle is identity-checked when opened. After `LOCK` is
/// replaced (FM-10.3) between `lock_client` and the table's first kernel try, the try fails with `IdentityMismatch` and a
/// probe through a new probe handle answers `Unknown`; a role handle opened before the replacement is not reopened.
#[test]
fn a_replaced_lock_fails_the_first_handle_opened_after_it() {
    for mode in MODES {
        let w = world_in(mode, 23);
        let lock = Path::new(STORE).join("LOCK");
        let (a, ra) = proc(&w, "a");
        let mut early = a.lock_client(&ra, LockMode::Acquire).unwrap();
        let Acquired::Granted(g) = a.try_acquire(&mut early, LockByte::Writer).unwrap() else {
            panic!("free");
        };
        a.release(&mut early, g);
        let (b, rb) = proc(&w, "b");
        let mut late = b.lock_client(&rb, LockMode::Acquire).unwrap();
        w.external_replace(&lock, &[0u8; 36 * 1024]).unwrap();
        assert_eq!(
            b.try_acquire(&mut late, LockByte::Writer).unwrap_err(),
            LockError::IdentityMismatch
        );
        assert_eq!(
            b.acquire_within(&mut late, LockByte::Flush, 100)
                .unwrap_err(),
            LockError::IdentityMismatch
        );
        assert_eq!(b.probe(&late, LockByte::Writer), ProbeResult::Unknown);
        assert!(w.trace().iter().any(|e| e.kind == EventKind::LockIdentity));
        // The role handle `a` opened before the replacement still locks the old file.
        let Acquired::Granted(g) = a.try_acquire(&mut early, LockByte::Writer).unwrap() else {
            panic!("the old file's byte is free");
        };
        // A client opened after the replacement locks the new file, independently.
        let (c, rc) = proc(&w, "c");
        let mut fresh = c.lock_client(&rc, LockMode::Acquire).unwrap();
        assert!(matches!(
            c.try_acquire(&mut fresh, LockByte::Writer).unwrap(),
            Acquired::Granted(_)
        ));
        a.release(&mut early, g);
    }
}

/// FM-8.3 (review regression): a delete-pending `LOCK` cannot be opened by a new client.
#[test]
fn a_delete_pending_lock_refuses_new_clients() {
    let w = world_in(WaitMode::CallerDriven, 24);
    let (a, ra) = proc(&w, "a");
    let c = a.lock_client(&ra, LockMode::Probe).unwrap();
    w.queue_choice_for(&a, Site::DeletePending, 1);
    a.unlink(&ra, common::rel("LOCK"), ShareRetry::None)
        .unwrap();
    let (b, rb) = proc(&w, "b");
    let e = b.lock_client(&rb, LockMode::Probe).unwrap_err();
    assert!(
        matches!(e, LockError::Io(ref v) if v.kind == VfsErrorKind::DeletePending),
        "{e:?}"
    );
    drop(c);
    assert_eq!(
        b.lock_client(&rb, LockMode::Probe).unwrap_err(),
        LockError::NoLockFile
    );
}
