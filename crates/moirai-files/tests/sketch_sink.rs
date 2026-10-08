//! WP-66: the pass-1 fingerprint sink ([F20 §2.6.1–§2.6.3]) against a whole-buffer definition written from chapter 20
//! alone: fingerprint lines, the bottom-64 sketch, `weight` and `distinct` with its estimate; piece and chunk
//! boundaries; a read attempt that starts again; binary content and the 4 GiB limit; the sink's fixed state on a
//! 16 MiB file.
//!
//! Every content here is synthetic and in memory: a product crate's tests open no file ([OS/README §2.5], PLAN R18).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use moirai_files::oid::ObjectFormat;
use moirai_files::sketch::{Fingerprint, FingerprintSink};
use moirai_files::text::{
    ByteSource, ContentReader, LineSink, ReadOptions, Snapshot, TextStats, analyse,
};
use proptest::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

// --- the whole-buffer definition, written from [F20 §1.2, §2.1, §2.2, §2.5, §2.6] alone -------------------------

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// `WS` = {`09`, `0A`, `0B`, `0C`, `0D`, `20`} ([F20 §1.2]).
fn is_ws(b: u8) -> bool {
    matches!(b, 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20)
}

/// `chars(x)`: the bytes of x outside `80`–`BF` ([F20 §1.2]).
fn chars(x: &[u8]) -> usize {
    x.iter().filter(|&&b| !(0x80..=0xBF).contains(&b)).count()
}

/// `is_text(b)`: [F20 §2.1]'s pseudocode, line by line.
fn naive_is_text(b: &[u8]) -> bool {
    let (mut lonecr, mut nul, mut printable, mut nonprintable) = (0u64, 0u64, 0u64, 0u64);
    let n = b.len();
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c == 0x0D {
            if i + 1 < n && b[i + 1] == 0x0A {
                i += 2;
                continue;
            }
            lonecr += 1;
            i += 1;
            continue;
        }
        if c == 0x0A {
            i += 1;
            continue;
        }
        if c == 0x7F {
            nonprintable += 1;
        } else if c < 0x20 {
            if [0x08, 0x09, 0x0C, 0x1B].contains(&c) {
                printable += 1;
            } else {
                if c == 0 {
                    nul += 1;
                }
                nonprintable += 1;
            }
        } else {
            printable += 1;
        }
        i += 1;
    }
    if b.last() == Some(&0x1A) {
        nonprintable -= 1;
    }
    lonecr == 0 && nul == 0 && (printable >> 7) >= nonprintable
}

/// `norm(b)` of a text content: every CR LF pair replaced by LF ([F20 §2.2]).
fn naive_norm(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x0D && b.get(i + 1) == Some(&0x0A) {
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// `lines(t)`: t split at every `0A`, the last piece dropped if empty ([F20 §2.5]).
fn naive_lines(t: &[u8]) -> Vec<&[u8]> {
    let mut v: Vec<&[u8]> = t.split(|&b| b == 0x0A).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// `collapse(nl(l))` ([F20 §2.5], §2.6.1).
fn collapse_nl(l: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for word in l.split(|&b| is_ws(b)).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(0x20);
        }
        out.extend_from_slice(word);
    }
    out
}

/// `sh(f) = low(XXH3-64(f), 32)` ([F20 §2.6.2]).
fn sh(f: &[u8]) -> u32 {
    (xxh3_64(f) & 0xFFFF_FFFF) as u32
}

/// `⌊63 × 2^32 / (s64 + 1)⌋`, the raw k-minimum-values estimate ([F20 §2.6.3]).
fn kmv_raw(s64: u32) -> u64 {
    (63u64 << 32) / (u64::from(s64) + 1)
}

/// Every field of a fingerprint, compared at once.
#[derive(Debug, PartialEq, Eq)]
struct Want {
    nlines: u32,
    nbytes: u32,
    weight: u32,
    distinct: u32,
    estimated: bool,
    sketch: Vec<u32>,
}

fn got(fp: &Fingerprint) -> Want {
    Want {
        nlines: fp.nlines(),
        nbytes: fp.nbytes(),
        weight: fp.weight(),
        distinct: fp.distinct(),
        estimated: fp.distinct_estimated(),
        sketch: fp.sketch().to_vec(),
    }
}

/// [F20 §2.6.3] over the given lines of an anchor text, with `nlines` and `nbytes` given.
fn naive_over_lines<'a>(
    lines: impl IntoIterator<Item = &'a [u8]>,
    nlines: u32,
    nbytes: u32,
) -> Want {
    let mut v = BTreeSet::new();
    let mut weight = 0usize;
    for l in lines {
        let f = collapse_nl(l);
        // `FP_MIN_CHARS` = 3 ([F20 §2.6.1]).
        if chars(&f) > 3 {
            weight += f.len();
            v.insert(sh(&f));
        }
    }
    // `SKETCH_K` = 64.
    let sketch: Vec<u32> = v.iter().take(64).copied().collect();
    let (distinct, estimated) = if v.len() <= 64 {
        (v.len() as u32, false)
    } else {
        (kmv_raw(sketch[63]).max(65) as u32, true)
    };
    Want {
        nlines,
        nbytes,
        weight: weight as u32,
        distinct,
        estimated,
        sketch,
    }
}

