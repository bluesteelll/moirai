//! What an open reads from a store directory, through the `Vfs` seam: the tail's log extents ([`VfsLog`]), and the
//! open floor of [60 §5.3] (`60-roadmap.md:790`: "`HEAD` pread + map of 8 files"); and the fixture a measurement-10
//! run writes and removes.
//!
//! - **`HEAD`** is 8,192 bytes, read with one `read_at` ([F04 §2]). The floor does not check the
//!   slots' checksums.
//! - **The sealed files** are the 8 entries of a full segment set ([F04 §4.1]: one base, up to five
//!   deltas, a yield delta and a dictionary), each 99·N/8 bytes at production ([AR §8.1]: 99 B
//!   of hot index per node). Before mapping one, the floor reads its 120-byte `SegHdr` with one `read_exact_at` and
//!   checks its magic and `hdr_xxh3` ([OS/map §4]; [F09 §2.1]), then maps it
//!   with the header's `total_len` ([AR §4.7]). The `SegRef` identity check, a 16-byte compare
//!   ([F04 §4.1]), is not modelled.
//! - **The log extents** `log.<n>` are E bytes each, the tail's bytes at their offsets and zero elsewhere
//!   ([F05 §2.2], [F05 §2.4]); the tail starts at extent 2 and `log.1` is not written (cut addendum m-6).
//! - **One command's files** (u20, M-A6), for `a1-opens10.n1e5` alone: `LOCK` and the spare extent after the tail's
//!   last one ([`write_command_files`]).
//! - **B-1's fixture** (u20): `pages`, seeded bytes, and its 64 source pages `pages.src` ([`write_pages_fixture`]).

use super::scan::{Expect, LogSource};
use super::synth::Tail;
use super::{Geometry, HEAD_LEN};
use moirai_vfs::{
    Access, OS_SHARE_RETRY_MS, OpenHint, RelPath, RelPathBuf, RootAccess, RootRole, SealedMaps,
    ShareRetry, StoreFs, SyncKind,
};
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

/// The sealed files of a full segment set ([F04 §4.1]).
pub const SEALED_FILES: usize = 8;
/// `SegHdr`'s length ([F09 §2.1]).
pub const SEG_HDR_LEN: usize = 120;
/// `SegHdr.magic` ([F09 §2.1]).
pub const SEG_MAGIC: [u8; 4] = *b"MSEG";
/// Hot-index bytes per node ([AR §8.1]).
pub const HOT_BYTES_PER_NODE: u64 = 99;

/// The sealed files' names: `seg.base.1` and `seg.d1` … `seg.d7` ([F02 §6] name families).
pub fn sealed_names() -> Vec<String> {
    std::iter::once("seg.base.1".to_string())
        .chain((1..SEALED_FILES).map(|k| format!("seg.d{k}")))
        .collect()
}

/// A `SegHdr` with `magic`, `format` 1, `seg_kind`, `file_no`, `total_len`, `table_xxh3` over the empty table and
/// `hdr_xxh3` over `[0, 112)`; every other field zero.
pub fn seg_header(seg_kind: u8, file_no: u32, total_len: u64) -> [u8; SEG_HDR_LEN] {
    let mut h = [0u8; SEG_HDR_LEN];
    h[0..4].copy_from_slice(&SEG_MAGIC);
    h[4..6].copy_from_slice(&1u16.to_le_bytes());
    h[6] = seg_kind;
    h[16..20].copy_from_slice(&file_no.to_le_bytes());
    h[64..72].copy_from_slice(&total_len.to_le_bytes());
    h[104..112].copy_from_slice(&xxh3_64(&[]).to_le_bytes());
    let sum = xxh3_64(&h[..112]);
    h[112..120].copy_from_slice(&sum.to_le_bytes());
    h
}

/// Checks a `SegHdr`'s magic and `hdr_xxh3` and returns its `total_len`.
pub fn check_seg_header(h: &[u8; SEG_HDR_LEN]) -> Result<u64, String> {
    if h[0..4] != SEG_MAGIC {
        return Err("a sealed file without the MSEG magic".into());
    }
    if u64::from_le_bytes(h[112..120].try_into().expect("8 bytes")) != xxh3_64(&h[..112]) {
        return Err("a SegHdr whose hdr_xxh3 does not match".into());
    }
    Ok(u64::from_le_bytes(h[64..72].try_into().expect("8 bytes")))
}

