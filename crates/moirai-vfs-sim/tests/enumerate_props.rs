//! Property tests of the crash enumerator's plan generator (WP-32, review pass 1): for crash surfaces the simulator
//! reaches — random writes, extensions, flushes (some failed, so poisoned sectors exist), namespace operations, and
//! writes caught in flight — every plan it generates is one the simulator accepts ([`CrashImage::materialize`]
//! validates it: sizes in H(f), only non-clean sectors named, candidates in range, at most one torn dirty sector per
//! file (FM-1.2), known pending operations and writes in flight), and no plan comes twice.

mod common;

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use common::*;
use moirai_vfs::{ShareRetry, StoreFs, SyncKind};
use moirai_vfs_sim::enumerate::{PlanLimits, PlanMode, crash_plans};
use moirai_vfs_sim::{CallKind, CrashImage, SectorKind, SectorPick, Site};
use proptest::prelude::*;

/// One step of a scenario on the driver thread.
#[derive(Clone, Debug)]
enum Step {
    /// Writes `len` 1 KiB blocks of `byte` into file `file` from 1 KiB block `at` (extending it past its end).
    Write { file: u8, at: u8, len: u8, byte: u8 },
    /// Flushes file `file` (with its size if `meta`); `fail` makes the flush fail (FM-3.1).
    Sync { file: u8, meta: bool, fail: bool },
    /// Creates name `n`.
    Create { n: u8 },
    /// Renames name `a` to name `b` (no replace).
    Rename { a: u8, b: u8 },
    /// Unlinks name `n`.
    Unlink { n: u8 },
    /// Flushes the store directory.
    SyncDir,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0u8..3, 0u8..20, 1u8..9, 1u8..=255).prop_map(|(file, at, len, byte)| Step::Write { file, at, len, byte }),
        2 => (0u8..3, any::<bool>(), proptest::bool::weighted(0.3))
            .prop_map(|(file, meta, fail)| Step::Sync { file, meta, fail }),
        1 => (0u8..3).prop_map(|n| Step::Create { n }),
        1 => (0u8..3, 0u8..3).prop_map(|(a, b)| Step::Rename { a, b }),
        1 => (0u8..3).prop_map(|n| Step::Unlink { n }),
        1 => Just(Step::SyncDir),
    ]
}

const FILES: [&str; 3] = ["f0", "f1", "f2"];
const NAMES: [&str; 3] = ["n0", "n1", "n2"];

/// Runs `steps` in a world with three durable files, capturing an image at every point of a call that changes storage
/// (inside writes too); returns the images and the node of `f0` (the slot file of the plans).
fn images(seed: u64, steps: &[Step]) -> (Vec<CrashImage>, Option<u64>) {
    let w = world(seed);
    for (i, f) in FILES.iter().enumerate() {
        w.put_file(
            &Path::new(STORE).join(f),
            &vec![0x10 + i as u8; 4096 * (i + 1)],
        )
        .expect("file");
    }
    let calls: Vec<CallKind> = CallKind::ALL
        .into_iter()
        .filter(|c| c.changes_storage())
        .collect();
    w.capture_calls(0, &calls, 4096);
    let mut n = 0;
    let (mut v, mut r) = proc(&w, "p0");
    for s in steps {
        match *s {
            Step::Write {
                file,
                at,
                len,
                byte,
            } => {
                let f = open_rw(&v, &r, FILES[file as usize]);
                let _ = v.write_at(
                    &f,
                    u64::from(at) * 1024,
                    &vec![byte; 1024 * usize::from(len)],
                );
            }
            Step::Sync { file, meta, fail } => {
                let f = open_rw(&v, &r, FILES[file as usize]);
                if fail {
                    w.queue_choice(Site::FlushFault, 1);
                }
                let kind = if meta {
                    SyncKind::DataAndMeta
                } else {
                    SyncKind::Data
                };
                if v.sync(&f, kind).is_err() {
                    // A durability failure ends the process's writing (fail-stop); another process goes on.
                    n += 1;
                    (v, r) = proc(&w, &format!("p{n}"));
                }
            }
            Step::Create { n } => {
                let _ = v.create_new(&r, rel(NAMES[n as usize]));
            }
            Step::Rename { a, b } => {
                let _ = v.rename_noreplace(
                    &r,
                    rel(NAMES[a as usize]),
                    &r,
                    rel(NAMES[b as usize]),
                    ShareRetry::None,
                );
            }
            Step::Unlink { n } => {
                let _ = v.unlink(&r, rel(NAMES[n as usize]), ShareRetry::None);
            }
            Step::SyncDir => {
                let _ = v.sync_dir(&r, None);
            }
        }
    }
    let mut imgs: Vec<CrashImage> = w.take_captures().into_iter().map(|(_, i)| i).collect();
    imgs.push(w.crash_image());
    let slot = w.node_at(&Path::new(STORE).join(FILES[0]));
    (imgs, slot)
}