/// The fingerprint of content `b` ([F20 §2.6]): `None` for binary content.
fn naive(b: &[u8]) -> Option<Want> {
    if !naive_is_text(b) {
        return None;
    }
    let norm = naive_norm(b);
    // `atext(b)`: `norm(b)` without one leading BOM ([F20 §2.5]).
    let atext = norm.strip_prefix(BOM).unwrap_or(&norm);
    Some(naive_over_lines(
        naive_lines(atext),
        naive_lines(&norm).len() as u32,
        norm.len() as u32,
    ))
}

// --- feeding ---------------------------------------------------------------------------------------------------

/// Statistics of a text content for sinks fed directly: any text content's `nlines` and `nbytes` do here.
fn text_stats() -> TextStats {
    TextStats {
        len: 1 << 20,
        ..TextStats::default()
    }
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

/// The fingerprint of a fresh sink fed `lines`, with [`text_stats`].
fn fingerprint_of(lines: &[&[u8]]) -> Fingerprint {
    let mut s = FingerprintSink::new();
    s.begin();
    feed(&mut s, lines);
    s.finish(&text_stats()).unwrap()
}

/// Content in memory, read in pieces whose sizes cycle through `sizes`.
struct Chunky<'a> {
    data: &'a [u8],
    pos: usize,
    sizes: Vec<usize>,
    k: usize,
}

impl ByteSource for Chunky<'_> {
    type Error = std::convert::Infallible;
    type Stamp = u64;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let want = self.sizes[self.k % self.sizes.len()];
        self.k += 1;
        let n = want.min(buf.len()).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }

    fn snapshot(&self) -> Result<Snapshot<u64>, Self::Error> {
        Ok(Snapshot {
            size: self.data.len() as u64,
            mtime: 1,
        })
    }
}

fn options() -> ReadOptions {
    ReadOptions {
        format: ObjectFormat::Sha1,
        max_read_bytes: u64::MAX,
        max_line_hashes: None,
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

/// A `WS` run: mostly spaces, some HT and FF, rarely VT (a non-printable byte, so a content with many is binary).
fn ws_run() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        prop_oneof![40 => Just(0x20u8), 10 => Just(0x09u8), 3 => Just(0x0Cu8), 1 => Just(0x0Bu8)],
        1..4,
    )
}

/// A word: from a pool large enough that a content often has more than 64 distinct lines, or a short word (a line of
/// at most 3 characters), 2-byte Cyrillic, or brackets.
fn word() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => (0u32..600).prop_map(|k| format!("w{k}").into_bytes()),
        2 => prop::sample::select(vec![
            "a", "ab", "abc", "abcd", "ж", "жж", "жжж", "жжжж", "{", "};", "fn", "x_y", "Ä",
        ])
        .prop_map(|s| s.as_bytes().to_vec()),
    ]
}

