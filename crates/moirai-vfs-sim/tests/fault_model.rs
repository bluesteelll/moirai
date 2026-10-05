//! One test per fault-model item ([F15 §3.1]–[F15 §3.12]), each showing the adverse behaviour the in-memory `Vfs` must
//! exhibit (WP-31 acceptance).

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::*;
use moirai_vfs::{
    Access, Acquired, BootIdentity, Clock, DurabilityClass, EnvGuard, ExtentMethod, GroupMember,
    Liveness, LockByte, LockMode, Locks, OpenHint, OsTag, ProbeResult, ProcHost, ProcId,
    RootAccess, RootRole, SealedMap, SealedMaps, ShareRetry, SlotIndex, StoreFs, StoreVolume,
    SyncKind, VfsErrorKind, WaitMode,
};
use moirai_vfs_sim::{
    CrashPlan, DeathCause, DeathPlan, EventKind, FaultRates, FilePlan, NsKind, PartialWrite,
    PlanError, ReleaseDelayLaw, SectorKind, SectorPick, SimConfig, SimUnwind, SimVfs, SimWorld,
    Site, TaskEnd, ViolationKind, VolumeProfile, catch_death,
};

fn path(name: &str) -> std::path::PathBuf {
    Path::new(STORE).join(name)
}

#[test]
fn fm01_unflushed_sectors_revert_in_any_subset_and_one_sector_tears() {
    let w = world(1);
    let (v, r) = proc(&w, "writer");
    durable_file(&v, &r, "f", &[0x11; 3 * 4096]);
    let f = open_rw(&v, &r, "f");
    v.write_at(&f, 0, &[0x22; 4096]).unwrap();
    v.write_at(&f, 4096, &[0x33; 4096]).unwrap();
    v.write_at(&f, 4096, &[0x44; 4096]).unwrap();
    let node = f.node();
    let img = w.crash_image();
    let s = img.surface();
    let fs = s.file(node).expect("the file has unflushed sectors");
    // Sector 2 is clean and is not on the surface; sector 1 keeps both of its versions (FM-1.1).
    assert_eq!(fs.sectors.len(), 2);
    assert_eq!((fs.sectors[0].index, fs.sectors[0].candidates), (0, 2));
    assert_eq!((fs.sectors[1].index, fs.sectors[1].candidates), (1, 3));

    let plan = |picks: &[(u64, SectorPick)]| {
        CrashPlan::baseline().with_file(
            node,
            FilePlan {
                sectors: picks.iter().copied().collect(),
                ..FilePlan::default()
            },
        )
    };
    // The later write survives while the earlier one is lost (FM-1.5); the clean sector never changes (FM-1.3).
    let after = img
        .materialize(&plan(&[
            (0, SectorPick::Version(0)),
            (1, SectorPick::Version(2)),
        ]))
        .unwrap();
    let c = content_of(&after, "f").unwrap();
    assert!(c[..4096].iter().all(|&b| b == 0x11));
    assert!(c[4096..8192].iter().all(|&b| b == 0x44));
    assert!(c[8192..].iter().all(|&b| b == 0x11));
    // An intermediate version may be the durable one (FM-1.1, OP-3).
    let after = img
        .materialize(&plan(&[(1, SectorPick::Version(1))]))
        .unwrap();
    assert!(
        content_of(&after, "f").unwrap()[4096..8192]
            .iter()
            .all(|&b| b == 0x33)
    );
    // One torn sector: its sub-sectors come independently from the allowed contents (FM-1.2).
    let after = img
        .materialize(&plan(&[(
            0,
            SectorPick::Subsectors([0, 1, 0, 1, 0, 1, 0, 1]),
        )]))
        .unwrap();
    let c = content_of(&after, "f").unwrap();
    assert_eq!(
        subsector_values(&c[..4096]),
        vec![0x11, 0x22, 0x11, 0x22, 0x11, 0x22, 0x11, 0x22]
    );
    // Two torn sectors in one file are refused, and so is a pick on a clean sector.
    assert_eq!(
        img.materialize(&plan(&[
            (0, SectorPick::Subsectors([0; 8])),
            (1, SectorPick::Subsectors([1; 8]))
        ]))
        .unwrap_err(),
        PlanError::TwoTornSectors { node }
    );
    assert_eq!(
        img.materialize(&plan(&[(2, SectorPick::Version(0))]))
            .unwrap_err(),
        PlanError::CleanSector { node, sector: 2 }
    );
    // Every subset of the dirty sectors at baseline or newest is a distinct reachable state.
    let mut seen = BTreeSet::new();
    for mask in 0u8..4 {
        let picks: Vec<(u64, SectorPick)> = fs
            .dirty()
            .enumerate()
            .map(|(i, sv)| {
                let newest = mask & (1 << i) != 0;
                (
                    sv.index,
                    SectorPick::Version(if newest { sv.candidates - 1 } else { 0 }),
                )
            })
            .collect();
        seen.insert(content_of(&img.materialize(&plan(&picks)).unwrap(), "f").unwrap());
    }
    assert_eq!(seen.len(), 4);
}

#[test]
fn fm02_names_are_lost_in_any_subset_until_every_parent_is_synced() {
    let w = world(2);
    let (v, r) = proc(&w, "p");
    for name in ["a", "b"] {
        let f = v.create_new(&r, rel(name)).unwrap();
        v.write_at(&f, 0, b"data").unwrap();
        v.sync(&f, SyncKind::DataAndMeta).unwrap();
    }
    // A data flush made no name durable (FM-2.5): both creates are pending.
    let creates: Vec<u64> = w
        .surface()
        .ops
        .iter()
        .filter(|o| o.kind == NsKind::Create)
        .map(|o| o.id)
        .collect();
    assert_eq!(creates.len(), 2);
    // The later create survives and the earlier one is lost: there is no prefix order (FM-2.3).
    let after = w
        .crash_image()
        .materialize(&CrashPlan::baseline().with_survivors([creates[1]]))
        .unwrap();
    assert!(!after.exists(&path("a")));
    assert_eq!(content_of(&after, "b").unwrap(), b"data");
    // After a sync_dir both are durable.
    v.sync_dir(&r, None).unwrap();
    assert!(w.surface().ops.is_empty());
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert!(after.exists(&path("a")) && after.exists(&path("b")));

    // A cross-directory rename is durable only after both parents are synced (FM-2.4).
    v.create_dir(&r, rel("d1")).unwrap();
    v.create_dir(&r, rel("d2")).unwrap();
    v.sync_dir(&r, None).unwrap();
    let x = v.create_new(&r, rel("d1/x")).unwrap();
    v.sync(&x, SyncKind::DataAndMeta).unwrap();
    v.sync_dir(&r, Some(rel("d1"))).unwrap();
    v.rename_noreplace(&r, rel("d1/x"), &r, rel("d2/y"), ShareRetry::None)
        .unwrap();
    v.sync_dir(&r, Some(rel("d2"))).unwrap();
    let ops = w.surface().ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].kind, NsKind::Rename);
    assert_eq!(ops[0].synced, vec![false, true]);
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert!(after.exists(&path("d1/x")) && !after.exists(&path("d2/y")));
    v.sync_dir(&r, Some(rel("d1"))).unwrap();
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert!(!after.exists(&path("d1/x")) && after.exists(&path("d2/y")));

    // `sync(Data)` makes no size durable (FM-2.1): a crash may keep the old size.
    let g = v.create_new(&r, rel("g")).unwrap();
    v.sync_dir(&r, None).unwrap();
    v.write_at(&g, 0, &[7u8; 100]).unwrap();
    v.sync(&g, SyncKind::Data).unwrap();
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert_eq!(content_of(&after, "g").unwrap(), Vec::<u8>::new());
}

