//! Seeded determinism ([F15 §6.4] "Determinism"; WP-31 acceptance: "a seed replays byte-identically") and the crash
//! enumeration interface the enumerator (WP-32) drives.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::*;
use moirai_vfs::ProcHost;
use moirai_vfs::{
    Access, Acquired, LockByte, LockMode, Locks, OpenHint, RootAccess, RootRole, StoreFs, SyncKind,
};
use moirai_vfs_sim::{
    CrashPlan, FaultRates, FilePlan, SectorPick, SimConfig, SimUnwind, SimVfs, SimWorld, TaskEnd,
    TraceMode,
};
use proptest::prelude::*;

/// Several processes, each with two tasks that append under the writer byte, flush, read and die on a durability
/// failure, under seeded faults and pauses; then a seeded crash and a recovery read.
fn scenario(seed: u64, mode: TraceMode) -> (Vec<u8>, u64, u64) {
    let mut cfg = SimConfig::new(seed);
    cfg.trace = mode;
    cfg.rates = FaultRates {
        pause: 50_000,
        write_fault: 10_000,
        flush_fault: 20_000,
        sharing: 10_000,
        read_fault: 5_000,
        probe_unknown: 10_000,
        spurious_wake: 50_000,
        torn: 500_000,
        ..FaultRates::default()
    };
    let w = SimWorld::new(cfg);
    w.mkdir_all(Path::new(STORE));
    let store = Path::new(STORE);
    w.put_file(&store.join("LOCK"), &[0u8; 36 * 1024]).unwrap();
    w.put_file(&store.join("log"), &[0u8; 16 * 1024]).unwrap();
    for i in 0..3u64 {
        let p = w.process(&format!("p{i}"));
        for t in 0..2u64 {
            w.spawn(&p, move |v: SimVfs| worker(&v, i * 2 + t));
        }
    }
    let report = w.run();
    assert!(!report.deadlock);
    w.crash(&CrashPlan::seeded()).unwrap();
    let (v, r) = proc(&w, "recover");
    if let Ok(f) = v.open(&r, rel("log"), Access::Read, OpenHint::Normal) {
        let mut buf = vec![0u8; 16 * 1024];
        let _ = v.read_at(&f, 0, &mut buf);
        v.note(
            1,
            u64::from(buf.iter().map(|&b| u32::from(b)).sum::<u32>()),
            0,
        );
    }
    (w.trace_bytes(), w.trace_digest(), w.event_count())
}

fn worker(v: &SimVfs, id: u64) {
    let Ok(root) = v.open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite) else {
        return;
    };
    let Ok(mut c) = v.lock_client(&root, LockMode::Acquire) else {
        return;
    };
    let Ok(f) = v.open(&root, rel("log"), Access::ReadWrite, OpenHint::Normal) else {
        return;
    };
    for k in 0..4u64 {
        if let Ok(Acquired::Granted(g)) = v.acquire_within(&mut c, LockByte::Writer, 50) {
            let off = (id * 4 + k) * 512;
            let _ = v.write_at(&f, off, &[(id * 16 + k) as u8; 512]);
            v.note(2, id, k);
            v.release(&mut c, g);
        }
        if let Err(e) = v.sync(&f, SyncKind::Data) {
            v.fail_stop(e);
        }
        let mut buf = [0u8; 64];
        let _ = v.read_at(&f, 0, &mut buf);
    }
}

