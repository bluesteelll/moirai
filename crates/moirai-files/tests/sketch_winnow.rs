//! WP-66: token winnowing ([F20 §2.9]) against a definition written from the section alone: the token rule, the
//! k-gram hashes, the window selection with its tie rule, `FW(t)`, `J` and the exact limit ([F20 §2.10.5]).
//!
//! `K` and `W` are holes (F20-winnow-k, F20-winnow-w): every test is written with `WINNOW_K` and `WINNOW_W`, never
//! with their draft values. Every content here is synthetic and in memory.

use std::collections::{BTreeSet, HashSet};

use moirai_files::r14::{EXACT_LIMIT, Ratio, WINNOW_K, WINNOW_W};
use moirai_files::sketch::{WinnowSink, jaccard, select, tokens, winnow};
use moirai_files::text::LineSink;
use proptest::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

// --- the definition, written from [F20 §1.2, §2.9] alone ----------------------------------------------------------

fn is_ws(b: u8) -> bool {
    matches!(b, 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20)
}

/// A word byte: `30`–`39`, `41`–`5A`, `61`–`7A`, `5F` or ≥ `80`.
fn is_word(b: u8) -> bool {
    (0x30..=0x39).contains(&b)
        || (0x41..=0x5A).contains(&b)
        || (0x61..=0x7A).contains(&b)
        || b == 0x5F
        || b >= 0x80
}

/// T: maximal runs of word bytes, every other non-`WS` byte alone.
fn naive_tokens(t: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if is_ws(t[i]) {
            i += 1;
        } else if is_word(t[i]) {
            let s = i;
            while i < t.len() && is_word(t[i]) {
                i += 1;
            }
            out.push(&t[s..i]);
        } else {
            out.push(&t[i..=i]);
            i += 1;
        }
    }
    out
}

/// `g_j` for j = 0 … m − K: XXH3-64 of the K token hashes, each a little-endian `u64`.
fn naive_kgrams(t: &[u8]) -> Vec<u64> {
    let th: Vec<u64> = naive_tokens(t).into_iter().map(xxh3_64).collect();
    kgrams_of(&th)
}

fn kgrams_of(th: &[u64]) -> Vec<u64> {
    if th.len() < WINNOW_K {
        return Vec::new();
    }
    (0..=th.len() - WINNOW_K)
        .map(|j| {
            let mut buf = Vec::new();
            for h in &th[j..j + WINNOW_K] {
                for k in 0..8 {
                    buf.push((h >> (8 * k)) as u8);
                }
            }
            xxh3_64(&buf)
        })
        .collect()
}

/// The windows of [F20 §2.9]: none when G = 0, one over all k-grams when 1 ≤ G < W, else `g_j … g_(j+W−1)`.
fn windows(g: &[u64]) -> Vec<&[u64]> {
    if g.is_empty() {
        Vec::new()
    } else if g.len() < WINNOW_W {
        vec![g]
    } else {
        g.windows(WINNOW_W).collect()
    }
}

/// The rightmost position of the minimum in each window.
fn naive_select(g: &[u64]) -> Vec<usize> {
    windows(g)
        .into_iter()
        .enumerate()
        .map(|(j, w)| {
            let min = *w.iter().min().unwrap();
            let start = if g.len() < WINNOW_W { 0 } else { j };
            start + w.iter().rposition(|&v| v == min).unwrap()
        })
        .collect()
}

/// `FW(t)`: the set of the windows' minimum values.
fn naive_fw(t: &[u8]) -> BTreeSet<u64> {
    let g = naive_kgrams(t);
    windows(&g)
        .into_iter()
        .map(|w| *w.iter().min().unwrap())
        .collect()
}

fn values(t: &[u8]) -> Vec<u64> {
    winnow(t).unwrap().values().to_vec()
}

// --- the property tests ------------------------------------------------------------------------------------------

/// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs 16
/// times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is printed), so a
/// failing case never writes a regressions file into the repository.
fn test_config(base: u32) -> ProptestConfig {
    let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => base * 16,
        Ok("exit") => base * 64,
        _ => base,
    };
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Text built from short words of a small pool, punctuation, `WS` of every kind (LF included), bytes ≥ `80`, now and
/// then any byte at all, and runs of a short unit of tokens repeated, so that equal k-grams meet in one window and
/// windows have tied minima.
fn text() -> impl Strategy<Value = Vec<u8>> {
    let word = prop::sample::select(vec![
        "a", "b", "fn", "foo", "foo_bar", "x1", "_", "Ä", "жж", "12", "let",
    ])
    .prop_map(|s| s.as_bytes().to_vec());
    let piece = prop_oneof![
        6 => word.clone(),
        3 => prop::sample::select(b"(){}[];:,.-+*/<>=!&|\"'#".to_vec()).prop_map(|b| vec![b]),
        4 => prop::collection::vec(
            prop::sample::select(vec![0x20u8, 0x20, 0x20, 0x09, 0x0A, 0x0B, 0x0C, 0x0D]),
            1..3,
        ),
        1 => any::<u8>().prop_map(|b| vec![b]),
        1 => (prop::collection::vec(word, 1..4), 2usize..12).prop_map(|(unit, n)| {
            let mut run = vec![b' '];
            for _ in 0..n {
                for w in &unit {
                    run.extend_from_slice(w);
                    run.push(b' ');
                }
            }
            run
        }),
    ];
    prop::collection::vec(piece, 0..160).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(test_config(128))]

    /// `tokens` splits as [F20 §2.9] says, over any bytes.
    #[test]
    fn tokens_equal_the_naive_split(t in prop_oneof![text(), prop::collection::vec(any::<u8>(), 0..200)]) {
        prop_assert_eq!(tokens(&t).collect::<Vec<_>>(), naive_tokens(&t));
    }

    /// `winnow(t)` is `FW(t)` of the definition: the k-gram hashes, the windows and their minimum values. `FW` is a set
    /// of values, so which of equal minima a window selects cannot show here (`select_takes_the_rightmost_minimum`).
    #[test]
    fn winnowing_equals_the_naive_definition(t in text()) {
        let fw = winnow(&t).unwrap();
        let want: Vec<u64> = naive_fw(&t).into_iter().collect();
        prop_assert_eq!(fw.values(), want.as_slice());
        prop_assert_eq!(fw.len(), fw.values().len());
        prop_assert_eq!(fw.is_empty(), fw.values().is_empty());
    }

    /// The sink fed a line in pieces cut anywhere, inside word tokens included, gives `winnow(t)`.
    #[test]
    fn winnow_sink_pieces_never_change_fw(
        t in text(),
        sizes in prop::collection::vec(1usize..5, 1..8),
    ) {
        let want = winnow(&t);
        let mut cuts = WinnowSink::new();
        cuts.begin();
        let mut bytes = WinnowSink::new();
        bytes.begin();
        let mut k = 0;
        for l in t.split(|&b| b == b'\n') {
            let mut rest = l;
            while !rest.is_empty() {
                let n = sizes[k % sizes.len()].min(rest.len());
                k += 1;
                cuts.piece(&rest[..n]);
                rest = &rest[n..];
            }
            cuts.end_line();
            for b in l.chunks(1) {
                bytes.piece(b);
            }
            bytes.end_line();
        }
        prop_assert_eq!(cuts.finish(), want.clone());
        prop_assert_eq!(bytes.finish(), want);
    }
}

// --- the examples --------------------------------------------------------------------------------------------------

/// Each byte alone and between two `a` bytes: a word byte extends the token, a `WS` byte separates, any other byte is
/// its own token.
#[test]
fn tokens_follow_the_byte_classes() {
    for b in 0..=255u8 {
        let (one, three) = ([b], [b'a', b, b'a']);
        let alone: Vec<&[u8]> = tokens(&one).collect();
        let between: Vec<&[u8]> = tokens(&three).collect();
        if is_ws(b) {
            assert!(alone.is_empty(), "{b:#04x}");
            assert_eq!(between, [b"a", b"a"], "{b:#04x}");
        } else if is_word(b) {
            assert_eq!(alone, [[b]], "{b:#04x}");
            assert_eq!(between, [[b'a', b, b'a']], "{b:#04x}");
        } else {
            assert_eq!(alone, [[b]], "{b:#04x}");
            assert_eq!(between, [&b"a"[..], &[b], b"a"], "{b:#04x}");
        }
    }
    let t: Vec<&str> = tokens("fn foo_bar(x: u8) -> Ä;".as_bytes())
        .map(|t| std::str::from_utf8(t).unwrap())
        .collect();
    assert_eq!(
        t,
        [
            "fn", "foo_bar", "(", "x", ":", "u8", ")", "-", ">", "Ä", ";"
        ]
    );
}

