//! The `symbol` and `heading` authoring forms ([F21 §6]): how a spec splits into a path and a selector (§6.1), the
//! selector's segments and escapes, the matching rules for Rust (§6.2), TOML (§6.3) and Markdown (§6.4), and the
//! outcomes of a capture (§6.5).
//!
//! While [F20 §6.1]'s interim scanner rule holds, capture refuses both forms before any of this runs ([F21 §6.5]);
//! that check is the resolver's.

use super::items::{Items, NO_PARENT, Stored};
use super::{Lang, SCOPE_MAX_BYTES, ScanFailed};
use crate::text::eqi;

/// `skind` of a Rust `impl` ([F08 §10.3.1]).
const SK_IMPL: u8 = 2;

fn ends_with_eqi(x: &[u8], suffix: &[u8]) -> bool {
    x.len() >= suffix.len() && eqi(&x[x.len() - suffix.len()..], suffix)
}

/// The `symbol` form `path::S1/…/Sk` ([F21 §6.1]): split at the first `::` whose prefix ends in `.rs` or `.toml`
/// (`eqi`) into (path, selector); `None` when the spec has no such `::`.
#[must_use]
pub fn split_symbol(spec: &str) -> Option<(&str, &str)> {
    let b = spec.as_bytes();
    let mut k = 0;
    while k + 1 < b.len() {
        if b[k] == b':'
            && b[k + 1] == b':'
            && (ends_with_eqi(&b[..k], b".rs") || ends_with_eqi(&b[..k], b".toml"))
        {
            return Some((&spec[..k], &spec[k + 2..]));
        }
        k += 1;
    }
    None
}

/// The `heading` form `path#H1/…/Hk` ([F21 §6.1]): split at the first `#` whose prefix ends in `.md` or
/// `.markdown` (`eqi`) into (path, selector); `None` when the spec has no such `#`.
#[must_use]
pub fn split_heading(spec: &str) -> Option<(&str, &str)> {
    let b = spec.as_bytes();
    (0..b.len())
        .find(|&k| {
            b[k] == b'#' && (ends_with_eqi(&b[..k], b".md") || ends_with_eqi(&b[..k], b".markdown"))
        })
        .map(|k| (&spec[..k], &spec[k + 1..]))
}

/// Which authoring form a selector belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormKind {
    /// `path::S1/…/Sk`, on a Rust or TOML file.
    Symbol,
    /// `path#H1/…/Hk`, on a Markdown file.
    Heading,
}

/// A malformed selector ([F21 §6.1]; the "syntax" outcome of §6.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectorError {
    /// An empty segment (two `/` in a row, or one at an end).
    EmptySegment,
    /// A `symbol` segment that is neither `X` nor `X[Y]` with non-empty X and Y free of `/`, `[` and `]`.
    Malformed,
    /// A segment longer than [`SCOPE_MAX_BYTES`] bytes after its escapes: the whole segment, so for `X[Y]` the
    /// bytes of X and Y and the two brackets (spec finding of WP-63's review round 2, on [F21 §6.1]).
    TooLong,
}

impl std::fmt::Display for SelectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SelectorError::EmptySegment => "selector: an empty segment",
            SelectorError::Malformed => "selector: a segment is neither X nor X[Y]",
            SelectorError::TooLong => "selector: a segment longer than 4096 bytes",
        })
    }
}

impl std::error::Error for SelectorError {}

/// One segment of a selector, its escapes resolved: `X`, or `X[Y]` for a trait impl.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SelectorSegment {
    /// X, or the whole heading text.
    pub x: String,
    /// Y of `X[Y]`.
    pub y: Option<String>,
}

/// The selector of a `symbol` or `heading` form: k ≥ 1 segments ([F21 §6.1]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Selector {
    kind: FormKind,
    segments: Vec<SelectorSegment>,
}

