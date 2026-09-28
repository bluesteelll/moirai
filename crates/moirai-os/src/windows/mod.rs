//! The Windows implementation of the seams ([OS/README §2.2] module tree `src/windows/…`), built from M0
//! (PLAN §6.2 R1). The whole tree is configured in by one `#[cfg(windows)]` on its declaration in the crate root, at
//! module level only ([OS/README §2.2]).
//!
//! | Module | Contents | Specification |
//! |---|---|---|
//! | `sys` | handles, wide strings, final paths, the OS-code → kind mapping, NT opens, positional I/O | [OS/fs §5.1, §6.2], [OS/path §4.1, §6] |
//! | `fs` | `StoreFs` for [`OsVfs`], the process-wide counters | [OS/fs] |
//! | `swap` | the swap intent and `swap_dirs`/`swap_recover` | [OS/fs §4.9] |
//! | `xxh3` | XXH3-64 (the swap intent's checksum) | [OS/fs §4.9.3] |
//! | `lock` | `Locks`: the lock registry and the caller-driven grant-table driver | [OS/lock] |
//! | `map` | `SealedMaps`: mappings, the registry and the vectored `EXCEPTION_IN_PAGE_ERROR` handler | [OS/map] |
//! | `env` | `EnvGuard`: classification, the full probe, the OS version, `doctor` warnings | [OS/env] |
//! | `proc` | `Clock`, `ProcHost`, `Entropy`, the boot identity, `peak_of_child` | [OS/clock], [OS/proc], [OS/README §4.6] |
//! | `spawn` | the detached `gc` child and `enter_background` | [OS/proc §11] |
//! | `mem` | `Meter` ([`OsMeter`]) and [`CountingAlloc`] | [OS/mem] |
//! | `path` | canonical roots, P12 paths, the CLI boundary, representability, the user config path | [OS/path] |
//! | `project` | `ProjectFs` ([`OsProjectFs`]) | [OS/project] |
//! | `test_host` | kill, suspend, resume, wall-clock offset (feature `test-host`) | [OS/proc §13], [OS/clock §9] |
//!
//! `os::ipc` (the leader's endpoint) and `os::term` are not built at M0 ([OS/README §3]).

mod env;
mod fs;
mod lock;
mod map;
mod mem;
pub(crate) mod path;
mod proc;
mod project;
mod spawn;
mod swap;
mod sys;
#[cfg(feature = "test-host")]
pub mod test_host;
mod xxh3;

pub use fs::{OsFile, OsRoot, OsVfs};
pub use lock::OsLockClient;
pub use map::OsMap;
pub use mem::{CountingAlloc, OsMeter};
pub use proc::{OsParentWatch, OsWake};
pub use project::{OsProjectFs, OsProjectRoot, OsReader};

/// Scratch directories for the unit tests that need a real NTFS directory: under the lane's target directory (on D:
/// with the lane layout of PLAN §2.1), beside the test executable, removed on drop.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A scratch directory removed (recursively) on drop.
    pub(crate) struct TempDir(PathBuf);

    impl TempDir {
        /// A new, empty directory named after `tag`.
        pub(crate) fn new(tag: &str) -> TempDir {
            static N: AtomicU32 = AtomicU32::new(0);
            let exe = std::env::current_exe().expect("the test executable's path");
            // <target>/debug/deps/<exe> → <target>/tmp
            let base = exe
                .ancestors()
                .nth(3)
                .expect("the executable lies under <target>/<profile>/deps")
                .join("tmp")
                .join("moirai-os-unit");
            std::fs::create_dir_all(&base).expect("the scratch base");
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let dir = base.join(format!(
                "{tag}-{}-{nanos}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).expect("a fresh scratch directory");
            TempDir(dir)
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
