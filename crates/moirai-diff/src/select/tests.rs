//! The selector against [F20 §6.4]'s greedy, written here by definition: every hit of E sorted by (d(e), e), each
//! recorded unless a recorded one lies closer than len(exact).

use super::Selector;
use crate::{Hit, Pattern};
use proptest::prelude::*;

/// [F20 §6.4] by definition, over all hits of a region (no hit is further than `width` from a quote that long).
fn naive(hits: &[Hit], width: usize, k: usize) -> Vec<Hit> {
    assert!(hits.iter().all(|h| h.distance <= width));
    let mut e: Vec<Hit> = hits.iter().copied().filter(|h| h.distance <= k).collect();
    e.sort_by_key(|h| (h.distance, h.end));
    let mut recorded: Vec<Hit> = Vec::new();
    for h in e {
        if recorded.iter().all(|r| r.end.abs_diff(h.end) >= width) {
            recorded.push(h);
        }
    }
    recorded.sort_by_key(|h| h.end);
    recorded
}

/// The most ends a selector may hold ([`super`]'s bound).
fn held_bound(sel: &Selector, width: usize, k: usize) -> usize {
    let levels = k.min(width) + 1;
    sel.lag() + 1 + levels * (sel.lag() / width.max(1) + 2)
}

#[test]
fn levels_follow_the_distances_seen() {
    let mut sel = Selector::new(1 << 20, 1 << 18);
    assert_eq!(sel.lag(), (1 << 18) * ((1 << 20) - 1));
    sel.push(h(5, 3), |_| {});
    assert_eq!(sel.levels.len(), 1);
    sel.push(h(6, 1), |_| {});
    sel.push(h(7, 3), |_| {});
    assert_eq!(sel.levels.iter().map(|l| l.d).collect::<Vec<_>>(), [1, 3]);
}

#[test]
fn extreme_distances_hold_one_level_each() {
    // The largest width and budget: distances up to usize::MAX are kept, one level per distance seen, and the lag
    // saturates. Nothing overflows or allocates in proportion to a distance.
    let mut sel = Selector::new(usize::MAX, usize::MAX);
    assert_eq!(sel.lag(), usize::MAX);
    let mut out = Vec::new();
    sel.push(h(10, usize::MAX), |c| out.push(c));
    sel.push(h(11, usize::MAX - 1), |c| out.push(c));
    sel.push(h(12, 0), |c| out.push(c));
    sel.advance(usize::MAX, |c| out.push(c));
    assert_eq!(sel.levels.len(), 3);
    // Width usize::MAX: the distance-0 hit at 12 suppresses the others, which are decided only at the end.
    assert_eq!(out, [h(12, 0)]);
    sel.finish(|c| out.push(c));
    assert_eq!(out, [h(12, 0)]);
    assert_eq!(sel.held(), 0);
    // A small width with the largest budget: the budget stops at the width.
    let mut sel = Selector::new(3, usize::MAX);
    sel.push(h(4, usize::MAX), |c| out.push(c));
    sel.push(h(5, 3), |c| out.push(c));
    sel.finish(|c| out.push(c));
    assert_eq!(out, [h(12, 0), h(5, 3)]);
    assert_eq!(sel.levels.len(), 1);
}

/// Streams `hits` through a selector, declaring a position after every hit whose index is in `cuts`; checks the lag
/// and the memory bound on the way. Returns the candidates in end order.
fn streamed(hits: &[Hit], width: usize, k: usize, cuts: &[usize]) -> Vec<Hit> {
    let mut sel = Selector::new(width, k);
    let lag = sel.lag();
    let bound = held_bound(&sel, width, k);
    let mut out: Vec<Hit> = Vec::new();
    let mut position = 0usize;
    for (n, &h) in hits.iter().enumerate() {
        position = position.max(h.end);
        sel.push(h, |c| out.push(c));
        if cuts.contains(&n) {
            // A position before the next hit: every hit up to it has been pushed.
            let before_next = hits.get(n + 1).map_or(usize::MAX, |x| x.end - 1);
            position = (position + n % 5).min(before_next);
            sel.advance(position, |c| out.push(c));
        }
        // Every hit that ends `lag` bytes or more before the position is decided (d × (width - 1) at distance d).
        for level in &sel.levels {
            let d = level.d;
            let wait = d * width.saturating_sub(1);
            assert!(wait <= lag);
            assert!(
                level.pending.iter().all(|&e| e + wait > position),
                "distance {d} at {position}: {:?}",
                level.pending
            );
        }
        assert!(sel.held() <= bound, "held {} > {bound}", sel.held());
    }
    sel.finish(|c| out.push(c));
    assert_eq!(sel.held(), 0);
    let emitted = out.len();
    out.sort_by_key(|h| h.end);
    out.dedup();
    assert_eq!(out.len(), emitted, "a candidate was emitted twice");
    out
}

