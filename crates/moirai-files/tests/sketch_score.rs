//! WP-66: the sketch resemblance `r` ([F20 §2.10.3]) and the pair score with its reported containments
//! ([F20 §2.10.4]), each against a definition written from chapter 20 alone, in exact rational arithmetic, and against
//! part 1's measures (`exact`, `jaccard`, `estimates`).
//!
//! Every content here is synthetic and in memory.

use std::collections::BTreeSet;

use moirai_files::oid::ObjectFormat;
use moirai_files::r14::Ratio;
use moirai_files::sketch::{
    Basis, Estimates, Fingerprint, FingerprintSink, HitSink, LineMultiset, MultisetSink, PairScore,
    estimates, exact, jaccard, pair_score, resemblance, winnow,
};
use moirai_files::text::analyse;
use proptest::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

// --- the definitions, written from [F20 §1.2, §2.2, §2.5, §2.6, §2.10.3] alone -------------------------------------

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

/// The sketch of a text content: the 64 smallest distinct values `low(XXH3-64(f), 32)` of its fingerprint lines.
fn naive_sketch(b: &[u8]) -> BTreeSet<u32> {
    fingerprint_lines(b)
        .iter()
        .map(|f| (xxh3_64(f) & 0xFFFF_FFFF) as u32)
        .collect::<BTreeSet<u32>>()
        .into_iter()
        .take(64)
        .collect()
}

/// `r(A, B)` as an exact `num / den` pair: X is the `min(64, |S_A ∪ S_B|)` smallest values of the union, and
/// `r = |X ∩ S_A ∩ S_B| / |X|`, 0 when X is empty.
fn naive_r(sa: &BTreeSet<u32>, sb: &BTreeSet<u32>) -> (u128, u128) {
    let union: BTreeSet<u32> = sa.union(sb).copied().collect();
    let x: Vec<u32> = union.iter().copied().take(64.min(union.len())).collect();
    if x.is_empty() {
        return (0, 1);
    }
    let both = x
        .iter()
        .filter(|v| sa.contains(v) && sb.contains(v))
        .count();
    (both as u128, x.len() as u128)
}

/// Whether `r` equals `num / den` by value.
fn same(r: Ratio, (num, den): (u128, u128)) -> bool {
    u128::from(r.num()) * den == num * u128::from(r.den())
}

/// A fingerprint value field by field ([F20 §2.6.4], little-endian) whose sketch is `values`, at most 64 of them.
fn fingerprint_with(values: &BTreeSet<u32>) -> Fingerprint {
    let n = u8::try_from(values.len()).unwrap();
    assert!(n <= 64);
    let mut b = vec![1, 0, n, 0];
    for v in [10u32, 1_000, 500, u32::from(n)] {
        b.extend(v.to_le_bytes());
    }
    for v in values {
        b.extend(v.to_le_bytes());
    }
    Fingerprint::from_bytes(&b).unwrap()
}

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

/// `estimates(fp(A), hit, fp(B))` with `hit` counted while B streams.
fn estimates_of(a: &[u8], b: &[u8]) -> Estimates {
    let (fa, fb) = (fingerprint_of(a), fingerprint_of(b));
    let mut hit = HitSink::new(&fa);
    analyse(b, ObjectFormat::Sha1, None, &mut hit);
    estimates(&fa, hit.hit(), &fb)
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

/// A text with many distinct lines, so that its sketch is often full: lines `line number <k> of the text`.
fn long_text() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0u32..400, 0..160).prop_map(|ks| {
        let mut t = Vec::new();
        for k in ks {
            t.extend_from_slice(format!("line number {k} of the text\n").as_bytes());
        }
        t
    })
}

/// B holds a part of A's lines too: the first `shared × 8` bytes of A, a line feed, then B.
fn with_shared(a: &[u8], b: &[u8], shared: usize) -> Vec<u8> {
    let cut = a.len().min(shared * 8);
    [&a[..cut], b"\n", b].concat()
}

