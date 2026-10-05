//! `ProjectFs` on real NTFS ([OS/project], [OS/path]; X-F7, X-F8): canonical roots through junctions, P12 paths, the
//! CLI boundary, stale roots, volume capabilities, `stat` and enumeration with ids, on-disk spellings, `locate_id`, the
//! streaming reader and enumeration with their containment checks, links, busy holders, stamps and the mtime
//! granularity probe, the write side with its flush accounting, and cloud-placeholder attributes that block every
//! automatic open.

#![cfg(windows)]
#![allow(unsafe_code)]

mod common;

use std::ops::ControlFlow;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use common::TempDir;
use moirai_os::{OsProjectFs, OsProjectRoot};
use moirai_vfs::{
    At, BtimeTrust, CanonicalRoot, CaseRule, CloudRule, EntryName, EntryNameRef, EnumEnd,
    FileAttrs, FileIdKind, IdLocate, Located, OsCode, OsFileId, PathError, ProjKind, ProjectFs,
    ProjectRead, ReadOpts, RelPath, RenameRule, Renamed, ShareRetry, Stat, StatMode, StatRec,
    VfsErrorKind, VolumeCaps, VolumeKey,
};

fn open(p: &OsProjectFs, dir: &Path) -> (CanonicalRoot, OsProjectRoot) {
    let c = p.canonical_root(dir).unwrap();
    let r = p.open_root(&c).unwrap();
    (c, r)
}

fn at<'a>(r: &'a OsProjectRoot, s: &'a str) -> At<'a, OsProjectRoot> {
    At::new(r, RelPath::new(s).unwrap())
}

fn present(s: Stat) -> StatRec {
    match s {
        Stat::Present(r) => r,
        Stat::Absent => panic!("expected a present entry"),
    }
}

/// `mklink /J link target` (no privilege needed).
fn junction(link: &Path, target: &Path) {
    let out = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        // `mklink` reads a `/` as the start of a switch: backslashes only.
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "mklink: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn text_of(t: &TempDir) -> String {
    OsProjectFs::new()
        .canonical_root(t.path())
        .unwrap()
        .text
        .as_str()
        .to_owned()
}

#[test]
fn canonical_roots_are_on_disk_spellings() {
    let t = TempDir::new("canon");
    std::fs::create_dir(t.join("Repo")).unwrap();
    std::fs::write(t.join("file"), b"x").unwrap();
    let p = OsProjectFs::new();
    let c = p.canonical_root(&t.join("repo")).unwrap();
    let base = text_of(&t);
    assert_eq!(
        c.text.as_str(),
        format!("{base}/Repo"),
        "the on-disk spelling"
    );
    assert!(c.text.as_str().as_bytes()[0].is_ascii_uppercase() && &c.text.as_str()[1..3] == ":/");
    assert_eq!(c.root_id.kind, FileIdKind::Ntfs128);
    assert_eq!(c.os, moirai_vfs::OsTag::Windows);
    let lower = t.join("Repo").to_string_lossy().replacen("D:", "d:", 1) + "\\.\\";
    let c2 = p.canonical_root(Path::new(&lower)).unwrap();
    assert_eq!(c2.text, c.text);
    assert!(c2.root_id.same_object(&c.root_id));
    junction(&t.path().join("link"), &t.path().join("Repo"));
    let j = p.canonical_root(&t.join("link")).unwrap();
    assert_eq!(j.text, c.text, "a junction resolves to its target");
    assert!(j.root_id.same_object(&c.root_id));
    assert_eq!(
        p.canonical_root(&t.join("file")).unwrap_err().kind,
        VfsErrorKind::NotFound
    );
    assert_eq!(
        p.canonical_root(&t.join("missing")).unwrap_err().kind,
        VfsErrorKind::NotFound
    );
}

#[test]
fn machine_local_paths_and_the_user_config() {
    let t = TempDir::new("abs");
    std::fs::write(t.join("Readme.MD"), b"x").unwrap();
    let p = OsProjectFs::new();
    let base = text_of(&t);
    assert_eq!(
        p.canonical_abs(&t.join("readme.md")).unwrap().as_str(),
        format!("{base}/Readme.MD")
    );
    let absent = p.canonical_abs(&t.join("no/./such/../file")).unwrap();
    assert!(absent.as_str().ends_with("/no/file"), "{absent}");
    let cfg = moirai_os::path::user_config_path().expect("APPDATA is set in a user session");
    assert!(cfg.as_str().ends_with("/moirai/config"), "{cfg}");
}

#[test]
fn cli_arguments_become_tree_paths() {
    let t = TempDir::new("cli");
    std::fs::create_dir_all(t.join("Repo/src")).unwrap();
    let p = OsProjectFs::new();
    let tree = p.canonical_root(&t.join("Repo")).unwrap();
    let cwd = t.join("Repo/src");
    let cli = |a: &str| p.cli_path(a.as_ref(), &cwd, &tree);
    assert_eq!(cli("a.rs").unwrap().as_str(), "src/a.rs");
    assert_eq!(cli("..\\b.rs").unwrap().as_str(), "b.rs");
    assert_eq!(cli("./x/../y").unwrap().as_str(), "src/y");
    assert_eq!(cli("..").unwrap().as_str(), "");
    assert_eq!(cli("../..").unwrap_err(), PathError::OutsideRoot);
    assert_eq!(cli("/elsewhere").unwrap_err(), PathError::OutsideRoot);
    let abs = format!("{}/src/c.rs", tree.text.as_str());
    assert_eq!(cli(&abs).unwrap().as_str(), "src/c.rs");
    let bad = std::ffi::OsString::from_wide(&[u16::from(b'a'), 0xD800]);
    assert_eq!(
        p.cli_path(&bad, &cwd, &tree).unwrap_err(),
        PathError::NotUtf8
    );
}

