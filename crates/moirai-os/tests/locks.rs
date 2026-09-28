//! `Locks` on real NTFS ([OS/lock]; X-F4): the in-process two-client case, hand-offs in the table, caller-driven kernel
//! waits against another process (granted, timed out and cancelled, handed to the oldest waiter), release at a
//! holder's death, slots, probes, the identity check, foreign locks and the programming-error panics.

#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{KillOnDrop, TempDir};
use moirai_os::{OsRoot, OsVfs};
use moirai_vfs::{
    Acquired, LockByte, LockError, LockMode, Locks, ProbeResult, RelPath, RootAccess, RootRole,
    ShareRetry, SlotIndex, StoreFs,
};
use windows_sys::Win32::System::IO::{OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0};

const LOCK: RelPath<'static> = RelPath::literal("LOCK");

/// A store root with a 36 KiB `LOCK`.
fn store(dir: &Path) -> OsRoot {
    let v = OsVfs;
    let r = v
        .open_root(dir, RootRole::Store, RootAccess::ReadWrite)
        .unwrap();
    if v.path_identity(&r, LOCK).is_err() {
        let f = v.create_new(&r, LOCK).unwrap();
        v.write_at(&f, 0, &[0u8; 36 * 1024]).unwrap();
    }
    r
}

fn granted(a: Acquired) -> moirai_vfs::Grant {
    match a {
        Acquired::Granted(g) => g,
        Acquired::Busy => panic!("expected a grant"),
    }
}

fn slot(i: u16) -> LockByte {
    LockByte::Slot(SlotIndex::new(i).unwrap())
}

#[test]
fn lock_client_refusals() {
    let t = TempDir::new("lockref");
    let v = OsVfs;
    let r = v
        .open_root(t.path(), RootRole::Store, RootAccess::ReadWrite)
        .unwrap();
    assert_eq!(
        v.lock_client(&r, LockMode::Acquire).unwrap_err(),
        LockError::NoLockFile
    );
    drop(store(t.path()));
    let ro = v
        .open_root(t.path(), RootRole::Store, RootAccess::Read)
        .unwrap();
    assert!(matches!(
        v.lock_client(&ro, LockMode::Acquire).unwrap_err(),
        LockError::AccessDenied { .. }
    ));
    let p = v.lock_client(&ro, LockMode::Probe).unwrap();
    assert_eq!(v.probe(&p, LockByte::Writer), ProbeResult::Free);
    assert_eq!(v.foreign_lock_check(&p), ProbeResult::Free);
    let mut head = [0u8; 16];
    v.read_exact_at(v.lock_data(&p), 0, &mut head).unwrap();
}