fn rel(s: String) -> Result<RelPathBuf, String> {
    RelPathBuf::from_string(s).map_err(|e| e.to_string())
}

/// The name of extent `n` ([F05 §2.1]).
pub fn extent_name(n: u64) -> String {
    format!("log.{n}")
}

/// A [`LogSource`] over the `log.<n>` files of a store root: it opens each extent when the scan first reads it and
/// keeps one open at a time.
pub struct VfsLog<'v, V: StoreFs> {
    vfs: &'v V,
    root: &'v V::Root,
    geo: Geometry,
    open: Option<(u64, V::File)>,
}

impl<'v, V: StoreFs> VfsLog<'v, V> {
    /// A source over `root`'s extents of length E.
    pub fn new(vfs: &'v V, root: &'v V::Root, geo: Geometry) -> VfsLog<'v, V> {
        VfsLog {
            vfs,
            root,
            geo,
            open: None,
        }
    }
}

impl<V: StoreFs> LogSource for VfsLog<'_, V> {
    fn read_at(&mut self, lsn: u64, buf: &mut [u8]) -> Result<usize, String> {
        let n = self.geo.extent(lsn);
        if self.open.as_ref().is_none_or(|(m, _)| *m != n) {
            let name = rel(extent_name(n))?;
            let f = self
                .vfs
                .open(
                    self.root,
                    name.as_rel_path(),
                    Access::Read,
                    OpenHint::Normal,
                )
                .map_err(|e| e.to_string())?;
            self.open = Some((n, f));
        }
        let (_, f) = self.open.as_ref().expect("opened");
        self.vfs
            .read_at(f, self.geo.offset(lsn), buf)
            .map_err(|e| e.to_string())
    }
}

/// The open floor ([60 §5.3], `60-roadmap.md:790`): opens `HEAD` and reads its 8,192 bytes with one `read_at`, then for
/// each sealed file opens it, reads its header, checks it and maps it with its `total_len`; drops the maps and handles
/// at the end. Returns the bytes mapped.
pub fn open_floor<V: StoreFs + SealedMaps>(
    vfs: &V,
    root: &V::Root,
    sealed: &[RelPathBuf],
) -> Result<u64, String> {
    let head = vfs
        .open(
            root,
            RelPath::literal("HEAD"),
            Access::Read,
            OpenHint::Normal,
        )
        .map_err(|e| e.to_string())?;
    let mut buf = [0u8; HEAD_LEN];
    let n = vfs.read_at(&head, 0, &mut buf).map_err(|e| e.to_string())?;
    if n != HEAD_LEN {
        return Err(format!("HEAD read {n} bytes, not 8,192"));
    }
    let mut maps = Vec::with_capacity(sealed.len());
    let mut mapped = 0;
    for name in sealed {
        let f = vfs
            .open(root, name.as_rel_path(), Access::Read, OpenHint::Normal)
            .map_err(|e| e.to_string())?;
        let mut h = [0u8; SEG_HDR_LEN];
        vfs.read_exact_at(&f, 0, &mut h)
            .map_err(|e| e.to_string())?;
        let total = check_seg_header(&h)?;
        let m = vfs
            .map_sealed(&f, total, name.as_rel_path())
            .map_err(|e| e.to_string())?;
        mapped += total;
        maps.push(m);
    }
    drop(maps);
    Ok(mapped)
}

/// A fixture written under a scratch directory: the tail's extents, `HEAD` and the sealed files.
#[derive(Clone, Debug)]
pub struct Fixture {
    /// The fixture directory.
    pub dir: PathBuf,
    /// Its name in its parent.
    pub name: String,
    /// The files written, by name.
    pub files: Vec<String>,
    /// The sealed files' names.
    pub sealed: Vec<RelPathBuf>,
    /// The tail's scan parameters.
    pub expect: Expect,
}

