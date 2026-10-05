//! The scope value of [F08 §10.3.1] — `lang`, `n` and n segments of (`skind`, `name`, `qual`) — which a capture
//! records ([F21 §2.3, §2.4]) and a resolve looks up ([F21 §2.5]), and its scope text ([F14 §5.6]).

use std::fmt;

use super::{Lang, SCOPE_MAX_SEGMENTS};

/// One segment of a name path: an item's kind, name and qualifier ([F21 §2.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Segment<'a> {
    /// The item kind of [F08 §10.3.1] in the scope's language.
    pub skind: u8,
    /// The item's name.
    pub name: &'a str,
    /// The item's qualifier; empty for most items.
    pub qual: &'a str,
}

/// A scope value ([F08 §10.3.1]): valid bytes with the positions of their segments' fields.
///
/// A capture builds one from an item's name path ([`Items::scope_of`](super::Items::scope_of),
/// [`Items::capture_scope`](super::Items::capture_scope)); a stored anchor's bytes are read back with
/// [`Scope::from_bytes`]. Its [`Display`](fmt::Display) form is the scope text of [F14 §5.6]. The names and
/// qualifiers are read out of the value's bytes, which hold them once.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Scope {
    lang: Lang,
    bytes: Vec<u8>,
    segs: Vec<SegIx>,
}

/// A segment's kind and the byte ranges of its name and qualifier in the value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct SegIx {
    skind: u8,
    name: (usize, usize),
    qual: (usize, usize),
}

/// Why bytes are not a scope value ([F08 §10.3.1], [F01 §5.2] for the `uvar32` lengths).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScopeError {
    /// The bytes end inside a field.
    Truncated,
    /// A `lang` byte outside 1–3.
    Lang(u8),
    /// A segment count outside 1–64.
    Count(u8),
    /// An `skind` the language does not define.
    Kind(u8),
    /// A length that is not a canonical `uvar32`.
    Varint,
    /// A name or qualifier that is not valid UTF-8.
    Utf8,
    /// An empty name.
    EmptyName,
    /// A name or qualifier holding U+0000, CR or LF.
    Control,
    /// Bytes after the last segment.
    Trailing,
}

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScopeError::Truncated => f.write_str("scope value: truncated"),
            ScopeError::Lang(b) => write!(f, "scope value: lang {b} is not 1-3"),
            ScopeError::Count(n) => write!(f, "scope value: {n} segments, not 1-64"),
            ScopeError::Kind(k) => {
                write!(f, "scope value: skind {k} is not defined for the language")
            }
            ScopeError::Varint => f.write_str("scope value: a length is not a canonical uvar32"),
            ScopeError::Utf8 => f.write_str("scope value: a name or qualifier is not UTF-8"),
            ScopeError::EmptyName => f.write_str("scope value: an empty name"),
            ScopeError::Control => {
                f.write_str("scope value: a name or qualifier holds U+0000, CR or LF")
            }
            ScopeError::Trailing => f.write_str("scope value: bytes after the last segment"),
        }
    }
}

impl std::error::Error for ScopeError {}

/// The number of bytes of `v` as unsigned LEB128 ([F01 §5.2]).
pub(crate) const fn leb_len(v: usize) -> usize {
    let bits = usize::BITS - v.leading_zeros();
    if bits == 0 {
        1
    } else {
        bits.div_ceil(7) as usize
    }
}

fn put_vstr(out: &mut Vec<u8>, s: &str) {
    let mut v = s.len();
    loop {
        let low = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(low);
            break;
        }
        out.push(low | 0x80);
    }
    out.extend_from_slice(s.as_bytes());
}

fn read_uvar32(b: &[u8]) -> Result<(usize, &[u8]), ScopeError> {
    let mut v: u64 = 0;
    for (k, &byte) in b.iter().enumerate().take(5) {
        v |= u64::from(byte & 0x7F) << (7 * k);
        if byte & 0x80 == 0 {
            if k > 0 && byte == 0 {
                return Err(ScopeError::Varint);
            }
            let v = u32::try_from(v).map_err(|_| ScopeError::Varint)?;
            let v = usize::try_from(v).map_err(|_| ScopeError::Varint)?;
            return Ok((v, &b[k + 1..]));
        }
    }
    Err(if b.len() < 5 {
        ScopeError::Truncated
    } else {
        ScopeError::Varint
    })
}

