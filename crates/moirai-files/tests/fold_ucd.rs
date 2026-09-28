//! WP-61: `fold_v1` and NFD against the pinned Unicode 17.0.0 data ([F20 §3.2]; PLAN §6.2 R6; review a1-S R6).
//!
//! - The pins: each file of `fixtures/ucd/17.0.0/` has the size and SHA-256 its `INDEX.md` gives.
//! - Every scalar value U+0000–U+10FFFF except the surrogates: the generated tables reproduce `UnicodeData.txt`'s
//!   canonical combining class and full canonical decomposition, and `CaseFolding.txt`'s `C` and `F` lines exactly
//!   (a code point with neither folds to itself); `fold_v1` and NFD of the one-scalar string equal a naive derivation
//!   from the two files.
//! - `NormalizationTest.txt`: the NFD column of every line (c3 = NFD(c1) = NFD(c2) = NFD(c3), c5 = NFD(c4) = NFD(c5)),
//!   and NFD(X) = X for every scalar value X not listed in part 1. The naive derivation passes the same file, so it is
//!   checked before it is trusted; `fold_v1` of every column equals the naive fold.
//! - Random strings over the code points the data touches: `fold_v1`, NFD and the predicates against the naive fold.
//! - Up to three starters each followed by up to 80 marks (every code point with a non-zero class, or whose
//!   decomposition begins with one; U+0345 drawn often): runs longer than a pipeline stage's 16-entry inline buffer,
//!   so both stages spill to the heap and drain back, against the naive fold.
//!
//! The naive derivation is written here from [F20 §3.1] alone: recursive decomposition through hash maps, Hangul by the
//! formula of the Unicode Standard §3.12, canonical ordering by the exchange rule of D109 (swap adjacent A B while
//! ccc(A) > ccc(B) > 0), and CF from the `C` and `F` lines. It shares no code with the generator or the product.
//!
//! The files are compiled in (`include_str!`): a product crate's tests open no file ([OS/README §2.5], PLAN §6.2 R18).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use moirai_files::fold::{
    UNICODE_VERSION, canonical_combining_class, canonical_decomposition, case_fold, ceq, fold_cmp,
    fold_eq, fold_matches, fold_v1, nfd,
};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

const UNICODE_DATA: &str = include_str!("../../../fixtures/ucd/17.0.0/UnicodeData.txt");
const CASE_FOLDING: &str = include_str!("../../../fixtures/ucd/17.0.0/CaseFolding.txt");
const NORMALIZATION_TEST: &str = include_str!("../../../fixtures/ucd/17.0.0/NormalizationTest.txt");
const INDEX: &str = include_str!("../../../fixtures/ucd/17.0.0/INDEX.md");

// --- the pins ------------------------------------------------------------------------------------------------

#[test]
fn the_files_match_their_pins() {
    let files = [
        ("UnicodeData.txt", UNICODE_DATA),
        ("CaseFolding.txt", CASE_FOLDING),
        ("NormalizationTest.txt", NORMALIZATION_TEST),
    ];
    let mut pinned = HashMap::new();
    for line in INDEX.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let name = cells[1].trim_matches('`');
        let bytes: usize = cells[2].parse().expect("a byte count");
        let sha = cells[3].trim_matches('`');
        assert!(
            pinned.insert(name, (bytes, sha)).is_none(),
            "{name} pinned twice"
        );
    }
    assert_eq!(
        pinned.len(),
        files.len(),
        "INDEX.md pins exactly the three files"
    );
    for (name, text) in files {
        let (bytes, sha) = pinned[name];
        assert_eq!(text.len(), bytes, "{name}: size");
        let digest: String = Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(digest, sha, "{name}: SHA-256");
    }
    assert!(UNICODE_DATA.lines().count() > 40_000);
    assert!(CASE_FOLDING.starts_with("# CaseFolding-17.0.0.txt"));
    assert!(NORMALIZATION_TEST.starts_with("# NormalizationTest-17.0.0.txt"));
    assert_eq!(UNICODE_VERSION, (17, 0, 0));
}

// --- the naive derivation ------------------------------------------------------------------------------------