#[test]
fn a_seed_replays_byte_identically() {
    let (a, da, na) = scenario(42, TraceMode::Full);
    let (b, db, nb) = scenario(42, TraceMode::Full);
    assert!(na > 100);
    assert_eq!(na, nb);
    assert_eq!(a, b);
    assert_eq!(da, db);
    let (_, dc, _) = scenario(43, TraceMode::Full);
    assert_ne!(da, dc, "another seed takes another path");
    // The digest-only mode proves the same replay with constant trace memory.
    let (bytes, dd, nd) = scenario(42, TraceMode::DigestOnly);
    assert!(bytes.is_empty());
    assert_eq!((dd, nd), (da, na));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(6), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn every_seed_replays(seed in any::<u64>()) {
        let (a, da, _) = scenario(seed, TraceMode::DigestOnly);
        let (b, db, _) = scenario(seed, TraceMode::DigestOnly);
        prop_assert_eq!(a, b);
        prop_assert_eq!(da, db);
    }

    /// A clean sector never changes at a crash, and every sector holds a content it held (G-4, G-5), whatever plan the
    /// enumerator picks.
    #[test]
    fn any_plan_keeps_clean_sectors_and_real_contents(
        writes in proptest::collection::vec((0u64..6, 1u8..=255), 1..12),
        picks in proptest::collection::vec(0u64..16, 6),
        torn in proptest::option::of(0usize..6),
    ) {
        let w = world(7);
        let (v, r) = proc(&w, "p");
        durable_file(&v, &r, "f", &[0u8; 6 * 4096]);
        let f = open_rw(&v, &r, "f");
        let mut written: Vec<BTreeSet<u8>> = vec![BTreeSet::from([0u8]); 6];
        for &(s, b) in &writes {
            v.write_at(&f, s * 4096, &[b; 4096]).unwrap();
            written[s as usize].insert(b);
        }
        let img = w.crash_image();
        let surface = img.surface();
        let fs = surface.file(f.node()).unwrap();
        let mut plan = FilePlan::default();
        let dirty: Vec<_> = fs.dirty().copied().collect();
        for (i, sv) in dirty.iter().enumerate() {
            let pick = if torn == Some(i) {
                let mut subs = [0u64; 8];
                for (j, s) in subs.iter_mut().enumerate() {
                    *s = (picks[i] + j as u64) % sv.candidates;
                }
                SectorPick::Subsectors(subs)
            } else {
                SectorPick::Version(picks[i] % sv.candidates)
            };
            plan.sectors.insert(sv.index, pick);
        }
        let after = img.materialize(&CrashPlan::baseline().with_file(f.node(), plan)).unwrap();
        let c = content_of(&after, "f").unwrap();
        prop_assert_eq!(c.len(), 6 * 4096);
        for (s, sector) in c.chunks(4096).enumerate() {
            for sub in subsector_values(sector) {
                prop_assert!(written[s].contains(&sub));
            }
            if !dirty.iter().any(|d| d.index == s as u64) {
                prop_assert!(sector.iter().all(|&b| b == 0));
            }
        }
    }
}

#[test]
fn the_enumerator_can_reach_every_subset_of_twelve_dirty_sectors() {
    let w = world(8);
    let (v, r) = proc(&w, "p");
    durable_file(&v, &r, "f", &[0u8; 12 * 4096]);
    let f = open_rw(&v, &r, "f");
    for s in 0..12u64 {
        v.write_at(&f, s * 4096, &[1u8; 4096]).unwrap();
    }
    let img = w.crash_image();
    let surface = img.surface();
    let fs = surface.file(f.node()).unwrap();
    assert_eq!(fs.dirty().count(), 12);
    // Every subset while there are ≤ 12 dirty sectors ([F15 §6.4]): 4,096 plans, 4,096 distinct states, each
    // materialised and read back as a sector bitmap. The `pr` tier checks every 97th mask; `nightly` and `exit` all.
    let step = if cases(1) == 1 { 97 } else { 1 };
    let mut seen = BTreeSet::new();
    for mask in (0u32..4096).step_by(step) {
        let mut plan = FilePlan::default();
        for (i, sv) in fs.dirty().enumerate() {
            let newest = mask & (1 << i) != 0;
            plan.sectors.insert(
                sv.index,
                SectorPick::Version(if newest { sv.candidates - 1 } else { 0 }),
            );
        }
        let after = img
            .materialize(&CrashPlan::baseline().with_file(f.node(), plan))
            .unwrap();
        let c = content_of(&after, "f").unwrap();
        let got: u32 = c
            .chunks(4096)
            .enumerate()
            .map(|(i, s)| u32::from(s[0]) << i)
            .sum();
        assert_eq!(got, mask);
        seen.insert(got);
    }
    assert_eq!(seen.len(), 4096usize.div_ceil(step));
}

