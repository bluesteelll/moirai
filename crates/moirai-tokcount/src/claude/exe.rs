//! Finding and checking the native Claude Code executable (PLAN WP-58: "the native `claude.exe` (never a `.cmd`
//! shim)").
//!
//! A package-manager install puts shims on `PATH` (`claude.cmd` and `claude.ps1` on Windows, a `#!` script
//! elsewhere) that start a script runtime, which then starts the CLI; a timeout that kills the shim can leave the
//! CLI running, and the shim's argument re-quoting is a second parser between the runner and Claude Code. The runner
//! therefore runs only a native executable image: a PE (`MZ`), ELF or Mach-O file, checked by its first bytes, whose
//! name carries the platform's executable suffix.

use std::env;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::Error;

/// Extensions that mark a script or shim, never a native image.
const SHIM_EXTENSIONS: [&str; 8] = ["cmd", "bat", "ps1", "sh", "js", "mjs", "cjs", "vbs"];

/// Checks that `path` is a native executable image: absolute, a file, not a shim by extension, named with the
/// platform's executable suffix (`.exe` on Windows), and starting with a PE, ELF or Mach-O signature.
///
/// # Errors
///
/// [`Error::Exe`] naming the first check that fails.
pub fn check_native(path: &Path) -> Result<(), Error> {
    if !path.is_absolute() {
        return Err(Error::Exe(format!(
            "{} is not an absolute path",
            path.display()
        )));
    }
    if let Some(ext) = path.extension().and_then(OsStr::to_str)
        && SHIM_EXTENSIONS
            .iter()
            .any(|shim| ext.eq_ignore_ascii_case(shim))
    {
        return Err(Error::Exe(format!(
            "{} is a .{ext} shim; configure the native executable (\"claude-exe\")",
            path.display()
        )));
    }
    let suffix = env::consts::EXE_SUFFIX;
    if !suffix.is_empty() {
        let name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
        let suffixed = name.len() > suffix.len()
            && name.is_char_boundary(name.len() - suffix.len())
            && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix);
        if !suffixed {
            return Err(Error::Exe(format!(
                "{} does not end in {suffix}",
                path.display()
            )));
        }
    }
    let magic = read_magic(path).map_err(|e| Error::Exe(format!("{}: {e}", path.display())))?;
    if is_native_magic(&magic) {
        Ok(())
    } else {
        Err(Error::Exe(format!(
            "{} is not a native executable image (no PE, ELF or Mach-O signature); a script shim is refused",
            path.display()
        )))
    }
}

/// Whether the first bytes of a file are a PE (`MZ`), ELF or Mach-O (thin or universal) signature.
#[must_use]
pub fn is_native_magic(head: &[u8]) -> bool {
    const MACH_O: [[u8; 4]; 5] = [
        [0xFE, 0xED, 0xFA, 0xCE],
        [0xFE, 0xED, 0xFA, 0xCF],
        [0xCE, 0xFA, 0xED, 0xFE],
        [0xCF, 0xFA, 0xED, 0xFE],
        [0xCA, 0xFE, 0xBA, 0xBE],
    ];
    head.starts_with(b"MZ")
        || head.starts_with(b"\x7FELF")
        || MACH_O.iter().any(|m| head.starts_with(m))
}

fn read_magic(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
    }
    let mut head = Vec::with_capacity(4);
    file.take(4).read_to_end(&mut head)?;
    Ok(head)
}

/// The executable's file name on this platform: `claude.exe` on Windows, `claude` elsewhere.
#[must_use]
pub fn file_name() -> String {
    format!("claude{}", env::consts::EXE_SUFFIX)
}

/// Finds the native executable: the first directory of `path_var` (a `PATH` value) holding a native
/// [`file_name`], then `<home>/.local/bin/<file_name>` (the native installer's location) for each of `homes`.
/// Shims and scripts of the same name are skipped.
#[must_use]
pub fn discover(path_var: Option<&OsStr>, homes: &[PathBuf]) -> Option<PathBuf> {
    let name = file_name();
    let on_path = path_var
        .into_iter()
        .flat_map(env::split_paths)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(&name));
    let installed = homes
        .iter()
        .map(|home| home.join(".local").join("bin").join(&name));
    on_path
        .chain(installed)
        .find(|candidate| check_native(candidate).is_ok())
}

