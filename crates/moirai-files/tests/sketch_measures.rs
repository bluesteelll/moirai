//! WP-66: the exact line measures ([F20 §2.10.1]) over the multiset of `(fh(f), len(f))` pairs, the exact limit
//! ([F20 §2.10.5]), and the sketch estimates ([F20 §2.10.2]) with the `hit` count, each against a definition written
//! from chapter 20 alone, in exact rational arithmetic.
//!
//! Every content here is synthetic and in memory.

use std::collections::{BTreeSet, HashMap};

use moirai_files::oid::ObjectFormat;
use moirai_files::r14::{EXACT_LIMIT, Ratio};
use moirai_files::sketch::{
    Fingerprint, FingerprintSink, HitSink, LineMultiset, MultisetSink, estimates, exact,
};
use moirai_files::text::{LineSink, analyse};
use proptest::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

// --- the definitions, written from [F20 §1.2, §2.2, §2.5, §2.6, §2.10] alone --------------------------------------

fn is_ws(b: u8) -> bool {
    matches!(b, 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20)
}

/// `chars(x)`: the bytes of x outside `80`–`BF`.
fn chars(x: &[u8]) -> usize {
    x.iter().filter(|&&b| !(0x80..=0xBF).contains(&b)).count()
}

/// The fingerprint lines `f = collapse(nl(l))` with `chars(f) > 3` of a text content b: the lines of `atext(b)`,
/// `norm(b)` without one leading BOM.
fn fingerprint_lines(b: &[u8]) -> Vec<Vec<u8>> {
    let mut norm = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !(b[i] == 0x0D && b.get(i + 1) == Some(&0x0A)) {
            norm.push(b[i]);
        }
        i += 1;
    }
    let atext = norm.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&norm);
    let mut lines: Vec<&[u8]> = atext.split(|&c| c == 0x0A).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|l| {
            let words: Vec<&[u8]> = l.split(|&c| is_ws(c)).filter(|w| !w.is_empty()).collect();
            words.join(&b' ')
        })
        .filter(|f| chars(f) > 3)
        .collect()
}

/// `M(X)`: pair → multiplicity.
type Multiset = HashMap<(u64, u64), u64>;

fn naive_multiset(b: &[u8]) -> Multiset {
    let mut m = Multiset::new();
    for f in fingerprint_lines(b) {
        *m.entry((xxh3_64(&f), f.len() as u64)).or_default() += 1;
    }
    m
}

fn weight(m: &Multiset) -> u64 {
    m.iter().map(|(&(_, len), &c)| c * len).sum()
}

/// `(oin, nio, sym)` as exact `num / den` pairs, a zero denominator giving 0.
fn naive_exact(a: &Multiset, b: &Multiset) -> [(u128, u128); 3] {
    let inter: u64 = a
        .iter()
        .map(|(k, &c)| c.min(b.get(k).copied().unwrap_or(0)) * k.1)
        .sum();
    let q = |d: u64| {
        if d == 0 {
            (0, 1)
        } else {
            (u128::from(inter), u128::from(d))
        }
    };
    let (wa, wb) = (weight(a), weight(b));
    [q(wa), q(wb), q(wa.max(wb))]
}

/// Whether `r` equals `num / den` by value.
fn same(r: Ratio, (num, den): (u128, u128)) -> bool {
    u128::from(r.num()) * den == num * u128::from(r.den())
}

/// The exact multiset of a content through `analyse`.
fn multiset_of(t: &[u8]) -> LineMultiset {
    let mut sink = MultisetSink::new();
    analyse(t, ObjectFormat::Sha1, None, &mut sink);
    sink.finish().unwrap()
}

fn fingerprint_of(t: &[u8]) -> Fingerprint {
    let mut sink = FingerprintSink::new();
    let c = analyse(t, ObjectFormat::Sha1, None, &mut sink);
    sink.finish(&c.stats).unwrap()
}

/// A fingerprint value field by field ([F20 §2.6.4], little-endian): `n_sketch` values `1, 2, …` ascending.
fn fingerprint(n: u8, estimated: bool, distinct: u32) -> Fingerprint {
    let mut b = vec![1, 0, n, u8::from(estimated)];
    for v in [10u32, 1_000, 500, distinct] {
        b.extend(v.to_le_bytes());
    }
    for i in 1..=u32::from(n) {
        b.extend((i * 1_000).to_le_bytes());
    }
    Fingerprint::from_bytes(&b).unwrap()
}

