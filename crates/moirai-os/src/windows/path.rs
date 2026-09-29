//! `os::path` on Windows ([OS/path §4.1, §5–§8, §10]; X-F7): canonical roots (P9), machine-local absolute paths (P12),
//! the CLI boundary, representability and the user-scope configuration location (X-F11).
//!
//! Every function here opens at most one handle and closes it before returning ([OS/path §4.1] step 6); a project root
//! is held as text and root id, never as an open handle ([OS/path §6]).

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use moirai_vfs::{
    AbsPath, CanonicalRoot, OsCode, OsTag, PathError, RelPathBuf, VfsError, VfsErrorKind,
};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::project::{file_id_of, volume_family};
use super::sys::{self, Domain, error, open_attrs, raw, standard_info, verbatim};

/// Rewrites a final path ([OS/path §4.1] step 4): a leading `\\?\UNC\` becomes `//`, a leading `\\?\` is removed,
/// every `\` becomes `/`, an ASCII lower-case drive letter is upper-cased, a trailing `/` is removed except in a drive
/// root `X:/`. The result must be valid UTF-16 and parse as `win-drive-path` or `win-unc-path` ([OS/path §2.2]).
pub(crate) fn rewrite_final(units: &[u16]) -> Option<String> {
    let s = String::from_utf16(units).ok()?;
    let rest = if let Some(r) = s.strip_prefix("\\\\?\\UNC\\") {
        format!("//{r}")
    } else if let Some(r) = s.strip_prefix("\\\\?\\") {
        r.to_owned()
    } else {
        s
    };
    let mut out = rest.replace('\\', "/");
    upper_drive(&mut out);
    while out.len() > 3 && out.ends_with('/') {
        out.pop();
    }
    let b = out.as_bytes();
    let drive = b.len() >= 3 && b[1] == b':' && b[2] == b'/';
    let unc = out.starts_with("//");
    if !(drive || unc) || AbsPath::new(&out).is_err() {
        return None;
    }
    Some(out)
}

/// Upper-cases an ASCII lower-case drive letter at position 0 before `:`.
fn upper_drive(s: &mut String) {
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_lowercase() {
        let up = (b[0] as char).to_ascii_uppercase();
        s.replace_range(0..1, up.encode_utf8(&mut [0u8; 4]));
    }
}

/// The `\\?\` form (no NUL) of a machine-local absolute path in the Windows forms of [OS/path §2.2]: `X:/a/b` →
/// `\\?\X:\a\b`, `//srv/share/a` → `\\?\UNC\srv\share\a`; `None` for any other form.
pub(crate) fn verbatim_of_abs(s: &str) -> Option<Vec<u16>> {
    let b = s.as_bytes();
    let body = if let Some(r) = s.strip_prefix("//") {
        format!("\\\\?\\UNC\\{}", r.replace('/', "\\"))
    } else if b.len() >= 3 && b[1] == b':' && b[2] == b'/' {
        format!("\\\\?\\{}", s.replace('/', "\\"))
    } else {
        return None;
    };
    Some(body.encode_utf16().collect())
}

/// The lexical normalisation of [OS/path §5] step 2 over an absolute path with `/` or `\` separators: drop empty and
/// `.` segments, let `..` remove the previous segment (never the drive, the `//server/share` prefix or the root), join
/// with `/`, upper-case the drive letter. `None` if `s` is not in a Windows absolute form.
pub(crate) fn lexical_normalize(s: &str) -> Option<String> {
    let s = s.replace('\\', "/");
    let (prefix, rest): (String, &str) = if let Some(r) = s.strip_prefix("//") {
        let mut parts = r.splitn(3, '/');
        let server = parts.next().filter(|p| !p.is_empty())?;
        let share = parts.next().filter(|p| !p.is_empty())?;
        (format!("//{server}/{share}"), parts.next().unwrap_or(""))
    } else {
        let b = s.as_bytes();
        if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/' {
            (format!("{}:", (b[0] as char).to_ascii_uppercase()), &s[3..])
        } else if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            (format!("{}:", (b[0] as char).to_ascii_uppercase()), "")
        } else {
            return None;
        }
    };
    let mut segs: Vec<&str> = Vec::new();
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    let mut out = prefix;
    if segs.is_empty() && !out.starts_with("//") {
        out.push('/');
    }
    for seg in segs {
        out.push('/');
        out.push_str(seg);
    }
    Some(out)
}

