//! Path value types and their syntax checks ([OS/path §2, §9, §11]): the one `RelPath` type for store and project
//! paths (P1, P4), `AbsPath` (P12), `CanonicalRoot` (P9), `EntryName` with its borrowed form `EntryNameRef`, and the
//! display of names.
//!
//! Everything here is pure: conversions that need the OS (`canonical_root`, `canonical_abs`, `cli_path`) are
//! `moirai-os::path` and are reached through [`crate::ProjectFs`]; the Unicode rules P3, P5 and P6 and
//! `representable(os, segment)` are `moirai-files`'s ([OS/path §1, §8.1]).
//!
//! **The Rust form of `RelPath`** ([OS/path §2.1, §11], spec sync 2a). `RelPath<'a>` is a `Copy` view over a validated
//! `&'a str`, two words wide, and every seam takes it **by value**: `rel: RelPath<'_>`, `dir: Option<RelPath<'_>>`,
//! `name: RelPath<'_>`, `At { path: RelPath<'a> }`, `GroupMember::Dir { dir: Option<RelPath<'a>> }`. An unsized
//! `RelPath(str)` passed as `&RelPath` could not be built from a `&str` without `unsafe`, which this crate forbids
//! ([OS/README §2.1]). A caller holding a [`RelPathBuf`] passes `buf.as_rel_path()` (or `(&buf).into()`); a map keyed by
//! `RelPathBuf` is queried with the borrowed text (`RelPathBuf: Borrow<str>`). The grammar, the byte-exact comparison
//! and the absence of any normalisation are those of [OS/path §2.1].

use core::borrow::Borrow;
use core::fmt;

use crate::proc::OsTag;
use crate::project::OsFileId;

/// Why a path value was refused ([OS/path §11]). Each variant has one meaning; the CLI reports it as its doc says
/// ([F19 §10.2]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum PathError {
    /// An empty segment: a leading or trailing `/`, or a `//` inside a `RelPath` or inside an `AbsPath` after its prefix;
    /// or an empty `seg` passed to [`RelPath::join`]. The empty `RelPath` itself is the root and valid. `bad_path`, P1.
    Empty,
    /// A malformed `AbsPath` prefix ([OS/path §2.2]): a drive letter that is not upper-case `A`–`Z` or is not followed by
    /// `:/` (so `C:` and `C:x` are not `AbsPath` values), or a UNC path without a server or a share. Exit 2.
    BadSegment,
    /// A segment that is exactly `.` or `..` (P1). `bad_path`, P1.
    DotSegment,
    /// A `/` inside a single segment (the argument of [`RelPath::join`]): a programming error of the caller.
    Separator,
    /// A C0 control character in a `RelPath` segment (P4), or U+0000 in an `AbsPath`. `bad_path`, P4.
    Control,
    /// A `\` in a `RelPath` segment (P4). `bad_path`, P4.
    Backslash,
    /// Input that is not valid Unicode: bytes that are not UTF-8 (Unix), or UTF-16 with an unpaired surrogate (Windows)
    /// (P4; [OS/path §7] step 1). `bad_path`, P4.
    NotUtf8,
    /// A value that must be an `AbsPath` matches none of [OS/path §2.2]'s three forms; or a CLI argument could not be
    /// made absolute at [OS/path §7] step 4 (the current directory has no canonical form, a leading `/` under a UNC
    /// current directory, or a `//` argument without a server or a share). Exit 2.
    NotAbsolute,
    /// A Windows CLI argument `X:rel`, or a bare `X:` ([OS/path §7] step 3). `bad_path`, rule `drive-relative`.
    DriveRelative,
    /// A Windows CLI argument in a device form `//./…` or `//?/…` ([OS/path §7] step 3). `bad_path`, rule `device`.
    DevicePath,
    /// A CLI argument whose normalised path is neither the tree root nor below it ([OS/path §7] step 5). Exit 2, or the
    /// verb's own refusal.
    OutsideRoot,
}