proptest! {
    #![proptest_config(test_config(256))]

    /// `r` over two sketches given as stored values equals the set computation: sketches of 0 to 64 values from a
    /// narrow range, so that they overlap and two full sketches have a union of more than 64 values.
    #[test]
    fn resemblance_equals_the_naive_set_computation(
        sa in prop::collection::btree_set(0u32..150, 0..=64),
        sb in prop::collection::btree_set(0u32..150, 0..=64),
    ) {
        let r = resemblance(&fingerprint_with(&sa), &fingerprint_with(&sb));
        prop_assert!(same(r, naive_r(&sa, &sb)), "{:?}", r);
        // The estimator is symmetric.
        prop_assert_eq!(r, resemblance(&fingerprint_with(&sb), &fingerprint_with(&sa)));
    }
}

proptest! {
    #![proptest_config(test_config(64))]

    /// `r` over the fingerprints of two contents read through `FingerprintSink` equals the set computation over the
    /// sketches written from [F20 §2.6] alone.
    #[test]
    fn resemblance_of_contents_equals_the_naive_set_computation(
        a in prop_oneof![text(), long_text()],
        b in prop_oneof![text(), long_text()],
        shared in 0usize..200,
    ) {
        let b = with_shared(&a, &b, shared);
        let r = resemblance(&fingerprint_of(&a), &fingerprint_of(&b));
        prop_assert!(same(r, naive_r(&naive_sketch(&a), &naive_sketch(&b))), "{:?}", r);
    }

    /// On the exact basis the score is `max(sym, J)` with J, `sym` without it, and the containments are `exact`'s
    /// `oin` and `nio`.
    #[test]
    fn pair_score_exact_basis_equals_the_measures(
        a in text(),
        b in text(),
        shared in 0usize..60,
        with_j in any::<bool>(),
    ) {
        let b = with_shared(&a, &b, shared);
        let (ma, mb) = (multiset_of(&a), multiset_of(&b));
        let (wa, wb) = (winnow(&a).unwrap(), winnow(&b).unwrap());
        let est = estimates_of(&a, &b);
        let ps = pair_score(Some((&ma, &mb)), with_j.then_some((&wa, &wb)), &est);
        let e = exact(&ma, &mb);
        let want = if with_j {
            let j = jaccard(&wa, &wb);
            if j > e.sym { j } else { e.sym }
        } else {
            e.sym
        };
        prop_assert_eq!(ps.score, want);
        prop_assert_eq!(ps.oin, e.oin);
        prop_assert_eq!(ps.nio, e.nio);
        prop_assert_eq!(ps.basis, Basis::Exact);
    }

    /// On the estimate basis the score is `esym` and the containments are `eoin` and `enio`, whether or not the
    /// winnowing sets are given.
    #[test]
    fn pair_score_estimate_basis_equals_the_measures(
        a in text(),
        b in text(),
        shared in 0usize..60,
        with_j in any::<bool>(),
    ) {
        let b = with_shared(&a, &b, shared);
        let (wa, wb) = (winnow(&a).unwrap(), winnow(&b).unwrap());
        let est = estimates_of(&a, &b);
        let ps = pair_score(None, with_j.then_some((&wa, &wb)), &est);
        prop_assert_eq!(ps.score, est.esym);
        prop_assert_eq!(ps.oin, est.eoin);
        prop_assert_eq!(ps.nio, est.enio);
        prop_assert_eq!(ps.basis, Basis::Estimate);
    }
}

// --- the examples --------------------------------------------------------------------------------------------------

/// X is the 64 smallest values of the union, never the whole union: `S_A = {1..=64}` and `S_B = {33..=96}` give
/// X = `{1..=64}`, of which `{33..=64}` lie in both sketches, so `r = 32/64 = 1/2`; over the whole union of 96 values
/// it would be 1/3.
#[test]
fn resemblance_keeps_the_64_smallest_of_the_union() {
    let sa: BTreeSet<u32> = (1..=64).collect();
    let sb: BTreeSet<u32> = (33..=96).collect();
    let r = resemblance(&fingerprint_with(&sa), &fingerprint_with(&sb));
    assert_eq!(r, Ratio::new(1, 2));
    assert_ne!(r, Ratio::new(1, 3));
    // A union of at most 64 values is X itself: `{1..=40}` and `{21..=60}` share 20 of 60.
    let sa: BTreeSet<u32> = (1..=40).collect();
    let sb: BTreeSet<u32> = (21..=60).collect();
    let r = resemblance(&fingerprint_with(&sa), &fingerprint_with(&sb));
    assert_eq!(r, Ratio::new(1, 3));
}