/// Resolves `%2F`, `%5B`, `%5D` and `%25` (either case) to `/`, `[`, `]` and `%`; every other `%` is itself.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut from = 0;
    let mut k = 0;
    while k < b.len() {
        if b[k] == b'%' && k + 2 < b.len() {
            let c = match (b[k + 1].to_ascii_uppercase(), b[k + 2].to_ascii_uppercase()) {
                (b'2', b'F') => Some('/'),
                (b'5', b'B') => Some('['),
                (b'5', b'D') => Some(']'),
                (b'2', b'5') => Some('%'),
                _ => None,
            };
            if let Some(c) = c {
                out.push_str(&s[from..k]);
                out.push(c);
                k += 3;
                from = k;
                continue;
            }
        }
        k += 1;
    }
    out.push_str(&s[from..]);
    out
}

impl Selector {
    /// Parses a selector of form `kind` ([F21 §6.1]): split at every `/`; a `symbol` segment is `X`, or `X[Y]`
    /// when it ends in `]` (its first `[` starting Y), with X and Y non-empty and free of `/`, `[` and `]`; a
    /// `heading` segment is any non-empty text. The escapes are resolved after the split.
    ///
    /// # Errors
    /// [`SelectorError`] for a malformed selector.
    pub fn parse(kind: FormKind, selector: &str) -> Result<Selector, SelectorError> {
        let mut segments = Vec::new();
        for raw in selector.split('/') {
            if raw.is_empty() {
                return Err(SelectorError::EmptySegment);
            }
            let seg = match kind {
                FormKind::Heading => SelectorSegment {
                    x: unescape(raw),
                    y: None,
                },
                FormKind::Symbol => {
                    let bracket = |s: &str| s.contains(['[', ']']);
                    if let Some(inner) = raw.strip_suffix(']') {
                        let Some(k) = inner.find('[') else {
                            return Err(SelectorError::Malformed);
                        };
                        let (x, y) = (&inner[..k], &inner[k + 1..]);
                        if x.is_empty() || y.is_empty() || bracket(x) || bracket(y) {
                            return Err(SelectorError::Malformed);
                        }
                        SelectorSegment {
                            x: unescape(x),
                            y: Some(unescape(y)),
                        }
                    } else if bracket(raw) {
                        return Err(SelectorError::Malformed);
                    } else {
                        SelectorSegment {
                            x: unescape(raw),
                            y: None,
                        }
                    }
                }
            };
            let len = seg.x.len() + seg.y.as_ref().map_or(0, |y| y.len() + 2);
            if len > SCOPE_MAX_BYTES {
                return Err(SelectorError::TooLong);
            }
            segments.push(seg);
        }
        Ok(Selector { kind, segments })
    }

    /// The form.
    #[must_use]
    pub fn kind(&self) -> FormKind {
        self.kind
    }

    /// The segments, S1 first.
    #[must_use]
    pub fn segments(&self) -> &[SelectorSegment] {
        &self.segments
    }
}

/// The outcome of a `symbol` or `heading` capture ([F21 §6.5]); the syntax case is [`Selector::parse`]'s error.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FormOutcome {
    /// Exactly one item matches, and its name path is recordable and names one item: the anchor's item.
    Found(usize),
    /// No item matches.
    NotFound,
    /// Two or more items match, or the one match's name path names several items: the items to list.
    Several(Vec<usize>),
    /// The one match's name path is not recordable ([F21 §2.3]).
    NotRecordable(usize),
    /// The scan failed ([F21 §2.7]).
    FailedScan,
}

/// The end of the run of word bytes at `b[i..]` ([F21 §3.1] rule 3: ASCII letters, digits, `_`, and every byte ≥
/// `80` at which no whitespace sequence starts).
fn word_end(b: &[u8], i: usize) -> usize {
    let mut k = i;
    while let Some(&c) = b.get(k) {
        let space = match c {
            0xC2 => b.get(k + 1) == Some(&0x85),
            0xE2 => {
                b.get(k + 1) == Some(&0x80)
                    && matches!(b.get(k + 2), Some(0x8E | 0x8F | 0xA8 | 0xA9))
            }
            _ => false,
        };
        if space || !(c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80) {
            break;
        }
        k += 1;
    }
    k
}