impl PathError {
    /// A stable lower-case description for diagnostics (not a frozen user text; [F19 §10.2] owns those).
    pub const fn as_str(self) -> &'static str {
        match self {
            PathError::Empty => "empty path segment",
            PathError::BadSegment => "malformed path",
            PathError::DotSegment => "'.' or '..' segment",
            PathError::Separator => "separator inside a segment",
            PathError::Control => "control character in a path",
            PathError::Backslash => "backslash in a path segment",
            PathError::NotUtf8 => "path is not UTF-8",
            PathError::NotAbsolute => "path is not absolute",
            PathError::DriveRelative => "drive-relative path",
            PathError::DevicePath => "device path",
            PathError::OutsideRoot => "path is outside the tree",
        }
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for PathError {}

/// Checks one `RelPath` segment `b[start..end]` (P1, P4): non-empty, not `.` or `..`, no `\`, no C0 control. The caller
/// guarantees the range holds no `/`.
const fn check_segment(b: &[u8], start: usize, end: usize) -> Result<(), PathError> {
    let len = end - start;
    if len == 0 {
        return Err(PathError::Empty);
    }
    if (len == 1 && b[start] == b'.') || (len == 2 && b[start] == b'.' && b[start + 1] == b'.') {
        return Err(PathError::DotSegment);
    }
    let mut i = start;
    while i < end {
        let c = b[i];
        if c == b'\\' {
            return Err(PathError::Backslash);
        }
        if c < 0x20 {
            return Err(PathError::Control);
        }
        i += 1;
    }
    Ok(())
}

/// Checks the grammar of [OS/path §2.1] over the bytes of a `&str` (so UTF-8 validity is already given). The empty value
/// is valid (the root). UTF-8 continuation and lead bytes are never `/`, `\` or a C0 byte, so a byte scan is exact.
const fn check_rel(b: &[u8]) -> Result<(), PathError> {
    if b.is_empty() {
        return Ok(());
    }
    let mut start = 0;
    let mut i = 0;
    while i <= b.len() {
        if i == b.len() || b[i] == b'/' {
            if let Err(e) = check_segment(b, start, i) {
                return Err(e);
            }
            start = i + 1;
        }
        i += 1;
    }
    Ok(())
}

/// A root-relative path ([OS/path §2.1], P1, P4): valid UTF-8, segments separated by `/`, no leading or trailing `/`, no
/// empty, `.` or `..` segment, no `\` and no C0 control character. The empty value denotes the root itself.
///
/// Compared and hashed as exact bytes; the type applies no normalisation of any kind (I-F8) and sets no length limit
/// (OS limits are use-time checks, [OS/fs §2.1], [OS/path §6]). It is the one path type for store and project paths
/// ([OS/README §2.1]); see the module documentation for why it is a `Copy` view passed by value rather than an unsized
/// newtype passed by reference.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct RelPath<'a>(&'a str);

impl<'a> RelPath<'a> {
    /// The root itself (the empty path).
    pub const ROOT: RelPath<'static> = RelPath("");

    /// Validates `s` against the grammar of [OS/path §2.1]. Usable in constant expressions.
    pub const fn new(s: &'a str) -> Result<RelPath<'a>, PathError> {
        match check_rel(s.as_bytes()) {
            Ok(()) => Ok(RelPath(s)),
            Err(e) => Err(e),
        }
    }

    /// The path's text, exactly as validated.
    pub const fn as_str(&self) -> &'a str {
        self.0
    }

    /// `true` for the root (the empty path).
    pub const fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The segments in order; none for the root.
    pub fn segments(&self) -> impl Iterator<Item = &'a str> + use<'a> {
        let s = self.0;
        s.split('/').filter(|seg| !seg.is_empty())
    }

    /// The path without its last segment; `None` for the root. The parent of a one-segment path is the root.
    pub fn parent(&self) -> Option<RelPath<'a>> {
        if self.0.is_empty() {
            return None;
        }
        Some(match self.0.rfind('/') {
            Some(i) => RelPath(&self.0[..i]),
            None => RelPath::ROOT,
        })
    }