/// Writes `tail` as a store directory `<parent>/<name>`: `log.<n>` for every extent the tail touches (E bytes each),
/// `HEAD` (8,192 bytes, the slot magic `MOIR` at 0 and 4,096) and the [`SEALED_FILES`] sealed files of `sealed_len`
/// bytes each, sealed. Nothing is flushed: the measurement is warm ([AR §4.7]).
pub fn write_fixture<V: StoreFs>(
    vfs: &V,
    parent: &Path,
    name: &str,
    tail: &Tail,
    sealed_len: u64,
) -> Result<Fixture, String> {
    let dir = parent.join(name);
    let root = vfs
        .create_root(&dir, RootRole::Other)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let geo = tail.expect.geo;
    let mut files = Vec::new();
    let io = |e: moirai_vfs::VfsError| e.to_string();
    if tail.end > tail.start {
        for n in geo.extent(tail.start)..=geo.extent(tail.end - 1) {
            let from = geo.extent_start(n).max(tail.start);
            let to = geo.extent_start(n + 1).min(tail.end);
            let fname = extent_name(n);
            let f = vfs
                .create_new(&root, rel(fname.clone())?.as_rel_path())
                .map_err(io)?;
            files.push(fname);
            let bytes = &tail.bytes[(from - tail.start) as usize..(to - tail.start) as usize];
            vfs.write_at(&f, geo.offset(from), bytes).map_err(io)?;
            if geo.offset(to) != 0 {
                vfs.write_at(&f, geo.e() - 1, &[0]).map_err(io)?;
            }
        }
    }
    let head = vfs
        .create_new(&root, RelPath::literal("HEAD"))
        .map_err(io)?;
    files.push("HEAD".into());
    let mut slots = vec![0u8; HEAD_LEN];
    slots[..4].copy_from_slice(b"MOIR");
    slots[4_096..4_100].copy_from_slice(b"MOIR");
    vfs.write_at(&head, 0, &slots).map_err(io)?;
    let len = sealed_len.max(SEG_HDR_LEN as u64);
    let mut sealed = Vec::new();
    for (i, s) in sealed_names().into_iter().enumerate() {
        let r = rel(s.clone())?;
        let f = vfs.create_new(&root, r.as_rel_path()).map_err(io)?;
        let kind = if i == 0 { 3 } else { 4 };
        vfs.write_at(&f, 0, &seg_header(kind, i.max(1) as u32, len))
            .map_err(io)?;
        if len > SEG_HDR_LEN as u64 {
            vfs.write_at(&f, len - 1, &[0]).map_err(io)?;
        }
        vfs.seal(&f).map_err(io)?;
        files.push(s);
        sealed.push(r);
    }
    Ok(Fixture {
        dir,
        name: name.to_string(),
        files,
        sealed,
        expect: tail.expect,
    })
}

/// The store lock file ([F04 §2]'s neighbour in the store directory; [AR §4.7]).
pub const LOCK_NAME: &str = "LOCK";

/// The extent after the tail's last one: the spare a writer keeps prepared ([F16] P-72, P-96, as
/// [F17 Holes] HOLE(F17-lock-flush) cites them).
pub fn spare_extent(tail: &Tail) -> u64 {
    tail.expect.geo.extent(tail.end - 1) + 1
}

/// Writes the files one command opens beyond the open floor's (u20 cut §3.6, M-A6): `LOCK`, empty, and the spare extent
/// `log.<k + 1>` after the tail's last extent k, E bytes. Only `a1-opens10.n1e5` writes them (u20 addendum m-7), so no
/// other fixture changes. They join `fx.files`, so [`remove_fixture`] removes them.
pub fn write_command_files<V: StoreFs>(
    vfs: &V,
    fx: &mut Fixture,
    tail: &Tail,
) -> Result<(), String> {
    let io = |e: moirai_vfs::VfsError| e.to_string();
    let root = vfs
        .open_root(&fx.dir, RootRole::Other, RootAccess::ReadWrite)
        .map_err(io)?;
    let lock = vfs
        .create_new(&root, RelPath::literal(LOCK_NAME))
        .map_err(io)?;
    drop(lock);
    fx.files.push(LOCK_NAME.into());
    let name = extent_name(spare_extent(tail));
    let f = vfs
        .create_new(&root, rel(name.clone())?.as_rel_path())
        .map_err(io)?;
    vfs.write_at(&f, tail.expect.geo.e() - 1, &[0])
        .map_err(io)?;
    fx.files.push(name);
    Ok(())
}