#[test]
fn fm03_a_failed_flush_leaves_its_range_indeterminate_forever() {
    let w = world(3);
    let (v, r) = proc(&w, "p1");
    durable_file(&v, &r, "f", &[0x11; 4096]);
    let f = open_rw(&v, &r, "f");
    let node = f.node();
    v.write_at(&f, 0, &[0xAA; 4096]).unwrap();
    w.queue_choice_for(&v, Site::FlushFault, 1);
    let failure = v.sync(&f, SyncKind::Data).unwrap_err();
    assert_eq!(failure.kind, VfsErrorKind::Io);
    assert_eq!(failure.class, DurabilityClass::Durable);

    // Every read draws each sub-sector from K = {old, new} afresh: two reads of one range differ (FM-3.2).
    let (v2, r2) = proc(&w, "p2");
    let f2 = open_rw(&v2, &r2, "f");
    let reads = |v: &moirai_vfs_sim::SimVfs, f: &moirai_vfs_sim::SimFile| -> BTreeSet<Vec<u8>> {
        (0..32)
            .map(|_| subsector_values(&read_all(v, f)[..4096]))
            .collect()
    };
    let seen = reads(&v2, &f2);
    assert!(seen.len() > 1, "reads of a poisoned sector vary");
    assert!(seen.iter().flatten().all(|&b| b == 0x11 || b == 0xAA));
    // A later successful flush, by another process, proves nothing (FM-3.4).
    v2.sync(&f2, SyncKind::DataAndMeta).unwrap();
    assert!(reads(&v2, &f2).len() > 1);
    // The failing process must `fail_stop`; a further write is a detected violation ([F15 §3.13]).
    v.write_at(&f, 4096, b"late").unwrap();
    assert!(
        w.violations()
            .iter()
            .any(|x| x.kind == ViolationKind::CallAfterDurabilityFailure)
    );
    assert_eq!(fail(&v, failure), SimUnwind::Died);
    assert_eq!(w.death(&v), Some(DeathCause::FailStop));
    assert!(w.stderr_lines()[0].starts_with(
        "error[durability_failure]: NtFlushBuffersFileEx (durable) failed: os 1117 ERROR_IO_DEVICE;"
    ));

    // Poisoning survives a system crash (FM-3.3), with no torn-sector bound.
    let after = w.crash_image().materialize(&CrashPlan::newest()).unwrap();
    let s = after.surface();
    assert_eq!(s.file(node).unwrap().sectors[0].state, SectorKind::Poisoned);
    // Only a re-write ends it (FM-3.5): re-written and flushed, the sector is clean and stable.
    let (v3, r3) = proc(&after, "p3");
    let f3 = open_rw(&v3, &r3, "f");
    v3.write_at(&f3, 0, &[0xBB; 4096]).unwrap();
    v3.sync(&f3, SyncKind::DataAndMeta).unwrap();
    assert!(after.surface().file(node).is_none());
    assert_eq!(reads(&v3, &f3).len(), 1);
    assert!(read_all(&v3, &f3)[..4096].iter().all(|&b| b == 0xBB));
}

fn fail(v: &moirai_vfs_sim::SimVfs, f: moirai_vfs::DurabilityFailure) -> SimUnwind {
    match catch_death(|| {
        v.fail_stop(f);
    }) {
        Ok(()) => unreachable!("fail_stop returned"),
        Err(u) => u,
    }
}

/// One run of a writer task and a reader task of one 4 KiB range: the read's result.
fn overlapping_read(seed: u64) -> Vec<u8> {
    let w = world(seed);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[0u8; 4096]);
    let open = |v: &moirai_vfs_sim::SimVfs| {
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .unwrap();
        v.open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap()
    };
    let writer = w.spawn(&v, move |v| {
        let f = open(&v);
        v.write_at(&f, 0, &[0xFF; 4096]).unwrap();
    });
    let reader = w.spawn(&v, move |v| {
        let f = open(&v);
        let mut buf = vec![0u8; 4096];
        v.read_exact_at(&f, 0, &mut buf).unwrap();
        buf
    });
    assert!(!w.run().deadlock);
    writer.end().unwrap().unwrap();
    reader.end().unwrap().unwrap()
}

#[test]
fn fm04_a_read_concurrent_with_a_write_returns_any_mix_of_sub_sectors() {
    let mut mixed = 0;
    for seed in 0..64 {
        let subs = subsector_values(&overlapping_read(seed));
        assert!(subs.iter().all(|&b| b == 0 || b == 0xFF));
        if subs.contains(&0) && subs.contains(&0xFF) {
            mixed += 1;
        }
    }
    assert!(
        mixed > 0,
        "some interleaving tore a read at sub-sector granularity"
    );
    // A read that overlaps no write returns the latest completed write (FM-4.2).
    let w = world(4);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[0u8; 4096]);
    let f = open_rw(&v, &r, "f");
    v.write_at(&f, 0, &[0xFF; 4096]).unwrap();
    let (v2, r2) = proc(&w, "q");
    assert!(
        read_all(&v2, &open_rw(&v2, &r2, "f"))
            .iter()
            .all(|&b| b == 0xFF)
    );
}

#[test]
fn fm05_disk_full_on_any_write_flush_create_and_namespace_operation() {
    let w = world(5);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[0x11; 4096]);
    let f = open_rw(&v, &r, "f");
    // An overwrite of written space fails, and the range holds a mix of old and new bytes (FM-5.1, FM-5.2).
    w.queue_choice_for(&v, Site::WriteFault, 1);
    w.queue_choice_for(
        &v,
        Site::PartialWrite,
        PartialWrite::Prefix(1000).to_choice(),
    );
    let e = v.write_at(&f, 0, &[0x22; 4096]).unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::DiskFull);
    let c = w.peek(&path("f")).unwrap();
    assert!(c[..1000].iter().all(|&b| b == 0x22) && c[1000..].iter().all(|&b| b == 0x11));
    // Disk-full is not persistent: the next write succeeds (FM-5.6).
    v.write_at(&f, 0, &[0x33; 4096]).unwrap();
    // A flush fails with DiskFull and is a failed flush (FM-5.3).
    w.queue_choice_for(&v, Site::FlushFault, 2);
    assert_eq!(
        v.sync(&f, SyncKind::Data).unwrap_err().kind,
        VfsErrorKind::DiskFull
    );
    assert_eq!(
        w.surface().file(f.node()).unwrap().sectors[0].state,
        SectorKind::Poisoned
    );
    // A failed create leaves the name absent, or present as an empty file (FM-5.4, OP-8).
    let (v2, r2) = proc(&w, "p2");
    w.queue_choice_for(&v2, Site::CreateFault, 1);
    assert_eq!(
        v2.create_new(&r2, rel("g")).unwrap_err().kind,
        VfsErrorKind::DiskFull
    );
    assert!(!w.exists(&path("g")));
    w.queue_choice_for(&v2, Site::CreateFault, 2);
    assert_eq!(
        v2.create_new(&r2, rel("g")).unwrap_err().kind,
        VfsErrorKind::DiskFull
    );
    assert_eq!(w.peek(&path("g")).unwrap(), Vec::<u8>::new());
    // A failed rename changes nothing; a failed sync_dir leaves every operation pending (FM-3.7).
    w.queue_choice_for(&v2, Site::NsFault, 1);
    let e = v2
        .rename_noreplace(&r2, rel("g"), &r2, rel("h"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::DiskFull);
    assert!(w.exists(&path("g")) && !w.exists(&path("h")));
    let pending = w.surface().ops.len();
    assert!(pending > 0);
    w.queue_choice_for(&v2, Site::SyncDirFault, 2);
    assert_eq!(
        v2.sync_dir(&r2, None).unwrap_err().kind,
        VfsErrorKind::DiskFull
    );
    assert_eq!(w.surface().ops.len(), pending);
}

#[test]
fn fm06_a_pause_of_any_length_while_holding_a_lock() {
    let w = world(6);
    let (d, r) = proc(&w, "init");
    durable_file(&d, &r, "LOCK", &[0u8; 36 * 1024]);
    let a = w.process_with("a", None, Some(true));
    let b = w.process_with("b", None, Some(true));
    let wa = w.clone();
    let ta = w.spawn(&a, move |v| {
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .unwrap();
        let mut c = v.lock_client(&root, LockMode::Acquire).unwrap();
        let Acquired::Granted(g) = v.try_acquire(&mut c, LockByte::Writer).unwrap() else {
            panic!("the writer byte is free at first");
        };
        let before = v.mono_ns();
        // Ten minutes' pause at the next event, holding the writer byte (FM-6.1, FM-6.2).
        wa.queue_choice_for(&v, Site::Pause, 600_000_000_000);
        v.file_size(v.lock_data(&c)).unwrap();
        let paused = v.mono_ns() - before;
        v.release(&mut c, g);
        paused
    });
    let tb = w.spawn(&b, |v| {
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .unwrap();
        let mut c = v.lock_client(&root, LockMode::Acquire).unwrap();
        while v.probe(&c, LockByte::Writer) != ProbeResult::Held {}
        let t0 = v.mono_ns();
        let busy = match v.acquire_within(&mut c, LockByte::Writer, 2_000).unwrap() {
            Acquired::Busy => true,
            Acquired::Granted(g) => {
                v.release(&mut c, g);
                false
            }
        };
        (busy, v.mono_ns() - t0)
    });
    assert!(!w.run().deadlock);
    assert!(ta.end().unwrap().unwrap() >= 600_000_000_000);
    let (busy, waited) = tb.end().unwrap().unwrap();
    assert!(busy, "the paused holder kept the byte past the wait bound");
    assert!((2_000_000_000..600_000_000_000).contains(&waited));
}

