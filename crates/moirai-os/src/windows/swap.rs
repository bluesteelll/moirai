//! `swap_dirs` and `swap_recover` on Windows ([OS/fs §4.9]; [F15 §5.6]).
//!
//! Windows has no exchange primitive, so `swap_dirs` always takes the emulated form of [OS/fs §4.9.2]: it writes the
//! intent file `<a>.swap` in `a_parent` (the bytes of §4.9.3, encoded and checked by [`SwapIntent`], the codec the
//! simulator shares), then renames `A → T`, `B → A`,
//! `T → B` with `MoveFileExW(…, MOVEFILE_WRITE_THROUGH)` (never replacing) and removes the intent, with a `durable-name`
//! after every step. `swap_recover` reads the intent and completes or rolls back by the table of §4.9.4. The paths in
//! the intent are the machine-local absolute form of [80 §2.10] P12 (`X:/…`, `//server/share/…`).
//!
//! A failed flush embedded in a step is returned as `VfsError` of kind `FlushFailed` with the flush's code and call
//! ([OS/fs §4.1, §4.9], spec sync 2a) and leaves the intent for `swap_recover`; the caller exits 7 with the
//! `durability-failure` text and issues no further write, flush, create or namespace call.

use moirai_vfs::{
    Access, DurabilityFailure, FileIdentity, OpenHint, OsCode, RelPath, RootAccess, ShareRetry,
    StoreFs, SwapIntent, SwapOutcome, SwapRecovery, SyncKind, VfsError, VfsErrorKind,
};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::fs::{COUNTERS, OsRoot, OsVfs, bump, flush_dir_path, move_file, parent_z};
use super::path::{rewrite_final, verbatim_of_abs};
use super::sys::{self, Domain, error, id_info, open_attrs, raw};

fn invalid(call: &'static str) -> VfsError {
    VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, call)
}

fn rel(s: &str) -> Result<RelPath<'_>, VfsError> {
    RelPath::new(s).map_err(|_| invalid("swap_dirs"))
}

