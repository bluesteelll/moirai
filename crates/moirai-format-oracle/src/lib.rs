//! The independent format oracle: a decoder and test-only re-encoder of every frozen structure, with hand-written
//! little-endian parsing, and the `.moi` and carrier ABNF conformance check (E3). Compressed payloads are opaque bytes
//! at M0.
//!
//! Test-only crate with no workspace dependency; checked by GT20 (e) on every target. Filled by WP-95 and WP-95b
//! (R-ORA), who is neither the fixture author nor the M1 codec author (S1). Sources: `docs/spec/format/`, [60 §3.1]
//! item 1; `docs/m0/PLAN.md` §2.2, §6.2 R3.