/// Hits with increasing ends from gaps, and distances at most `width`.
fn hits_of(gaps: &[(usize, usize)], width: usize) -> Vec<Hit> {
    let mut end = 0;
    gaps.iter()
        .enumerate()
        .map(|(n, &(gap, d))| {
            end += if n == 0 { gap } else { gap + 1 };
            Hit {
                end,
                distance: d % (width + 1),
            }
        })
        .collect()
}

fn h(end: usize, distance: usize) -> Hit {
    Hit { end, distance }
}

#[test]
fn known_selections() {
    // Width 4: 20 (d 0) removes 23; 8 (d 1) comes before 10, and 26 stands 6 from 20; 12 (d 2) stands 4 from 8.
    let hits = [h(8, 1), h(10, 1), h(12, 2), h(20, 0), h(23, 0), h(26, 1)];
    let want = [h(8, 1), h(12, 2), h(20, 0), h(26, 1)];
    assert_eq!(naive(&hits, 4, 2), want);
    assert_eq!(streamed(&hits, 4, 2, &[]), want);
    assert_eq!(streamed(&hits, 4, 2, &[0, 1, 2, 3, 4, 5]), want);
    // A lower distance to the right suppresses: 22 (d 0) removes 20 (d 1).
    assert_eq!(streamed(&[h(20, 1), h(22, 0)], 4, 1, &[0]), [h(22, 0)]);
    // k = 1 drops the distance-2 hit.
    assert_eq!(streamed(&hits, 4, 1, &[2]), [h(8, 1), h(20, 0), h(26, 1)]);
    // Widths 0 and 1 suppress nothing; a quote that short has hits of distance at most 0 or 1.
    let short = [h(3, 0), h(4, 1), h(5, 0), h(6, 1)];
    assert_eq!(streamed(&short, 1, 2, &[]), short);
    assert_eq!(streamed(&short[..1], 0, 2, &[]), short[..1]);
    // A distance over the width only declares its end.
    let mut sel = Selector::new(1, 3);
    let mut out = Vec::new();
    sel.push(h(3, 1), |c| out.push(c));
    sel.push(h(9, 2), |c| out.push(c));
    sel.push(h(8, 0), |c| out.push(c));
    sel.finish(|c| out.push(c));
    assert_eq!(out, [h(3, 1)]);
    assert_eq!(Selector::new(4, 2).lag(), 6);
    assert_eq!(Selector::new(0, 9).lag(), 0);
    // Levels stop at the width: no hit is further than the quote's length.
    assert_eq!(Selector::new(3, 1000).lag(), 6);
    assert_eq!(Selector::new(usize::MAX, 2).lag(), usize::MAX);
    assert_eq!(Selector::new(1 << 20, 2).lag(), 2 * ((1 << 20) - 1));
}

#[test]
fn decisions_wait_for_lower_distances() {
    // Width 5: a distance-2 hit at 10 is decided only when the position reaches 10 + 2 × 4 = 18, since a hit of
    // distance 1 ending at 14 or a hit of distance 0 ending up to 14 could still suppress it.
    let mut sel = Selector::new(5, 2);
    let mut out = Vec::new();
    sel.push(h(10, 2), |c| out.push(c));
    sel.advance(17, |c| out.push(c));
    assert!(out.is_empty());
    assert_eq!(sel.held(), 1);
    sel.advance(18, |c| out.push(c));
    assert_eq!(out, [h(10, 2)]);
    // Declaring an earlier position changes nothing.
    sel.advance(3, |c| out.push(c));
    sel.push(h(18, 0), |c| out.push(c));
    assert_eq!(out, [h(10, 2)]);
    sel.push(h(19, 0), |c| out.push(c));
    assert_eq!(out, [h(10, 2), h(19, 0)]);
    sel.finish(|c| out.push(c));
    assert_eq!(out, [h(10, 2), h(19, 0)]);
}