#[test]
fn two_clients_in_one_process_conflict_before_the_kernel() {
    let t = TempDir::new("twoclients");
    let v = OsVfs;
    let r = store(t.path());
    // The second client comes through another root value of the same directory: one table per `LOCK` identity.
    let r2 = v
        .open_root(t.path(), RootRole::Store, RootAccess::ReadWrite)
        .unwrap();
    let mut c1 = v.lock_client(&r, LockMode::Acquire).unwrap();
    let mut c2 = v.lock_client(&r2, LockMode::Acquire).unwrap();
    let g = granted(v.try_acquire(&mut c1, LockByte::Writer).unwrap());
    assert!(v.holds(&c1, LockByte::Writer) && !v.holds(&c2, LockByte::Writer));
    assert!(v.holds_any_role());
    assert_eq!(
        v.try_acquire(&mut c2, LockByte::Writer).unwrap(),
        Acquired::Busy
    );
    assert_eq!(v.probe(&c2, LockByte::Writer), ProbeResult::Held);
    let start = Instant::now();
    assert_eq!(
        v.acquire_within(&mut c2, LockByte::Writer, 150).unwrap(),
        Acquired::Busy
    );
    assert!(
        start.elapsed() >= Duration::from_millis(140),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(
        v.acquire_within(&mut c2, LockByte::Writer, 0).unwrap(),
        Acquired::Busy,
        "0 ms is a try"
    );
    v.release(&mut c1, g);
    assert!(!v.holds(&c1, LockByte::Writer));
    let g2 = granted(v.try_acquire(&mut c2, LockByte::Writer).unwrap());
    assert_eq!(v.probe(&c1, LockByte::Writer), ProbeResult::Held);
    v.release(&mut c2, g2);
    assert_eq!(v.probe(&c1, LockByte::Writer), ProbeResult::Free);
}

#[test]
fn a_waiter_in_the_table_gets_the_byte_on_release() {
    let t = TempDir::new("handoff");
    let v = OsVfs;
    let r = store(t.path());
    let mut c1 = v.lock_client(&r, LockMode::Acquire).unwrap();
    let mut c2 = v.lock_client(&r, LockMode::Acquire).unwrap();
    let g = granted(v.try_acquire(&mut c1, LockByte::Flush).unwrap());
    let holder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        OsVfs.release(&mut c1, g);
        c1
    });
    let start = Instant::now();
    let g2 = granted(v.acquire_within(&mut c2, LockByte::Flush, 5_000).unwrap());
    assert!(start.elapsed() >= Duration::from_millis(80));
    let c1 = holder.join().unwrap();
    assert_eq!(v.probe(&c1, LockByte::Flush), ProbeResult::Held);
    v.release(&mut c2, g2);
}

#[test]
fn slots_are_tried_and_probed() {
    let t = TempDir::new("slots");
    let v = OsVfs;
    let r = store(t.path());
    let mut c1 = v.lock_client(&r, LockMode::Acquire).unwrap();
    let mut c2 = v.lock_client(&r, LockMode::Acquire).unwrap();
    let g = granted(v.try_acquire(&mut c1, slot(3)).unwrap());
    assert!(v.holds(&c1, slot(3)) && !v.holds(&c1, LockByte::Writer));
    assert_eq!(v.try_acquire(&mut c2, slot(3)).unwrap(), Acquired::Busy);
    assert_eq!(v.probe(&c2, slot(3)), ProbeResult::Held);
    let g4 = granted(v.try_acquire(&mut c2, slot(4)).unwrap());
    v.release(&mut c1, g);
    assert_eq!(v.probe(&c2, slot(3)), ProbeResult::Free);
    let g3 = granted(v.try_acquire(&mut c2, slot(3)).unwrap());
    // Dropping a client releases its grants (T0).
    drop(c2);
    let _ = (g3, g4);
    assert_eq!(v.probe(&c1, slot(3)), ProbeResult::Free);
    assert_eq!(v.probe(&c1, slot(4)), ProbeResult::Free);
}

#[test]
fn dropping_a_client_releases_its_role_bytes() {
    let t = TempDir::new("droprel");
    let v = OsVfs;
    let r = store(t.path());
    let mut c1 = v.lock_client(&r, LockMode::Acquire).unwrap();
    let gf = granted(v.try_acquire(&mut c1, LockByte::Flush).unwrap());
    let gw = granted(v.acquire_within(&mut c1, LockByte::Writer, 1_000).unwrap());
    let mut c2 = v.lock_client(&r, LockMode::Acquire).unwrap();
    assert_eq!(
        v.try_acquire(&mut c2, LockByte::Writer).unwrap(),
        Acquired::Busy
    );
    drop(c1);
    let _ = (gf, gw);
    let g = granted(v.try_acquire(&mut c2, LockByte::Writer).unwrap());
    v.release(&mut c2, g);
}

#[test]
#[should_panic(expected = "Probe mode")]
fn probe_mode_clients_cannot_acquire() {
    let t = TempDir::new("probemode");
    let v = OsVfs;
    let r = store(t.path());
    let mut p = v.lock_client(&r, LockMode::Probe).unwrap();
    let _ = v.try_acquire(&mut p, LockByte::Writer);
}

