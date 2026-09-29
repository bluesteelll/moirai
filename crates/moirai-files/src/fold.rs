//! `fold_v1`, NFD and canonical equivalence at Unicode 17.0.0 ([F20 §3.1], [F20 §3.4]; [80 §2.10] P6; [40 §2.4]).
//!
//! ```text
//! fold_v1(x) = NFD( CF( NFD(x) ) )          at Unicode 17.0.0
//! ```
//!
//! - **NFD** ([F20 §3.1]): full canonical decomposition (every canonical mapping of UnicodeData.txt field 5, applied
//!   recursively; Hangul syllables U+AC00–U+D7A3 arithmetically; compatibility mappings never), then canonical
//!   ordering: a stable sort by `Canonical_Combining_Class` of every maximal run of code points whose class is not 0.
//! - **CF** ([F20 §3.1]): full case folding, each code point replaced by the mapping of its `C` or `F` line of
//!   CaseFolding.txt, if it has one.
//! - Code points unassigned in Unicode 17.0.0 map to themselves.
//!
//! The data is the generated private module `tables` (`cargo xtask ucd`, from the pinned files of
//! `fixtures/ucd/17.0.0/`).
//! `tests/fold_ucd.rs` checks it against those files over every scalar value, and `NormalizationTest.txt` for NFD
//! ([F20 §3.2]).
//!
//! **Why the inner NFD matters.** CF is not closed under canonical ordering: U+0345 (class 240) folds to U+03B9
//! (class 0), so the position the first NFD sorts it to decides where the starter lands. `fold_v1` applies the three
//! steps in that order; it is not the per-character composition.
//!
//! **Cost.** All-ASCII input takes a byte-wise fast path (its fold is its ASCII lower case) and touches no table.
//! Other input is folded by a streaming pipeline of two canonical-ordering buffers ([`FoldChars`]), which allocates
//! nothing unless a run of combining marks exceeds 16 code points. Every lookup below the tables' limit is three array
//! reads (`STAGE1`, `STAGE2`, `RECORDS`), plus one slice of `DECOMP` or `FOLD` when the record has a mapping.
//!
//! `fold_v1` is used for `PATHIDX` order, collision and twin detection only, never for identity ([F20 §3.3]).

mod tables;

use std::cmp::Ordering;
use std::str::Chars;

/// The Unicode version of `fold_v1` ([F20 §3.1]): 17.0.0.
pub const UNICODE_VERSION: (u8, u8, u8) = tables::UNICODE_VERSION;

// Hangul syllable decomposition ([F20 §3.1]; the Unicode Standard §3.12).
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = 588;
const S_COUNT: u32 = 11_172;

/// A table record: (class, decomposition length, decomposition offset, folding length, folding offset).
type Rec = (u8, u8, u16, u8, u16);

/// The record of `c` in the two-stage table (the generated module's header describes the layout).
#[inline]
fn record(c: char) -> Rec {
    let cp = c as u32;
    if cp >= tables::LIMIT {
        return (0, 0, 0, 0, 0);
    }
    let block = usize::from(tables::STAGE1[(cp >> tables::SHIFT) as usize]);
    let slot = (block << tables::SHIFT) | (cp & ((1 << tables::SHIFT) - 1)) as usize;
    tables::RECORDS[usize::from(tables::STAGE2[slot])]
}

/// The jamo of a Hangul syllable offset `s` (`s < S_COUNT`): (count, [L, V, T]).
#[inline]
fn hangul(s: u32) -> (usize, [char; 3]) {
    let jamo = |cp: u32| {
        char::from_u32(cp)
            .unwrap_or_else(|| unreachable!("Hangul jamo U+{cp:04X} is a scalar value"))
    };
    let t = s % T_COUNT;
    let lv = [
        jamo(L_BASE + s / N_COUNT),
        jamo(V_BASE + (s % N_COUNT) / T_COUNT),
    ];
    if t == 0 {
        (2, [lv[0], lv[1], '\0'])
    } else {
        (3, [lv[0], lv[1], jamo(T_BASE + t)])
    }
}

