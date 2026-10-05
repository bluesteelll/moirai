//! Path rules of FL-1: P5 portability ([OS/path §8.2]), representability on each supported OS ([OS/path §8.1]) and
//! the equivalences built on P6's `fold_v1` and on canonical equivalence ([F20 §3.3]–[F20 §3.6]): fold siblings, fold
//! equivalents, twin-candidate groups, twin sets and the collision kind. Every function is pure: stat results and file
//! identities are data the caller passes in.
//!
//! Where the rules P1–P12 of [OS/path §3] ([80 §2.10], frozen by X-F7 and X-F9) live, per [OS/path §1]:
//!
//! | Rule | Home |
//! |---|---|
//! | P1, P4 (stored-path grammar, refused names) | `moirai-vfs` (`RelPath`, `EntryName`) |
//! | P2, P7 (git's spelling; `origin_path`) | the link layer (M6) over the git reader (M4) |
//! | P3 (NFC of untracked names on normalization-insensitive volumes) | here, in the port phase: [OS/path §3] marks it "`moirai-files` (port)", and its function is git's precomposition ([OS/path] open point 2), tested against git on APFS |
//! | P5 (portable names) | here: [`portable_issues`]; and [`representable`] for every OS ([OS/path §8.1]), which `moirai-os::path::representable_here` restates for the build OS |
//! | P6 (`fold_v1`) | [`crate::fold`]; its uses ([F20 §3.3]) are here |
//! | P8 (symlinks) | `ProjectFs` reads the target text; its `oid` is [`crate::oid::blob_oid`] ([F20 §2.3]) |
//! | P9 (canonical root), P10 (relative walks), P12 (`abs`) | `ProjectFs`, `moirai-os::path` |
//! | P11 (a) query file names, (b) ref names, (c) store names | [F14 §7.2], [F12 §2], [F02 §6]; (b)'s device list is [`is_device_name`]. (b)'s `fold_v1` rule needs no fold call: a ref name is lower-case ASCII ([F12 §2]), so two ref names are equal under `fold_v1` exactly when their bytes are, and the refusal is a byte lookup of the live names |
//!
//! Orders are [F01 §6.6]'s path order (bytewise, a proper prefix first), which is `str`'s `Ord`; every list returned
//! here is sorted in it and holds each spelling once ([80 §2.11.4] rule 4: sort before any selection).

use crate::fold::{ceq, fold_matches, fold_v1, fold_v1_into, nfd_chars};

/// The longest portable segment, in UTF-8 bytes ([OS/path §8.2] `too-long`; [80 §2.10] P5).
pub const MAX_SEGMENT_BYTES: usize = 255;

/// The characters Windows refuses in a name ([OS/path §8.2] `reserved-char`).
pub const RESERVED_CHARS: [char; 7] = ['<', '>', ':', '"', '|', '?', '*'];

/// The longest name each OS can hold, in its own unit ([OS/path §6] component limits, §8.1): UTF-16 code units on
/// Windows (NTFS), bytes on Linux (`NAME_MAX`), UTF-8 bytes on macOS (APFS).
pub const MAX_NAME_UNITS: usize = 255;

/// An OS of [OS/path §8.1]'s table, named by its OS tag byte ([OS/proc §2], registry [F01 §3.2]).
///
/// [OS/path §11] types [`representable`]'s first argument as `moirai-vfs`'s `OsTag`. This crate may not depend on
/// `moirai-vfs` (PLAN §2.2), so it names the three OSes itself with the same discriminants; a caller holding an
/// `OsTag` converts with [`Os::from_tag`]. Tag 0 (`Unspecified`) and the reserved tags name no OS and have no row.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Os {
    /// Windows (tag 1).
    Windows = 1,
    /// Linux (tag 2).
    Linux = 2,
    /// macOS (tag 3).
    MacOs = 3,
}

impl Os {
    /// Every OS, in tag order.
    pub const ALL: [Os; 3] = [Os::Windows, Os::Linux, Os::MacOs];

    /// The OS of tag byte `tag`; `None` for 0 (`Unspecified`) and the reserved values 4–255.
    pub const fn from_tag(tag: u8) -> Option<Os> {
        match tag {
            1 => Some(Os::Windows),
            2 => Some(Os::Linux),
            3 => Some(Os::MacOs),
            _ => None,
        }
    }

    /// The OS tag byte.
    pub const fn tag(self) -> u8 {
        self as u8
    }
}

