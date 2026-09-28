//! `EnvGuard` on the lane's NTFS volume ([OS/env]): classification at both depths, the full probe of `init`/`restore`,
//! the OS-version check and `doctor`'s warnings.

#![cfg(windows)]

mod common;

use common::TempDir;
use moirai_os::OsVfs;
use moirai_vfs::{
    Classification, ClassifyDepth, EnvGuard, EnvWarning, ExtentMethod, FsKind, OsVersion,
    ProbeOutcome, RelPath, RootRole, StoreFs, VfsErrorKind,
};

#[test]
fn the_lane_volume_is_admitted_as_ntfs() {
    let t = TempDir::new("classify");
    let v = OsVfs;
    let s = v.create_root(&t.join("store"), RootRole::Store).unwrap();
    for depth in [ClassifyDepth::Open, ClassifyDepth::Full] {
        match v.classify(&s, depth).unwrap() {
            Classification::Local(vol) => {
                assert_eq!(vol.fs, FsKind::Ntfs);
                assert_eq!(vol.extent_method, ExtentMethod::ZeroFill);
                assert!(!vol.read_only);
            }
            Classification::Refused(r) => panic!("refused at {depth:?}: {r:?} ({})", r.reason_id()),
        }
    }
}

#[test]
fn the_full_probe_admits_and_cleans_up() {
    let t = TempDir::new("probe");
    let v = OsVfs;
    let s = v.create_root(&t.join("store"), RootRole::Store).unwrap();
    v.create_dir(&s, RelPath::literal("tmp")).unwrap();
    // A probe file another prober or a crash left behind is neither a failure nor touched: it is a `probe.<nonce>` name
    // of [F02 §6.3]'s grammar, which the orphan sweep owns ([OS/env §5], pass 1, A1-40, P1-32, S1-38).
    let leftover = RelPath::literal("tmp/probe.1");
    drop(v.create_new(&s, leftover).unwrap());
    match v.probe_store(&s).unwrap() {
        ProbeOutcome::Admitted(report) => {
            assert_eq!(report.volume.fs, FsKind::Ntfs);
            assert!(
                report.os
                    >= OsVersion {
                        major: 10,
                        minor: 0,
                        build: 17_134
                    }
            );
        }
        ProbeOutcome::Refused(r) => panic!("refused: {r:?}"),
    }
    let names = |v: &OsVfs| -> Vec<String> {
        v.list_dir(&s, Some(RelPath::literal("tmp")))
            .unwrap()
            .iter()
            .map(|e| e.name.as_segment().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        names(&v),
        ["probe.1"],
        "the probe removed its own files and only those"
    );
    // Twice in a row: the probe leaves nothing that blocks the next one.
    assert!(matches!(
        v.probe_store(&s).unwrap(),
        ProbeOutcome::Admitted(_)
    ));
    assert_eq!(names(&v), ["probe.1"]);
}

#[test]
fn concurrent_probes_use_their_own_names() {
    // Two probes of one location at once (a `restore` probe beside an `init` retry, or two tests): each draws its own
    // three nonces, so neither deletes or renames onto the other's files, and both admit the location.
    let t = TempDir::new("probepair");
    let v = OsVfs;
    let s = v.create_root(&t.join("store"), RootRole::Store).unwrap();
    v.create_dir(&s, RelPath::literal("tmp")).unwrap();
    let runs: Vec<_> = (0..2)
        .map(|_| {
            let s = s.clone();
            std::thread::spawn(move || {
                (0..10)
                    .filter(|_| matches!(OsVfs.probe_store(&s).unwrap(), ProbeOutcome::Admitted(_)))
                    .count()
                    == 10
            })
        })
        .collect();
    for r in runs {
        assert!(r.join().unwrap());
    }
    assert!(
        v.list_dir(&s, Some(RelPath::literal("tmp")))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_probe_needs_tmp() {
    let t = TempDir::new("probetmp");
    let v = OsVfs;
    let s = v.create_root(&t.join("store"), RootRole::Store).unwrap();
    assert_eq!(v.probe_store(&s).unwrap_err().kind, VfsErrorKind::NotFound);
}

#[test]
fn os_version_and_doctor_warnings() {
    let t = TempDir::new("doctor");
    let v = OsVfs;
    let s = v.create_root(&t.join("store"), RootRole::Store).unwrap();
    let os = v.check_os_version().unwrap();
    let w = v.doctor_warnings(&s);
    println!("OBSERVED: {os} {w:?}");
    if os.build >= 22_000 {
        assert!(
            !w.iter()
                .any(|x| matches!(x, EnvWarning::UntestedOsRelease { .. }))
        );
    }
    assert!(
        !w.contains(&EnvWarning::NoBarrier)
            && !w.contains(&EnvWarning::WriteCacheForcedWriteThrough)
    );
}