#[test]
fn a_replaced_root_is_stale() {
    let t = TempDir::new("stale");
    std::fs::create_dir(t.join("Repo")).unwrap();
    let p = OsProjectFs::new();
    let c = p.canonical_root(&t.join("Repo")).unwrap();
    let (_, parent) = open(&p, t.path());
    p.rename_noreplace(
        at(&parent, "Repo"),
        at(&parent, "Repo.old"),
        ShareRetry::None,
    )
    .unwrap();
    std::fs::create_dir(t.join("Repo")).unwrap();
    assert_eq!(p.open_root(&c).unwrap_err().kind, VfsErrorKind::Stale);
    let c2 = p.canonical_root(&t.join("Repo")).unwrap();
    assert!(p.open_root(&c2).is_ok());
}

#[test]
fn ntfs_volume_capabilities() {
    let t = TempDir::new("caps");
    let p = OsProjectFs::new();
    let (c, r) = open(&p, t.path());
    let (key, caps) = p.volume(&r).unwrap();
    assert_eq!(key, c.root_id.vol_key);
    assert_eq!(caps.id_kind, FileIdKind::Ntfs128 as u8);
    assert_eq!(caps.id_locate, IdLocate::ById);
    assert_eq!(caps.btime, BtimeTrust::TunneledNotCopied);
    assert_eq!(caps.ctime_on_rename, Some(true));
    assert_eq!(caps.case_rule, CaseRule::PerDirFlag);
    assert!(caps.case_insensitive_default && !caps.norm_insensitive_always);
    assert_eq!(caps.cloud, CloudRule::RecallAttrs);
    assert_eq!(caps.rename_noreplace, RenameRule::Native);
    assert!(caps.ids_persistent && !caps.clone_indicators && !caps.docids);
    println!("OBSERVED: journal {:?}", caps.journal);
    assert_eq!(VolumeCaps::from_snapshot(&caps.to_snapshot()), Some(caps));
    assert_eq!(p.volume(&r).unwrap(), (key, caps), "cached per command");
    let eq = p.case_equivalent(at(&r, "")).unwrap();
    assert!(eq.case_insensitive && !eq.norm_insensitive);
    let drive = &c.text.as_str()[..1];
    assert_eq!(
        p.trash_dirs(&r)
            .unwrap()
            .iter()
            .map(|a| a.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec![format!("{drive}:/$Recycle.Bin")]
    );
    assert_eq!(p.file_handle_digest(at(&r, "")).unwrap(), None);
}

#[test]
fn stat_and_enumeration_agree() {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, SetFileAttributesW,
    };
    let t = TempDir::new("stat");
    std::fs::write(t.join("a.txt"), b"hello").unwrap();
    std::fs::create_dir(t.join("sub")).unwrap();
    std::fs::write(t.join("sub/b.bin"), [1u8; 3]).unwrap();
    std::fs::write(t.join("c.txt"), b"c").unwrap();
    let cw: Vec<u16> = t
        .join("c.txt")
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    // SAFETY: `cw` is NUL-terminated.
    let ok =
        unsafe { SetFileAttributesW(cw.as_ptr(), FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_HIDDEN) };
    assert_ne!(ok, 0);
    let p = OsProjectFs::new();
    let (c, r) = open(&p, t.path());
    let read = present(p.stat(at(&r, "a.txt"), StatMode::Read).unwrap());
    assert_eq!(
        (read.kind, read.size, read.id, read.nlink),
        (ProjKind::File, 5, None, 0)
    );
    assert!(!read.mtime.is_absent() && !read.btime.is_absent() && read.ctime.is_absent());
    assert_eq!(read.mtime.gran, 2);
    let with = present(p.stat(at(&r, "a.txt"), StatMode::WithId).unwrap());
    let id = with.id.unwrap();
    assert_eq!(id.kind, FileIdKind::Ntfs128);
    assert_eq!(id.parent, c.root_id.id, "the parent directory's id");
    assert_eq!(id.vol_key, c.root_id.vol_key);
    assert!(!with.ctime.is_absent() && with.nlink == 1);
    assert_eq!(with.mtime, read.mtime);
    assert_eq!(
        p.stat(at(&r, "missing"), StatMode::Read).unwrap(),
        Stat::Absent
    );
    assert_eq!(
        p.stat(at(&r, "a.txt/x"), StatMode::Read).unwrap(),
        Stat::Absent
    );
    assert_eq!(
        p.stat(at(&r, "missing"), StatMode::WithId).unwrap(),
        Stat::Absent
    );
    let dir = present(p.stat(at(&r, "sub"), StatMode::WithId).unwrap());
    assert_eq!(dir.kind, ProjKind::Dir);
    // Each entry is borrowed for one call of the visitor ([OS/project §5.2]); a caller that keeps a name takes
    // `to_owned()`.
    let mut seen: Vec<(String, StatRec)> = Vec::new();
    let mut kept: Vec<EntryName> = Vec::new();
    let end = p
        .enumerate(at(&r, ""), |e| {
            let EntryNameRef::Utf8(n) = e.name else {
                panic!("unrepresentable")
            };
            assert_eq!(e.stat.unwrap().kind, e.kind);
            seen.push((n.to_string(), e.stat.unwrap()));
            kept.push(e.name.to_owned());
            ControlFlow::Continue(())
        })
        .unwrap();
    kept.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(kept[0], EntryName::Utf8("a.txt".into()));
    assert_eq!(end, EnumEnd::Complete);
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    let names: Vec<&str> = seen.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["a.txt", "c.txt", "sub"]);
    assert_eq!(
        seen[0].1.id,
        Some(id),
        "enumeration and stat give one identity"
    );
    assert_eq!(seen[0].1.size, 5);
    assert!(
        seen[1]
            .1
            .attrs
            .contains(FileAttrs::READONLY | FileAttrs::HIDDEN)
    );
    assert_eq!(seen[2].1.id.unwrap().id, dir.id.unwrap().id);
    let mut n = 0;
    let end = p
        .enumerate(at(&r, ""), |_| {
            n += 1;
            ControlFlow::Break(())
        })
        .unwrap();
    assert_eq!((end, n), (EnumEnd::Stopped, 1));
    assert!(
        p.enumerate(at(&r, "a.txt"), |_| ControlFlow::Continue(()))
            .is_err()
    );
}