/// The `Canonical_Combining_Class` of `c` ([F20 §3.1]: UnicodeData.txt field 3; 0 for unassigned code points).
#[inline]
pub fn canonical_combining_class(c: char) -> u8 {
    if c.is_ascii() { 0 } else { record(c).0 }
}

/// The full case folding of `c` ([F20 §3.1] CF): the mapping of its `C` or `F` line of CaseFolding.txt, or `None`
/// when it has neither (the code point folds to itself). `S` and `T` lines are never used.
pub fn case_fold(c: char) -> Option<&'static [char]> {
    let (_, _, _, flen, foff) = record(c);
    (flen != 0).then(|| &tables::FOLD[usize::from(foff)..][..usize::from(flen)])
}

/// The full canonical decomposition of `c` ([F20 §3.1]), not canonically ordered; `None` when `c` has none (it
/// decomposes to itself).
pub fn canonical_decomposition(c: char) -> Option<CanonicalDecomposition> {
    let s = (c as u32).wrapping_sub(S_BASE);
    if s < S_COUNT {
        let (len, jamo) = hangul(s);
        return Some(CanonicalDecomposition(Decomp::Hangul {
            jamo,
            next: 0,
            len: len as u8,
        }));
    }
    let (_, dlen, doff, _, _) = record(c);
    (dlen != 0).then(|| {
        CanonicalDecomposition(Decomp::Table(
            tables::DECOMP[usize::from(doff)..][..usize::from(dlen)].iter(),
        ))
    })
}

/// The code points of a full canonical decomposition ([`canonical_decomposition`]).
#[derive(Clone, Debug)]
pub struct CanonicalDecomposition(Decomp);

#[derive(Clone, Debug)]
enum Decomp {
    Table(std::slice::Iter<'static, char>),
    Hangul { jamo: [char; 3], next: u8, len: u8 },
}

impl Iterator for CanonicalDecomposition {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        match &mut self.0 {
            Decomp::Table(it) => it.next().copied(),
            Decomp::Hangul { jamo, next, len } => (*next < *len).then(|| {
                *next += 1;
                jamo[usize::from(*next - 1)]
            }),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = match &self.0 {
            Decomp::Table(it) => it.len(),
            Decomp::Hangul { next, len, .. } => usize::from(len - next),
        };
        (n, Some(n))
    }
}

impl ExactSizeIterator for CanonicalDecomposition {}

/// Code points held inline by a [`Pending`] buffer before it spills to the heap.
const INLINE: usize = 16;

/// The canonical-ordering buffer of one pipeline stage: code points with their classes. Inline up to [`INLINE`]
/// entries; a longer run of combining marks moves the whole buffer to the heap until it drains.
#[derive(Clone, Debug)]
struct Pending {
    inline: [(char, u8); INLINE],
    len: usize,
    heap: Vec<(char, u8)>,
}

impl Pending {
    const fn new() -> Pending {
        Pending {
            inline: [('\0', 0); INLINE],
            len: 0,
            heap: Vec::new(),
        }
    }

    #[inline]
    fn len(&self) -> usize {
        if self.heap.is_empty() {
            self.len
        } else {
            self.heap.len()
        }
    }

    #[inline]
    fn get(&self, i: usize) -> char {
        if self.heap.is_empty() {
            self.inline[i].0
        } else {
            self.heap[i].0
        }
    }

    #[inline]
    fn slice_mut(&mut self) -> &mut [(char, u8)] {
        if self.heap.is_empty() {
            &mut self.inline[..self.len]
        } else {
            &mut self.heap
        }
    }

    #[inline]
    fn push(&mut self, x: (char, u8)) {
        if self.heap.is_empty() {
            if self.len < INLINE {
                self.inline[self.len] = x;
                self.len += 1;
                return;
            }
            self.heap.reserve(2 * INLINE);
            self.heap.extend_from_slice(&self.inline[..self.len]);
            self.len = 0;
        }
        self.heap.push(x);
    }