#[test]
fn a_crash_at_a_scheduling_point_unwinds_every_task_and_boots_again() {
    let w = world(50);
    let (d, r) = proc(&w, "init");
    durable_file(&d, &r, "log", &[0u8; 8192]);
    let p = w.process_with("p", None, Some(true));
    let start = w.points();
    w.capture_at(start + 6);
    w.crash_at(start + 12, CrashPlan::seeded());
    let body = |v: SimVfs| {
        let root = v
            .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
            .unwrap();
        let f = v
            .open(&root, rel("log"), Access::ReadWrite, OpenHint::Normal)
            .unwrap();
        for i in 0..100u64 {
            v.write_at(&f, (i % 16) * 512, &[i as u8; 512]).unwrap();
        }
    };
    let t1 = w.spawn(&p, body);
    let t2 = w.spawn(&p, body);
    assert!(!w.run().deadlock);
    for t in [t1, t2] {
        assert!(matches!(
            t.end().unwrap(),
            TaskEnd::Unwound(SimUnwind::Died)
        ));
    }
    assert_eq!(w.boot().0, 2);
    let caps = w.take_captures();
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0].0, start + 6);
    assert_eq!(caps[0].1.point(), start + 6);
    let after = caps[0].1.materialize(&CrashPlan::newest()).unwrap();
    assert_eq!(after.boot().0, 2);
    assert!(content_of(&after, "log").is_some());
}

#[test]
fn tasks_that_can_never_wake_are_aborted_as_a_deadlock() {
    let w = world(51);
    let parent = w.process_with("parent", None, Some(true));
    let child = w.process_with("child", Some(&parent), Some(true));
    let t = w.spawn(&child, |v| {
        let watch = v.watch_parent().unwrap();
        let wake = v.new_wake().unwrap();
        v.wait_parent_or_wake(&watch, &wake).unwrap()
    });
    assert!(w.run().deadlock);
    assert!(matches!(
        t.end().unwrap(),
        TaskEnd::Unwound(SimUnwind::Deadlock)
    ));
}

/// Every kind of call, every fault at adversarial rates, pauses, suspends and system crashes: no task may end in a
/// panic of the simulator itself, and the run replays.
fn chaos(seed: u64) -> (u64, Vec<String>, u64, u64) {
    let mut cfg = SimConfig::new(seed);
    cfg.trace = TraceMode::DigestOnly;
    cfg.rates = FaultRates {
        system_crash: 2_000,
        ..FaultRates::adversarial()
    };
    let w = SimWorld::new(cfg);
    w.mkdir_all(Path::new(STORE));
    let store = Path::new(STORE);
    w.put_file(&store.join("LOCK"), &[0u8; 36 * 1024]).unwrap();
    w.put_file(&store.join("log"), &[0u8; 8192]).unwrap();
    let mut tasks = Vec::new();
    for i in 0..3u64 {
        let p = w.process(&format!("p{i}"));
        for t in 0..2u64 {
            tasks.push(w.spawn(&p, move |v: SimVfs| chaos_worker(&v, i * 2 + t)));
        }
    }
    assert!(!w.run().deadlock);
    let mut panics = Vec::new();
    let mut returned = 0;
    for t in tasks {
        match t.end() {
            Some(moirai_vfs_sim::TaskEnd::Panicked(m)) => panics.push(m),
            Some(moirai_vfs_sim::TaskEnd::Returned(())) => returned += 1,
            _ => {}
        }
    }
    let points = w.points();
    w.crash(&CrashPlan::seeded()).unwrap();
    // The recovery reads run on the driver thread, where a drawn system crash unwinds too.
    let _ = moirai_vfs_sim::catch_death(|| recover_reads(&w));
    (w.trace_digest(), panics, returned, points)
}