/// An embedded durability failure as the call's error: `FlushFailed` with the flush's code and call ([OS/fs §4.1]).
fn embedded(r: Result<(), DurabilityFailure>) -> Result<(), VfsError> {
    r.map_err(|f| f.embedded())
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
    let (i_name, t_name) = SwapIntent::side_names(a.as_str());
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
    if ![&a_path, &b_path, &t_path]
        .iter()
        .all(|p| SwapIntent::path_fits(p))
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
    let bytes = SwapIntent {
        a_id,
        b_id,
        a_path,
        b_path,
        t_path,
    }
    .encode()
    .ok_or_else(|| invalid("swap_dirs"))?;
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

/// Whether `rel` names an object under `root` (`NotFound` = absent; any other error is the call's).
fn present(v: &OsVfs, root: &OsRoot, rel: RelPath<'_>) -> Result<bool, VfsError> {
    match v.path_identity(root, rel) {
        Ok(_) => Ok(true),
        Err(e) if e.kind == VfsErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// A rename recorded in the intent, then `durable-name` of both parents, once if they are one directory
/// ([OS/fs §4.9.4]). The two parents come from the intent's paths, which `swap_dirs` built from final paths in on-disk
/// spelling, so one directory has one spelling and the comparison is byte-exact: no case folding, which in a tree with
/// per-directory case sensitivity would take two directories for one and skip a flush (FM-2.3).
fn recover_rename(from: &[u16], to: &[u16], retry: ShareRetry) -> Result<(), VfsError> {
    rename_dir(&sys::with_nul(from), &sys::with_nul(to), retry)?;
    let pf = parent_z(from).ok_or_else(|| unrecognised("swap state unrecognised"))?;
    let pt = parent_z(to).ok_or_else(|| unrecognised("swap state unrecognised"))?;
    embedded(flush_dir_path(&pf))?;
    if pf != pt {
        embedded(flush_dir_path(&pt))?;
    }
    Ok(())
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
    let (i_name, t_name) = SwapIntent::side_names(a.as_str());
    let i = rel(&i_name)?;
    let f = match v.open(a_parent, i, Access::Read, OpenHint::Normal) {
        Ok(f) => f,
        Err(e) if e.kind == VfsErrorKind::NotFound => return Ok(SwapRecovery::NoIntent),
        Err(e) => return Err(e),
    };
    // A read that returns an error is not an unreadable intent: the recovery fails with it and changes nothing
    // ([OS/fs §4.9.3]).
    let n = v.file_size(&f)?;
    let intent = if n > SwapIntent::MAX_LEN {
        None
    } else {
        let mut buf = vec![0u8; n as usize];
        v.read_exact_at(&f, 0, &mut buf)?;
        SwapIntent::decode(&buf)
    };
    drop(f);
    let Some(intent) = intent else {
        // [OS/fs §4.9.4] row "`I` unreadable": its write in step 3 never completed (a crash, or a failed flush), so
        // nothing was renamed, since step 4 starts only after `I` is durable. With `A` present and `T` absent the intent
        // is removed. `B` is named only inside the intent, so its presence cannot be read here; the soundness argument
        // needs only that step 4 never started, which `A` present and `T` absent confirm.
        let t = rel(&t_name)?;
        if present(v, a_parent, a)? && !present(v, a_parent, t)? {
            v.unlink(a_parent, i, retry)?;
            embedded(v.sync_dir(a_parent, None))?;
            return Ok(SwapRecovery::NothingDone);
        }
        return Err(unrecognised("swap intent unreadable"));
    };
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
    // The intent's codec is `moirai_vfs::SwapIntent`, tested there (golden bytes, damage, properties); these tests run
    // the swap and its recovery on real NTFS directories.
    use super::*;
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
        let bytes = SwapIntent {
            a_id: v.path_identity(p, A).unwrap(),
            b_id: v.path_identity(p, B).unwrap(),
            a_path: join(&base, "a"),
            b_path: join(&base, "b"),
            t_path: join(&base, "a.swap-old"),
        }
        .encode()
        .unwrap();
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
            let bytes = SwapIntent {
                a_id: v.path_identity(&pa, A).unwrap(),
                b_id: v.path_identity(&pb, B).unwrap(),
                a_path: join(&root_text(&pa).unwrap(), "a"),
                b_path: join(&root_text(&pb).unwrap(), "b"),
                t_path: join(&root_text(&pa).unwrap(), "a.swap-old"),
            }
            .encode()
            .unwrap();
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

    /// [OS/fs §4.9.3, §4.9.4] (spec sync 2a): an unreadable intent (a crash inside step 3 kept its name without its
    /// bytes) with `a` present and `a.swap-old` absent is removed (`NothingDone`); with `a.swap-old` present, or `a`
    /// absent, nothing changes and the recovery fails `Io` ("swap intent unreadable").
    #[test]
    fn an_unreadable_intent_before_any_rename_is_removed() {
        let v = OsVfs;
        for damage in [0usize, 1] {
            let (t, p) = scene("unreadable");
            write_intent(&t, &p);
            let path = t.path().join("a.swap");
            let mut bytes = std::fs::read(&path).unwrap();
            if damage == 0 {
                bytes.truncate(10); // a torn write
            } else {
                bytes[70] ^= 1; // a checksum mismatch
            }
            std::fs::write(&path, &bytes).unwrap();
            std::fs::create_dir(t.path().join("a.swap-old")).unwrap();
            let e = v.swap_recover(&p, A, ShareRetry::None).unwrap_err();
            assert_eq!(
                (e.kind, e.call),
                (VfsErrorKind::Io, "swap intent unreadable")
            );
            assert!(path.exists(), "nothing changes");
            std::fs::remove_dir(t.path().join("a.swap-old")).unwrap();
            assert_eq!(
                v.swap_recover(&p, A, ShareRetry::None).unwrap(),
                SwapRecovery::NothingDone
            );
            assert!(!path.exists(), "the intent is removed");
            assert_eq!(which(&t, "a").as_deref(), Some("A"));
            assert_eq!(which(&t, "b").as_deref(), Some("B"));
        }
        // An empty intent file (the create survived, no byte did) is unreadable too.
        let (t, p) = scene("unreadable-empty");
        std::fs::write(t.path().join("a.swap"), b"").unwrap();
        assert_eq!(
            v.swap_recover(&p, A, ShareRetry::None).unwrap(),
            SwapRecovery::NothingDone
        );
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