#[test]
fn disk_spellings() {
    let t = TempDir::new("spell");
    std::fs::create_dir(t.join("MixedCase")).unwrap();
    std::fs::write(t.join("MixedCase/Inner.TXT"), b"x").unwrap();
    let p = OsProjectFs::new();
    let (_, r) = open(&p, t.path());
    assert_eq!(
        p.disk_spelling(at(&r, "mixedcase/inner.txt"))
            .unwrap()
            .as_str(),
        "MixedCase/Inner.TXT"
    );
    assert_eq!(p.disk_spelling(at(&r, "")).unwrap().as_str(), "");
    assert!(
        present(
            p.stat(at(&r, "MIXEDCASE/INNER.txt"), StatMode::Read)
                .unwrap()
        )
        .kind
            == ProjKind::File
    );
}

#[test]
fn locate_id_follows_renames_out_of_and_into_nothing() {
    let outer = TempDir::new("locate");
    std::fs::create_dir_all(outer.join("tree/sub")).unwrap();
    std::fs::create_dir(outer.join("other")).unwrap();
    std::fs::write(outer.join("tree/x"), b"x").unwrap();
    let p = OsProjectFs::new();
    let (_, r) = open(&p, &outer.join("tree"));
    let (_, other) = open(&p, &outer.join("other"));
    let id = present(p.stat(at(&r, "x"), StatMode::WithId).unwrap())
        .id
        .unwrap();
    assert_eq!(
        p.locate_id(&r, &id, FileAttrs::NONE).unwrap(),
        Located::InRoot(RelPath::literal("x").to_buf())
    );
    p.rename_noreplace(at(&r, "x"), at(&r, "sub/y"), ShareRetry::None)
        .unwrap();
    assert_eq!(
        p.locate_id(&r, &id, FileAttrs::NONE).unwrap(),
        Located::InRoot(RelPath::literal("sub/y").to_buf())
    );
    p.rename_noreplace(at(&r, "sub/y"), at(&other, "z"), ShareRetry::None)
        .unwrap();
    match p.locate_id(&r, &id, FileAttrs::NONE).unwrap() {
        Located::Elsewhere(a) => assert!(a.as_str().ends_with("/other/z"), "{a}"),
        l => panic!("{l:?}"),
    }
    assert_eq!(
        p.locate_id(&r, &id, FileAttrs::OFFLINE).unwrap(),
        Located::NotLocatable
    );
    let foreign = OsFileId {
        vol_key: VolumeKey([9; 16]),
        ..id
    };
    assert_eq!(
        p.locate_id(&r, &foreign, FileAttrs::NONE).unwrap(),
        Located::NotLocatable
    );
    assert_eq!(
        p.locate_id(&r, &OsFileId::NONE, FileAttrs::NONE).unwrap(),
        Located::NotLocatable
    );
    p.unlink(at(&other, "z"), ShareRetry::None).unwrap();
    assert_eq!(
        p.locate_id(&r, &id, FileAttrs::NONE).unwrap(),
        Located::Gone
    );
    assert!(p.counters().id_lookups >= 4);
}