/// X empty gives 0; disjoint sketches give 0; identical sketches give 1.
#[test]
fn resemblance_empty_disjoint_and_identical() {
    let empty = BTreeSet::new();
    let some: BTreeSet<u32> = [3, 7, 11].into_iter().collect();
    let other: BTreeSet<u32> = [4, 8, 12].into_iter().collect();
    let full: BTreeSet<u32> = (100..164).collect();
    let r = |a: &BTreeSet<u32>, b: &BTreeSet<u32>| {
        resemblance(&fingerprint_with(a), &fingerprint_with(b))
    };
    assert_eq!(r(&empty, &empty), Ratio::ZERO);
    assert_eq!(r(&empty, &some), Ratio::ZERO);
    assert_eq!(r(&some, &empty), Ratio::ZERO);
    assert_eq!(r(&some, &other), Ratio::ZERO);
    assert_eq!(r(&some, &some), Ratio::ONE);
    assert_eq!(r(&full, &full), Ratio::ONE);
    // Contents without a fingerprint line have an empty sketch.
    assert_eq!(
        resemblance(&fingerprint_of(b"ab\n{}\n"), &fingerprint_of(b"")),
        Ratio::ZERO
    );
}

/// `words` words `w0 w1 …`, `per_line` to a line.
fn wrapped(words: usize, per_line: usize) -> Vec<u8> {
    let mut t = Vec::new();
    for i in 0..words {
        t.extend_from_slice(format!("w{i}").as_bytes());
        t.push(if (i + 1) % per_line == 0 || i + 1 == words {
            b'\n'
        } else {
            b' '
        });
    }
    t
}

/// The exact score is the larger of `sym` and J: the same tokens wrapped into other lines have J = 1 and `sym` = 0,
/// and the same lines in reverse order have `sym` = 1 and J below 1.
#[test]
fn pair_score_takes_the_larger_of_sym_and_j() {
    let zero = Estimates {
        eoin: Ratio::ZERO,
        enio: Ratio::ZERO,
        esym: Ratio::ZERO,
    };
    // J above sym.
    let (a, b) = (wrapped(48, 6), wrapped(48, 8));
    let (ma, mb) = (multiset_of(&a), multiset_of(&b));
    let (wa, wb) = (winnow(&a).unwrap(), winnow(&b).unwrap());
    let (sym, j) = (exact(&ma, &mb).sym, jaccard(&wa, &wb));
    assert!(j > sym, "J {j:?}, sym {sym:?}");
    let ps = pair_score(Some((&ma, &mb)), Some((&wa, &wb)), &zero);
    assert_eq!(ps.score, j);
    assert_eq!(ps.basis, Basis::Exact);

    // sym above J.
    let a = wrapped(48, 6);
    let mut lines: Vec<&[u8]> = a.split_inclusive(|&c| c == b'\n').collect();
    lines.reverse();
    let b = lines.concat();
    let (ma, mb) = (multiset_of(&a), multiset_of(&b));
    let (wa, wb) = (winnow(&a).unwrap(), winnow(&b).unwrap());
    let (sym, j) = (exact(&ma, &mb).sym, jaccard(&wa, &wb));
    assert_eq!(sym, Ratio::ONE);
    assert!(j < sym, "J {j:?}, sym {sym:?}");
    let ps = pair_score(Some((&ma, &mb)), Some((&wa, &wb)), &zero);
    assert_eq!(ps.score, sym);
}

/// Beyond the exact limit (`lines` absent) the score is `esym` even when the winnowing sets give a larger J.
#[test]
fn pair_score_ignores_j_beyond_the_exact_limit() {
    let a = wrapped(48, 6);
    let wa = winnow(&a).unwrap();
    assert!(!wa.is_empty());
    assert_eq!(jaccard(&wa, &wa), Ratio::ONE);
    let est = Estimates {
        eoin: Ratio::new(1, 2),
        enio: Ratio::new(1, 4),
        esym: Ratio::new(1, 4),
    };
    let ps = pair_score(None, Some((&wa, &wa)), &est);
    assert_eq!(
        ps,
        PairScore {
            score: Ratio::new(1, 4),
            oin: Ratio::new(1, 2),
            nio: Ratio::new(1, 4),
            basis: Basis::Estimate,
        }
    );
}
