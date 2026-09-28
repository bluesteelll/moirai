//! Helpers shared by the integration tests of `moirai-os`: scratch directories on the lane's target volume (D: in the
//! lane layout of PLAN §2.1, a real NTFS volume) and child processes that re-run the test executable in a named mode.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// The environment variable that puts a re-run test executable into a child mode.
pub const CHILD_ENV: &str = "MOIRAI_OS_TEST_CHILD";

/// A scratch directory under `CARGO_TARGET_TMPDIR`, removed recursively on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("moirai-os");
        std::fs::create_dir_all(&base).unwrap();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir = base.join(format!(
            "{tag}-{}-{nanos}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }
}

/// Clears the read-only attribute of every file below `p` (sealed files), so the tree can be removed.
fn clear_readonly(p: &Path) {
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if let Ok(md) = std::fs::symlink_metadata(&path) {
                if md.is_dir() {
                    clear_readonly(&path);
                } else {
                    let mut perm = md.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(&path, perm);
                }
            }
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {
            clear_readonly(&self.0);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// The mode of this process when a test re-ran it as a child.
pub fn child_mode() -> Option<String> {
    std::env::var(CHILD_ENV).ok()
}

/// A command that re-runs this test executable's test `test` (exactly) in child mode `mode`.
pub fn child_command(test: &str, mode: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, mode);
    c
}

/// A child process that is killed and reaped when the guard drops, so a failed assertion or `unwrap` between the spawn
/// and the test's own wait never leaves a long-lived child behind (every process a test starts must finish). Derefs to
/// the `Child`.
pub struct KillOnDrop(std::process::Child);

impl KillOnDrop {
    pub fn new(child: std::process::Child) -> KillOnDrop {
        KillOnDrop(child)
    }
}

impl std::ops::Deref for KillOnDrop {
    type Target = std::process::Child;
    fn deref(&self) -> &std::process::Child {
        &self.0
    }
}

impl std::ops::DerefMut for KillOnDrop {
    fn deref_mut(&mut self) -> &mut std::process::Child {
        &mut self.0
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // Both results are ignored: the child may already have exited and been waited for.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits until `p` exists (a child's readiness marker), up to `ms`.
pub fn wait_for_file(p: &Path, ms: u64) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < u128::from(ms) {
        if p.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    p.exists()
}