#[test]
#[should_panic(expected = "reentrant")]
fn reentrant_acquisition_panics() {
    let t = TempDir::new("reentrant");
    let v = OsVfs;
    let r = store(t.path());
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let _g = v.try_acquire(&mut c, LockByte::Maintenance).unwrap();
    let _ = v.try_acquire(&mut c, LockByte::Maintenance);
}

#[test]
#[should_panic(expected = "lock order")]
fn waiting_against_the_lock_order_panics() {
    let t = TempDir::new("order");
    let v = OsVfs;
    let r = store(t.path());
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let _g = v.try_acquire(&mut c, LockByte::Writer).unwrap();
    let _ = v.acquire_within(&mut c, LockByte::Flush, 10);
}

#[test]
#[should_panic(expected = "not waitable")]
fn waiting_for_a_try_only_byte_panics() {
    let t = TempDir::new("waitable");
    let v = OsVfs;
    let r = store(t.path());
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let _ = v.acquire_within(&mut c, LockByte::Maintenance, 10);
}

/// Starts a child that holds the writer byte of the store at `dir` for `ms` (0 = until killed) and waits until it
/// holds it. The guard kills and reaps the child if the test fails before its own wait.
fn holder(dir: &Path, ms: u64, tag: &str) -> KillOnDrop {
    let ready = dir.join(format!("ready.{tag}"));
    let child = KillOnDrop::new(
        common::child_command("child", "hold-writer")
            .env("MOIRAI_OS_TEST_DIR", dir)
            .env("MOIRAI_OS_TEST_MS", ms.to_string())
            .env("MOIRAI_OS_TEST_READY", &ready)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(
        common::wait_for_file(&ready, 30_000),
        "the holder never became ready"
    );
    child
}

#[test]
fn a_kernel_wait_times_out_then_is_granted() {
    let t = TempDir::new("xproc");
    let v = OsVfs;
    let r = store(t.path());
    let mut child = holder(t.path(), 600, "a");
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let p = v.lock_client(&r, LockMode::Probe).unwrap();
    assert_eq!(v.probe(&p, LockByte::Writer), ProbeResult::Held);
    assert_eq!(
        v.try_acquire(&mut c, LockByte::Writer).unwrap(),
        Acquired::Busy
    );
    // A driven kernel wait whose deadline passes: cancelled and settled, `Busy`.
    let start = Instant::now();
    assert_eq!(
        v.acquire_within(&mut c, LockByte::Writer, 120).unwrap(),
        Acquired::Busy
    );
    let e = start.elapsed();
    assert!(
        e >= Duration::from_millis(110) && e < Duration::from_millis(1_500),
        "{e:?}"
    );
    // A driven kernel wait that the holder's release completes.
    let g = granted(v.acquire_within(&mut c, LockByte::Writer, 10_000).unwrap());
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert_eq!(
        v.probe(&p, LockByte::Writer),
        ProbeResult::Held,
        "held in this process now"
    );
    v.release(&mut c, g);
    assert_eq!(v.probe(&p, LockByte::Writer), ProbeResult::Free);
}

#[test]
fn a_kernel_grant_goes_to_the_oldest_waiter() {
    let t = TempDir::new("oldest");
    let v = OsVfs;
    let r = store(t.path());
    let mut child = holder(t.path(), 500, "b");
    let mut older = v.lock_client(&r, LockMode::Acquire).unwrap();
    let mut younger = v.lock_client(&r, LockMode::Acquire).unwrap();
    let first = std::thread::spawn(move || {
        let g = granted(
            OsVfs
                .acquire_within(&mut older, LockByte::Writer, 20_000)
                .unwrap(),
        );
        let at = Instant::now();
        std::thread::sleep(Duration::from_millis(100));
        OsVfs.release(&mut older, g);
        at
    });
    std::thread::sleep(Duration::from_millis(100));
    let g = granted(
        v.acquire_within(&mut younger, LockByte::Writer, 20_000)
            .unwrap(),
    );
    let younger_at = Instant::now();
    let older_at = first.join().unwrap();
    assert!(older_at < younger_at, "the oldest waiter was served first");
    v.release(&mut younger, g);
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

#[test]
fn a_killed_holder_releases_its_bytes() {
    let t = TempDir::new("killed");
    let v = OsVfs;
    let r = store(t.path());
    let mut child = holder(t.path(), 0, "c");
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    assert_eq!(
        v.try_acquire(&mut c, LockByte::Writer).unwrap(),
        Acquired::Busy
    );
    child.kill().unwrap();
    child.wait().unwrap();
    let g = granted(v.acquire_within(&mut c, LockByte::Writer, 5_000).unwrap());
    v.release(&mut c, g);
}

#[test]
fn a_replaced_lock_is_an_identity_mismatch() {
    let t = TempDir::new("replaced");
    let v = OsVfs;
    let r = store(t.path());
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    // Another tool replaces `LOCK` (the data handle shares delete, so the rename succeeds).
    v.rename_noreplace(&r, LOCK, &r, RelPath::literal("LOCK.old"), ShareRetry::None)
        .unwrap();
    drop(v.create_new(&r, LOCK).unwrap());
    assert_eq!(
        v.try_acquire(&mut c, LockByte::Writer).unwrap_err(),
        LockError::IdentityMismatch
    );
    assert_eq!(
        v.probe(&c, LockByte::Writer),
        ProbeResult::Unknown,
        "never Free"
    );
    // A new client sees the new `LOCK`, in a table of its own.
    let mut n = v.lock_client(&r, LockMode::Acquire).unwrap();
    let g = granted(v.try_acquire(&mut n, LockByte::Writer).unwrap());
    v.release(&mut n, g);
}

#[test]
fn a_foreign_whole_range_lock_is_detected() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFileEx,
    };
    let t = TempDir::new("foreign");
    let v = OsVfs;
    let r = store(t.path());
    let c = v.lock_client(&r, LockMode::Probe).unwrap();
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(t.join("LOCK"))
        .unwrap();
    let mut ov = OVERLAPPED::default();
    // SAFETY: the handle is valid; `ov` is a live local; the range is the whole file space (a `File::lock` equivalent).
    let ok = unsafe {
        LockFileEx(
            f.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut ov,
        )
    };
    assert_ne!(ok, 0);
    assert_eq!(v.foreign_lock_check(&c), ProbeResult::Held);
    assert_eq!(v.probe(&c, LockByte::Writer), ProbeResult::Held);
    let mut ov = OVERLAPPED::default();
    // SAFETY: as above; unlocks the same range.
    unsafe { UnlockFileEx(f.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut ov) };
    assert_eq!(v.foreign_lock_check(&c), ProbeResult::Free);
}

