//! `swap_dirs` and `swap_recover` on Windows ([OS/fs §4.9]; [F15 §5.6]).
//!
//! Windows has no exchange primitive, so `swap_dirs` always takes the emulated form of [OS/fs §4.9.2]: it writes the
//! intent file `<a>.swap` in `a_parent` (the bytes of §4.9.3, XXH3-64 checksum), then renames `A → T`, `B → A`,
//! `T → B` with `MoveFileExW(…, MOVEFILE_WRITE_THROUGH)` (never replacing) and removes the intent, with a `durable-name`
//! after every step. `swap_recover` reads the intent and completes or rolls back by the table of §4.9.4. The paths in
//! the intent are the machine-local absolute form of [80 §2.10] P12 (`X:/…`, `//server/share/…`).
//!
//! Failures of the embedded durability steps are returned as `VfsError` (the signatures of [OS/fs §4.9]); the caller
//! exits 7 through its error path.

use moirai_vfs::{
    Access, DurabilityFailure, FileIdentity, OpenHint, OsCode, RelPath, RootAccess, ShareRetry,
    StoreFs, SwapOutcome, SwapRecovery, SyncKind, VfsError, VfsErrorKind,
};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::fs::{COUNTERS, OsRoot, OsVfs, bump, flush_dir_path, move_file, parent_z};
use super::path::{rewrite_final, verbatim_of_abs};
use super::sys::{self, Domain, error, id_info, open_attrs, raw};
use super::xxh3::xxh3_64;

/// The intent's magic ([OS/fs §4.9.3]).
const MAGIC: [u8; 4] = *b"MSWP";
/// The fixed header length.
const HEADER: usize = 64;
/// The longest path an intent holds.
const MAX_PATH: usize = 4096;
/// The longest intent file.
const MAX_INTENT: u64 = (HEADER + 3 * MAX_PATH + 7 + 8) as u64;

/// The content of a swap intent ([OS/fs §4.9.3]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Intent {
    pub(crate) a_id: FileIdentity,
    pub(crate) b_id: FileIdentity,
    pub(crate) a_path: String,
    pub(crate) b_path: String,
    pub(crate) t_path: String,
}

fn u16_at(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn id_from(b: &[u8]) -> FileIdentity {
    let mut w = [0u8; 24];
    w.copy_from_slice(&b[..24]);
    FileIdentity::from_bytes(&w)
}

impl Intent {
    /// The file's bytes ([OS/fs §4.9.3]): the 64-byte header, the three paths, zero padding to a multiple of 8, and the
    /// XXH3-64 (seed 0) of all of that. The paths are 1–4096 bytes each (checked by the caller).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let paths = [&self.a_path, &self.b_path, &self.t_path];
        let p = HEADER + paths.iter().map(|s| s.len()).sum::<usize>();
        let pad = (8 - p % 8) % 8;
        let mut b = Vec::with_capacity(p + pad + 8);
        b.extend_from_slice(&MAGIC);
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&self.a_id.to_bytes());
        b.extend_from_slice(&self.b_id.to_bytes());
        for s in paths {
            b.extend_from_slice(&(s.len() as u16).to_le_bytes());
        }
        b.extend_from_slice(&0u16.to_le_bytes());
        for s in paths {
            b.extend_from_slice(s.as_bytes());
        }
        b.resize(p + pad, 0);
        let sum = xxh3_64(&b);
        b.extend_from_slice(&sum.to_le_bytes());
        b
    }

    /// The intent in `b`, if the length, the magic, `version = 1`, the reserved fields, the padding, the UTF-8 paths and
    /// the checksum all check; `None` otherwise ("swap intent unreadable").
    pub(crate) fn decode(b: &[u8]) -> Option<Intent> {
        if b.len() < HEADER + 8 || b[..4] != MAGIC || u16_at(b, 4) != 1 || u16_at(b, 6) != 0 {
            return None;
        }
        let lens = [u16_at(b, 56), u16_at(b, 58), u16_at(b, 60)];
        if u16_at(b, 62) != 0 || lens.iter().any(|&l| l == 0 || l > MAX_PATH) {
            return None;
        }
        let p = HEADER + lens.iter().sum::<usize>();
        let pad = (8 - p % 8) % 8;
        if b.len() != p + pad + 8 || b[p..p + pad].iter().any(|&x| x != 0) {
            return None;
        }
        let mut sum = [0u8; 8];
        sum.copy_from_slice(&b[p + pad..]);
        if xxh3_64(&b[..p + pad]) != u64::from_le_bytes(sum) {
            return None;
        }
        let mut at = HEADER;
        let mut path = |l: usize| {
            let s = core::str::from_utf8(&b[at..at + l]).ok().map(str::to_owned);
            at += l;
            s
        };
        Some(Intent {
            a_id: id_from(&b[8..32]),
            b_id: id_from(&b[32..56]),
            a_path: path(lens[0])?,
            b_path: path(lens[1])?,
            t_path: path(lens[2])?,
        })
    }
}

