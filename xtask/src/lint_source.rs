//! GT20 (d) and (a): the source scans, with the scopes of docs/m0/PLAN.md §2.1 as amended by §6.2 R18 ([AR §8.3]
//! GT20 (d); [80 §5.5] (a); A1 re-review A1P-10).
//!
//! | Scan | Refuses | Where |
//! |---|---|---|
//! | `cfg-os` | `cfg(target_os)`, `cfg(windows)`, `cfg(unix)` (and `target_family`) in any `cfg` form | every crate except `moirai-os`, tests included |
//! | `std-os` | `std::os::*` | every crate except `moirai-os`, tests included |
//! | `store-file` | `std::fs::rename`, `File::lock` and the other `File` lock calls, a glob import of `std::fs` | product crates, `moirai-os` included, tests included |
//! | `std-fs` | any `std::fs` path (`File::open`, `OpenOptions`, `read_dir`, `metadata`, `remove_file`, ...) | product crates except `moirai-os`, non-test code |
//! | `spawn` | `Command::new` (of `std::process`), `CreateProcess*`, `ShellExecute*`, `WinExec`, `posix_spawn*`, `libc::fork`/`exec*`/`system`, and `.spawn()`/`.output()`/`.status()`/`.exec()` calls in a file that names `std::process::Command` | product crates, non-test code, except the one allowed site: `moirai-os`'s `spawn` module (`src/<os>/spawn.rs` or `src/<os>/spawn/**`), the detached `moirai gc` child ([OS/proc §11], [OS/README §2.2]) |
//! | `clippy-config` | a product crate without its `clippy.toml`, one whose `disallowed-methods` lacks an entry of [`STORE_FILE_METHODS`] (and, outside `moirai-os`, of [`PATH_IO_METHODS`]), an `allow`/`expect` of `clippy::disallowed_methods` in its source, or `disallowed_methods = "allow"` in its or the workspace's lint table | product crates |
//!
//! The syntactic scans cannot see a receiver's type: `f.lock()` on a `File`, `f.try_lock()` and `f.unlock()` look like
//! any other `lock()`, and `path.exists()` like any other `exists()`. The type-aware layer is clippy's
//! `disallowed_methods`, configured by the `clippy.toml` in each product crate's directory (clippy reads the file from
//! the crate's manifest directory) and enforced by the gate's `clippy -D warnings`; `clippy-config` keeps those files
//! complete, and the scans above stay as the second layer.
//!
//! Test and tool crates that need a `cfg-os` or `std-os` exemption are listed in `xtask/osdeps-allow.toml`; a product
//! crate never is. Test code is: files under `tests/`, `benches/` and `examples/`; items under an attribute whose
//! `cfg` implies `test`, and `#[test]`/`#[bench]` functions; a file under `#![cfg(test)]`; and the files of an
//! out-of-line `#[cfg(test)] mod x;` (`x.rs` or `x/**` beside the declaring module).
//!
//! A spawn is the construction or the running of a process, not a mention of the type: `Meter::prepare_child(&self,
//! cmd: &mut std::process::Command)` ([OS/README §4.3], [OS/mem §4]) takes the caller's command and spawns nothing.
//! Without types, a scan cannot see a spawn through a `Command` built elsewhere and passed in; the method rule above
//! narrows that gap to files that do not name `std::process::Command` at all.

use crate::config::{Kind, OsdepsAllow};
use crate::diag::Diag;
use crate::paths::Pattern;
use crate::rustscan::{self, FileScan};

/// One crate to scan: its name, kind and every `.rs` file as `(repo-relative path, crate-relative path, text)`.
pub struct CrateSource<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub files: Vec<(String, String, String)>,
}

const OS_CRATE: &str = "moirai-os";

const SPAWN_FFI: &[&str] = &[
    "CreateProcessA",
    "CreateProcessW",
    "CreateProcessAsUserA",
    "CreateProcessAsUserW",
    "CreateProcessWithLogonW",
    "CreateProcessWithTokenW",
    "ShellExecuteA",
    "ShellExecuteW",
    "ShellExecuteExA",
    "ShellExecuteExW",
    "WinExec",
    "posix_spawn",
    "posix_spawnp",
];

const LIBC_SPAWN: &[&str] = &[
    "fork", "vfork", "execv", "execve", "execvp", "execvpe", "execl", "execle", "execlp", "system",
];