/// One line: an optional leading `WS` run, words separated by `WS` runs, an optional trailing run; or one of a few
/// repeated lines, so fingerprint lines repeat.
fn line() -> impl Strategy<Value = Vec<u8>> {
    let built = (
        prop::option::of(ws_run()),
        prop::collection::vec((word(), ws_run()), 0..5),
        any::<bool>(),
    )
        .prop_map(|(lead, parts, trailing)| {
            let mut l = lead.unwrap_or_default();
            let n = parts.len();
            for (i, (w, s)) in parts.into_iter().enumerate() {
                l.extend_from_slice(&w);
                if i + 1 < n || trailing {
                    l.extend_from_slice(&s);
                }
            }
            l
        });
    prop_oneof![
        4 => built,
        1 => (0u32..20).prop_map(|k| format!("repeated line {k}").into_bytes()),
    ]
}

/// A text content: an optional BOM, 0–300 lines ended by LF or CR LF, a final terminator or not.
fn text() -> impl Strategy<Value = Vec<u8>> {
    (
        any::<bool>(),
        prop::collection::vec((line(), any::<bool>()), 0..300),
        any::<bool>(),
    )
        .prop_map(|(bom, lines, final_eol)| {
            let mut t = if bom { BOM.to_vec() } else { Vec::new() };
            let n = lines.len();
            for (i, (l, crlf)) in lines.into_iter().enumerate() {
                t.extend_from_slice(&l);
                if i + 1 < n || final_eol {
                    t.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
                }
            }
            t
        })
}

/// `bytes` cut into pieces whose sizes cycle through `sizes`, starting at `*k`.
fn cut<'a>(bytes: &'a [u8], sizes: &[usize], k: &mut usize) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let n = sizes[*k % sizes.len()].min(rest.len());
        *k += 1;
        out.push(&rest[..n]);
        rest = &rest[n..];
    }
    out
}

proptest! {
    #![proptest_config(test_config(64))]

    /// `analyse` with the sink gives every field of the whole-buffer definition, `None` for binary content.
    #[test]
    fn sink_equals_the_naive_bottom_64(t in text()) {
        let mut sink = FingerprintSink::new();
        let c = analyse(&t, ObjectFormat::Sha1, None, &mut sink);
        prop_assert_eq!(sink.finish(&c.stats).as_ref().map(got), naive(&t));
    }

    /// A line cut into pieces anywhere (inside `WS` runs and UTF-8 sequences) gives the whole line's result.
    #[test]
    fn pieces_never_change_the_fingerprint(
        t in text(),
        sizes in prop::collection::vec(1usize..6, 1..8),
    ) {
        let lines = naive_lines(&t);
        let whole = fingerprint_of(&lines);

        let mut cuts = FingerprintSink::new();
        cuts.begin();
        let mut k = 0;
        for l in &lines {
            for p in cut(l, &sizes, &mut k) {
                cuts.piece(p);
            }
            cuts.end_line();
        }
        prop_assert_eq!(cuts.finish(&text_stats()), Some(whole));

        let mut bytes = FingerprintSink::new();
        bytes.begin();
        for l in &lines {
            for b in l.chunks(1) {
                bytes.piece(b);
            }
            bytes.end_line();
        }
        prop_assert_eq!(bytes.finish(&text_stats()), Some(whole));
    }

    /// The streaming reader over chunks of 1–9 bytes (a CR LF pair or the BOM split by a chunk boundary) feeds the
    /// sink what `analyse` feeds it.
    #[test]
    fn chunked_reads_equal_analyse(
        t in text(),
        sizes in prop::collection::vec(1usize..10, 1..8),
    ) {
        let mut whole = FingerprintSink::new();
        let c = analyse(&t, ObjectFormat::Sha1, None, &mut whole);
        let mut chunked = FingerprintSink::new();
        let mut src = Chunky { data: &t, pos: 0, sizes, k: 0 };
        let r = ContentReader::new().read(&mut src, &options(), &mut chunked).unwrap();
        prop_assert_eq!(r.stats, c.stats);
        prop_assert_eq!(chunked.finish(&r.stats), whole.finish(&c.stats));
    }
}

