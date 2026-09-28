//! The rest of the simulated seam: extents, sealing, the namespace forms of [F15 §5], the environment guard's probe,
//! the process seam ([OS/proc §10.1]) and the protocol-violation checks of [F15 §3.13].

mod common;

use std::path::Path;

use common::*;
use moirai_vfs::{
    Access, Acquired, Classification, ClassifyDepth, DurabilityClass, EnvGuard, ExtentMethod,
    FsKind, GroupMember, LockByte, LockMode, Locks, OpenHint, OsCode, OsTag, ProbeOutcome,
    ProcHost, Refusal, RootAccess, RootRole, SealedMap, SealedMaps, ShareRetry, StoreFs,
    StoreVolume, SyncKind, VfsErrorKind, Wake, WatchEvent,
};
use moirai_vfs_sim::{
    CrashPlan, EventKind, NsKind, SimConfig, SimWorld, Site, ViolationKind, VolumeProfile,
};

#[test]
fn extents_are_zero_and_their_names_need_the_directory_flush() {
    let w = world(30);
    let (v, r) = proc(&w, "p");
    let vol = match v.classify(&r, ClassifyDepth::Open).unwrap() {
        Classification::Local(vol) => vol,
        Classification::Refused(x) => panic!("{x:?}"),
    };
    assert_eq!(vol.fs, FsKind::Ntfs);
    let f = v.create_extent(&r, rel("log.1"), 3 << 20, &vol).unwrap();
    assert_eq!(v.file_size(&f).unwrap(), 3 << 20);
    // Zero-filled extent: every sector was written with the zeros it held, so none keeps a window, but each is dirty
    // (F15 §5.2: the zeros are written data), and size and name are not durable yet (FM-2.2).
    let s = w.surface();
    let fs = s.file(f.node()).unwrap();
    assert!(fs.sectors.is_empty());
    assert_eq!(fs.rewritten.iter().map(|(a, b)| b - a).sum::<u64>(), 768);
    assert_eq!(fs.durable_size, 0);
    assert!(s.ops.iter().any(|o| o.kind == NsKind::Create));
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert!(!after.exists(&Path::new(STORE).join("log.1")));
    v.sync(&f, SyncKind::DataAndMeta).unwrap();
    v.sync_dir(&r, None).unwrap();
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert_eq!(content_of(&after, "log.1").unwrap(), vec![0u8; 3 << 20]);
    // The sparse method checks free space first and extends by a size change.
    let sparse = StoreVolume {
        extent_method: ExtentMethod::Sparse,
        ..vol
    };
    let g = v.create_extent(&r, rel("log.2"), 1 << 20, &sparse).unwrap();
    assert_eq!(v.file_size(&g).unwrap(), 1 << 20);
    let e = v
        .create_extent(&r, rel("log.3"), 1 << 40, &sparse)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::InsufficientSpace);
    // Recycling zero-fills the extent (G25).
    v.write_at(&g, 0, &[5u8; 100]).unwrap();
    v.recycle_extent(&g, 1 << 20, &sparse).unwrap();
    assert!(read_all(&v, &g)[..100].iter().all(|&b| b == 0));
}

