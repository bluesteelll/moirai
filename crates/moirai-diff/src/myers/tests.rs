//! The matcher against naive dynamic programs written here ([F20 §6.4]'s definitions, enumerated).

use super::{Hit, Located, Pattern, levenshtein};
use proptest::prelude::*;

/// d(e) for every end offset e in 0..=n: the semi-global dynamic program (the top row is 0).
fn naive_ends(pat: &[u8], text: &[u8]) -> Vec<usize> {
    let m = pat.len();
    let mut col: Vec<usize> = (0..=m).collect();
    let mut out = vec![m];
    for &t in text {
        let mut diag = col[0];
        col[0] = 0;
        for i in 1..=m {
            let left = col[i];
            col[i] = (diag + usize::from(pat[i - 1] != t))
                .min(left + 1)
                .min(col[i - 1] + 1);
            diag = left;
        }
        out.push(col[m]);
    }
    out
}

/// The Levenshtein distance by the textbook dynamic program.
fn naive_lev(a: &[u8], b: &[u8]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, &y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(x != y))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// [F20 §6.4] by enumeration: the least distance over starts s in [lo, end], and the largest s achieving it. The
/// distance of every s comes from one dynamic program over the reversed pattern and the reversed `text[lo..end)`,
/// whose last row holds lev(pat, text[end − L..end)) for every length L.
fn naive_locate(pat: &[u8], text: &[u8], end: usize, lo: usize, k: usize) -> Option<Located> {
    let rp: Vec<u8> = pat.iter().rev().copied().collect();
    let m = rp.len();
    let mut col: Vec<usize> = (0..=m).collect();
    let mut best = Located {
        start: end,
        distance: m,
    };
    for (l, &t) in text[lo..end].iter().rev().enumerate() {
        let mut diag = col[0];
        col[0] = l + 1;
        for i in 1..=m {
            let left = col[i];
            col[i] = (diag + usize::from(rp[i - 1] != t))
                .min(left + 1)
                .min(col[i - 1] + 1);
            diag = left;
        }
        if col[m] < best.distance {
            best = Located {
                start: end - (l + 1),
                distance: col[m],
            };
        }
    }
    (best.distance <= k).then_some(best)
}

fn expected(pat: &[u8], text: &[u8], k: usize) -> Vec<Hit> {
    naive_ends(pat, text)
        .into_iter()
        .enumerate()
        .filter(|&(_, d)| d <= k)
        .map(|(end, distance)| Hit { end, distance })
        .collect()
}

fn hits(pat: &[u8], text: &[u8], k: usize) -> Vec<Hit> {
    let mut out = Vec::new();
    Pattern::new(pat).search(text, k, |h| out.push(h));
    out
}

/// A text drawn from `alphabet`, deterministic in `seed` (xorshift).
fn text_of(seed: u64, len: usize, alphabet: &[u8]) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            alphabet[(x % alphabet.len() as u64) as usize]
        })
        .collect()
}

#[test]
fn levenshtein_known_values() {
    assert_eq!(levenshtein(b"kitten", b"sitting"), 3);
    assert_eq!(levenshtein(b"sitting", b"kitten"), 3);
    assert_eq!(levenshtein(b"", b"abc"), 3);
    assert_eq!(levenshtein(b"abc", b""), 3);
    assert_eq!(levenshtein(b"", b""), 0);
    assert_eq!(levenshtein(b"flaw", b"lawn"), 2);
    assert_eq!(levenshtein(b"same", b"same"), 0);
    assert_eq!(Pattern::new(b"abc").distance(b"abc"), 0);
    assert_eq!(Pattern::new(b"").distance(b"xyz"), 3);
}

#[test]
fn exact_and_one_error_hits() {
    assert_eq!(
        hits(b"abc", b"xxabcxxabdx", 0),
        [Hit {
            end: 5,
            distance: 0
        }]
    );
    let one = hits(b"abc", b"xxabcxxabdx", 1);
    assert_eq!(one, expected(b"abc", b"xxabcxxabdx", 1));
    assert!(one.contains(&Hit {
        end: 9,
        distance: 1
    }));
    assert!(one.contains(&Hit {
        end: 10,
        distance: 1
    }));
    assert!(one.windows(2).all(|w| w[0].end < w[1].end));
}

#[test]
fn empty_pattern_and_empty_text() {
    let all: Vec<Hit> = (0..=3).map(|end| Hit { end, distance: 0 }).collect();
    assert_eq!(hits(b"", b"abc", 0), all);
    assert!(hits(b"ab", b"", 1).is_empty());
    assert_eq!(
        hits(b"ab", b"", 2),
        [Hit {
            end: 0,
            distance: 2
        }]
    );
    // A budget ≥ m reports every end, the empty substring included.
    assert_eq!(hits(b"ab", b"zz", 9), expected(b"ab", b"zz", 9));
    assert_eq!(hits(b"ab", b"zz", 9).len(), 3);
    assert!(!Pattern::new(b"ab").is_empty());
    let p = Pattern::new(b"");
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    assert_eq!(
        p.locate(b"abc", 2, 0, 0),
        Some(Located {
            start: 2,
            distance: 0
        })
    );
}

