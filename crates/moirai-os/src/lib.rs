//! The OS layer, the only crate that touches the operating system. At M0 it holds the Windows modules: `fs` (with
//! `free_space`), `lock`, `map`, `env`, `proc` (with `peak_of_child`), `mem` (`available_physical`, `CountingAlloc`),
//! the complete `project` (`ProjectFs`, read and write side), `path` with `canonical_root`, the `Meter`
//! implementation, and `test_host` behind the `test-host` feature. The Unix modules are configured out and export
//! nothing, so the crate type-checks empty on the Linux and macOS targets ([OS/README §2.2], [90 §11.1]).
//!
//! Product crate; only a composition root (`xtask/roots.toml`) may depend on it. It is the one crate allowed `unsafe`
//! code (FFI, mapping, the vectored exception handler, `CountingAlloc`), and every unsafe block carries a `SAFETY:`
//! comment. Filled by WP-33 (R-HARN-O). Sources: [80 §2.1–§2.7, §2.10–§2.11], [AR §14]; `docs/m0/PLAN.md` §2.1, §2.2,
//! §6.2 R1; `docs/spec/os/` (the OS-layer specification, cited as `[OS/<file> §x]`).
//!
//! # What the crate exports (Windows)
//!
//! | Item | Seam | Specification |
//! |---|---|---|
//! | [`OsVfs`] | `Vfs` = `StoreFs` + `Locks` + `SealedMaps` + `EnvGuard` + `Clock` + `ProcHost` + `Entropy` | [OS/README §4.1], [OS/fs], [OS/lock], [OS/map], [OS/env], [OS/clock], [OS/proc], [OS/README §4.6] |
//! | [`OsProjectFs`] | `ProjectFs` | [OS/project], [OS/path] |
//! | [`OsMeter`] | `Meter` | [OS/README §4.3], [OS/mem], [OS/proc §9] |
//! | [`CountingAlloc`] | the counting global allocator of probe roots | [OS/mem §6] |
//! | [`path`] | `canonical_root`, `canonical_abs`, `cli_path`, `representable_here`, `user_config_path` | [OS/path §11] |
//! | `test_host` (feature `test-host`) | `kill`, `suspend`, `resume`, the wall-clock offset | [OS/proc §13], [OS/clock §9] |
//!
//! Each implementation value is a small handle over process-global state ([OS/README §5.4]): the lock registry, the
//! mapping registry and the fault handler are shared by every value in the process. The handle types of the seams
//! (`OsRoot`, `OsFile`, `OsLockClient`, `OsMap`, `OsParentWatch`, `OsWake`, `OsProjectRoot`, `OsReader`) are exported
//! so that composition roots and probes can name them.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{
    CountingAlloc, OsFile, OsLockClient, OsMap, OsMeter, OsParentWatch, OsProjectFs, OsProjectRoot,
    OsReader, OsRoot, OsVfs, OsWake,
};

/// The OS-dependent path conversions of [OS/path §11] (Windows built from M0). Generic code reaches them through
/// `ProjectFs` ([OS/project §2.1]); composition roots and tools may call them directly.
#[cfg(windows)]
pub mod path {
    pub use crate::windows::path::{
        canonical_abs, canonical_root, cli_path, representable_here, user_config_path,
    };
}

/// Test-host controls ([OS/proc §13], [OS/clock §9]): only with the `test-host` feature, which the product root never
/// enables ([OS/README §2.4]).
#[cfg(all(windows, feature = "test-host"))]
pub use windows::test_host;