struct Naive {
    /// Non-zero canonical combining classes.
    ccc: HashMap<u32, u8>,
    /// One level of canonical decomposition (field 5 without a tag).
    dm: HashMap<u32, Vec<u32>>,
    /// C and F foldings.
    cf: HashMap<u32, Vec<u32>>,
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn naive() -> &'static Naive {
    static N: OnceLock<Naive> = OnceLock::new();
    N.get_or_init(|| {
        let mut n = Naive {
            ccc: HashMap::new(),
            dm: HashMap::new(),
            cf: HashMap::new(),
        };
        for line in UNICODE_DATA.lines() {
            let f: Vec<&str> = line.split(';').collect();
            let cp = hex(f[0]);
            let ccc: u8 = f[3].parse().expect("class");
            if ccc != 0 {
                n.ccc.insert(cp, ccc);
            }
            if !f[5].is_empty() && !f[5].starts_with('<') {
                n.dm.insert(cp, f[5].split(' ').map(hex).collect());
            }
        }
        for line in CASE_FOLDING.lines() {
            let line = line.split('#').next().unwrap_or("");
            let f: Vec<&str> = line.split(';').map(str::trim).collect();
            if f.len() >= 3 && (f[1] == "C" || f[1] == "F") {
                let prev = n.cf.insert(hex(f[0]), f[2].split(' ').map(hex).collect());
                assert!(prev.is_none(), "one C or F line per code point");
            }
        }
        n
    })
}

impl Naive {
    fn class(&self, cp: u32) -> u8 {
        self.ccc.get(&cp).copied().unwrap_or(0)
    }

    fn decompose(&self, cp: u32, out: &mut Vec<u32>) {
        // The Unicode Standard §3.12, Hangul syllable decomposition.
        let s_index = cp.wrapping_sub(0xAC00);
        if s_index < 19 * 21 * 28 {
            let l = 0x1100 + s_index / (21 * 28);
            let v = 0x1161 + (s_index % (21 * 28)) / 28;
            let t = 0x11A7 + s_index % 28;
            out.push(l);
            out.push(v);
            if t != 0x11A7 {
                out.push(t);
            }
        } else if let Some(m) = self.dm.get(&cp) {
            for &d in m {
                self.decompose(d, out);
            }
        } else {
            out.push(cp);
        }
    }

    fn nfd(&self, s: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        for &c in s {
            self.decompose(c, &mut out);
        }
        // D109: exchange adjacent A B with ccc(A) > ccc(B) > 0 until none is left. Each code point carries its class,
        // so the quadratic exchange passes over long runs of marks look nothing up.
        let mut v: Vec<(u8, u32)> = out.into_iter().map(|c| (self.class(c), c)).collect();
        let mut swapped = true;
        while swapped {
            swapped = false;
            for i in 1..v.len() {
                let (a, b) = (v[i - 1].0, v[i].0);
                if a > b && b > 0 {
                    v.swap(i - 1, i);
                    swapped = true;
                }
            }
        }
        v.into_iter().map(|(_, c)| c).collect()
    }

    fn fold(&self, s: &[u32]) -> Vec<u32> {
        let d = self.nfd(s);
        let mut cf = Vec::with_capacity(d.len());
        for c in d {
            match self.cf.get(&c) {
                Some(m) => cf.extend_from_slice(m),
                None => cf.push(c),
            }
        }
        self.nfd(&cf)
    }
}

fn text(s: &[u32]) -> String {
    s.iter()
        .map(|&c| char::from_u32(c).expect("scalar"))
        .collect()
}

fn scalars() -> impl Iterator<Item = char> {
    (0..=0x10FFFFu32).filter_map(char::from_u32)
}

// --- every scalar value --------------------------------------------------------------------------------------

