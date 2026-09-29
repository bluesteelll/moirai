//! `StoreFs` on real NTFS ([OS/fs]): roots, names, positional I/O, the durability classes, sealing, renames, sharing
//! violations, deletes of open files, identities, extents, use-time name checks and `fail_stop`.

#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use std::os::windows::fs::OpenOptionsExt;

use common::TempDir;
use moirai_os::{OsRoot, OsVfs};
use moirai_vfs::{
    Access, DirEntry, EntryKind, EntryName, ExtentMethod, FsKind, GroupMember, OpenHint, RelPath,
    RootAccess, RootRole, ShareRetry, StoreFs, StoreVolume, SwapIntent, SyncKind, VfsErrorKind,
};

fn rel(s: &str) -> RelPath<'_> {
    RelPath::new(s).unwrap()
}

fn store(t: &TempDir) -> OsRoot {
    OsVfs
        .open_root(t.path(), RootRole::Store, RootAccess::ReadWrite)
        .unwrap()
}

const NTFS: StoreVolume = StoreVolume {
    fs: FsKind::Ntfs,
    extent_method: ExtentMethod::ZeroFill,
    read_only: false,
    removable: false,
};

#[test]
fn roots_open_and_refuse() {
    let t = TempDir::new("roots");
    let v = OsVfs;
    let e = v
        .open_root(&t.join("missing"), RootRole::Store, RootAccess::Read)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::NotFound);
    let e = v
        .open_root(
            std::path::Path::new("relative"),
            RootRole::Other,
            RootAccess::Read,
        )
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::InvalidName);
    std::fs::write(t.join("file"), b"x").unwrap();
    let e = v
        .open_root(&t.join("file"), RootRole::Other, RootAccess::Read)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::NotFound, "a file is not a directory");
    let r = store(&t);
    assert_eq!(
        (r.role(), r.access()),
        (RootRole::Store, RootAccess::ReadWrite)
    );
    // A lower-case drive letter and `/` separators name the same directory.
    let lower = t
        .path()
        .to_string_lossy()
        .replacen("D:", "d:", 1)
        .replace('\\', "/");
    let r2 = v
        .open_root(
            std::path::Path::new(&lower),
            RootRole::Other,
            RootAccess::Read,
        )
        .unwrap();
    assert_eq!(v.root_identity(&r).unwrap(), v.root_identity(&r2).unwrap());
}

/// The explicit, inheritable ACE that `create_root` gives a store directory ([OS/fs §4.1], [90 §5.4]).
fn has_owner_ace(dir: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, EqualSid,
        GetAce, GetTokenInformation, INHERITED_ACE, OBJECT_INHERIT_ACE, PSECURITY_DESCRIPTOR,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = core::ptr::null_mut();
    // SAFETY: the pseudo-handle is valid; `token` is a live local.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    assert_ne!(ok, 0);
    let mut buf = vec![0u64; 64];
    let mut len = 0u32;
    // SAFETY: `buf` is 512 writable, 8-aligned bytes, enough for a `TOKEN_USER`.
    let ok =
        unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), 512, &mut len) };
    assert_ne!(ok, 0);
    // SAFETY: the buffer holds the `TOKEN_USER` just written.
    let sid = unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid };
    let path: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let mut dacl: *mut ACL = core::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = core::ptr::null_mut();
    // SAFETY: `path` is NUL-terminated; the out-pointers are live locals.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &mut dacl,
            core::ptr::null_mut(),
            &mut sd,
        )
    };
    assert_eq!(rc, 0);
    let mut found = false;
    // SAFETY: `dacl` points into `sd`, alive until `LocalFree` below.
    let count = unsafe { (*dacl).AceCount };
    for i in 0..u32::from(count) {
        let mut ace: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `i` is below the ACE count.
        if unsafe { GetAce(dacl, i, &mut ace) } == 0 {
            continue;
        }
        // SAFETY: every ACE starts with an `ACE_HEADER`; type 0 is `ACCESS_ALLOWED_ACE`.
        let a = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
        let flags = u32::from(a.Header.AceFlags);
        if a.Header.AceType == 0
            && flags & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE)
                == OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
            && flags & INHERITED_ACE == 0
        {
            // SAFETY: the SID starts at `SidStart` inside the ACE.
            let ace_sid = (&a.SidStart as *const u32).cast_mut().cast();
            // SAFETY: both SIDs are valid for the call.
            if unsafe { EqualSid(ace_sid, sid) } != 0 {
                found = true;
            }
        }
    }
    // SAFETY: `sd` was allocated by `GetNamedSecurityInfoW` and is freed once; `token` is closed once.
    unsafe {
        LocalFree(sd);
        windows_sys::Win32::Foundation::CloseHandle(token);
    }
    found
}

