//! `ProcHost`, `Clock` and `Entropy` on Windows ([OS/proc], [OS/clock], [OS/README §4.6]): identities, liveness, the
//! parent watch, the boot clock across processes, the detached `gc` child, background priority and (feature
//! `test-host`) kill, suspend, resume and the wall-clock offset.

#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::time::Duration;

use common::{KillOnDrop, TempDir};
use moirai_os::OsVfs;
use moirai_vfs::{BootIdentity, Clock, Liveness, OsTag, ProcHost, ProcId, Wake, WatchEvent};
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::Threading::GetProcessTimes;

fn line_value(out: &[u8], key: &str) -> String {
    let text = String::from_utf8_lossy(out);
    text.lines()
        .find_map(|l| l.find(key).map(|i| l[i + key.len()..].trim().to_owned()))
        .unwrap_or_else(|| panic!("no {key} in {text:?}"))
}

#[test]
fn identities_and_the_boot_clock_across_processes() {
    let v = OsVfs;
    let me = v.self_id();
    assert_eq!(v.os_tag(), OsTag::Windows);
    assert_eq!((me.os, me.pid), (1, std::process::id()));
    assert_eq!(ProcId::from_bytes(&me.to_bytes()), Some(me));
    let boot = v.boot_identity();
    println!("OBSERVED: boot identity {boot:?}");
    assert!(matches!(boot, BootIdentity::Known(_)), "{boot:?}");
    let before = v.boot_ns();
    let out = common::child_command("child", "clocks").output().unwrap();
    let after = v.boot_ns();
    let theirs: u64 = line_value(&out.stdout, "BOOT_NS").parse().unwrap();
    assert!(
        before <= theirs && theirs <= after,
        "one boot clock: {before} {theirs} {after}"
    );
    let hash: u64 = line_value(&out.stdout, "BOOT_HASH").parse().unwrap();
    assert_eq!(
        hash,
        boot.boot_hash(),
        "every process of one boot has one boot identity"
    );
    let wall: i64 = line_value(&out.stdout, "WALL_MS").parse().unwrap();
    assert!((wall - v.wall_ms()).abs() < 60_000);
}

/// The creation time of a child in ns since the Unix epoch.
fn start_of(child: &std::process::Child) -> u64 {
    let z = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut c, mut e, mut k, mut u) = (z, z, z, z);
    // SAFETY: the child's handle is valid; the four out-pointers are live locals.
    let ok = unsafe { GetProcessTimes(child.as_raw_handle(), &mut c, &mut e, &mut k, &mut u) };
    assert_ne!(ok, 0);
    let f = (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime);
    (f - 116_444_736_000_000_000) * 100
}

#[test]
fn liveness_of_a_child() {
    let v = OsVfs;
    let mut child = KillOnDrop::new(common::child_command("child", "sleep").spawn().unwrap());
    let me = v.self_id();
    let p = ProcId {
        pid: child.id(),
        start: start_of(&child),
        ..me
    };
    assert_eq!(v.alive(&p), Liveness::Alive);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        v.alive(&p),
        Liveness::Dead,
        "exited, its object still open here"
    );
    drop(child);
}

#[test]
fn the_parent_watch_wakes() {
    let v = OsVfs;
    let w = v.watch_parent().unwrap();
    let wake = std::sync::Arc::new(v.new_wake().unwrap());
    let w2 = std::sync::Arc::clone(&wake);
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        w2.signal();
    });
    assert_eq!(v.wait_parent_or_wake(&w, &wake).unwrap(), WatchEvent::Woken);
    t.join().unwrap();
    let p = v.parent().unwrap();
    assert!(p.pid != 0 && p.start_known);
    println!("OBSERVED: parent image {:?}", v.parent_image());
}

#[test]
fn the_parent_watch_sees_the_parent_exit() {
    let t = TempDir::new("watch");
    let out = common::child_command("child", "orphan-maker")
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        common::wait_for_file(&t.join("parent-exited"), 30_000),
        "the grandchild never saw its parent exit"
    );
}

#[test]
fn the_gc_child_runs_detached() {
    let t = TempDir::new("gc");
    let v = OsVfs;
    let cmd = Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32\\cmd.exe");
    let pid = v
        .spawn_gc_child(&cmd, &["/c", "cd>here.txt"], t.path())
        .unwrap();
    assert_ne!(pid, 0);
    assert!(common::wait_for_file(&t.join("here.txt"), 30_000));
    std::thread::sleep(Duration::from_millis(100));
    let text = std::fs::read_to_string(t.join("here.txt")).unwrap();
    assert_eq!(Path::new(text.trim()), t.path(), "the child ran in `cwd`");
}

#[test]
fn spawning_while_holding_a_role_byte_panics() {
    let t = TempDir::new("gcheld");
    let out = common::child_command("child", "spawn-held")
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&out.stderr).contains("role byte is held"));
}

