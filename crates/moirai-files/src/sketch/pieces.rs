//! The split and merged predicates ([F20 §5.11.4] rows 2 and 5; §5.15 "split" and "merged"): an old content X found
//! in pieces across several candidates, or folded into one host.

use super::nat::Nat;
use crate::r14::{MERGED_NIO_MAX, MERGED_OIN, Ratio, SPLIT_MIN_PIECES, SPLIT_NIO, SPLIT_SUM};

// "c has at least 2 added candidates" ([F20 §5.11.4] row 2) needs no test of its own: the pieces are candidates, and
// there are at least `SPLIT_MIN_PIECES` of them.
const _: () = assert!(SPLIT_MIN_PIECES >= 2);

/// A candidate a of a split or merged test: its path and the containments `oin(X, a)` and `nio(X, a)` of the old
/// content X in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate<'a> {
    /// The candidate's root-relative path.
    pub path: &'a [u8],
    /// `oin(X, a)`, the share of X found in a.
    pub oin: Ratio,
    /// `nio(X, a)`, the share of a found in X.
    pub nio: Ratio,
}

/// The split test ([F20 §5.11.4] row 2; §5.15 "split"): at least `SPLIT_MIN_PIECES` candidates have
/// `nio(X, a) ≥ SPLIT_NIO`, and the `oin(X, a)` of those pieces sum to at least `SPLIT_SUM`.
///
/// Returns the pieces' indices into `candidates` in path order (bytewise, a proper prefix first; equal paths by
/// index), or `None` when the test fails. The sum is exact for every [`Ratio`]: it is kept as one fraction `N / D` of
/// natural numbers, and a piece `a / b` adds `a · (D / b)` when b divides D, else makes `N := N · b + a · D` and
/// `D := D · b`. Pieces that share a denominator, as the exact containments of one X do (`w(X)`), keep D at that
/// denominator. A denominator that does not divide D makes D grow by at most one limb, so n pieces with pairwise coprime
/// denominators cost O(n²) limb operations.
///
/// Left to the caller:
/// - the containments are the exact measures, or their estimates where the exact limit fails ([F20 §5.11.4]
///   "Containments"); in §5.15 they are the containments reported with the pair score;
/// - which entries are candidates: in E6, the added candidates of the commit's changes (same class, outside the exact
///   pairs and the ambiguous groups); in the similarity search, stage 2's list;
/// - the paths are root-relative and distinct.
#[must_use]
pub fn split(candidates: &[Candidate<'_>]) -> Option<Vec<usize>> {
    let mut pieces: Vec<usize> = (0..candidates.len())
        .filter(|&i| candidates[i].nio >= SPLIT_NIO)
        .collect();
    if (pieces.len() as u64) < u64::from(SPLIT_MIN_PIECES) {
        return None;
    }
    let mut num = Nat::from_u64(0);
    let mut den = Nat::from_u64(1);
    for &i in &pieces {
        let (a, b) = (candidates[i].oin.num(), candidates[i].oin.den());
        let (q, r) = den.div_rem_u64(b);
        let mut add = if r == 0 {
            // a / b = a · (D / b) / D.
            q
        } else {
            num.mul_u64(b);
            let d = den.clone();
            den.mul_u64(b);
            d
        };
        add.mul_u64(a);
        num.add(&add);
    }
    // N / D ≥ s / t ⟺ N · t ≥ s · D.
    num.mul_u64(SPLIT_SUM.den());
    den.mul_u64(SPLIT_SUM.num());
    if num < den {
        return None;
    }
    pieces.sort_by(|&i, &j| candidates[i].path.cmp(candidates[j].path).then(i.cmp(&j)));
    Some(pieces)
}

/// The merged test ([F20 §5.11.4] row 5; §5.15 "merged"): a candidate m with `oin(X, m) ≥ MERGED_OIN` and
/// `nio(X, m) < MERGED_NIO_MAX`, the first in path order (bytewise, a proper prefix first; equal paths by index) when
/// several qualify.
///
/// Returns m's index into `candidates`, or `None` when no candidate qualifies.
///
/// Left to the caller:
/// - the containments are the exact measures, or their estimates where the exact limit fails ([F20 §5.11.4]
///   "Containments"); in §5.15 they are the containments reported with the pair score;
/// - which entries are candidates: in E6, the host and added candidates of the commit's changes; in the similarity
///   search, stage 2's list;
/// - the paths are root-relative and distinct.
#[must_use]
pub fn merged(candidates: &[Candidate<'_>]) -> Option<usize> {
    (0..candidates.len())
        .filter(|&i| candidates[i].oin >= MERGED_OIN && candidates[i].nio < MERGED_NIO_MAX)
        .min_by(|&i, &j| candidates[i].path.cmp(candidates[j].path).then(i.cmp(&j)))
}