    /// Removes the first `n` entries.
    fn remove_front(&mut self, n: usize) {
        if self.heap.is_empty() {
            self.inline.copy_within(n..self.len, 0);
            self.len -= n;
        } else {
            self.heap.drain(..n);
            if self.heap.len() <= INLINE {
                self.len = self.heap.len();
                self.inline[..self.len].copy_from_slice(&self.heap);
                self.heap.clear();
            }
        }
    }
}

/// What a pipeline stage does with each code point of its source before canonical ordering.
#[derive(Clone, Copy, Debug)]
enum Stage {
    /// NFD: the full canonical decomposition.
    Decompose,
    /// CF, then NFD: the full case folding, each code point of it fully decomposed. The source is NFD, so a code
    /// point without a folding is pushed unchanged.
    FoldDecompose,
}

/// One stage of the pipeline: expands each source code point, then puts every run of non-starters in canonical
/// order ([F20 §3.1]). Everything before the last starter is final and is emitted; the rest waits.
#[derive(Clone, Debug)]
struct Reorder<I> {
    src: I,
    stage: Stage,
    buf: Pending,
    /// Entries of `buf` below this index are final.
    ready: usize,
    /// The next final entry to emit.
    pos: usize,
    done: bool,
}

impl<I: Iterator<Item = char>> Reorder<I> {
    const fn new(src: I, stage: Stage) -> Reorder<I> {
        Reorder {
            src,
            stage,
            buf: Pending::new(),
            ready: 0,
            pos: 0,
            done: false,
        }
    }

    /// Canonical ordering of the open run: a stable sort by class. The run is an optional starter followed by
    /// non-starters (each starter closes the run before it), so the starter, class 0, keeps its place.
    fn close(&mut self) {
        let ready = self.ready;
        let run = &mut self.buf.slice_mut()[ready..];
        if run.len() > 2 || (run.len() == 2 && run[0].1 > run[1].1) {
            run.sort_by_key(|&(_, k)| k);
        }
        self.ready = self.buf.len();
    }

    #[inline]
    fn push(&mut self, c: char, class: u8) {
        if class == 0 && self.buf.len() > self.ready {
            self.close();
        }
        self.buf.push((c, class));
    }

    /// Pushes the full canonical decomposition of `c`, whose record is `r`.
    #[inline]
    fn push_decomposed(&mut self, c: char, r: Rec) {
        let s = (c as u32).wrapping_sub(S_BASE);
        if s < S_COUNT {
            // Hangul jamo are starters.
            let (len, jamo) = hangul(s);
            for &j in &jamo[..len] {
                self.push(j, 0);
            }
        } else if r.1 == 0 {
            self.push(c, r.0);
        } else {
            for &d in &tables::DECOMP[usize::from(r.2)..][..usize::from(r.1)] {
                self.push(d, canonical_combining_class(d));
            }
        }
    }

    fn feed(&mut self, c: char) {
        if c.is_ascii() {
            // Class 0, no decomposition; A–Z fold to a–z.
            let c = match self.stage {
                Stage::Decompose => c,
                Stage::FoldDecompose => c.to_ascii_lowercase(),
            };
            return self.push(c, 0);
        }
        let r = record(c);
        match self.stage {
            Stage::Decompose => self.push_decomposed(c, r),
            Stage::FoldDecompose if r.3 == 0 => self.push(c, r.0),
            Stage::FoldDecompose => {
                for &e in &tables::FOLD[usize::from(r.4)..][..usize::from(r.3)] {
                    self.push_decomposed(e, record(e));
                }
            }
        }
    }
}

impl<I: Iterator<Item = char>> Iterator for Reorder<I> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            if self.pos < self.ready {
                self.pos += 1;
                return Some(self.buf.get(self.pos - 1));
            }
            if self.ready > 0 {
                self.buf.remove_front(self.ready);
                self.ready = 0;
                self.pos = 0;
            }
            if self.done {
                return None;
            }
            match self.src.next() {
                Some(c) => self.feed(c),
                None => {
                    self.done = true;
                    self.close();
                }
            }
        }
    }
}

/// The code points of `NFD(x)`, streamed ([`nfd_chars`]).
#[derive(Clone, Debug)]
pub struct NfdChars<'a>(Reorder<Chars<'a>>);