#[test]
fn background_mode_lowers_memory_priority() {
    let out = common::child_command("child", "background")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(line_value(&out.stdout, "MEMPRIO"), "2");
}

#[cfg(feature = "test-host")]
#[test]
fn test_host_suspends_resumes_and_kills() {
    use moirai_os::test_host;
    let t = TempDir::new("suspend");
    let mut child = KillOnDrop::new(
        common::child_command("child", "tick")
            .env("MOIRAI_OS_TEST_DIR", t.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let tick = t.join("tick");
    assert!(common::wait_for_file(&tick, 30_000));
    let read = || {
        std::fs::read_to_string(&tick)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
    };
    test_host::suspend(child.id()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let a = read();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(read(), a, "a suspended process does not run");
    test_host::resume(child.id()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_ne!(read(), a, "a resumed process runs");
    test_host::kill(&mut child).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(1));
}

#[cfg(feature = "test-host")]
#[test]
fn test_host_shifts_only_the_wall_clock() {
    let v = OsVfs;
    let out = common::child_command("child", "clocks")
        .env(moirai_os::test_host::WALL_OFFSET_ENV, "-3600000")
        .output()
        .unwrap();
    let wall: i64 = line_value(&out.stdout, "WALL_MS").parse().unwrap();
    let delta = v.wall_ms() - wall;
    assert!((3_540_000..3_660_000).contains(&delta), "{delta}");
    let theirs: u64 = line_value(&out.stdout, "BOOT_NS").parse().unwrap();
    assert!(theirs <= v.boot_ns(), "the boot clock is never shifted");
}

#[test]
fn child() {
    let Some(mode) = common::child_mode() else {
        return;
    };
    let v = OsVfs;
    let dir = std::env::var_os("MOIRAI_OS_TEST_DIR").map(std::path::PathBuf::from);
    match mode.as_str() {
        "clocks" => {
            println!("BOOT_NS {}", v.boot_ns());
            println!("BOOT_HASH {}", v.boot_identity().boot_hash());
            println!("WALL_MS {}", v.wall_ms());
            std::process::exit(0);
        }
        "sleep" => {
            std::thread::sleep(Duration::from_secs(60));
            std::process::exit(0);
        }
        "orphan-maker" => {
            let dir = dir.unwrap();
            let _grandchild = common::child_command("child", "watcher")
                .env("MOIRAI_OS_TEST_DIR", &dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            assert!(common::wait_for_file(&dir.join("watching"), 30_000));
            // Exit without waiting: the grandchild's parent is gone.
            std::process::exit(0);
        }
        "watcher" => {
            let dir = dir.unwrap();
            let w = v.watch_parent().unwrap();
            let wake = v.new_wake().unwrap();
            std::fs::write(dir.join("watching"), b"1").unwrap();
            if v.wait_parent_or_wake(&w, &wake).unwrap() == WatchEvent::ParentExited {
                std::fs::write(dir.join("parent-exited"), b"1").unwrap();
            }
            std::process::exit(0);
        }
        "spawn-held" => {
            use moirai_vfs::{LockByte, LockMode, Locks, RelPath, RootAccess, RootRole, StoreFs};
            let dir = dir.unwrap();
            let r = v
                .open_root(&dir, RootRole::Store, RootAccess::ReadWrite)
                .unwrap();
            drop(v.create_new(&r, RelPath::literal("LOCK")).unwrap());
            let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
            let _g = v.try_acquire(&mut c, LockByte::Writer).unwrap();
            let cmd = Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32\\cmd.exe");
            let _ = v.spawn_gc_child(&cmd, &["/c", "exit"], &dir);
            std::process::exit(0);
        }
        "background" => {
            use windows_sys::Win32::System::Threading::{
                GetCurrentProcess, GetProcessInformation, MEMORY_PRIORITY_INFORMATION,
                ProcessMemoryPriority,
            };
            v.enter_background();
            let mut info = MEMORY_PRIORITY_INFORMATION { MemoryPriority: 0 };
            // SAFETY: `info` is a live structure of the size passed.
            let ok = unsafe {
                GetProcessInformation(
                    GetCurrentProcess(),
                    ProcessMemoryPriority,
                    (&mut info as *mut MEMORY_PRIORITY_INFORMATION).cast(),
                    core::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
                )
            };
            assert_ne!(ok, 0);
            println!("MEMPRIO {}", info.MemoryPriority);
            std::process::exit(0);
        }
        "tick" => {
            let dir = dir.unwrap();
            let mut n = 0u64;
            loop {
                n += 1;
                let _ = std::fs::write(dir.join("tick.tmp"), n.to_string());
                let _ = std::fs::copy(dir.join("tick.tmp"), dir.join("tick"));
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        _ => std::process::exit(3),
    }
}
