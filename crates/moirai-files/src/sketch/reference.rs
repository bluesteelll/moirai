//! The content conditions of the `replaced` test ([F20 §5.4.1]) and of E8 ([F20 §5.13]): both compare the
//! reference fingerprint R of a file node with a content through the sketch estimates, even when the old content is
//! readable ([F20 §2.10.2]).

use super::fingerprint::Fingerprint;
use super::measure::{Estimates, estimates};
use crate::r14::{E8_MIN, REPLACED_MAX, REPLACED_MIN_LINES};

/// The reference fingerprint R of a file node, the fingerprint of a content (p's in `replaced`, q's in E8) and the
/// sketch estimates of the pair, `estimates(R, hit, content)` ([F20 §2.10.2]).
///
/// R is the fingerprint of the node's `last_oid` if present, else of `o` ([F20 §5.4.1], §5.13). Both fingerprints
/// exist, so both contents are text of less than 4 GiB. [`RefEstimates::new`] computes `est`. The field is public so
/// that a caller holding the estimates already, or a test, can set them.
#[derive(Clone, Copy, Debug)]
pub struct RefEstimates<'a> {
    /// R.
    pub reference: &'a Fingerprint,
    /// The fingerprint of the content compared with R.
    pub content: &'a Fingerprint,
    /// `eoin(R, content)`, `enio(R, content)` and `esym(R, content)`.
    pub est: Estimates,
}

impl<'a> RefEstimates<'a> {
    /// The pair with `est = estimates(reference, hit, content)`, where `hit` is the [`super::HitSink`] count of R's
    /// sketch over the content.
    #[must_use]
    pub fn new(reference: &'a Fingerprint, hit: u32, content: &'a Fingerprint) -> RefEstimates<'a> {
        RefEstimates {
            reference,
            content,
            est: estimates(reference, hit, content),
        }
    }
}

/// The content conditions of the `replaced` test ([F20 §5.4.1], bullets 4 and 5): R exists, R and p's content are
/// both text with `nlines ≥ REPLACED_MIN_LINES`, and `eoin(R, p) < REPLACED_MAX` and `enio(R, p) < REPLACED_MAX`.
///
/// `pair` is `None` when R does not exist, or when p's content has no fingerprint (binary content, or a text of
/// 4 GiB or more); the test is then false.
///
/// Left to the caller, which must check them as well before p is `replaced`: the test runs at a settle;
/// `oid(p) ∉ {o, last_oid}`; p's file id is known and differs from `FILEOBS.file_id` ([F20 §5.3]); and F's
/// `artifact_kind` is not `generated`.
#[must_use]
pub fn replaced_content(pair: Option<&RefEstimates<'_>>) -> bool {
    let Some(p) = pair else {
        return false;
    };
    u64::from(p.reference.nlines()) >= REPLACED_MIN_LINES
        && u64::from(p.content.nlines()) >= REPLACED_MIN_LINES
        && p.est.eoin < REPLACED_MAX
        && p.est.enio < REPLACED_MAX
}

/// The content conditions of E8 ([F20 §5.13]): q is text, R exists, `eoin(R, q) ≥ E8_MIN` and `enio(R, q) ≥ E8_MIN`,
/// and F is not tiny.
///
/// `pair` is `None` when R does not exist ("without R, E8 yields nothing"), or when q's content has no fingerprint;
/// the test is then false. "Not for tiny F" is `!R.is_tiny()`: with R present, [F20 §2.6.5] makes F tiny exactly
/// when R says so.
///
/// Left to the caller, which must check them as well before q is `strong`: a settle that is not the tree's first; q
/// passes [F20 §4]; `basename(q) = basename(p)` bytewise; and E7's time predicate ([F20 §5.12]).
#[must_use]
pub fn e8_content(pair: Option<&RefEstimates<'_>>) -> bool {
    let Some(p) = pair else {
        return false;
    };
    !p.reference.is_tiny() && p.est.eoin >= E8_MIN && p.est.enio >= E8_MIN
}