/// The bare name of a Rust `name` or `qual` ([F21 §6.2]): for `[!] w1::…::wm[<…]` — words of [F21 §3.1], a raw
/// identifier being one — the `!` and wm; `None` for any other form.
fn bare(v: &str) -> Option<(bool, &str)> {
    let b = v.as_bytes();
    let bang = b.first() == Some(&b'!');
    let mut a = usize::from(bang);
    loop {
        let mut z = word_end(b, a);
        if z == a {
            return None;
        }
        if &b[a..z] == b"r" && b.get(z) == Some(&b'#') && word_end(b, z + 1) > z + 1 {
            z = word_end(b, z + 1);
        }
        match b.get(z) {
            // `a` follows `!`, `::` or the start and `z` stops at an ASCII byte or the lead of a whitespace
            // character, so both are character boundaries.
            None | Some(b'<') => return Some((bang, &v[a..z])),
            Some(b':') if b.get(z + 1) == Some(&b':') => a = z + 2,
            Some(_) => return None,
        }
    }
}

/// [F21 §6.2]'s bare name of a Rust name or qualifier, read in pieces that each end at a character boundary: the
/// same function as [`bare`] over their concatenation, kept for a name longer than [`SCOPE_MAX_BYTES`] bytes, of
/// which an item keeps only a prefix ([F21 §2.3]). A word longer than [`SCOPE_MAX_BYTES`] bytes, which no selector
/// segment can equal, gives none.
#[derive(Clone, Debug, Default)]
pub(crate) struct Bare {
    st: BareSt,
    bang: bool,
    /// The current word wm, while it is at most [`SCOPE_MAX_BYTES`] bytes.
    word: String,
    word_long: bool,
}

/// Where [`Bare`] stands in the form `[!] w1::…::wm[<…]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BareSt {
    /// Nothing read.
    #[default]
    Start,
    /// After the `!` or a `::`: a word must start.
    WordStart,
    /// In a word.
    Word,
    /// After the word `r` and a `#`: the rest of a raw identifier must follow.
    RawHash,
    /// After a word and one `:`.
    Colon,
    /// A `<` followed the last word, which is the bare name whatever comes after.
    Done,
    /// The form does not hold.
    Fail,
}

impl Bare {
    /// Whether no later byte can change the result.
    pub(crate) fn decided(&self) -> bool {
        matches!(self.st, BareSt::Done | BareSt::Fail)
    }

    /// The next piece of the spelling.
    pub(crate) fn feed(&mut self, piece: &str) {
        let b = piece.as_bytes();
        let mut k = 0;
        while k < b.len() && !self.decided() {
            let z = word_end(b, k);
            if z > k {
                match self.st {
                    BareSt::Start | BareSt::WordStart => {
                        self.word.clear();
                        self.word_long = false;
                    }
                    BareSt::Word | BareSt::RawHash => {}
                    _ => {
                        self.st = BareSt::Fail;
                        return;
                    }
                }
                // `k` follows a non-word ASCII byte or the piece's start, and `z` stops at an ASCII byte, the lead
                // of a whitespace character or the piece's end: both are character boundaries.
                self.push_word(&piece[k..z]);
                self.st = BareSt::Word;
                k = z;
                continue;
            }
            self.st = match (self.st, b[k]) {
                (BareSt::Start, b'!') => {
                    self.bang = true;
                    BareSt::WordStart
                }
                (BareSt::Word, b'#') if self.word == "r" => {
                    self.push_word("#");
                    BareSt::RawHash
                }
                (BareSt::Word, b'<') => BareSt::Done,
                (BareSt::Word, b':') => BareSt::Colon,
                (BareSt::Colon, b':') => BareSt::WordStart,
                _ => BareSt::Fail,
            };
            k += 1;
        }
    }

    fn push_word(&mut self, s: &str) {
        if self.word_long {
            return;
        }
        if self.word.len() + s.len() > SCOPE_MAX_BYTES {
            self.word_long = true;
            self.word = String::new();
        } else {
            self.word.push_str(s);
        }
    }

    /// The bare name of what was read: the `!` flag and wm; `None` when the form does not hold or wm is too long.
    pub(crate) fn result(&self) -> Option<(bool, &str)> {
        (matches!(self.st, BareSt::Word | BareSt::Done) && !self.word_long)
            .then_some((self.bang, self.word.as_str()))
    }
}