#[test]
fn block_boundaries_every_budget() {
    for m in [1usize, 2, 63, 64, 65, 127, 128, 129, 191, 192, 193, 300] {
        let pat = text_of(m as u64 * 7 + 1, m, b"abcd");
        // A text holding a mutated copy of the pattern between random bytes.
        let mut text = text_of(m as u64 + 99, 90, b"abcd");
        let mut copy = pat.clone();
        for x in (0..copy.len()).step_by(9) {
            copy[x] = b'z';
        }
        // Odd lengths turn the substitutions into deletions.
        if copy.len() % 2 == 1 {
            copy.retain(|&b| b != b'z');
        }
        text.extend_from_slice(&copy);
        text.extend(text_of(m as u64 + 5, 70, b"abcd"));
        let ends = naive_ends(&pat, &text);
        let mut budgets: Vec<usize> =
            vec![0, 1, 2, m / 4, m / 3, m / 2, m.saturating_sub(1), m, m + 3];
        budgets.dedup();
        for k in budgets {
            let want: Vec<Hit> = ends
                .iter()
                .enumerate()
                .filter(|&(_, &d)| d <= k)
                .map(|(end, &distance)| Hit { end, distance })
                .collect();
            assert_eq!(hits(&pat, &text, k), want, "m = {m}, k = {k}");
        }
    }
}

#[test]
fn every_byte_value_in_the_pattern() {
    // 256 distinct bytes: no absent class, four blocks.
    let pat: Vec<u8> = (0..=255u8).collect();
    let mut text: Vec<u8> = (0..=255u8).rev().collect();
    text.extend(
        pat.iter()
            .map(|&b| if b % 17 == 0 { b.wrapping_add(1) } else { b }),
    );
    text.extend_from_slice(b"tail");
    for k in [0usize, 10, 16, 20, 64, 255, 256] {
        assert_eq!(hits(&pat, &text, k), expected(&pat, &text, k), "k = {k}");
    }
    let p = Pattern::new(&pat);
    assert_eq!(p.distance(&text), naive_lev(&pat, &text));
    assert_eq!(
        p.locate(&text, 512, 0, 16),
        naive_locate(&pat, &text, 512, 0, 16)
    );
}

#[test]
fn locate_largest_start() {
    let p = Pattern::new(b"abc");
    // "xabcx": the match ending at 4 starts at 1.
    assert_eq!(
        p.locate(b"xabcx", 4, 0, 1),
        Some(Located {
            start: 1,
            distance: 0
        })
    );
    // Ending at 5 ("abcx" at 1, one deletion; "bcx" at 2, two edits): distance 1 from start 1.
    assert_eq!(
        p.locate(b"xabcx", 5, 0, 3),
        naive_locate(b"abc", b"xabcx", 5, 0, 3)
    );
    // Ties: "aab" ending at 3 with pattern "ab": "ab" at 1 is exact.
    assert_eq!(
        Pattern::new(b"ab").locate(b"aab", 3, 0, 2),
        Some(Located {
            start: 1,
            distance: 0
        })
    );
    // Equal distances keep the largest start: "xb" (s = 0) and "b" (s = 1) are both one edit from "ab".
    assert_eq!(
        Pattern::new(b"ab").locate(b"xb", 2, 0, 1),
        Some(Located {
            start: 1,
            distance: 1
        })
    );
    // The lower bound restricts the starts.
    assert_eq!(
        p.locate(b"abcabc", 6, 4, 9),
        naive_locate(b"abc", b"abcabc", 6, 4, 9)
    );
    assert_eq!(p.locate(b"abcabc", 6, 4, 0), None);
    // Out-of-range arguments are clamped.
    assert_eq!(
        p.locate(b"abc", 99, 0, 0),
        Some(Located {
            start: 0,
            distance: 0
        })
    );
    assert_eq!(
        p.locate(b"abc", 2, 5, 9),
        naive_locate(b"abc", b"abc", 2, 2, 9)
    );
}