    /// The last segment; `None` for the root.
    pub fn file_name(&self) -> Option<&'a str> {
        if self.0.is_empty() {
            return None;
        }
        Some(match self.0.rfind('/') {
            Some(i) => &self.0[i + 1..],
            None => self.0,
        })
    }

    /// This path followed by one more segment `seg`, which must itself be a valid segment (no `/`).
    pub fn join(&self, seg: &str) -> Result<RelPathBuf, PathError> {
        let b = seg.as_bytes();
        if b.contains(&b'/') {
            return Err(PathError::Separator);
        }
        check_segment(b, 0, b.len())?;
        if self.0.is_empty() {
            return Ok(RelPathBuf(Box::from(seg)));
        }
        let mut s = String::with_capacity(self.0.len() + 1 + seg.len());
        s.push_str(self.0);
        s.push('/');
        s.push_str(seg);
        Ok(RelPathBuf(s.into_boxed_str()))
    }

    /// An owned copy.
    pub fn to_buf(&self) -> RelPathBuf {
        RelPathBuf(Box::from(self.0))
    }
}

impl RelPath<'static> {
    /// A path from a literal, checked at compile time when used in a constant (for example the store names of [F02 §6]:
    /// `const HEAD: RelPath<'static> = RelPath::literal("HEAD");`). Panics if `s` violates the grammar.
    pub const fn literal(s: &'static str) -> RelPath<'static> {
        match RelPath::new(s) {
            Ok(p) => p,
            Err(_) => panic!("RelPath::literal: the literal violates the RelPath grammar"),
        }
    }
}

impl fmt::Display for RelPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl AsRef<str> for RelPath<'_> {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl PartialEq<RelPathBuf> for RelPath<'_> {
    fn eq(&self, other: &RelPathBuf) -> bool {
        self.0 == &*other.0
    }
}

/// An owned [`RelPath`] ([OS/path §11]).
///
/// Its `Eq`, `Ord` and `Hash` are those of its text (`Box<str>` delegates all three to `str`), so `Borrow<str>` is
/// sound: a `HashMap<RelPathBuf, _>` or `BTreeMap<RelPathBuf, _>` is queried with `path.as_str()` without allocating.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct RelPathBuf(Box<str>);

impl RelPathBuf {
    /// Validates and copies `s`.
    pub fn new(s: &str) -> Result<RelPathBuf, PathError> {
        check_rel(s.as_bytes())?;
        Ok(RelPathBuf(Box::from(s)))
    }

    /// Validates and takes `s` without copying its bytes.
    pub fn from_string(s: String) -> Result<RelPathBuf, PathError> {
        check_rel(s.as_bytes())?;
        Ok(RelPathBuf(s.into_boxed_str()))
    }

    /// The borrowed view.
    pub fn as_rel_path(&self) -> RelPath<'_> {
        RelPath(&self.0)
    }

    /// The path's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<RelPath<'_>> for RelPathBuf {
    fn from(p: RelPath<'_>) -> RelPathBuf {
        p.to_buf()
    }
}

impl<'a> From<&'a RelPathBuf> for RelPath<'a> {
    fn from(p: &'a RelPathBuf) -> RelPath<'a> {
        p.as_rel_path()
    }
}

