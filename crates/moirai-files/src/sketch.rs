//! Fingerprints of text content and the measures that compare two contents ([F20 §2.6], §2.9, §2.10.1, §2.10.2,
//! §2.10.5): the part of chapter 20 that lets the resolver tell whether a candidate file holds a recorded content.
//!
//! # The fingerprint
//!
//! [`FingerprintSink`] is the pass-1 hook of the streaming reader ([`crate::text::LineSink`], [F20 §2.4]): it
//! receives the lines of the anchor text and computes, in constant memory and in the same read as `oid`, the
//! quantities of [F20 §2.6.3] over the **fingerprint lines** `f = collapse(nl(l))` with `chars(f) > FP_MIN_CHARS`
//! ([F20 §2.6.1]): the weight, the bottom-`SKETCH_K` sketch of `sh(f) = low(XXH3-64(f), 32)` ([F20 §2.6.2]) and the
//! number of distinct values, estimated by k minimum values above `SKETCH_K`. [`FingerprintSink::finish`] gives the
//! [`Fingerprint`] of a text content of less than 4 GiB, with `nlines` and `nbytes` from the same read's statistics.
//!
//! Every sink of this module hashes a line the same way: one private hasher streams `collapse(nl(l))` over the
//! pieces the reader hands it, wherever a piece boundary falls.
//!
//! # Memory
//!
//! The fingerprint sink holds a fixed-size state and no heap memory: the line hasher, the weight, the 64 sketch values
//! and a flag. No structure of this module grows with the content in pass 1 (PLAN WP-62's 0.5 MB bound).
//!
//! # Arithmetic
//!
//! Integers only: no measure is a floating-point number ([F20 §1.2] "Rationals").

mod fingerprint;
mod line;

pub use fingerprint::{Fingerprint, FingerprintSink};
