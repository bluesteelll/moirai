//! The OS layer, the only crate that touches the operating system. At M0 it holds the Windows modules: `fs` (with
//! `free_space`), `lock`, `map`, `env`, `proc` (with `peak_of_child`), `mem` (`available_physical`, `CountingAlloc`),
//! the complete `project` (`ProjectFs`, read and write side), `path` with `canonical_root`, the `Meter`
//! implementation, and `test_host` behind a test feature. The Unix modules are configured out and export nothing, so
//! the crate type-checks empty on the Linux and macOS targets.
//!
//! Product crate; only a composition root (`xtask/roots.toml`) may depend on it. It is the one crate allowed `unsafe`
//! code (FFI, mapping, the vectored exception handler, `CountingAlloc`), and every unsafe block carries a `SAFETY:`
//! comment. Filled by WP-33 (R-HARN-O). Sources: [80 §2.1–§2.7, §2.10–§2.11], [AR §14]; `docs/m0/PLAN.md` §2.1, §2.2,
//! §6.2 R1.