/// B-1's file of pages (u20 cut §3.7; `design-B.md:636`): another process modifies it, 16 readers map it.
pub const PAGES_NAME: &str = "pages";
/// B-1's source pages: every modifier writes these bytes, read once from `<pages>.src`.
pub const PAGES_SRC_NAME: &str = "pages.src";
/// A page.
pub const PAGE: u64 = 4_096;
/// The source pages.
pub const SRC_PAGES: u64 = 64;
/// The chunk the pages file is written in.
pub const PAGES_CHUNK: u64 = 64 * 1_024 * 1_024;

/// SplitMix64's step.
fn splitmix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Seeded bytes: `len` bytes from the 8-byte words `splitmix(seed ^ i)` for i from `first_word`.
fn seeded(seed: u64, first_word: u64, len: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(len.next_multiple_of(8));
    let mut i = first_word;
    while v.len() < len {
        v.extend_from_slice(&splitmix(seed ^ i).to_le_bytes());
        i += 1;
    }
    v.truncate(len);
    v
}

/// The bytes of the pages file of `seed` at `off` (a multiple of 8), `len` of them.
pub fn pages_bytes(seed: u64, off: u64, len: usize) -> Vec<u8> {
    seeded(seed, off / 8, len)
}

/// The 64 source pages of `seed`: 256 KiB.
pub fn pages_src(seed: u64) -> Vec<u8> {
    seeded(!seed, 0, (SRC_PAGES * PAGE) as usize)
}

/// Writes B-1's fixture as a store directory `<parent>/<name>` (u20 cut §3.7): `pages`, `len` seeded bytes written in
/// 64-MiB chunks and flushed ([`pages_bytes`]), and `pages.src`, the 64 source pages ([`pages_src`]), flushed. Neither
/// is sealed: the modifiers write `pages`, and `map_sealed` checks only the length. `expect` is kept for the fixture's
/// form.
pub fn write_pages_fixture<V: StoreFs>(
    vfs: &V,
    parent: &Path,
    name: &str,
    len: u64,
    seed: u64,
    expect: Expect,
) -> Result<Fixture, String> {
    let dir = parent.join(name);
    let root = vfs
        .create_root(&dir, RootRole::Other)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let io = |e: moirai_vfs::VfsError| e.to_string();
    let flush = |e: moirai_vfs::DurabilityFailure| format!("{e:?}");
    let mut files = Vec::new();
    let pages = vfs
        .create_new(&root, RelPath::literal(PAGES_NAME))
        .map_err(io)?;
    files.push(PAGES_NAME.to_string());
    let mut off = 0;
    while off < len {
        let n = PAGES_CHUNK.min(len - off);
        vfs.write_at(&pages, off, &pages_bytes(seed, off, n as usize))
            .map_err(io)?;
        off += n;
    }
    vfs.sync(&pages, SyncKind::DataAndMeta).map_err(flush)?;
    drop(pages);
    let src = vfs
        .create_new(&root, RelPath::literal(PAGES_SRC_NAME))
        .map_err(io)?;
    files.push(PAGES_SRC_NAME.to_string());
    vfs.write_at(&src, 0, &pages_src(seed)).map_err(io)?;
    vfs.sync(&src, SyncKind::DataAndMeta).map_err(flush)?;
    drop(src);
    Ok(Fixture {
        dir,
        name: name.to_string(),
        files,
        sealed: Vec::new(),
        expect,
    })
}