#[test]
fn create_root_adds_the_owner_ace_and_is_exclusive() {
    let t = TempDir::new("createroot");
    let v = OsVfs;
    let before = v.counters();
    let s = v.create_root(&t.join("store"), RootRole::Store).unwrap();
    assert_eq!(s.access(), RootAccess::ReadWrite);
    assert!(has_owner_ace(&t.join("store")));
    let after = v.counters();
    assert!(after.creates > before.creates && after.sync_dir > before.sync_dir);
    let e = v
        .create_root(&t.join("store"), RootRole::Store)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AlreadyExists);
    let e = v
        .create_root(&t.join("no/parent"), RootRole::Other)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::NotFound);
    let o = v.create_root(&t.join("other"), RootRole::Other).unwrap();
    assert_eq!(o.role(), RootRole::Other);
    assert!(
        !has_owner_ace(&t.join("other")),
        "only a store directory gets the ACE"
    );
}

#[test]
fn files_round_trip_positionally() {
    let t = TempDir::new("files");
    let v = OsVfs;
    let r = store(&t);
    let f = v.create_new(&r, rel("seg.1")).unwrap();
    assert_eq!(
        v.create_new(&r, rel("seg.1")).unwrap_err().kind,
        VfsErrorKind::AlreadyExists
    );
    v.write_at(&f, 0, b"hello").unwrap();
    v.write_at(&f, 10, b"world").unwrap();
    assert_eq!(v.file_size(&f).unwrap(), 15);
    let mut buf = [0xAAu8; 15];
    assert_eq!(v.read_at(&f, 0, &mut buf).unwrap(), 15);
    assert_eq!(&buf, b"hello\0\0\0\0\0world", "the gap reads as zeros");
    let mut tail = [0u8; 8];
    assert_eq!(
        v.read_at(&f, 12, &mut tail).unwrap(),
        3,
        "fewer bytes only at the end"
    );
    assert_eq!(v.read_at(&f, 15, &mut tail).unwrap(), 0);
    assert_eq!(v.read_at(&f, 1 << 40, &mut tail).unwrap(), 0);
    let e = v.read_exact_at(&f, 12, &mut tail).unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::UnexpectedEof);
    let mut five = [0u8; 5];
    v.read_exact_at(&f, 10, &mut five).unwrap();
    assert_eq!(&five, b"world");
    // A large write crosses many pages and returns only when all of it is written.
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 253) as u8).collect();
    v.write_at(&f, 100, &big).unwrap();
    let mut back = vec![0u8; big.len()];
    v.read_exact_at(&f, 100, &mut back).unwrap();
    assert!(back == big);
    drop(f);
    let ro = v
        .open(&r, rel("seg.1"), Access::Read, OpenHint::Sequential)
        .unwrap();
    assert_eq!(v.file_size(&ro).unwrap(), 3_000_100);
    assert_eq!(
        v.open(&r, rel("missing"), Access::Read, OpenHint::Normal)
            .unwrap_err()
            .kind,
        VfsErrorKind::NotFound
    );
    assert_eq!(
        v.open(&r, rel("nodir/x"), Access::Read, OpenHint::Normal)
            .unwrap_err()
            .kind,
        VfsErrorKind::NotFound
    );
}

#[test]
fn list_dir_reports_every_entry_once() {
    let t = TempDir::new("list");
    let v = OsVfs;
    let r = store(&t);
    v.create_dir(&r, rel("tmp")).unwrap();
    for i in 0..300 {
        drop(v.create_new(&r, rel(&format!("tmp/f.{i}"))).unwrap());
    }
    drop(v.create_new(&r, rel("HEAD")).unwrap());
    let mut top = v.list_dir(&r, None).unwrap();
    top.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    assert_eq!(
        top,
        vec![
            DirEntry {
                name: EntryName::Utf8("HEAD".into()),
                kind: EntryKind::File
            },
            DirEntry {
                name: EntryName::Utf8("tmp".into()),
                kind: EntryKind::Dir
            },
        ]
    );
    let sub = v.list_dir(&r, Some(rel("tmp"))).unwrap();
    assert_eq!(sub.len(), 300);
    assert!(sub.iter().all(|e| e.kind == EntryKind::File));
    assert_eq!(v.list_dir(&r, Some(RelPath::ROOT)).unwrap().len(), 2);
    assert_eq!(
        v.list_dir(&r, Some(rel("nope"))).unwrap_err().kind,
        VfsErrorKind::NotFound
    );
}

