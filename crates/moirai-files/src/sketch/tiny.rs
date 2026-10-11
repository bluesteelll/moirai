//! Tiny content and the tiny file node ([F20 §2.6.5]): the two forms that [`super::Fingerprint::is_tiny`] does not
//! cover.

use super::fingerprint::Fingerprint;
use crate::r14::{TINY_BYTES, TINY_LINES};
use crate::text::TextStats;

/// Whether the content X whose read gave `stats` is tiny ([F20 §2.6.5]).
///
/// Text is tiny when `nlines < TINY_LINES` or `nbytes < TINY_BYTES`, where `nbytes` is `len(norm(b))`. Binary content
/// is tiny when its raw length is below `TINY_BYTES`; its lines are never counted.
///
/// The test reads only `stats`, so it is defined for every content, a text of 4 GiB or more included, which has no
/// fingerprint. For a text with a fingerprint it equals [`Fingerprint::is_tiny`].
#[must_use]
pub fn content_is_tiny(stats: &TextStats) -> bool {
    if stats.is_text() {
        stats.nlines() < TINY_LINES || stats.norm_len() < TINY_BYTES
    } else {
        stats.len < TINY_BYTES
    }
}

/// Whether the file node F is tiny ([F20 §2.6.5]): the fingerprint of `last_oid` (else of `o`) exists and says so,
/// or no such fingerprint exists and `F.bytes < TINY_BYTES`.
///
/// - `reference` is that fingerprint, R. The caller picks it: the fingerprint of F's `last_oid` when F has one, else
///   the fingerprint of `o` ([F20 §5.4.1]). It is `None` when that content has no fingerprint (binary content, a text
///   of 4 GiB or more, or no stored value).
/// - `bytes` is `F.bytes`, read only when `reference` is `None`.
///
/// There is no store access here: the caller looks up both values.
#[must_use]
pub fn node_is_tiny(reference: Option<&Fingerprint>, bytes: u64) -> bool {
    match reference {
        Some(r) => r.is_tiny(),
        None => bytes < TINY_BYTES,
    }
}
