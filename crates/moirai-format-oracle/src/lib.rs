//! The independent format oracle: a decoder and test-only re-encoder of every frozen structure, with hand-written
//! little-endian parsing, and the `.moi` and carrier ABNF conformance check (E3). Compressed payloads are opaque bytes
//! at M0.
//!
//! Test-only crate with no workspace dependency; checked by GT20 (e) on every target. Filled by WP-95 and WP-95b
//! (R-ORA), who is neither the fixture author nor the M1 codec author (S1). Sources: `docs/spec/format/`, [60 §3.1]
//! item 1; `docs/m0/PLAN.md` §2.2, §6.2 R3.
//!
//! The modules follow the chapters of `docs/spec/format/`:
//!
//! | module | chapter |
//! |---|---|
//! | [`prim`] | [F01] conventions: integers, varints, strings, hashes |
//! | [`lock`] | [F03] the `LOCK` file |
//! | [`head`] | [F04] the `HEAD` file |
//! | [`log`] | [F05] extents, records, groups, the chain, the scan and every record payload |
//! | [`commit`] | [F06] the `Commit` payload |
//! | [`canon`] | [F07] canonical items (the parts the carrier check re-derives) and the typed value `cv` |
//! | [`value`] | [F08] values, the field block, `NodeHdr`, schema items, edge props, anchors |
//! | [`segment`] | [F09] segments and their sections |
//! | [`sealed`] | [F10] sealed files |
//! | [`runtime`] | [F11] runtime-table rows and row images |
//! | [`image`] | [F14] the `.moi` ABNF, trees, git objects and carrier trailers |
//! | [`sha`] | [F01 §7.1] SHA-1 and SHA-256 for git object ids |
//! | [`fixture`] | the fixture checks: whole files, `HEAD` selection, log scans, fragments, the expectation framing |
//!
//! [`holes`] names the holed values the decoders use until WP-95b.

pub mod canon;
pub mod commit;
pub mod fixture;
pub mod head;
pub mod holes;
pub mod image;
pub mod lock;
pub mod log;
pub mod prim;
pub mod runtime;
pub mod sealed;
pub mod segment;
pub mod sha;
pub mod value;