#[test]
fn sealed_files_refuse_writes_and_renames_follow_their_rules() {
    let w = world(31);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "seg.1", b"sealed");
    let f = open_rw(&v, &r, "seg.1");
    v.seal(&f).unwrap();
    assert_eq!(
        v.write_at(&f, 0, b"x").unwrap_err().kind,
        VfsErrorKind::AccessDenied
    );
    assert!(
        w.violations()
            .iter()
            .any(|x| x.kind == ViolationKind::WriteToSealed)
    );
    assert_eq!(
        v.open(&r, rel("seg.1"), Access::ReadWrite, OpenHint::Normal)
            .unwrap_err()
            .kind,
        VfsErrorKind::AccessDenied
    );
    durable_file(&v, &r, "tmpfile", b"t");
    // No-replace onto an existing name fails; replace never targets a sealed file (§5.4, §5.5).
    let e = v
        .rename_noreplace(&r, rel("tmpfile"), &r, rel("seg.1"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AlreadyExists);
    let e = v
        .rename_replace(&r, rel("tmpfile"), &r, rel("seg.1"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AccessDenied);
    // A replace rename is lost as a whole before the directory flush: the old file keeps the name.
    durable_file(&v, &r, "config", b"old");
    durable_file(&v, &r, "config.new", b"new");
    v.rename_replace(&r, rel("config.new"), &r, rel("config"), ShareRetry::None)
        .unwrap();
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert_eq!(content_of(&after, "config").unwrap(), b"old");
    assert_eq!(content_of(&after, "config.new").unwrap(), b"new");
    let after = w.crash_image().materialize(&CrashPlan::newest()).unwrap();
    assert_eq!(content_of(&after, "config").unwrap(), b"new");
    // Unlink clears the attribute and removes a sealed file.
    drop(f);
    v.unlink(&r, rel("seg.1"), ShareRetry::None).unwrap();
    assert!(!w.exists(&Path::new(STORE).join("seg.1")));
}

#[test]
fn the_init_probe_admits_the_volume_and_refuses_a_failing_flush() {
    let w = world(33);
    let (v, r) = proc(&w, "init");
    v.create_dir(&r, rel("tmp")).unwrap();
    v.sync_dir(&r, None).unwrap();
    match v.probe_store(&r).unwrap() {
        ProbeOutcome::Admitted(report) => assert_eq!(report.volume.fs, FsKind::Ntfs),
        ProbeOutcome::Refused(x) => panic!("{x:?}"),
    }
    w.queue_choice_for(&v, Site::FlushFault, 3);
    match v.probe_store(&r).unwrap() {
        ProbeOutcome::Refused(Refusal::NoDurableFlush { call, .. }) => {
            assert_eq!(call, "NtFlushBuffersFileEx");
        }
        other => panic!("{other:?}"),
    }
    // The probe is the one consumer of that failure: no violation follows its clean-up.
    assert!(w.violations().is_empty());
    assert!(v.list_dir(&r, Some(rel("tmp"))).unwrap().is_empty());
    // A refused volume is classified as refused.
    let mut cfg = SimConfig::new(33);
    cfg.volumes.push((
        "/net".into(),
        VolumeProfile {
            refusal: Some(Refusal::Unc),
            ..VolumeProfile::default()
        },
    ));
    let w2 = SimWorld::new(cfg);
    let v2 = w2.process("p");
    let net = v2
        .open_root(Path::new("/net"), RootRole::Store, RootAccess::ReadWrite)
        .unwrap();
    assert_eq!(
        v2.classify(&net, ClassifyDepth::Full).unwrap(),
        Classification::Refused(Refusal::Unc)
    );
    // Renames across volumes fail (NS-2).
    let root = v2
        .open_root(Path::new("/"), RootRole::Other, RootAccess::ReadWrite)
        .unwrap();
    let f = v2.create_new(&root, rel("x")).unwrap();
    drop(f);
    let e = v2
        .rename_noreplace(&root, rel("x"), &net, rel("x"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::CrossDevice);
}

#[test]
fn the_process_seam_watches_parents_and_records_spawns() {
    let w = world(34);
    let parent = w.process_with("claude", None, Some(true));
    let child = w.process_with("moirai", Some(&parent), Some(true));
    assert_eq!(child.parent().unwrap().pid, parent.self_id().pid);
    assert_eq!(child.parent_image().as_deref(), Some("claude"));
    let watch = child.watch_parent().unwrap();
    let wake = child.new_wake().unwrap();
    wake.signal();
    assert_eq!(
        child.wait_parent_or_wake(&watch, &wake).unwrap(),
        WatchEvent::Woken
    );
    // A task blocked in the watch wakes when the parent exits.
    let t = w.spawn(&child, move |v| {
        let wake = v.new_wake().unwrap();
        v.wait_parent_or_wake(&watch, &wake).unwrap()
    });
    let wp = w.clone();
    let pp = parent.clone();
    let killer = w.spawn(&parent, move |v| {
        v.mono_ns_point();
        wp.exit(&pp);
    });
    assert!(!w.run().deadlock);
    assert_eq!(t.end().unwrap().unwrap(), WatchEvent::ParentExited);
    let _ = killer.end();
    // spawn_gc_child creates a simulated process for the harness to run.
    let pid = child
        .spawn_gc_child(Path::new("moirai.exe"), &["gc", "--rollup"], Path::new("/"))
        .unwrap();
    let reqs = w.take_spawn_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].child.self_id().pid, pid);
    assert_eq!(reqs[0].args, vec!["gc".to_owned(), "--rollup".to_owned()]);
}