#[test]
fn directories_create_and_remove() {
    let t = TempDir::new("dirs");
    let v = OsVfs;
    let r = store(&t);
    v.create_dir(&r, rel("trash")).unwrap();
    assert_eq!(
        v.create_dir(&r, rel("trash")).unwrap_err().kind,
        VfsErrorKind::AlreadyExists
    );
    v.create_dir(&r, rel("trash/1")).unwrap();
    drop(v.create_new(&r, rel("trash/1/x")).unwrap());
    assert_eq!(
        v.remove_dir(&r, rel("trash/1")).unwrap_err().kind,
        VfsErrorKind::NotEmpty
    );
    v.unlink(&r, rel("trash/1/x"), ShareRetry::None).unwrap();
    v.remove_dir(&r, rel("trash/1")).unwrap();
    v.remove_dir(&r, rel("trash")).unwrap();
    assert!(v.list_dir(&r, None).unwrap().is_empty());
}

#[test]
fn durability_classes_succeed_on_ntfs_and_count() {
    let t = TempDir::new("sync");
    let v = OsVfs;
    let r = store(&t);
    v.create_dir(&r, rel("tmp")).unwrap();
    let f = v.create_new(&r, rel("log.1")).unwrap();
    v.write_at(&f, 0, &[7u8; 8192]).unwrap();
    let c0 = v.counters();
    v.sync(&f, SyncKind::Data).unwrap();
    v.sync(&f, SyncKind::DataAndMeta).unwrap();
    v.sync_dir(&r, None).unwrap();
    v.sync_dir(&r, None).unwrap();
    v.sync_dir(&r, Some(rel("tmp"))).unwrap();
    // Counters are process-wide and the tests of this binary run in parallel: exact deltas are checked in a child
    // (`counters_count_each_call_once`).
    let c1 = v.counters();
    assert!(c1.sync_data > c0.sync_data && c1.sync_meta > c0.sync_meta);
    assert!(c1.sync_dir >= c0.sync_dir + 3);
    assert_eq!(c1.full_barriers, 0, "macOS only");
    v.sync_group(&[
        GroupMember::File {
            file: &f,
            kind: SyncKind::DataAndMeta,
        },
        GroupMember::Dir {
            root: &r,
            dir: None,
        },
        GroupMember::Dir {
            root: &r,
            dir: Some(rel("tmp")),
        },
    ])
    .unwrap();
    let c2 = v.counters();
    assert!(c2.sync_meta > c1.sync_meta && c2.sync_dir >= c1.sync_dir + 2);
    let df = v.sync_dir(&r, Some(rel("missing"))).unwrap_err();
    assert_eq!(df.kind, VfsErrorKind::NotFound);
}

#[test]
#[should_panic(expected = "sync on a handle without write access")]
fn sync_on_a_read_handle_is_a_programming_error() {
    let t = TempDir::new("syncro");
    let v = OsVfs;
    let r = store(&t);
    drop(v.create_new(&r, rel("f")).unwrap());
    let f = v
        .open(&r, rel("f"), Access::Read, OpenHint::Normal)
        .unwrap();
    let _ = v.sync(&f, SyncKind::Data);
}