fn recover_reads(w: &SimWorld) {
    let (v, r) = proc(w, "recover");
    if let Ok(entries) = v.list_dir(&r, None) {
        for e in entries {
            let Some(name) = e.name.as_segment() else {
                continue;
            };
            let Ok(p) = moirai_vfs::RelPath::new(name) else {
                continue;
            };
            if let Ok(f) = v.open(&r, p, Access::Read, OpenHint::Normal) {
                let mut buf = [0u8; 256];
                let _ = v.read_at(&f, 0, &mut buf);
            }
        }
    }
}

fn chaos_worker(v: &SimVfs, id: u64) {
    use moirai_vfs::{Clock, SealedMap, SealedMaps, ShareRetry};
    const NAMES: [&str; 4] = ["a", "b", "c", "d"];
    let Ok(root) = v.open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite) else {
        return;
    };
    let mut client = v.lock_client(&root, LockMode::Acquire).ok();
    for k in 0..8u64 {
        let name = moirai_vfs::RelPath::new(NAMES[((id + k) % 4) as usize]).unwrap();
        let other = moirai_vfs::RelPath::new(NAMES[((id + k + 1) % 4) as usize]).unwrap();
        let _ = v.wall_ms();
        let _ = v.boot_identity();
        match (id * 3 + k) % 8 {
            0 => {
                if let Ok(f) = v.create_new(&root, name) {
                    let _ = v.write_at(&f, 0, &[id as u8; 700]);
                    if let Err(e) = v.sync(&f, SyncKind::DataAndMeta) {
                        v.fail_stop(e);
                    }
                }
            }
            1 => {
                if let Ok(f) = v.open(&root, name, Access::ReadWrite, OpenHint::Normal) {
                    let _ = v.write_at(&f, k * 300, &[k as u8; 900]);
                    let mut buf = [0u8; 1024];
                    let _ = v.read_at(&f, 0, &mut buf);
                }
            }
            2 => {
                let _ = if k % 2 == 0 {
                    v.rename_noreplace(&root, name, &root, other, ShareRetry::None)
                } else {
                    v.rename_replace(&root, name, &root, other, ShareRetry::None)
                };
            }
            3 => {
                let _ = v.unlink(&root, name, ShareRetry::None);
            }
            4 => {
                if let Err(e) = v.sync_dir(&root, None) {
                    v.fail_stop(e);
                }
            }
            5 => {
                if let Some(c) = client.as_mut()
                    && let Ok(Acquired::Granted(g)) = v.acquire_within(c, LockByte::Writer, 20)
                {
                    if let Ok(f) = v.open(&root, rel("log"), Access::ReadWrite, OpenHint::Normal) {
                        let _ = v.write_at(&f, id * 512, &[1u8; 512]);
                        if let Err(e) = v.sync(&f, SyncKind::Data) {
                            v.fail_stop(e);
                        }
                    }
                    v.release(c, g);
                }
            }
            6 => {
                if let Ok(f) = v.open(&root, name, Access::ReadWrite, OpenHint::Normal) {
                    let _ = v.seal(&f);
                    if let Ok(len) = v.file_size(&f)
                        && len > 0
                        && let Ok(m) = v.map_sealed(&f, len, name)
                    {
                        let _ = m.bytes().len();
                    }
                }
            }
            _ => {
                let _ = v.create_dir(&root, name);
                let _ = v.list_dir(&root, None);
            }
        }
    }
}

#[test]
fn chaos_runs_end_without_simulator_panics_and_replay() {
    let (mut returned, mut points) = (0, 0);
    for seed in 0..cases(24) as u64 {
        let (d1, panics, r, p) = chaos(seed);
        assert!(panics.is_empty(), "seed {seed}: {panics:?}");
        returned += r;
        points += p;
        if seed < 4 {
            assert_eq!(chaos(seed).0, d1, "seed {seed} replays");
        }
    }
    // The runs do work: most tasks reach their end, through hundreds of scheduling points each.
    assert!(
        returned > 0 && points > 100 * u64::from(cases(24)),
        "{returned} {points}"
    );
}