#[test]
fn protocol_violations_are_reported() {
    let w = world(35);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "LOCK", &[0u8; 36 * 1024]);
    durable_file(&v, &r, "log", &[0u8; 8192]);
    let f = open_rw(&v, &r, "log");
    v.write_at(&f, 0, b"group").unwrap();
    // advise_dontneed on a file with unflushed bytes ([OS/fs §4.12]).
    v.advise_dontneed(&f, 0, 8192);
    // A bounded sharing retry while holding the writer byte ([OS/fs §6.3]).
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let Acquired::Granted(g) = v.try_acquire(&mut c, LockByte::Writer).unwrap() else {
        panic!("free");
    };
    w.hold_exclusive(&Path::new(STORE).join("log"), Some(1))
        .unwrap();
    drop(f);
    v.unlink(&r, rel("log"), ShareRetry::Bounded { total_ms: 100 })
        .unwrap();
    v.release(&mut c, g);
    let kinds: Vec<ViolationKind> = w.violations().iter().map(|x| x.kind).collect();
    assert!(kinds.contains(&ViolationKind::AdviseOnUnflushed));
    assert!(kinds.contains(&ViolationKind::RetryUnderWriterOrFlush));
}

trait Point {
    fn mono_ns_point(&self);
}

impl Point for moirai_vfs_sim::SimVfs {
    /// One scheduling point (a cheap `Vfs` call), so that other tasks may run first.
    fn mono_ns_point(&self) {
        let root = self
            .open_root(Path::new("/"), RootRole::Other, RootAccess::Read)
            .unwrap();
        let _ = self.list_dir(&root, None);
    }
}

/// Review regression: a durable removal or replacement frees its node and its bytes (free space returns).
#[test]
fn durable_unlinks_and_replacements_return_their_space() {
    let w = world(36);
    let (v, r) = proc(&w, "p");
    let free = || v.free_space(&r).unwrap().available;
    let before = free();
    durable_file(&v, &r, "big", &[7u8; 1 << 20]);
    assert_eq!(free(), before - (1 << 20));
    v.unlink(&r, rel("big"), ShareRetry::None).unwrap();
    // The name is gone but a crash may still restore it: its bytes stay until the removal is durable.
    assert_eq!(free(), before - (1 << 20));
    v.sync_dir(&r, None).unwrap();
    assert_eq!(free(), before);
    durable_file(&v, &r, "config", &[1u8; 1 << 20]);
    durable_file(&v, &r, "config.new", &[2u8; 1 << 20]);
    v.rename_replace(&r, rel("config.new"), &r, rel("config"), ShareRetry::None)
        .unwrap();
    v.sync_dir(&r, None).unwrap();
    assert_eq!(free(), before - (1 << 20));
    assert_eq!(w.peek(&Path::new(STORE).join("config")).unwrap()[0], 2);
}

