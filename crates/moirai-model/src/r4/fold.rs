//! `fold_v1` and canonical equivalence ([F20 §3.1], §3.4; [80 §2.10] P6), derived by definition from the UCD text
//! ([`crate::r4::ucd`]): full canonical decomposition by the recursive application of `UnicodeData.txt`'s canonical
//! mappings with Hangul syllables decomposed arithmetically, canonical ordering by a stable sort of every run of
//! non-zero combining classes, and full case folding by the `C` and `F` lines of `CaseFolding.txt`. No table is
//! generated; every step reads the parsed lines.

use crate::r4::ucd::{Ucd, ucd};

/// Hangul constants of the Unicode Standard §3.12, as [F20 §3.1] lists them.
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = 588;
const S_COUNT: u32 = 11_172;

/// Appends the full canonical decomposition of `c`: Hangul syllables arithmetically, every other code point by its
/// mapping applied recursively; a code point without a mapping is itself.
// spec: [F20 §3.1] NFD full canonical decomposition
fn decompose_into(u: &Ucd, c: u32, out: &mut Vec<u32>) {
    if (S_BASE..S_BASE + S_COUNT).contains(&c) {
        let s = c - S_BASE;
        out.push(L_BASE + s / N_COUNT);
        out.push(V_BASE + (s % N_COUNT) / T_COUNT);
        let t = s % T_COUNT;
        if t != 0 {
            out.push(T_BASE + t);
        }
        return;
    }
    match u.decomposition(c) {
        Some(m) => {
            for &d in m {
                decompose_into(u, d, out);
            }
        }
        None => out.push(c),
    }
}

/// Canonical ordering: in every maximal run of code points whose combining class is not 0, a stable sort by class.
// spec: [F20 §3.1] NFD canonical ordering
fn canonical_order(u: &Ucd, v: &mut [u32]) {
    let mut i = 0;
    while i < v.len() {
        if u.ccc(v[i]) == 0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < v.len() && u.ccc(v[i]) != 0 {
            i += 1;
        }
        v[start..i].sort_by_key(|&c| u.ccc(c));
    }
}

/// NFD of a code-point sequence.
// spec: [F20 §3.1] NFD
pub fn nfd_cps(u: &Ucd, x: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(x.len());
    for &c in x {
        decompose_into(u, c, &mut out);
    }
    canonical_order(u, &mut out);
    out
}

/// CF: every code point replaced by its `C` or `F` folding, if it has one.
// spec: [F20 §3.1] CF full case folding
pub fn cf_cps(u: &Ucd, x: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(x.len());
    for &c in x {
        match u.folding(c) {
            Some(m) => out.extend_from_slice(m),
            None => out.push(c),
        }
    }
    out
}

/// The code points of a text.
pub fn cps(x: &str) -> Vec<u32> {
    x.chars().map(u32::from).collect()
}

/// The text of code points, which are scalar values by construction (the UCD maps scalar values to scalar values).
pub fn text(v: &[u32]) -> String {
    v.iter()
        .map(|&c| char::from_u32(c).expect("NFD and CF map scalar values to scalar values"))
        .collect()
}

/// `fold_v1(x) = NFD(CF(NFD(x)))` at Unicode 17.0.0.
// spec: [F20 §3.1]
pub fn fold_v1(x: &str) -> String {
    let u = ucd();
    text(&nfd_cps(u, &cf_cps(u, &nfd_cps(u, &cps(x)))))
}

/// NFD of a text ([F20 §3.1]).
pub fn nfd(x: &str) -> String {
    text(&nfd_cps(ucd(), &cps(x)))
}