#[test]
fn fm07_wall_steps_monotonic_and_boot_clocks_and_the_boot_identity() {
    let w = world(7);
    let (v, _) = proc(&w, "p");
    // The wall clock steps backward, explicitly or as a drawn step at a reading (FM-7.1).
    let w1 = v.wall_ms();
    w.step_wall(&v, -86_400_000);
    let w2 = v.wall_ms();
    assert!(w2 < w1);
    w.queue_choice_for(&v, Site::WallStep, (-5_000i64) as u64);
    assert!(v.wall_ms() < w2);
    // A suspend: the boot clock counts it, the monotonic clock may not (FM-7.2, FM-7.3, OP-14).
    let (m0, b0) = (v.mono_ns(), v.boot_ns());
    w.suspend(3_600_000_000_000, false);
    let (m1, b1) = (v.mono_ns(), v.boot_ns());
    assert!(b1 - b0 >= 3_600_000_000_000);
    assert!(m1 >= m0 && m1 - m0 < 3_600_000_000_000);
    // The boot identity: constant within a boot; a single read may be unknown; a process may be in Unknown-boot mode.
    let BootIdentity::Known(id1) = v.boot_identity() else {
        panic!("the process was started with a known boot");
    };
    w.queue_choice_for(&v, Site::BootRead, 1);
    assert!(matches!(v.boot_identity(), BootIdentity::Unknown(_)));
    assert_eq!(v.boot_identity(), BootIdentity::Known(id1));
    let u = w.process_with("u", None, Some(false));
    assert!(matches!(u.boot_identity(), BootIdentity::Unknown(_)));
    assert_eq!(u.self_id().boot_hash, 0);
    // A crash starts a new boot: a new identity (FM-7.4, G-10). [OS/proc §6.1]: a process of another known boot is
    // `Unknown` to a checker whose boot is known (row 2); a checker in Unknown-boot mode looks it up and finds no such
    // process in its boot (row 5).
    let old = v.self_id();
    w.crash(&CrashPlan::seeded()).unwrap();
    assert_eq!(w.boot().0, 2);
    let (v2, _) = proc(&w, "p2");
    assert_ne!(v2.boot_identity(), BootIdentity::Known(id1));
    assert_eq!(v2.alive(&old), Liveness::Unknown);
    let blind = w.process_with("blind", None, Some(false));
    assert_eq!(blind.alive(&old), Liveness::Dead);
    assert_eq!(blind.alive(&v2.self_id()), Liveness::Alive);
    assert_eq!(catch_death(|| v.mono_ns()), Err(SimUnwind::Died));
}

/// [OS/proc §3.1, §3.2, §6.1] in the simulator: the flags follow the simulated OS, and `alive` answers the rows in order.
#[test]
fn liveness_follows_the_rows_of_the_process_table() {
    for os in [OsTag::Windows, OsTag::Linux, OsTag::MacOs] {
        let mut cfg = SimConfig::new(70);
        cfg.os = os;
        let w = SimWorld::new(cfg);
        let a = w.process_with("a", None, Some(true));
        let b = w.process_with("b", None, Some(true));
        let id = b.self_id();
        assert_eq!(
            id.flags & ProcId::START_BOOT_RELATIVE != 0,
            os == OsTag::Linux,
            "{os:?}"
        );
        assert!(ProcId::from_bytes(&id.to_bytes()).is_some());
        assert_eq!(a.alive(&id), Liveness::Alive);
        // Row 1: another OS, or reserved flag bits.
        let other = if os == OsTag::Windows { 2 } else { 1 };
        assert_eq!(a.alive(&ProcId { os: other, ..id }), Liveness::Unknown);
        assert_eq!(
            a.alive(&ProcId {
                flags: id.flags | 0x40,
                ..id
            }),
            Liveness::Unknown
        );
        // Row 2: both boots known and different.
        assert_eq!(
            a.alive(&ProcId {
                boot_hash: id.boot_hash ^ 2,
                ..id
            }),
            Liveness::Unknown
        );
        // Row 5: no such process; row 7: another start.
        assert_eq!(a.alive(&ProcId { pid: 7, ..id }), Liveness::Dead);
        assert_eq!(
            a.alive(&ProcId {
                start: id.start ^ 1,
                ..id
            }),
            Liveness::Dead
        );
        // Row 6: exited.
        w.exit(&b);
        assert_eq!(a.alive(&id), Liveness::Dead);
    }
}

#[test]
fn fm08_lock_release_lags_a_death_sharing_violations_repeat_and_unlinks_linger() {
    let w = world(8);
    let (d, r) = proc(&w, "init");
    durable_file(&d, &r, "LOCK", &[0u8; 36 * 1024]);
    // A dead holder's byte stays held beyond the wait bound (class (b), FM-8.1).
    let (a, ra) = proc(&w, "a");
    let mut ca = a.lock_client(&ra, LockMode::Acquire).unwrap();
    assert!(matches!(
        a.try_acquire(&mut ca, LockByte::Writer).unwrap(),
        Acquired::Granted(_)
    ));
    w.kill(
        &a,
        DeathPlan {
            release_class: Some(1),
            release_delay_ns: Some(5_000_000_000),
            ..DeathPlan::default()
        },
    );
    let (b, rb) = proc(&w, "b");
    let mut cb = b.lock_client(&rb, LockMode::Acquire).unwrap();
    assert_eq!(
        b.probe(&cb, LockByte::Writer),
        ProbeResult::Held,
        "Held although the holder is dead"
    );
    assert_eq!(
        b.acquire_within(&mut cb, LockByte::Writer, 2_000).unwrap(),
        Acquired::Busy
    );
    w.advance(3_500_000_000);
    assert_eq!(b.probe(&cb, LockByte::Writer), ProbeResult::Free);
    let Acquired::Granted(g) = b.try_acquire(&mut cb, LockByte::Writer).unwrap() else {
        panic!("the byte is free after the delay");
    };
    b.release(&mut cb, g);
    // Class (c): never released within the scenario.
    let (c, rc) = proc(&w, "c");
    let mut cc = c.lock_client(&rc, LockMode::Acquire).unwrap();
    assert!(matches!(
        c.try_acquire(&mut cc, LockByte::Writer).unwrap(),
        Acquired::Granted(_)
    ));
    w.kill(
        &c,
        DeathPlan {
            release_class: Some(2),
            ..DeathPlan::default()
        },
    );
    w.advance(3_600_000_000_000);
    assert_eq!(b.probe(&cb, LockByte::Writer), ProbeResult::Held);

    // Sharing violations repeat for any number of attempts (FM-8.2), as Windows error 32 or 5.
    durable_file(&b, &rb, "s", b"x");
    w.hold_exclusive(&path("s"), Some(3)).unwrap();
    let mut kinds = BTreeSet::new();
    for answer in [0, 1, 0] {
        w.queue_choice_for(&b, Site::SharingKind, answer);
        let e = b
            .open(&rb, rel("s"), Access::Read, OpenHint::Normal)
            .unwrap_err();
        kinds.insert(format!("{:?}/{}", e.kind, e.os.0));
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        vec!["AccessDenied/5", "SharingViolation/32"]
    );
    assert!(
        b.open(&rb, rel("s"), Access::Read, OpenHint::Normal)
            .is_ok()
    );
    w.hold_exclusive(&path("s"), None).unwrap();
    let t0 = b.mono_ns();
    let e = b
        .unlink(&rb, rel("s"), ShareRetry::Bounded { total_ms: 500 })
        .unwrap_err();
    assert!(matches!(
        e.kind,
        VfsErrorKind::SharingViolation | VfsErrorKind::AccessDenied
    ));
    assert!(b.mono_ns() - t0 >= 400_000_000);
    assert!(b.counters().share_retries > 0);

    // A delete-pending name stays occupied until the last handle closes (FM-8.3).
    durable_file(&b, &rb, "dp", b"y");
    let holder = b
        .open(&rb, rel("dp"), Access::Read, OpenHint::Normal)
        .unwrap();
    let (e, re) = proc(&w, "e");
    w.queue_choice_for(&e, Site::DeletePending, 1);
    e.unlink(&re, rel("dp"), ShareRetry::None).unwrap();
    let err = e
        .open(&re, rel("dp"), Access::Read, OpenHint::Normal)
        .unwrap_err();
    assert_eq!(err.kind, VfsErrorKind::DeletePending);
    w.queue_choice_for(&e, Site::CreateOverPending, 0);
    assert_eq!(
        e.create_new(&re, rel("dp")).unwrap_err().kind,
        VfsErrorKind::AlreadyExists
    );
    assert!(
        e.list_dir(&re, None)
            .unwrap()
            .iter()
            .any(|d| d.name.as_segment() == Some("dp"))
    );
    drop(holder);
    assert!(!w.exists(&path("dp")));
    assert!(w.surface().ops.iter().any(|o| o.kind == NsKind::Remove));
}