fn read_vstr(b: &[u8]) -> Result<(&str, &[u8]), ScopeError> {
    let (len, rest) = read_uvar32(b)?;
    if rest.len() < len {
        return Err(ScopeError::Truncated);
    }
    let s = std::str::from_utf8(&rest[..len]).map_err(|_| ScopeError::Utf8)?;
    if s.bytes().any(|c| matches!(c, 0x00 | 0x0A | 0x0D)) {
        return Err(ScopeError::Control);
    }
    Ok((s, &rest[len..]))
}

impl Scope {
    /// Reads a scope value ([F08 §10.3.1]): `lang` 1–3, `n` 1–64, and n segments of `skind` (defined for the
    /// language), `name` (non-empty) and `qual` as `vstr` — valid UTF-8 without U+0000, CR or LF, with canonical
    /// `uvar32` lengths ([F01 §5.2]). The 4,096-byte bound of [F21 §2.3] binds capture only, so a longer imported
    /// value is read ([F21] open point 7) and resolves like any other, except that a segment whose name is longer
    /// than 4,096 bytes names no item ([`Items::matching`](super::Items::matching)).
    ///
    /// # Errors
    /// The first defect found, as a [`ScopeError`].
    pub fn from_bytes(b: &[u8]) -> Result<Scope, ScopeError> {
        let (&lang, rest) = b.split_first().ok_or(ScopeError::Truncated)?;
        let lang = Lang::from_code(lang).ok_or(ScopeError::Lang(lang))?;
        let (&n, mut rest) = rest.split_first().ok_or(ScopeError::Truncated)?;
        if n == 0 || usize::from(n) > SCOPE_MAX_SEGMENTS {
            return Err(ScopeError::Count(n));
        }
        let mut segs = Vec::with_capacity(usize::from(n));
        // The byte range of a field just read, from what is left after it.
        let range = |field: &str, left: &[u8]| {
            let end = b.len() - left.len();
            (end - field.len(), end)
        };
        for _ in 0..n {
            let (&skind, r) = rest.split_first().ok_or(ScopeError::Truncated)?;
            if lang.skind_name(skind).is_none() {
                return Err(ScopeError::Kind(skind));
            }
            let (name, r) = read_vstr(r)?;
            if name.is_empty() {
                return Err(ScopeError::EmptyName);
            }
            let name = range(name, r);
            let (qual, r) = read_vstr(r)?;
            let qual = range(qual, r);
            rest = r;
            segs.push(SegIx { skind, name, qual });
        }
        if !rest.is_empty() {
            return Err(ScopeError::Trailing);
        }
        Ok(Scope {
            lang,
            bytes: b.to_vec(),
            segs,
        })
    }

    /// The scope value of a name path given by its segments, checked as [`Scope::from_bytes`] checks bytes.
    ///
    /// # Errors
    /// As [`Scope::from_bytes`].
    pub fn from_segments(lang: Lang, segments: &[Segment<'_>]) -> Result<Scope, ScopeError> {
        let n = u8::try_from(segments.len()).map_err(|_| ScopeError::Count(u8::MAX))?;
        let mut b = vec![lang.code(), n];
        for s in segments {
            if u32::try_from(s.name.len()).is_err() || u32::try_from(s.qual.len()).is_err() {
                return Err(ScopeError::Varint);
            }
            b.push(s.skind);
            put_vstr(&mut b, s.name);
            put_vstr(&mut b, s.qual);
        }
        Scope::from_bytes(&b)
    }

    /// The scope value of the segments of an item's name path, which the scanner guarantees valid (non-empty,
    /// recordable, known kinds, one-line UTF-8 names).
    pub(crate) fn encode<'a>(lang: Lang, segments: impl Iterator<Item = Segment<'a>>) -> Scope {
        let mut bytes = vec![lang.code(), 0];
        let mut segs = Vec::new();
        for s in segments {
            bytes.push(s.skind);
            put_vstr(&mut bytes, s.name);
            let name = (bytes.len() - s.name.len(), bytes.len());
            put_vstr(&mut bytes, s.qual);
            let qual = (bytes.len() - s.qual.len(), bytes.len());
            segs.push(SegIx {
                skind: s.skind,
                name,
                qual,
            });
        }
        debug_assert!(
            (1..=SCOPE_MAX_SEGMENTS).contains(&segs.len()),
            "a recordable name path"
        );
        bytes[1] = u8::try_from(segs.len()).unwrap_or(u8::MAX);
        Scope { lang, bytes, segs }
    }