/// The names the emulated form uses in `a_parent`: the intent `<a>.swap` and the temporary `<a>.swap-old`.
fn side_names(a: &str) -> (String, String) {
    (format!("{a}.swap"), format!("{a}.swap-old"))
}

fn invalid(call: &'static str) -> VfsError {
    VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, call)
}

fn rel(s: &str) -> Result<RelPath<'_>, VfsError> {
    RelPath::new(s).map_err(|_| invalid("swap_dirs"))
}

/// An embedded durability failure as the call's error ([OS/fs §4.9]).
fn embedded(r: Result<(), DurabilityFailure>) -> Result<(), VfsError> {
    r.map_err(|f| VfsError::new(f.kind, f.os, f.call))
}

/// The P12 text of a root's directory.
fn root_text(r: &OsRoot) -> Result<String, VfsError> {
    rewrite_final(r.path()).ok_or_else(|| invalid("swap_dirs"))
}

fn join(p: &str, n: &str) -> String {
    if p.ends_with('/') {
        format!("{p}{n}")
    } else {
        format!("{p}/{n}")
    }
}

/// The identity of the directory `rel` under `root`; a missing or non-directory object is `NotFound`.
fn dir_identity(v: &OsVfs, root: &OsRoot, rel: RelPath<'_>) -> Result<FileIdentity, VfsError> {
    let p = root.path_of(rel, "CreateFileW")?;
    let h = open_attrs(&p, FILE_FLAG_BACKUP_SEMANTICS)
        .map_err(|e| error(e, Domain::Store, "CreateFileW"))?;
    let std = sys::standard_info(raw(&h))
        .map_err(|e| error(e, Domain::Store, "GetFileInformationByHandleEx"))?;
    if !std.Directory {
        return Err(VfsError::new(
            VfsErrorKind::NotFound,
            OsCode(267),
            "swap_dirs",
        ));
    }
    drop(h);
    v.path_identity(root, rel)
}

/// `durable-name` on `a` and, if it is another directory, on `b`.
fn sync_both(v: &OsVfs, a: &OsRoot, b: &OsRoot, same: bool) -> Result<(), VfsError> {
    embedded(v.sync_dir(a, None))?;
    if !same {
        embedded(v.sync_dir(b, None))?;
    }
    Ok(())
}

/// A directory rename by `\\?\` paths: no replace, write-through, the bounded retry ([OS/fs §4.9.2]).
fn rename_dir(from: &[u16], to: &[u16], retry: ShareRetry) -> Result<(), VfsError> {
    move_file(from, to, false, retry).map_err(|e| error(e, Domain::Store, "MoveFileExW"))?;
    bump(&COUNTERS.renames, 1);
    Ok(())
}