fn invalid(call: &'static str) -> VfsError {
    VfsError::new(VfsErrorKind::InvalidName, OsCode(123), call)
}

/// The canonical root of `dir` ([OS/path §4.1], P9): `GetFullPathNameW` for a relative input, `CreateFileW(…,
/// FILE_READ_ATTRIBUTES, share R|W|D, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS)` (reparse points and a `subst` drive are
/// followed), `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)` rewritten by step 4, `FileIdInfo` and
/// the volume's file-system name for the root's `OsFileId`, then the handle is closed. `D:\` and `d:\`, a junction into
/// the tree and a `subst` of a local folder give one text and one root id.
pub fn canonical_root(dir: &Path) -> Result<CanonicalRoot, VfsError> {
    let path = verbatim(dir).ok_or_else(|| invalid("GetFullPathNameW"))?;
    let h = open_attrs(&sys::with_nul(&path), FILE_FLAG_BACKUP_SEMANTICS)
        .map_err(|e| error(e, Domain::Project, "CreateFileW"))?;
    let std = standard_info(raw(&h))
        .map_err(|e| error(e, Domain::Project, "GetFileInformationByHandleEx"))?;
    if !std.Directory {
        return Err(VfsError::new(
            VfsErrorKind::NotFound,
            OsCode(267),
            "canonical_root",
        ));
    }
    let fin = sys::final_path(raw(&h))
        .map_err(|e| error(e, Domain::Project, "GetFinalPathNameByHandleW"))?;
    let text = rewrite_final(&fin).ok_or_else(|| invalid("GetFinalPathNameByHandleW"))?;
    let fs = volume_family(raw(&h), &fin)?;
    let root_id = file_id_of(raw(&h), fs, [0; 16])?;
    Ok(CanonicalRoot {
        text: AbsPath::from_string(text).map_err(|_| invalid("canonical_root"))?,
        root_id,
        os: OsTag::Windows,
    })
}

/// The machine-local absolute path of `p` ([OS/path §5], P12): an existing object is canonicalised as a root is (a file
/// or a directory; links on the path are followed); an absent one is made absolute (`GetFullPathNameW`) and normalised
/// lexically. Anything that does not match [OS/path §2.2] is `InvalidName`.
pub fn canonical_abs(p: &Path) -> Result<AbsPath, VfsError> {
    let path = verbatim(p).ok_or_else(|| invalid("GetFullPathNameW"))?;
    match open_attrs(&sys::with_nul(&path), FILE_FLAG_BACKUP_SEMANTICS) {
        Ok(h) => {
            let fin = sys::final_path(raw(&h))
                .map_err(|e| error(e, Domain::Project, "GetFinalPathNameByHandleW"))?;
            let text = rewrite_final(&fin).ok_or_else(|| invalid("GetFinalPathNameByHandleW"))?;
            AbsPath::from_string(text).map_err(|_| invalid("canonical_abs"))
        }
        Err(2 | 3) => {
            let s = String::from_utf16(&path).map_err(|_| invalid("GetFullPathNameW"))?;
            let body = if let Some(r) = s.strip_prefix("\\\\?\\UNC\\") {
                format!("//{r}")
            } else {
                s.strip_prefix("\\\\?\\").unwrap_or(&s).to_owned()
            };
            let norm = lexical_normalize(&body).ok_or_else(|| invalid("canonical_abs"))?;
            AbsPath::from_string(norm).map_err(|_| invalid("canonical_abs"))
        }
        Err(e) => Err(error(e, Domain::Project, "CreateFileW")),
    }
}