    /// The bytes of the value, as they enter `captured` and the selector block ([F08 §11.4], [F07 §8.2]).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The scope's language.
    #[must_use]
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// The number of segments, 1–64.
    #[must_use]
    pub fn len(&self) -> usize {
        self.segs.len()
    }

    /// Always `false`: a scope value has at least one segment.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    /// The segments, outermost first.
    #[must_use]
    pub fn segments(&self) -> Segments<'_> {
        Segments { scope: self, i: 0 }
    }

    /// The last segment, whose kind is the anchor's item kind ([F21 §2.6]).
    #[must_use]
    pub fn last(&self) -> Option<Segment<'_>> {
        self.segs.last().map(|s| self.seg(s))
    }

    fn seg(&self, s: &SegIx) -> Segment<'_> {
        Segment {
            skind: s.skind,
            name: self.field(s.name),
            qual: self.field(s.qual),
        }
    }

    /// A name or qualifier: valid UTF-8, checked when the value was read or built from `str`s.
    fn field(&self, (a, b): (usize, usize)) -> &str {
        std::str::from_utf8(&self.bytes[a..b]).unwrap_or_default()
    }
}

impl fmt::Debug for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Scope({self})")
    }
}

/// The segments of a [`Scope`], outermost first.
#[derive(Clone, Debug)]
pub struct Segments<'a> {
    scope: &'a Scope,
    i: usize,
}

impl<'a> Iterator for Segments<'a> {
    type Item = Segment<'a>;

    fn next(&mut self) -> Option<Segment<'a>> {
        let s = self.scope.segs.get(self.i)?;
        self.i += 1;
        Some(self.scope.seg(s))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.scope.segs.len() - self.i;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Segments<'_> {}

/// Writes `s` as a scope-text name ([F14 §5.6]): each byte of `%`, `/`, `[`, `]` and of a control character as `%`
/// and two lower-case hexadecimal digits.
fn put_escaped(out: &mut String, s: &str) {
    for ch in s.chars() {
        if matches!(ch, '%' | '/' | '[' | ']') || ch.is_control() {
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).bytes() {
                out.push('%');
                out.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
                out.push(char::from(b"0123456789abcdef"[usize::from(b & 0x0F)]));
            }
        } else {
            out.push(ch);
        }
    }
}

/// The mark written after the kept prefix of a long name or qualifier ([F21 §2.3]) in a listed name path: `%` and
/// `…`. A scope text escapes every `%` of a name as `%25` ([F14 §5.6]), so no real name's text holds it.
pub(crate) const LONG_MARK: &str = "%\u{2026}";

