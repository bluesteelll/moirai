//! Helpers shared by the simulator's integration tests.
#![allow(dead_code)]

use std::path::Path;

use moirai_vfs::{Access, OpenHint, RelPath, RootAccess, RootRole, StoreFs, SyncKind};
use moirai_vfs_sim::{SimConfig, SimFile, SimRoot, SimVfs, SimWorld};

pub const STORE: &str = "/sim/store";

/// Property-test cases for the tier named by `MOIRAI_TEST_TIER` (PLAN §2.1): `pr` (the default) runs `pr` cases,
/// `nightly` 16 times as many, `exit` 256 times as many.
pub fn cases(pr: u32) -> u32 {
    match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => pr * 16,
        Ok("exit") => pr * 256,
        _ => pr,
    }
}

pub fn rel(s: &'static str) -> RelPath<'static> {
    RelPath::literal(s)
}

/// A fault-free world with the store directory present (durably, before the scenario).
pub fn world(seed: u64) -> SimWorld {
    let w = SimWorld::new(SimConfig::new(seed));
    w.mkdir_all(Path::new(STORE));
    w
}

/// A process with a known boot identity and its read-write store root.
pub fn proc(w: &SimWorld, name: &str) -> (SimVfs, SimRoot) {
    let v = w.process_with(name, None, Some(true));
    let r = v
        .open_root(Path::new(STORE), RootRole::Store, RootAccess::ReadWrite)
        .expect("store root");
    (v, r)
}

/// Creates `name` with `data`, makes data, size and name durable, and closes it.
pub fn durable_file(v: &SimVfs, root: &SimRoot, name: &'static str, data: &[u8]) {
    let f = v.create_new(root, rel(name)).expect("create");
    v.write_at(&f, 0, data).expect("write");
    v.sync(&f, SyncKind::DataAndMeta).expect("sync");
    v.sync_dir(root, None).expect("sync_dir");
}

pub fn open_rw(v: &SimVfs, root: &SimRoot, name: &'static str) -> SimFile {
    v.open(root, rel(name), Access::ReadWrite, OpenHint::Normal)
        .expect("open")
}

pub fn read_all(v: &SimVfs, f: &SimFile) -> Vec<u8> {
    let n = v.file_size(f).expect("size") as usize;
    let mut buf = vec![0u8; n];
    v.read_exact_at(f, 0, &mut buf).expect("read");
    buf
}

/// The file's content as a fresh process of `w` reads it.
pub fn content_of(w: &SimWorld, name: &'static str) -> Option<Vec<u8>> {
    let (v, r) = proc(w, "reader");
    let f = v.open(&r, rel(name), Access::Read, OpenHint::Normal).ok()?;
    Some(read_all(&v, &f))
}

/// The value of every 512-byte sub-sector of a sector-aligned buffer whose sub-sectors are each uniform.
pub fn subsector_values(buf: &[u8]) -> Vec<u8> {
    buf.chunks(512)
        .map(|c| {
            assert!(c.iter().all(|&b| b == c[0]), "a sub-sector is uniform");
            c[0]
        })
        .collect()
}