/// Review regression: a failed directory creation leaves nothing (FM-5.4, NS-4); a file parent is `NotFound`.
#[test]
fn a_failed_directory_create_leaves_the_namespace_unchanged() {
    let w = world(37);
    let (v, r) = proc(&w, "p");
    for fault in [1, 2] {
        w.queue_choice_for(&v, Site::CreateFault, fault);
        assert_eq!(
            v.create_dir(&r, rel("d")).unwrap_err().kind,
            VfsErrorKind::DiskFull
        );
        assert!(!w.exists(&Path::new(STORE).join("d")));
        w.queue_choice_for(&v, Site::CreateFault, fault);
        let e = v.create_root(&Path::new(STORE).join("root"), RootRole::Other);
        assert_eq!(e.unwrap_err().kind, VfsErrorKind::DiskFull);
        assert!(!w.exists(&Path::new(STORE).join("root")));
    }
    assert!(w.surface().ops.is_empty());
    durable_file(&v, &r, "file", b"x");
    let e = v
        .create_root(&Path::new(STORE).join("file").join("sub"), RootRole::Other)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::NotFound);
    let root = v
        .create_root(&Path::new(STORE).join("root"), RootRole::Store)
        .unwrap();
    assert_eq!(root.role(), RootRole::Store);
}

/// Review regression: every call's start (a `Point` of phase 0) is paired with its `Return`, on every path, errors and
/// early refusals included ([F13 §1.4] trace predicates).
#[test]
fn every_call_returns_in_the_trace() {
    let w = world(38);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "LOCK", &[0u8; 36 * 1024]);
    let f = v.create_new(&r, rel("f")).unwrap();
    v.write_at(&f, 0, b"abc").unwrap();
    let _ = v.file_size(&f);
    let _ = v.identity(&f);
    let _ = v.root_identity(&r);
    let _ = v.path_identity(&r, rel("f"));
    let _ = v.path_identity(&r, rel("missing"));
    let _ = v.free_space(&r);
    let _ = v.classify(&r, ClassifyDepth::Open);
    let _ = v.open(&r, rel("missing"), Access::Read, OpenHint::Normal);
    let _ = v.create_dir(&r, rel("f/x"));
    let _ = v.remove_dir(&r, rel("missing"));
    let _ = v.list_dir(&r, Some(rel("missing")));
    let _ = v.unlink(&r, rel("missing"), ShareRetry::None);
    let _ = v.rename_noreplace(&r, rel("missing"), &r, rel("x"), ShareRetry::None);
    let _ = v.set_len(&f, 10);
    let _ = v.seal(&f);
    v.sync(&f, SyncKind::DataAndMeta).unwrap();
    let mut buf = [0u8; 16];
    let _ = v.read_at(&f, 0, &mut buf);
    let _ = v.map_sealed(&f, 10, rel("f"));
    let _ = v.map_sealed(&f, 11, rel("f"));
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let Acquired::Granted(g) = v.try_acquire(&mut c, LockByte::Writer).unwrap() else {
        panic!("free");
    };
    let _ = v.probe(&c, LockByte::Flush);
    let _ = v.foreign_lock_check(&c);
    v.release(&mut c, g);
    let _ = v.acquire_within(&mut c, LockByte::Writer, 5);
    drop(c);
    let _ = v.sync_dir(&r, Some(rel("missing")));
    let _ = v.sync_group(&[GroupMember::Dir {
        root: &r,
        dir: Some(rel("missing")),
    }]);
    let t = w.trace();
    let starts = t
        .iter()
        .filter(|e| e.kind == EventKind::Point && e.c == 0)
        .count();
    let returns = t.iter().filter(|e| e.kind == EventKind::Return).count();
    assert_eq!(starts, returns);
}