/// A CLI path argument as a path under `tree` ([OS/path §7]): the argument's UTF-16 must be valid; `\` becomes `/`;
/// `X:/…` and `//…` are absolute, a single leading `/` is the root of the current directory's drive; a drive-relative
/// `X:rel` (a drive letter and `:` not followed by `/`, a bare `X:` included) is refused with
/// [`PathError::DriveRelative`] and the device forms `//./…` and `//?/…` (from `\\.\…` and `\\?\…`) with
/// [`PathError::DevicePath`] (exit 2 `bad_path`, rules `drive-relative` and `device`; step 3, pass 1, P1-37), because
/// their meaning depends on a per-drive current directory or bypasses Win32 name handling; anything else is joined to
/// the canonical current directory; the result is normalised lexically; the tree's canonical text followed by `/` must
/// be its byte prefix (the tree root itself gives the empty path), and the rest must be a valid `RelPath`.
pub fn cli_path(arg: &OsStr, cwd: &Path, tree: &CanonicalRoot) -> Result<RelPathBuf, PathError> {
    let units: Vec<u16> = arg.encode_wide().collect();
    let s = String::from_utf16(&units)
        .map_err(|_| PathError::NotUtf8)?
        .replace('\\', "/");
    let b = s.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    let drive_abs = drive && b.get(2) == Some(&b'/');
    if drive && !drive_abs {
        return Err(PathError::DriveRelative);
    }
    if s.starts_with("//./") || s.starts_with("//?/") {
        return Err(PathError::DevicePath);
    }
    let joined = if drive_abs || s.starts_with("//") {
        s
    } else {
        let cwd = canonical_root(cwd).map_err(|_| PathError::NotAbsolute)?;
        let base = cwd.text.as_str();
        if let Some(rest) = s.strip_prefix('/') {
            // The root of the current directory's drive.
            if base.starts_with("//") {
                return Err(PathError::NotAbsolute);
            }
            format!("{}/{rest}", &base[..2])
        } else if base.ends_with('/') {
            format!("{base}{s}")
        } else {
            format!("{base}/{s}")
        }
    };
    let norm = lexical_normalize(&joined).ok_or(PathError::NotAbsolute)?;
    let root = tree.text.as_str();
    if norm == root {
        return Ok(RelPathBuf::from(moirai_vfs::RelPath::ROOT));
    }
    let rest = if root.ends_with('/') {
        norm.strip_prefix(root)
    } else {
        norm.strip_prefix(root).and_then(|r| r.strip_prefix('/'))
    };
    let rest = rest.ok_or(PathError::OutsideRoot)?;
    RelPathBuf::new(rest)
}

/// The Windows device names of [OS/path §8.2] (pass 1, A1-60): `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`,
/// `COM0`–`COM9`, `LPT0`–`LPT9`, and `COM` or `LPT` followed by a superscript digit `¹`, `²` or `³` (U+00B9, U+00B2,
/// U+00B3), which Windows also reserves.
const DEVICE_NAMES: [&str; 32] = [
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "CONIN$",
    "CONOUT$",
    "COM0",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{B9}",
    "COM\u{B2}",
    "COM\u{B3}",
    "LPT0",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{B9}",
    "LPT\u{B2}",
    "LPT\u{B3}",
];

/// `true` if the segment's stem — the part before its first `.`, trailing ASCII spaces removed — is a Windows device
/// name, ignoring ASCII case ([OS/path §8.2] `device-name`).
pub(crate) fn is_device_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or("").trim_end_matches(' ');
    DEVICE_NAMES.iter().any(|d| d.eq_ignore_ascii_case(stem))
}

/// Whether Windows can hold `segment` ([OS/path §8.1]): not a device name, not ending in `.` or a space, none of
/// `< > : " | ? *`, at most 255 UTF-16 code units. (`/`, `\`, U+0000 and C0 controls are excluded by P1 and P4 before.)
pub fn representable_here(segment: &str) -> bool {
    !is_device_name(segment)
        && !segment.ends_with('.')
        && !segment.ends_with(' ')
        && !segment.contains(['<', '>', ':', '"', '|', '?', '*'])
        && segment.encode_utf16().count() <= 255
}