// --- the examples --------------------------------------------------------------------------------------------------

/// [F20 §2.4]: every read attempt starts with `begin`, which forgets the earlier attempt, after a closed line and in
/// the middle of a line.
#[test]
fn begin_discards_an_earlier_attempt() {
    let earlier: Vec<String> = (0..200)
        .map(|i| format!("earlier attempt line {i}"))
        .collect();
    let earlier: Vec<&[u8]> = earlier.iter().map(String::as_bytes).collect();
    let later: [&[u8]; 3] = [b"the line that stays", b"another line", b"  third   line  "];
    let fresh = fingerprint_of(&later);
    assert_eq!(fresh.sketch().len(), 3);

    // After a closed line: the earlier attempt saw more than 64 values, so its flag was set.
    let mut s = FingerprintSink::new();
    s.begin();
    feed(&mut s, &earlier);
    assert!(s.finish(&text_stats()).unwrap().distinct_estimated());
    s.begin();
    feed(&mut s, &later);
    assert_eq!(s.finish(&text_stats()), Some(fresh));

    // In the middle of a line, and after a pending space.
    for half in [&b"half a li"[..], b"half a line \t"] {
        let mut s = FingerprintSink::new();
        s.begin();
        feed(&mut s, &earlier);
        s.piece(half);
        s.begin();
        feed(&mut s, &later);
        assert_eq!(s.finish(&text_stats()), Some(fresh));
    }
}

/// [F20 §2.6.1]: f is a fingerprint line iff `chars(f) > 3`, characters, not bytes, after `collapse(nl(l))`.
#[test]
fn fp_min_chars_counts_characters() {
    let cases: [(&str, bool); 10] = [
        ("abc", false),
        ("abcd", true),
        ("жж", false),
        ("жжж", false),
        ("жжжж", true),
        ("a \t b", false),
        ("  a\x0cb\tc  ", true),
        ("", false),
        (" \t\x0b\x0c ", false),
        ("Ä Ä", false),
    ];
    let mut kept = BTreeSet::new();
    let mut weight = 0;
    for (l, keep) in cases {
        let fp = fingerprint_of(&[l.as_bytes()]);
        let f = collapse_nl(l.as_bytes());
        if keep {
            assert_eq!(fp.weight() as usize, f.len(), "{l:?}");
            assert_eq!(fp.sketch(), [sh(&f)], "{l:?}");
            assert_eq!(fp.distinct(), 1, "{l:?}");
            kept.insert(sh(&f));
            weight += f.len();
        } else {
            assert_eq!(fp.weight(), 0, "{l:?}");
            assert!(fp.sketch().is_empty(), "{l:?}");
            assert_eq!(fp.distinct(), 0, "{l:?}");
        }
    }
    let all: Vec<&[u8]> = cases.iter().map(|(l, _)| l.as_bytes()).collect();
    let fp = fingerprint_of(&all);
    assert_eq!(fp.weight() as usize, weight);
    assert_eq!(fp.sketch(), kept.into_iter().collect::<Vec<_>>());
    assert_eq!(fp.distinct(), 3);
}

/// [F20 §2.6.1]: `nl` trims the line and `collapse` turns every `WS` run into one `20` before the hash.
#[test]
fn collapse_and_trim_before_hashing() {
    let fp = fingerprint_of(&[b"\x0b a\t\t b \x0cc  "]);
    assert_eq!(fp.sketch(), [(xxh3_64(b"a b c") & 0xFFFF_FFFF) as u32]);
    assert_eq!(fp.weight(), 5);

    // Lines that differ only in their `WS` runs are one value, each counted in the weight.
    let fp = fingerprint_of(&[b"a b c", b"a  b\tc", b"\ta b    c\x0c", b"a\x0bb\x0cc\r"]);
    assert_eq!(fp.sketch(), [sh(b"a b c")]);
    assert_eq!(fp.weight(), 20);
    assert_eq!(fp.distinct(), 1);
    assert!(!fp.distinct_estimated());
}