const FILE_LOCKS: &[&str] = &[
    "lock",
    "try_lock",
    "lock_shared",
    "try_lock_shared",
    "unlock",
];

/// The store-file calls every product crate's `clippy.toml` disallows, `moirai-os` included (PLAN §2.1: `File::lock`
/// and `std::fs::rename`).
pub const STORE_FILE_METHODS: &[&str] = &[
    "std::fs::File::lock",
    "std::fs::File::try_lock",
    "std::fs::File::lock_shared",
    "std::fs::File::try_lock_shared",
    "std::fs::File::unlock",
    "std::fs::rename",
];

/// The `Path` methods that do file-system I/O, disallowed in every product crate but `moirai-os` (PLAN §6.2 R18,
/// A1P-10: file I/O goes through the `Vfs` seam).
pub const PATH_IO_METHODS: &[&str] = &[
    "std::path::Path::exists",
    "std::path::Path::try_exists",
    "std::path::Path::is_file",
    "std::path::Path::is_dir",
    "std::path::Path::is_symlink",
    "std::path::Path::metadata",
    "std::path::Path::symlink_metadata",
    "std::path::Path::read_dir",
    "std::path::Path::read_link",
    "std::path::Path::canonicalize",
];

/// Follows a path of table keys from a root table.
fn table_path<'a>(t: &'a crate::toml::Table, path: &[&str]) -> Option<&'a crate::toml::Value> {
    let (first, rest) = path.split_first()?;
    t.get(*first)?.get_path(rest)
}

/// Whether a lint table (`[lints.clippy]` or `[workspace.lints.clippy]`) sets `disallowed_methods` to `allow`.
fn lint_allowed(t: Option<&crate::toml::Value>) -> bool {
    let Some(v) = t else { return false };
    let level = match v.as_table() {
        Some(tab) => tab.get("level").and_then(|l| l.as_str()),
        None => v.as_str(),
    };
    level == Some("allow")
}

/// The `clippy-config` check of one product crate: its `clippy.toml` (`None`: missing), its manifest and the
/// workspace root manifest.
pub fn check_clippy_config(
    krate: &str,
    dir: &str,
    clippy_toml: Option<&str>,
    manifest: Option<&crate::toml::Table>,
    root: Option<&crate::toml::Table>,
) -> Vec<Diag> {
    let path = format!("{dir}/clippy.toml");
    let d = |m: String| Diag {
        krate: Some(krate.to_string()),
        ..Diag::path("gt20-d", &path, m)
    };
    let mut out = Vec::new();
    let mut required: Vec<&str> = STORE_FILE_METHODS.to_vec();
    if krate != OS_CRATE {
        required.extend(PATH_IO_METHODS);
    }
    match clippy_toml {
        None => out.push(d(format!(
            "{krate} is a product crate without {path}: its `disallowed-methods` are the type-aware GT20 (d) layer (PLAN §2.1, §6.2 R18)"
        ))),
        Some(text) => match crate::toml::parse(text) {
            Err(e) => out.push(d(format!("{path}: {e}"))),
            Ok(t) => {
                let listed: Vec<String> = t
                    .get("disallowed-methods")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|v| match v.as_table() {
                        Some(tab) => tab.get("path").and_then(|p| p.as_str()).map(str::to_string),
                        None => v.as_str().map(str::to_string),
                    })
                    .collect();
                let missing: Vec<&str> = required
                    .iter()
                    .copied()
                    .filter(|r| !listed.iter().any(|l| l == r))
                    .collect();
                if !missing.is_empty() {
                    out.push(d(format!(
                        "`disallowed-methods` lacks {} (PLAN §2.1, §6.2 R18)",
                        missing.join(", ")
                    )));
                }
            }
        },
    }
    if lint_allowed(
        manifest.and_then(|m| table_path(m, &["lints", "clippy", "disallowed_methods"])),
    ) {
        out.push(Diag::krate(
            "gt20-d",
            krate,
            format!("{krate}'s [lints.clippy] sets disallowed_methods = \"allow\", switching off the type-aware GT20 (d) layer"),
        ));
    }
    if lint_allowed(
        root.and_then(|m| table_path(m, &["workspace", "lints", "clippy", "disallowed_methods"])),
    ) {
        out.push(Diag::krate(
            "gt20-d",
            krate,
            "[workspace.lints.clippy] sets disallowed_methods = \"allow\", switching off the type-aware GT20 (d) layer",
        ));
    }
    out
}

