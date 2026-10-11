//! The sketch resemblance `r` ([F20 §2.10.3]) and the pair score with its reported containments ([F20 §2.10.4]).

use core::cmp::{Ordering, max};

use super::fingerprint::Fingerprint;
use super::measure::{Estimates, LineMultiset, exact};
use super::winnow::{WinnowSet, jaccard};
use crate::r14::{Ratio, SKETCH_K};

/// `r(A, B)`, the bottom-k resemblance estimator of two fingerprints ([F20 §2.10.3]): X is the
/// `min(SKETCH_K, |S_A ∪ S_B|)` smallest values of `S_A ∪ S_B`, and `r = |X ∩ S_A ∩ S_B| / |X|`, 0 when X is empty.
///
/// One merge walk over the two strictly ascending sketches: it stops after `SKETCH_K` values of the union, so X is
/// never the whole union of two full sketches.
#[must_use]
pub fn resemblance(a: &Fingerprint, b: &Fingerprint) -> Ratio {
    let (sa, sb) = (a.sketch(), b.sketch());
    let (mut i, mut j) = (0, 0);
    // `|X|` and `|X ∩ S_A ∩ S_B|`.
    let (mut x, mut both) = (0u64, 0u64);
    while x < SKETCH_K as u64 {
        match (sa.get(i), sb.get(j)) {
            (None, None) => break,
            (Some(_), None) => i += 1,
            (None, Some(_)) => j += 1,
            (Some(va), Some(vb)) => match va.cmp(vb) {
                Ordering::Less => i += 1,
                Ordering::Greater => j += 1,
                Ordering::Equal => {
                    both += 1;
                    i += 1;
                    j += 1;
                }
            },
        }
        x += 1;
    }
    Ratio::or_zero(both, x)
}

/// Which measures a [`PairScore`] was computed from ([F20 §2.10.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    /// Both contents were read and the exact limit holds: `max(sym, J)` (or `sym` when J is not defined), with the
    /// exact containments `oin` and `nio` ([F20 §2.10.1], §2.10.5).
    Exact,
    /// The old content is known only by its fingerprint, or the exact limit fails: `esym`, with the containment
    /// estimates `eoin` and `enio` ([F20 §2.10.2]).
    Estimate,
}

/// The pair score of an old content A and a new content B, and the containments reported with it
/// ([F20 §2.10.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairScore {
    /// The score: `max(sym, J)` or `sym` on the exact basis, `esym` on the estimate basis.
    pub score: Ratio,
    /// The old-in-new containment reported with the score: `oin(A, B)` when exact, else `eoin(A, B)`.
    pub oin: Ratio,
    /// The new-in-old containment reported with the score: `nio(A, B)` when exact, else `enio(A, B)`.
    pub nio: Ratio,
    /// The measures the score was computed from.
    pub basis: Basis,
}

/// The pair score of an old content A and a new content B, with its containments ([F20 §2.10.4]).
///
/// - `lines` is `(M(A), M(B))` ([`super::MultisetSink`]). The caller passes it exactly when both contents were read
///   and each side's multiset exists, that is when the exact limit holds ([F20 §2.10.5]). The basis is then
///   [`Basis::Exact`]: `score = max(sym, J)` when `winnow` is `Some`, else `score = sym`, and the containments are
///   [`exact`]'s `oin` and `nio`.
/// - `winnow` is `(FW(A), FW(B))` ([`super::WinnowSink`]). The caller passes it when both winnowing sets exist, that
///   is when J is defined ([F20 §2.9], §2.10.5). It is read only on the exact basis, so J is never used beyond the
///   exact limit.
/// - `estimates` is `estimates(R, hit, fp(B))` ([`super::estimates`]), with R the old content's fingerprint. When
///   `lines` is `None` (A known only by its fingerprint, or the exact limit fails) the basis is
///   [`Basis::Estimate`]: `score = esym`, and the containments are `eoin` and `enio`. On the exact basis it is not
///   read.
///
/// Left to the caller: reading the contents, deciding which side is old, and checking that both are text.
#[must_use]
pub fn pair_score(
    lines: Option<(&LineMultiset, &LineMultiset)>,
    winnow: Option<(&WinnowSet, &WinnowSet)>,
    estimates: &Estimates,
) -> PairScore {
    match lines {
        Some((old, new)) => {
            let e = exact(old, new);
            let score = match winnow {
                Some((fa, fb)) => max(e.sym, jaccard(fa, fb)),
                None => e.sym,
            };
            PairScore {
                score,
                oin: e.oin,
                nio: e.nio,
                basis: Basis::Exact,
            }
        }
        None => PairScore {
            score: estimates.esym,
            oin: estimates.eoin,
            nio: estimates.enio,
            basis: Basis::Estimate,
        },
    }
}