/// Review regression: every durability failure obliges `fail_stop`, the refusals before any flush included; a
/// `sync_group` directory member needs a writable root like `sync_dir`.
#[test]
fn every_durability_failure_obliges_fail_stop() {
    let w = world(39);
    let (v, r) = proc(&w, "p");
    let f = v.sync_dir(&r, Some(rel("missing"))).unwrap_err();
    assert_eq!(
        (f.kind, f.class),
        (VfsErrorKind::NotFound, DurabilityClass::DurableName)
    );
    let _ = v.create_new(&r, rel("late"));
    assert!(
        w.violations()
            .iter()
            .any(|x| x.kind == ViolationKind::CallAfterDurabilityFailure)
    );
    let (v2, _) = proc(&w, "q");
    let ro = v2
        .open_root(Path::new(STORE), RootRole::Store, RootAccess::Read)
        .unwrap();
    let f = v2
        .sync_group(&[GroupMember::Dir {
            root: &ro,
            dir: None,
        }])
        .unwrap_err();
    assert_eq!(
        (f.kind, f.class),
        (VfsErrorKind::AccessDenied, DurabilityClass::SyncGroup)
    );
    let f = v2.sync_dir(&ro, None).unwrap_err();
    assert_eq!(f.kind, VfsErrorKind::AccessDenied);
}

/// Review regression: macOS issues one `F_FULLFSYNC` per `sync_dir` and one per volume of a `sync_group`
/// ([OS/fs §4.4.3, §4.4.4, §4.13]); the other OSes none.
#[test]
fn macos_counts_its_device_barriers() {
    for (os, want) in [(OsTag::MacOs, 3), (OsTag::Windows, 0), (OsTag::Linux, 0)] {
        let mut cfg = SimConfig::new(40);
        cfg.os = os;
        cfg.volumes.push(("/vol2".into(), VolumeProfile::default()));
        let w = SimWorld::new(cfg);
        w.mkdir_all(Path::new(STORE));
        let (v, r) = proc(&w, "p");
        let other = v
            .open_root(Path::new("/vol2"), RootRole::Other, RootAccess::ReadWrite)
            .unwrap();
        let f = v.create_new(&r, rel("f")).unwrap();
        let g = v.create_new(&other, rel("g")).unwrap();
        v.sync_group(&[
            GroupMember::File {
                file: &f,
                kind: SyncKind::DataAndMeta,
            },
            GroupMember::Dir {
                root: &r,
                dir: None,
            },
            GroupMember::File {
                file: &g,
                kind: SyncKind::DataAndMeta,
            },
            GroupMember::Dir {
                root: &other,
                dir: None,
            },
        ])
        .unwrap();
        v.sync_dir(&r, None).unwrap();
        let c = v.counters();
        assert_eq!(c.full_barriers, want, "{os:?}");
        assert_eq!((c.sync_meta, c.sync_dir), (2, 3));
    }
}