/// The user-scope configuration file ([OS/path §10], X-F11): `canonical_abs` of `%APPDATA%\moirai\config` (the file
/// need not exist); `None` when `APPDATA` is unset, empty or relative. The environment is read rather than
/// `SHGetKnownFolderPath`, which would load `shell32.dll` into every process.
pub fn user_config_path() -> Option<AbsPath> {
    let appdata = std::env::var_os("APPDATA")?;
    if appdata.is_empty() {
        return None;
    }
    let base = Path::new(&appdata);
    if !base.is_absolute() {
        return None;
    }
    canonical_abs(&base.join("moirai").join("config")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn final_paths_are_rewritten() {
        assert_eq!(
            rewrite_final(&w("\\\\?\\d:\\Repo\\x")).as_deref(),
            Some("D:/Repo/x")
        );
        assert_eq!(rewrite_final(&w("\\\\?\\C:\\")).as_deref(), Some("C:/"));
        assert_eq!(
            rewrite_final(&w("\\\\?\\UNC\\srv\\share\\a\\")).as_deref(),
            Some("//srv/share/a")
        );
        assert_eq!(rewrite_final(&[0xD800]), None, "not valid UTF-16");
        assert_eq!(rewrite_final(&w("\\\\?\\Volume{x}\\a")), None);
    }

    #[test]
    fn abs_to_verbatim() {
        let s = |x: &str| verbatim_of_abs(x).map(|v| String::from_utf16(&v).unwrap());
        assert_eq!(s("D:/a/b").as_deref(), Some("\\\\?\\D:\\a\\b"));
        assert_eq!(s("//srv/sh/a").as_deref(), Some("\\\\?\\UNC\\srv\\sh\\a"));
        assert_eq!(s("/home/u"), None);
    }

    #[test]
    fn lexical_rules() {
        assert_eq!(
            lexical_normalize("c:/a/./b/../c/").as_deref(),
            Some("C:/a/c")
        );
        assert_eq!(lexical_normalize("C:\\..\\..").as_deref(), Some("C:/"));
        assert_eq!(lexical_normalize("//s/h/../x").as_deref(), Some("//s/h/x"));
        assert_eq!(lexical_normalize("//s/h/..").as_deref(), Some("//s/h"));
        assert_eq!(lexical_normalize("relative/x"), None);
        assert_eq!(lexical_normalize("//s"), None);
    }

    #[test]
    fn representability() {
        assert!(representable_here("readme.md"));
        for bad in [
            "CON", "con.txt", "Lpt3", "nul .x", "aux", "com0", "x.", "x ", "a:b", "q?", "s*",
        ] {
            assert!(!representable_here(bad), "{bad:?}");
        }
        assert!(representable_here("CONX"));
        assert!(representable_here("xcon"));
        assert!(representable_here(&"a".repeat(255)));
        assert!(!representable_here(&"a".repeat(256)));
        // The pass-1 additions of [OS/path §8.2] (A1-60): the console devices and the superscript-digit ports.
        for bad in [
            "CONIN$",
            "conout$.txt",
            "COM\u{B9}",
            "com\u{B2}.c",
            "LPT\u{B3}.x",
            "lpt\u{B9} .md",
        ] {
            assert!(!representable_here(bad), "{bad:?}");
        }
        assert!(representable_here("CONIN"));
        assert!(representable_here("COM\u{B9}\u{B9}"));
        assert!(
            representable_here("COM\u{2074}"),
            "only ¹, ² and ³ are reserved"
        );
    }

    /// The common cases [OS/path §8.1] requires both copies of the rule (this one and `moirai-files`' `representable`) to
    /// test: every device name of §8.2 bare, with an extension and with trailing spaces before the extension, in two ASCII
    /// cases; a name ending in `.` and one ending in ` `; each reserved character; a name of 255 and one of 256 UTF-16
    /// code units, one of them built from a supplementary-plane character (two units each).
    #[test]
    fn representability_common_cases() {
        for dev in DEVICE_NAMES {
            for d in [dev.to_ascii_uppercase(), dev.to_ascii_lowercase()] {
                for form in [d.clone(), format!("{d}.txt"), format!("{d}  .txt")] {
                    assert!(!representable_here(&form), "{form:?}");
                }
            }
        }
        assert!(!representable_here("name."));
        assert!(!representable_here("name "));
        for c in ['<', '>', ':', '"', '|', '?', '*'] {
            assert!(!representable_here(&format!("a{c}b")), "{c:?}");
        }
        let sup = "\u{1F600}";
        assert_eq!(sup.encode_utf16().count(), 2);
        assert!(
            representable_here(&format!("{}a", sup.repeat(127))),
            "255 units"
        );
        assert!(!representable_here(&sup.repeat(128)), "256 units");
        assert!(representable_here(&"a".repeat(255)));
        assert!(!representable_here(&"a".repeat(256)));
    }

    #[test]
    fn cli_paths_against_a_tree() {
        let tree = CanonicalRoot {
            text: AbsPath::new("D:/repo").unwrap(),
            root_id: moirai_vfs::OsFileId::NONE,
            os: OsTag::Windows,
        };
        let cwd = Path::new("C:\\");
        let p = |a: &str| cli_path(OsStr::new(a), cwd, &tree);
        assert_eq!(p("D:\\repo\\src\\a.rs").unwrap().as_str(), "src/a.rs");
        assert_eq!(p("d:/repo/./src/../b").unwrap().as_str(), "b");
        assert_eq!(p("D:/repo").unwrap().as_str(), "");
        assert_eq!(p("D:/repository/x"), Err(PathError::OutsideRoot));
        assert_eq!(p("E:/x"), Err(PathError::OutsideRoot));
        // Step 3 (pass 1, P1-37): drive-relative and device forms are refused before any join, whatever the cwd, each
        // with its own variant ([OS/path §11]).
        for bad in ["C:foo", "d:repo\\x", "D:", "z:"] {
            assert_eq!(p(bad), Err(PathError::DriveRelative), "{bad:?}");
        }
        for bad in [
            "\\\\?\\D:\\repo\\x",
            "\\\\.\\C:\\x",
            "//?/D:/repo/x",
            "//./C:/x",
            "\\\\?\\UNC\\srv\\share\\x",
        ] {
            assert_eq!(p(bad), Err(PathError::DevicePath), "{bad:?}");
        }
        // Not valid UTF-16: an unpaired surrogate.
        use std::os::windows::ffi::OsStringExt;
        let lone = std::ffi::OsString::from_wide(&[0x61, 0xD800]);
        assert_eq!(cli_path(&lone, cwd, &tree), Err(PathError::NotUtf8));
    }

    #[test]
    fn drive_relative_args_are_refused_with_the_cwd_inside_the_tree() {
        // A cwd inside the tree: before the fix `C:foo` was joined to it and accepted as the `RelPath` "C:foo".
        let t = crate::windows::testing::TempDir::new("clipath");
        let tree = canonical_root(t.path()).unwrap();
        let cwd = t.path();
        let p = |a: &str| cli_path(OsStr::new(a), cwd, &tree);
        assert_eq!(p("a/b").unwrap().as_str(), "a/b");
        let drive = &tree.text.as_str()[..1];
        assert_eq!(p(&format!("{drive}:foo")), Err(PathError::DriveRelative));
        assert_eq!(p("C:foo"), Err(PathError::DriveRelative));
        let dev = format!("\\\\?\\{}\\x", tree.text.as_str().replace('/', "\\"));
        assert_eq!(p(&dev), Err(PathError::DevicePath));
    }

    proptest! {
        #![proptest_config(crate::windows::testing::proptest_config())]

        /// Normalisation is idempotent, always yields a valid `AbsPath`, and never climbs above its prefix.
        #[test]
        fn normalisation_is_idempotent(segs in proptest::collection::vec("[a-z]{1,3}|\\.|\\.\\.|", 0..8),
                                       drive in "[a-zA-Z]") {
            let input = format!("{drive}:/{}", segs.join("/"));
            let once = lexical_normalize(&input).unwrap();
            prop_assert!(AbsPath::new(&once).is_ok(), "{}", once);
            prop_assert_eq!(lexical_normalize(&once).unwrap(), once.clone());
            let prefix = format!("{}:/", drive.to_ascii_uppercase());
            prop_assert!(once.starts_with(&prefix), "{} does not start with {}", once, prefix);
        }

        /// A rewritten final path converts back to the same `\\?\` form (for paths already in canonical case).
        #[test]
        fn verbatim_round_trip(segs in proptest::collection::vec("[A-Za-z0-9 _-]{1,6}", 0..5), drive in "[A-Z]") {
            let p12 = if segs.is_empty() { format!("{drive}:/") } else { format!("{drive}:/{}", segs.join("/")) };
            let v = verbatim_of_abs(&p12).unwrap();
            prop_assert_eq!(rewrite_final(&v).unwrap(), p12);
        }
    }
}