/// `representable(os, segment)` ([OS/path §8.1]): whether the OS `os` can hold a name. It decides `missing (not
/// representable on this OS)` for a tracked path ([F18 §4.6] detail 44), and the resolver applies it, with the OS tag
/// of the process, to every segment of every path before any OS call ([F20 §4.9]); `--allow-nonportable` never
/// relaxes it.
///
/// | OS | A segment is representable iff |
/// |---|---|
/// | Windows | it is not a device name ([`is_device_name`], [OS/path §8.2]); does not end in `.` or ` `; contains none of [`RESERVED_CHARS`]; and is at most [`MAX_NAME_UNITS`] UTF-16 code units |
/// | Linux | it is at most [`MAX_NAME_UNITS`] bytes |
/// | macOS | it is at most [`MAX_NAME_UNITS`] UTF-8 bytes |
///
/// `/`, `\`, U+0000 and the C0 controls are excluded by P1 and P4 before this check, so the function does not test
/// them. `moirai-os::path::representable_here(segment)` is `representable(<the build OS>, segment)` restated in
/// `moirai-os`, which may not depend on this crate; the two are one rule ([OS/path §8.1]).
///
/// No allocation: a segment of at most 255 UTF-8 bytes has at most 255 UTF-16 code units, so only a longer one is
/// counted.
pub fn representable(os: Os, segment: &str) -> bool {
    match os {
        Os::Windows => {
            device_name_stem(segment).is_none()
                && !segment.ends_with(['.', ' '])
                && !segment.contains(RESERVED_CHARS)
                && (segment.len() <= MAX_NAME_UNITS
                    || segment.encode_utf16().count() <= MAX_NAME_UNITS)
        }
        Os::Linux | Os::MacOs => segment.len() <= MAX_NAME_UNITS,
    }
}

/// The stem of `segment` when it is a Windows device name ([OS/path §8.2] `device-name`, used by P11 (b) too): the
/// part before the first `.`, trailing ASCII spaces removed, equal ignoring ASCII case to one of `CON`, `PRN`, `AUX`,
/// `NUL`, `CONIN$`, `CONOUT$`, `COM0`–`COM9`, `LPT0`–`LPT9`, or `COM`/`LPT` followed by `¹`, `²` or `³`.
pub fn device_name_stem(segment: &str) -> Option<&str> {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ');
    let b = stem.as_bytes();
    let com_lpt = |p: &[u8]| p.eq_ignore_ascii_case(b"COM") || p.eq_ignore_ascii_case(b"LPT");
    let device = match b.len() {
        3 => [&b"CON"[..], b"PRN", b"AUX", b"NUL"]
            .iter()
            .any(|d| b.eq_ignore_ascii_case(d)),
        4 => com_lpt(&b[..3]) && b[3].is_ascii_digit(),
        // U+00B9, U+00B2, U+00B3 are C2 B9, C2 B2, C2 B3 in UTF-8.
        5 => com_lpt(&b[..3]) && b[3] == 0xC2 && matches!(b[4], 0xB9 | 0xB2 | 0xB3),
        6 => b.eq_ignore_ascii_case(b"CONIN$"),
        7 => b.eq_ignore_ascii_case(b"CONOUT$"),
        _ => false,
    };
    device.then_some(stem)
}

/// Whether `segment` is a Windows device name ([`device_name_stem`]).
pub fn is_device_name(segment: &str) -> bool {
    device_name_stem(segment).is_some()
}

/// One issue of [OS/path §8.2], in the table's order.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum PortableIssue {
    /// The segment's stem is a Windows device name.
    DeviceName,
    /// The segment ends in `.` or a space.
    TrailingDotOrSpace,
    /// The segment contains one of [`RESERVED_CHARS`].
    ReservedChar,
    /// The segment is longer than [`MAX_SEGMENT_BYTES`] UTF-8 bytes.
    TooLong,
    /// A sibling differs from the segment but is equal to it under `fold_v1`.
    FoldSibling,
}

impl PortableIssue {
    /// Every issue, in [OS/path §8.2]'s order.
    pub const ALL: [PortableIssue; 5] = [
        PortableIssue::DeviceName,
        PortableIssue::TrailingDotOrSpace,
        PortableIssue::ReservedChar,
        PortableIssue::TooLong,
        PortableIssue::FoldSibling,
    ];

    /// The issue name of [OS/path §8.2] and [F19 §10.2] (`nonportable_name`; the JSON `rule` key).
    pub const fn name(self) -> &'static str {
        match self {
            PortableIssue::DeviceName => "device-name",
            PortableIssue::TrailingDotOrSpace => "trailing-dot-or-space",
            PortableIssue::ReservedChar => "reserved-char",
            PortableIssue::TooLong => "too-long",
            PortableIssue::FoldSibling => "fold-sibling",
        }
    }
}