/// Whether `x` is the bare name `[!] w` ([F21 §6.2]): `!w` when the name has the `!`, else `w`.
fn bare_hit(x: &str, (bang, w): (bool, &str)) -> bool {
    if bang {
        x.strip_prefix('!') == Some(w)
    } else {
        x == w
    }
}

/// Whether `x` equals Rust name `n` (`Some(true)`), is its bare name (`Some(false)`), or neither.
fn match_name(x: &str, n: &str) -> Option<bool> {
    if x == n {
        return Some(true);
    }
    bare_hit(x, bare(n)?).then_some(false)
}

/// [`match_name`] over a stored name or qualifier: a long one ([F21 §2.3]) equals no `x`, which is at most
/// [`SCOPE_MAX_BYTES`] bytes, and matches only by its bare name.
fn match_stored(x: &str, v: Stored<'_>) -> Option<bool> {
    if !v.long {
        return match_name(x, v.text);
    }
    bare_hit(x, v.bare?).then_some(false)
}

/// Whether item `i` matches selector segment `s`, and whether by equality; `last` for Sk.
fn seg_match(items: &Items, i: usize, s: &SelectorSegment, last: bool) -> Option<bool> {
    let (name, qual, skind) = (items.name_stored(i), items.qual_stored(i), items.skind(i));
    let qual_empty = qual.text.is_empty() && !qual.long;
    match (items.lang(), &s.y) {
        (Lang::Rust, Some(y)) => {
            if skind != SK_IMPL || qual_empty {
                return None;
            }
            let ex = match_stored(&s.x, name)?;
            let ey = match_stored(y, qual)?;
            Some(ex && ey)
        }
        (Lang::Rust, None) => {
            if !qual_empty || (last && skind == SK_IMPL) {
                return None;
            }
            match_stored(&s.x, name)
        }
        (Lang::Toml, None) => (!name.long && s.x == name.text).then_some(true),
        (Lang::Markdown, None) => {
            let h = s.x.as_str();
            ((!name.long && h == name.text)
                || items.heading_text_is(i, h)
                || (!qual_empty && !qual.long && h == qual.text))
                .then_some(true)
        }
        // A bracketed segment names only a Rust trait impl; a heading selector has none.
        (Lang::Toml | Lang::Markdown, Some(_)) => None,
    }
}

/// Whether item `i` matches the whole selector by suffix ([F21 §6.2] "Path match"), and whether every segment
/// matched by equality.
fn path_match(items: &Items, i: usize, segs: &[SelectorSegment]) -> Option<bool> {
    let mut x = i;
    let mut all_eq = true;
    for (k, s) in segs.iter().enumerate().rev() {
        if x == NO_PARENT {
            return None;
        }
        all_eq &= seg_match(items, x, s, k + 1 == segs.len())?;
        x = items.parent(x);
    }
    Some(all_eq)
}