fn starts(segs: &[String], prefix: &[&str]) -> bool {
    segs.len() >= prefix.len() && segs.iter().zip(prefix).all(|(a, b)| a == b)
}

/// Whether a crate-relative file of `moirai-os` lies in its `spawn` module, the GT20 (a) lint's one allowed site: the
/// detached `moirai gc` child ([OS/proc §11]), placed per OS by the module tree of [OS/README §2.2].
fn spawn_site(rel: &str) -> bool {
    Pattern::new("src/{windows,unix,linux,macos}/spawn.rs").matches(rel)
        || Pattern::new("src/{windows,unix,linux,macos}/spawn/**").matches(rel)
}

/// Whether a crate-relative file is test code by its location.
fn test_by_location(rel: &str) -> bool {
    rel.starts_with("tests/") || rel.starts_with("benches/") || rel.starts_with("examples/")
}

/// The crate-relative directory under which an out-of-line module `m` declared in `rel` lives.
fn module_dir(rel: &str, m: &str) -> String {
    let (dir, file) = match rel.rfind('/') {
        Some(i) => (&rel[..i], &rel[i + 1..]),
        None => ("", rel),
    };
    let base = if matches!(file, "lib.rs" | "main.rs" | "mod.rs")
        || rel.starts_with("src/bin/") && dir == "src/bin"
    {
        dir.to_string()
    } else {
        let stem = file.trim_end_matches(".rs");
        if dir.is_empty() {
            stem.to_string()
        } else {
            format!("{dir}/{stem}")
        }
    };
    if base.is_empty() {
        m.to_string()
    } else {
        format!("{base}/{m}")
    }
}