/// The portability issues of one segment ([OS/path §8.2]), with the details the texts of [F19 §10.2] name.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PortableIssues<'s, 'n> {
    /// `device-name`: the stem that is a device name (`<stem> is a Windows device name`).
    pub device_stem: Option<&'s str>,
    /// `trailing-dot-or-space`.
    pub trailing_dot_or_space: bool,
    /// `reserved-char`: the first reserved character of the segment (`it contains <char>`).
    pub reserved_char: Option<char>,
    /// `too-long`.
    pub too_long: bool,
    /// `fold-sibling`: the least sibling in path order that differs from the segment but is equal to it under
    /// `fold_v1` (`it differs from <sibling> only in case or normalization`).
    pub fold_sibling: Option<&'n str>,
}

impl PortableIssues<'_, '_> {
    /// Whether the segment is portable.
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// Whether the segment has `issue`.
    pub fn contains(&self, issue: PortableIssue) -> bool {
        match issue {
            PortableIssue::DeviceName => self.device_stem.is_some(),
            PortableIssue::TrailingDotOrSpace => self.trailing_dot_or_space,
            PortableIssue::ReservedChar => self.reserved_char.is_some(),
            PortableIssue::TooLong => self.too_long,
            PortableIssue::FoldSibling => self.fold_sibling.is_some(),
        }
    }

    /// The issues present, in [OS/path §8.2]'s order: one warning each under `files.portable-names = warn`.
    pub fn iter(&self) -> impl Iterator<Item = PortableIssue> + '_ {
        PortableIssue::ALL.into_iter().filter(|&i| self.contains(i))
    }
}

