//! The similarity classification of [F20 §5.15]: the class of the old content X of a file node among the candidates
//! of the similarity search, from their pair scores, their containments and two path corroborations.

use super::nat::Nat;
use super::pieces::{Candidate, merged, split};
use super::score::PairScore;
use crate::r14::{Ratio, SIM_MARGIN, SIM_STRONG, SIM_WEAK};

/// A candidate q of the similarity search with its pair score against X ([F20 §2.10.4]) and the two corroborations of
/// a strong candidate ([F20 §5.15]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scored<'a> {
    /// q's root-relative path.
    pub path: &'a [u8],
    /// The pair score of X and q, with the containments reported with it.
    pub score: PairScore,
    /// `dirname(q) = dirname(p)`, bytewise.
    pub same_dir: bool,
    /// `basename(q) = basename(p)`, bytewise.
    pub same_base: bool,
}

/// The class of [F20 §5.15], with the candidates it names as indices into the list given to [`classify`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Similarity {
    /// split: `moved-needs-confirm (split)`, the pieces in path order.
    Split(Vec<usize>),
    /// strong: `moved-needs-confirm (similar …)` to this candidate.
    Strong(usize),
    /// merged: `moved-needs-confirm (merged)` into this host.
    Merged(usize),
    /// weak, one candidate with a score of at least `SIM_WEAK`: `moved-needs-confirm` to it.
    Weak(usize),
    /// weak, two or more candidates with a score of at least `SIM_WEAK`: `ambiguous`, every such candidate in
    /// (score descending, path ascending) order.
    Ambiguous(Vec<usize>),
    /// none, no score of at least `SIM_WEAK`: `missing`.
    Missing,
}

/// The similarity class of the candidates ([F20 §5.15]; RULINGS R-13 for the order of the rows and the weak row).
///
/// The candidates are ordered by (score descending, path ascending, index) ([F20 §4.1]). The best is the first, and
/// the runner-up's score is the second's, or 0 for one candidate. The rows apply in [F20 §5.11.4]'s order, and the
/// first that holds decides:
/// 1. split: [`split`] over the candidates' reported containments;
/// 2. strong: the best score is at least `SIM_STRONG`, the best less the runner-up's is at least `SIM_MARGIN`, and the
///    best has `same_dir` or `same_base`; equal top scores have a margin of 0, so a tie is never strong;
/// 3. merged: [`merged`] over the same containments;
/// 4. weak, the residual class: one candidate with a score of at least `SIM_WEAK` gives [`Similarity::Weak`], two or
///    more give [`Similarity::Ambiguous`]; a best of at least `SIM_STRONG` with the margin but no corroboration is
///    therefore weak;
/// 5. none: [`Similarity::Missing`], exactly when no row above held and no score is at least `SIM_WEAK`.
///
/// \[F20 §5.15\] states no row order; R-13 rules [F20 §5.11.4]'s, so a split is never strong to its largest piece,
/// and rules weak the residual class. The result is a proposal, never an automatic re-bind. The margin is exact for
/// every [`Ratio`].
///
/// Left to the caller: the candidate list (stage 2's, [F20 §5.14]); the bytewise `dirname` and `basename`
/// equalities; cutting an `ambiguous` listing to `LIST_MAX` ([F20 §4.1]); and the state strings (\[F18\]). The
/// paths are root-relative and distinct.
#[must_use]
pub fn classify(candidates: &[Scored<'_>]) -> Similarity {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|&i, &j| {
        let (a, b) = (&candidates[i], &candidates[j]);
        b.score
            .score
            .cmp(&a.score.score)
            .then_with(|| a.path.cmp(b.path))
            .then(i.cmp(&j))
    });
    let Some(&best) = order.first() else {
        return Similarity::Missing;
    };
    let projected: Vec<Candidate<'_>> = candidates
        .iter()
        .map(|c| Candidate {
            path: c.path,
            oin: c.score.oin,
            nio: c.score.nio,
        })
        .collect();
    if let Some(pieces) = split(&projected) {
        return Similarity::Split(pieces);
    }
    let b = &candidates[best];
    let runner_up = order
        .get(1)
        .map_or(Ratio::ZERO, |&i| candidates[i].score.score);
    if b.score.score >= SIM_STRONG
        && margin_at_least(b.score.score, runner_up, SIM_MARGIN)
        && (b.same_dir || b.same_base)
    {
        return Similarity::Strong(best);
    }
    if let Some(m) = merged(&projected) {
        return Similarity::Merged(m);
    }
    let weak: Vec<usize> = order
        .into_iter()
        .filter(|&i| candidates[i].score.score >= SIM_WEAK)
        .collect();
    match weak.as_slice() {
        [] => Similarity::Missing,
        &[one] => Similarity::Weak(one),
        _ => Similarity::Ambiguous(weak),
    }
}

/// `best − runner_up ≥ margin`, exactly: for `a/b`, `c/d` and `e/f`, `a·d·f ≥ (c·f + e·d)·b`, with no subtraction.
fn margin_at_least(best: Ratio, runner_up: Ratio, margin: Ratio) -> bool {
    let (a, b) = (best.num(), best.den());
    let (c, d) = (runner_up.num(), runner_up.den());
    let (e, f) = (margin.num(), margin.den());
    let mut lhs = Nat::from_u64(a);
    lhs.mul_u64(d);
    lhs.mul_u64(f);
    let mut rhs = Nat::from_u64(c);
    rhs.mul_u64(f);
    let mut ed = Nat::from_u64(e);
    ed.mul_u64(d);
    rhs.add(&ed);
    rhs.mul_u64(b);
    lhs >= rhs
}