#[test]
fn fm09_a_mapped_read_of_a_truncated_or_faulty_sealed_file_ends_the_process() {
    let w = world(9);
    let (d, r) = proc(&w, "init");
    for name in ["s1", "s2", "s3"] {
        let f = d.create_new(&r, rel(name)).unwrap();
        d.write_at(&f, 0, &[0x5A; 8192]).unwrap();
        d.sync(&f, SyncKind::DataAndMeta).unwrap();
        d.seal(&f).unwrap();
    }
    d.sync_dir(&r, None).unwrap();
    let map = |name: &'static str| {
        let (m, rm) = proc(&w, name);
        let f = m
            .open(&rm, rel(name), Access::Read, OpenHint::Normal)
            .unwrap();
        let map = m.map_sealed(&f, 8192, rel(name)).unwrap();
        (m, map)
    };
    // An unmodified sealed file maps to its bytes (FM-9.3).
    let (m1, map1) = map("s1");
    assert!(map1.bytes().iter().all(|&b| b == 0x5A));
    // An external truncation below the read ends the reading process (FM-9.1, FM-10.2).
    w.external_truncate(&path("s1"), 4096).unwrap();
    w.queue_choice_for(&m1, Site::MapTruncated, 0);
    assert_eq!(catch_death(|| map1.bytes().len()), Err(SimUnwind::Died));
    assert_eq!(w.death(&m1), Some(DeathCause::MapFault));
    // Or the read returns zeros beyond the new end (FM-9.2).
    let (m2, map2) = map("s2");
    w.external_truncate(&path("s2"), 4096).unwrap();
    w.queue_choice_for(&m2, Site::MapTruncated, 1);
    let bytes = map2.bytes();
    assert!(bytes[..4096].iter().all(|&b| b == 0x5A) && bytes[4096..].iter().all(|&b| b == 0));
    // An injected media fault ends the reader (FM-9.1).
    let (m3, map3) = map("s3");
    w.inject_map_fault(&path("s3")).unwrap();
    assert_eq!(catch_death(|| map3.bytes().len()), Err(SimUnwind::Died));
    assert_eq!(w.death(&m3), Some(DeathCause::MapFault));
}

#[test]
fn fm10_external_actors_truncate_rewrite_and_replace_store_files() {
    let w = world(10);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "LOCK", &[0u8; 36 * 1024]);
    durable_file(&v, &r, "seg", &[1u8; 8192]);
    let f = open_rw(&v, &r, "seg");
    // A foreign client's rewrite and truncation are visible to moirai's reads (FM-10.1).
    w.external_write(&path("seg"), 0, b"EVIL").unwrap();
    assert_eq!(&read_all(&v, &f)[..4], b"EVIL");
    w.external_truncate(&path("seg"), 100).unwrap();
    assert_eq!(v.file_size(&f).unwrap(), 100);
    // They are writes like any other: a crash may revert them (FM-1 applies to foreign clients).
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert_eq!(content_of(&after, "seg").unwrap(), vec![1u8; 8192]);
    // `LOCK` replaced under its name (FM-10.3): the identity check of [80 §2.2.1] item 9 can see it.
    let c = v.lock_client(&r, LockMode::Acquire).unwrap();
    let held = v.identity(v.lock_data(&c)).unwrap();
    w.external_replace(&path("LOCK"), &[0u8; 36 * 1024])
        .unwrap();
    assert_ne!(v.path_identity(&r, rel("LOCK")).unwrap(), held);
    let c2 = v.lock_client(&r, LockMode::Acquire).unwrap();
    assert_eq!(
        v.identity(v.lock_data(&c2)).unwrap(),
        v.path_identity(&r, rel("LOCK")).unwrap()
    );
}

#[test]
fn fm11_a_flush_holder_dies_inside_its_flush_with_each_outcome() {
    for outcome in 0u8..3 {
        let w = world(11);
        let (d, r) = proc(&w, "init");
        durable_file(&d, &r, "log", &[0u8; 16384]);
        let (a, ra) = proc(&w, "appender");
        let fa = open_rw(&a, &ra, "log");
        a.write_at(&fa, 0, &[0x77; 4096]).unwrap();
        let (h, rh) = proc(&w, "flusher");
        let fh = open_rw(&h, &rh, "log");
        // The flush's start is the next scheduling point and its interval the one after: the holder dies there.
        w.kill_at(
            w.points() + 2,
            &h,
            DeathPlan {
                flush: Some(outcome),
                ..DeathPlan::default()
            },
        );
        let died = catch_death(|| {
            let _ = h.sync(&fh, SyncKind::Data);
        });
        assert_eq!(died, Err(SimUnwind::Died));
        let state = w.surface().file(fa.node()).map(|f| f.sectors[0].state);
        let want = match outcome {
            0 => None,
            1 => Some(SectorKind::Poisoned),
            _ => Some(SectorKind::Dirty),
        };
        assert_eq!(state, want, "outcome {outcome}");
        // No live client was told: the appender goes on.
        a.write_at(&fa, 4096, &[0x78; 16]).unwrap();
    }
}

#[test]
fn fm12_read_errors_are_transient_or_persistent_across_crashes() {
    let w = world(12);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[9u8; 8192]);
    let f = open_rw(&v, &r, "f");
    let mut buf = [0u8; 4096];
    w.inject_read_error(&path("f"), 0, 512, false).unwrap();
    assert_eq!(
        v.read_at(&f, 0, &mut buf).unwrap_err().kind,
        VfsErrorKind::Io
    );
    assert_eq!(v.read_at(&f, 0, &mut buf).unwrap(), 4096);
    w.inject_read_error(&path("f"), 4096, 4096, true).unwrap();
    for _ in 0..3 {
        assert_eq!(
            v.read_at(&f, 4096, &mut buf).unwrap_err().kind,
            VfsErrorKind::Io
        );
    }
    // A drawn read error that persists (FM-12.2).
    w.queue_choice_for(&v, Site::ReadFault, 2);
    let mut small = [0u8; 10];
    assert_eq!(
        v.read_at(&f, 100, &mut small).unwrap_err().kind,
        VfsErrorKind::Io
    );
    assert_eq!(
        v.read_at(&f, 100, &mut small).unwrap_err().kind,
        VfsErrorKind::Io
    );
    assert_eq!(v.read_at(&f, 1000, &mut small).unwrap(), 10);
    // Persistent errors survive a system crash (§2.5 step 5).
    w.crash(&CrashPlan::newest()).unwrap();
    let (v2, r2) = proc(&w, "p2");
    let f2 = open_rw(&v2, &r2, "f");
    assert_eq!(
        v2.read_at(&f2, 4096, &mut buf).unwrap_err().kind,
        VfsErrorKind::Io
    );
    assert_eq!(v2.read_at(&f2, 0, &mut small).unwrap(), 10);
}

#[test]
fn a_seeded_crash_plan_resolves_every_open_choice() {
    let w = world(13);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[1u8; 8192]);
    let f = open_rw(&v, &r, "f");
    v.write_at(&f, 0, &[2u8; 8192]).unwrap();
    let img = w.crash_image();
    let mut contents = BTreeMap::new();
    for seed in 0..32u64 {
        let mut cfg = SimConfig::new(seed);
        cfg.rates.torn = 1_000_000;
        let adv = moirai_vfs_sim::SeededAdversary::new(cfg.rates, cfg.release_law.clone());
        let after = img
            .reseeded(seed)
            .materialize_with(&CrashPlan::seeded(), Box::new(adv))
            .unwrap();
        let c = content_of(&after, "f").unwrap();
        // Every sub-sector holds a content it had; at most one sector is torn (G-5).
        let torn = c
            .chunks(4096)
            .filter(|s| {
                let v = subsector_values(s);
                v.iter().any(|&b| b != v[0])
            })
            .count();
        assert!(torn <= 1);
        contents.insert(c, seed);
    }
    assert!(contents.len() > 2);
}