#[test]
fn out_of_order_and_over_budget_hits_are_ignored() {
    let mut sel = Selector::new(3, 1);
    let mut out = Vec::new();
    sel.push(h(5, 1), |c| out.push(c));
    sel.push(h(5, 0), |c| out.push(c));
    sel.push(h(4, 0), |c| out.push(c));
    sel.push(h(6, 2), |c| out.push(c));
    sel.advance(3, |c| out.push(c));
    sel.push(h(7, 0), |c| out.push(c));
    sel.finish(|c| out.push(c));
    assert_eq!(out, [h(7, 0)]);
    // Finishing empties the selector for the next region, whose ends start again from 0.
    sel.push(h(0, 1), |c| out.push(c));
    sel.finish(|c| out.push(c));
    assert_eq!(out, [h(7, 0), h(0, 1)]);
}

/// A text of 'a' with a 'b' at every seventh byte on average (xorshift).
fn mostly_a(len: usize) -> Vec<u8> {
    let mut x = 7u64;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if x.is_multiple_of(7) { b'b' } else { b'a' }
        })
        .collect()
}

#[test]
fn a_hit_at_every_offset_holds_little() {
    // A quote of 128 'a' in 64 KiB of 'a' (every end from 128 on is exact), and in a text of 'a' and 'b' where many
    // distances occur.
    let quote = vec![b'a'; 128];
    let p = Pattern::new(&quote);
    let k = 32;
    for text in [vec![b'a'; 1 << 16], mostly_a(1 << 16)] {
        let mut all = Vec::new();
        let mut sel = Selector::new(quote.len(), k);
        let bound = held_bound(&sel, quote.len(), k);
        let mut out = Vec::new();
        let mut s = p.searcher(k);
        let mut most = 0;
        for chunk in text.chunks(4096) {
            s.feed(chunk, |hit| {
                all.push(hit);
                sel.push(hit, |c| out.push(c));
                most = most.max(sel.held());
            });
            sel.advance(s.position(), |c| out.push(c));
        }
        sel.finish(|c| out.push(c));
        assert!(most <= bound, "{most} > {bound}");
        assert!(all.len() > 1000);
        out.sort_by_key(|c| c.end);
        assert_eq!(out, naive(&all, quote.len(), k));
    }
}

proptest! {
    #![proptest_config(crate::test_config(256))]

    #[test]
    fn equals_the_greedy(
        gaps in prop::collection::vec((0usize..6, 0usize..5), 0..120),
        width in 0usize..8,
        k in 0usize..5,
        cuts in prop::collection::vec(0usize..120, 0..20),
    ) {
        let hits = hits_of(&gaps, width);
        prop_assert_eq!(streamed(&hits, width, k, &cuts), naive(&hits, width, k));
    }

    #[test]
    fn equals_the_greedy_on_searched_text(
        pat in prop::collection::vec(prop_oneof![Just(b'a'), Just(b'b'), Just(b'c')], 1..24),
        text in prop::collection::vec(prop_oneof![Just(b'a'), Just(b'b'), Just(b'c')], 0..400),
        frac in 0usize..5,
        chunk in 1usize..50,
    ) {
        let k = pat.len() * frac / 4;
        let p = Pattern::new(&pat);
        let mut all = Vec::new();
        let mut sel = Selector::new(pat.len(), k);
        let mut out = Vec::new();
        let mut s = p.searcher(k);
        for c in text.chunks(chunk) {
            s.feed(c, |hit| {
                all.push(hit);
                sel.push(hit, |x| out.push(x));
            });
            sel.advance(s.position(), |x| out.push(x));
        }
        sel.finish(|x| out.push(x));
        out.sort_by_key(|x| x.end);
        prop_assert_eq!(out, naive(&all, pat.len(), k));
    }
}