#[test]
fn the_reader_streams_two_passes_on_one_handle() {
    let outer = TempDir::new("reader");
    std::fs::create_dir_all(outer.join("tree/d")).unwrap();
    std::fs::create_dir(outer.join("outside")).unwrap();
    std::fs::write(outer.join("outside/secret"), b"s").unwrap();
    let data: Vec<u8> = (0..100_000u32).map(|i| (i % 241) as u8).collect();
    std::fs::write(outer.join("tree/f.bin"), &data).unwrap();
    junction(
        &outer.path().join("tree").join("j"),
        &outer.path().join("outside"),
    );
    let p = OsProjectFs::new();
    let (_, r) = open(&p, &outer.join("tree"));
    let opts = ReadOpts {
        allow_hydrate: false,
    };
    let b0 = p.counters().bytes_read;
    let mut rd = p.read_for_hash(at(&r, "f.bin"), opts).unwrap();
    let s1 = rd.snapshot().unwrap();
    assert_eq!(s1.size, 100_000);
    let pass = |rd: &mut moirai_os::OsReader| {
        let mut out = Vec::new();
        let mut buf = [0u8; 7_000];
        loop {
            let n = rd.read(&mut buf).unwrap();
            if n == 0 {
                return out;
            }
            out.extend_from_slice(&buf[..n]);
        }
    };
    assert!(pass(&mut rd) == data);
    rd.rewind().unwrap();
    assert!(pass(&mut rd) == data);
    assert_eq!(rd.snapshot().unwrap(), s1);
    let id = present(p.stat(at(&r, "f.bin"), StatMode::WithId).unwrap())
        .id
        .unwrap();
    assert!(rd.identity().unwrap().same_object(&id));
    assert!(p.counters().bytes_read >= b0 + 200_000);
    drop(rd);
    let kind = |s: &str| {
        p.read_for_hash(at(&r, s), opts)
            .map(|_| ())
            .unwrap_err()
            .kind
    };
    assert_eq!(kind("d"), VfsErrorKind::IsDirectory);
    assert_eq!(kind("missing"), VfsErrorKind::NotFound);
    assert_eq!(
        kind("j/secret"),
        VfsErrorKind::OutsideRoot,
        "a junction on the path never leads out"
    );
    assert_eq!(kind("j"), VfsErrorKind::Other, "a junction is never read");
    assert_eq!(
        present(p.stat(at(&r, "j"), StatMode::Read).unwrap()).kind,
        ProjKind::Other
    );
    let mut out = Vec::new();
    assert_eq!(
        p.read_link(at(&r, "j"), &mut out).unwrap_err().kind,
        VfsErrorKind::Other
    );
    assert_eq!(
        p.read_link(at(&r, "f.bin"), &mut out).unwrap_err().kind,
        VfsErrorKind::Other
    );
}

#[test]
fn symbolic_links_where_the_os_allows_them() {
    use windows_sys::Win32::Storage::FileSystem::{
        CreateSymbolicLinkW, SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
        SYMBOLIC_LINK_FLAG_DIRECTORY,
    };
    let t = TempDir::new("symlink");
    std::fs::create_dir(t.join("sub")).unwrap();
    std::fs::write(t.join("sub/b.bin"), b"b").unwrap();
    let link: Vec<u16> = t.join("l").as_os_str().encode_wide().chain([0]).collect();
    let target: Vec<u16> = "sub\\b.bin".encode_utf16().chain([0]).collect();
    // SAFETY: both strings are NUL-terminated.
    let ok = unsafe {
        CreateSymbolicLinkW(
            link.as_ptr(),
            target.as_ptr(),
            SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
        )
    };
    if !ok {
        println!(
            "OBSERVED: symbolic links need Developer Mode or elevation here; the link cases are skipped"
        );
        return;
    }
    let p = OsProjectFs::new();
    let (_, r) = open(&p, t.path());
    assert_eq!(
        present(p.stat(at(&r, "l"), StatMode::Read).unwrap()).kind,
        ProjKind::Symlink
    );
    let mut out = b"x=".to_vec();
    p.read_link(at(&r, "l"), &mut out).unwrap();
    assert_eq!(out, b"x=sub/b.bin", "git's spelling: `/` separators");
    let e = p
        .read_for_hash(
            at(&r, "l"),
            ReadOpts {
                allow_hydrate: false,
            },
        )
        .map(|_| ())
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::IsSymlink);

    // A directory symbolic link: `enumerate` refuses it as `IsSymlink` without a visit instead of listing its target
    // ([OS/project §5.2]), and `unlink` removes the link with `RemoveDirectoryW`, never the target ([OS/project §6.3]).
    let dlink: Vec<u16> = t.join("dl").as_os_str().encode_wide().chain([0]).collect();
    let dtarget: Vec<u16> = "sub".encode_utf16().chain([0]).collect();
    // SAFETY: both strings are NUL-terminated.
    let ok = unsafe {
        CreateSymbolicLinkW(
            dlink.as_ptr(),
            dtarget.as_ptr(),
            SYMBOLIC_LINK_FLAG_DIRECTORY | SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE,
        )
    };
    assert!(ok, "a directory link where a file link could be made");
    assert_eq!(
        present(p.stat(at(&r, "dl"), StatMode::Read).unwrap()).kind,
        ProjKind::Symlink
    );
    let mut n = 0;
    let e = p
        .enumerate(at(&r, "dl"), |_| {
            n += 1;
            ControlFlow::Continue(())
        })
        .unwrap_err();
    assert_eq!((e.kind, n), (VfsErrorKind::IsSymlink, 0));
    p.unlink(at(&r, "dl"), ShareRetry::None).unwrap();
    assert!(
        std::fs::symlink_metadata(t.join("dl")).is_err(),
        "the link is gone"
    );
    assert_eq!(
        std::fs::read(t.join("sub/b.bin")).unwrap(),
        b"b",
        "the target and its content stay"
    );
}