impl Iterator for NfdChars<'_> {
    type Item = char;

    #[inline]
    fn next(&mut self) -> Option<char> {
        self.0.next()
    }
}

/// The code points of `fold_v1(x)`, streamed ([`fold_chars`]).
#[derive(Clone, Debug)]
pub struct FoldChars<'a>(Reorder<Reorder<Chars<'a>>>);

impl Iterator for FoldChars<'_> {
    type Item = char;

    #[inline]
    fn next(&mut self) -> Option<char> {
        self.0.next()
    }
}

/// `NFD(x)` as a stream of code points ([F20 §3.1]).
pub fn nfd_chars(x: &str) -> NfdChars<'_> {
    NfdChars(Reorder::new(x.chars(), Stage::Decompose))
}

/// `fold_v1(x)` as a stream of code points ([F20 §3.1]).
pub fn fold_chars(x: &str) -> FoldChars<'_> {
    FoldChars(Reorder::new(
        Reorder::new(x.chars(), Stage::Decompose),
        Stage::FoldDecompose,
    ))
}

/// The capacity reserved for the NFD or `fold_v1` of non-ASCII `x`: one and a half times its length, the expansion
/// of a precomposed Latin letter (`é`, 2 bytes, to `e` U+0301, 3 bytes). Longer expansions (a Hangul syllable, 3
/// bytes to 9) grow the string once more.
#[inline]
fn expanded_capacity(x: &str) -> usize {
    x.len() + x.len() / 2
}

/// `fold_v1(x)` ([F20 §3.1]), as UTF-8.
pub fn fold_v1(x: &str) -> String {
    if x.is_ascii() {
        return x.to_ascii_lowercase();
    }
    let mut s = String::with_capacity(expanded_capacity(x));
    s.extend(fold_chars(x));
    s
}

/// Appends `fold_v1(x)` ([F20 §3.1]) to `out`.
pub fn fold_v1_into(x: &str, out: &mut String) {
    if x.is_ascii() {
        let start = out.len();
        out.push_str(x);
        out[start..].make_ascii_lowercase();
    } else {
        out.extend(fold_chars(x));
    }
}

/// `NFD(x)` ([F20 §3.1]), as UTF-8.
pub fn nfd(x: &str) -> String {
    if x.is_ascii() {
        return x.to_owned();
    }
    let mut s = String::with_capacity(expanded_capacity(x));
    s.extend(nfd_chars(x));
    s
}

/// Appends `NFD(x)` ([F20 §3.1]) to `out`.
pub fn nfd_into(x: &str, out: &mut String) {
    if x.is_ascii() {
        out.push_str(x);
    } else {
        out.extend(nfd_chars(x));
    }
}

/// `fold_v1(a) = fold_v1(b)`, without allocating.
pub fn fold_eq(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(b)
    } else {
        fold_chars(a).eq(fold_chars(b))
    }
}

/// `fold_v1(s) = folded`, for a `folded` computed once ([`fold_v1`]) and compared with many strings, as the sibling
/// check of [OS/path §8.2] and a `PATHIDX` probe do.
pub fn fold_matches(folded: &str, s: &str) -> bool {
    if s.is_ascii() {
        folded.len() == s.len()
            && folded
                .bytes()
                .zip(s.bytes())
                .all(|(f, b)| f == b.to_ascii_lowercase())
    } else {
        fold_chars(s).eq(folded.chars())
    }
}

/// The order of `fold_v1(a)` and `fold_v1(b)` as UTF-8 byte strings ([F01 §6.6] path order), without allocating:
/// the `fold` component of the `PATHIDX` key `(root, fold, path, id)` ([F09 §13.1]). Scalar-value order and UTF-8
/// byte order agree, so the code point streams are compared directly.
pub fn fold_cmp(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
    } else {
        fold_chars(a).cmp(fold_chars(b))
    }
}