impl Borrow<str> for RelPathBuf {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for RelPathBuf {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<RelPath<'_>> for RelPathBuf {
    fn eq(&self, other: &RelPath<'_>) -> bool {
        &*self.0 == other.0
    }
}

impl fmt::Display for RelPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Checks one `abs-seg` of [OS/path §2.2]: non-empty, not `.` or `..`, no U+0000 (the caller splits on `/`).
fn check_abs_segment(seg: &str) -> Result<(), PathError> {
    if seg.is_empty() {
        return Err(PathError::Empty);
    }
    if seg == "." || seg == ".." {
        return Err(PathError::DotSegment);
    }
    if seg.as_bytes().contains(&0) {
        return Err(PathError::Control);
    }
    Ok(())
}

/// Checks `abs-rel` (`abs-seg *( "/" abs-seg )`); the empty string is not an `abs-rel`.
fn check_abs_rel(s: &str) -> Result<(), PathError> {
    s.split('/').try_for_each(check_abs_segment)
}

/// Checks the grammar of [OS/path §2.2].
fn check_abs(s: &str) -> Result<(), PathError> {
    let b = s.as_bytes();
    if let Some(rest) = s.strip_prefix("//") {
        // win-unc-path = "//" abs-seg "/" abs-seg [ "/" abs-rel ]
        let mut parts = rest.splitn(3, '/');
        let server = parts.next().unwrap_or("");
        let share = parts.next().ok_or(PathError::BadSegment)?;
        if server.is_empty() || share.is_empty() {
            return Err(PathError::BadSegment);
        }
        check_abs_segment(server)?;
        check_abs_segment(share)?;
        return match parts.next() {
            Some(tail) => check_abs_rel(tail),
            None => Ok(()),
        };
    }
    if let Some(rest) = s.strip_prefix('/') {
        // unix-path = "/" [ abs-rel ]
        return if rest.is_empty() {
            Ok(())
        } else {
            check_abs_rel(rest)
        };
    }
    if b.len() >= 2 && b[1] == b':' {
        // win-drive-path = drive ":/" [ abs-rel ], drive = A–Z
        if !b[0].is_ascii_uppercase() || b.len() < 3 || b[2] != b'/' {
            return Err(PathError::BadSegment);
        }
        let rest = &s[3..];
        return if rest.is_empty() {
            Ok(())
        } else {
            check_abs_rel(rest)
        };
    }
    Err(PathError::NotAbsolute)
}

/// A machine-local absolute path ([OS/path §2.2], P12): `X:/…` with an upper-case drive letter, `//server/share/…`, or
/// `/…`; `/` separators; no empty, `.` or `..` segment; no `\\?\` prefix. Existence and `oid` checks only: never
/// re-bound, never a candidate, never compared across machines or OSes. The type accepts all three forms on every
/// target; `moirai-os` produces the Windows forms only on Windows and the Unix form only on Linux and macOS.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct AbsPath(Box<str>);

impl AbsPath {
    /// Validates and copies `s` ([OS/path §2.2] grammar).
    pub fn new(s: &str) -> Result<AbsPath, PathError> {
        check_abs(s)?;
        Ok(AbsPath(Box::from(s)))
    }

    /// Validates and takes `s` without copying its bytes.
    pub fn from_string(s: String) -> Result<AbsPath, PathError> {
        check_abs(&s)?;
        Ok(AbsPath(s.into_boxed_str()))
    }

    /// The path's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AbsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A canonical top-level ([OS/path §2.3], P9): the canonical text, the directory's `OsFileId` (kind `none` where the
/// volume has no trusted ids) and the OS tag of the process that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalRoot {
    /// The canonical text of [OS/path §4].
    pub text: AbsPath,
    /// The root directory's identity ([OS/project §3.1]).
    pub root_id: OsFileId,
    /// The OS tag of the producing process ([OS/proc §2]).
    pub os: OsTag,
}

/// A name as an OS returned it ([OS/path §2.4]).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum EntryName {
    /// The name is valid Unicode and passes P4: usable as a `RelPath` segment.
    Utf8(Box<str>),
    /// The name is not UTF-8 (Linux), has an unpaired surrogate (Windows), or contains `\` or a C0 control (P4). Holds
    /// the OS bytes: WTF-8 of the UTF-16 name on Windows, the raw bytes on Unix. Never a candidate, never stored.
    Unrepresentable(Box<[u8]>),
}

impl EntryName {
    /// Classifies the bytes of one directory entry's name as an OS returned them (WTF-8 on Windows, raw bytes on Unix):
    /// `Utf8` iff they are valid UTF-8 and form a valid `RelPath` segment, else `Unrepresentable`.
    pub fn from_os_bytes(bytes: &[u8]) -> EntryName {
        EntryNameRef::from_os_bytes(bytes).to_owned()
    }

    /// The borrowed form ([OS/path §2.4]): the same bytes and the same classification.
    pub fn as_entry_ref(&self) -> EntryNameRef<'_> {
        match self {
            EntryName::Utf8(s) => EntryNameRef::Utf8(s),
            EntryName::Unrepresentable(b) => EntryNameRef::Unrepresentable(b),
        }
    }

    /// The name as a `RelPath` segment; `None` if it is unrepresentable.
    pub fn as_segment(&self) -> Option<&str> {
        match self {
            EntryName::Utf8(s) => Some(s),
            EntryName::Unrepresentable(_) => None,
        }
    }

    /// The name's bytes (the UTF-8 text, or the OS bytes).
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            EntryName::Utf8(s) => s.as_bytes(),
            EntryName::Unrepresentable(b) => b,
        }
    }

    /// The display form of [OS/path §9].
    pub fn display(&self) -> String {
        display_name(self.as_bytes())
    }
}