/// `ceq(a, b)` ⟺ `NFD(a) = NFD(b)`: canonical equivalence, no folding.
// spec: [F20 §3.4]
pub fn ceq(a: &str, b: &str) -> bool {
    let u = ucd();
    nfd_cps(u, &cps(a)) == nfd_cps(u, &cps(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canon::fixtures::json;
    use crate::r4::ucd;
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    fn r4_cases(file: &str) -> Vec<crate::canon::fixtures::Case> {
        crate::r4::tests::cases(file)
    }

    fn parse_cps(s: &str) -> Vec<u32> {
        s.split_ascii_whitespace()
            .map(|t| u32::from_str_radix(t.strip_prefix("U+").expect("U+XXXX"), 16).expect("hex"))
            .collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// Every case of `fixtures/r4/cases/fold.cases`.
    #[test]
    fn fold_cases_pass() {
        let cases = r4_cases("fold.cases");
        assert!(cases.len() >= 37, "{} fold cases", cases.len());
        let mut n = 0;
        for c in &cases {
            let input = json(c.line("input-text").expect("input-text"));
            match c.line("function").expect("function") {
                "fold_v1" => {
                    assert_eq!(
                        cps(&input),
                        parse_cps(c.line("input-cps").unwrap()),
                        "{}",
                        c.id
                    );
                    assert_eq!(
                        input.as_bytes(),
                        unhex(c.line("input-utf8").unwrap()).as_slice(),
                        "{}",
                        c.id
                    );
                    let got = fold_v1(&input);
                    assert_eq!(
                        cps(&got),
                        parse_cps(c.line("output-cps").unwrap()),
                        "{}: fold_v1({input:?})",
                        c.id
                    );
                    assert_eq!(
                        got.as_bytes(),
                        unhex(c.line("output-utf8").unwrap()).as_slice(),
                        "{}",
                        c.id
                    );
                }
                "ceq" => {
                    let b = json(c.line("input2-text").expect("input2-text"));
                    let want = c.line("output") == Some("true");
                    assert_eq!(ceq(&input, &b), want, "{}: ceq({input:?}, {b:?})", c.id);
                }
                f => panic!("{}: unknown function {f}", c.id),
            }
            n += 1;
        }
        assert_eq!(n, cases.len());
    }

    /// [F20 §3.2]'s conformance of NFD: the NFD column of every line of `NormalizationTest.txt` (c3 = NFD(c1) = NFD(c2)
    /// = NFD(c3), c5 = NFD(c4) = NFD(c5)), and NFD(X) = X for every scalar value that part 1 does not list.
    #[test]
    fn nfd_conforms_to_normalization_test() {
        let u = ucd::ucd();
        let text = String::from_utf8(ucd::read_pinned("NormalizationTest.txt")).unwrap();
        let mut part = String::new();
        let mut listed = BTreeSet::new();
        let mut lines = 0usize;
        for line in text.lines() {
            if let Some(p) = line.strip_prefix('@') {
                part = p.split_whitespace().next().unwrap_or("").to_string();
                continue;
            }
            let body = line.split('#').next().unwrap_or("").trim();
            if body.is_empty() {
                continue;
            }
            let cols: Vec<Vec<u32>> = body
                .split(';')
                .take(5)
                .map(|s| {
                    s.split_ascii_whitespace()
                        .map(|h| u32::from_str_radix(h, 16).unwrap())
                        .collect()
                })
                .collect();
            let (c1, c2, c3, c4, c5) = (&cols[0], &cols[1], &cols[2], &cols[3], &cols[4]);
            for x in [c1, c2, c3] {
                assert_eq!(&nfd_cps(u, x), c3, "NFD of {x:X?} ({part})");
            }
            for x in [c4, c5] {
                assert_eq!(&nfd_cps(u, x), c5, "NFD of {x:X?} ({part})");
            }
            if part == "Part1" {
                listed.insert(c1[0]);
            }
            lines += 1;
        }
        assert!(lines > 19_000, "{lines} test lines");
        let mut others = 0u32;
        for c in (0..=0x10FFFFu32).filter(|c| !(0xD800..0xE000).contains(c)) {
            if !listed.contains(&c) {
                assert_eq!(nfd_cps(u, &[c]), vec![c], "NFD(U+{c:04X}) ≠ itself");
                others += 1;
            }
        }
        assert!(others > 1_000_000);
    }

    /// `fold_v1` over every scalar value is in NFD and folds again to itself, and its size is bounded ([PLAN §3.2] item 9:
    /// "sized in WP-92"): the pinned UCD 17.0.0 input is 2,285,748 bytes; the maps hold 2,081 canonical
    /// decompositions, 968 non-zero combining classes and 1,585 `C`/`F` foldings in at most 135,808 bytes of heap
    /// (asserted below at 192 KiB); 14,397 scalar values fold to something else. Measured on the development machine
    /// in the test profile (recorded in the WP-92 report, not asserted: timings are never a gate): about 57 ms to read
    /// and check the pins, 71 ms to parse, and 2.4 s to fold all 1,112,064 scalar values.
    #[test]
    fn fold_over_every_scalar_and_its_size() {
        let u = ucd::ucd();
        let mut changed = 0u32;
        let mut not_idempotent = Vec::new();
        for c in (0..=0x10FFFFu32).filter(|c| !(0xD800..0xE000).contains(c)) {
            let f = nfd_cps(u, &cf_cps(u, &nfd_cps(u, &[c])));
            assert_eq!(nfd_cps(u, &f), f, "fold_v1(U+{c:04X}) is not in NFD");
            if f != [c] {
                changed += 1;
                let ff = nfd_cps(u, &cf_cps(u, &nfd_cps(u, &f)));
                if ff != f {
                    not_idempotent.push(c);
                }
            }
        }
        assert!(
            not_idempotent.is_empty(),
            "fold_v1 is not idempotent at {not_idempotent:X?}"
        );
        assert_eq!(changed, 14_397, "scalar values fold_v1 changes");
        assert_eq!(u.cost.bytes, 2_285_748, "the pinned input's size");
        assert_eq!(
            (u.cost.decompositions, u.cost.combining, u.cost.foldings),
            (2_081, 968, 1_585)
        );
        assert!(
            u.cost.heap_bytes <= 192 * 1024,
            "{} B of heap",
            u.cost.heap_bytes
        );
    }

    fn arb_text() -> impl Strategy<Value = String> {
        // Letters with case, combining marks, Hangul, the characters of the fixture edge cases and ASCII.
        let pool: Vec<char> =
            "aAbBzZ.-_/ßẞİıIiǅǄǆΩΩÅÅﬃ한각\u{0301}\u{0323}\u{0307}\u{0345}\u{1100}\u{1161}\u{11A8}é"
                .chars()
                .collect();
        proptest::collection::vec(proptest::sample::select(pool), 0..12)
            .prop_map(|v| v.into_iter().collect())
    }

    /// Canonically equivalent texts fold alike; `ceq` is an equivalence; fold is in NFD; ASCII folds to lower case (the
    /// tier's fixed seed, [`crate::r4::tests::runner`]).
    #[test]
    fn fold_respects_canonical_equivalence() {
        crate::r4::tests::runner(256)
            .run(&(arb_text(), arb_text()), |(a, b)| {
                prop_assert!(ceq(&a, &a));
                prop_assert_eq!(ceq(&a, &b), ceq(&b, &a));
                if ceq(&a, &b) {
                    prop_assert_eq!(fold_v1(&a), fold_v1(&b));
                }
                let f = fold_v1(&a);
                prop_assert_eq!(nfd(&f), f.clone());
                prop_assert_eq!(fold_v1(&f), f);
                let ascii: String = a.chars().filter(char::is_ascii).collect();
                prop_assert_eq!(fold_v1(&ascii), ascii.to_ascii_lowercase());
                Ok(())
            })
            .unwrap();
    }
}