/// [`discover`] over this process's `PATH`, with `USERPROFILE` and `HOME` as the homes.
#[must_use]
pub fn discover_from_env() -> Option<PathBuf> {
    let homes: Vec<PathBuf> = ["USERPROFILE", "HOME"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .collect();
    discover(env::var_os("PATH").as_deref(), &homes)
}

/// The configured executable, checked, or else the discovered one.
///
/// # Errors
///
/// The configured path fails [`check_native`], or discovery finds nothing.
pub fn resolve(configured: Option<&Path>) -> Result<PathBuf, Error> {
    match configured {
        Some(path) => check_native(path).map(|()| path.to_path_buf()),
        None => discover_from_env().ok_or_else(|| {
            Error::Exe(format!(
                "no native {} on PATH or in ~/.local/bin; set \"claude-exe\" in the runner configuration",
                file_name()
            ))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{check_native, discover, file_name, is_native_magic};
    use crate::claude::Error;
    use std::fs;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("moirai-tokcount-exe-{tag}-{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn exe_message(path: &std::path::Path) -> String {
        match check_native(path) {
            Err(Error::Exe(msg)) => msg,
            other => panic!("expected an executable error, got {other:?}"),
        }
    }

    #[test]
    fn signatures() {
        assert!(is_native_magic(b"MZ\x90\x00"));
        assert!(is_native_magic(b"\x7FELF"));
        assert!(is_native_magic(&[0xCF, 0xFA, 0xED, 0xFE]));
        assert!(is_native_magic(&[0xCA, 0xFE, 0xBA, 0xBE]));
        assert!(!is_native_magic(b"#!/usr/bin/env node"));
        assert!(!is_native_magic(b"@ECHO off"));
        assert!(!is_native_magic(b"M"));
        assert!(!is_native_magic(b""));
    }

    #[test]
    fn shims_and_scripts_are_refused() {
        let tmp = TempDir::new("shim");
        let cmd = tmp.0.join("claude.cmd");
        fs::write(&cmd, b"MZ pretending").unwrap();
        assert!(exe_message(&cmd).contains(".cmd shim"));
        let script = tmp.0.join(file_name());
        fs::write(&script, b"#!/bin/sh\nexec node cli.js \"$@\"\n").unwrap();
        assert!(exe_message(&script).contains("not a native executable image"));
        assert!(exe_message(&tmp.0.join("absent").join(file_name())).contains("absent"));
        assert!(exe_message(std::path::Path::new("claude")).contains("absolute"));
    }

    #[test]
    fn discovery_skips_shims_and_falls_back_to_the_installer_location() {
        let tmp = TempDir::new("discover");
        let shim_dir = tmp.0.join("npm");
        let home = tmp.0.join("home");
        let installed = home.join(".local").join("bin");
        fs::create_dir_all(&shim_dir).unwrap();
        fs::create_dir_all(&installed).unwrap();
        fs::write(shim_dir.join(file_name()), b"#!/bin/sh\n").unwrap();
        fs::write(shim_dir.join("claude.cmd"), b"@ECHO off\r\n").unwrap();
        fs::write(installed.join(file_name()), b"MZ\x90\x00native").unwrap();
        let path_var = std::env::join_paths([shim_dir.clone()]).unwrap();
        assert_eq!(
            discover(Some(&path_var), std::slice::from_ref(&home)),
            Some(installed.join(file_name()))
        );
        assert_eq!(discover(Some(&path_var), &[]), None);
        let native_dir = tmp.0.join("native");
        fs::create_dir_all(&native_dir).unwrap();
        fs::write(native_dir.join(file_name()), b"\x7FELF native").unwrap();
        let path_var = std::env::join_paths([shim_dir, native_dir.clone()]).unwrap();
        assert_eq!(
            discover(Some(&path_var), &[home]),
            Some(native_dir.join(file_name()))
        );
    }
}