fn store_root(v: &SimVfs) -> moirai_vfs_sim::SimRoot {
    v.open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
        .unwrap()
}

fn local_volume(v: &SimVfs, r: &moirai_vfs_sim::SimRoot) -> StoreVolume {
    match v.classify(r, moirai_vfs::ClassifyDepth::Open).unwrap() {
        moirai_vfs::Classification::Local(vol) => vol,
        moirai_vfs::Classification::Refused(x) => panic!("{x:?}"),
    }
}

/// Queues, for `v`'s process, no pause at its first five scheduling points and a pause of `ns` at the sixth: in a task
/// that opens a root, opens a file, writes once and flushes, the sixth point is inside the flush.
fn pause_inside_first_flush(w: &SimWorld, v: &SimVfs, ns: u64) {
    for _ in 0..5 {
        w.queue_choice_for(v, Site::Pause, 0);
    }
    w.queue_choice_for(v, Site::Pause, ns);
}

/// Loops on cheap calls until `proc` has started a flush.
fn wait_for_flush_of(w: &SimWorld, v: &SimVfs, proc: u32) {
    let root = store_root(v);
    while !w
        .trace()
        .iter()
        .any(|e| e.kind == EventKind::FlushStart && e.proc == proc)
    {
        let _ = v.list_dir(&root, None);
    }
}

/// F15 §2.2 write row and §5.2: a write of the bytes a sector holds (an extent's zero-fill) is a write. A failed flush
/// poisons every such sector; later appends make them dirty-over-poison, with no torn-sector bound at a crash, and they
/// stay poisoned after it (FM-3.1, FM-3.3, FM-3.5).
#[test]
fn fm01_a_rewrite_of_unchanged_bytes_is_a_write_that_a_failed_flush_poisons() {
    let w = world(14);
    let (v, r) = proc(&w, "p");
    let vol = local_volume(&v, &r);
    let f = v.create_extent(&r, rel("log"), 16 * 4096, &vol).unwrap();
    let node = f.node();
    w.queue_choice_for(&v, Site::FlushFault, 1);
    let failure = v.sync(&f, SyncKind::DataAndMeta).unwrap_err();
    let s = w.surface();
    let fs = s.file(node).unwrap();
    assert_eq!(fs.sectors.len(), 16);
    assert!(
        fs.sectors
            .iter()
            .all(|sv| sv.state == SectorKind::Poisoned && sv.candidates == 1)
    );
    assert!(fs.rewritten.is_empty());
    assert_eq!(fail(&v, failure), SimUnwind::Died);
    // A live writer appends two sectors over the poisoned zeros.
    let (v2, r2) = proc(&w, "q");
    let g = open_rw(&v2, &r2, "log");
    v2.write_at(&g, 0, &[0xAB; 8192]).unwrap();
    let fs = w.surface().file(node).unwrap().clone();
    assert_eq!(fs.sectors[0].state, SectorKind::DirtyOverPoison);
    // Two torn sectors in one file: accepted, because neither is plain `dirty` (FM-1.2 does not bound them).
    let torn = |s| {
        (
            s,
            SectorPick::Subsectors(if s == 0 {
                [0, 1, 0, 1, 0, 1, 0, 1]
            } else {
                [1, 0, 1, 0, 1, 0, 1, 0]
            }),
        )
    };
    let plan = CrashPlan::newest().with_file(
        node,
        FilePlan {
            sectors: [torn(0), torn(1)].into_iter().collect(),
            ..FilePlan::default()
        },
    );
    let after = w.crash_image().materialize(&plan).unwrap();
    let c = after.peek(&path("log")).unwrap();
    assert_eq!(
        subsector_values(&c[..4096]),
        vec![0, 0xAB, 0, 0xAB, 0, 0xAB, 0, 0xAB]
    );
    assert_eq!(
        subsector_values(&c[4096..8192]),
        vec![0xAB, 0, 0xAB, 0, 0xAB, 0, 0xAB, 0]
    );
    // Poisoning survives the crash, on the re-written sectors and on the others.
    let s = after.surface();
    let fs = s.file(node).unwrap();
    assert_eq!(fs.sectors.len(), 16);
    assert!(fs.sectors.iter().all(|sv| sv.state == SectorKind::Poisoned));
}

/// FM-2.2, G-2: a `sync(DataAndMeta)` that began first and returns last never moves the durable size back below the size
/// a later one made durable (review regression: two flushes overlapping in two processes).
#[test]
fn fm02_an_older_meta_flush_never_moves_the_durable_size_back() {
    let w = world(15);
    w.put_file(&path("f"), &[]).unwrap();
    let a = w.process_with("a", None, Some(true));
    let b = w.process_with("b", None, Some(true));
    let a_proc = a.process();
    pause_inside_first_flush(&w, &a, 1_000_000_000_000);
    let ta = w.spawn(&a, |v| {
        let root = store_root(&v);
        let f = v
            .open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap();
        v.write_at(&f, 0, &[1; 100]).unwrap();
        v.sync(&f, SyncKind::DataAndMeta).is_ok()
    });
    let wb = w.clone();
    let tb = w.spawn(&b, move |v| {
        let root = store_root(&v);
        let f = v
            .open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap();
        wait_for_flush_of(&wb, &v, a_proc);
        v.write_at(&f, 100, &[2; 100]).unwrap();
        v.sync(&f, SyncKind::DataAndMeta).is_ok()
    });
    assert!(!w.run().deadlock);
    assert!(ta.end().unwrap().unwrap() && tb.end().unwrap().unwrap());
    for plan in [CrashPlan::baseline(), CrashPlan::newest()] {
        let after = w.crash_image().materialize(&plan).unwrap();
        let c = content_of(&after, "f").unwrap();
        assert_eq!(c.len(), 200);
        assert!(c[..100].iter().all(|&x| x == 1) && c[100..].iter().all(|&x| x == 2));
    }
}

/// FM-2.6: a `sync_group` gives each member its class; its members on each volume form one group, in the order of first
/// appearance ([OS/fs §4.4.4]); a failed group is a failed flush of each file member and leaves each directory member's
/// operations pending, while an earlier group of the same call stays durable.
#[test]
fn fm02_sync_group_is_per_member_and_per_volume() {
    let mut cfg = SimConfig::new(16);
    cfg.volumes.push(("/vol2".into(), VolumeProfile::default()));
    let w = SimWorld::new(cfg);
    w.mkdir_all(Path::new(STORE));
    let (v, r) = proc(&w, "p");
    let other = v
        .open_root(Path::new("/vol2"), RootRole::Other, RootAccess::ReadWrite)
        .unwrap();
    let f = v.create_new(&r, rel("f")).unwrap();
    v.write_at(&f, 0, &[1; 4096]).unwrap();
    let g = v.create_new(&other, rel("g")).unwrap();
    v.write_at(&g, 0, &[2; 4096]).unwrap();
    let group = |fk, gk| {
        [
            GroupMember::File { file: &f, kind: fk },
            GroupMember::Dir {
                root: &r,
                dir: None,
            },
            GroupMember::File { file: &g, kind: gk },
            GroupMember::Dir {
                root: &other,
                dir: None,
            },
        ]
    };
    v.sync_group(&group(SyncKind::DataAndMeta, SyncKind::DataAndMeta))
        .unwrap();
    let s = w.surface();
    assert!(s.ops.is_empty() && s.files.is_empty());
    // The second volume's group fails.
    v.write_at(&f, 0, &[3; 4096]).unwrap();
    v.write_at(&g, 0, &[4; 4096]).unwrap();
    drop(v.create_new(&r, rel("f2")).unwrap());
    drop(v.create_new(&other, rel("g2")).unwrap());
    w.queue_choice_for(&v, Site::FlushFault, 0);
    w.queue_choice_for(&v, Site::FlushFault, 2);
    let e = v
        .sync_group(&group(SyncKind::Data, SyncKind::Data))
        .unwrap_err();
    assert_eq!(
        (e.kind, e.class),
        (VfsErrorKind::DiskFull, DurabilityClass::SyncGroup)
    );
    let s = w.surface();
    assert!(s.file(f.node()).is_none(), "the first group is durable");
    assert_eq!(
        s.file(g.node()).unwrap().sectors[0].state,
        SectorKind::Poisoned
    );
    let names: Vec<&str> = s.ops.iter().map(|o| o.names[0].as_str()).collect();
    assert_eq!(names, vec!["/vol2/g2"]);
    assert_eq!(fail(&v, e), SimUnwind::Died);
}

