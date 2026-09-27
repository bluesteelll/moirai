//! The tree-sitter-rust oracle for FL-1's Rust scope scanner: it emits scope items as JSON for the differential in
//! `moirai-replay`.
//!
//! Host-only crate (`xtask/host-only.toml`): tree-sitter compiles C, so the crate stays outside every checked graph,
//! is excluded from GT20 (e) and is built only by the replay job, unpoisoned. No crate depends on it. Filled by WP-74
//! (R-REPLAY). Sources: [90 §11.1]; `docs/m0/PLAN.md` §2.1, §2.2, §6.2 R7.