/// Canonical equivalence `ceq(a, b)` ⟺ `NFD(a) = NFD(b)` ([F20 §3.4]); equal to `NFC(a) = NFC(b)`, so the
/// normalization rules of [80 §2.11.4] rule 2 need no composition table.
pub fn ceq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return false;
    }
    nfd_chars(a).eq(nfd_chars(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs
    /// 16 times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is
    /// printed).
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

    #[test]
    fn the_f20_examples() {
        // [F20 §3.1], the informative table.
        let cases: &[(&str, &str)] = &[
            ("Plan.md", "plan.md"),
            ("\u{DF}", "ss"),
            ("\u{1E9E}", "ss"),
            ("\u{130}", "i\u{307}"),
            ("caf\u{E9}", "cafe\u{301}"),
            ("cafe\u{301}", "cafe\u{301}"),
            ("\u{212B}", "a\u{30A}"),
            ("\u{2126}", "\u{3C9}"),
            ("\u{FB03}", "ffi"),
            ("\u{1C5}", "\u{1C6}"),
            ("\u{D55C}", "\u{1112}\u{1161}\u{11AB}"),
        ];
        for &(x, want) in cases {
            assert_eq!(fold_v1(x), want, "fold_v1({x:?})");
            assert!(fold_eq(x, want));
            assert_eq!(fold_cmp(x, want), Ordering::Equal);
            assert!(fold_matches(want, x));
        }
    }

    #[test]
    fn the_inner_nfd_orders_before_folding() {
        // U+0345 (class 240) sorts after U+0301 (class 230) first, then folds to the starter U+03B9.
        assert_eq!(fold_v1("\u{3B1}\u{345}\u{301}"), "\u{3B1}\u{301}\u{3B9}");
        // Per-character folding would have given α ι U+0301.
        assert_ne!(fold_v1("\u{3B1}\u{345}\u{301}"), "\u{3B1}\u{3B9}\u{301}");
        // U+1FB3 (α with ypogegrammeni) folds with F to α ι.
        assert_eq!(fold_v1("\u{1FB3}"), "\u{3B1}\u{3B9}");
    }

    #[test]
    fn canonical_ordering_and_nfd() {
        // ḍ̇: U+1E0B (d with dot above) + U+0323 (dot below, 220) → d U+0323 U+0307.
        assert_eq!(nfd("\u{1E0B}\u{323}"), "d\u{323}\u{307}");
        assert!(ceq("\u{1E0B}\u{323}", "\u{1E0D}\u{307}"));
        assert!(!ceq("\u{1E0B}", "\u{1E0D}"));
        assert!(ceq("caf\u{E9}", "cafe\u{301}"));
        assert!(!ceq("Plan", "plan"));
        // Leading non-starters are ordered too.
        assert_eq!(nfd("\u{301}\u{323}"), "\u{323}\u{301}");
        assert_eq!(canonical_combining_class('\u{301}'), 230);
        assert_eq!(canonical_combining_class('\u{345}'), 240);
        assert_eq!(canonical_combining_class('a'), 0);
    }

    #[test]
    fn hangul_and_table_decompositions() {
        let d: Vec<char> = canonical_decomposition('\u{D55C}').unwrap().collect();
        assert_eq!(d, ['\u{1112}', '\u{1161}', '\u{11AB}']);
        let d = canonical_decomposition('\u{AC00}').unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d.collect::<String>(), "\u{1100}\u{1161}");
        assert_eq!(
            canonical_decomposition('\u{212B}')
                .unwrap()
                .collect::<String>(),
            "A\u{30A}"
        );
        assert!(canonical_decomposition('A').is_none());
        assert!(
            canonical_decomposition('\u{1C5}').is_none(),
            "compatibility only"
        );
        assert_eq!(case_fold('A'), Some(&['a'][..]));
        assert_eq!(case_fold('\u{DF}'), Some(&['s', 's'][..]));
        assert_eq!(case_fold('\u{1E9E}'), Some(&['s', 's'][..]), "F, not S");
        assert_eq!(case_fold('a'), None);
        assert_eq!(case_fold('\u{10FFFF}'), None);
        assert_eq!(UNICODE_VERSION, (17, 0, 0));
    }

    #[test]
    fn long_runs_of_marks_spill_and_stay_ordered() {
        // 40 alternating marks of classes 230 and 220 after a starter: the run exceeds the inline buffer.
        let mut x = String::from("A");
        for i in 0..40 {
            x.push(if i % 2 == 0 { '\u{301}' } else { '\u{323}' });
        }
        x.push('B');
        x.push('\u{301}');
        let want: String = std::iter::once('a')
            .chain(std::iter::repeat_n('\u{323}', 20))
            .chain(std::iter::repeat_n('\u{301}', 20))
            .chain(['b', '\u{301}'])
            .collect();
        assert_eq!(fold_v1(&x), want);
        assert_eq!(fold_chars(&x).count(), 43);
    }

    #[test]
    fn unassigned_and_private_use_map_to_themselves() {
        for c in ['\u{378}', '\u{E000}', '\u{10FFFD}', '\u{EFFFF}'] {
            let s = c.to_string();
            assert_eq!(fold_v1(&s), s);
            assert_eq!(nfd(&s), s);
        }
    }

    /// Strings over an alphabet that exercises every path: ASCII, decomposable letters, marks of several classes,
    /// U+0345, F foldings, Hangul, a Kelvin sign and supplementary-plane letters.
    fn mixed() -> impl Strategy<Value = String> {
        let alphabet = vec![
            'a',
            'A',
            'z',
            'Z',
            'k',
            'K',
            's',
            'S',
            '.',
            '/',
            ' ',
            '0',
            '\u{C0}',
            '\u{E9}',
            '\u{DF}',
            '\u{1E9E}',
            '\u{130}',
            '\u{131}',
            '\u{212A}',
            '\u{212B}',
            '\u{2126}',
            '\u{300}',
            '\u{301}',
            '\u{323}',
            '\u{345}',
            '\u{3B1}',
            '\u{391}',
            '\u{1FB3}',
            '\u{1F82}',
            '\u{FB03}',
            '\u{AC00}',
            '\u{D55C}',
            '\u{1100}',
            '\u{1161}',
            '\u{11AB}',
            '\u{10400}',
            '\u{10428}',
            '\u{1E900}',
            '\u{1D15E}',
            '\u{F900}',
        ];
        proptest::collection::vec(proptest::sample::select(alphabet), 0..24)
            .prop_map(|v| v.into_iter().collect())
    }

    proptest! {
        #![proptest_config(test_config(256))]

        #[test]
        fn ascii_fast_paths_agree_with_the_pipeline(x in "[ -~]{0,40}", y in "[ -~]{0,40}") {
            let slow: String = fold_chars(&x).collect();
            prop_assert_eq!(fold_v1(&x), slow);
            prop_assert_eq!(nfd(&x), nfd_chars(&x).collect::<String>());
            prop_assert_eq!(fold_eq(&x, &y), fold_chars(&x).eq(fold_chars(&y)));
            prop_assert_eq!(fold_cmp(&x, &y), fold_chars(&x).cmp(fold_chars(&y)));
            prop_assert_eq!(ceq(&x, &y), x == y);
        }

        #[test]
        fn predicates_agree_with_the_folded_strings(x in mixed(), y in mixed()) {
            let (fx, fy) = (fold_v1(&x), fold_v1(&y));
            prop_assert_eq!(fold_eq(&x, &y), fx == fy);
            prop_assert_eq!(fold_cmp(&x, &y), fx.as_bytes().cmp(fy.as_bytes()));
            prop_assert_eq!(fold_matches(&fx, &y), fx == fy);
            prop_assert_eq!(ceq(&x, &y), nfd(&x) == nfd(&y));
        }

        #[test]
        fn fold_is_idempotent_and_nfd_stable(x in mixed()) {
            let f = fold_v1(&x);
            prop_assert_eq!(fold_v1(&f), f.clone());
            prop_assert_eq!(nfd(&f), f);
            let d = nfd(&x);
            prop_assert_eq!(nfd(&d), d.clone());
            prop_assert_eq!(fold_v1(&d), fold_v1(&x));
            let mut appended = String::from("prefix/");
            fold_v1_into(&x, &mut appended);
            prop_assert_eq!(appended, format!("prefix/{}", fold_v1(&x)));
        }
    }
}