/// Review regression: a read-only volume refuses every change to its namespace and files.
#[test]
fn a_read_only_volume_refuses_every_change() {
    let mut cfg = SimConfig::new(41);
    cfg.volumes.push((
        "/ro".into(),
        VolumeProfile {
            read_only: true,
            ..VolumeProfile::default()
        },
    ));
    let w = SimWorld::new(cfg);
    w.put_file(Path::new("/ro/f"), b"x").unwrap();
    w.mkdir_all(Path::new("/ro/d"));
    let v = w.process_with("p", None, Some(true));
    let r = v
        .open_root(Path::new("/ro"), RootRole::Other, RootAccess::ReadWrite)
        .unwrap();
    let ro = Err(VfsErrorKind::ReadOnlyVolume);
    let kind = |e: moirai_vfs::VfsError| e.kind;
    assert_eq!(v.unlink(&r, rel("f"), ShareRetry::None).map_err(kind), ro);
    assert_eq!(
        v.rename_noreplace(&r, rel("f"), &r, rel("g"), ShareRetry::None)
            .map_err(kind),
        ro
    );
    assert_eq!(v.create_new(&r, rel("n")).map(drop).map_err(kind), ro);
    assert_eq!(v.create_dir(&r, rel("n")).map_err(kind), ro);
    assert_eq!(v.remove_dir(&r, rel("d")).map_err(kind), ro);
    assert_eq!(
        v.open(&r, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .map(drop)
            .map_err(kind),
        ro
    );
    assert!(v.open(&r, rel("f"), Access::Read, OpenHint::Normal).is_ok());
}

/// Review regression: the rename, removal, create and open rules of FM-8.2, FM-8.3 and [OS/fs §4.8, §6.2, §6.4].
#[test]
fn renames_removals_and_opens_follow_the_sharing_and_delete_pending_rules() {
    let w = world(42);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "src", b"s");
    durable_file(&v, &r, "dst", b"d");
    v.create_dir(&r, rel("dir")).unwrap();
    v.sync_dir(&r, None).unwrap();
    // `rename_replace` renames files only.
    assert_eq!(
        v.rename_replace(&r, rel("dir"), &r, rel("moved"), ShareRetry::None)
            .unwrap_err()
            .kind,
        VfsErrorKind::AccessDenied
    );
    // A replaced destination held without the share mode fails the rename (FM-8.2).
    w.hold_exclusive(&Path::new(STORE).join("dst"), Some(1))
        .unwrap();
    let e = v
        .rename_replace(&r, rel("src"), &r, rel("dst"), ShareRetry::None)
        .unwrap_err();
    assert!(matches!(
        e.kind,
        VfsErrorKind::SharingViolation | VfsErrorKind::AccessDenied
    ));
    assert_eq!(w.peek(&Path::new(STORE).join("dst")).unwrap(), b"d");
    // A held directory cannot be removed.
    w.hold_exclusive(&Path::new(STORE).join("dir"), Some(1))
        .unwrap();
    assert!(v.remove_dir(&r, rel("dir")).is_err());
    assert!(w.exists(&Path::new(STORE).join("dir")));
    v.remove_dir(&r, rel("dir")).unwrap();
    // A create over a delete-pending name fails with any of the three answers ([OS/fs §6.4]); a rename onto it with
    // `AlreadyExists` or `AccessDenied` (FM-8.3).
    let holder = v
        .open(&r, rel("dst"), Access::Read, OpenHint::Normal)
        .unwrap();
    w.queue_choice_for(&v, Site::DeletePending, 1);
    v.unlink(&r, rel("dst"), ShareRetry::None).unwrap();
    for (answer, kind) in [
        (0, VfsErrorKind::AlreadyExists),
        (1, VfsErrorKind::AccessDenied),
        (2, VfsErrorKind::DeletePending),
    ] {
        w.queue_choice_for(&v, Site::CreateOverPending, answer);
        assert_eq!(v.create_new(&r, rel("dst")).unwrap_err().kind, kind);
    }
    w.queue_choice_for(&v, Site::CreateOverPending, 1);
    assert_eq!(
        v.rename_noreplace(&r, rel("src"), &r, rel("dst"), ShareRetry::None)
            .unwrap_err()
            .kind,
        VfsErrorKind::AccessDenied
    );
    drop(holder);
    // Opening a directory as a file: Windows reports error 5 ([OS/fs §6.2]); Unix `EISDIR`, an unlisted code.
    v.create_dir(&r, rel("dir2")).unwrap();
    let e = v
        .open(&r, rel("dir2"), Access::Read, OpenHint::Normal)
        .unwrap_err();
    assert_eq!((e.kind, e.os), (VfsErrorKind::AccessDenied, OsCode(5)));
    let mut cfg = SimConfig::new(42);
    cfg.os = OsTag::Linux;
    let lw = SimWorld::new(cfg);
    lw.mkdir_all(&Path::new(STORE).join("d"));
    let (lv, lr) = proc(&lw, "p");
    let e = lv
        .open(&lr, rel("d"), Access::Read, OpenHint::Normal)
        .unwrap_err();
    assert_eq!((e.kind, e.os), (VfsErrorKind::Other, OsCode(21)));
}