/// Runs the scans over one crate.
pub fn check_crate(c: &CrateSource<'_>, osdeps: &OsdepsAllow) -> Vec<Diag> {
    let mut out = Vec::new();
    let scans: Vec<(&(String, String, String), FileScan)> =
        c.files.iter().map(|f| (f, rustscan::scan(&f.2))).collect();
    // Files of out-of-line test modules.
    let mut test_dirs: Vec<String> = Vec::new();
    for ((_, rel, _), s) in &scans {
        for m in &s.test_modules {
            test_dirs.push(module_dir(rel, m));
        }
    }
    let is_test_file = |rel: &str| {
        test_by_location(rel)
            || test_dirs
                .iter()
                .any(|d| rel == format!("{d}.rs") || rel.starts_with(&format!("{d}/")))
    };

    let os_crate = c.name == OS_CRATE;
    let product = c.kind == Kind::Product;
    let cfg_scope = !os_crate && !osdeps.exempt(c.name, "cfg-os");
    let stdos_scope = !os_crate && !osdeps.exempt(c.name, "std-os");

    for ((path, rel, _), s) in &scans {
        let file_test = is_test_file(rel);
        let spawn_ok = os_crate && spawn_site(rel);
        let d =
            |lint: &'static str, line: u32, msg: String| Diag::at(lint, c.name, path, line, msg);
        if cfg_scope {
            for (line, pred) in &s.cfg_os {
                out.push(d(
                    "gt20-d",
                    *line,
                    format!("OS-specific `{pred}` outside moirai-os (PLAN §2.1; tests included)"),
                ));
            }
        }
        for p in &s.paths {
            let test = file_test || p.test;
            let shown = || format!("{}{}", p.segs.join("::"), if p.glob { "::*" } else { "" });
            if stdos_scope && starts(&p.segs, &["std", "os"]) {
                out.push(d(
                    "gt20-d",
                    p.line,
                    format!(
                        "`{}` outside moirai-os (std::os::*, PLAN §2.1; tests included)",
                        shown()
                    ),
                ));
            }
            if product {
                let rename = starts(&p.segs, &["std", "fs", "rename"]);
                let lock = starts(&p.segs, &["std", "fs", "File"])
                    && p.segs
                        .get(3)
                        .is_some_and(|m| FILE_LOCKS.contains(&m.as_str()));
                let glob = p.glob && (p.segs == ["std", "fs"] || p.segs == ["std", "fs", "File"]);
                if rename || lock || glob {
                    out.push(d(
                        "gt20-d",
                        p.line,
                        format!("`{}` in a product crate (store-file calls: File::lock and std::fs::rename, PLAN §2.1)", shown()),
                    ));
                } else if !os_crate && !test && starts(&p.segs, &["std", "fs"]) {
                    out.push(d(
                        "gt20-d",
                        p.line,
                        format!("`{}`: direct std::fs I/O in a product crate other than moirai-os (PLAN §6.2 R18, A1P-10)", shown()),
                    ));
                }
                let spawn = p.segs == ["std", "process", "Command", "new"]
                    || p.segs
                        .last()
                        .is_some_and(|l| SPAWN_FFI.contains(&l.as_str()))
                    || (p.segs.len() == 2
                        && p.segs[0] == "libc"
                        && LIBC_SPAWN.contains(&p.segs[1].as_str()));
                if spawn && !test && !spawn_ok {
                    out.push(d(
                        "gt20-a",
                        p.line,
                        format!("`{}`: process spawn in the non-test code of a product crate (PLAN §2.1)", shown()),
                    ));
                }
            }
        }
        if product {
            for line in &s.lint_allows {
                out.push(d(
                    "gt20-d",
                    *line,
                    "an allow or expect of clippy::disallowed_methods in a product crate switches off the type-aware GT20 (d) layer (the crate's clippy.toml)".into(),
                ));
            }
            let names_command = s
                .paths
                .iter()
                .any(|p| !(file_test || p.test) && starts(&p.segs, &["std", "process", "Command"]));
            for (m, line, test) in &s.methods {
                if m.contains("lock") {
                    out.push(d(
                        "gt20-d",
                        *line,
                        format!(
                            "`.{m}()`: a std::fs::File lock call in a product crate (PLAN §2.1)"
                        ),
                    ));
                } else if names_command && !(file_test || *test) && !spawn_ok {
                    out.push(d(
                        "gt20-a",
                        *line,
                        format!("`.{m}()` in a file that names std::process::Command: a process spawn in the non-test code of a product crate (PLAN §2.1)"),
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, kind: Kind, files: &[(&str, &str)]) -> Vec<Diag> {
        let c = CrateSource {
            name,
            kind,
            files: files
                .iter()
                .map(|(r, t)| (format!("crates/{name}/{r}"), r.to_string(), t.to_string()))
                .collect(),
        };
        check_crate(&c, &OsdepsAllow::default())
    }

    fn lints(d: &[Diag]) -> Vec<(&str, u32)> {
        d.iter().map(|x| (x.lint, x.line.unwrap_or(0))).collect()
    }

    // Seeded source cases (PLAN WP-02 acceptance), each on both sides of its scope boundary.

    #[test]
    fn cfg_windows_outside_moirai_os_is_refused() {
        let src = "#[cfg(windows)]\nfn a() {}\n";
        assert_eq!(
            lints(&run("moirai-files", Kind::Product, &[("src/lib.rs", src)])),
            vec![("gt20-d", 1)]
        );
        assert_eq!(
            lints(&run("moirai-model", Kind::TestOnly, &[("tests/t.rs", src)])),
            vec![("gt20-d", 1)]
        );
        assert!(run("moirai-os", Kind::Product, &[("src/lib.rs", src)]).is_empty());
    }

    #[test]
    fn std_os_outside_moirai_os_is_refused_unless_exempt() {
        let src = "use std::os::windows::ffi::OsStrExt;\n";
        assert_eq!(
            lints(&run(
                "moirai-probes",
                Kind::TestOnly,
                &[("src/lib.rs", src)]
            )),
            vec![("gt20-d", 1)]
        );
        let osdeps = OsdepsAllow::from_table(
            &crate::toml::parse("version = 1\n[[crate]]\nname = \"moirai-probes\"\nscans = [\"std-os\"]\nreason = \"r\"\n").unwrap(),
        )
        .unwrap();
        let c = CrateSource {
            name: "moirai-probes",
            kind: Kind::TestOnly,
            files: vec![(
                "crates/moirai-probes/src/lib.rs".into(),
                "src/lib.rs".into(),
                src.into(),
            )],
        };
        assert!(check_crate(&c, &osdeps).is_empty());
        assert!(run("moirai-os", Kind::Product, &[("src/lib.rs", src)]).is_empty());
    }

    #[test]
    fn std_fs_rename_in_a_product_crate_is_refused() {
        let src = "fn m(a: &str, b: &str) { std::fs::rename(a, b).unwrap(); }\n";
        assert_eq!(
            lints(&run("moirai-files", Kind::Product, &[("src/lib.rs", src)])),
            vec![("gt20-d", 1)]
        );
        assert_eq!(
            lints(&run("moirai-os", Kind::Product, &[("src/fs.rs", src)])),
            vec![("gt20-d", 1)]
        );
        // Tests of a product crate are in scope; a test-only crate is not.
        assert_eq!(
            lints(&run("moirai-diff", Kind::Product, &[("tests/t.rs", src)])),
            vec![("gt20-d", 1)]
        );
        assert!(run("moirai-toylog", Kind::TestOnly, &[("src/lib.rs", src)]).is_empty());
        let aliased = "use std::fs::rename as mv;\nfn m() { mv(a, b); }\n";
        assert_eq!(
            lints(&run("moirai-os", Kind::Product, &[("src/fs.rs", aliased)])),
            vec![("gt20-d", 1), ("gt20-d", 2)]
        );
        let lock = "use std::fs::File;\nfn l(f: &File) { File::lock(f); f.try_lock_shared(); }\n";
        assert_eq!(
            lints(&run("moirai-os", Kind::Product, &[("src/lock.rs", lock)])),
            vec![("gt20-d", 2), ("gt20-d", 2)]
        );
    }

    #[test]
    fn std_fs_io_outside_moirai_os_is_refused_in_product_non_test_code() {
        let src = "fn r(p: &str) -> Vec<u8> { std::fs::read(p).unwrap() }\n#[cfg(test)]\nmod t { fn f() { std::fs::read(\"x\"); } }\n";
        assert_eq!(
            lints(&run("moirai-files", Kind::Product, &[("src/lib.rs", src)])),
            vec![("gt20-d", 1)]
        );
        assert!(run("moirai-os", Kind::Product, &[("src/fs.rs", src)]).is_empty());
        assert!(run("moirai-files", Kind::Product, &[("tests/r.rs", src)]).is_empty());
    }

    #[test]
    fn spawn_in_product_non_test_code_is_refused() {
        let src = "use std::process::Command;\nfn g() { Command::new(\"git\").status(); }\n";
        assert_eq!(
            lints(&run("moirai-files", Kind::Product, &[("src/lib.rs", src)])),
            vec![("gt20-a", 2), ("gt20-a", 2)]
        );
        // Naming the type is not a spawn ([OS/README §4.3] `prepare_child`); running a passed-in command is.
        let sig = "pub trait M { fn prepare_child(&self, cmd: &mut std::process::Command); }\n";
        assert!(run("moirai-vfs", Kind::Product, &[("src/meter.rs", sig)]).is_empty());
        let run_it = "fn go(cmd: &mut std::process::Command) { let _ = cmd.output(); }\n";
        assert_eq!(
            lints(&run("moirai-vfs", Kind::Product, &[("src/x.rs", run_it)])),
            vec![("gt20-a", 1)]
        );
        // The other side of the boundary: test code of a product crate, and a test-only crate.
        let test_src = "#[cfg(test)]\nmod t {\n    use std::process::Command;\n    fn g() { Command::new(\"git\"); }\n}\n";
        assert!(run("moirai-files", Kind::Product, &[("src/lib.rs", test_src)]).is_empty());
        assert!(run("moirai-files", Kind::Product, &[("tests/git.rs", src)]).is_empty());
        assert!(run("moirai-replay", Kind::TestOnly, &[("src/lib.rs", src)]).is_empty());
        let ffi =
            "fn s() { unsafe { windows_sys::Win32::System::Threading::CreateProcessW(); } }\n";
        assert_eq!(
            lints(&run("moirai-os", Kind::Product, &[("src/proc.rs", ffi)])),
            vec![("gt20-a", 1)]
        );
        // moirai-os's spawn module is the one allowed site ([OS/proc §11]); its namesakes elsewhere are not.
        assert_eq!(
            lints(&run(
                "moirai-os",
                Kind::Product,
                &[("src/windows/proc.rs", ffi)]
            )),
            vec![("gt20-a", 1)]
        );
        assert!(run("moirai-os", Kind::Product, &[("src/windows/spawn.rs", ffi)]).is_empty());
        assert!(
            run(
                "moirai-os",
                Kind::Product,
                &[("src/unix/spawn/posix.rs", src)]
            )
            .is_empty()
        );
        assert_eq!(
            lints(&run(
                "moirai-files",
                Kind::Product,
                &[("src/windows/spawn.rs", ffi)]
            )),
            vec![("gt20-a", 1)]
        );
        assert_eq!(
            lints(&run(
                "moirai-os",
                Kind::Product,
                &[("src/windows/spawner.rs", ffi)]
            )),
            vec![("gt20-a", 1)]
        );
    }

    fn clippy_toml(methods: &[&str]) -> String {
        let mut s = String::from("disallowed-methods = [\n");
        for m in methods {
            s.push_str(&format!("    {{ path = \"{m}\", reason = \"r\" }},\n"));
        }
        s.push_str("]\n");
        s
    }

    #[test]
    fn product_crates_carry_the_type_aware_layer() {
        let full: Vec<&str> = STORE_FILE_METHODS
            .iter()
            .chain(PATH_IO_METHODS)
            .copied()
            .collect();
        assert!(
            check_clippy_config(
                "moirai-files",
                "crates/moirai-files",
                Some(&clippy_toml(&full)),
                None,
                None
            )
            .is_empty()
        );
        // moirai-os needs only the store-file calls; the other product crates need the Path I/O methods too.
        let os = clippy_toml(STORE_FILE_METHODS);
        assert!(
            check_clippy_config("moirai-os", "crates/moirai-os", Some(&os), None, None).is_empty()
        );
        let d = check_clippy_config("moirai-vfs", "crates/moirai-vfs", Some(&os), None, None);
        assert!(
            d.len() == 1 && d[0].message.contains("std::path::Path::exists"),
            "{d:?}"
        );
        let d = check_clippy_config("moirai-diff", "crates/moirai-diff", None, None, None);
        assert!(
            d.len() == 1
                && d[0]
                    .message
                    .contains("without crates/moirai-diff/clippy.toml"),
            "{d:?}"
        );
        // Plain strings are accepted as entries; a lint table that allows the lint is refused.
        let strings = format!(
            "disallowed-methods = [{}]\n",
            STORE_FILE_METHODS
                .iter()
                .map(|m| format!("\"{m}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert!(
            check_clippy_config("moirai-os", "crates/moirai-os", Some(&strings), None, None)
                .is_empty()
        );
        let m = crate::toml::parse("[lints.clippy]\ndisallowed_methods = \"allow\"\n").unwrap();
        let r = crate::toml::parse(
            "[workspace.lints.clippy]\ndisallowed_methods = { level = \"allow\", priority = 1 }\n",
        )
        .unwrap();
        assert_eq!(
            check_clippy_config(
                "moirai-os",
                "crates/moirai-os",
                Some(&os),
                Some(&m),
                Some(&r)
            )
            .len(),
            2
        );
        // An allow in the source of a product crate is refused; in a test-only crate it is not.
        let src = "#![allow(clippy::disallowed_methods)]\n";
        assert_eq!(
            lints(&run("moirai-files", Kind::Product, &[("src/lib.rs", src)])),
            vec![("gt20-d", 1)]
        );
        assert!(run("moirai-toylog", Kind::TestOnly, &[("src/lib.rs", src)]).is_empty());
    }

    #[test]
    fn the_repository_product_crates_are_configured() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let kinds = crate::config::Config::load(&repo).unwrap().kinds;
        for (name, _) in kinds.crates.iter().filter(|(_, k)| *k == Kind::Product) {
            let dir = format!("crates/{name}");
            let text = std::fs::read_to_string(repo.join(&dir).join("clippy.toml")).ok();
            let d = check_clippy_config(name, &dir, text.as_deref(), None, None);
            assert!(d.is_empty(), "{d:#?}");
        }
    }

    #[test]
    fn out_of_line_test_modules_are_test_code() {
        let lib = "#[cfg(test)]\nmod tests;\n";
        let t = "fn g() { std::process::Command::new(\"x\"); std::fs::read(\"y\"); }\n";
        let d = run(
            "moirai-files",
            Kind::Product,
            &[
                ("src/lib.rs", lib),
                ("src/tests.rs", t),
                ("src/tests/more.rs", t),
            ],
        );
        assert!(d.is_empty(), "{d:?}");
        let nested = "#[cfg(test)]\nmod tests;\n";
        let d = run(
            "moirai-files",
            Kind::Product,
            &[("src/scan.rs", nested), ("src/scan/tests.rs", t)],
        );
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(module_dir("src/scan/mod.rs", "t"), "src/scan/t");
        assert_eq!(module_dir("src/lib.rs", "t"), "src/t");
    }
}