/// [OS/project §6.3]: "once if they are one directory" means one directory object. In a directory with per-directory
/// case sensitivity (`CaseRule::PerDirFlag`), `Src` and `src` are two directories, and `durable_rename(Src/a.rs →
/// src/a.rs)` flushes both parents; skipping the second would leave the new entry pending (FM-2.3). In a child, so the
/// process-wide counters move only by this rename.
#[test]
fn durable_rename_flushes_parents_that_differ_only_in_case() {
    let t = TempDir::new("pfscase");
    if !common::set_case_sensitive(t.path()) {
        println!(
            "OBSERVED: per-directory case sensitivity cannot be set here (fsutil); the case is skipped"
        );
        return;
    }
    std::fs::create_dir(t.join("Src")).unwrap();
    std::fs::create_dir(t.join("src")).unwrap();
    std::fs::write(t.join("Src/a.rs"), b"a").unwrap();
    let out = common::child_command("child", "case-rename")
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let line = text
        .lines()
        .find_map(|l| l.find("CASE").map(|i| &l[i..]))
        .unwrap_or_else(|| panic!("no CASE line in {text:?}"));
    // renames dir_syncs: one rename, a flush of `Src` and a flush of `src`.
    assert_eq!(line, "CASE 1 2");
    assert_eq!(std::fs::read(t.join("src/a.rs")).unwrap(), b"a");
    assert!(std::fs::symlink_metadata(t.join("Src/a.rs")).is_err());
}

#[test]
fn busy_holders_name_this_process() {
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    let t = TempDir::new("busy");
    std::fs::write(t.join("held"), b"h").unwrap();
    std::fs::create_dir(t.join("dir")).unwrap();
    let _h = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(t.join("held"))
        .unwrap();
    let p = OsProjectFs::new();
    let (_, r) = open(&p, t.path());
    let holders = p.busy_holders(at(&r, "held")).unwrap();
    assert!(
        holders.iter().any(|h| h.pid == std::process::id()),
        "{holders:?}"
    );
    assert_eq!(
        p.busy_holders(at(&r, "dir")).unwrap_err().kind,
        VfsErrorKind::Unsupported
    );
}

#[test]
fn stamps_and_the_granularity_probe() {
    let t = TempDir::new("stamp");
    let p = OsProjectFs::new();
    let (_, r) = open(&p, t.path());
    let m = p.touch_stamp(at(&r, "settle.stamp")).unwrap();
    assert!(!m.is_absent() && m.gran == 2);
    assert_eq!(std::fs::read(t.join("settle.stamp")).unwrap(), [0u8]);
    std::fs::write(t.join("settle.stamp"), b"abc").unwrap();
    p.touch_stamp(at(&r, "settle.stamp")).unwrap();
    assert_eq!(
        std::fs::read(t.join("settle.stamp")).unwrap(),
        b"\0bc",
        "one byte at offset 0"
    );
    let g = p.measure_mtime_granularity(&r).unwrap();
    println!("OBSERVED: effective mtime granularity {g} ns");
    assert!(g >= 100 && g % 100 == 0, "{g}");
}

#[test]
fn the_write_side_and_its_flush_accounting() {
    let t = TempDir::new("pfscounts");
    let out = common::child_command("child", "write-side")
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let line = text
        .lines()
        .find_map(|l| l.find("COUNTS").map(|i| &l[i..]))
        .unwrap();
    // renames dir_syncs unlinks: two durable renames (other dir: 2 flushes; same dir: 1), one plain rename, one
    // `sync_dir`, a durable unlink of a file and of an empty directory (1 flush each), a plain unlink.
    assert_eq!(line, "COUNTS 3 6 3");
}

#[test]
fn renames_never_replace_and_never_cross_volumes() {
    let t = TempDir::new("pfsrename");
    std::fs::create_dir_all(t.join("a/d")).unwrap();
    std::fs::write(t.join("a/f"), b"1").unwrap();
    std::fs::write(t.join("a/g"), b"2").unwrap();
    let p = OsProjectFs::new();
    let (_, r) = open(&p, t.path());
    let e = p
        .rename_noreplace(at(&r, "a/f"), at(&r, "a/g"), ShareRetry::None)
        .unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(t.join("a/g")).unwrap(), b"2");
    assert_eq!(
        p.rename_noreplace(at(&r, "a/d"), at(&r, "d2"), ShareRetry::None)
            .unwrap(),
        Renamed::Renamed,
        "a directory may be renamed"
    );
    let other = std::env::temp_dir();
    if other.to_string_lossy().get(..1) != t.path().to_string_lossy().get(..1) {
        let (_, o) = open(&p, &other);
        let name = format!("moirai-os-xvol-{}", std::process::id());
        let e = p
            .rename_noreplace(at(&r, "a/f"), at(&o, &name), ShareRetry::None)
            .unwrap_err();
        assert_eq!(e.kind, VfsErrorKind::CrossDevice, "no copy, ever");
        assert!(t.join("a/f").exists());
    }
}