/// Review regression: errors 5 and 32 are retried under `Bounded` on Windows only ([OS/fs §6.3]); a `Bounded` call
/// under the writer byte is flagged once, at its start, even when no retry happens.
#[test]
fn bounded_retries_are_windows_only_and_flagged_once() {
    for (os, retried) in [(OsTag::Windows, true), (OsTag::Linux, false)] {
        let mut cfg = SimConfig::new(43);
        cfg.os = os;
        let w = SimWorld::new(cfg);
        w.mkdir_all(Path::new(STORE));
        let (v, r) = proc(&w, "p");
        durable_file(&v, &r, "held", b"x");
        w.hold_exclusive(&Path::new(STORE).join("held"), Some(3))
            .unwrap();
        // Both kinds appear across the attempts, and both are retried.
        w.queue_choice_for(&v, Site::SharingKind, 1);
        w.queue_choice_for(&v, Site::SharingKind, 0);
        let res = v.unlink(&r, rel("held"), ShareRetry::Bounded { total_ms: 1_000 });
        assert_eq!(res.is_ok(), retried, "{os:?}");
        assert_eq!(v.counters().share_retries, if retried { 3 } else { 0 });
        if !retried {
            assert_eq!(res.unwrap_err().kind, VfsErrorKind::AccessDenied);
        }
    }
    // An injected `AccessDenied` of the operation itself is retried too.
    let w = world(44);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "a", b"x");
    w.queue_choice_for(&v, Site::NsFault, 2);
    v.rename_noreplace(
        &r,
        rel("a"),
        &r,
        rel("b"),
        ShareRetry::Bounded { total_ms: 100 },
    )
    .unwrap();
    assert_eq!(v.counters().share_retries, 1);
    // Under the writer byte a `Bounded` call is one violation, retries or not.
    durable_file(&v, &r, "LOCK", &[0u8; 36 * 1024]);
    let mut c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let Acquired::Granted(g) = v.try_acquire(&mut c, LockByte::Writer).unwrap() else {
        panic!("free");
    };
    w.hold_exclusive(&Path::new(STORE).join("b"), Some(4))
        .unwrap();
    v.unlink(&r, rel("b"), ShareRetry::Bounded { total_ms: 1_000 })
        .unwrap();
    let _ = v.unlink(&r, rel("missing"), ShareRetry::Bounded { total_ms: 1_000 });
    v.release(&mut c, g);
    let n = w
        .violations()
        .iter()
        .filter(|x| x.kind == ViolationKind::RetryUnderWriterOrFlush)
        .count();
    assert_eq!(n, 2);
}

/// Review regression: a truncation's cut sectors go with the `sync(DataAndMeta)` that makes the smaller size durable,
/// so a flushed file is clean for `advise_dontneed`.
#[test]
fn a_flushed_truncation_leaves_nothing_unflushed() {
    let w = world(45);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[3u8; 8192]);
    let f = open_rw(&v, &r, "f");
    v.set_len(&f, 100).unwrap();
    v.sync(&f, SyncKind::DataAndMeta).unwrap();
    v.advise_dontneed(&f, 0, 100);
    assert!(w.violations().is_empty());
    assert!(w.surface().file(f.node()).is_none());
}