/// Among equal minima the rightmost position is selected ([F20 §2.9]).
#[test]
fn select_takes_the_rightmost_minimum() {
    let w = WINNOW_W;
    // All equal, G = W + 2: windows 0, 1, 2 select their last positions.
    assert_eq!(select(&vec![7; w + 2]), [w - 1, w, w + 1]);

    // Mixed ties: the minimum 1 at positions 0 and W − 1 (both in window 0) and again at 2W − 1.
    let mut g = vec![9; 2 * w + 1];
    g[0] = 1;
    g[w - 1] = 1;
    g[2 * w - 1] = 1;
    let got = select(&g);
    assert_eq!(got, naive_select(&g));
    assert_eq!(got[0], w - 1);
    assert_eq!(got[w], 2 * w - 1);

    // A tie between the minimum and a larger value later: the minimum's last position stays.
    let mut g = vec![5; w + 1];
    g[0] = 2;
    g[1] = 2;
    assert_eq!(select(&g)[0], if w > 1 { 1 } else { 0 });
    assert_eq!(select(&g), naive_select(&g));

    // G < W with ties: one window, its last position.
    if w > 1 {
        assert_eq!(select(&vec![3; w - 1]), [w - 2]);
        let mut g = vec![4; w - 1];
        g[0] = 1;
        assert_eq!(select(&g), [0]);
    }
    // G = 0: no window.
    assert!(select(&[]).is_empty());
}

/// Distinct one-letter-and-number word tokens `t0 t1 …`, m of them.
fn distinct_tokens(m: usize) -> Vec<u8> {
    (0..m)
        .map(|i| format!("t{i}"))
        .collect::<Vec<_>>()
        .join(" ")
        .into_bytes()
}

/// m = K − 1 tokens: no k-gram, an empty set; m = K: `{g_0}`; G = W − 1 and G = W: one window each.
#[test]
fn short_texts_g0_and_one_window() {
    let empty = winnow(&distinct_tokens(WINNOW_K - 1)).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(winnow(b"").unwrap().is_empty());
    assert!(winnow(b" \n\t ").unwrap().is_empty());

    let t = distinct_tokens(WINNOW_K);
    let g = naive_kgrams(&t);
    assert_eq!(g.len(), 1);
    assert_eq!(values(&t), g);

    for big_g in [WINNOW_W.saturating_sub(1).max(1), WINNOW_W] {
        let t = distinct_tokens(WINNOW_K + big_g - 1);
        let g = naive_kgrams(&t);
        assert_eq!(g.len(), big_g);
        assert_eq!(values(&t), [*g.iter().min().unwrap()], "G = {big_g}");
        assert_eq!(select(&g).len(), 1, "G = {big_g}");
    }
}

/// `J` is exact: 1 for equal non-empty sets, 0 for disjoint or empty ones, and `|∩| / |∪|` for a worked pair.
#[test]
fn jaccard_is_exact_and_zero_on_empty() {
    let a = winnow(b"fn main() { let x = compute(alpha, beta); println!(\"{x}\"); }").unwrap();
    assert!(!a.is_empty());
    assert_eq!(jaccard(&a, &a), Ratio::ONE);

    let b = winnow(b"struct Other { field: u32, more: Vec<u8> }").unwrap();
    let (sa, sb): (BTreeSet<u64>, BTreeSet<u64>) = (
        a.values().iter().copied().collect(),
        b.values().iter().copied().collect(),
    );
    assert!(sa.is_disjoint(&sb));
    assert_eq!(jaccard(&a, &b), Ratio::ZERO);

    let e = winnow(&distinct_tokens(WINNOW_K - 1)).unwrap();
    assert!(e.is_empty());
    assert_eq!(jaccard(&e, &e), Ratio::ZERO);
    assert_eq!(jaccard(&a, &e), Ratio::ZERO);

    // A worked pair: the second text shares a run of 12 tokens with the first.
    let t1 = b"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi";
    let t2 = b"one two three gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi four five six";
    let (f1, f2) = (naive_fw(t1), naive_fw(t2));
    let inter = f1.intersection(&f2).count() as u64;
    let union = f1.union(&f2).count() as u64;
    assert!(inter > 0 && inter < union);
    let (w1, w2) = (winnow(t1).unwrap(), winnow(t2).unwrap());
    assert_eq!(jaccard(&w1, &w2), Ratio::new(inter, union));
    assert_eq!(jaccard(&w2, &w1), Ratio::new(inter, union));
}

