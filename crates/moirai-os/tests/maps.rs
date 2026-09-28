//! `SealedMaps` on real NTFS ([OS/map]; X-F6): the size check, whole-file read-only mappings, the bounded registry, the
//! vectored `EXCEPTION_IN_PAGE_ERROR` handler (exit 7 with the one line) and what Windows does to a mapped file.

#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use std::path::Path;

use common::TempDir;
use moirai_os::{OsRoot, OsVfs};
use moirai_vfs::{
    Access, Advice, MAP_REGISTRY_SLOTS, MapError, OpenHint, RelPath, RootAccess, RootRole,
    SealedMap, SealedMaps, ShareRetry, StoreFs, SyncKind, VfsErrorKind,
};

fn store(dir: &Path) -> OsRoot {
    OsVfs
        .open_root(dir, RootRole::Store, RootAccess::ReadWrite)
        .unwrap()
}

/// Writes a sealed file of `len` pattern bytes and returns the pattern.
fn sealed(r: &OsRoot, name: &str, len: usize) -> Vec<u8> {
    let v = OsVfs;
    let data: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
    let f = v.create_new(r, RelPath::new(name).unwrap()).unwrap();
    v.write_at(&f, 0, &data).unwrap();
    v.sync(&f, SyncKind::DataAndMeta).unwrap();
    v.seal(&f).unwrap();
    data
}

#[test]
fn sealed_files_map_whole_and_read_back() {
    let t = TempDir::new("map");
    let v = OsVfs;
    let r = store(t.path());
    let data = sealed(&r, "seg.base.1", 100_000);
    let f = v
        .open(
            &r,
            RelPath::literal("seg.base.1"),
            Access::Read,
            OpenHint::Normal,
        )
        .unwrap();
    let name = RelPath::literal("seg.base.1");
    assert_eq!(v.map_sealed(&f, 0, name).unwrap_err(), MapError::Empty);
    assert_eq!(
        v.map_sealed(&f, 99_999, name).unwrap_err(),
        MapError::SizeMismatch {
            expected: 99_999,
            actual: 100_000
        }
    );
    let c0 = v.counters();
    let m = v.map_sealed(&f, 100_000, name).unwrap();
    let c1 = v.counters();
    assert!(c1.maps > c0.maps && c1.mapped_bytes >= c0.mapped_bytes + 100_000);
    drop(f);
    assert_eq!(m.len(), 100_000);
    assert!(
        m.bytes() == &data[..],
        "the mapping outlives the handle it was made from"
    );
    for advice in [Advice::WillNeed, Advice::Random, Advice::Sequential] {
        v.advise(&m, 4_000, 50_000, advice);
    }
    v.advise(&m, 0, u64::MAX, Advice::WillNeed);
    v.advise(&m, 200_000, 10, Advice::WillNeed);
    // Many readers share one mapping.
    let m = std::sync::Arc::new(m);
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let m = std::sync::Arc::clone(&m);
            std::thread::spawn(move || m.bytes().iter().map(|&b| u64::from(b)).sum::<u64>())
        })
        .collect();
    let want: u64 = data.iter().map(|&b| u64::from(b)).sum();
    for h in readers {
        assert_eq!(h.join().unwrap(), want);
    }
}

#[test]
fn a_mapped_file_cannot_be_truncated() {
    // [OS/map §2] rule 3, Appendix A: on Windows the OS refuses to truncate a mapped file (`ERROR_USER_MAPPED_FILE`).
    let t = TempDir::new("maptrunc");
    let v = OsVfs;
    let r = store(t.path());
    let f = v.create_new(&r, RelPath::literal("blobs.1")).unwrap();
    v.write_at(&f, 0, &[5u8; 8192]).unwrap();
    let m = v.map_sealed(&f, 8192, RelPath::literal("blobs.1")).unwrap();
    let w = std::fs::OpenOptions::new()
        .write(true)
        .open(t.join("blobs.1"))
        .unwrap();
    let e = w.set_len(0).unwrap_err();
    assert_eq!(e.raw_os_error(), Some(1224));
    assert_eq!(m.bytes()[8191], 5);
}