/// The scope text of [F14 §5.6] for a language and segments: `lang ":" seg *("/" seg)`, a segment being its kind
/// name, SP, its escaped name and, when the qualifier is not empty, `[` the escaped qualifier `]`. Each segment comes
/// with whether its name and its qualifier are long prefixes, which are followed by [`LONG_MARK`].
pub(crate) fn scope_text<'a>(
    lang: Lang,
    segments: impl Iterator<Item = (Segment<'a>, bool, bool)>,
) -> String {
    let mut out = String::from(lang.name());
    out.push(':');
    for (k, (s, name_long, qual_long)) in segments.enumerate() {
        if k > 0 {
            out.push('/');
        }
        out.push_str(lang.skind_name(s.skind).unwrap_or("?"));
        out.push(' ');
        put_escaped(&mut out, s.name);
        if name_long {
            out.push_str(LONG_MARK);
        }
        if !s.qual.is_empty() {
            out.push('[');
            put_escaped(&mut out, s.qual);
            if qual_long {
                out.push_str(LONG_MARK);
            }
            out.push(']');
        }
    }
    out
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&scope_text(
            self.lang,
            self.segments().map(|s| (s, false, false)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn informative_encodings_of_f21_2_3() {
        let md = Scope::from_segments(
            Lang::Markdown,
            &[
                Segment {
                    skind: 1,
                    name: "Design",
                    qual: "1",
                },
                Segment {
                    skind: 2,
                    name: "Storage",
                    qual: "1.1",
                },
            ],
        )
        .unwrap();
        assert_eq!(
            md.as_bytes(),
            [
                0x02, 0x02, 0x01, 0x06, 0x44, 0x65, 0x73, 0x69, 0x67, 0x6E, 0x01, 0x31, 0x02, 0x07,
                0x53, 0x74, 0x6F, 0x72, 0x61, 0x67, 0x65, 0x03, 0x31, 0x2E, 0x31
            ]
        );
        assert_eq!(md.to_string(), "markdown:h1 Design[1]/h2 Storage[1.1]");
        let toml = Scope::from_segments(
            Lang::Toml,
            &[
                Segment {
                    skind: 1,
                    name: "package",
                    qual: "",
                },
                Segment {
                    skind: 3,
                    name: "name",
                    qual: "",
                },
            ],
        )
        .unwrap();
        assert_eq!(
            toml.as_bytes(),
            [
                0x03, 0x02, 0x01, 0x07, 0x70, 0x61, 0x63, 0x6B, 0x61, 0x67, 0x65, 0x00, 0x03, 0x04,
                0x6E, 0x61, 0x6D, 0x65, 0x00
            ]
        );
        assert_eq!(toml.to_string(), "toml:table package/key name");
        assert_eq!(Scope::from_bytes(toml.as_bytes()), Ok(toml));
    }

    #[test]
    fn scope_text_escapes() {
        let s = Scope::from_segments(
            Lang::Rust,
            &[
                Segment {
                    skind: 2,
                    name: "S<[u8; 4]>",
                    qual: "Tr%/\u{7}",
                },
                Segment {
                    skind: 3,
                    name: "f\u{85}",
                    qual: "",
                },
            ],
        )
        .unwrap();
        assert_eq!(
            s.to_string(),
            "rust:impl S<%5bu8; 4%5d>[Tr%25%2f%07]/fn f%c2%85"
        );
    }

    #[test]
    fn decoding_refuses_defects() {
        assert_eq!(Scope::from_bytes(&[]), Err(ScopeError::Truncated));
        assert_eq!(Scope::from_bytes(&[4, 1]), Err(ScopeError::Lang(4)));
        assert_eq!(Scope::from_bytes(&[1, 0]), Err(ScopeError::Count(0)));
        assert_eq!(Scope::from_bytes(&[1, 65]), Err(ScopeError::Count(65)));
        assert_eq!(
            Scope::from_bytes(&[1, 1, 10, 1, b'a', 0]),
            Err(ScopeError::Kind(10))
        );
        assert_eq!(
            Scope::from_bytes(&[3, 1, 4, 1, b'a', 0]),
            Err(ScopeError::Kind(4))
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 0, 0]),
            Err(ScopeError::EmptyName)
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 0x81, 0x00, b'a', 0]),
            Err(ScopeError::Varint)
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 1, 0xFF, 0]),
            Err(ScopeError::Utf8)
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 1, b'\n', 0]),
            Err(ScopeError::Control)
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 1, b'a', 0, 9]),
            Err(ScopeError::Trailing)
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 2, b'a']),
            Err(ScopeError::Truncated)
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 0x80, 0x80, 0x80, 0x80, 0x80]),
            Err(ScopeError::Varint)
        );
        assert_eq!(
            Scope::from_bytes(&[1, 1, 3, 0x80, 0x80]),
            Err(ScopeError::Truncated)
        );
        let ok = Scope::from_bytes(&[1, 1, 3, 1, b'a', 0]).unwrap();
        assert_eq!(ok.to_string(), "rust:fn a");
        assert_eq!(
            ok.last(),
            Some(Segment {
                skind: 3,
                name: "a",
                qual: ""
            })
        );
    }

    #[test]
    fn leb_lengths() {
        assert_eq!(leb_len(0), 1);
        assert_eq!(leb_len(127), 1);
        assert_eq!(leb_len(128), 2);
        assert_eq!(leb_len(16_383), 2);
        assert_eq!(leb_len(16_384), 3);
        let mut b = Vec::new();
        put_vstr(&mut b, &"x".repeat(300));
        assert_eq!(&b[..2], &[0xAC, 0x02]);
        assert_eq!(read_uvar32(&b).map(|(v, _)| v), Ok(300));
    }
}
