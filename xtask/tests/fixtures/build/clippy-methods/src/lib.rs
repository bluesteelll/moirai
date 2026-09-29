//! Seeded clippy case (GT20 (d), the type-aware layer): calls the syntactic scan cannot tell from their namesakes.

use std::fs::File;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

/// The `File` lock calls in method form.
pub fn store_file(f: &File) -> std::io::Result<()> {
    f.lock()?;
    let _ = f.try_lock();
    f.unlock()
}

/// A `Path` method that does file-system I/O.
pub fn path_io(p: &Path) -> bool {
    p.exists()
}

/// Namesakes that stay allowed on every side of the boundary.
pub fn namesakes(m: &Mutex<u8>, p: &Path) -> (u8, bool) {
    let v = *m.lock().unwrap_or_else(PoisonError::into_inner);
    (v, p.is_absolute())
}