/// A `LockFileEx` request block at byte offset `b` with `event` (null: none).
fn overlapped_at(b: u64, event: windows_sys::Win32::Foundation::HANDLE) -> OVERLAPPED {
    OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 {
            Anonymous: OVERLAPPED_0_0 {
                Offset: b as u32,
                OffsetHigh: (b >> 32) as u32,
            },
        },
        hEvent: event,
    }
}

#[test]
fn fail_immediately_completes_at_once_on_ntfs() {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{
        ERROR_IO_PENDING, ERROR_LOCK_VIOLATION, GetLastError, HANDLE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OVERLAPPED, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
        UnlockFile,
    };
    use windows_sys::Win32::System::IO::GetOverlappedResult;
    use windows_sys::Win32::System::Threading::CreateEventW;
    // [OS/lock] open point 9 and §7.1 `KernelTry` [I]: a `FAIL_IMMEDIATELY` try on an overlapped handle may complete as
    // pending (997), which `lock_try` settles with `GetOverlappedResult(TRUE)`. This records which completion NTFS gives
    // for a grant and for a conflict, through handles opened as the role handles are (overlapped, with an event).
    let t = TempDir::new("failimm");
    drop(store(t.path()));
    let open = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .open(t.join("LOCK"))
            .unwrap()
    };
    let (a, b) = (open(), open());
    // SAFETY: no security attributes, manual reset, initially unsignalled, unnamed.
    let ev = unsafe { CreateEventW(core::ptr::null(), 1, 0, core::ptr::null()) };
    assert!(!ev.is_null());
    // SAFETY: `ev` is the event just created, owned here and closed once.
    let ev_owned = unsafe { OwnedHandle::from_raw_handle(ev) };
    let byte = moirai_vfs::ROLE_BASE + 20;
    let try_lock = |h: HANDLE| -> (&'static str, bool) {
        let mut ov = overlapped_at(byte, ev);
        // SAFETY: `h` is a valid overlapped handle; `ov` is live until the request is settled below.
        let ok = unsafe {
            LockFileEx(
                h,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut ov,
            )
        };
        if ok != 0 {
            return ("immediate", true);
        }
        // SAFETY: reads this thread's last error.
        match unsafe { GetLastError() } {
            ERROR_LOCK_VIOLATION => ("immediate", false),
            ERROR_IO_PENDING => {
                let mut n = 0u32;
                // SAFETY: waits for the pending request on `ov`, which stays live for the wait.
                let done = unsafe { GetOverlappedResult(h, &ov, &mut n, 1) };
                ("pending", done != 0)
            }
            e => panic!("LockFileEx failed with {e}"),
        }
    };
    let (grant, granted) = try_lock(a.as_raw_handle());
    let (busy, got) = try_lock(b.as_raw_handle());
    println!(
        "OBSERVED: LockFileEx(EXCLUSIVE | FAIL_IMMEDIATELY) on an overlapped NTFS handle completes a grant {grant} and a conflict {busy}"
    );
    assert!(granted && !got);
    assert_eq!(
        (grant, busy),
        ("immediate", "immediate"),
        "NTFS completes both at once; the pending branch stays for other file systems"
    );
    // SAFETY: `a` holds the byte; one byte at `byte`.
    let ok = unsafe { UnlockFile(a.as_raw_handle(), byte as u32, (byte >> 32) as u32, 1, 0) };
    assert_ne!(ok, 0);
    drop(ev_owned);
}

