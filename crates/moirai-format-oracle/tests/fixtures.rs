//! E3 (PLAN WP-95 acceptance): every `fixtures/hex` fixture (the `.bin` that `cargo xtask hex` assembles from each
//! `.hex`) decodes and re-encodes byte-identically and yields the conclusion its comment states; every `fixtures/moi`
//! fixture parses, or is refused, as its `.expect` and its name say; every `fixtures/carrier` case's files parse and its
//! trailers re-derive the canonical items; every `fixtures/canonical` case's ids are the BLAKE3-256 of its inputs.
//!
//! Each family has one walk over all its fixtures; no test is ignored. A fixture whose conclusion disagrees with the
//! oracle's reading of the specification is listed in the walk's `KNOWN` and reported as a spec finding until the ruling
//! lands. The walk still runs it, as an expected failure: a known mismatch that passes, or that names no fixture, fails
//! the walk, so a stale entry cannot hide a fixture.

#[path = "fixtures/canonical.rs"]
mod canonical;
#[path = "fixtures/carrier.rs"]
mod carrier;
#[path = "fixtures/common.rs"]
mod common;
#[path = "fixtures/hex.rs"]
mod hex;
#[path = "fixtures/moi.rs"]
mod moi;