#[test]
fn unlink_keeps_what_it_could_not_delete() {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_READONLY, FILE_SHARE_READ, SetFileAttributesW,
    };
    let t = TempDir::new("pfsunlink");
    std::fs::write(t.join("ro"), b"r").unwrap();
    std::fs::create_dir_all(t.join("full/x")).unwrap();
    let w: Vec<u16> = t.join("ro").as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `w` is NUL-terminated.
    let ok = unsafe { SetFileAttributesW(w.as_ptr(), FILE_ATTRIBUTE_READONLY) };
    assert_ne!(ok, 0);
    let p = OsProjectFs::new();
    let (_, r) = open(&p, t.path());
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(t.join("ro"))
        .unwrap();
    let e = p.unlink(at(&r, "ro"), ShareRetry::None).unwrap_err();
    assert!(
        matches!(
            e.kind,
            VfsErrorKind::SharingViolation | VfsErrorKind::AccessDenied
        ),
        "{e:?}"
    );
    assert!(
        std::fs::metadata(t.join("ro"))
            .unwrap()
            .permissions()
            .readonly(),
        "a failed delete restores the read-only attribute"
    );
    drop(holder);
    p.unlink(at(&r, "ro"), ShareRetry::None).unwrap();
    assert!(!t.join("ro").exists());
    assert_eq!(
        p.unlink(at(&r, "full"), ShareRetry::None).unwrap_err().kind,
        VfsErrorKind::IsDirectory
    );
    assert_eq!(
        p.remove_dir(at(&r, "full"), ShareRetry::None)
            .unwrap_err()
            .kind,
        VfsErrorKind::NotEmpty
    );
    p.durable_unlink(at(&r, "full/x"), ShareRetry::None)
        .unwrap();
    p.durable_unlink(at(&r, "full"), ShareRetry::None).unwrap();
    assert!(!t.join("full").exists());
}

/// Runs this test executable's `child` in `mode` on a fresh scratch directory and returns its `tag` line.
fn child_line(tag: &str, mode: &str) -> String {
    let t = TempDir::new(mode);
    let out = common::child_command("child", mode)
        .env("MOIRAI_OS_TEST_DIR", t.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    text.lines()
        .find_map(|l| l.find(tag).map(|i| l[i..].to_owned()))
        .unwrap_or_else(|| panic!("no {tag} line in {text:?}"))
}

#[test]
fn cloud_only_entries_are_never_opened() {
    // In a child, so the process-wide counters move only by this test's calls ([OS/project §2.4]).
    assert_eq!(child_line("CLOUD", "cloud"), "CLOUD ok");
}

#[test]
fn every_method_refuses_unrepresentable_names_before_any_os_call() {
    // [OS/project §2.3] (pass 1, P1-15): in a child, so an unmoved counter proves that no counted call ran.
    assert_eq!(child_line("NAMES", "names"), "NAMES ok");
}

#[test]
fn junctions_are_removed_as_links_and_never_enumerated() {
    let t = TempDir::new("pfsjunction");
    std::fs::create_dir(t.join("tree")).unwrap();
    std::fs::create_dir(t.join("target")).unwrap();
    std::fs::write(t.join("target/keep"), b"k").unwrap();
    junction(&t.path().join("tree").join("j"), &t.path().join("target"));
    junction(&t.path().join("tree").join("k"), &t.path().join("target"));
    let p = OsProjectFs::new();
    let (_, r) = open(&p, &t.join("tree"));
    assert_eq!(
        present(p.stat(at(&r, "j"), StatMode::Read).unwrap()).kind,
        ProjKind::Other
    );
    // A junction named by `enumerate` is refused, not followed out of the tree and not listed empty.
    let mut n = 0;
    let e = p
        .enumerate(at(&r, "j"), |_| {
            n += 1;
            ControlFlow::Continue(())
        })
        .unwrap_err();
    assert_eq!((e.kind, n), (VfsErrorKind::Other, 0));
    // `unlink` removes the link with `RemoveDirectoryW`; the target and its content stay.
    p.unlink(at(&r, "j"), ShareRetry::None).unwrap();
    assert!(std::fs::symlink_metadata(t.join("tree/j")).is_err());
    p.durable_unlink(at(&r, "k"), ShareRetry::None).unwrap();
    assert!(std::fs::symlink_metadata(t.join("tree/k")).is_err());
    assert_eq!(std::fs::read(t.join("target/keep")).unwrap(), b"k");
    // A real directory is still `IsDirectory` for `unlink`.
    std::fs::create_dir(t.join("tree/d")).unwrap();
    assert_eq!(
        p.unlink(at(&r, "d"), ShareRetry::None).unwrap_err().kind,
        VfsErrorKind::IsDirectory
    );
}

#[test]
fn enumeration_checks_the_open_directory_s_final_path() {
    // [OS/project §5.2] "Containment after the open (Windows)": `tree/j` is a junction to a directory outside the tree,
    // and `j/sub` is a plain directory reached through it, so the attribute read passes and only the final-path check
    // of the opened handle keeps the walk inside the tree, deterministically and without a race.
    let t = TempDir::new("pfsenumcontain");
    std::fs::create_dir_all(t.join("tree/inner/deep")).unwrap();
    std::fs::write(t.join("tree/inner/f"), b"f").unwrap();
    std::fs::create_dir_all(t.join("outside/sub/nested")).unwrap();
    std::fs::write(t.join("outside/sub/secret"), b"s").unwrap();
    junction(&t.path().join("tree").join("j"), &t.path().join("outside"));
    let p = OsProjectFs::new();
    let (_, r) = open(&p, &t.join("tree"));
    assert_eq!(
        present(p.stat(at(&r, "j/sub"), StatMode::Read).unwrap()).kind,
        ProjKind::Dir,
        "the leaf itself is a plain directory"
    );
    let listed = |s: &str| {
        let mut names = Vec::new();
        p.enumerate(at(&r, s), |e| {
            names.push(e.name.to_owned());
            ControlFlow::Continue(())
        })
        .map(|end| {
            assert_eq!(end, EnumEnd::Complete);
            names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            names
        })
    };
    let e = listed("j/sub").unwrap_err();
    assert_eq!(
        (e.kind, e.os, e.call),
        (VfsErrorKind::OutsideRoot, OsCode::NONE, "enumerate"),
        "a directory reached through a junction is never listed"
    );
    let e = listed("j/sub/nested").unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::OutsideRoot);
    // The handle was closed: the outside directory can be removed at once.
    std::fs::remove_dir(t.join("outside/sub/nested")).unwrap();
    // A normal subdirectory and the root itself (the final path equal to the root's text) still list.
    let names = |v: Vec<EntryName>| -> Vec<Vec<u8>> {
        v.into_iter().map(|n| n.as_bytes().to_vec()).collect()
    };
    assert_eq!(
        names(listed("inner").unwrap()),
        [b"deep".to_vec(), b"f".to_vec()]
    );
    assert_eq!(
        names(listed("").unwrap()),
        [b"inner".to_vec(), b"j".to_vec()]
    );
    assert!(listed("inner/deep").unwrap().is_empty());
}