/// `distinct` is #V when #V ≤ 64 ([F20 §2.6.3]); a value fed again after the sketch filled, the largest or an
/// inner one, sets no flag.
#[test]
fn distinct_is_exact_up_to_64_values() {
    let lines: Vec<String> = (0..64).map(|i| format!("exactly sixty-four {i}")).collect();
    let mut by_sh: Vec<(u32, &[u8])> = lines
        .iter()
        .map(|l| (sh(l.as_bytes()), l.as_bytes()))
        .collect();
    by_sh.sort_unstable();
    let values: BTreeSet<u32> = by_sh.iter().map(|&(v, _)| v).collect();
    assert_eq!(values.len(), 64, "the 64 lines have 64 distinct values");

    let mut fed: Vec<&[u8]> = lines.iter().map(String::as_bytes).collect();
    // The largest value, an inner one and the smallest, again, after the 64th distinct value.
    fed.extend([by_sh[63].1, by_sh[31].1, by_sh[0].1, by_sh[63].1]);
    let fp = fingerprint_of(&fed);
    assert_eq!(fp.distinct(), 64);
    assert!(!fp.distinct_estimated());
    assert_eq!(fp.sketch().len(), 64);
    assert_eq!(fp.sketch(), values.into_iter().collect::<Vec<_>>());
    let weight: usize = fed.iter().map(|l| l.len()).sum();
    assert_eq!(fp.weight() as usize, weight);
}

/// Lines `prefix i` for i = 0, 1, … whose sketch line hash satisfies `keep`, until `count` are found.
fn search(prefix: &str, count: usize, keep: impl Fn(u32) -> bool) -> Vec<String> {
    let mut found = Vec::new();
    let mut s = String::new();
    for i in 0u64.. {
        s.clear();
        write!(s, "{prefix} {i}").unwrap();
        if keep(sh(s.as_bytes())) {
            found.push(s.clone());
            if found.len() == count {
                break;
            }
        }
    }
    found
}

/// Above 64 values, `distinct` is `max(65, ⌊63 × 2^32 / (s64 + 1)⌋)` with the flag set ([F20 §2.6.3]), whether the
/// 65th value is evicted or lands above a full sketch.
#[test]
fn distinct_estimate_from_65_values() {
    // 65 values below 500,000: there s64·(s64 + 1) < 63·2^32, so the `+ 1` and the 63 both change the estimate.
    let low = search("low sketch value", 65, |v| v < 500_000);
    let mut low: Vec<(u32, &[u8])> = low
        .iter()
        .map(|l| (sh(l.as_bytes()), l.as_bytes()))
        .collect();
    low.sort_unstable();
    let values: Vec<u32> = low.iter().map(|&(v, _)| v).collect();
    assert_eq!(values.iter().collect::<BTreeSet<_>>().len(), 65);
    let s64 = values[63];
    let want = kmv_raw(s64);
    assert_ne!(
        want,
        (63u64 << 32) / u64::from(s64),
        "the `+ 1` changes this estimate"
    );
    assert_ne!(
        want,
        (64u64 << 32) / (u64::from(s64) + 1),
        "the 63 changes this estimate"
    );
    assert!(want > 65);

    let ascending: Vec<&[u8]> = low.iter().map(|&(_, l)| l).collect();
    let descending: Vec<&[u8]> = ascending.iter().rev().copied().collect();
    for order in [&ascending, &descending] {
        let fp = fingerprint_of(order);
        assert!(fp.distinct_estimated());
        assert_eq!(u64::from(fp.distinct()), want);
        assert_eq!(fp.sketch(), &values[..64]);
    }

    // 1,000 values.
    let many: Vec<String> = (0..1000)
        .map(|i| format!("one of a thousand {i}"))
        .collect();
    let many: Vec<&[u8]> = many.iter().map(String::as_bytes).collect();
    let fp = fingerprint_of(&many);
    let st = text_stats();
    let want = naive_over_lines(
        many.iter().copied(),
        st.nlines() as u32,
        st.norm_len() as u32,
    );
    assert!(want.estimated);
    assert_eq!(got(&fp), want);
}

