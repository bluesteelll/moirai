//! `swap_dirs` and `swap_recover` ([OS/fs §4.9]; [F15 §5.6]): the Windows two-rename form guarded by the intent file, a
//! crash after every step with the recovery table of [OS/fs §4.9.4], and the native exchange of Linux and macOS worlds.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::*;
use moirai_vfs::{
    OsTag, RootAccess, RootRole, ShareRetry, StoreFs, SwapOutcome, SwapRecovery, VfsErrorKind,
};
use moirai_vfs_sim::{
    CrashImage, CrashPlan, EventKind, NsKind, SimConfig, SimRoot, SimVfs, SimWorld, Site,
};

const A: &str = "/sim/a";
const B: &str = "/sim/b";

/// A world with `/sim/a` holding `mark = "A"` and `/sim/b` holding `mark = "B"`, all durable, and a process with the
/// parent `/sim` open.
fn setup(cfg: SimConfig) -> (SimWorld, SimVfs, SimRoot) {
    let w = SimWorld::new(cfg);
    w.mkdir_all(Path::new(A));
    w.mkdir_all(Path::new(B));
    w.put_file(&Path::new(A).join("mark"), b"A").unwrap();
    w.put_file(&Path::new(B).join("mark"), b"B").unwrap();
    let v = w.process_with("restore", None, Some(true));
    let parent = v
        .open_root(Path::new("/sim"), RootRole::Other, RootAccess::ReadWrite)
        .unwrap();
    (w, v, parent)
}

fn mark(w: &SimWorld, dir: &str) -> Option<Vec<u8>> {
    let p = Path::new(dir).join("mark");
    w.exists(&p).then(|| w.peek(&p).unwrap())
}

fn swap(v: &SimVfs, parent: &SimRoot) -> Result<SwapOutcome, moirai_vfs::VfsError> {
    v.swap_dirs(parent, rel("a"), parent, rel("b"), ShareRetry::None)
}

/// Whether the world is un-swapped (`Some(false)`), swapped (`Some(true)`) or neither.
fn swapped(w: &SimWorld) -> Option<bool> {
    match (mark(w, A).as_deref(), mark(w, B).as_deref()) {
        (Some(b"A"), Some(b"B")) => Some(false),
        (Some(b"B"), Some(b"A")) => Some(true),
        _ => None,
    }
}

#[test]
fn windows_swaps_by_renames_guarded_by_an_intent() {
    let (w, v, parent) = setup(SimConfig::new(60));
    let before = w.event_count();
    assert_eq!(swap(&v, &parent).unwrap(), SwapOutcome::TwoRenames);
    assert_eq!(swapped(&w), Some(true));
    assert!(!w.exists(Path::new("/sim/a.swap")) && !w.exists(Path::new("/sim/a.swap-old")));
    // The steps are calls of their own: the intent's create, three renames and the unlink, in that order.
    let ops: Vec<NsKind> = w.trace()[before as usize..]
        .iter()
        .filter(|e| e.kind == EventKind::NsOp)
        .map(|e| match e.b {
            0 => NsKind::Create,
            1 => NsKind::Remove,
            2 => NsKind::Rename,
            3 => NsKind::RenameReplace,
            _ => NsKind::Exchange,
        })
        .collect();
    assert_eq!(
        ops,
        vec![
            NsKind::Create,
            NsKind::Rename,
            NsKind::Rename,
            NsKind::Rename,
            NsKind::Remove
        ]
    );
    // Every step was made durable before the next: nothing is pending, and a crash keeps the swap.
    assert!(w.surface().ops.is_empty());
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert_eq!(swapped(&after), Some(true));
    assert_eq!(
        v.swap_recover(&parent, rel("a"), ShareRetry::None).unwrap(),
        SwapRecovery::NoIntent
    );
    // An intent or a temporary name left behind refuses a new swap ([OS/fs §4.9.2] step 1).
    w.mkdir_all(Path::new("/sim/a.swap-old"));
    assert_eq!(
        swap(&v, &parent).unwrap_err().kind,
        VfsErrorKind::AlreadyExists
    );
}

/// The images of a swap captured at every scheduling point of its run.
fn images(seed: u64) -> Vec<(u64, CrashImage)> {
    let (w, v, parent) = setup(SimConfig::new(seed));
    let start = w.points();
    swap(&v, &parent).unwrap();
    let end = w.points();
    let (w, v, parent) = setup(SimConfig::new(seed));
    assert_eq!(w.points(), start);
    for p in start + 1..=end {
        w.capture_at(p);
    }
    swap(&v, &parent).unwrap();
    w.take_captures()
}

