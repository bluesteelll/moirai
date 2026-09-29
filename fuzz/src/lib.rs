//! Support code shared by moirai's libFuzzer targets (docs/m0/PLAN.md §2.2 `fuzz/`; docs/m0/tools.md §4).
//!
//! Host-only, in the separate `fuzz/` workspace under its pinned nightly; nothing here enters a checked graph. The
//! targets themselves live in `fuzz_targets/` and belong to FL-1's work packages (WP-65, WP-67; docs/m0/authors.md
//! §3). This library is fallback A of tools.md §4.4 for libFuzzer on `x86_64-pc-windows-msvc` with the sanitizer off:
//! - `build.rs` links the SanitizerCoverage section bounds of `src/sancov_sections.c` into every target (failure 1:
//!   the link error);
//! - [`record`] keeps each input and installs a panic hook and an allocation-error hook that write the failing input
//!   where libFuzzer would (failure 2: a Rust panic or allocation failure aborts with `__fastfail`, which leaves no
//!   artifact).
//!
//! Every target calls [`record`] first, and the gate's `fuzz` step refuses a target that does not:
//!
//! ```ignore
//! #![no_main]
//! use libfuzzer_sys::fuzz_target;
//!
//! fuzz_target!(|data: &[u8]| {
//!     moirai_fuzz::record(data);
//!     // the code under test
//! });
//! ```
//!
//! The allocation-error hook uses the nightly feature `alloc_error_hook` (tracking issue rust-lang/rust#51245), which
//! the pinned nightly of `rust-toolchain.toml` has; a toolchain move re-checks it (tools.md §2.2). The feature also
//! makes a build under the root's stable toolchain fail loudly instead of silently bypassing the pin (tools.md §4.2).
//!
//! The library target also lets the manifest resolve before the first fuzz target exists, so `fuzz/Cargo.lock` is
//! kept and scanned from the start (tools.md §4.1).

#![feature(alloc_error_hook)]

mod artifact;

pub use artifact::{Dest, record, sha1_hex};