/// A name as an OS returned it, borrowed ([OS/path §2.4]): the form an enumeration hands to its visitor, so that a tree
/// scan allocates nothing per entry ([OS/project §5.2]). It lives only as long as the buffer it borrows from; a caller
/// that keeps a name takes [`EntryNameRef::to_owned`]. The two forms carry the same bytes and the same classification.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum EntryNameRef<'a> {
    /// The name is valid Unicode and passes P4: usable as a `RelPath` segment.
    Utf8(&'a str),
    /// The name is not UTF-8 (Linux), has an unpaired surrogate (Windows), or contains `\` or a C0 control (P4): the OS
    /// bytes (WTF-8 of the UTF-16 name on Windows, the raw bytes on Unix). Never a candidate, never stored.
    Unrepresentable(&'a [u8]),
}

impl<'a> EntryNameRef<'a> {
    /// Classifies the bytes of one directory entry's name as an OS returned them, as [`EntryName::from_os_bytes`] does,
    /// without allocating.
    pub fn from_os_bytes(bytes: &'a [u8]) -> EntryNameRef<'a> {
        match core::str::from_utf8(bytes) {
            Ok(s) if !bytes.contains(&b'/') && check_segment(bytes, 0, bytes.len()).is_ok() => {
                EntryNameRef::Utf8(s)
            }
            _ => EntryNameRef::Unrepresentable(bytes),
        }
    }

    /// The owned form ([OS/path §2.4]), with the signature the specification fixes: `to_owned(&self)`.
    ///
    /// The receiver is `&self` although the type is `Copy` (so clippy's `wrong_self_convention` is allowed here): an
    /// inherent `&self` method is found before the blanket `ToOwned::to_owned` both for `name.to_owned()` and for
    /// `(&name).to_owned()`, while a by-value receiver would let the second form resolve to `ToOwned` and return an
    /// `EntryNameRef` instead of an `EntryName`.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_owned(&self) -> EntryName {
        match *self {
            EntryNameRef::Utf8(s) => EntryName::Utf8(Box::from(s)),
            EntryNameRef::Unrepresentable(b) => EntryName::Unrepresentable(Box::from(b)),
        }
    }

    /// The name as a `RelPath` segment; `None` if it is unrepresentable.
    pub const fn as_segment(&self) -> Option<&'a str> {
        match *self {
            EntryNameRef::Utf8(s) => Some(s),
            EntryNameRef::Unrepresentable(_) => None,
        }
    }

    /// The name's bytes (the UTF-8 text, or the OS bytes).
    pub const fn as_bytes(&self) -> &'a [u8] {
        match *self {
            EntryNameRef::Utf8(s) => s.as_bytes(),
            EntryNameRef::Unrepresentable(b) => b,
        }
    }

    /// The display form of [OS/path §9].
    pub fn display(&self) -> String {
        display_name(self.as_bytes())
    }
}

impl PartialEq<EntryName> for EntryNameRef<'_> {
    fn eq(&self, other: &EntryName) -> bool {
        *self == other.as_entry_ref()
    }
}

impl PartialEq<EntryNameRef<'_>> for EntryName {
    fn eq(&self, other: &EntryNameRef<'_>) -> bool {
        self.as_entry_ref() == *other
    }
}