/// The estimate is never below 65 ([F20 §2.6.3] `max(65, …)`), even when the raw quotient is.
#[test]
fn distinct_estimate_is_at_least_65() {
    // The raw estimate is below 65 iff s64 ≥ ⌊63 × 2^32 / 65⌋ (4,162,814,456).
    let threshold = u32::try_from((63u64 << 32) / 65).unwrap();
    assert!(kmv_raw(threshold) < 65 && kmv_raw(threshold - 1) >= 65);
    let mut lines = search("high sketch value", 2, |v| v >= threshold);
    lines.extend(search("lower sketch value", 63, |v| v < threshold));
    let lines: Vec<&[u8]> = lines.iter().map(String::as_bytes).collect();
    let values: BTreeSet<u32> = lines.iter().map(|l| sh(l)).collect();
    assert_eq!(values.len(), 65);
    let s64 = *values.iter().nth(63).unwrap();
    assert!(kmv_raw(s64) < 65, "the raw estimate is below 65");

    let fp = fingerprint_of(&lines);
    assert!(fp.distinct_estimated());
    assert_eq!(fp.distinct(), 65);
    assert_eq!(fp.sketch().last(), Some(&s64));
}

/// Through `analyse`: `nlines` and `nbytes` are quantities of `norm(b)`, the lines those of the anchor text; binary
/// content has no fingerprint ([F20 §2.6.3]).
#[test]
fn analyse_on_text_and_binary() {
    let run = |b: &[u8]| {
        let mut sink = FingerprintSink::new();
        let c = analyse(b, ObjectFormat::Sha1, None, &mut sink);
        sink.finish(&c.stats)
    };

    // A BOM, CR LF and no final LF: `nbytes` counts the BOM, and line 1 of the anchor text does not hold it.
    let b = b"\xEF\xBB\xBFfirst line here\r\nsecond line here\r\nlast";
    let fp = run(b).unwrap();
    assert_eq!(fp.nlines(), 3);
    assert_eq!(fp.nbytes() as usize, b.len() - 2);
    assert_eq!(fp.weight() as usize, 15 + 16 + 4);
    let mut want = vec![sh(b"first line here"), sh(b"second line here"), sh(b"last")];
    want.sort_unstable();
    assert_eq!(fp.sketch(), want);
    assert_eq!(Some(got(&fp)), naive(b));

    // Exactly `EF BB BF`: one line of `norm(b)`, no fingerprint line.
    let fp = run(BOM).unwrap();
    assert_eq!(
        (fp.nlines(), fp.nbytes(), fp.weight(), fp.distinct()),
        (1, 3, 0, 0)
    );
    assert!(fp.sketch().is_empty());

    // The empty content: a fingerprint of zeros.
    let fp = run(b"").unwrap();
    assert_eq!(
        got(&fp),
        Want {
            nlines: 0,
            nbytes: 0,
            weight: 0,
            distinct: 0,
            estimated: false,
            sketch: Vec::new(),
        }
    );

    // Binary: a NUL, a lone CR, and the ratio test after every line was delivered.
    for b in [&b"abcd\0efgh\n"[..], b"abcd\refgh\n", b"abcd\x01\n"] {
        assert!(!naive_is_text(b));
        assert_eq!(run(b), None, "{b:?}");
    }
}