/// Finds the item a `symbol` or `heading` form names ([F21 §6.2–§6.5]) in a file's scan. The conditions are tested
/// in §6.5's order: failed scan, not found, several matches, not recordable, a name path naming several items,
/// found. A form whose kind does not fit the file's language matches nothing.
#[must_use]
pub fn find_form(scan: &Result<Items, ScanFailed>, selector: &Selector) -> FormOutcome {
    let Ok(items) = scan else {
        return FormOutcome::FailedScan;
    };
    let fits = matches!(
        (selector.kind, items.lang()),
        (FormKind::Symbol, Lang::Rust | Lang::Toml) | (FormKind::Heading, Lang::Markdown)
    );
    if !fits {
        return FormOutcome::NotFound;
    }
    let (mut exact, mut all) = (Vec::new(), Vec::new());
    for i in 0..items.len() {
        if let Some(eq) = path_match(items, i, &selector.segments) {
            all.push(i);
            if eq {
                exact.push(i);
            }
        }
    }
    let hits = if exact.is_empty() { all } else { exact };
    match hits[..] {
        [] => FormOutcome::NotFound,
        [i] if !items.recordable(i) => FormOutcome::NotRecordable(i),
        [i] => {
            let twins = items.path_twins(i);
            if twins.len() > 1 {
                FormOutcome::Several(twins)
            } else {
                FormOutcome::Found(i)
            }
        }
        _ => FormOutcome::Several(hits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_of_f21_6_1() {
        assert_eq!(
            split_symbol("src/lock.rs::LockFile/acquire"),
            Some(("src/lock.rs", "LockFile/acquire"))
        );
        assert_eq!(split_symbol("a::b/x.rs::F"), Some(("a::b/x.rs", "F")));
        assert_eq!(
            split_symbol("Cargo.TOML::dependencies/serde"),
            Some(("Cargo.TOML", "dependencies/serde"))
        );
        assert_eq!(split_symbol("x.py::F"), None);
        assert_eq!(split_symbol("x.rs"), None);
        assert_eq!(
            split_heading("docs/C#/intro.md#Setup"),
            Some(("docs/C#/intro.md", "Setup"))
        );
        assert_eq!(
            split_heading("a.markdown#H1/H2"),
            Some(("a.markdown", "H1/H2"))
        );
        assert_eq!(split_heading("a.txt#H"), None);
    }

    #[test]
    fn selectors_and_escapes() {
        let s = Selector::parse(FormKind::Heading, "Input%2Foutput/Recovery").unwrap();
        assert_eq!(s.segments()[0].x, "Input/output");
        let s = Selector::parse(FormKind::Heading, "[Unreleased]").unwrap();
        assert_eq!(s.segments()[0].x, "[Unreleased]");
        let s = Selector::parse(FormKind::Symbol, "Wrap<u16>[From<u8>]/from").unwrap();
        assert_eq!(
            s.segments()[0],
            SelectorSegment {
                x: "Wrap<u16>".into(),
                y: Some("From<u8>".into())
            }
        );
        assert_eq!(unescape("%%2f%5b%5D%25%zz%2"), "%/[]%%zz%2");
        for bad in [
            "", "a//b", "a/", "A[]", "[B]", "A]", "A[B]]", "A[B[C]]", "A]B",
        ] {
            assert!(Selector::parse(FormKind::Symbol, bad).is_err(), "{bad}");
        }
        assert_eq!(
            Selector::parse(FormKind::Heading, &"x".repeat(4097)),
            Err(SelectorError::TooLong)
        );
        assert!(Selector::parse(FormKind::Heading, &"x".repeat(4096)).is_ok());
    }

    #[test]
    fn a_segment_is_measured_whole_after_its_escapes() {
        let sym = |x: usize, y: Option<usize>| {
            let mut seg = "x".repeat(x);
            if let Some(y) = y {
                seg.push('[');
                seg.push_str(&"y".repeat(y));
                seg.push(']');
            }
            Selector::parse(FormKind::Symbol, &format!("a/{seg}")).map(|s| s.segments().len())
        };
        assert_eq!(sym(4096, None), Ok(2));
        assert_eq!(sym(4097, None), Err(SelectorError::TooLong));
        // `X[Y]` counts X, Y and both brackets: 4,093 + 1 + 2 bytes pass, 4,094 + 1 + 2 do not.
        assert_eq!(sym(4093, Some(1)), Ok(2));
        assert_eq!(sym(4094, Some(1)), Err(SelectorError::TooLong));
        assert_eq!(sym(4096, Some(1)), Err(SelectorError::TooLong));
        assert_eq!(sym(1, Some(4093)), Ok(2));
        assert_eq!(sym(1, Some(4094)), Err(SelectorError::TooLong));
        // Escapes are resolved first: 1,366 escaped slashes are 4,098 bytes typed and 1,366 after.
        let s = Selector::parse(FormKind::Heading, &"%2F".repeat(1366)).unwrap();
        assert_eq!(s.segments()[0].x.len(), 1366);
        let s = Selector::parse(
            FormKind::Symbol,
            &format!("{}[{}]", "%5B".repeat(2000), "y".repeat(2000)),
        );
        assert_eq!(s.map(|s| s.segments()[0].x.len()), Ok(2000));
        let over = format!("{}[{}]", "%25".repeat(2048), "y".repeat(2047));
        assert_eq!(
            Selector::parse(FormKind::Symbol, &over),
            Err(SelectorError::TooLong)
        );
    }

    #[test]
    fn bare_names_of_f21_6_2() {
        assert_eq!(bare("Wrap<T>"), Some((false, "Wrap")));
        assert_eq!(bare("crate::a::Tr<u8>"), Some((false, "Tr")));
        assert_eq!(bare("!Send"), Some((true, "Send")));
        assert_eq!(bare("std::fmt::Display"), Some((false, "Display")));
        assert_eq!(bare("r#type"), Some((false, "r#type")));
        assert_eq!(bare("&'a mut[u8]"), None);
        assert_eq!(bare("(A,B)"), None);
        assert_eq!(bare("dyn Fn(u8)->u8+Send"), None);
        assert_eq!(bare("<T as Iterator>::Item"), None);
        assert_eq!(bare("a:b"), None);
        assert_eq!(bare("a:::b"), None);
        assert_eq!(bare("a::"), None);
        assert_eq!(bare("b\"x\""), None);
        assert_eq!(match_name("From", "From<T>"), Some(false));
        assert_eq!(match_name("From<T>", "From<T>"), Some(true));
        assert_eq!(match_name("!Send", "!Send"), Some(true));
        assert_eq!(match_name("Send", "!Send"), None);
    }

    fn bare_in_pieces(v: &str, cuts: &[usize]) -> Option<(bool, String)> {
        let mut at: Vec<usize> = cuts
            .iter()
            .map(|&c| c % (v.len() + 1))
            .filter(|&c| v.is_char_boundary(c))
            .collect();
        at.extend([0, v.len()]);
        at.sort_unstable();
        let mut b = Bare::default();
        for w in at.windows(2) {
            b.feed(&v[w[0]..w[1]]);
        }
        b.result().map(|(bang, w)| (bang, w.to_owned()))
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 2048,
            failure_persistence: None,
            ..proptest::prelude::ProptestConfig::default()
        })]

        /// The streaming bare name equals [`bare`] over the whole text, however the text is cut.
        #[test]
        fn streaming_bare_names_equal_the_whole_text(
            v in proptest::collection::vec(
                proptest::sample::select(&[
                    "a", "Tr", "r", "#", "r#", "!", ":", "::", "<", ">", "u8", "_", "é", "\u{85}", "\u{2028}",
                    " ", "(", "\"", "'a", "{", "1",
                ][..]),
                0..12,
            ),
            cuts in proptest::collection::vec(proptest::prelude::any::<usize>(), 0..6),
        ) {
            let v = v.concat();
            let want = bare(&v).map(|(bang, w)| (bang, w.to_owned()));
            proptest::prop_assert_eq!(bare_in_pieces(&v, &cuts), want, "{:?}", v);
        }
    }

    #[test]
    fn streaming_bare_names_of_long_spellings() {
        let long = format!("crate::a::Tr<{}>", "x".repeat(10_000));
        assert_eq!(
            bare_in_pieces(&long, &[3, 20, 5000]),
            Some((false, "Tr".to_owned()))
        );
        let path = format!("{}Last", "seg::".repeat(2000));
        assert_eq!(
            bare_in_pieces(&path, &[7]),
            Some((false, "Last".to_owned()))
        );
        let word = "w".repeat(4097);
        assert_eq!(bare_in_pieces(&word, &[]), None);
        assert_eq!(
            bare_in_pieces(&"w".repeat(4096), &[]).map(|(_, w)| w.len()),
            Some(4096)
        );
        let mut b = Bare::default();
        b.feed("!Sen");
        assert!(!b.decided());
        b.feed("d<");
        assert!(b.decided());
        assert_eq!(b.result(), Some((true, "Send")));
    }

    #[test]
    fn stored_fields_match_long_names_by_their_bare_names_only() {
        let long = |bare| Stored {
            text: "Foo<{",
            long: true,
            bare,
        };
        assert_eq!(match_stored("Foo", long(Some((false, "Foo")))), Some(false));
        assert_eq!(match_stored("Foo<{", long(Some((false, "Foo")))), None);
        assert_eq!(match_stored("Foo", long(None)), None);
        assert_eq!(
            match_stored("!Send", long(Some((true, "Send")))),
            Some(false)
        );
        let short = Stored {
            text: "Foo<T>",
            long: false,
            bare: None,
        };
        assert_eq!(match_stored("Foo<T>", short), Some(true));
        assert_eq!(match_stored("Foo", short), Some(false));
    }
}