#[test]
fn tables_reproduce_the_ucd_for_every_scalar_value() {
    let n = naive();
    let mut bad = Vec::new();
    let mut count = 0u32;
    for c in scalars() {
        count += 1;
        let cp = c as u32;
        if canonical_combining_class(c) != n.class(cp) {
            bad.push(format!(
                "U+{cp:04X}: class {}",
                canonical_combining_class(c)
            ));
        }
        let mut want = Vec::new();
        n.decompose(cp, &mut want);
        let got: Option<Vec<u32>> =
            canonical_decomposition(c).map(|d| d.map(|x| x as u32).collect());
        let want = (want != [cp]).then_some(want);
        if got != want {
            bad.push(format!(
                "U+{cp:04X}: decomposition {got:X?}, want {want:X?}"
            ));
        }
        let got: Option<Vec<u32>> = case_fold(c).map(|m| m.iter().map(|&x| x as u32).collect());
        if got.as_ref() != n.cf.get(&cp) {
            bad.push(format!("U+{cp:04X}: folding {got:X?}"));
        }
        if bad.len() > 20 {
            break;
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    assert_eq!(count, 0x110000 - 0x800, "every scalar value");
    // Every C and F line was reached through its code point above; count them to be sure.
    let lines = CASE_FOLDING
        .lines()
        .filter(|l| l.contains("; C; ") || l.contains("; F; "))
        .count();
    assert_eq!(lines, n.cf.len());
    assert_eq!(scalars().filter(|&c| case_fold(c).is_some()).count(), lines);
}

#[test]
fn fold_v1_and_nfd_equal_the_naive_derivation_for_every_scalar_value() {
    let n = naive();
    let mut bad = Vec::new();
    let mut buf = String::new();
    for c in scalars() {
        buf.clear();
        buf.push(c);
        let cp = [c as u32];
        let want_fold = text(&n.fold(&cp));
        let want_nfd = text(&n.nfd(&cp));
        if fold_v1(&buf) != want_fold || nfd(&buf) != want_nfd {
            bad.push(format!("U+{:04X}", c as u32));
        }
        if !fold_matches(&want_fold, &buf)
            || fold_cmp(&buf, &want_fold).is_ne()
            || !fold_eq(&buf, &want_fold)
        {
            bad.push(format!("U+{:04X}: predicates", c as u32));
        }
        if bad.len() > 20 {
            break;
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

// --- NormalizationTest.txt -------------------------------------------------------------------------------------

/// The five columns of every test line, with the part it belongs to.
fn normalization_lines() -> Vec<(u8, [Vec<u32>; 5])> {
    let mut part = 0u8;
    let mut out = Vec::new();
    for line in NORMALIZATION_TEST.lines() {
        if let Some(p) = line.strip_prefix("@Part") {
            part = p[..1].parse().expect("part number");
            continue;
        }
        let data = line.split('#').next().unwrap_or("").trim();
        if data.is_empty() {
            continue;
        }
        let cols: Vec<Vec<u32>> = data
            .split(';')
            .take(5)
            .map(|c| c.split(' ').map(hex).collect())
            .collect();
        out.push((part, cols.try_into().expect("five columns")));
    }
    out
}

#[test]
fn nfd_passes_normalization_test() {
    let n = naive();
    let lines = normalization_lines();
    assert!(lines.len() > 19_000, "{} lines", lines.len());
    let mut part1 = HashSet::new();
    let mut bad = Vec::new();
    for (part, cols) in &lines {
        if *part == 1 {
            assert_eq!(cols[0].len(), 1);
            part1.insert(cols[0][0]);
        }
        let [c1, c2, c3, c4, c5] = cols.each_ref().map(|c| text(c));
        // The naive derivation first, so it is known to be right before it judges fold_v1.
        for (x, want) in [
            (&cols[0], &cols[2]),
            (&cols[1], &cols[2]),
            (&cols[2], &cols[2]),
            (&cols[3], &cols[4]),
            (&cols[4], &cols[4]),
        ] {
            assert_eq!(&n.nfd(x), want, "naive NFD of {x:X?}");
        }
        for (x, want) in [(&c1, &c3), (&c2, &c3), (&c3, &c3), (&c4, &c5), (&c5, &c5)] {
            if nfd(x) != *want {
                bad.push(format!(
                    "NFD({:X?})",
                    x.chars().map(|c| c as u32).collect::<Vec<_>>()
                ));
            }
        }
        // Canonically equivalent columns fold alike, and fold_v1 is the naive fold on every column.
        let f = fold_v1(&c1);
        for (x, raw) in [(&c1, &cols[0]), (&c2, &cols[1]), (&c3, &cols[2])] {
            if fold_v1(x) != text(&n.fold(raw)) || fold_v1(x) != f {
                bad.push(format!("fold_v1({:X?})", raw));
            }
        }
        if !ceq(&c1, &c2) || !ceq(&c3, &c1) || !ceq(&c4, &c5) {
            bad.push(format!("ceq on {:X?}", cols[0]));
        }
        if bad.len() > 20 {
            break;
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    // Part 1's rule: NFD(X) = X for every scalar value X it does not list.
    let mut buf = String::new();
    let mut unlisted = 0u32;
    for c in scalars().filter(|&c| !part1.contains(&(c as u32))) {
        unlisted += 1;
        buf.clear();
        buf.push(c);
        assert_eq!(nfd(&buf), buf, "U+{:04X} is not in part 1", c as u32);
    }
    assert!(unlisted > 1_000_000 && part1.len() > 10_000);
}

/// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs 16
/// times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is printed).
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

// --- random strings ---------------------------------------------------------------------------------------------

/// The code points the data touches (classes, decompositions, foldings and their targets), a slice of Hangul and
/// ASCII: every path of the pipeline.
fn pool() -> &'static Vec<char> {
    static P: OnceLock<Vec<char>> = OnceLock::new();
    P.get_or_init(|| {
        let n = naive();
        let mut v: Vec<u32> = n
            .ccc
            .keys()
            .chain(n.dm.keys())
            .chain(n.cf.keys())
            .chain(n.cf.values().flatten())
            .chain(n.dm.values().flatten())
            .copied()
            .chain((0xAC00..0xAC00 + 60).chain(0xD7A0..=0xD7A3))
            .chain(0x20..0x7F)
            .collect();
        v.sort_unstable();
        v.dedup();
        v.into_iter().filter_map(char::from_u32).collect()
    })
}

fn strings() -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::sample::select(pool().as_slice()), 0..16)
        .prop_map(|v| v.into_iter().collect())
}

/// The starters of the pool: class 0 (ASCII, decomposing letters such as U+1FB3, F-folding letters such as U+00DF
/// and U+FB03, Hangul syllables).
fn starters() -> &'static Vec<char> {
    static S: OnceLock<Vec<char>> = OnceLock::new();
    S.get_or_init(|| {
        let n = naive();
        pool()
            .iter()
            .copied()
            .filter(|&c| {
                n.class(c as u32) == 0
                    && n.nfd(&[c as u32]).first().is_some_and(|&d| n.class(d) == 0)
            })
            .collect()
    })
}

/// Every mark: each code point with a non-zero class (U+0345 among them), and each code point whose decomposition
/// begins with one (U+0344, U+0F73, U+0F75, U+0F81, U+1D15E and the like).
fn marks() -> &'static Vec<char> {
    static M: OnceLock<Vec<char>> = OnceLock::new();
    M.get_or_init(|| {
        let n = naive();
        let mut v: Vec<u32> = n
            .ccc
            .keys()
            .copied()
            .chain(
                n.dm.keys()
                    .copied()
                    .filter(|&cp| n.nfd(&[cp]).first().is_some_and(|&d| n.class(d) != 0)),
            )
            .collect();
        v.sort_unstable();
        v.dedup();
        v.into_iter().filter_map(char::from_u32).collect()
    })
}

/// Up to three blocks of a starter followed by 0–80 marks: runs longer than the 16 code points a pipeline stage holds
/// inline, so both stages spill to the heap and drain back. U+0345 is drawn often, since its fold (a starter) moves
/// with the run's order.
fn long_mark_runs() -> impl Strategy<Value = String> {
    let mark = prop_oneof![
        3 => proptest::sample::select(marks().as_slice()),
        1 => Just('\u{345}'),
    ];
    let block = (
        proptest::sample::select(starters().as_slice()),
        proptest::collection::vec(mark, 0..=80),
    );
    proptest::collection::vec(block, 1..=3).prop_map(|blocks| {
        let mut s = String::new();
        for (starter, run) in blocks {
            s.push(starter);
            s.extend(run);
        }
        s
    })
}

proptest! {
    #![proptest_config(test_config(512))]

    #[test]
    fn random_strings_fold_as_the_naive_derivation(x in strings(), y in strings()) {
        let n = naive();
        let raw = |s: &str| s.chars().map(|c| c as u32).collect::<Vec<u32>>();
        let (fx, fy) = (text(&n.fold(&raw(&x))), text(&n.fold(&raw(&y))));
        prop_assert_eq!(fold_v1(&x), fx.clone());
        prop_assert_eq!(nfd(&x), text(&n.nfd(&raw(&x))));
        prop_assert_eq!(fold_eq(&x, &y), fx == fy);
        prop_assert_eq!(fold_cmp(&x, &y), fx.as_bytes().cmp(fy.as_bytes()));
        prop_assert_eq!(ceq(&x, &y), n.nfd(&raw(&x)) == n.nfd(&raw(&y)));
        // Concatenation: a fold boundary never depends on what follows beyond the next starter.
        let xy = format!("{x}{y}");
        prop_assert_eq!(fold_v1(&xy), text(&n.fold(&raw(&xy))));
    }
}

proptest! {
    #![proptest_config(test_config(256))]

    #[test]
    fn long_runs_of_marks_fold_as_the_naive_derivation(x in long_mark_runs(), y in long_mark_runs()) {
        let n = naive();
        let raw = |s: &str| s.chars().map(|c| c as u32).collect::<Vec<u32>>();
        let (fx, fy) = (text(&n.fold(&raw(&x))), text(&n.fold(&raw(&y))));
        prop_assert_eq!(fold_v1(&x), fx.clone());
        prop_assert_eq!(nfd(&x), text(&n.nfd(&raw(&x))));
        prop_assert_eq!(fold_eq(&x, &y), fx == fy);
        prop_assert_eq!(fold_cmp(&x, &y), fx.as_bytes().cmp(fy.as_bytes()));
        prop_assert!(fold_matches(&fx, &x));
        prop_assert_eq!(ceq(&x, &y), n.nfd(&raw(&x)) == n.nfd(&raw(&y)));
    }
}