/// [F20 §2.6.3]: a fingerprint exists only when `nbytes = len(norm(b)) < 2^32`; the raw length does not decide.
#[test]
fn no_fingerprint_from_4_gib() {
    let sink = FingerprintSink::new();
    let at = |len: u64, crlf: u64| TextStats {
        len,
        crlf,
        ..TextStats::default()
    };
    assert_eq!(sink.finish(&at(1 << 32, 0)), None);
    let fp = sink.finish(&at(1 << 32, 1)).unwrap();
    assert_eq!(fp.nbytes(), u32::MAX);
    assert_eq!(fp.nlines(), 1);
    let fp = sink.finish(&at((1 << 32) - 1, 0)).unwrap();
    assert_eq!(fp.nbytes(), u32::MAX);
    assert_eq!(fp.weight(), 0);
}

// --- 16 MiB in a fixed state ([F20 §2.6.3] "constant whatever the file size") ------------------------------------

const SIXTEEN_MIB: u64 = 16 << 20;
const PREFIX: &[u8] = b"    let value_";
const SUFFIX: &[u8] = b" = compute(alpha, beta, gamma);";

/// Line i of the synthetic file: 62 bytes with i's ten digits in the middle, then CR LF.
fn synthetic_line(i: u64) -> [u8; 64] {
    let mut l = [b' '; 64];
    l[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut d = i;
    for k in (0..10).rev() {
        l[PREFIX.len() + k] = b'0' + (d % 10) as u8;
        d /= 10;
    }
    l[PREFIX.len() + 10..PREFIX.len() + 10 + SUFFIX.len()].copy_from_slice(SUFFIX);
    l[62] = b'\r';
    l[63] = b'\n';
    l
}

/// 16 MiB of CRLF text, every line different, generated on the fly: no copy of the file exists anywhere.
struct Synthetic {
    pos: u64,
}

impl ByteSource for Synthetic {
    type Error = std::convert::Infallible;
    type Stamp = ();
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let n = usize::try_from((SIXTEEN_MIB - self.pos).min(buf.len() as u64)).unwrap();
        let mut filled = 0;
        while filled < n {
            let line = synthetic_line(self.pos / 64);
            let off = (self.pos % 64) as usize;
            let take = (64 - off).min(n - filled);
            buf[filled..filled + take].copy_from_slice(&line[off..off + take]);
            filled += take;
            self.pos += take as u64;
        }
        Ok(n)
    }
    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }
    fn snapshot(&self) -> Result<Snapshot<()>, Self::Error> {
        Ok(Snapshot {
            size: SIXTEEN_MIB,
            mtime: (),
        })
    }
}

/// The sink's state is a fixed size, and a 16 MiB read through it gives the whole-buffer definition's fingerprint,
/// with the sketch evicting and the estimate taken at scale.
///
/// The 0.5 MB of PLAN WP-62 is extra RSS, which this crate cannot measure (`tests/text_reader.rs`,
/// `sixteen_mib_heap_accounting`): the evidence is this size bound and a sink with no heap field.
#[test]
fn sixteen_mib_read_in_constant_state() {
    assert!(size_of::<FingerprintSink>() <= 1024);
    assert!(size_of::<Fingerprint>() <= 300);

    let lines = SIXTEEN_MIB / 64;
    let mut values = BTreeSet::new();
    let mut weight = 0usize;
    for i in 0..lines {
        let l = synthetic_line(i);
        let f = collapse_nl(&l[..62]);
        weight += f.len();
        values.insert(sh(&f));
    }
    let sketch: Vec<u32> = values.iter().take(64).copied().collect();
    let want = Want {
        nlines: lines as u32,
        nbytes: (SIXTEEN_MIB - lines) as u32,
        weight: weight as u32,
        distinct: kmv_raw(sketch[63]).max(65) as u32,
        estimated: true,
        sketch,
    };

    let mut sink = FingerprintSink::new();
    let c = ContentReader::new()
        .read(&mut Synthetic { pos: 0 }, &options(), &mut sink)
        .unwrap();
    assert!(c.is_text());
    assert_eq!(sink.finish(&c.stats).as_ref().map(got), Some(want));
}