#[test]
fn deleting_a_mapped_file() {
    // [OS/fs] open point 8: the GC delete of a mapped file either fails (error 5) or succeeds; the mapping stays valid.
    let t = TempDir::new("mapdel");
    let v = OsVfs;
    let r = store(t.path());
    let data = sealed(&r, "hist.1", 4096);
    let f = v
        .open(
            &r,
            RelPath::literal("hist.1"),
            Access::Read,
            OpenHint::Normal,
        )
        .unwrap();
    let m = v.map_sealed(&f, 4096, RelPath::literal("hist.1")).unwrap();
    drop(f);
    match v.unlink(&r, RelPath::literal("hist.1"), ShareRetry::None) {
        Ok(()) => println!("OBSERVED: the delete of a mapped file succeeded"),
        Err(e) => {
            println!("OBSERVED: the delete of a mapped file failed: {e:?}");
            assert_eq!(e.kind, VfsErrorKind::AccessDenied);
        }
    }
    assert!(m.bytes() == &data[..]);
}

#[test]
fn the_registry_is_bounded() {
    let t = TempDir::new("mapreg");
    let out = common::child_command("child", "registry")
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("REGISTRY ok"));
}

fn raise_child(mode: &str, t: &TempDir) -> std::process::Output {
    common::child_command("child", mode)
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap()
}

#[test]
fn an_in_page_error_inside_a_mapping_exits_7() {
    let t = TempDir::new("fault");
    let out = raise_child("fault-inside", &t);
    assert_eq!(out.status.code(), Some(7));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.ends_with("store I/O fault in seg.base.7 at 4099: run moirai doctor --fsck\n"),
        "{err:?}"
    );
}

#[test]
fn an_in_page_error_elsewhere_is_passed_on() {
    let t = TempDir::new("faultout");
    let out = raise_child("fault-outside", &t);
    assert_eq!(out.status.code().map(|c| c as u32), Some(0xC000_0006));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("store I/O fault"));
}

#[test]
fn child() {
    let Some(mode) = common::child_mode() else {
        return;
    };
    use windows_sys::Win32::System::Diagnostics::Debug::{
        RaiseException, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
    };
    let v = OsVfs;
    let dir = std::env::var_os("MOIRAI_OS_TEST_DIR").unwrap();
    let r = store(Path::new(&dir));
    match mode.as_str() {
        "registry" => {
            sealed(&r, "cs.0001", 64);
            let f = v
                .open(
                    &r,
                    RelPath::literal("cs.0001"),
                    Access::Read,
                    OpenHint::Normal,
                )
                .unwrap();
            let mut maps: Vec<_> = (0..MAP_REGISTRY_SLOTS)
                .map(|_| v.map_sealed(&f, 64, RelPath::literal("cs.0001")).unwrap())
                .collect();
            assert_eq!(
                v.map_sealed(&f, 64, RelPath::literal("cs.0001"))
                    .unwrap_err(),
                MapError::RegistryFull
            );
            maps.pop();
            maps.push(v.map_sealed(&f, 64, RelPath::literal("cs.0001")).unwrap());
            assert!(maps.iter().all(|m| m.bytes()[63] == (63 * 7 % 251) as u8));
            println!("REGISTRY ok");
            std::process::exit(0);
        }
        "fault-inside" | "fault-outside" => {
            sealed(&r, "seg.base.7", 16_384);
            let f = v
                .open(
                    &r,
                    RelPath::literal("seg.base.7"),
                    Access::Read,
                    OpenHint::Normal,
                )
                .unwrap();
            let m = v
                .map_sealed(&f, 16_384, RelPath::literal("seg.base.7"))
                .unwrap();
            let base = m.bytes().as_ptr() as usize;
            let addr = if mode == "fault-inside" {
                base + 4099
            } else {
                base.wrapping_add(1usize << 40)
            };
            // STATUS_DEVICE_DATA_ERROR as the underlying status, the address inside or outside the mapping.
            let args: [usize; 3] = [0, addr, 0xC000_009C];
            // SAFETY: suppresses the error dialog of an unhandled exception, then raises a continuable in-page error
            // with three parameters, exactly as a pager failure reports one ([OS/map] open point 7).
            unsafe {
                SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
                RaiseException(0xC000_0006, 0, 3, args.as_ptr());
            }
            std::process::exit(9);
        }
        _ => std::process::exit(3),
    }
}