#[test]
fn heap_bytes_are_small() {
    let quote = text_of(3, 128, b"abcdefghijklmnopqrstuvwxyz0123456789 _.,");
    let p = Pattern::new(&quote);
    // Two blocks, at most 41 classes, two tables.
    assert!(p.heap_bytes() <= 2 * 41 * 2 * 8, "{}", p.heap_bytes());
    assert_eq!(p.len(), 128);
    // "ab": two classes and the zero class, one block, two tables.
    assert_eq!(Pattern::new(b"ab").heap_bytes(), 3 * 8 * 2);
    // Every byte present: no zero class, four blocks.
    let all: Vec<u8> = (0..=255u8).collect();
    assert_eq!(Pattern::new(&all).heap_bytes(), 256 * 4 * 8 * 2);
    assert_eq!(Pattern::new(b"").heap_bytes(), 0);
}

#[test]
fn searcher_reports_position_and_empty_match_once() {
    let p = Pattern::new(b"ab");
    let mut s = p.searcher(2);
    let mut out = Vec::new();
    s.feed(b"", |h| out.push(h));
    s.feed(b"", |h| out.push(h));
    assert_eq!(
        out,
        [Hit {
            end: 0,
            distance: 2
        }]
    );
    s.feed(b"xa", |h| out.push(h));
    assert_eq!(s.position(), 2);
    s.feed(b"b", |h| out.push(h));
    assert_eq!(out, expected(b"ab", b"xab", 2));
}

fn byte_text(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        3 => prop::collection::vec(prop_oneof![Just(b'a'), Just(b'b'), Just(b'c')], 0..max),
        1 => prop::collection::vec(any::<u8>(), 0..max),
    ]
}

proptest! {
    #![proptest_config(crate::test_config(256))]

    #[test]
    fn search_equals_dynamic_program(pat in byte_text(200), text in byte_text(300), extra in 0usize..4, frac in 0usize..5) {
        let k = pat.len() * frac / 4 + extra;
        prop_assert_eq!(hits(&pat, &text, k), expected(&pat, &text, k));
    }

    #[test]
    fn long_patterns_small_budget(seed in any::<u64>(), m in 65usize..330, cuts in prop::collection::vec(0usize..330, 0..12)) {
        // Mutated copies of a multi-block pattern inside noise: the cut-off must reactivate blocks correctly.
        let pat = text_of(seed, m, b"acgt");
        let mut text = text_of(seed ^ 0xabcd, 50, b"acgt");
        let mut copy = pat.clone();
        for c in cuts {
            if c < copy.len() {
                if c % 3 == 0 { copy.remove(c); } else { copy[c] = b"acgt"[c % 4]; }
            }
        }
        text.extend_from_slice(&copy);
        text.extend(text_of(seed ^ 0x1234, 40, b"acgt"));
        text.extend_from_slice(&pat);
        for k in [0, 1, m / 10, m / 4] {
            prop_assert_eq!(hits(&pat, &text, k), expected(&pat, &text, k), "k = {}", k);
        }
    }

    #[test]
    fn chunked_feed_equals_one_pass(pat in byte_text(140), text in byte_text(260), k in 0usize..40, cuts in prop::collection::vec(0usize..260, 0..6)) {
        let p = Pattern::new(&pat);
        let mut whole = Vec::new();
        p.search(&text, k, |h| whole.push(h));
        let mut cuts: Vec<usize> = cuts.into_iter().map(|c| c.min(text.len())).collect();
        cuts.sort_unstable();
        let mut s = p.searcher(k);
        let mut chunked = Vec::new();
        let mut at = 0;
        for c in cuts.into_iter().chain([text.len()]) {
            s.feed(&text[at..c], |h| chunked.push(h));
            at = c;
        }
        prop_assert_eq!(s.position(), text.len());
        prop_assert_eq!(chunked, whole);
    }

    #[test]
    fn locate_equals_enumeration(pat in byte_text(70), text in byte_text(110), end_f in 0usize..=100, lo_f in 0usize..=100, k in 0usize..30) {
        let end = text.len() * end_f / 100;
        let lo = end * lo_f / 100;
        prop_assert_eq!(Pattern::new(&pat).locate(&text, end, lo, k), naive_locate(&pat, &text, end, lo, k));
    }

    #[test]
    fn hit_starts_are_located(pat in byte_text(90), text in byte_text(200), frac in 0usize..5) {
        // [F20 §6.4]: for each recorded end e, s is the largest start with distance d(e).
        let k = pat.len() * frac / 4;
        let p = Pattern::new(&pat);
        let mut all = Vec::new();
        p.search(&text, k, |h| all.push(h));
        for h in all.into_iter().take(40) {
            let got = p.locate(&text, h.end, 0, h.distance);
            prop_assert_eq!(got, naive_locate(&pat, &text, h.end, 0, h.distance));
            prop_assert_eq!(got.map(|l| l.distance), Some(h.distance));
        }
    }

    #[test]
    fn distance_equals_dynamic_program(a in byte_text(300), b in byte_text(300)) {
        prop_assert_eq!(levenshtein(&a, &b), naive_lev(&a, &b));
        prop_assert_eq!(Pattern::new(&a).distance(&b), naive_lev(&a, &b));
    }
}