#[test]
fn two_quiet_class_bytes_tried_at_once_never_share_a_request_block() {
    // Each role byte has its own role handle, event and request block ([OS/lock §2], pass 1, P1-10): two try-only bytes
    // tried concurrently from two threads never arm one `OVERLAPPED`. Until WP-30 adds `QuietIndex`, the seam has one
    // quiet byte, so the second thread uses the other try-only role byte, `Maintenance`; with `Quiet(1)` and `Quiet(2)`
    // the test keeps its shape.
    let t = TempDir::new("quietpair");
    let r = store(t.path());
    let run = |byte: LockByte| {
        let r = r.clone();
        std::thread::spawn(move || {
            let v = OsVfs;
            let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
            for _ in 0..500 {
                let g = granted(v.try_acquire(&mut c, byte).unwrap());
                assert!(v.holds(&c, byte));
                v.release(&mut c, g);
            }
        })
    };
    let (a, b) = (run(LockByte::Quiet), run(LockByte::Maintenance));
    a.join().unwrap();
    b.join().unwrap();
    let v = OsVfs;
    let p = v.lock_client(&r, LockMode::Probe).unwrap();
    assert_eq!(v.probe(&p, LockByte::Quiet), ProbeResult::Free);
    assert_eq!(v.probe(&p, LockByte::Maintenance), ProbeResult::Free);
}