/// FM-3.1: a failed flush poisons every sector dirty at any instant of its interval, sectors another client wrote while
/// it ran included (review regression).
#[test]
fn fm03_a_failed_flush_poisons_what_another_client_wrote_while_it_ran() {
    let w = world(17);
    w.put_file(&path("f"), &[0u8; 8192]).unwrap();
    let a = w.process_with("a", None, Some(true));
    let b = w.process_with("b", None, Some(true));
    let a_proc = a.process();
    pause_inside_first_flush(&w, &a, 1_000_000_000_000);
    w.queue_choice_for(&a, Site::FlushFault, 1);
    let ta = w.spawn(&a, |v| {
        let root = store_root(&v);
        let f = v
            .open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap();
        v.write_at(&f, 0, &[1; 4096]).unwrap();
        if let Err(e) = v.sync(&f, SyncKind::Data) {
            v.fail_stop(e);
        }
    });
    let wb = w.clone();
    let tb = w.spawn(&b, move |v| {
        let root = store_root(&v);
        let f = v
            .open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap();
        wait_for_flush_of(&wb, &v, a_proc);
        v.write_at(&f, 4096, &[2; 4096]).unwrap();
        f.node()
    });
    assert!(!w.run().deadlock);
    assert!(matches!(
        ta.end().unwrap(),
        TaskEnd::Unwound(SimUnwind::Died)
    ));
    let node = tb.end().unwrap().unwrap();
    let s = w.surface();
    let states: Vec<(u64, SectorKind)> = s
        .file(node)
        .unwrap()
        .sectors
        .iter()
        .map(|sv| (sv.index, sv.state))
        .collect();
    assert_eq!(
        states,
        vec![(0, SectorKind::Poisoned), (1, SectorKind::Poisoned)]
    );
}

/// FM-3.1 (spec sync 2a): a sector that another process writes and makes durable with its own successful flush while a
/// failing flush is in flight was `dirty` at an instant of that interval: the failure poisons it, with K = {the content the
/// successful flush made durable}, so it is no longer bound by the one-torn-sector rule.
#[test]
fn fm03_a_failed_flush_poisons_what_a_concurrent_flush_cleaned_while_it_ran() {
    let w = world(19);
    w.put_file(&path("f"), &[0u8; 8192]).unwrap();
    let a = w.process_with("a", None, Some(true));
    let b = w.process_with("b", None, Some(true));
    let a_proc = a.process();
    pause_inside_first_flush(&w, &a, 1_000_000_000_000);
    w.queue_choice_for(&a, Site::FlushFault, 1);
    let ta = w.spawn(&a, |v| {
        let root = store_root(&v);
        let f = v
            .open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap();
        v.write_at(&f, 0, &[1; 4096]).unwrap();
        if let Err(e) = v.sync(&f, SyncKind::Data) {
            v.fail_stop(e);
        }
    });
    let wb = w.clone();
    let tb = w.spawn(&b, move |v| {
        let root = store_root(&v);
        let f = v
            .open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
            .unwrap();
        wait_for_flush_of(&wb, &v, a_proc);
        v.write_at(&f, 4096, &[2; 4096]).unwrap();
        v.sync(&f, SyncKind::Data).unwrap();
        f.node()
    });
    assert!(!w.run().deadlock);
    assert!(matches!(
        ta.end().unwrap(),
        TaskEnd::Unwound(SimUnwind::Died)
    ));
    let node = tb.end().unwrap().unwrap();
    let s = w.surface();
    let views: Vec<(u64, SectorKind, u64)> = s
        .file(node)
        .unwrap()
        .sectors
        .iter()
        .map(|sv| (sv.index, sv.state, sv.candidates))
        .collect();
    // Both sectors were clean when the failure returned: b's successful flush covered a's write to sector 0 (written
    // before b's flush began) and its own write to sector 1. K is taken at the return (FM-3.1): each holds the one
    // content b's flush made durable, which FM-2.1's guarantee for that flush keeps.
    assert_eq!(
        views,
        vec![(0, SectorKind::Poisoned, 1), (1, SectorKind::Poisoned, 1)]
    );
    let after = w.crash_image().materialize(&CrashPlan::seeded()).unwrap();
    let c = content_of(&after, "f").unwrap();
    assert!(c[..4096].iter().all(|&x| x == 1) && c[4096..].iter().all(|&x| x == 2));
}

/// FM-5.1: every step of `create_extent` — the exclusive create, each zero write, the sparse size change — and a plain
/// size change may fail with `DiskFull` (or `Io`, FM-5.5), a failed write or growth having applied any part (FM-5.2).
#[test]
fn fm05_disk_full_hits_every_step_of_extent_creation_and_size_changes() {
    let w = world(18);
    let (v, r) = proc(&w, "p");
    let vol = local_volume(&v, &r);
    w.queue_choice_for(&v, Site::CreateFault, 1);
    assert_eq!(
        v.create_extent(&r, rel("e1"), 1 << 20, &vol)
            .unwrap_err()
            .kind,
        VfsErrorKind::DiskFull
    );
    assert!(!w.exists(&path("e1")));
    w.queue_choice_for(&v, Site::WriteFault, 1);
    w.queue_choice_for(
        &v,
        Site::PartialWrite,
        PartialWrite::Prefix(4096).to_choice(),
    );
    assert_eq!(
        v.create_extent(&r, rel("e2"), 1 << 20, &vol)
            .unwrap_err()
            .kind,
        VfsErrorKind::DiskFull
    );
    assert_eq!(w.peek(&path("e2")).unwrap(), vec![0u8; 4096]);
    let sparse = StoreVolume {
        extent_method: ExtentMethod::Sparse,
        ..vol
    };
    w.queue_choice_for(&v, Site::WriteFault, 1);
    w.queue_choice_for(&v, Site::PartialWrite, PartialWrite::Nothing.to_choice());
    assert_eq!(
        v.create_extent(&r, rel("e3"), 1 << 20, &sparse)
            .unwrap_err()
            .kind,
        VfsErrorKind::DiskFull
    );
    assert_eq!(w.peek(&path("e3")).unwrap(), Vec::<u8>::new());
    let f = v.create_new(&r, rel("s")).unwrap();
    w.queue_choice_for(&v, Site::WriteFault, 2);
    assert_eq!(v.set_len(&f, 100).unwrap_err().kind, VfsErrorKind::Io);
    v.set_len(&f, 100).unwrap();
    // The extension's zeros are written data (F15 §5.2): a rewritten sector.
    assert_eq!(w.surface().file(f.node()).unwrap().rewritten, vec![(0, 1)]);
}

/// FM-7.2, FM-6.3: a suspend always advances the boot clock; the monotonic clock only when the adversary says so.
#[test]
fn fm07_a_suspend_may_or_may_not_advance_the_monotonic_clock() {
    let w = world(19);
    let (v, r) = proc(&w, "p");
    for counts in [true, false] {
        let (m0, b0) = (v.mono_ns(), v.boot_ns());
        w.queue_choice_for(&v, Site::Suspend, 5_000_000_000);
        w.queue_choice_for(&v, Site::SuspendMono, u64::from(counts));
        let _ = v.list_dir(&r, None);
        let (m1, b1) = (v.mono_ns(), v.boot_ns());
        assert!(b1 - b0 >= 5_000_000_000);
        assert_eq!(m1 - m0 >= 5_000_000_000, counts);
    }
}