/// The `cloud` child: an `OFFLINE` entry is refused by every automatic path without an open. The file is held open
/// with share mode 0, so any open for data would fail with a sharing violation instead of `CloudOnly`.
fn child_cloud(dir: &Path) {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW,
    };
    std::fs::write(dir.join("ph"), b"placeholder").unwrap();
    let w: Vec<u16> = dir
        .join("ph")
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    // SAFETY: `w` is NUL-terminated.
    let ok = unsafe { SetFileAttributesW(w.as_ptr(), FILE_ATTRIBUTE_OFFLINE) };
    assert_ne!(ok, 0);
    let p = OsProjectFs::new();
    let (_, r) = open(&p, dir);
    let s = present(p.stat(at(&r, "ph"), StatMode::Read).unwrap());
    assert!(s.attrs.is_cloud_only());
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.join("ph"))
        .unwrap();
    let no = ReadOpts {
        allow_hydrate: false,
    };
    let c0 = p.counters();
    assert_eq!(
        p.stat(at(&r, "ph"), StatMode::WithId).unwrap_err().kind,
        VfsErrorKind::CloudOnly
    );
    let e = p.read_for_hash(at(&r, "ph"), no).map(|_| ()).unwrap_err();
    assert_eq!(e.kind, VfsErrorKind::CloudOnly, "{e:?}");
    assert_eq!(
        p.disk_spelling(at(&r, "ph")).unwrap_err().kind,
        VfsErrorKind::CloudOnly
    );
    let c1 = p.counters();
    assert_eq!(c1.stats - c0.stats, 1, "one stat call");
    assert_eq!(
        (c1.content_opens, c1.bytes_read),
        (c0.content_opens, c0.bytes_read),
        "nothing was opened or read"
    );
    drop(holder);
    // Only an explicit verb with `--allow-hydrate` reads it.
    let mut rd = p
        .read_for_hash(
            at(&r, "ph"),
            ReadOpts {
                allow_hydrate: true,
            },
        )
        .unwrap();
    let mut buf = [0u8; 32];
    assert_eq!(rd.read(&mut buf).unwrap(), 11);
    drop(rd);
    let c2 = p.counters();
    assert_eq!(
        (
            c2.content_opens - c1.content_opens,
            c2.bytes_read - c1.bytes_read
        ),
        (1, 11)
    );
    // SAFETY: `w` is NUL-terminated.
    let ok = unsafe { SetFileAttributesW(w.as_ptr(), FILE_ATTRIBUTE_NORMAL) };
    assert_ne!(ok, 0);
    println!("CLOUD ok");
}

