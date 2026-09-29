//! The candidates of a fuzzy-quote region ([F20 §6.4] "Candidates in a region R"), selected as the hits stream in.
//!
//! [F20 §6.4] takes E = {e : d(e) ≤ k}, then repeats: record the element of E least by (d(e), e), and remove from E
//! every e′ with |e′ − e| < len(exact). So a hit is recorded exactly when no recorded hit closer than len(exact) has a
//! smaller (d, e): one of a lower distance on either side, or one of the same distance to its left. Deciding the
//! distances in increasing order, and each distance left to right, gives the same set.
//!
//! [`Selector`] decides as the stream advances. A hit of distance 0 depends only on hits to its left, so it is decided
//! at once. A hit e of distance d also needs the recorded hits of lower distances up to e + len(exact) − 1, which are
//! decided once the stream is len(exact) − 1 bytes further on at each lower distance: the decision lags the stream by
//! at most d × (len(exact) − 1) bytes, [`Selector::lag`] for d = k. Memory does not grow with the region: the
//! undecided hits have distinct ends inside the lag, so there are at most lag + 1 of them, and each distance keeps
//! only the recorded ends within len(exact) of them, at most lag / len(exact) + 2 (recorded ends of one distance are
//! len(exact) apart). For a 128-byte quote and k = 32 that is about 5,200 ends.

use std::collections::VecDeque;

use crate::Hit;

/// The hits of one distance d: undecided ends, and recorded ends still needed to decide others (both increasing).
#[derive(Clone, Debug, Default)]
struct Level {
    d: usize,
    pending: VecDeque<usize>,
    recorded: VecDeque<usize>,
}

impl Level {
    /// Whether a recorded end lies within `width` of `e`, on either side.
    fn near(&self, e: usize, width: usize) -> bool {
        let at = self
            .recorded
            .partition_point(|&y| y.saturating_add(width) <= e);
        self.recorded
            .get(at)
            .is_some_and(|&y| y < e.saturating_add(width))
    }
}

/// A streaming selection of [F20 §6.4]'s candidates from the hits of one region ([`crate::Searcher`] reports them).
///
/// Hits enter with [`Selector::push`] in increasing order of end, as a searcher reports them; after each chunk the
/// caller calls [`Selector::advance`] with the searcher's position, and [`Selector::finish`] at the end of the region.
/// Each recorded hit is emitted once, as soon as it is decided, in the order of decision (not of end).
#[derive(Clone, Debug)]
pub struct Selector {
    width: usize,
    /// The greatest distance kept: k, or the width when that is smaller, since no hit is further from the quote than
    /// the empty substring, at `width`.
    top: usize,
    /// Every hit with an end below `next` has been pushed.
    next: usize,
    /// One level per distance seen so far, in increasing order of distance: memory follows the distances that
    /// occur, never their values.
    levels: Vec<Level>,
}

impl Selector {
    /// A selector for hits of distance at most `k` from a quote of `width` = len(exact) bytes. Hits closer than
    /// `width` suppress each other; with a width of 0 or 1 every hit is a candidate.
    #[must_use]
    pub fn new(width: usize, k: usize) -> Self {
        Self {
            width,
            top: k.min(width),
            next: 0,
            levels: Vec::new(),
        }
    }

    /// The most bytes by which a decision trails the position passed to [`Selector::advance`]: k × (width − 1). A
    /// caller that scores candidates as they are emitted keeps this much text before the position, plus the quote's
    /// length, k and the context.
    #[must_use]
    pub fn lag(&self) -> usize {
        self.top.saturating_mul(self.width.saturating_sub(1))
    }

    /// Ends held: undecided hits and recorded hits still needed.
    #[must_use]
    pub fn held(&self) -> usize {
        self.levels
            .iter()
            .map(|l| l.pending.len() + l.recorded.len())
            .sum()
    }

    /// Adds a hit, declaring every hit up to its end pushed, and emits the candidates that can now be decided. A hit
    /// whose end is not after every end pushed or declared before is ignored; one whose distance is over k (or over
    /// the width, which no hit of a quote of that width has) only declares its end.
    pub fn push(&mut self, hit: Hit, on_candidate: impl FnMut(Hit)) {
        if hit.end < self.next {
            return;
        }
        if hit.distance <= self.top {
            let at = self.levels.partition_point(|l| l.d < hit.distance);
            if self.levels.get(at).is_none_or(|l| l.d != hit.distance) {
                self.levels.insert(
                    at,
                    Level {
                        d: hit.distance,
                        ..Level::default()
                    },
                );
            }
            self.levels[at].pending.push_back(hit.end);
        }
        self.next = hit.end.saturating_add(1);
        self.decide(Some(hit.end), on_candidate);
    }

    /// Declares that every hit with an end at most `position` has been pushed (a searcher's
    /// [`crate::Searcher::position`] after a chunk), and emits the candidates that can now be decided.
    pub fn advance(&mut self, position: usize, on_candidate: impl FnMut(Hit)) {
        self.next = self.next.max(position.saturating_add(1));
        self.decide(Some(self.next - 1), on_candidate);
    }

    /// Ends the region: emits every remaining candidate and empties the selector for the next region.
    pub fn finish(&mut self, on_candidate: impl FnMut(Hit)) {
        self.decide(None, on_candidate);
        for l in &mut self.levels {
            l.pending.clear();
            l.recorded.clear();
        }
        self.next = 0;
    }

    /// Decides every hit whose lower distances are complete near it: at distance d, the ends up to
    /// `known − d × (width − 1)`, or all of them when `known` is `None`.
    fn decide(&mut self, known: Option<usize>, mut on_candidate: impl FnMut(Hit)) {
        let w = self.width;
        let step = w.saturating_sub(1);
        for n in 0..self.levels.len() {
            let frontier = match known {
                Some(k) => match k.checked_sub(self.levels[n].d.saturating_mul(step)) {
                    Some(f) => f,
                    None => break,
                },
                None => usize::MAX,
            };
            let (below, rest) = self.levels.split_at_mut(n);
            let level = &mut rest[0];
            while let Some(&e) = level.pending.front().filter(|&&e| e <= frontier) {
                level.pending.pop_front();
                let left = level
                    .recorded
                    .back()
                    .is_some_and(|&y| y.saturating_add(w) > e);
                if !left && !below.iter().any(|l| l.near(e, w)) {
                    level.recorded.push_back(e);
                    on_candidate(Hit {
                        end: e,
                        distance: level.d,
                    });
                }
            }
        }
        self.prune();
    }

    /// Drops recorded ends that no undecided or future hit can be near: every such hit of distance ≥ d ends at or
    /// after the least pending end of those distances, or at or after `next`.
    fn prune(&mut self) {
        let w = self.width;
        let mut least = self.next;
        for level in self.levels.iter_mut().rev() {
            if let Some(&e) = level.pending.front() {
                least = least.min(e);
            }
            while level
                .recorded
                .front()
                .is_some_and(|&y| y.saturating_add(w) <= least)
            {
                level.recorded.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests;