#[test]
fn read_roots_refuse_writes() {
    let t = TempDir::new("readroot");
    let v = OsVfs;
    drop(v.create_new(&store(&t), rel("f")).unwrap());
    let r = v
        .open_root(t.path(), RootRole::Store, RootAccess::Read)
        .unwrap();
    let denied = VfsErrorKind::AccessDenied;
    assert_eq!(
        v.open(&r, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap_err()
            .kind,
        denied
    );
    assert_eq!(v.create_new(&r, rel("g")).unwrap_err().kind, denied);
    assert_eq!(v.create_dir(&r, rel("d")).unwrap_err().kind, denied);
    assert_eq!(
        v.unlink(&r, rel("f"), ShareRetry::None).unwrap_err().kind,
        denied
    );
    assert_eq!(
        v.rename_noreplace(&r, rel("f"), &r, rel("g"), ShareRetry::None)
            .unwrap_err()
            .kind,
        denied
    );
    assert_eq!(v.sync_dir(&r, None).unwrap_err().kind, denied);
    assert!(
        v.open(&r, rel("f"), Access::Read, OpenHint::Normal).is_ok(),
        "readers work"
    );
}

#[test]
fn sealing_makes_files_read_only() {
    let t = TempDir::new("seal");
    let v = OsVfs;
    let r = store(&t);
    let f = v.create_new(&r, rel("cs.0001")).unwrap();
    v.write_at(&f, 0, b"sealed").unwrap();
    v.sync(&f, SyncKind::DataAndMeta).unwrap();
    v.seal(&f).unwrap();
    drop(f);
    let md = std::fs::metadata(t.join("cs.0001")).unwrap();
    assert!(md.permissions().readonly());
    let e = v
        .open(&r, rel("cs.0001"), Access::ReadWrite, OpenHint::Normal)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AccessDenied, "a casual writer fails");
    drop(v.create_new(&r, rel("tmp.1")).unwrap());
    let e = v
        .rename_replace(&r, rel("tmp.1"), &r, rel("cs.0001"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AccessDenied, "never renamed over");
    // GC clears the attribute before the delete.
    v.unlink(&r, rel("cs.0001"), ShareRetry::None).unwrap();
    assert!(!t.join("cs.0001").exists());
    assert_eq!(
        v.unlink(&r, rel("cs.0001"), ShareRetry::None)
            .unwrap_err()
            .kind,
        VfsErrorKind::NotFound
    );
}

#[test]
fn renames_never_replace_unless_asked() {
    let t = TempDir::new("rename");
    let v = OsVfs;
    let r = store(&t);
    v.create_dir(&r, rel("tmp")).unwrap();
    let f = v.create_new(&r, rel("tmp/c.1")).unwrap();
    v.write_at(&f, 0, b"new").unwrap();
    drop(f);
    let g = v.create_new(&r, rel("config")).unwrap();
    v.write_at(&g, 0, b"old").unwrap();
    drop(g);
    let c0 = v.counters().renames;
    let e = v
        .rename_noreplace(&r, rel("tmp/c.1"), &r, rel("config"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AlreadyExists);
    v.rename_replace(&r, rel("tmp/c.1"), &r, rel("config"), ShareRetry::None)
        .unwrap();
    assert_eq!(std::fs::read(t.join("config")).unwrap(), b"new");
    drop(v.create_new(&r, rel("tmp/d.2")).unwrap());
    v.rename_noreplace(&r, rel("tmp/d.2"), &r, rel("d.2"), ShareRetry::None)
        .unwrap();
    assert!(v.counters().renames >= c0 + 2);
    // Across two roots on one volume.
    let other = TempDir::new("rename2");
    let o = store(&other);
    v.rename_noreplace(&r, rel("d.2"), &o, rel("moved"), ShareRetry::None)
        .unwrap();
    assert!(other.join("moved").exists());
}

#[test]
fn sharing_violations_are_retried_within_the_bound() {
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    let t = TempDir::new("share");
    let v = OsVfs;
    let r = store(&t);
    drop(v.create_new(&r, rel("pack")).unwrap());
    // A scanner that opens the file without FILE_SHARE_DELETE.
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(t.join("pack"))
        .unwrap();
    let e = v.unlink(&r, rel("pack"), ShareRetry::None).unwrap_err();
    assert!(
        matches!(
            e.kind,
            VfsErrorKind::SharingViolation | VfsErrorKind::AccessDenied
        ),
        "{e:?}"
    );
    let r0 = v.counters().share_retries;
    let start = std::time::Instant::now();
    let e = v
        .rename_noreplace(
            &r,
            rel("pack"),
            &r,
            rel("pack.2"),
            ShareRetry::Bounded { total_ms: 120 },
        )
        .unwrap_err();
    let spent = start.elapsed().as_millis();
    assert!(
        matches!(
            e.kind,
            VfsErrorKind::SharingViolation | VfsErrorKind::AccessDenied
        ),
        "{e:?}"
    );
    assert!(
        v.counters().share_retries - r0 >= 5,
        "1, 2, 4, 8, 16, 32 ms …"
    );
    assert!(spent < 1_000, "bounded: {spent} ms");
    // The holder lets go while the retry runs: the rename succeeds.
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(40));
        drop(holder);
    });
    v.rename_noreplace(
        &r,
        rel("pack"),
        &r,
        rel("pack.2"),
        ShareRetry::Bounded { total_ms: 2_000 },
    )
    .unwrap();
    release.join().unwrap();
    assert!(t.join("pack.2").exists());
}

