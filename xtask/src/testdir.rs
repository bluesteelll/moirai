//! A scratch directory for unit tests: made under the system temporary directory with a name unique to the test and
//! the process, and removed when dropped, so a failed assertion leaves nothing behind.

use std::path::{Path, PathBuf};

pub struct TestDir(PathBuf);

impl TestDir {
    /// An empty directory `moirai-xtask-<name>-<pid>`, replacing what a crashed earlier run left.
    pub fn new(name: &str) -> TestDir {
        let dir = std::env::temp_dir().join(format!("moirai-xtask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TestDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `content` at `rel`, making its parent directories.
    pub fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let p = self.0.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, content).unwrap();
        p
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