/// FM-8.1, [F15 §3.8]: the seeded law draws all three release classes without a death plan, and the run report counts
/// them for the nightly obligation (a delay beyond every wait bound, a byte never released).
#[test]
fn fm08_the_seeded_law_draws_every_release_class() {
    let mut cfg = SimConfig::new(20);
    cfg.release_law = ReleaseDelayLaw::adversarial();
    let w = SimWorld::new(cfg);
    w.mkdir_all(Path::new(STORE));
    w.put_file(&path("LOCK"), &[0u8; 36 * 1024]).unwrap();
    for i in 0..100u16 {
        let (p, rp) = proc(&w, "holder");
        let mut c = p.lock_client(&rp, LockMode::Acquire).unwrap();
        let slot = LockByte::Slot(SlotIndex::new(i).unwrap());
        assert!(matches!(
            p.try_acquire(&mut c, slot).unwrap(),
            Acquired::Granted(_)
        ));
        w.kill(&p, DeathPlan::default());
    }
    let classes = w.release_classes();
    assert_eq!(classes.iter().sum::<u64>(), 100);
    assert!(classes.iter().all(|&n| n > 0), "{classes:?}");
    let never = w
        .trace()
        .iter()
        .filter(|e| e.kind == EventKind::LockZombie && e.c == u64::MAX)
        .count() as u64;
    assert_eq!(never, classes[2]);
    // The run report carries the counts.
    let t = w.process("t");
    let _ = w.spawn(&t, |v| v.mono_ns());
    assert_eq!(w.run().release_classes, classes);
    // A byte of class (b) is still held after every wait bound.
    let (q, rq) = proc(&w, "late");
    let c = q.lock_client(&rq, LockMode::Probe).unwrap();
    w.advance(ReleaseDelayLaw::adversarial().wait_bound_ns);
    let held = (0..100u16)
        .filter(|&i| q.probe(&c, LockByte::Slot(SlotIndex::new(i).unwrap())) == ProbeResult::Held)
        .count() as u64;
    assert!(held >= classes[1] + classes[2]);
}

/// [F15 §3.13]: a probe may answer `Unknown`, and never `Free` for a held byte.
#[test]
fn s3_13_a_probe_may_answer_unknown_and_never_free_for_a_held_byte() {
    let w = world(21);
    let (d, r) = proc(&w, "init");
    durable_file(&d, &r, "LOCK", &[0u8; 36 * 1024]);
    let (a, ra) = proc(&w, "a");
    let mut ca = a.lock_client(&ra, LockMode::Acquire).unwrap();
    let Acquired::Granted(g) = a.try_acquire(&mut ca, LockByte::Writer).unwrap() else {
        panic!("free");
    };
    let (b, rb) = proc(&w, "b");
    let cb = b.lock_client(&rb, LockMode::Probe).unwrap();
    w.queue_choice_for(&b, Site::ProbeUnknown, 1);
    assert_eq!(b.probe(&cb, LockByte::Writer), ProbeResult::Unknown);
    assert_eq!(b.probe(&cb, LockByte::Writer), ProbeResult::Held);
    a.release(&mut ca, g);
    assert_eq!(b.probe(&cb, LockByte::Writer), ProbeResult::Free);
}

/// [F15 §3.13]: a waiter may time out (`Busy`) although the byte was free for a moment — a spurious timeout — in both
/// wait modes. Caller-driven, the timed-out waiter cancelled its kernel wait and the byte is free for a try; with a
/// waiter thread, the abandoned waiter still waits in the kernel ([OS/lock §7.2]), so the table answers `Busy`.
#[test]
fn s3_13_a_waiter_may_time_out_although_the_byte_was_free_for_a_moment() {
    for mode in [WaitMode::CallerDriven, WaitMode::WaiterThread] {
        let mut cfg = SimConfig::new(22);
        cfg.wait_mode = Some(mode);
        cfg.rates = FaultRates {
            spurious_wake: 1_000_000,
            ..FaultRates::default()
        };
        let w = SimWorld::new(cfg);
        w.mkdir_all(Path::new(STORE));
        w.put_file(&path("LOCK"), &[0u8; 36 * 1024]).unwrap();
        let a = w.process_with("a", None, Some(true));
        let b = w.process_with("b", None, Some(true));
        let b_proc = b.process();
        let wa = w.clone();
        let ta = w.spawn(&a, move |v| {
            let root = store_root(&v);
            let mut c = v.lock_client(&root, LockMode::Acquire).unwrap();
            let Acquired::Granted(g) = v.try_acquire(&mut c, LockByte::Writer).unwrap() else {
                return false;
            };
            // Hold it until the other process waits in the kernel, then release.
            while !wa
                .trace()
                .iter()
                .any(|e| e.kind == EventKind::LockWait && e.proc == b_proc)
            {
                let _ = v.list_dir(&root, None);
            }
            v.release(&mut c, g);
            true
        });
        let wb = w.clone();
        let tb = w.spawn(&b, move |v| {
            let root = store_root(&v);
            let mut c = v.lock_client(&root, LockMode::Acquire).unwrap();
            while v.probe(&c, LockByte::Writer) != ProbeResult::Held {
                let _ = v.list_dir(&root, None);
            }
            let waited = v.acquire_within(&mut c, LockByte::Writer, 200).unwrap();
            let free_meanwhile = wb.trace().iter().any(|e| e.kind == EventKind::LockUnlock);
            let then = v.try_acquire(&mut c, LockByte::Writer).unwrap();
            let busy = waited == Acquired::Busy;
            if let Acquired::Granted(g) = then {
                v.release(&mut c, g);
                return (busy, free_meanwhile, true);
            }
            (busy, free_meanwhile, false)
        });
        assert!(!w.run().deadlock);
        assert!(ta.end().unwrap().unwrap(), "{mode:?}");
        let then_granted = mode == WaitMode::CallerDriven;
        assert_eq!(
            tb.end().unwrap().unwrap(),
            (true, true, then_granted),
            "{mode:?}"
        );
    }
}

/// [F15 §3.13], FM-3.1, FM-3.7: `Unsupported` from a flush is a failed flush; from a `sync_dir` it leaves the operations
/// pending.
#[test]
fn s3_13_an_unsupported_flush_is_a_failed_flush() {
    let w = world(23);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[1u8; 4096]);
    let f = open_rw(&v, &r, "f");
    v.write_at(&f, 0, &[2u8; 4096]).unwrap();
    w.queue_choice_for(&v, Site::FlushFault, 3);
    let e = v.sync(&f, SyncKind::Data).unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::Unsupported);
    assert_eq!(
        w.surface().file(f.node()).unwrap().sectors[0].state,
        SectorKind::Poisoned
    );
    let (v2, r2) = proc(&w, "q");
    drop(v2.create_new(&r2, rel("n")).unwrap());
    w.queue_choice_for(&v2, Site::SyncDirFault, 3);
    assert_eq!(
        v2.sync_dir(&r2, None).unwrap_err().kind,
        VfsErrorKind::Unsupported
    );
    assert!(w.surface().ops.iter().any(|o| o.kind == NsKind::Create));
    assert_eq!(fail(&v, e), SimUnwind::Died);
}

/// FM-11.2 with FM-3.1 (review regression): two tasks of one process are inside flushes of one file when the process
/// dies. The flush that started first resolves as succeeded and the other as failed. Every sector the success cleaned was
/// `dirty` at an instant of the failure's interval, so the failure poisons it too, whatever order the two resolve in: the
/// death path resolves the process's flushes one by one, each staying in flight (and absorbing what an earlier success
/// cleaned) until its own turn.
#[test]
fn fm11_two_flushes_of_one_dying_process_resolve_one_by_one() {
    let w = world(25);
    w.put_file(&path("f"), &[0u8; 8192]).unwrap();
    let node = {
        let (d, r) = proc(&w, "init");
        open_rw(&d, &r, "f").node()
    };
    let a = w.process_with("a", None, Some(true));
    let killer = w.process_with("killer", None, Some(true));
    let a_proc = a.process();
    // Each task opens the root, opens the file, writes one sector and flushes: its sixth point is inside the flush,
    // where it pauses long enough for the killer to act.
    let flusher = |sector: u64, byte: u8| {
        move |v: SimVfs| {
            let root = store_root(&v);
            let f = v
                .open(&root, rel("f"), Access::ReadWrite, OpenHint::Normal)
                .unwrap();
            v.write_at(&f, sector * 4096, &[byte; 4096]).unwrap();
            let _ = v.sync(&f, SyncKind::Data);
        }
    };
    let t1 = w.spawn(&a, flusher(0, 1));
    let t2 = w.spawn(&a, flusher(1, 2));
    for t in [t1.id(), t2.id()] {
        for _ in 0..5 {
            w.queue_choice_on(Some(&a), Site::Pause, u64::from(t), 0);
        }
        w.queue_choice_on(Some(&a), Site::Pause, u64::from(t), 1_000_000_000_000);
    }
    // The first flush to resolve succeeds, the second fails.
    w.queue_choice_for(&a, Site::FlushAtDeath, 0);
    w.queue_choice_for(&a, Site::FlushAtDeath, 1);
    let (wk, ak) = (w.clone(), a.clone());
    let tk = w.spawn(&killer, move |v| {
        let root = store_root(&v);
        let started = |w: &SimWorld| {
            w.trace()
                .iter()
                .filter(|e| e.kind == EventKind::FlushStart && e.proc == a_proc)
                .count()
        };
        while started(&wk) < 2 {
            let _ = v.list_dir(&root, None);
        }
        wk.kill(&ak, DeathPlan::default());
    });
    assert!(!w.run().deadlock);
    for t in [t1, t2] {
        assert!(matches!(
            t.end().unwrap(),
            TaskEnd::Unwound(SimUnwind::Died)
        ));
    }
    assert!(matches!(tk.end().unwrap(), TaskEnd::Returned(())));
    let outcomes: Vec<u64> = w
        .trace()
        .iter()
        .filter(|e| e.kind == EventKind::InFlight && e.proc == a_proc && e.b == 1)
        .map(|e| e.c)
        .collect();
    assert_eq!(
        outcomes,
        vec![0, 1],
        "both flushes were in flight at the death"
    );
    let s = w.surface();
    let states: Vec<(u64, SectorKind)> = s
        .file(node)
        .unwrap()
        .sectors
        .iter()
        .map(|sv| (sv.index, sv.state))
        .collect();
    assert_eq!(
        states,
        vec![(0, SectorKind::Poisoned), (1, SectorKind::Poisoned)]
    );
}