#[test]
fn a_crash_at_every_step_leaves_a_prefix_that_recovery_completes_or_rolls_back() {
    let caps = images(61);
    assert!(caps.len() > 20, "every step has its own points");
    let mut results = BTreeSet::new();
    let mut store_missing = false;
    for (point, img) in &caps {
        for plan in [CrashPlan::baseline(), CrashPlan::newest()] {
            let w = img.materialize(&plan).unwrap();
            // Between steps 4 and 5 the name `a` is missing: a reader finds no store there ([F15 §5.6]).
            store_missing |= !w.exists(Path::new(A));
            let v = w.process_with("doctor", None, Some(true));
            let parent = v
                .open_root(Path::new("/sim"), RootRole::Other, RootAccess::ReadWrite)
                .unwrap();
            let done = swapped(&w);
            let r = match v.swap_recover(&parent, rel("a"), ShareRetry::None) {
                Ok(r) => r,
                // A crash between the intent's create and its flush may keep the name without its bytes (the
                // create survives, the data does not): [OS/fs §4.9.3] refuses such an intent, before any rename.
                Err(e) if e.call == "swap intent unreadable" => {
                    assert_eq!(done, Some(false), "point {point}");
                    assert!(w.exists(Path::new("/sim/a.swap")));
                    results.insert("Unreadable/false".to_owned());
                    continue;
                }
                Err(e) => panic!("point {point}: {e:?}"),
            };
            let done = swapped(&w).unwrap_or_else(|| panic!("point {point}: a mixed state"));
            match r {
                SwapRecovery::NoIntent | SwapRecovery::NothingDone | SwapRecovery::RolledBack => {
                    // A swap whose intent is durable but whose unlink was lost completes as NoIntent is impossible:
                    // NoIntent means the intent's create was lost, so nothing was renamed either.
                    assert!(!done || r == SwapRecovery::NoIntent, "point {point}: {r:?}");
                }
                SwapRecovery::Completed => assert!(done, "point {point}"),
            }
            assert!(!w.exists(Path::new("/sim/a.swap")));
            assert!(!w.exists(Path::new("/sim/a.swap-old")));
            results.insert(format!("{r:?}/{done}"));
        }
    }
    assert!(store_missing);
    // Every row of the recovery table is reached: no intent (before step 3, or after step 7), nothing renamed,
    // rolled back after step 4, completed after step 5 and after step 6.
    for want in [
        "NoIntent/false",
        "NoIntent/true",
        "NothingDone/false",
        "RolledBack/false",
        "Completed/true",
    ] {
        assert!(results.contains(want), "{want} missing from {results:?}");
    }
}

#[test]
fn recovery_distinguishes_the_states_after_steps_five_and_six() {
    // Crash exactly after step 5's rename (B → A) is durable and after step 6's (T → B) is durable.
    let caps = images(62);
    let mut rows = BTreeSet::new();
    for (_, img) in &caps {
        let w = img.materialize(&CrashPlan::newest()).unwrap();
        let (a, b, t) = (mark(&w, A), mark(&w, B), mark(&w, "/sim/a.swap-old"));
        let intent = w.exists(Path::new("/sim/a.swap"));
        if !intent {
            continue;
        }
        rows.insert((
            a.map(|m| m[0] as char),
            b.map(|m| m[0] as char),
            t.map(|m| m[0] as char),
        ));
    }
    // (A, B, T) as the recovery table lists them: nothing renamed, after 4, after 5, after 6.
    for want in [
        (Some('A'), Some('B'), None),
        (None, Some('B'), Some('A')),
        (Some('B'), None, Some('A')),
        (Some('B'), Some('A'), None),
    ] {
        assert!(rows.contains(&want), "{want:?} missing from {rows:?}");
    }
}

