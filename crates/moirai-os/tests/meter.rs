//! `Meter` on Windows ([OS/mem], [OS/proc §9]): a running child's private bytes, threads and CPU time, and its peak
//! read after it exits, before its handle is dropped.

#![cfg(windows)]

mod common;

use moirai_os::OsMeter;
use moirai_vfs::{ChildTicket, Meter, PeakKind};

#[test]
fn a_childs_readings_and_peak() {
    let m = OsMeter;
    let ready = common::TempDir::new("meter");
    let marker = ready.join("allocated");
    let mut cmd = common::child_command("child", "alloc");
    cmd.env("MOIRAI_OS_TEST_READY", &marker)
        .stdout(std::process::Stdio::null());
    let ticket = m.prepare_child(&mut cmd).unwrap();
    assert_eq!(ticket, ChildTicket(0));
    let mut child = common::KillOnDrop::new(cmd.spawn().unwrap());
    m.bind_child(ticket, &child);
    assert!(common::wait_for_file(&marker, 30_000));
    let now = m.child_private_now(&child).unwrap();
    assert!(now >= 64 << 20, "{now}");
    assert!(m.child_threads(&child).unwrap() >= 1);
    let cpu = m.cpu_times(Some(&child)).unwrap();
    assert!(cpu.user_ns + cpu.kernel_ns > 0);
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let peak = m.peak_of_child(&child).unwrap();
    assert_eq!(peak.kind, PeakKind::Peak);
    assert!(peak.private_peak_bytes >= 64 << 20, "{peak:?}");
}

#[test]
fn child() {
    let Some(mode) = common::child_mode() else {
        return;
    };
    if mode == "alloc" {
        let mut v = vec![0u8; 64 << 20];
        for i in (0..v.len()).step_by(4096) {
            v[i] = 1;
        }
        // Some CPU time, so that the tick-granular process times are non-zero.
        let start = std::time::Instant::now();
        let mut x = 0u64;
        while start.elapsed().as_millis() < 60 {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
        }
        std::fs::write(std::env::var_os("MOIRAI_OS_TEST_READY").unwrap(), b"1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::hint::black_box(&v);
        std::process::exit(0);
    }
    std::process::exit(3);
}