#[test]
fn deleting_an_open_file() {
    // [OS/fs §6.4]: another process's delete of a file moirai holds open must not disturb it. On Windows 11 NTFS,
    // `DeleteFileW` uses POSIX delete semantics, so the name goes at once (the file is not delete-pending); a delete
    // with the classic semantics leaves it delete-pending. Either way the open handle keeps reading its bytes.
    let t = TempDir::new("delopen");
    let v = OsVfs;
    let r = store(&t);
    let f = v.create_new(&r, rel("seg.9")).unwrap();
    v.write_at(&f, 0, b"still readable").unwrap();
    v.unlink(&r, rel("seg.9"), ShareRetry::None).unwrap();
    let mut buf = [0u8; 14];
    v.read_exact_at(&f, 0, &mut buf).unwrap();
    assert_eq!(&buf, b"still readable");
    let e = v
        .open(&r, rel("seg.9"), Access::Read, OpenHint::Normal)
        .unwrap_err();
    assert!(
        matches!(e.kind, VfsErrorKind::NotFound | VfsErrorKind::DeletePending),
        "{e:?}"
    );
}

#[test]
fn a_delete_pending_name_is_reported_as_such() {
    // The classic delete (`FileDispositionInfo`, not POSIX semantics) leaves the name delete-pending while a handle is
    // open: opens fail with `DeletePending` (code 303, [OS/fs §6.1, §6.4]) and the name cannot be created again.
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileDispositionInfo, SetFileInformationByHandle,
    };
    let t = TempDir::new("delpending");
    let v = OsVfs;
    let r = store(&t);
    let keep = v.create_new(&r, rel("seg.3")).unwrap();
    v.write_at(&keep, 0, b"kept").unwrap();
    let del = std::fs::OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(t.join("seg.3"))
        .unwrap();
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the handle has `DELETE` access; `info` is a live structure of the size passed.
    let ok = unsafe {
        SetFileInformationByHandle(
            del.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            core::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    assert_ne!(ok, 0);
    drop(del);
    let e = v
        .open(&r, rel("seg.3"), Access::Read, OpenHint::Normal)
        .unwrap_err();
    assert_eq!(
        (e.kind, e.os),
        (VfsErrorKind::DeletePending, moirai_vfs::OsCode(303))
    );
    let e = v.create_new(&r, rel("seg.3")).unwrap_err();
    assert!(
        matches!(
            e.kind,
            VfsErrorKind::DeletePending | VfsErrorKind::AccessDenied
        ),
        "{e:?}"
    );
    let mut buf = [0u8; 4];
    v.read_exact_at(&keep, 0, &mut buf).unwrap();
    assert_eq!(&buf, b"kept", "the holder still reads its bytes");
    drop(keep);
    assert_eq!(
        v.open(&r, rel("seg.3"), Access::Read, OpenHint::Normal)
            .unwrap_err()
            .kind,
        VfsErrorKind::NotFound,
        "gone with the last handle"
    );
}

#[test]
fn identities_agree() {
    let t = TempDir::new("ident");
    let v = OsVfs;
    let r = store(&t);
    v.create_dir(&r, rel("sub")).unwrap();
    let f = v.create_new(&r, rel("LOCK")).unwrap();
    let a = v.identity(&f).unwrap();
    assert_eq!(v.path_identity(&r, rel("LOCK")).unwrap(), a);
    let g = v
        .open(&r, rel("LOCK"), Access::Read, OpenHint::Normal)
        .unwrap();
    assert_eq!(v.identity(&g).unwrap(), a);
    let sub = v.path_identity(&r, rel("sub")).unwrap();
    assert_ne!(sub, a);
    assert_eq!(
        v.path_identity(&r, RelPath::ROOT).unwrap(),
        v.root_identity(&r).unwrap()
    );
    let r2 = v
        .open_root(&t.join("sub"), RootRole::Other, RootAccess::Read)
        .unwrap();
    assert_eq!(v.root_identity(&r2).unwrap(), sub);
    assert_eq!(sub.volume, a.volume);
    assert_eq!(
        v.path_identity(&r, rel("nope")).unwrap_err().kind,
        VfsErrorKind::NotFound
    );
    let fs = v.free_space(&r).unwrap();
    assert!(fs.total > 0 && fs.available <= fs.total);
}

#[test]
fn extents_read_as_zero() {
    let t = TempDir::new("extent");
    let v = OsVfs;
    let r = store(&t);
    let len = 3 * 256 * 1024 + 4096 + 17;
    let f = v.create_extent(&r, rel("log.1"), len, &NTFS).unwrap();
    assert_eq!(v.file_size(&f).unwrap(), len);
    let mut buf = vec![1u8; len as usize];
    v.read_exact_at(&f, 0, &mut buf).unwrap();
    assert!(buf.iter().all(|&b| b == 0));
    v.write_at(&f, 5000, &[9u8; 70_000]).unwrap();
    v.recycle_extent(&f, len, &NTFS).unwrap();
    v.read_exact_at(&f, 0, &mut buf).unwrap();
    assert!(buf.iter().all(|&b| b == 0), "recycled");
    // Re-preparation sets the length whatever it was ([OS/fs §4.5], S1-24): a longer file is cut to `len`, a shorter one
    // is extended.
    v.write_at(&f, len + 4096, &[7u8; 100]).unwrap();
    assert_eq!(v.file_size(&f).unwrap(), len + 4196);
    v.recycle_extent(&f, len, &NTFS).unwrap();
    assert_eq!(v.file_size(&f).unwrap(), len, "cut to len");
    v.read_exact_at(&f, 0, &mut buf).unwrap();
    assert!(buf.iter().all(|&b| b == 0));
    let short = v.create_new(&r, rel("log.9")).unwrap();
    v.write_at(&short, 0, &[5u8; 1000]).unwrap();
    v.recycle_extent(&short, len, &NTFS).unwrap();
    assert_eq!(v.file_size(&short).unwrap(), len, "extended to len");
    v.read_exact_at(&short, 0, &mut buf).unwrap();
    assert!(buf.iter().all(|&b| b == 0));
    assert_eq!(
        v.create_extent(&r, rel("log.1"), len, &NTFS)
            .unwrap_err()
            .kind,
        VfsErrorKind::AlreadyExists
    );
    let sparse = StoreVolume {
        extent_method: ExtentMethod::Sparse,
        ..NTFS
    };
    let s = v.create_extent(&r, rel("log.2"), len, &sparse).unwrap();
    assert_eq!(v.file_size(&s).unwrap(), len);
    let mut end = [1u8; 64];
    v.read_exact_at(&s, len - 64, &mut end).unwrap();
    assert_eq!(end, [0u8; 64]);
    let huge = v
        .create_extent(&r, rel("log.3"), u64::MAX / 4, &sparse)
        .unwrap_err();
    assert_eq!(huge.kind, VfsErrorKind::InsufficientSpace);
}

#[test]
fn use_time_name_checks() {
    let t = TempDir::new("names");
    let v = OsVfs;
    let r = store(&t);
    for bad in [
        "a:b",
        "trailing.",
        "trailing ",
        "q?",
        "star*",
        "pipe|x",
        "lt<",
        "gt>",
        "quote\"",
    ] {
        let e = v.create_new(&r, rel(bad)).unwrap_err();
        assert_eq!(e.kind, VfsErrorKind::InvalidName, "{bad:?}");
    }
    let long = "x".repeat(256);
    assert_eq!(
        v.create_new(&r, rel(&long)).unwrap_err().kind,
        VfsErrorKind::InvalidName
    );
    assert!(v.create_new(&r, rel(&"y".repeat(255))).is_ok());
    assert!(
        v.list_dir(&r, None).unwrap().len() == 1,
        "nothing else was created"
    );
}

#[test]
fn directory_renames_and_open_handles() {
    // [OS/fs] open point 6: a directory whose only open handle is on the directory itself (with full sharing) can be
    // renamed; a directory with any file open inside, even with full sharing, cannot (error 5), so a Windows `restore`
    // swap must hold no handle inside the swapped store.
    let t = TempDir::new("dirhandles");
    let v = OsVfs;
    let parent = v
        .open_root(t.path(), RootRole::Other, RootAccess::ReadWrite)
        .unwrap();
    v.create_dir(&parent, rel("a")).unwrap();
    let a = v
        .open_root(&t.join("a"), RootRole::Store, RootAccess::ReadWrite)
        .unwrap();
    drop(v.create_new(&a, rel("LOCK")).unwrap());
    v.rename_noreplace(&parent, rel("a"), &parent, rel("b"), ShareRetry::None)
        .unwrap();
    v.rename_noreplace(&parent, rel("b"), &parent, rel("a"), ShareRetry::None)
        .unwrap();
    let f = v
        .open(&a, rel("LOCK"), Access::Read, OpenHint::Normal)
        .unwrap();
    let e = v
        .rename_noreplace(&parent, rel("a"), &parent, rel("b"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AccessDenied);
    drop(f);
    drop(a);
}

#[test]
fn fail_stop_exits_7_with_one_line() {
    let out = common::child_command("child", "fail-stop")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    let err = String::from_utf8(out.stderr).unwrap();
    let line = "error[durability_failure]: NtFlushBuffersFileEx (durable) failed: os 112 ERROR_DISK_FULL; \
                outcome unknown: re-run with the same key or check moirai changes\n";
    assert!(err.ends_with(line), "{err:?}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("after fail_stop"));
}

#[test]
fn counters_count_each_call_once() {
    let t = TempDir::new("counts");
    let out = common::child_command("child", "counters")
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    // The harness prints `test child ... ` before the child's own output on the same line.
    let line = text
        .lines()
        .find_map(|l| l.find("COUNTS").map(|i| &l[i..]))
        .unwrap_or_else(|| panic!("no COUNTS line in {text:?}"));
    // opens creates bytes_written sync_data sync_meta sync_dir renames unlinks
    assert_eq!(line, "COUNTS 1 3 6 1 2 5 2 1");
}

/// [OS/fs §4.9.4]: a recovery rename between two parents that differ only in case, in a directory with per-directory
/// case sensitivity, flushes both parents (they are two directories; FM-2.3), then `a_parent` for the intent's removal.
/// In a child, so the process-wide counters move only by the recovery.
#[test]
fn swap_recovery_flushes_parents_that_differ_only_in_case() {
    let t = TempDir::new("swapcase");
    if !common::set_case_sensitive(t.path()) {
        println!(
            "OBSERVED: per-directory case sensitivity cannot be set here (fsutil); the case is skipped"
        );
        return;
    }
    for (p, d, m) in [("P", "a", "A"), ("p", "b", "B")] {
        std::fs::create_dir_all(t.join(p).join(d)).unwrap();
        std::fs::write(t.join(p).join(d).join("which"), m).unwrap();
    }
    let out = common::child_command("child", "swap-case")
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let line = text
        .lines()
        .find_map(|l| l.find("SWAPCASE").map(|i| &l[i..]))
        .unwrap_or_else(|| panic!("no SWAPCASE line in {text:?}"));
    // result sync_dir renames: the completing rename `T → B` from `P` to `p`, a flush of each, one of `P` for the intent.
    assert_eq!(line, "SWAPCASE Completed 3 1");
    let which = |p: &str| std::fs::read_to_string(t.join(p).join("which")).unwrap();
    assert_eq!(
        (which("P/a"), which("p/b")),
        ("B".to_owned(), "A".to_owned())
    );
    assert!(std::fs::symlink_metadata(t.join("P/a.swap")).is_err());
}

/// The machine-local P12 form of an existing directory ([OS/fs §4.9.3]): its final path with `\\?\` removed and `/`
/// separators.
fn p12(dir: &std::path::Path) -> String {
    let fin = std::fs::canonicalize(dir).unwrap();
    let s = fin.to_string_lossy();
    match s.strip_prefix(r"\\?\UNC\") {
        Some(unc) => format!("//{}", unc.replace('\\', "/")),
        None => s.trim_start_matches(r"\\?\").replace('\\', "/"),
    }
}

#[test]
fn child() {
    let Some(mode) = common::child_mode() else {
        return;
    };
    if mode == "swap-case" {
        let v = OsVfs;
        let dir = std::path::PathBuf::from(std::env::var_os("MOIRAI_OS_TEST_DIR").unwrap());
        let open = |p: &str| {
            v.open_root(&dir.join(p), RootRole::Other, RootAccess::ReadWrite)
                .unwrap()
        };
        let (pa, pb) = (open("P"), open("p"));
        let bytes = SwapIntent {
            a_id: v.path_identity(&pa, rel("a")).unwrap(),
            b_id: v.path_identity(&pb, rel("b")).unwrap(),
            a_path: format!("{}/a", p12(&dir.join("P"))),
            b_path: format!("{}/b", p12(&dir.join("p"))),
            t_path: format!("{}/a.swap-old", p12(&dir.join("P"))),
        }
        .encode()
        .unwrap();
        let f = v.create_new(&pa, rel("a.swap")).unwrap();
        v.write_at(&f, 0, &bytes).unwrap();
        drop(f);
        // The state after step 5 ([OS/fs §4.9.2]): `A → T`, then `B → A`.
        v.rename_noreplace(&pa, rel("a"), &pa, rel("a.swap-old"), ShareRetry::None)
            .unwrap();
        v.rename_noreplace(&pb, rel("b"), &pa, rel("a"), ShareRetry::None)
            .unwrap();
        let c0 = v.counters();
        let r = v.swap_recover(&pa, rel("a"), ShareRetry::None).unwrap();
        let c = v.counters();
        println!(
            "SWAPCASE {r:?} {} {}",
            c.sync_dir - c0.sync_dir,
            c.renames - c0.renames
        );
        std::process::exit(0);
    }
    if mode == "counters" {
        let v = OsVfs;
        let dir = std::env::var_os("MOIRAI_OS_TEST_DIR").unwrap();
        let r = v
            .open_root(
                std::path::Path::new(&dir),
                RootRole::Store,
                RootAccess::ReadWrite,
            )
            .unwrap();
        let c0 = v.counters();
        v.create_dir(&r, rel("tmp")).unwrap();
        let f = v.create_new(&r, rel("tmp/a")).unwrap();
        v.write_at(&f, 0, b"abcdef").unwrap();
        v.sync(&f, SyncKind::Data).unwrap();
        v.sync(&f, SyncKind::DataAndMeta).unwrap();
        v.sync_dir(&r, None).unwrap();
        v.sync_dir(&r, Some(rel("tmp"))).unwrap();
        v.sync_group(&[
            GroupMember::File {
                file: &f,
                kind: SyncKind::DataAndMeta,
            },
            GroupMember::Dir {
                root: &r,
                dir: None,
            },
            GroupMember::Dir {
                root: &r,
                dir: Some(rel("tmp")),
            },
            GroupMember::Dir {
                root: &r,
                dir: None,
            },
        ])
        .unwrap();
        drop(f);
        drop(v.create_new(&r, rel("b")).unwrap());
        drop(
            v.open(&r, rel("b"), Access::Read, OpenHint::Normal)
                .unwrap(),
        );
        v.rename_noreplace(&r, rel("tmp/a"), &r, rel("a"), ShareRetry::None)
            .unwrap();
        v.rename_replace(&r, rel("a"), &r, rel("b"), ShareRetry::None)
            .unwrap();
        v.unlink(&r, rel("b"), ShareRetry::None).unwrap();
        let c = v.counters();
        println!(
            "COUNTS {} {} {} {} {} {} {} {}",
            c.opens - c0.opens,
            c.creates - c0.creates,
            c.bytes_written - c0.bytes_written,
            c.sync_data - c0.sync_data,
            c.sync_meta - c0.sync_meta,
            c.sync_dir - c0.sync_dir,
            c.renames - c0.renames,
            c.unlinks - c0.unlinks,
        );
        std::process::exit(0);
    }
    if mode == "fail-stop" {
        use moirai_vfs::{DurabilityClass, DurabilityFailure, OsCode};
        OsVfs.fail_stop(DurabilityFailure {
            class: DurabilityClass::Durable,
            call: "NtFlushBuffersFileEx",
            kind: VfsErrorKind::DiskFull,
            os: OsCode(112),
        });
    }
    println!("after fail_stop");
    std::process::exit(3);
}
