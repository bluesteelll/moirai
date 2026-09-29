//! E3 (PLAN WP-95 acceptance): every `fixtures/hex` fixture (the `.bin` that `cargo xtask hex` assembles from each
//! `.hex`) decodes and re-encodes byte-identically and yields the conclusion its comment states; every `fixtures/moi`
//! fixture parses, or is refused, as its `.expect` and its name say; every `fixtures/carrier` case's files parse and its
//! trailers re-derive the canonical items.
//!
//! Each family has one walk over all its fixtures. A fixture whose conclusion disagrees with the oracle's reading of the
//! specification is listed as a known mismatch: the walk skips it, and an ignored test of its own runs it
//! (`cargo test -p moirai-format-oracle --test fixtures -- --ignored`) until the ruling lands.

#[path = "fixtures/carrier.rs"]
mod carrier;
#[path = "fixtures/common.rs"]
mod common;
#[path = "fixtures/hex.rs"]
mod hex;
#[path = "fixtures/moi.rs"]
mod moi;