/// FM-11.2 with FM-2.6: a death inside `sync_group` resolves every member by the plan's outcome — succeeded (files clean,
/// names durable), failed (files poisoned, names pending) or not performed (files dirty, names pending).
#[test]
fn fm11_a_death_inside_sync_group_resolves_every_member() {
    for outcome in 0u8..3 {
        let w = world(24);
        let (d, r) = proc(&w, "init");
        durable_file(&d, &r, "y", &[0u8; 4096]);
        let (v, rv) = proc(&w, "flusher");
        let x = v.create_new(&rv, rel("x")).unwrap();
        v.write_at(&x, 0, &[1; 4096]).unwrap();
        let y = open_rw(&v, &rv, "y");
        v.write_at(&y, 0, &[2; 4096]).unwrap();
        w.kill_at(
            w.points() + 2,
            &v,
            DeathPlan {
                flush: Some(outcome),
                ..DeathPlan::default()
            },
        );
        let died = catch_death(|| {
            let _ = v.sync_group(&[
                GroupMember::File {
                    file: &x,
                    kind: SyncKind::DataAndMeta,
                },
                GroupMember::File {
                    file: &y,
                    kind: SyncKind::Data,
                },
                GroupMember::Dir {
                    root: &rv,
                    dir: None,
                },
            ]);
        });
        assert_eq!(died, Err(SimUnwind::Died));
        let s = w.surface();
        let state = |n: u64| s.file(n).and_then(|f| f.sectors.first()).map(|sv| sv.state);
        let (want, pending) = match outcome {
            0 => (None, false),
            1 => (Some(SectorKind::Poisoned), true),
            _ => (Some(SectorKind::Dirty), true),
        };
        assert_eq!(state(x.node()), want, "outcome {outcome}");
        assert_eq!(state(y.node()), want, "outcome {outcome}");
        assert_eq!(!s.ops.is_empty(), pending, "outcome {outcome}");
    }
}

/// S4 finding 3: the FM-3.2 query a checker uses before it judges two reads of one range. A failed flush poisons the
/// sector (FM-3.1) and the query says so, by path and by node, exactly where reads vary; a successful flush (FM-3.4), a
/// rename and a crash (FM-3.3) leave it so; a re-write of part of the sector ends it (FM-3.5); a sector a truncation
/// left beyond the file's size is not read, so it does not count.
#[test]
fn fm03_poisoned_below_says_where_reads_of_a_prefix_may_differ() {
    let w = world(31);
    let (v, r) = proc(&w, "p1");
    durable_file(&v, &r, "f", &[0x11; 2 * 4096]);
    let f = open_rw(&v, &r, "f");
    let node = f.node();
    let p = path("f");
    v.write_at(&f, 4096, &[0xAA; 4096]).unwrap();
    assert!(
        !w.poisoned_below(&p, u64::MAX),
        "a dirty sector reads its cache content"
    );
    w.queue_choice_for(&v, Site::FlushFault, 1);
    let failure = v.sync(&f, SyncKind::Data).unwrap_err();
    assert_eq!(fail(&v, failure), SimUnwind::Died);
    // Sector 0 was clean; sector 1 is poisoned.
    assert!(!w.poisoned_below(&p, 0));
    assert!(!w.poisoned_below(&p, 4096));
    assert!(w.poisoned_below(&p, 4097));
    assert!(w.poisoned_below(&p, u64::MAX));
    assert!(!w.poisoned_below_node(node, 4096));
    assert!(w.poisoned_below_node(node, 4097));
    assert!(!w.poisoned_below(Path::new(STORE), u64::MAX), "a directory");
    assert!(!w.poisoned_below(&path("none"), u64::MAX), "no file");
    assert!(!w.poisoned_below_node(u64::MAX, u64::MAX), "no node");
    // Exactly where reads vary: reads of sector 0 always agree, reads of sector 1 do not.
    let (v2, r2) = proc(&w, "p2");
    let f2 = open_rw(&v2, &r2, "f");
    let reads = |v: &SimVfs, f: &moirai_vfs_sim::SimFile, off: u64| -> BTreeSet<Vec<u8>> {
        (0..32)
            .map(|_| {
                let mut b = vec![0u8; 4096];
                v.read_exact_at(f, off, &mut b).unwrap();
                b
            })
            .collect()
    };
    assert_eq!(reads(&v2, &f2, 0).len(), 1);
    assert!(reads(&v2, &f2, 4096).len() > 1);
    // A successful flush by another process proves nothing (FM-3.4).
    v2.sync(&f2, SyncKind::DataAndMeta).unwrap();
    assert!(w.poisoned_below(&p, 4097));
    // A rename moves the name, not the poison; the node query follows the file.
    v2.rename_noreplace(&r2, rel("f"), &r2, rel("g"), ShareRetry::None)
        .unwrap();
    v2.sync_dir(&r2, None).unwrap();
    assert!(!w.poisoned_below(&p, u64::MAX));
    assert!(w.poisoned_below(&path("g"), 4097));
    assert!(w.poisoned_below_node(node, 4097));
    // A task may ask too.
    let (wt, pt) = (w.clone(), path("g"));
    let t = w.spawn(&v2, move |_| wt.poisoned_below(&pt, u64::MAX));
    assert!(!w.run().deadlock);
    assert!(t.end().unwrap().unwrap());

    // The poison survives a crash (FM-3.3).
    let after = w.crash_image().materialize(&CrashPlan::newest()).unwrap();
    assert!(after.poisoned_below(&path("g"), 4097));
    assert!(after.poisoned_below_node(node, 4097));
    // A write of four bytes ends it: the sector is dirty-over-poison and reads its cache content (FM-3.5).
    let (v3, r3) = proc(&after, "p3");
    let f3 = open_rw(&v3, &r3, "g");
    v3.write_at(&f3, 4096 + 600, b"diff").unwrap();
    assert!(!after.poisoned_below(&path("g"), u64::MAX));
    assert_eq!(
        after.surface().file(node).unwrap().sectors[0].state,
        SectorKind::DirtyOverPoison
    );
    assert_eq!(reads(&v3, &f3, 4096).len(), 1);
    // A truncation below it, then a failed flush: the sector is poisoned beyond cs(f), where no read reaches it.
    v3.set_len(&f3, 4096).unwrap();
    after.queue_choice_for(&v3, Site::FlushFault, 1);
    let failure = v3.sync(&f3, SyncKind::Data).unwrap_err();
    assert_eq!(fail(&v3, failure), SimUnwind::Died);
    let s = after.surface();
    let fs = s.file(node).unwrap();
    assert_eq!(
        (fs.sectors[0].index, fs.sectors[0].state),
        (1, SectorKind::Poisoned)
    );
    assert!(!after.poisoned_below(&path("g"), u64::MAX));
    assert!(!after.poisoned_below_node(node, u64::MAX));
}