#[test]
fn a_failed_step_leaves_the_intent_for_recovery() {
    let (w, v, parent) = setup(SimConfig::new(63));
    // Step 4 (A → T) succeeds, step 5 (B → A) fails with disk-full: `a` is missing until recovery rolls back.
    w.queue_choice_for(&v, Site::NsFault, 0);
    w.queue_choice_for(&v, Site::NsFault, 1);
    assert_eq!(swap(&v, &parent).unwrap_err().kind, VfsErrorKind::DiskFull);
    assert!(!w.exists(Path::new(A)));
    assert!(w.exists(Path::new("/sim/a.swap")));
    assert!(w.violations().is_empty());
    assert_eq!(
        v.swap_recover(&parent, rel("a"), ShareRetry::None).unwrap(),
        SwapRecovery::RolledBack
    );
    assert_eq!(swapped(&w), Some(false));
    // A state the table does not list changes nothing and fails `Io`.
    let caps = images(64);
    let (_, img) = caps
        .iter()
        .find(|(_, img)| {
            let w = img.materialize(&CrashPlan::baseline()).unwrap();
            w.exists(Path::new("/sim/a.swap")) && swapped(&w) == Some(false)
        })
        .expect("a durable intent before any rename");
    let w = img.materialize(&CrashPlan::baseline()).unwrap();
    let v = w.process_with("doctor", None, Some(true));
    let parent = v
        .open_root(Path::new("/sim"), RootRole::Other, RootAccess::ReadWrite)
        .unwrap();
    v.rename_noreplace(&parent, rel("b"), &parent, rel("c"), ShareRetry::None)
        .unwrap();
    let e = v
        .swap_recover(&parent, rel("a"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(
        (e.kind, e.call),
        (VfsErrorKind::Io, "swap state unrecognised")
    );
    assert!(w.exists(Path::new("/sim/a.swap")));
}

#[test]
fn an_intent_whose_bytes_a_crash_lost_is_unreadable() {
    // The intent's name survives a crash that loses its unflushed content: recovery refuses ([OS/fs §4.9.3]).
    let caps = images(65);
    let mut seen = false;
    for (_, img) in &caps {
        let s = img.surface();
        let Some(create) = s
            .ops
            .iter()
            .find(|o| o.kind == NsKind::Create && o.names[0].ends_with("/a.swap"))
        else {
            continue;
        };
        let intent = s
            .files
            .iter()
            .find(|f| f.paths.iter().any(|p| p.ends_with("/a.swap")));
        if intent.is_some_and(|f| f.durable_size > 0) {
            continue;
        }
        let plan = CrashPlan::baseline().with_survivors([create.id]);
        let w = img.materialize(&plan).unwrap();
        assert!(w.exists(Path::new("/sim/a.swap")));
        let v = w.process_with("doctor", None, Some(true));
        let parent = v
            .open_root(Path::new("/sim"), RootRole::Other, RootAccess::ReadWrite)
            .unwrap();
        let e = v
            .swap_recover(&parent, rel("a"), ShareRetry::None)
            .unwrap_err();
        assert_eq!(
            (e.kind, e.call),
            (VfsErrorKind::Io, "swap intent unreadable")
        );
        assert_eq!(swapped(&w), Some(false));
        seen = true;
        break;
    }
    assert!(seen);
}

#[test]
fn linux_and_macos_exchange_natively_where_the_volume_can() {
    for os in [OsTag::Linux, OsTag::MacOs] {
        let mut cfg = SimConfig::new(66);
        cfg.os = os;
        let (w, v, parent) = setup(cfg);
        assert_eq!(swap(&v, &parent).unwrap(), SwapOutcome::Exchanged);
        assert_eq!(swapped(&w), Some(true));
        let s = w.surface();
        assert!(s.ops.is_empty(), "both parents synced");
        // A volume without the exchange takes the renames.
        w.queue_choice(Site::SwapExchange, 1);
        assert_eq!(swap(&v, &parent).unwrap(), SwapOutcome::TwoRenames);
        assert_eq!(swapped(&w), Some(false));
    }
    // Before its parents are synced, the native exchange is lost as a whole.
    let mut cfg = SimConfig::new(67);
    cfg.os = OsTag::Linux;
    let (w, v, parent) = setup(cfg);
    w.queue_choice(Site::SyncDirFault, 1);
    assert_eq!(swap(&v, &parent).unwrap_err().kind, VfsErrorKind::Io);
    assert_eq!(swapped(&w), Some(true));
    let after = w.crash_image().materialize(&CrashPlan::baseline()).unwrap();
    assert_eq!(swapped(&after), Some(false));
    // The failure was reported as the swap's error, not a durability failure: later calls are no violations.
    let f = v.create_new(&parent, rel("later")).unwrap();
    drop(f);
    assert!(w.violations().is_empty());
}