/// The `names` child: every method refuses a segment `representable_here` rejects, before any OS call — no counter
/// moves, and the entries such a name would reach through `\\?\` (the default stream of `x`, an alternate stream of
/// `a`, a literal `y.`) are untouched or never created.
fn child_names(dir: &Path) {
    std::fs::write(dir.join("x"), b"secret").unwrap();
    std::fs::create_dir(dir.join("d")).unwrap();
    std::fs::write(dir.join("d/f"), b"f").unwrap();
    let p = OsProjectFs::new();
    let (_, r) = open(&p, dir);
    let bad = [
        "x::$DATA",
        "a:b",
        "y.",
        "y ",
        "CON",
        "con.txt",
        "a*b",
        "q?",
        "d/x::$DATA",
        "d/NUL",
        "COM\u{B9}",
        "CONIN$",
    ];
    let invalid = |what: &str, s: &str, kind: VfsErrorKind, os: moirai_vfs::OsCode| {
        assert_eq!(
            (kind, os),
            (VfsErrorKind::InvalidName, moirai_vfs::OsCode(123)),
            "{what}({s:?})"
        );
    };
    let c0 = p.counters();
    let no = ReadOpts {
        allow_hydrate: false,
    };
    for s in bad {
        let e = |r: Result<(), moirai_vfs::VfsError>, what: &str| {
            let e = r.unwrap_err();
            invalid(what, s, e.kind, e.os);
        };
        let rf = |r: Result<Renamed, moirai_vfs::RenameFailure>, what: &str| match r.unwrap_err() {
            moirai_vfs::RenameFailure::NotDone(e) => invalid(what, s, e.kind, e.os),
            f => panic!("{what}({s:?}): {f:?}"),
        };
        e(p.stat(at(&r, s), StatMode::Read).map(|_| ()), "stat(Read)");
        e(
            p.stat(at(&r, s), StatMode::WithId).map(|_| ()),
            "stat(WithId)",
        );
        e(p.disk_spelling(at(&r, s)).map(|_| ()), "disk_spelling");
        e(
            p.enumerate(at(&r, s), |_| ControlFlow::Continue(()))
                .map(|_| ()),
            "enumerate",
        );
        e(p.read_for_hash(at(&r, s), no).map(|_| ()), "read_for_hash");
        e(p.read_link(at(&r, s), &mut Vec::new()), "read_link");
        e(p.busy_holders(at(&r, s)).map(|_| ()), "busy_holders");
        e(p.touch_stamp(at(&r, s)).map(|_| ()), "touch_stamp");
        e(p.case_equivalent(at(&r, s)).map(|_| ()), "case_equivalent");
        e(
            p.file_handle_digest(at(&r, s)).map(|_| ()),
            "file_handle_digest",
        );
        e(
            p.rename_noreplace(at(&r, "d/f"), at(&r, s), ShareRetry::None)
                .map(|_| ()),
            "rename_noreplace(to)",
        );
        e(
            p.rename_noreplace(at(&r, s), at(&r, "d/g"), ShareRetry::None)
                .map(|_| ()),
            "rename_noreplace(from)",
        );
        let f = p.sync_dir(at(&r, s)).unwrap_err();
        invalid("sync_dir", s, f.kind, f.os);
        rf(
            p.durable_rename(at(&r, "d/f"), at(&r, s), ShareRetry::None),
            "durable_rename(to)",
        );
        rf(
            p.durable_rename(at(&r, s), at(&r, "d/g"), ShareRetry::None),
            "durable_rename(from)",
        );
        e(p.unlink(at(&r, s), ShareRetry::None), "unlink");
        e(p.remove_dir(at(&r, s), ShareRetry::None), "remove_dir");
        match p.durable_unlink(at(&r, s), ShareRetry::None).unwrap_err() {
            moirai_vfs::RenameFailure::NotDone(e) => invalid("durable_unlink", s, e.kind, e.os),
            f => panic!("durable_unlink({s:?}): {f:?}"),
        }
    }
    assert_eq!(p.counters(), c0, "no counted call ran");
    assert_eq!(
        std::fs::read(dir.join("x")).unwrap(),
        b"secret",
        "x's default stream is untouched"
    );
    assert_eq!(std::fs::read(dir.join("d/f")).unwrap(), b"f");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["d", "x"], "nothing was created");
    println!("NAMES ok");
}

#[test]
fn child() {
    let Some(mode) = common::child_mode() else {
        return;
    };
    let dir = || std::path::PathBuf::from(std::env::var_os("MOIRAI_OS_TEST_DIR").unwrap());
    if mode == "cloud" {
        child_cloud(&dir());
        std::process::exit(0);
    }
    if mode == "names" {
        child_names(&dir());
        std::process::exit(0);
    }
    if mode == "case-rename" {
        let p = OsProjectFs::new();
        let (_, r) = open(&p, &dir());
        let c0 = p.counters();
        p.durable_rename(at(&r, "Src/a.rs"), at(&r, "src/a.rs"), ShareRetry::None)
            .unwrap();
        let c = p.counters();
        println!(
            "CASE {} {}",
            c.renames - c0.renames,
            c.dir_syncs - c0.dir_syncs
        );
        std::process::exit(0);
    }
    if mode == "write-side" {
        let dir = std::path::PathBuf::from(std::env::var_os("MOIRAI_OS_TEST_DIR").unwrap());
        for d in ["x", "y", "e"] {
            std::fs::create_dir(dir.join(d)).unwrap();
        }
        for f in ["x/1", "x/2", "x/3", "x/4"] {
            std::fs::write(dir.join(f), b"f").unwrap();
        }
        let p = OsProjectFs::new();
        let (_, r) = open(&p, &dir);
        let c0 = p.counters();
        p.durable_rename(at(&r, "x/1"), at(&r, "y/1"), ShareRetry::None)
            .unwrap();
        p.durable_rename(at(&r, "x/2"), at(&r, "x/2b"), ShareRetry::None)
            .unwrap();
        p.rename_noreplace(at(&r, "x/3"), at(&r, "x/3b"), ShareRetry::None)
            .unwrap();
        p.sync_dir(at(&r, "y")).unwrap();
        p.durable_unlink(at(&r, "x/4"), ShareRetry::None).unwrap();
        p.durable_unlink(at(&r, "e"), ShareRetry::None).unwrap();
        p.unlink(at(&r, "x/3b"), ShareRetry::None).unwrap();
        let c = p.counters();
        println!(
            "COUNTS {} {} {}",
            c.renames - c0.renames,
            c.dir_syncs - c0.dir_syncs,
            c.unlinks - c0.unlinks
        );
        std::process::exit(0);
    }
    std::process::exit(3);
}