/// Review regression: a mapping shows the file's current content — each truncation's own zeros, an external rewrite —
/// from one buffer shared by every mapping of an unchanged file, and earlier borrows stay valid.
#[test]
fn mappings_are_coherent_with_the_file() {
    let w = world(46);
    let (d, r) = proc(&w, "init");
    let f = d.create_new(&r, rel("s")).unwrap();
    d.write_at(&f, 0, &[0x5A; 8192]).unwrap();
    d.sync(&f, SyncKind::DataAndMeta).unwrap();
    d.seal(&f).unwrap();
    d.sync_dir(&r, None).unwrap();
    let (m, rm) = proc(&w, "reader");
    let h = m
        .open(&rm, rel("s"), Access::Read, OpenHint::Normal)
        .unwrap();
    let map1 = m.map_sealed(&h, 8192, rel("s")).unwrap();
    let map2 = m.map_sealed(&h, 8192, rel("s")).unwrap();
    let first = map1.bytes();
    assert_eq!(first.as_ptr(), map2.bytes().as_ptr(), "one shared snapshot");
    // An external rewrite of the sealed file (FM-10.1, FM-10.2) shows through the mapping.
    w.external_write(&Path::new(STORE).join("s"), 0, b"EVIL")
        .unwrap();
    assert_eq!(&map1.bytes()[..4], b"EVIL");
    assert_eq!(first[0], 0x5A, "an earlier borrow keeps its bytes");
    // Two truncations, each read as zeros beyond its own end (FM-9.2).
    w.external_truncate(&Path::new(STORE).join("s"), 6000)
        .unwrap();
    w.queue_choice_for(&m, Site::MapTruncated, 1);
    let b = map1.bytes();
    assert!(b[4..6000].iter().all(|&x| x == 0x5A) && b[6000..].iter().all(|&x| x == 0));
    w.external_truncate(&Path::new(STORE).join("s"), 1000)
        .unwrap();
    w.queue_choice_for(&m, Site::MapTruncated, 1);
    let b = map1.bytes();
    assert!(b[4..1000].iter().all(|&x| x == 0x5A) && b[1000..].iter().all(|&x| x == 0));
    assert_eq!(map1.len(), 8192);
}

/// Review regression: a queued choice is consumed by its site even where the site has one answer, and a node-keyed
/// choice waits for its node.
#[test]
fn queued_choices_never_drift() {
    let w = world(47);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "x", &[0u8; 4096]);
    durable_file(&v, &r, "y", &[0u8; 4096]);
    let fx = open_rw(&v, &r, "x");
    let ny = open_rw(&v, &r, "y").node();
    // A write of the bytes it holds, then a failed flush: poisoned with K = {zeros}, |K| = 1.
    v.write_at(&fx, 0, &[0u8; 4096]).unwrap();
    w.queue_choice_for(&v, Site::FlushFault, 1);
    let failure = v.sync(&fx, SyncKind::Data).unwrap_err();
    let (v2, r2) = proc(&w, "q");
    let gx = open_rw(&v2, &r2, "x");
    w.queue_choice(Site::PoisonRead, 1);
    let mut buf = [9u8; 512];
    v2.read_at(&gx, 0, &mut buf).unwrap();
    assert!(buf.iter().all(|&b| b == 0));
    let injected = w
        .trace()
        .iter()
        .filter(|e| e.kind == EventKind::Injected && e.a == u64::from(Site::PoisonRead.code()))
        .count();
    assert_eq!(injected, 1, "consumed at the one-answer site");
    // A choice keyed to node y is not taken by a write to x.
    let gy = open_rw(&v2, &r2, "y");
    w.queue_choice_on(Some(&v2), Site::WriteFault, ny, 1);
    v2.write_at(&gx, 0, b"ok").unwrap();
    assert_eq!(
        v2.write_at(&gy, 0, b"no").unwrap_err().kind,
        VfsErrorKind::DiskFull
    );
    assert_eq!(fail(&v, failure), moirai_vfs_sim::SimUnwind::Died);
}

fn fail(v: &moirai_vfs_sim::SimVfs, f: moirai_vfs::DurabilityFailure) -> moirai_vfs_sim::SimUnwind {
    match moirai_vfs_sim::catch_death(|| {
        v.fail_stop(f);
    }) {
        Ok(()) => unreachable!("fail_stop returned"),
        Err(u) => u,
    }
}