/// `portable_issues(segment, siblings)` ([OS/path §8.2]; [80 §2.10] P5): what makes `segment` a name some supported
/// OS cannot hold. `file mv` refuses a name with any issue under `files.portable-names = refuse` unless
/// `--allow-nonportable` is given; `link`, `file add` and `file mv` under `warn` print one warning per issue.
///
/// `siblings` are the names the segment's directory will hold **after** the operation, other than the segment itself
/// (which may be among them; it is never its own sibling). A move's source name is therefore left out when source
/// and destination share the directory: the case-only rename `file mv a.md A.md` has no `fold-sibling` issue,
/// because `a.md` no longer exists once `A.md` does.
///
/// `fold_v1(segment)` is computed at most once, at the first sibling that differs from the segment bytewise; each
/// sibling is compared with it as a stream, and an ASCII sibling without touching a table.
pub fn portable_issues<'s, 'n, I>(segment: &'s str, siblings: I) -> PortableIssues<'s, 'n>
where
    I: IntoIterator<Item = &'n str>,
{
    let mut folded: Option<String> = None;
    let fold_sibling = siblings
        .into_iter()
        .filter(|&s| {
            s != segment && fold_matches(folded.get_or_insert_with(|| fold_v1(segment)), s)
        })
        .min();
    PortableIssues {
        device_stem: device_name_stem(segment),
        trailing_dot_or_space: segment.ends_with(['.', ' ']),
        reserved_char: segment.chars().find(|c| RESERVED_CHARS.contains(c)),
        too_long: segment.len() > MAX_SEGMENT_BYTES,
        fold_sibling,
    }
}

/// The spellings equal to `path` under `fold_v1` but not bytewise ([F20 §3.3]), in path order, each once.
///
/// - With the paths of a root's spellings, `{path}` plus these is `path`'s twin-candidate group ([F20 §3.5]).
/// - With the paths of τ(H), exactly one result is the `git/case` candidate of [F20 §3.6].
/// - With a directory's names after an operation, the first is P5's fold sibling ([`portable_issues`]).
///
/// Ref names ([80 §2.10] P11 (b)) need no call: they are lower-case ASCII ([F12 §2]), so for them `fold_v1`
/// equality is byte equality and this result is always empty.
///
/// `fold_v1(path)` is computed at most once, at the first spelling that differs from `path` bytewise.
pub fn fold_equivalents<'n, I>(path: &str, spellings: I) -> Vec<&'n str>
where
    I: IntoIterator<Item = &'n str>,
{
    let mut folded: Option<String> = None;
    let mut v: Vec<&str> = spellings
        .into_iter()
        .filter(|&s| s != path && fold_matches(folded.get_or_insert_with(|| fold_v1(path)), s))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The entries canonically equivalent to `name` ([F20 §3.4], `ceq`) but not equal to it bytewise, in path order,
/// each once: the set E of [F20 §3.6]'s normalization rule when `entries` are the unbound entries of the parent
/// directory (|E| = 1: `ok (normalization differs on disk)`; |E| ≥ 2: `ambiguous (normalization collision)`).
pub fn ceq_equivalents<'n, I>(name: &str, entries: I) -> Vec<&'n str>
where
    I: IntoIterator<Item = &'n str>,
{
    let ascii = name.is_ascii();
    let mut v: Vec<&str> = entries
        .into_iter()
        // Two distinct ASCII strings are never canonically equivalent.
        .filter(|&e| e != name && !(ascii && e.is_ascii()) && nfd_chars(e).eq(nfd_chars(name)))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The twin-candidate groups of a set of spellings ([F20 §3.5]): the classes of equal `fold_v1` with at least 2
/// members. Each group is in path order; the groups are in the order of their fold, which is `PATHIDX`'s key order
/// `(fold, path)` ([F09 §13.1]). Duplicate spellings count once.
///
/// The folds are computed once into one buffer; the cost is one sort of the spellings by `(fold, path)`.
pub fn twin_candidate_groups<'n, I>(spellings: I) -> Vec<Vec<&'n str>>
where
    I: IntoIterator<Item = &'n str>,
{
    let mut paths: Vec<&str> = spellings.into_iter().collect();
    paths.sort_unstable();
    paths.dedup();
    let mut folds = String::with_capacity(paths.iter().map(|p| p.len()).sum());
    let mut keyed: Vec<(usize, usize, &str)> = Vec::with_capacity(paths.len());
    for p in paths {
        let start = folds.len();
        fold_v1_into(p, &mut folds);
        keyed.push((start, folds.len(), p));
    }
    keyed.sort_by(|a, b| {
        folds[a.0..a.1]
            .cmp(&folds[b.0..b.1])
            .then_with(|| a.2.cmp(b.2))
    });
    let mut groups = Vec::new();
    let mut i = 0;
    while i < keyed.len() {
        let key = &folds[keyed[i].0..keyed[i].1];
        let mut j = i + 1;
        while j < keyed.len() && &folds[keyed[j].0..keyed[j].1] == key {
            j += 1;
        }
        if j - i >= 2 {
            groups.push(keyed[i..j].iter().map(|k| k.2).collect());
        }
        i = j;
    }
    groups
}

/// One member of a twin-candidate group as a tree T observes it ([F20 §3.5]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TwinMember<'n, F> {
    /// The spelling. The members of one group are distinct spellings.
    pub path: &'n str,
    /// Whether the spelling is the path of a live file node with status `present` or `planned` on the view (else it
    /// is only a blob or link path of τ(H)).
    pub node: bool,
    /// The identity of the file the spelling denotes when it stats successfully in T through its own spelling (its
    /// `OsFileId`); `None` when the stat fails. [`twin_sets`] compares identities with the caller's identity rule,
    /// never with `==`.
    pub file: Option<F>,
}

/// The twin sets of one twin-candidate group ([F20 §3.5]; [80 §2.11.4] rule 2): among the members that stat, the
/// classes of members denoting the same file, each with at least 2 members of which at least one is a node's path.
/// This is the directory's actual equivalence as the volume observes it: a case-insensitive directory gives one set
/// for `docs/Plan.md` and `docs/plan.md`, a case-sensitive one none. Members that do not stat are in no set.
///
/// `same` is the identity rule of [F20 §5.3] and [OS/project §3.2]: callers pass `moirai_vfs::OsFileId::same_object`,
/// under which kind `none` is equal to nothing and `parent`, `aux` and `docid` take no part. It is not `==`: an
/// `OsFileId` of kind `none` equals `OsFileId::NONE` under `==`, and a stat that fills `parent` differs under `==`
/// from an enumeration that leaves it zero. A member whose identity is not `same` as itself (kind `none`: a volume
/// without trusted ids) denotes no file that another member can share, so it is in no set, like a member that does
/// not stat. `same` must be symmetric and transitive on the identities that are `same` as themselves; each class is
/// the members `same` as its first member.
///
/// Each set lists member indices in the members' path order; the sets are in the order of their first member. The
/// cost is quadratic in the group's size (identity is only a predicate); a group holds the few spellings one
/// `fold_v1` key has.
pub fn twin_sets<F>(group: &[TwinMember<'_, F>], same: impl Fn(&F, &F) -> bool) -> Vec<Vec<usize>> {
    debug_assert!(
        {
            let mut p: Vec<&str> = group.iter().map(|m| m.path).collect();
            p.sort_unstable();
            p.windows(2).all(|w| w[0] != w[1])
        },
        "the members of a twin-candidate group are distinct spellings"
    );
    // The members with an identity, in path order: (member index, identity).
    let mut identified: Vec<(usize, &F)> = group
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.file.as_ref().filter(|f| same(f, f)).map(|f| (i, f)))
        .collect();
    identified.sort_by(|a, b| group[a.0].path.cmp(group[b.0].path));
    let mut taken = vec![false; identified.len()];
    let mut sets = Vec::new();
    for k in 0..identified.len() {
        if taken[k] {
            continue;
        }
        let first = identified[k].1;
        let mut class = Vec::new();
        for (t, &(j, f)) in taken[k..].iter_mut().zip(&identified[k..]) {
            if !*t && same(first, f) {
                *t = true;
                class.push(j);
            }
        }
        if class.len() >= 2 && class.iter().any(|&j| group[j].node) {
            sets.push(class);
        }
    }
    sets
}

/// The kind of collision an ambiguous twin set reports ([F20 §3.5]; [F18 §4.6] R-16).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Collision {
    /// `ambiguous (case collision)`: some two members are not canonically equivalent.
    Case,
    /// `ambiguous (normalization collision)`: all members are pairwise canonically equivalent (`ceq`).
    Normalization,
}

/// The collision kind of a twin set's member spellings ([F20 §3.5]): `Normalization` iff every two members are
/// `ceq` ([F20 §3.4]). `ceq` is an equivalence, so each member is compared with the first only.
pub fn collision(members: &[&str]) -> Collision {
    match members.split_first() {
        Some((first, rest)) if !rest.iter().all(|m| ceq(first, m)) => Collision::Case,
        _ => Collision::Normalization,
    }
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
    fn device_names() {
        for d in [
            "CON",
            "con",
            "Con.txt",
            "CON .tar.gz",
            "prn",
            "AUX.",
            "nul.x.y",
            "COM0",
            "com9.log",
            "LPT1",
            "lpt5",
            "COM\u{B9}",
            "lpt\u{B2}.txt",
            "Com\u{B3}",
            "CONIN$",
            "conout$.txt",
            "NUL   .md",
        ] {
            assert!(is_device_name(d), "{d:?}");
        }
        for n in [
            "CONSOLE",
            "COM10",
            "COM",
            "LPT",
            "xCON",
            ".CON",
            "CON-1",
            "COM\u{2074}",
            "NUL\t",
            " CON",
            "CONIN",
            "CONOUT$$",
            "",
            ".",
            "LPT\u{B9}\u{B9}",
            "COM\u{C2}",
        ] {
            assert!(!is_device_name(n), "{n:?}");
        }
        assert_eq!(device_name_stem("Con .tar.gz"), Some("Con"));
        assert_eq!(device_name_stem("lpt\u{B2}.txt"), Some("lpt\u{B2}"));
    }

    /// The cases [OS/path §8.1] asks of both copies of the rule (this crate's and `moirai-os`'s): every device name of
    /// §8.2 bare, with an extension and with trailing spaces before the extension, in two ASCII cases; a name ending in
    /// `.` and one ending in ` `; each reserved character; a name of 255 and one of 256 UTF-16 code units, one pair
    /// built from a supplementary-plane character (two units each). Linux and macOS count bytes only.
    #[test]
    fn representable_common_cases() {
        let mut devices: Vec<String> = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
            .iter()
            .map(|d| (*d).to_owned())
            .collect();
        for p in ["COM", "LPT"] {
            devices.extend((0..10).map(|n| format!("{p}{n}")));
            devices.extend(['\u{B9}', '\u{B2}', '\u{B3}'].map(|s| format!("{p}{s}")));
        }
        assert_eq!(devices.len(), 6 + 2 * 13);
        for d in &devices {
            for name in [d.clone(), format!("{d}.txt"), format!("{d}  .txt")] {
                for spelled in [name.clone(), name.to_ascii_lowercase()] {
                    assert!(!representable(Os::Windows, &spelled), "{spelled:?}");
                    assert!(representable(Os::Linux, &spelled), "{spelled:?}");
                    assert!(representable(Os::MacOs, &spelled), "{spelled:?}");
                }
            }
        }
        for n in ["notes.", "notes "] {
            assert!(!representable(Os::Windows, n));
            assert!(representable(Os::Linux, n) && representable(Os::MacOs, n));
        }
        for c in RESERVED_CHARS {
            let n = format!("a{c}b");
            assert!(!representable(Os::Windows, &n), "{n:?}");
            assert!(representable(Os::Linux, &n) && representable(Os::MacOs, &n));
        }
        let a255 = "a".repeat(255);
        let a256 = "a".repeat(256);
        for os in Os::ALL {
            assert!(representable(os, &a255));
            assert!(!representable(os, &a256));
        }
        // U+1D11E: two UTF-16 units, four UTF-8 bytes.
        let clef = '\u{1D11E}';
        let units255 = format!("{}a", clef.to_string().repeat(127));
        let units256 = clef.to_string().repeat(128);
        assert_eq!(units255.encode_utf16().count(), 255);
        assert_eq!(units256.encode_utf16().count(), 256);
        assert!(representable(Os::Windows, &units255));
        assert!(!representable(Os::Windows, &units256));
        // 509 and 512 UTF-8 bytes: too long for Linux and macOS either way.
        assert!(!representable(Os::Linux, &units255) && !representable(Os::MacOs, &units255));
        // 128 × U+00E9: 128 UTF-16 units but 256 bytes.
        let e256 = "\u{E9}".repeat(128);
        assert!(representable(Os::Windows, &e256));
        assert!(!representable(Os::Linux, &e256) && !representable(Os::MacOs, &e256));
        for n in ["console", "com10.txt", "CON-1", "x.CON", "plan.md"] {
            for os in Os::ALL {
                assert!(representable(os, n), "{os:?} {n:?}");
            }
        }
    }

    #[test]
    fn os_tags() {
        for os in Os::ALL {
            assert_eq!(Os::from_tag(os.tag()), Some(os));
        }
        assert_eq!(Os::ALL.map(Os::tag), [1, 2, 3], "[OS/proc §2]'s tag bytes");
        assert_eq!(Os::from_tag(0), None, "Unspecified names no OS");
        assert!((4..=255).all(|t| Os::from_tag(t).is_none()));
    }

    #[test]
    fn each_issue_of_the_table() {
        let none: [&str; 0] = [];
        assert!(portable_issues("main.rs", none).is_empty());
        let i = portable_issues("aux.rs", none);
        assert_eq!(i.device_stem, Some("aux"));
        assert_eq!(i.iter().collect::<Vec<_>>(), [PortableIssue::DeviceName]);
        assert!(portable_issues("notes.", none).trailing_dot_or_space);
        assert!(portable_issues("notes ", none).trailing_dot_or_space);
        assert!(!portable_issues("notes.\u{A0}", none).trailing_dot_or_space);
        assert_eq!(portable_issues("a:b|c", none).reserved_char, Some(':'));
        assert_eq!(portable_issues("what?", none).reserved_char, Some('?'));
        assert!(!portable_issues(&"x".repeat(255), none).too_long);
        assert!(portable_issues(&"x".repeat(256), none).too_long);
        assert!(
            portable_issues(&"\u{E9}".repeat(128), none).too_long,
            "256 UTF-8 bytes"
        );
        let all = portable_issues("CON.a:*.", ["CON.a:*."]);
        assert!(
            !all.contains(PortableIssue::FoldSibling),
            "the segment is not its own sibling"
        );
        let all = portable_issues("CON.a:*.", ["con.A:*.", "cOn.a:*.", "CON.b:*."]);
        assert_eq!(all.device_stem, Some("CON"));
        assert_eq!(all.reserved_char, Some(':'));
        assert_eq!(all.fold_sibling, Some("cOn.a:*."));
        let names: Vec<&str> = all.iter().map(PortableIssue::name).collect();
        assert_eq!(
            names,
            [
                "device-name",
                "trailing-dot-or-space",
                "reserved-char",
                "fold-sibling"
            ]
        );
    }

    #[test]
    fn fold_siblings() {
        let siblings = [
            "README.md",
            "Plan.md",
            "plan.md",
            "PLAN.MD",
            "stra\u{DF}e",
            "caf\u{E9}",
        ];
        // The segment itself is never its own sibling; the least equal sibling is named.
        let i = portable_issues("plan.md", siblings);
        assert_eq!(i.fold_sibling, Some("PLAN.MD"));
        assert_eq!(
            portable_issues("STRASSE", siblings).fold_sibling,
            Some("stra\u{DF}e")
        );
        assert_eq!(
            portable_issues("cafe\u{301}", siblings).fold_sibling,
            Some("caf\u{E9}")
        );
        assert_eq!(
            portable_issues("Caf\u{C9}", siblings).fold_sibling,
            Some("caf\u{E9}")
        );
        assert_eq!(portable_issues("readme.txt", siblings).fold_sibling, None);
        assert_eq!(
            portable_issues("\u{212A}elvin", ["kelvin"]).fold_sibling,
            Some("kelvin")
        );
        // `siblings` are the names after the operation: the case-only rename `a.md` → `A.md` leaves `a.md` out and
        // passes, while a copy that keeps `a.md` beside `A.md` has the issue.
        assert!(portable_issues("A.md", ["b.md", "A.md"]).is_empty());
        assert_eq!(
            portable_issues("A.md", ["a.md", "b.md"]).fold_sibling,
            Some("a.md")
        );
    }

    #[test]
    fn equivalents_and_groups() {
        let spellings = [
            "docs/Plan.md",
            "docs/plan.md",
            "docs/plan.md",
            "docs/caf\u{E9}.md",
            "docs/cafe\u{301}.md",
            "docs/other.md",
            "src/\u{130}.rs",
            "src/i\u{307}.rs",
            "src/I.rs",
        ];
        assert_eq!(
            fold_equivalents("docs/PLAN.md", spellings),
            ["docs/Plan.md", "docs/plan.md"]
        );
        assert_eq!(
            fold_equivalents("docs/plan.md", spellings),
            ["docs/Plan.md"]
        );
        assert!(fold_equivalents("docs/other.md", spellings).is_empty());
        assert_eq!(
            fold_equivalents("src/i\u{307}.rs", spellings),
            ["src/\u{130}.rs"],
            "U+0130 folds to i U+0307, not to I"
        );
        assert_eq!(
            twin_candidate_groups(spellings),
            vec![
                vec!["docs/cafe\u{301}.md", "docs/caf\u{E9}.md"],
                vec!["docs/Plan.md", "docs/plan.md"],
                vec!["src/i\u{307}.rs", "src/\u{130}.rs"],
            ]
        );
        assert_eq!(
            ceq_equivalents(
                "caf\u{E9}.md",
                ["cafe\u{301}.md", "caf\u{E9}.md", "CAF\u{C9}.md", "cafe.md"]
            ),
            ["cafe\u{301}.md"]
        );
        assert!(ceq_equivalents("a.md", ["A.md", "a.md"]).is_empty());
    }

    /// A file identity shaped like `moirai_vfs::OsFileId` ([OS/project §3.1]): `==` compares every field, as
    /// `OsFileId`'s does, and [`Id::same_object`] is the identity rule of [OS/project §3.2].
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    struct Id {
        /// 0 is `none`.
        kind: u8,
        vol_key: u8,
        id: u32,
        parent: u32,
    }

    impl Id {
        const NONE: Id = Id {
            kind: 0,
            vol_key: 0,
            id: 0,
            parent: 0,
        };

        fn of(id: u32, parent: u32) -> Id {
            Id {
                kind: 1,
                vol_key: 9,
                id,
                parent,
            }
        }

        fn same_object(&self, other: &Id) -> bool {
            self.kind != 0
                && self.kind == other.kind
                && self.vol_key == other.vol_key
                && self.id == other.id
        }
    }

    #[test]
    fn twin_sets_follow_the_observed_identity() {
        let m = |path, node, file: Option<Id>| TwinMember { path, node, file };
        // Case-insensitive directory: both spellings stat to file 7.
        let ci = [
            m("docs/plan.md", true, Some(Id::of(7, 1))),
            m("docs/Plan.md", true, Some(Id::of(7, 1))),
        ];
        assert_eq!(twin_sets(&ci, Id::same_object), vec![vec![1, 0]]);
        // Case-sensitive directory: two files, no twin set.
        let cs = [
            m("docs/plan.md", true, Some(Id::of(7, 1))),
            m("docs/Plan.md", true, Some(Id::of(8, 1))),
        ];
        assert!(twin_sets(&cs, Id::same_object).is_empty());
        // A member that does not stat is in no set; a set of τ(H)-only spellings is not a twin set.
        let mixed = [
            m("a/X", true, Some(Id::of(1, 5))),
            m("a/x", false, Some(Id::of(1, 5))),
            m("a/\u{1E8B}", true, None),
            m("b/Y", false, Some(Id::of(2, 6))),
            m("b/y", false, Some(Id::of(2, 6))),
        ];
        assert_eq!(twin_sets(&mixed, Id::same_object), vec![vec![0, 1]]);
    }

    #[test]
    fn twin_sets_use_the_identity_rule_not_equality() {
        let m = |path, node, file: Option<Id>| TwinMember { path, node, file };
        // A volume without trusted ids (a case-sensitive network share): every stat gives kind `none`. The two
        // files are not one file, though their ids are equal under `==`.
        assert_eq!(Id::NONE, Id::NONE);
        let no_ids = [
            m("docs/plan.md", true, Some(Id::NONE)),
            m("docs/Plan.md", true, Some(Id::NONE)),
            m("docs/PLAN.md", false, Some(Id::NONE)),
        ];
        assert!(twin_sets(&no_ids, Id::same_object).is_empty());
        // A kind-`none` member beside two members with ids is in no set; the others still form theirs.
        let partly = [
            m("x/A", true, Some(Id::of(3, 2))),
            m("x/a", true, Some(Id::NONE)),
            m("x/\u{1D00}", false, Some(Id::of(3, 2))),
        ];
        assert_eq!(twin_sets(&partly, Id::same_object), vec![vec![0, 2]]);
        // One file seen through a stat that filled `parent` and an enumeration that left it zero: unequal under
        // `==`, one object under the rule, so the twin set is found.
        let (stat, listed) = (Id::of(7, 42), Id::of(7, 0));
        assert_ne!(stat, listed);
        let seen = [
            m("docs/Plan.md", true, Some(stat)),
            m("docs/plan.md", false, Some(listed)),
        ];
        assert_eq!(twin_sets(&seen, Id::same_object), vec![vec![0, 1]]);
        // Another volume with the same id bytes is another file.
        let other_volume = Id {
            vol_key: 8,
            ..Id::of(7, 0)
        };
        let two_volumes = [
            m("docs/Plan.md", true, Some(stat)),
            m("docs/plan.md", true, Some(other_volume)),
        ];
        assert!(twin_sets(&two_volumes, Id::same_object).is_empty());
    }

    #[test]
    fn collision_kinds() {
        assert_eq!(
            collision(&["docs/Plan.md", "docs/plan.md"]),
            Collision::Case
        );
        assert_eq!(
            collision(&["caf\u{E9}", "cafe\u{301}"]),
            Collision::Normalization
        );
        assert_eq!(collision(&["Caf\u{E9}", "cafe\u{301}"]), Collision::Case);
    }

    fn spelling() -> impl Strategy<Value = String> {
        let alphabet = vec![
            'a', 'A', 'b', 'B', 's', 'S', 'k', 'K', '/', '.', '\u{DF}', '\u{1E9E}', '\u{212A}',
            '\u{E9}', '\u{C9}', '\u{301}', '\u{130}', '\u{307}', 'i', 'I',
        ];
        proptest::collection::vec(proptest::sample::select(alphabet), 1..6)
            .prop_map(|v| v.into_iter().collect())
    }

    /// Segments around every condition of [OS/path §8.1]: device stems, dots, spaces, reserved characters, and runs
    /// of one-, two- and four-byte characters long enough to cross 255 units in each OS's measure.
    fn os_segment() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            proptest::sample::select(vec![
                "CON",
                "com1",
                "LPT\u{B3}",
                "conout$",
                "Nul",
                ".",
                " ",
                ":",
                "?",
                "*",
                "a",
                "txt",
                "\u{E9}",
                "\u{1D11E}",
            ])
            .prop_map(str::to_owned),
            (1usize..130).prop_map(|n| "b".repeat(n)),
            (1usize..90).prop_map(|n| "\u{E9}".repeat(n)),
            (1usize..70).prop_map(|n| "\u{1D11E}".repeat(n)),
        ];
        proptest::collection::vec(piece, 1..6).prop_map(|v| v.concat())
    }

    proptest! {
        #![proptest_config(test_config(256))]

        #[test]
        fn groups_partition_by_fold(v in proptest::collection::vec(spelling(), 0..12)) {
            let refs: Vec<&str> = v.iter().map(String::as_str).collect();
            let groups = twin_candidate_groups(refs.iter().copied());
            let mut seen = std::collections::BTreeSet::new();
            for g in &groups {
                prop_assert!(g.len() >= 2);
                prop_assert!(g.windows(2).all(|w| w[0] < w[1]));
                for p in g {
                    prop_assert!(seen.insert(*p));
                    prop_assert_eq!(fold_v1(p), fold_v1(g[0]));
                    // Every group is one path's equivalents plus the path.
                    let mut want = fold_equivalents(p, refs.iter().copied());
                    want.push(p);
                    want.sort_unstable();
                    prop_assert_eq!(&want, g);
                }
            }
            // A spelling in no group has no equivalent.
            for p in &refs {
                if !seen.contains(p) {
                    prop_assert!(fold_equivalents(p, refs.iter().copied()).is_empty());
                }
            }
        }

        /// [OS/path §8.1] against §8.2: Windows refuses exactly the device-name, trailing and reserved-character
        /// issues and counts UTF-16 code units; Linux and macOS refuse exactly `too-long`. For an ASCII segment the
        /// two units agree, so it is representable on Windows iff it has no issue (it has no sibling here).
        #[test]
        fn representable_agrees_with_portable_issues(seg in os_segment()) {
            let none: [&str; 0] = [];
            let issues = portable_issues(&seg, none);
            let units = seg.encode_utf16().count();
            prop_assert_eq!(
                representable(Os::Windows, &seg),
                issues.device_stem.is_none()
                    && !issues.trailing_dot_or_space
                    && issues.reserved_char.is_none()
                    && units <= MAX_NAME_UNITS
            );
            prop_assert_eq!(representable(Os::Linux, &seg), !issues.too_long);
            prop_assert_eq!(representable(Os::MacOs, &seg), !issues.too_long);
            if seg.is_ascii() {
                prop_assert_eq!(representable(Os::Windows, &seg), issues.is_empty());
            }
        }

        #[test]
        fn fold_sibling_is_the_least_equivalent(seg in spelling(), v in proptest::collection::vec(spelling(), 0..8)) {
            let refs: Vec<&str> = v.iter().map(String::as_str).collect();
            let eq = fold_equivalents(&seg, refs.iter().copied());
            prop_assert_eq!(portable_issues(&seg, refs.iter().copied()).fold_sibling, eq.first().copied());
        }
    }
}