/// Removes a fixture: its files, then its directory.
pub fn remove_fixture<V: StoreFs>(vfs: &V, parent: &Path, fx: &Fixture) -> Result<(), String> {
    let io = |e: moirai_vfs::VfsError| e.to_string();
    let retry = ShareRetry::Bounded {
        total_ms: OS_SHARE_RETRY_MS,
    };
    {
        let root = vfs
            .open_root(&fx.dir, RootRole::Other, RootAccess::ReadWrite)
            .map_err(io)?;
        for f in &fx.files {
            vfs.unlink(&root, rel(f.clone())?.as_rel_path(), retry)
                .map_err(io)?;
        }
    }
    let up = vfs
        .open_root(parent, RootRole::Other, RootAccess::ReadWrite)
        .map_err(io)?;
    vfs.remove_dir(&up, rel(fx.name.clone())?.as_rel_path())
        .map_err(io)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::overlay::Overlay;
    use crate::tail::memfs::MemFs;
    use crate::tail::scan::MemLog;
    use crate::tail::synth::{Clause, generate, tests::test_target};
    use crate::tail::{Params, TAIL_READ_CHUNK};

    /// A test-profile tail of at least 4 extents (an `ops` tail of 1,200 ops; not a grid point).
    pub(crate) fn multi_extent_tail() -> Tail {
        let mut t = test_target(Clause::Ops, 1_000, 21);
        t.threshold = 1_200;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, false).unwrap();
        let geo = tail.expect.geo;
        assert!(geo.extent(tail.end - 1) - geo.extent(tail.start) + 1 >= 4);
        tail
    }

    pub(crate) fn scratch() -> PathBuf {
        PathBuf::from("/scratch")
    }

    /// `VfsLog` reads a tail written as `log.<n>` files ([F05 §2.1], §2.3): its replay equals the replay from memory,
    /// one open per extent, every read inside one extent and at most one chunk.
    #[test]
    fn vfs_log_reads_across_extent_files() {
        let tail = multi_extent_tail();
        let fs = MemFs::with_dir(&scratch());
        let fx = write_fixture(&fs, &scratch(), "fx", &tail, 4_096).unwrap();
        let root = fs
            .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
            .unwrap();
        fs.clear_log();
        let geo = tail.expect.geo;
        let from_files = Overlay::replay(
            &mut VfsLog::new(&fs, &root, geo),
            &tail.expect,
            tail.start,
            tail.end,
            tail.chain_in,
            tail.base,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        let from_memory = Overlay::replay(
            &mut MemLog::new(tail.start, &tail.bytes),
            &tail.expect,
            tail.start,
            tail.end,
            tail.chain_in,
            tail.base,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        assert_eq!(from_files, from_memory);
        assert!(from_files.entries() > 100);
        let log = fs.log();
        let extents = (geo.extent(tail.start)..=geo.extent(tail.end - 1))
            .map(extent_name)
            .collect::<Vec<_>>();
        assert_eq!(log.opens, extents, "each extent opened once, in order");
        assert!(
            log.reads
                .iter()
                .all(|(_, off, n)| *n <= TAIL_READ_CHUNK && off + *n as u64 <= geo.e())
        );
        assert!(log.reads.iter().any(|(f, _, _)| f == &extents[3]));
    }

    /// The fixture: `log.<n>` of E bytes holding the tail, `HEAD` of 8,192 bytes, 8 sealed files with a valid `SegHdr`
    /// of their length; removing it leaves nothing.
    #[test]
    fn fixture_is_written_and_removed() {
        let tail = multi_extent_tail();
        let fs = MemFs::with_dir(&scratch());
        let fx = write_fixture(&fs, &scratch(), "fx", &tail, 5_000).unwrap();
        let geo = tail.expect.geo;
        let files = fs.files();
        let (first, last) = (geo.extent(tail.start), geo.extent(tail.end - 1));
        assert_eq!(
            files.len() as u64,
            (last - first + 1) + 1 + SEALED_FILES as u64
        );
        let mut joined = Vec::new();
        for n in first..=last {
            let b = fs.bytes(&fx.dir.join(extent_name(n))).unwrap();
            assert_eq!(b.len() as u64, Params::TEST.e, "log.{n} is E bytes");
            let from = geo.extent_start(n).max(tail.start);
            let to = geo.extent_start(n + 1).min(tail.end);
            joined.extend_from_slice(
                &b[geo.offset(from) as usize..(geo.offset(from) + to - from) as usize],
            );
            assert!(
                b[(geo.offset(from) + to - from) as usize..]
                    .iter()
                    .all(|&x| x == 0)
            );
        }
        assert_eq!(
            joined, tail.bytes,
            "the extents hold the tail at its offsets"
        );
        let head = fs.bytes(&fx.dir.join("HEAD")).unwrap();
        assert_eq!(head.len(), HEAD_LEN);
        assert_eq!(
            (&head[..4], &head[4_096..4_100]),
            (&b"MOIR"[..], &b"MOIR"[..])
        );
        for (i, s) in sealed_names().iter().enumerate() {
            let p = fx.dir.join(s);
            let b = fs.bytes(&p).unwrap();
            assert_eq!(b.len(), 5_000);
            assert!(fs.is_sealed(&p));
            let h: [u8; SEG_HDR_LEN] = b[..SEG_HDR_LEN].try_into().unwrap();
            assert_eq!(check_seg_header(&h), Ok(5_000));
            assert_eq!(
                h[6],
                if i == 0 { 3 } else { 4 },
                "seg_kind base, then deltas"
            );
        }
        let mut bad = seg_header(3, 1, 5_000);
        bad[70] ^= 1;
        assert!(check_seg_header(&bad).unwrap_err().contains("hdr_xxh3"));
        remove_fixture(&fs, &scratch(), &fx).unwrap();
        assert!(fs.files().is_empty() && !fs.has_dir(&fx.dir));
        assert!(fs.has_dir(&scratch()));
    }

    /// B-1's fixture (u20 cut §3.7): `pages` holds `len` seeded bytes, the same at every chunk boundary whichever chunk
    /// wrote them, and `pages.src` the 64 seeded source pages; both are in the file list, not sealed, and removed with the
    /// fixture.
    #[test]
    fn pages_fixture_holds_its_seeded_pages_and_source() {
        let fs = MemFs::with_dir(&scratch());
        let tail = multi_extent_tail();
        let len = 3 * 4_096 + 512;
        let fx = write_pages_fixture(&fs, &scratch(), "b1", len, 7, tail.expect).unwrap();
        assert_eq!(fx.files, [PAGES_NAME, PAGES_SRC_NAME]);
        let pages = fs.bytes(&fx.dir.join(PAGES_NAME)).unwrap();
        assert_eq!(pages, pages_bytes(7, 0, len as usize));
        assert_eq!(pages[4_096..8_192], pages_bytes(7, 4_096, 4_096)[..]);
        assert_ne!(pages[..4_096], pages[4_096..8_192]);
        let src = fs.bytes(&fx.dir.join(PAGES_SRC_NAME)).unwrap();
        assert_eq!(src, pages_src(7));
        assert_eq!(src.len() as u64, SRC_PAGES * PAGE);
        assert_ne!(pages_src(7), pages_src(8));
        assert!(!fs.is_sealed(&fx.dir.join(PAGES_NAME)));
        remove_fixture(&fs, &scratch(), &fx).unwrap();
        assert!(fs.files().is_empty() && !fs.has_dir(&fx.dir));
    }

    /// One command's files (u20 cut §3.6, M-A6; addendum m-7): `write_command_files` adds an empty `LOCK` and the spare
    /// extent after the tail's last one, E bytes of zeros, to the fixture and its file list, so `remove_fixture` removes
    /// them; a fixture without them holds neither.
    #[test]
    fn fixture_holds_lock_and_a_spare_extent() {
        let tail = multi_extent_tail();
        let fs = MemFs::with_dir(&scratch());
        let mut fx = write_fixture(&fs, &scratch(), "fx", &tail, 4_096).unwrap();
        let spare = fx.dir.join(extent_name(spare_extent(&tail)));
        assert!(fs.bytes(&fx.dir.join(LOCK_NAME)).is_none() && fs.bytes(&spare).is_none());
        let before = fs.files().len();
        write_command_files(&fs, &mut fx, &tail).unwrap();
        assert_eq!(fs.files().len(), before + 2);
        assert_eq!(fs.bytes(&fx.dir.join(LOCK_NAME)), Some(Vec::new()));
        let b = fs.bytes(&spare).unwrap();
        assert_eq!(b.len() as u64, Params::TEST.e);
        assert!(b.iter().all(|&x| x == 0));
        assert_eq!(
            spare_extent(&tail),
            tail.expect.geo.extent(tail.end - 1) + 1
        );
        remove_fixture(&fs, &scratch(), &fx).unwrap();
        assert!(fs.files().is_empty() && !fs.has_dir(&fx.dir));
    }
}