/// `FW` is kept up to `EXACT_LIMIT` values and refused above ([F20 §2.10.5]).
#[test]
fn jaccard_exact_limit_boundary() {
    let limit = EXACT_LIMIT as usize;
    // One long list of distinct tokens, long enough for about 2 / (W + 1) distinct minima per k-gram; the
    // incremental set size after each window finds the two prefixes.
    let n = (limit + 1) * (WINNOW_W + 1) * 3 / 4 + WINNOW_K + WINNOW_W;
    let words: Vec<String> = (0..n).map(|i| format!("w{i}")).collect();
    let th: Vec<u64> = words.iter().map(|w| xxh3_64(w.as_bytes())).collect();
    let g = kgrams_of(&th);
    let mut seen = HashSet::new();
    let (mut at_limit, mut over) = (None, None);
    for (j, w) in windows(&g).into_iter().enumerate() {
        seen.insert(*w.iter().min().unwrap());
        if seen.len() == limit && at_limit.is_none() {
            at_limit = Some(j);
        }
        if seen.len() == limit + 1 {
            over = Some(j);
            break;
        }
    }
    // Window j ends at k-gram j + W − 1, which ends at token j + W − 1 + K − 1.
    let tokens_for = |j: usize| j + WINNOW_W + WINNOW_K - 1;
    let prefix = |m: usize| words[..m].join(" ").into_bytes();

    let fw = winnow(&prefix(tokens_for(at_limit.unwrap()))).unwrap();
    assert_eq!(fw.len(), limit);
    assert_eq!(jaccard(&fw, &fw), Ratio::ONE);
    assert_eq!(winnow(&prefix(tokens_for(over.unwrap()))), None);
}

/// Feeds a text line by line, each line as one piece.
fn feed(sink: &mut WinnowSink, t: &[u8]) {
    for l in t.split(|&b| b == b'\n') {
        if !l.is_empty() {
            sink.piece(l);
        }
        sink.end_line();
    }
}

/// [F20 §2.4]: every read attempt starts with `begin`, which forgets the earlier attempt (its tokens, k-grams, window
/// and values), a half-received word token, and a set dropped past `EXACT_LIMIT`.
#[test]
fn winnow_sink_begin_discards_an_earlier_attempt() {
    let earlier: &[u8] = b"fn earlier(a: u8) -> u8 { a + 1 }\nlet earlier_value = compute(alpha, beta);\nearlier tokens";
    // Exactly `WINNOW_K` tokens over two lines, so `FW(later) = {g_0}`: a change to any later token changes it.
    let words: Vec<String> = (0..WINNOW_K).map(|i| format!("later_{i}")).collect();
    let later = format!("{}\n  {}  ", words[0], words[1..].join(" ")).into_bytes();
    let want = winnow(&later).unwrap();
    assert_eq!(want.values(), naive_kgrams(&later).as_slice());

    // After a closed line.
    let mut s = WinnowSink::new();
    s.begin();
    feed(&mut s, earlier);
    assert!(!s.clone().finish().unwrap().is_empty());
    s.begin();
    feed(&mut s, &later);
    assert_eq!(s.finish(), Some(want.clone()));

    // In the middle of a word token, and after a pending space.
    for half in [&b"earlier_half_wo"[..], b"half (a) line \t"] {
        let mut s = WinnowSink::new();
        s.begin();
        feed(&mut s, earlier);
        s.piece(half);
        s.begin();
        feed(&mut s, &later);
        assert_eq!(
            s.finish(),
            Some(want.clone()),
            "{:?}",
            String::from_utf8_lossy(half)
        );
    }

    // After an attempt past `EXACT_LIMIT` values (about 2 / (W + 1) values per k-gram: twice the limit), whose set
    // was dropped.
    let long = distinct_tokens((EXACT_LIMIT as usize + 1) * (WINNOW_W + 1));
    let mut s = WinnowSink::new();
    s.begin();
    feed(&mut s, &long);
    assert_eq!(s.clone().finish(), None);
    s.begin();
    feed(&mut s, &later);
    assert_eq!(s.finish(), Some(want));
}