/// `swap_dirs` ([OS/fs §4.9.2]): the intent, three renames, the intent's removal, each step made durable.
pub(crate) fn swap_dirs(
    v: &OsVfs,
    a_parent: &OsRoot,
    a: RelPath<'_>,
    b_parent: &OsRoot,
    b: RelPath<'_>,
    retry: ShareRetry,
) -> Result<SwapOutcome, VfsError> {
    if a.segments().count() != 1 || b.segments().count() != 1 {
        return Err(invalid("swap_dirs"));
    }
    if a_parent.access() == RootAccess::Read || b_parent.access() == RootAccess::Read {
        return Err(VfsError::new(
            VfsErrorKind::AccessDenied,
            OsCode(5),
            "swap_dirs",
        ));
    }
    // Step 1: an intent or a temporary name left by an earlier swap is `doctor`'s.
    let (i_name, t_name) = side_names(a.as_str());
    let (i, t) = (rel(&i_name)?, rel(&t_name)?);
    for n in [i, t] {
        match v.path_identity(a_parent, n) {
            Err(e) if e.kind == VfsErrorKind::NotFound => {}
            Err(e) => return Err(e),
            Ok(_) => {
                return Err(VfsError::new(
                    VfsErrorKind::AlreadyExists,
                    OsCode(183),
                    "swap_dirs",
                ));
            }
        }
    }
    let (ap, bp) = (root_text(a_parent)?, root_text(b_parent)?);
    let (a_path, b_path, t_path) = (
        join(&ap, a.as_str()),
        join(&bp, b.as_str()),
        join(&ap, &t_name),
    );
    if [&a_path, &b_path, &t_path]
        .iter()
        .any(|p| p.len() > MAX_PATH)
    {
        return Err(invalid("swap_dirs"));
    }
    // Step 2: the identities (both must be directories on one volume).
    let a_id = dir_identity(v, a_parent, a)?;
    let b_id = dir_identity(v, b_parent, b)?;
    if a_id.volume != b_id.volume {
        return Err(VfsError::new(
            VfsErrorKind::CrossDevice,
            OsCode(17),
            "swap_dirs",
        ));
    }
    let same = v.root_identity(a_parent)? == v.root_identity(b_parent)?;
    // Step 3: the intent, durable with its name.
    let bytes = Intent {
        a_id,
        b_id,
        a_path,
        b_path,
        t_path,
    }
    .encode();
    let f = v.create_new(a_parent, i)?;
    v.write_at(&f, 0, &bytes)?;
    embedded(v.sync(&f, SyncKind::DataAndMeta))?;
    drop(f);
    embedded(v.sync_dir(a_parent, None))?;
    let (pa, pb, pt) = (
        a_parent.path_of(a, "MoveFileExW")?,
        b_parent.path_of(b, "MoveFileExW")?,
        a_parent.path_of(t, "MoveFileExW")?,
    );
    // Step 4: A → T.
    rename_dir(&pa, &pt, retry)?;
    embedded(v.sync_dir(a_parent, None))?;
    // Step 5: B → A.
    rename_dir(&pb, &pa, retry)?;
    sync_both(v, b_parent, a_parent, same)?;
    // Step 6: T → B.
    rename_dir(&pt, &pb, retry)?;
    sync_both(v, a_parent, b_parent, same)?;
    // Step 7: the intent goes.
    v.unlink(a_parent, i, retry)?;
    embedded(v.sync_dir(a_parent, None))?;
    Ok(SwapOutcome::TwoRenames)
}

fn unrecognised(what: &'static str) -> VfsError {
    VfsError::new(VfsErrorKind::Io, OsCode::NONE, what)
}

/// The identity of the object at an intent path, `None` if it is absent.
fn identity_at(verb: &[u16]) -> Result<Option<FileIdentity>, VfsError> {
    match open_attrs(&sys::with_nul(verb), FILE_FLAG_BACKUP_SEMANTICS) {
        Ok(h) => {
            let info = id_info(raw(&h))
                .map_err(|e| error(e, Domain::Store, "GetFileInformationByHandleEx"))?;
            Ok(Some(FileIdentity {
                volume: info.VolumeSerialNumber,
                file: info.FileId.Identifier,
            }))
        }
        Err(2 | 3) => Ok(None),
        Err(e) => Err(error(e, Domain::Store, "CreateFileW")),
    }
}