/// Feeds whole lines, as one piece each.
fn feed<S: LineSink>(sink: &mut S, lines: &[&[u8]]) {
    for l in lines {
        if !l.is_empty() {
            sink.piece(l);
        }
        sink.end_line();
    }
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

/// A line of a pool of 60: words, `WS` runs that collapse, short lines of at most 3 characters, Cyrillic.
fn pool_line(k: u32) -> Vec<u8> {
    match k % 6 {
        0 => format!("let v{k} = f(x);").into_bytes(),
        1 => format!("  let v{} =\tf(x);  ", k - 1).into_bytes(),
        2 => format!("ж{k}").into_bytes(),
        3 => b"ab".to_vec(),
        4 => format!("fn item_{k}() {{}}").into_bytes(),
        _ => format!("{{ {k} }}").into_bytes(),
    }
}

/// A text content: an optional BOM, 0–120 lines of the pool (repeats and shared lines between two texts are common),
/// LF or CR LF, a final terminator or not.
fn text() -> impl Strategy<Value = Vec<u8>> {
    (
        any::<bool>(),
        prop::collection::vec((0u32..60, any::<bool>()), 0..120),
        any::<bool>(),
    )
        .prop_map(|(bom, lines, final_eol)| {
            let mut t = if bom {
                b"\xEF\xBB\xBF".to_vec()
            } else {
                Vec::new()
            };
            let n = lines.len();
            for (i, (k, crlf)) in lines.into_iter().enumerate() {
                t.extend_from_slice(&pool_line(k));
                if i + 1 < n || final_eol {
                    t.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
                }
            }
            t
        })
}

/// A valid fingerprint value's `(n_sketch, flag, distinct)`: below 64 values exact, at 64 exact or estimated, with
/// `distinct` from 65 to `u32::MAX`.
fn shape() -> impl Strategy<Value = (u8, bool, u32)> {
    prop_oneof![
        (0u8..64).prop_map(|n| (n, false, u32::from(n))),
        Just((64u8, false, 64u32)),
        (65u32..2_000).prop_map(|d| (64u8, true, d)),
        (2_000u32..=u32::MAX).prop_map(|d| (64u8, true, d)),
    ]
}

proptest! {
    #![proptest_config(test_config(64))]

    /// `exact` over two contents read through `MultisetSink` equals the definition over the pairs, and
    /// `sym = min(oin, nio)`.
    #[test]
    fn exact_measures_equal_the_naive_multiset(a in text(), b in text()) {
        let (ma, mb) = (multiset_of(&a), multiset_of(&b));
        let (na, nb) = (naive_multiset(&a), naive_multiset(&b));
        prop_assert_eq!(ma.weight(), weight(&na));
        let e = exact(&ma, &mb);
        let [oin, nio, sym] = naive_exact(&na, &nb);
        prop_assert!(same(e.oin, oin) && same(e.nio, nio) && same(e.sym, sym), "{:?}", e);
        prop_assert_eq!(e.sym, e.oin.min(e.nio));
    }

    /// Pairs with equal `fh` and different lengths are different elements of `M(X)`; `sym = min(oin, nio)`.
    #[test]
    fn sym_is_min_of_oin_and_nio_over_pairs(
        a in prop::collection::vec((0u64..3, 1u32..5), 0..40),
        b in prop::collection::vec((0u64..3, 1u32..5), 0..40),
    ) {
        let naive = |v: &[(u64, u32)]| {
            let mut m = Multiset::new();
            for &(fh, len) in v {
                *m.entry((fh, u64::from(len))).or_default() += 1;
            }
            m
        };
        let ma = LineMultiset::from_pairs(a.iter().copied()).unwrap();
        let mb = LineMultiset::from_pairs(b.iter().copied()).unwrap();
        let e = exact(&ma, &mb);
        let [oin, nio, sym] = naive_exact(&naive(&a), &naive(&b));
        prop_assert!(same(e.oin, oin) && same(e.nio, nio) && same(e.sym, sym), "{:?}", e);
        prop_assert_eq!(e.sym, e.oin.min(e.nio));
    }

    /// `hit` is the number of values of `S_A` equal to `sh(f)` of some fingerprint line f of B.
    #[test]
    fn hits_equal_the_naive_count(a in text(), b in text(), shared in 0usize..60) {
        let fa = fingerprint_of(&a);
        // B holds a part of A's lines too.
        let cut = a.len().min(shared * 8);
        let b: Vec<u8> = [&a[..cut], b"\n", &b[..]].concat();
        let mut sink = HitSink::new(&fa);
        analyse(&b, ObjectFormat::Sha1, None, &mut sink);
        let values: BTreeSet<u32> = fingerprint_lines(&b)
            .iter()
            .map(|f| (xxh3_64(f) & 0xFFFF_FFFF) as u32)
            .collect();
        let want = fa.sketch().iter().filter(|v| values.contains(v)).count();
        prop_assert_eq!(sink.hit() as usize, want);
    }

    /// `eoin = hit / #S_A`, `enio = min(1, eoin × D_A / D_B)` and `esym = min(eoin, enio)`, each 0 on its empty case,
    /// over fingerprints of every valid shape.
    #[test]
    fn estimates_equal_the_naive_definition(
        (n_a, f_a, d_a) in shape(),
        (n_b, f_b, d_b) in shape(),
        hit_frac in 0u32..=64,
    ) {
        let (old, new) = (fingerprint(n_a, f_a, d_a), fingerprint(n_b, f_b, d_b));
        let hit = hit_frac.min(u32::from(n_a));
        let e = estimates(&old, hit, &new);

        let (h, n, da, db) = (u128::from(hit), u128::from(n_a), u128::from(d_a), u128::from(d_b));
        let eoin = if n == 0 { (0, 1) } else { (h, n) };
        // eoin × D_A / D_B = hit × D_A / (n × D_B), clamped at 1.
        let enio = if db == 0 || n == 0 {
            (0, 1)
        } else if h * da >= n * db {
            (1, 1)
        } else {
            (h * da, n * db)
        };
        let esym = if eoin.0 * enio.1 <= enio.0 * eoin.1 { eoin } else { enio };
        prop_assert!(same(e.eoin, eoin) && same(e.enio, enio) && same(e.esym, esym), "{:?}", e);
    }
}

// --- the examples --------------------------------------------------------------------------------------------------

/// A measure whose denominator is 0 is 0: empty or `WS`-only sides, `w(A) = 0`, `w(B) = 0`.
#[test]
fn zero_denominators_give_zero() {
    let empty = multiset_of(b"");
    let blank = multiset_of(b"  \n\t\r\n\x0c\nab\n");
    let full = multiset_of(b"let x = compute(alpha);\nlet y = 2;\n");
    assert_eq!(empty.weight(), 0);
    assert_eq!(blank.weight(), 0);
    assert!(full.weight() > 0);
    let zero = |e: moirai_files::sketch::Exact| {
        e.oin == Ratio::ZERO && e.nio == Ratio::ZERO && e.sym == Ratio::ZERO
    };
    assert!(zero(exact(&empty, &empty)));
    assert!(zero(exact(&empty, &blank)));
    // `w(A) = 0`: oin's denominator; `w(B) = 0`: nio's.
    assert!(zero(exact(&blank, &full)));
    assert!(zero(exact(&full, &empty)));
    let none = LineMultiset::from_pairs([]).unwrap();
    assert!(zero(exact(&none, &none)));
    // The same content: every measure is 1.
    let e = exact(&full, &full);
    assert!(e.oin == Ratio::ONE && e.nio == Ratio::ONE && e.sym == Ratio::ONE);
}

/// `M(X)` exists for at most `EXACT_LIMIT` fingerprint lines and not for one more ([F20 §2.10.5]); lines that are
/// not fingerprint lines do not count.
#[test]
fn exact_limit_boundary() {
    let limit = EXACT_LIMIT as usize;
    let feed = |n: usize| {
        let mut sink = MultisetSink::new();
        sink.begin();
        for i in 0..n {
            // Every tenth line repeats an earlier one; a short line between, never counted.
            let l = format!("fingerprint line {}", if i % 10 == 9 { i - 1 } else { i });
            sink.piece(l.as_bytes());
            sink.end_line();
            sink.piece(b"ab");
            sink.end_line();
        }
        sink.finish()
    };
    let m = feed(limit).unwrap();
    assert!(m.weight() > 0);
    assert_eq!(feed(limit + 1), None);

    let pairs = |n: usize| (0..n).map(|i| ((i % 1_000) as u64, 7u32));
    let m = LineMultiset::from_pairs(pairs(limit)).unwrap();
    assert_eq!(m.weight(), 7 * limit as u64);
    assert_eq!(LineMultiset::from_pairs(pairs(limit + 1)), None);
}

/// `enio` is clamped at 1, is 0 when `D_B = 0`, and an empty `S_A` gives 0 everywhere ([F20 §2.10.2]).
#[test]
fn enio_clamps_and_empty_cases() {
    // `hit × D_A > #S_A × D_B`: 5 × 10 > 10 × 3.
    let (a, b) = (fingerprint(10, false, 10), fingerprint(3, false, 3));
    let e = estimates(&a, 5, &b);
    assert_eq!(e.eoin, Ratio::new(1, 2));
    assert_eq!(e.enio, Ratio::ONE);
    assert_eq!(e.esym, Ratio::new(1, 2));

    // Not clamped: 5 × 10 / (10 × 20) = 1/4.
    let e = estimates(&a, 5, &fingerprint(20, false, 20));
    assert_eq!(e.enio, Ratio::new(1, 4));
    assert_eq!(e.esym, Ratio::new(1, 4));

    // An estimated old side: 64 values, `D_A` = 1,000; all hit; `D_B` = 10: clamped.
    let big = fingerprint(64, true, 1_000);
    let e = estimates(&big, 64, &fingerprint(10, false, 10));
    assert_eq!(
        (e.eoin, e.enio, e.esym),
        (Ratio::ONE, Ratio::ONE, Ratio::ONE)
    );

    // `D_B = 0`: an empty new side.
    let e = estimates(&a, 5, &fingerprint(0, false, 0));
    assert_eq!(e.eoin, Ratio::new(1, 2));
    assert_eq!(e.enio, Ratio::ZERO);
    assert_eq!(e.esym, Ratio::ZERO);

    // An empty `S_A`.
    let e = estimates(&fingerprint(0, false, 0), 0, &b);
    assert_eq!(
        (e.eoin, e.enio, e.esym),
        (Ratio::ZERO, Ratio::ZERO, Ratio::ZERO)
    );

    // Through the sinks: a content against itself hits its whole sketch.
    let t = b"one line here\nanother line\nthird line\n";
    let fp = fingerprint_of(t);
    let mut sink = HitSink::new(&fp);
    analyse(t, ObjectFormat::Sha1, None, &mut sink);
    assert_eq!(sink.hit(), 3);
    let e = estimates(&fp, sink.hit(), &fp);
    assert_eq!(
        (e.eoin, e.enio, e.esym),
        (Ratio::ONE, Ratio::ONE, Ratio::ONE)
    );
}

/// [F20 §2.4]: every read attempt starts with `begin`, which forgets the hits of the earlier attempt and a half line,
/// after a closed line and in the middle of a line, and keeps `S_A`.
#[test]
fn hit_sink_begin_discards_an_earlier_attempt() {
    let earlier: [&[u8]; 4] = [
        b"earlier attempt line 0",
        b"earlier attempt line 1",
        b"earlier attempt line 2",
        b"earlier attempt line 3",
    ];
    // The first two later lines are in A, the third is not.
    let later: [&[u8]; 3] = [b"the line that stays", b"another line", b"a line A lacks"];
    let a = b"earlier attempt line 0\nearlier attempt line 1\nearlier attempt line 2\nearlier attempt line 3\n\
              the line that stays\nanother line\n";
    let fa = fingerprint_of(a);
    // `S_A` holds every fingerprint line of A.
    assert_eq!(fa.sketch().len(), 6);
    let mut fresh = HitSink::new(&fa);
    fresh.begin();
    feed(&mut fresh, &later);
    assert_eq!(fresh.hit(), 2);

    // After a closed line.
    let mut s = HitSink::new(&fa);
    s.begin();
    feed(&mut s, &earlier);
    assert_eq!(s.hit(), 4);
    s.begin();
    assert_eq!(s.hit(), 0);
    feed(&mut s, &later);
    assert_eq!(s.hit(), fresh.hit());

    // In the middle of a line, and after a pending space: the later attempt's first line is a hit only on its own.
    for half in [&b"half a li"[..], b"half a line \t"] {
        let mut s = HitSink::new(&fa);
        s.begin();
        feed(&mut s, &earlier);
        s.piece(half);
        s.begin();
        assert_eq!(s.hit(), 0);
        feed(&mut s, &later);
        assert_eq!(s.hit(), fresh.hit(), "{:?}", String::from_utf8_lossy(half));
    }
}