const LIMITS: PlanLimits = PlanLimits {
    exhaustive_max: 4,
    random_min: 16,
    random_per_point: 4,
    cross_budget: 16,
};

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(12), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn every_generated_plan_is_valid_and_new(
        seed in any::<u64>(),
        steps in proptest::collection::vec(step(), 1..14),
        slot in any::<bool>(),
        pick in any::<u64>(),
    ) {
        let (imgs, f0) = images(seed, &steps);
        let slots: BTreeSet<u64> = f0.filter(|_| slot).into_iter().collect();
        // At most five images per case: the last (the end), and up to four others picked at random.
        let n = imgs.len();
        let chosen: BTreeSet<usize> = (0..4).map(|k| ((pick >> (k * 8)) as usize) % n).chain([n - 1]).collect();
        for &i in &chosen {
            let img = &imgs[i];
            let surface = img.surface();
            for mode in [PlanMode::Prefix, PlanMode::Full] {
                let plans = crash_plans(&surface, &slots, mode, &LIMITS, seed ^ i as u64);
                let distinct: HashSet<_> = plans.iter().map(|(_, p)| p.clone()).collect();
                prop_assert_eq!(distinct.len(), plans.len(), "a plan came twice");
                for (dim, plan) in &plans {
                    // At most one torn dirty sector per file (FM-1.2), checked here against the surface as well.
                    for (node, fp) in &plan.files {
                        let f = surface.file(*node);
                        prop_assert!(f.is_some(), "{dim:?}: a plan names node {node}, which has nothing to decide");
                        let f = f.expect("checked");
                        let torn = fp
                            .sectors
                            .iter()
                            .filter(|(s, p)| {
                                matches!(p, SectorPick::Subsectors(_))
                                    && f.sectors.iter().any(|v| v.index == **s && v.state == SectorKind::Dirty)
                            })
                            .count();
                        prop_assert!(torn <= 1, "{dim:?}: {torn} torn dirty sectors in node {node}");
                    }
                    if let Err(e) = img.materialize(plan) {
                        return Err(TestCaseError::fail(format!("{dim:?} {plan:?}: {e}")));
                    }
                }
            }
        }
    }
}

/// The scenario generator reaches poisoned sectors and writes in flight (so the property above covers FM-3.3 mixes and
/// §2.5 partial writes, not just dirty sectors).
#[test]
fn the_scenarios_reach_poison_and_writes_in_flight() {
    let steps = [
        Step::Write {
            file: 0,
            at: 0,
            len: 8,
            byte: 1,
        },
        Step::Sync {
            file: 0,
            meta: false,
            fail: true,
        },
        Step::Write {
            file: 0,
            at: 2,
            len: 2,
            byte: 2,
        },
        Step::Create { n: 1 },
    ];
    let (imgs, _) = images(3, &steps);
    let surfaces: Vec<_> = imgs.iter().map(CrashImage::surface).collect();
    assert!(
        surfaces.iter().any(|s| !s.writes.is_empty()),
        "a write in flight"
    );
    assert!(
        surfaces.iter().any(|s| s
            .files
            .iter()
            .any(|f| f.sectors.iter().any(|v| v.state != SectorKind::Dirty))),
        "a poisoned sector"
    );
    assert!(
        surfaces.iter().any(|s| !s.ops.is_empty()),
        "a pending create"
    );
}