/// A rename recorded in the intent, then `durable-name` of both parents (once if they are one directory).
fn recover_rename(from: &[u16], to: &[u16], retry: ShareRetry) -> Result<(), VfsError> {
    rename_dir(&sys::with_nul(from), &sys::with_nul(to), retry)?;
    let pf = parent_z(from).ok_or_else(|| unrecognised("swap state unrecognised"))?;
    let pt = parent_z(to).ok_or_else(|| unrecognised("swap state unrecognised"))?;
    embedded(flush_dir_path(&pf))?;
    if !eq_ascii_ci(&pf, &pt) {
        embedded(flush_dir_path(&pt))?;
    }
    Ok(())
}

/// ASCII-case-insensitive equality of two UTF-16 paths (the intent's paths are canonical; only the drive letter's case
/// can differ from a path built here).
fn eq_ascii_ci(a: &[u16], b: &[u16]) -> bool {
    let fold = |u: u16| {
        if (0x61..=0x7A).contains(&u) {
            u - 0x20
        } else {
            u
        }
    };
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| fold(x) == fold(y))
}

/// `swap_recover` ([OS/fs §4.9.4]).
pub(crate) fn swap_recover(
    v: &OsVfs,
    a_parent: &OsRoot,
    a: RelPath<'_>,
    retry: ShareRetry,
) -> Result<SwapRecovery, VfsError> {
    if a.segments().count() != 1 {
        return Err(invalid("swap_recover"));
    }
    let (i_name, _) = side_names(a.as_str());
    let i = rel(&i_name)?;
    let f = match v.open(a_parent, i, Access::Read, OpenHint::Normal) {
        Ok(f) => f,
        Err(e) if e.kind == VfsErrorKind::NotFound => return Ok(SwapRecovery::NoIntent),
        Err(e) => return Err(e),
    };
    let n = v.file_size(&f)?;
    if n > MAX_INTENT {
        return Err(unrecognised("swap intent unreadable"));
    }
    let mut buf = vec![0u8; n as usize];
    v.read_exact_at(&f, 0, &mut buf)?;
    drop(f);
    let intent = Intent::decode(&buf).ok_or_else(|| unrecognised("swap intent unreadable"))?;
    let verb = |p: &str| verbatim_of_abs(p).ok_or_else(|| unrecognised("swap state unrecognised"));
    let (va, vb, vt) = (
        verb(&intent.a_path)?,
        verb(&intent.b_path)?,
        verb(&intent.t_path)?,
    );
    let (ia, ib, it) = (identity_at(&va)?, identity_at(&vb)?, identity_at(&vt)?);
    let (aid, bid) = (Some(intent.a_id), Some(intent.b_id));
    let result = if ia == aid && ib == bid && it.is_none() {
        SwapRecovery::NothingDone
    } else if ia.is_none() && ib == bid && it == aid {
        recover_rename(&vt, &va, retry)?;
        SwapRecovery::RolledBack
    } else if ia == bid && ib.is_none() && it == aid {
        recover_rename(&vt, &vb, retry)?;
        SwapRecovery::Completed
    } else if ia == bid && ib == aid && it.is_none() {
        SwapRecovery::Completed
    } else {
        return Err(unrecognised("swap state unrecognised"));
    };
    v.unlink(a_parent, i, retry)?;
    embedded(v.sync_dir(a_parent, None))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn id(n: u8) -> FileIdentity {
        FileIdentity {
            volume: 0x1234_5678_9ABC_DEF0,
            file: [n; 16],
        }
    }

    #[test]
    fn the_intent_round_trips_and_rejects_every_damage() {
        let i = Intent {
            a_id: id(1),
            b_id: id(2),
            a_path: "D:/repo/.git/moirai".into(),
            b_path: "D:/repo/.git/restore.1".into(),
            t_path: "D:/repo/.git/moirai.swap-old".into(),
        };
        let b = i.encode();
        assert_eq!(&b[..4], b"MSWP");
        assert_eq!(b.len() % 8, 0);
        assert_eq!(u16_at(&b, 56), i.a_path.len());
        assert_eq!(&b[8..16], &0x1234_5678_9ABC_DEF0u64.to_le_bytes());
        assert_eq!(Intent::decode(&b), Some(i));
        for at in [0, 4, 6, 8, 40, 56, 62, 64, b.len() - 9, b.len() - 1] {
            let mut d = b.clone();
            d[at] ^= 1;
            assert_eq!(Intent::decode(&d), None, "byte {at}");
        }
        assert_eq!(Intent::decode(&b[..b.len() - 1]), None);
        assert_eq!(Intent::decode(&[]), None);
        assert!(MAX_INTENT >= b.len() as u64);
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config { failure_persistence: None, ..Default::default() })]

        #[test]
        fn intents_round_trip(a in "[A-Z]:/[a-z0-9./ ]{0,60}", b in "[A-Z]:/[a-z0-9]{0,60}", t in "//[a-z]{1,8}/[a-z]{1,8}",
                              va in any::<u64>(), fa in any::<[u8; 16]>(), vb in any::<u64>(), fb in any::<[u8; 16]>()) {
            let i = Intent {
                a_id: FileIdentity { volume: va, file: fa },
                b_id: FileIdentity { volume: vb, file: fb },
                a_path: a, b_path: b, t_path: t,
            };
            let bytes = i.encode();
            prop_assert_eq!(bytes.len() % 8, 0);
            prop_assert_eq!(Intent::decode(&bytes), Some(i));
        }
    }

    // ---- On real NTFS directories ----

    use crate::windows::testing::TempDir;
    use moirai_vfs::{RootAccess, RootRole};

    const A: RelPath<'static> = RelPath::literal("a");
    const B: RelPath<'static> = RelPath::literal("b");
    const T: RelPath<'static> = RelPath::literal("a.swap-old");

    /// A parent directory with `a/which` = "A" and `b/which` = "B".
    fn scene(tag: &str) -> (TempDir, OsRoot) {
        let t = TempDir::new(tag);
        for (d, m) in [("a", "A"), ("b", "B")] {
            std::fs::create_dir(t.path().join(d)).unwrap();
            std::fs::write(t.path().join(d).join("which"), m).unwrap();
        }
        let p = OsVfs
            .open_root(t.path(), RootRole::Other, RootAccess::ReadWrite)
            .unwrap();
        (t, p)
    }

    fn which(t: &TempDir, d: &str) -> Option<String> {
        std::fs::read_to_string(t.path().join(d).join("which")).ok()
    }

    fn mv(p: &OsRoot, from: RelPath<'_>, to: RelPath<'_>) {
        let (f, t) = (p.path_of(from, "t").unwrap(), p.path_of(to, "t").unwrap());
        move_file(&f, &t, false, ShareRetry::None).unwrap();
    }

    fn write_intent(t: &TempDir, p: &OsRoot) {
        let v = OsVfs;
        let base = root_text(p).unwrap();
        let bytes = Intent {
            a_id: v.path_identity(p, A).unwrap(),
            b_id: v.path_identity(p, B).unwrap(),
            a_path: join(&base, "a"),
            b_path: join(&base, "b"),
            t_path: join(&base, "a.swap-old"),
        }
        .encode();
        std::fs::write(t.path().join("a.swap"), bytes).unwrap();
    }

    #[test]
    fn swap_exchanges_two_directories() {
        let (t, p) = scene("swap");
        let v = OsVfs;
        let before = v.counters().renames;
        assert_eq!(
            v.swap_dirs(&p, A, &p, B, ShareRetry::None).unwrap(),
            SwapOutcome::TwoRenames
        );
        assert!(v.counters().renames >= before + 3);
        assert_eq!(which(&t, "a").as_deref(), Some("B"));
        assert_eq!(which(&t, "b").as_deref(), Some("A"));
        assert!(!t.path().join("a.swap").exists() && !t.path().join("a.swap-old").exists());
        assert_eq!(
            v.swap_recover(&p, A, ShareRetry::None).unwrap(),
            SwapRecovery::NoIntent
        );
        // Swapping back restores the original assignment.
        v.swap_dirs(&p, A, &p, B, ShareRetry::None).unwrap();
        assert_eq!(which(&t, "a").as_deref(), Some("A"));
    }

    /// Two parents under one scratch directory: `pa/a/which` = "A" and `pb/b/which` = "B".
    fn scene2(tag: &str) -> (TempDir, OsRoot, OsRoot) {
        let t = TempDir::new(tag);
        for (p, d, m) in [("pa", "a", "A"), ("pb", "b", "B")] {
            std::fs::create_dir_all(t.path().join(p).join(d)).unwrap();
            std::fs::write(t.path().join(p).join(d).join("which"), m).unwrap();
        }
        let open = |p: &str| {
            OsVfs
                .open_root(&t.path().join(p), RootRole::Other, RootAccess::ReadWrite)
                .unwrap()
        };
        let (pa, pb) = (open("pa"), open("pb"));
        (t, pa, pb)
    }

    fn mv2(pf: &OsRoot, from: RelPath<'_>, pt: &OsRoot, to: RelPath<'_>) {
        let (f, t) = (pf.path_of(from, "t").unwrap(), pt.path_of(to, "t").unwrap());
        move_file(&f, &t, false, ShareRetry::None).unwrap();
    }

    #[test]
    fn swap_across_two_parents_flushes_both() {
        // [OS/fs §4.9.2] steps 5 and 6 flush the second parent when `b_parent` is another directory.
        let (t, pa, pb) = scene2("swap2");
        let v = OsVfs;
        let before = v.counters();
        assert_eq!(
            v.swap_dirs(&pa, A, &pb, B, ShareRetry::None).unwrap(),
            SwapOutcome::TwoRenames
        );
        let after = v.counters();
        // Steps 3, 4 and 7 flush `a_parent` once each; steps 5 and 6 flush both parents: 7 directory flushes (5 with
        // one parent). Other tests of this process may add to the process-wide counter, never subtract.
        assert!(after.sync_dir - before.sync_dir >= 7);
        assert!(after.renames - before.renames >= 3);
        let which2 =
            |p: &str, d: &str| std::fs::read_to_string(t.path().join(p).join(d).join("which")).ok();
        assert_eq!(which2("pa", "a").as_deref(), Some("B"));
        assert_eq!(which2("pb", "b").as_deref(), Some("A"));
        assert!(!t.path().join("pa/a.swap").exists() && !t.path().join("pa/a.swap-old").exists());
        assert!(!t.path().join("pb/b.swap").exists());
        v.swap_dirs(&pa, A, &pb, B, ShareRetry::None).unwrap();
        assert_eq!(which2("pa", "a").as_deref(), Some("A"));
        assert_eq!(which2("pb", "b").as_deref(), Some("B"));
    }

    #[test]
    fn recovery_across_two_parents() {
        let v = OsVfs;
        // (state reached before the crash, expected result, marker in `pa/a`, marker in `pb/b`)
        let cases = [
            (0, SwapRecovery::NothingDone, "A", "B"),
            (1, SwapRecovery::RolledBack, "A", "B"),
            (2, SwapRecovery::Completed, "B", "A"),
            (3, SwapRecovery::Completed, "B", "A"),
        ];
        for (state, want, ma, mb) in cases {
            let (t, pa, pb) = scene2("recover2");
            let bytes = Intent {
                a_id: v.path_identity(&pa, A).unwrap(),
                b_id: v.path_identity(&pb, B).unwrap(),
                a_path: join(&root_text(&pa).unwrap(), "a"),
                b_path: join(&root_text(&pb).unwrap(), "b"),
                t_path: join(&root_text(&pa).unwrap(), "a.swap-old"),
            }
            .encode();
            std::fs::write(t.path().join("pa/a.swap"), bytes).unwrap();
            if state >= 1 {
                mv2(&pa, A, &pa, T);
            }
            if state >= 2 {
                mv2(&pb, B, &pa, A);
            }
            if state >= 3 {
                mv2(&pa, T, &pb, B);
            }
            let before = v.counters().sync_dir;
            assert_eq!(
                v.swap_recover(&pa, A, ShareRetry::None).unwrap(),
                want,
                "state {state}"
            );
            if state == 2 {
                // The completing rename `T → B` crosses parents: both are flushed, then `a_parent` for the intent.
                assert!(v.counters().sync_dir - before >= 3, "state {state}");
            }
            let m = |p: &str, d: &str| {
                std::fs::read_to_string(t.path().join(p).join(d).join("which")).ok()
            };
            assert_eq!(m("pa", "a").as_deref(), Some(ma), "state {state}");
            assert_eq!(m("pb", "b").as_deref(), Some(mb), "state {state}");
            assert!(
                !t.path().join("pa/a.swap").exists(),
                "the intent is removed"
            );
            assert!(!t.path().join("pa/a.swap-old").exists());
        }
    }

    #[test]
    fn swap_refuses_leftovers_and_bad_arguments() {
        let (t, p) = scene("swapleft");
        let v = OsVfs;
        std::fs::write(t.path().join("a.swap-old"), b"x").unwrap();
        let e = v.swap_dirs(&p, A, &p, B, ShareRetry::None).unwrap_err();
        assert_eq!(e.kind, VfsErrorKind::AlreadyExists);
        let e = v
            .swap_dirs(&p, RelPath::literal("a/x"), &p, B, ShareRetry::None)
            .unwrap_err();
        assert_eq!(e.kind, VfsErrorKind::InvalidName);
        let e = v
            .swap_dirs(&p, RelPath::literal("missing"), &p, B, ShareRetry::None)
            .unwrap_err();
        assert_eq!(e.kind, VfsErrorKind::NotFound);
        let ro = v
            .open_root(t.path(), RootRole::Other, RootAccess::Read)
            .unwrap();
        let e = v.swap_dirs(&ro, A, &ro, B, ShareRetry::None).unwrap_err();
        assert_eq!(e.kind, VfsErrorKind::AccessDenied);
        assert_eq!(which(&t, "a").as_deref(), Some("A"), "nothing moved");
    }

    #[test]
    fn recovery_follows_the_table() {
        let v = OsVfs;
        // (state reached before the crash, expected result, marker in `a`, marker in `b`)
        let cases = [
            (0, SwapRecovery::NothingDone, "A", "B"),
            (1, SwapRecovery::RolledBack, "A", "B"),
            (2, SwapRecovery::Completed, "B", "A"),
            (3, SwapRecovery::Completed, "B", "A"),
        ];
        for (state, want, ma, mb) in cases {
            let (t, p) = scene("recover");
            write_intent(&t, &p);
            if state >= 1 {
                mv(&p, A, T);
            }
            if state >= 2 {
                mv(&p, B, A);
            }
            if state >= 3 {
                mv(&p, T, B);
            }
            assert_eq!(
                v.swap_recover(&p, A, ShareRetry::None).unwrap(),
                want,
                "state {state}"
            );
            assert_eq!(which(&t, "a").as_deref(), Some(ma), "state {state}");
            assert_eq!(which(&t, "b").as_deref(), Some(mb), "state {state}");
            assert!(!t.path().join("a.swap").exists(), "the intent is removed");
            assert!(!t.path().join("a.swap-old").exists());
        }
    }

    #[test]
    fn recovery_refuses_what_it_does_not_recognise() {
        let v = OsVfs;
        let (t, p) = scene("unrec");
        write_intent(&t, &p);
        mv(&p, A, RelPath::literal("elsewhere"));
        let e = v.swap_recover(&p, A, ShareRetry::None).unwrap_err();
        assert_eq!(
            (e.kind, e.call),
            (VfsErrorKind::Io, "swap state unrecognised")
        );
        assert!(t.path().join("a.swap").exists(), "nothing changes");
        let mut bytes = std::fs::read(t.path().join("a.swap")).unwrap();
        bytes[70] ^= 1;
        std::fs::write(t.path().join("a.swap"), bytes).unwrap();
        let e = v.swap_recover(&p, A, ShareRetry::None).unwrap_err();
        assert_eq!(
            (e.kind, e.call),
            (VfsErrorKind::Io, "swap intent unreadable")
        );
    }
}