#[test]
fn the_drivers_deadline_passes_while_another_waiter_queues() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFile,
    };
    // A foreign handle in this process holds the writer byte in the kernel (a byte-range lock conflicts across handles),
    // so the table's first waiter drives a kernel wait. Its deadline passes while a second client queues with a long
    // one: T5 cancels, and either T6 makes the second client the new driver, or — when the release races the cancel or
    // lands at the driver's deadline — T4 hands the grant to the second client, which is not the driver. Whatever the
    // interleaving, the second client is granted, the driver ends `Busy` or `Granted`, and nothing leaks.
    let t = TempDir::new("newdriver");
    let v = OsVfs;
    let r = store(t.path());
    let w = LockByte::Writer.offset();
    let foreign = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(t.join("LOCK"))
        .unwrap();
    let fh = foreign.as_raw_handle();
    // Release offsets relative to the driver's deadline: well after (T6, then the new driver's own wait), and around it
    // (the races of T4 with T5).
    for (round, offset_ms) in [120i64, 0, -3, 3, 0, 6, -6, 1].into_iter().enumerate() {
        let mut ov = overlapped_at(w, core::ptr::null_mut());
        // SAFETY: `fh` is the foreign handle (synchronous); `ov` is a live local; one byte at the writer offset.
        let ok = unsafe {
            LockFileEx(
                fh,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut ov,
            )
        };
        assert_ne!(ok, 0, "round {round}: the foreign handle takes the byte");
        let driver_ms = 150u32;
        let mut driver = v.lock_client(&r, LockMode::Acquire).unwrap();
        let mut second = v.lock_client(&r, LockMode::Acquire).unwrap();
        let start = Instant::now();
        let d = std::thread::spawn(move || {
            let a = OsVfs
                .acquire_within(&mut driver, LockByte::Writer, driver_ms)
                .unwrap();
            let got = matches!(a, Acquired::Granted(_));
            if let Acquired::Granted(g) = a {
                OsVfs.release(&mut driver, g);
            }
            (got, driver)
        });
        std::thread::sleep(Duration::from_millis(30));
        let s = std::thread::spawn(move || {
            let a = OsVfs
                .acquire_within(&mut second, LockByte::Writer, 10_000)
                .unwrap();
            (a, second)
        });
        let at = (i64::from(driver_ms) + offset_ms).max(0) as u64;
        let elapsed = start.elapsed().as_millis() as u64;
        std::thread::sleep(Duration::from_millis(at.saturating_sub(elapsed)));
        // SAFETY: the foreign handle holds the byte; one byte at the writer offset.
        let ok = unsafe { UnlockFile(fh, w as u32, (w >> 32) as u32, 1, 0) };
        assert_ne!(ok, 0);
        let (driver_granted, driver) = d.join().unwrap();
        let (a, mut second) = s.join().unwrap();
        let g = granted(a);
        assert!(v.holds(&second, LockByte::Writer), "round {round}");
        assert!(!v.holds(&driver, LockByte::Writer), "round {round}");
        println!(
            "OBSERVED: round {round} (release at deadline {offset_ms:+} ms): driver granted {driver_granted}"
        );
        v.release(&mut second, g);
        let p = v.lock_client(&r, LockMode::Probe).unwrap();
        assert_eq!(
            v.probe(&p, LockByte::Writer),
            ProbeResult::Free,
            "round {round}: no grant leaked"
        );
    }
    drop(foreign);
}

#[test]
fn child() {
    let Some(mode) = common::child_mode() else {
        return;
    };
    if mode == "hold-writer" {
        let v = OsVfs;
        let dir = std::env::var_os("MOIRAI_OS_TEST_DIR").unwrap();
        let ms: u64 = std::env::var("MOIRAI_OS_TEST_MS").unwrap().parse().unwrap();
        let ready = std::env::var_os("MOIRAI_OS_TEST_READY").unwrap();
        let r = v
            .open_root(Path::new(&dir), RootRole::Store, RootAccess::ReadWrite)
            .unwrap();
        let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
        let g = granted(v.acquire_within(&mut c, LockByte::Writer, 30_000).unwrap());
        std::fs::write(&ready, b"1").unwrap();
        if ms == 0 {
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        std::thread::sleep(Duration::from_millis(ms));
        v.release(&mut c, g);
        std::process::exit(0);
    }
    std::process::exit(3);
}
