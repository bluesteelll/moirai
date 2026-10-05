//! A test's scratch directory under the target directory's temporary directory (`CARGO_TARGET_TMPDIR`): scratch
//! repositories and generated files live only there.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A scratch directory `CARGO_TARGET_TMPDIR/moirai-replay/<name>-<pid>-<n>`, removed with everything in it when
/// dropped, so a failed assertion leaves nothing behind.
pub struct Scratch(PathBuf);

impl Scratch {
    /// An empty directory named after the test.
    pub fn new(name: &str) -> Scratch {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("moirai-replay")
            .join(format!(
                "{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory can be made");
        Scratch(dir)
    }

    /// Its path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