/// Displays a name ([OS/path §9]): its UTF-8 bytes as they are, and each byte of an ill-formed sequence (including the
/// WTF-8 bytes of a Windows unpaired surrogate) as `\x` followed by two lower-case hex digits. Escaping of control
/// characters and quotes inside rendered text is [F19 §2.5]'s untrusted-text rule, not this function's.
pub fn display_name(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len());
    // One pass: each chunk is a valid run followed by the ill-formed bytes before the next valid run.
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for &byte in chunk.invalid() {
            out.push('\\');
            out.push('x');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn rel_path_grammar() {
        for ok in [
            "",
            "HEAD",
            "a/b/c",
            "tmp/probe.1",
            "refs/heads/lane/x",
            "a b/ c",
            "ü/日本",
            "a..b/.c/..d",
        ] {
            assert!(RelPath::new(ok).is_ok(), "{ok:?} must be valid");
        }
        let cases: [(&str, PathError); 11] = [
            ("/a", PathError::Empty),
            ("a/", PathError::Empty),
            ("a//b", PathError::Empty),
            ("/", PathError::Empty),
            (".", PathError::DotSegment),
            ("a/..", PathError::DotSegment),
            ("./a", PathError::DotSegment),
            ("a\\b", PathError::Backslash),
            ("a\u{0}b", PathError::Control),
            ("a\u{1f}", PathError::Control),
            ("a/\tb", PathError::Control),
        ];
        for (bad, err) in cases {
            assert_eq!(RelPath::new(bad), Err(err), "{bad:?}");
        }
        // DEL and C1 controls are not C0: allowed by the grammar.
        assert!(RelPath::new("a\u{7f}\u{85}").is_ok());
    }

    #[test]
    fn rel_path_navigation() {
        const HEAD: RelPath<'static> = RelPath::literal("HEAD");
        assert_eq!(HEAD.as_str(), "HEAD");
        let p = RelPath::new("a/b/c").unwrap();
        assert_eq!(p.segments().collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(p.file_name(), Some("c"));
        assert_eq!(p.parent(), Some(RelPath::new("a/b").unwrap()));
        assert_eq!(RelPath::new("a").unwrap().parent(), Some(RelPath::ROOT));
        assert_eq!(RelPath::ROOT.parent(), None);
        assert_eq!(RelPath::ROOT.file_name(), None);
        assert_eq!(RelPath::ROOT.segments().count(), 0);
        assert_eq!(RelPath::ROOT.join("x").unwrap().as_str(), "x");
        assert_eq!(p.join("d").unwrap().as_str(), "a/b/c/d");
        assert_eq!(p.join("d/e"), Err(PathError::Separator));
        assert_eq!(p.join(""), Err(PathError::Empty));
        assert_eq!(p.join(".."), Err(PathError::DotSegment));
        assert_eq!(
            RelPathBuf::new("x/y").unwrap().as_rel_path(),
            RelPath::new("x/y").unwrap()
        );
        assert_eq!(
            RelPathBuf::from_string("x/".to_owned()),
            Err(PathError::Empty)
        );
    }

    #[test]
    fn abs_path_grammar() {
        for ok in [
            "C:/",
            "C:/Users/x",
            "//server/share",
            "//srv/share/a/b",
            "/",
            "/home/u/a\\b",
            "Z:/a b/ü",
        ] {
            assert!(AbsPath::new(ok).is_ok(), "{ok:?} must be valid");
        }
        let cases: [(&str, PathError); 12] = [
            ("", PathError::NotAbsolute),
            ("a/b", PathError::NotAbsolute),
            ("c:/x", PathError::BadSegment),
            ("C:", PathError::BadSegment),
            ("C:x", PathError::BadSegment),
            ("C:/x/", PathError::Empty),
            ("//server", PathError::BadSegment),
            ("//server/", PathError::BadSegment),
            ("/a//b", PathError::Empty),
            ("/a/./b", PathError::DotSegment),
            ("/a/..", PathError::DotSegment),
            ("/a\u{0}", PathError::Control),
        ];
        for (bad, err) in cases {
            assert_eq!(AbsPath::new(bad), Err(err), "{bad:?}");
        }
    }

    #[test]
    fn entry_names_and_display() {
        assert_eq!(
            EntryName::from_os_bytes(b"file.txt"),
            EntryName::Utf8(Box::from("file.txt"))
        );
        for bad in [&b"a\\b"[..], b"\x01x", b".", b"..", b"\xff", b"a/b", b""] {
            assert!(
                matches!(EntryName::from_os_bytes(bad), EntryName::Unrepresentable(_)),
                "{bad:?}"
            );
        }
        assert_eq!(display_name(b"ok"), "ok");
        assert_eq!(display_name(b"a\xffb"), "a\\xffb");
        // An unpaired surrogate in WTF-8 (U+D800 = ED A0 80): three ill-formed bytes.
        assert_eq!(display_name(b"x\xed\xa0\x80"), "x\\xed\\xa0\\x80");
        // A truncated sequence at the end.
        assert_eq!(display_name("é".as_bytes().split_at(1).0), "\\xc3");
        assert_eq!(display_name("日本".as_bytes()), "日本");
        // A valid run after ill-formed bytes is kept; each ill-formed byte appears once.
        assert_eq!(
            display_name(b"\xc0\xafok\xe6\x97"),
            "\\xc0\\xafok\\xe6\\x97"
        );
    }

    /// [OS/path §2.4]: the borrowed form carries the same bytes and the same classification as the owned one.
    #[test]
    fn borrowed_entry_names_match_the_owned_ones() {
        for bytes in [
            &b"file.txt"[..],
            b"a\\b",
            b"\x01x",
            b".",
            b"..",
            b"\xff",
            b"a/b",
            b"",
            "日本".as_bytes(),
            b"x\xed\xa0\x80",
        ] {
            let r = EntryNameRef::from_os_bytes(bytes);
            let owned = EntryName::from_os_bytes(bytes);
            assert_eq!(r.to_owned(), owned, "{bytes:?}");
            assert_eq!(owned.as_entry_ref(), r);
            assert!(r == owned && owned == r);
            assert_eq!(r.as_bytes(), bytes);
            assert_eq!(r.as_segment(), owned.as_segment());
            assert_eq!(r.display(), owned.display());
        }
        assert_eq!(EntryNameRef::from_os_bytes(b"ok"), EntryNameRef::Utf8("ok"));
        assert_eq!(
            EntryNameRef::from_os_bytes(b"a\\b"),
            EntryNameRef::Unrepresentable(b"a\\b")
        );
    }

    /// [OS/path §2.4] `to_owned(&self)`: every call form reaches the inherent method and gives an `EntryName`, never the
    /// blanket `ToOwned` of the `Copy` type (which would give back an `EntryNameRef`). The annotations make this a
    /// compile-time check; `PartialEq<EntryName> for EntryNameRef` would hide it from a plain `assert_eq!`.
    #[test]
    #[allow(clippy::needless_borrow)]
    fn to_owned_gives_the_owned_form_by_every_call_form() {
        let r = EntryNameRef::from_os_bytes(b"x\xff");
        let by_place: EntryName = r.to_owned();
        let by_ref: EntryName = (&r).to_owned();
        let rr = &r;
        let by_binding: EntryName = rr.to_owned();
        let by_path: EntryName = EntryNameRef::to_owned(&r);
        for o in [by_place, by_ref, by_binding, by_path] {
            assert_eq!(o, EntryName::Unrepresentable(Box::from(&b"x\xff"[..])));
        }
    }

    /// [OS/path §11]: every variant has its own description; the drive-relative and device refusals of §7 step 3 are
    /// variants of their own.
    #[test]
    fn path_errors_are_distinct() {
        let all = [
            PathError::Empty,
            PathError::BadSegment,
            PathError::DotSegment,
            PathError::Separator,
            PathError::Control,
            PathError::Backslash,
            PathError::NotUtf8,
            PathError::NotAbsolute,
            PathError::DriveRelative,
            PathError::DevicePath,
            PathError::OutsideRoot,
        ];
        let texts: std::collections::BTreeSet<&str> = all.iter().map(|e| e.as_str()).collect();
        assert_eq!(texts.len(), all.len());
        assert_eq!(PathError::DriveRelative.to_string(), "drive-relative path");
        assert_eq!(PathError::DevicePath.to_string(), "device path");
    }

    /// Byte strings that mix valid multi-byte UTF-8 with ill-formed bytes: lone continuation bytes, truncated sequences,
    /// surrogates in WTF-8 (`ED A0..BF xx`), overlong forms, and bytes that can never occur (`C0`, `C1`, `F5`–`FF`).
    fn ill_formed_mix() -> impl Strategy<Value = Vec<u8>> {
        let piece = prop_oneof![
            "[a-z\u{e9}\u{65e5}\u{1f600}]{1,3}".prop_map(String::into_bytes),
            any::<u8>().prop_map(|b| vec![b]),
            Just(vec![0xED, 0xA0, 0x80]),
            Just(vec![0xE6, 0x97]),
            Just(vec![0xF0, 0x9F, 0x98]),
            Just(vec![0xC0, 0xAF]),
            Just(vec![0x80]),
            Just(vec![0xFF]),
        ];
        proptest::collection::vec(piece, 0..8).prop_map(|v| v.concat())
    }

    #[test]
    fn rel_path_buf_is_queried_by_its_text() {
        use std::collections::{BTreeMap, HashMap};
        let buf = RelPathBuf::new("refs/heads/x").unwrap();
        let view = RelPath::new("refs/heads/x").unwrap();
        assert!(buf == view && view == buf);
        assert_eq!(RelPath::from(&buf), view);
        let mut h = HashMap::new();
        h.insert(buf.clone(), 1);
        let mut b = BTreeMap::new();
        b.insert(buf.clone(), 2);
        assert_eq!(h.get(view.as_str()), Some(&1));
        assert_eq!(b.get("refs/heads/x"), Some(&2));
        assert_eq!(h.get("refs/heads/y"), None);
        let s: &str = buf.as_ref();
        assert_eq!(s, view.as_ref());
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        /// A string is a valid `RelPath` iff it is the join of valid segments, and the view round-trips.
        #[test]
        fn rel_path_is_joined_segments(segs in proptest::collection::vec("[a-z0-9 ._\\\\\u{1}-]{1,6}", 0..5)) {
            let text = segs.join("/");
            let every_seg_ok = segs.iter().all(|s| check_segment(s.as_bytes(), 0, s.len()).is_ok());
            prop_assert_eq!(RelPath::new(&text).is_ok(), every_seg_ok);
            if let Ok(p) = RelPath::new(&text) {
                prop_assert_eq!(p.segments().collect::<Vec<_>>(), segs.iter().map(String::as_str).collect::<Vec<_>>());
                let mut rebuilt = RelPathBuf::from(RelPath::ROOT);
                for s in p.segments() {
                    rebuilt = rebuilt.as_rel_path().join(s).unwrap();
                }
                prop_assert_eq!(rebuilt.as_str(), text.as_str());
            }
        }

        /// The borrowed and the owned name agree on every byte string ([OS/path §2.4]).
        #[test]
        fn entry_name_forms_agree(bytes in ill_formed_mix()) {
            let r = EntryNameRef::from_os_bytes(&bytes);
            let owned = EntryName::from_os_bytes(&bytes);
            prop_assert_eq!(r.as_bytes(), &bytes[..]);
            prop_assert_eq!(owned.as_entry_ref(), r);
            prop_assert_eq!(r.to_owned(), owned);
        }

        /// Valid UTF-8 displays unchanged; otherwise every valid run is kept as it is and every ill-formed byte appears
        /// exactly once, in order, as `\x` and two lower-case hex digits. The reference below is built independently of
        /// `utf8_chunks`, from `from_utf8`'s error positions.
        #[test]
        fn display_name_escapes_only_ill_formed(bytes in ill_formed_mix()) {
            let shown = display_name(&bytes);
            let mut reference = String::new();
            let (mut valid_len, mut invalid_len) = (0usize, 0usize);
            let mut rest = &bytes[..];
            while !rest.is_empty() {
                let (valid, bad) = match core::str::from_utf8(rest) {
                    Ok(s) => (s, 0),
                    Err(e) => {
                        let valid = core::str::from_utf8(&rest[..e.valid_up_to()]).expect("the accepted prefix");
                        (valid, e.error_len().unwrap_or(rest.len() - e.valid_up_to()))
                    }
                };
                reference.push_str(valid);
                valid_len += valid.len();
                for b in &rest[valid.len()..valid.len() + bad] {
                    reference.push_str(&format!("\\x{b:02x}"));
                }
                invalid_len += bad;
                rest = &rest[valid.len() + bad..];
            }
            prop_assert_eq!(&shown, &reference);
            prop_assert_eq!(shown.len(), valid_len + 4 * invalid_len);
            if let Ok(s) = core::str::from_utf8(&bytes) {
                prop_assert_eq!(shown.as_str(), s);
            }
        }
    }
}
