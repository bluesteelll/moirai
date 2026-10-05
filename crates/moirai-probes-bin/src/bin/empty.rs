//! `empty`: the empty Rust executable whose spawn-to-exit time is measurement 11's floor ([60 §5.2] row 11, §5.3), and
//! whose peak private bytes are the floor of the floor-relative RSS form ([80 §2.9], [OS/mem §8]). It is measured
//! interleaved with the operation it floors ([MP §4.3], [MP §4.6], [MP §4.9]; `docs/spec/measurement-protocol.md`). It
//! does nothing, by definition.
//!
//! `moirai-probes-bin` is a test-only composition root, Windows only (`xtask/roots.toml`): it holds only
//! `src/bin/*.rs` wiring and is checked with
//! `cargo check -p moirai-probes-bin --locked --target x86_64-pc-windows-msvc`. Its binaries are R-HARN's (WP-50 and
//! the measurement work packages). Sources: `docs/m0/PLAN.md` §2.1, §2.2, §6.2 R17.

fn main() {}