proptest! {
    #![proptest_config(crate::test_config(64))]

    #[test]
    fn long_patterns_locate_and_distance(
        seed in any::<u64>(),
        m in 65usize..330,
        cuts in prop::collection::vec(0usize..330, 0..12),
        end_f in 0usize..=100,
        lo_f in 0usize..=100,
    ) {
        // Multi-block patterns (a 128-byte quote takes two blocks) in the banded global mode of locate and distance:
        // blocks activate and deactivate under the global top row. Mutated copies inside noise, at the copy's end
        // and at any end and lower bound.
        let pat = text_of(seed, m, b"acgt");
        let mut copy = pat.clone();
        for c in cuts {
            if c < copy.len() {
                if c % 3 == 0 { copy.remove(c); } else { copy[c] = b"acgt"[c % 4]; }
            }
        }
        let mut text = text_of(seed ^ 0xabcd, 50, b"acgt");
        text.extend_from_slice(&copy);
        let copy_end = text.len();
        text.extend(text_of(seed ^ 0x1234, 40, b"acgt"));
        let p = Pattern::new(&pat);
        prop_assert_eq!(p.distance(&copy), naive_lev(&pat, &copy));
        let end = text.len() * end_f / 100;
        let lo = end * lo_f / 100;
        for k in [0, 1, m / 10, m / 4] {
            prop_assert_eq!(p.locate(&text, copy_end, 0, k), naive_locate(&pat, &text, copy_end, 0, k), "k = {}", k);
            prop_assert_eq!(p.locate(&text, end, lo, k), naive_locate(&pat, &text, end, lo, k), "k = {}, end = {}, lo = {}", k, end, lo);
        }
    }
}

/// The test-only work counters after `f`: blocks advanced and heap columns.
fn work_of(f: impl FnOnce()) -> (usize, usize) {
    super::WORK.with(|w| w.set((0, 0)));
    f();
    super::WORK.with(std::cell::Cell::get)
}

#[test]
fn banded_work_is_pinned() {
    // Speed-only choices change these counts and nothing else: the band's activation and cut-off in a search, the
    // early stop of locate, the side levenshtein takes as the pattern, and where a column is kept.
    let pat = text_of(21, 150, b"acgt");
    let mut text = text_of(22, 600, b"acgt");
    text.extend_from_slice(&pat[..140]);
    text.extend_from_slice(b"gg");
    text.extend_from_slice(&pat[140..]);
    text.extend(text_of(23, 300, b"acgt"));
    let p = Pattern::new(&pat);
    for (k, blocks, count, last) in [
        (0, 1224, 0, None),
        (5, 1287, 8, Some((755, 5))),
        (20, 1544, 41, Some((770, 20))),
        (40, 2198, 81, Some((790, 40))),
        (70, 3071, 141, Some((820, 70))),
    ] {
        let (mut n, mut end) = (0, None);
        let w = work_of(|| {
            p.search(&text, k, |h| {
                n += 1;
                end = Some((h.end, h.distance));
            });
        });
        assert_eq!((w, n, end), ((blocks, 0), count, last), "k = {k}");
    }
    // A copy with two bytes deleted: the best (distance 2, 148 bytes) is found before the stop after 151 bytes.
    let mut cut = pat.clone();
    cut.remove(90);
    cut.remove(30);
    let mut text2 = text_of(26, 600, b"acgt");
    text2.extend_from_slice(&cut);
    text2.extend(text_of(27, 100, b"acgt"));
    let end = 600 + 148;
    let best = Some(Located {
        start: 600,
        distance: 2,
    });
    for (lo, k, blocks, want) in [(0, 2, 265, best), (0, 40, 341, best), (700, 10, 48, None)] {
        let mut got = None;
        let w = work_of(|| got = p.locate(&text2, end, lo, k));
        assert_eq!((w, got), ((blocks, 0), want), "lo = {lo}, k = {k}");
    }
    // Global distance: every block of the 150-byte pattern for each of 100 bytes.
    assert_eq!(
        work_of(|| assert_eq!(p.distance(&pat[..100]), 50)),
        (300, 0)
    );
    // levenshtein takes the shorter side as the pattern: one block for each of the 300 bytes, on the stack. The other
    // side would take five blocks for each of the 40 bytes, on the heap.
    let long = text_of(24, 300, b"acgt");
    let short = text_of(25, 40, b"acgt");
    for (a, b) in [(&long, &short), (&short, &long)] {
        let mut d = 0;
        assert_eq!(work_of(|| d = levenshtein(a, b)), (300, 0));
        assert_eq!(d, 260);
    }
    assert_eq!(
        work_of(|| assert_eq!(Pattern::new(&long).distance(&short), 260)),
        (200, 1)
    );
}
