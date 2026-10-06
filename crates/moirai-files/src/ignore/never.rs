//! The never-candidate names of resolver version 1 ([F20 §4.7]): the 21 basename patterns of [F20 §4.7.2] — [40 §4.3]'s
//! temporaries and backups and the [80 §2.11.4] rule 5 (X-F8) additions — and the two contextual rules of
//! [F20 §4.7.3]. A path that matches is never a candidate, and an id source that locates a node there makes it
//! `missing` with the place, never re-bound ([F20 §4.7.3] "Located paths", the caller's rule).
//!
//! Matching ([F20 §4.7.1]) is on the last component, whole-name: `*` matches any byte sequence (the empty one too),
//! `?` exactly one byte, and every other byte a byte `eqi`-equal to it ([F20 §1.2]); there is no other metacharacter.
//! One list applies on every OS. The match keeps one backtrack point (the last `*`) and allocates nothing; it takes
//! O(|pattern| × |name|) steps, which is linear in the name for the fixed patterns of the list.

use crate::r14::NEVER_CANDIDATE_PATTERNS;
use crate::text::eqi;

/// Which rule of [F20 §4.7] keeps a path from being a candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NeverCandidate {
    /// The pattern of [F20 §4.7.2]'s row `n` (1–21), [`NEVER_CANDIDATE_PATTERNS`]`[n - 1]`.
    Pattern(usize),
    /// The old name plus a suffix ([F20 §4.7.3]).
    OldNamePlusSuffix,
    /// A cloud conflict copy beside a live file node ([F20 §4.7.3]).
    CloudConflictCopy,
}

/// The last component of a root-relative path: the bytes after its last `/` ([F20 §1.2] `basename`).
#[must_use]
pub fn last_component(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// True iff `name` matches `pattern` under [F20 §4.7.1]'s syntax (whole name; `*`, `?`, otherwise `eqi`).
// spec: [F20 §4.7.1]
#[must_use]
pub fn matches_name_pattern(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0usize, 0usize);
    // The last `*` seen and the name position its run would end at next.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == b'?' || c.eq_ignore_ascii_case(&name[n]) => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p.min(pattern.len())..].iter().all(|&c| c == b'*')
}

/// The row (1–21) of the first pattern of [F20 §4.7.2] that the last component of `path` matches, or `None`.
// spec: [F20 §4.7.2] (the list, resolver version 1, with the [80 §2.11.4] rule 5 additions)
#[must_use]
pub fn never_pattern(path: &[u8]) -> Option<usize> {
    let name = last_component(path);
    NEVER_CANDIDATE_PATTERNS
        .iter()
        .position(|pat| matches_name_pattern(pat, name))
        .map(|i| i + 1)
}

/// The old-name-plus-suffix rule ([F20 §4.7.3], [41 M10]): true iff the last component of `q` is the last component
/// of `p` (F's current path), compared with `eqi`, followed by at least one more byte.
// spec: [F20 §4.7.3] (old name plus a suffix)
#[must_use]
pub fn is_old_name_plus_suffix(q: &[u8], p: &[u8]) -> bool {
    let (bq, bp) = (last_component(q), last_component(p));
    bq.len() > bp.len() && eqi(&bq[..bp.len()], bp)
}

/// The cloud-conflict-copy rule ([F20 §4.7.3], [40 §4.6]) for one live file node beside q: true iff `linked` (the
/// node's path) lies in q's directory (the bytes before the last `/` are equal; both root-relative and in one
/// spelling) and the last component of `q` is `stem ‖ "-" ‖ X ‖ ext`, where the last component of `linked` is
/// `stem ‖ ext`, ext is empty when that component has no `.` and is otherwise its last `.` and the bytes after it, and
/// X is non-empty and holds neither `.` nor `/`. Bytes compare exactly. The caller applies it only when q lies under a
/// cloud sync root (`TREES.flags` bit 1 `cloud_root`, [F11 §12.4]) and over every live file node in q's directory.
// spec: [F20 §4.7.3] (cloud conflict copies)
#[must_use]
pub fn is_cloud_conflict_copy(q: &[u8], linked: &[u8]) -> bool {
    let (bq, bl) = (last_component(q), last_component(linked));
    if q[..q.len() - bq.len()] != linked[..linked.len() - bl.len()] {
        return false;
    }
    let ext_at = bl.iter().rposition(|&b| b == b'.').unwrap_or(bl.len());
    let (stem, ext) = bl.split_at(ext_at);
    if bq.len() < stem.len() + 2 + ext.len() {
        return false;
    }
    let Some(rest) = bq.strip_prefix(stem).and_then(|r| r.strip_prefix(b"-")) else {
        return false;
    };
    let Some(x) = rest.strip_suffix(ext) else {
        return false;
    };
    !x.is_empty() && !x.iter().any(|&b| b == b'.' || b == b'/')
}

/// The first rule of [F20 §4.7] that keeps `q` from being a candidate or a target: a pattern of [F20 §4.7.2], then the
/// old-name-plus-suffix rule against F's current path `p`, then, when q lies under a cloud sync root, the
/// cloud-conflict-copy rule against the paths of the live file nodes in q's directory (`siblings`; a path outside q's
/// directory never counts).
// spec: [F20 §4.7.2], [F20 §4.7.3]
#[must_use]
pub fn never_candidate<'s, I>(
    q: &[u8],
    p: &[u8],
    under_cloud_root: bool,
    siblings: I,
) -> Option<NeverCandidate>
where
    I: IntoIterator<Item = &'s [u8]>,
{
    if let Some(n) = never_pattern(q) {
        return Some(NeverCandidate::Pattern(n));
    }
    if is_old_name_plus_suffix(q, p) {
        return Some(NeverCandidate::OldNamePlusSuffix);
    }
    if under_cloud_root && siblings.into_iter().any(|s| is_cloud_conflict_copy(q, s)) {
        return Some(NeverCandidate::CloudConflictCopy);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_row_has_an_example() {
        let rows: [(&[u8], usize); 21] = [
            (b"a.tmp", 1),
            (b"a.tmp.123", 2),
            (b"Main.java___jb_tmp___", 3),
            (b"Main.java___jb_old___", 4),
            (b"notes.md~", 5),
            (b"x.bak", 6),
            (b"x.orig", 7),
            (b"x.old", 8),
            (b"x.rej", 9),
            (b".x.swp", 10),
            (b".x.swo", 11),
            (b"4913", 12),
            (b".#notes.md", 13),
            (b"~$report.docx", 14),
            (b"sedAb3x9Q", 15),
            (b"._notes.md", 16),
            (b".DS_Store", 17),
            (b".fuse_hidden0000001", 18),
            (b".nfs000000000123", 19),
            (b".goutputstream-ABC123", 20),
            (b".~lock.notes.odt#", 21),
        ];
        for (name, row) in rows {
            assert_eq!(
                never_pattern(name),
                Some(row),
                "{}",
                String::from_utf8_lossy(name)
            );
            let mut path = b"dir/sub/".to_vec();
            path.extend_from_slice(name);
            assert_eq!(never_pattern(&path), Some(row));
        }
    }

    #[test]
    fn ordinary_names_pass() {
        for name in [
            &b"main.rs"[..],
            b"tmp",
            b"a.tmpx",
            b"x.bakk",
            b"49134",
            b"sed12345",
            b"sed1234567",
            b"lock.odt#",
            b".~lock.odt",
            b"DS_Store",
            b"",
            b"dir.tmp/file.rs",
        ] {
            assert_eq!(
                never_pattern(name),
                None,
                "{}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn eqi_and_literal_metacharacters() {
        assert_eq!(never_pattern(b"A.TMP"), Some(1));
        assert_eq!(never_pattern(b".ds_store"), Some(17));
        assert_eq!(never_pattern(b"SEDabcdef"), Some(15));
        // `$`, `#`, `.` and `~` are literal; non-ASCII bytes compare exactly.
        assert!(matches_name_pattern(b"~$*", b"~$x"));
        assert!(!matches_name_pattern(b"~$*", b"~x"));
        assert!(!matches_name_pattern(b"*.tmp", b"atmp"));
        assert!(matches_name_pattern(b"\xc3\x80*", b"\xc3\x80x"));
        assert!(!matches_name_pattern(b"\xc3\x80*", b"\xc3\xa0x"));
        assert!(matches_name_pattern(b"*", b""));
        assert!(matches_name_pattern(b"**a**", b"xay"));
        assert!(!matches_name_pattern(b"?", b""));
        assert!(matches_name_pattern(b"*?", b"x"));
        assert!(!matches_name_pattern(b"a", b"ab"));
    }

    #[test]
    fn old_name_plus_suffix() {
        assert!(is_old_name_plus_suffix(
            b"docs/plan.md.bak2",
            b"docs/plan.md"
        ));
        assert!(is_old_name_plus_suffix(b"x/PLAN.MDx", b"plan.md"));
        assert!(!is_old_name_plus_suffix(b"docs/plan.md", b"docs/plan.md"));
        assert!(!is_old_name_plus_suffix(b"docs/xplan.md", b"docs/plan.md"));
        assert!(!is_old_name_plus_suffix(b"plan.m", b"plan.md"));
    }

    #[test]
    fn cloud_conflict_copy() {
        assert!(is_cloud_conflict_copy(
            b"d/notes-DESKTOP1.md",
            b"d/notes.md"
        ));
        assert!(is_cloud_conflict_copy(b"d/a.b-x.c", b"d/a.b.c"));
        assert!(is_cloud_conflict_copy(b"Makefile-x", b"Makefile"));
        assert!(!is_cloud_conflict_copy(b"notes-.md", b"notes.md"));
        assert!(!is_cloud_conflict_copy(b"notes-a.b.md", b"notes.md"));
        assert!(!is_cloud_conflict_copy(b"notes.md", b"notes.md"));
        assert!(!is_cloud_conflict_copy(b"Notes-x.md", b"notes.md"));
        assert!(!is_cloud_conflict_copy(b"notes-x.MD", b"notes.md"));
        // A leading-dot name: the stem is empty.
        assert!(is_cloud_conflict_copy(b"-x.env", b".env"));
        // Only a node in q's own directory counts.
        assert!(!is_cloud_conflict_copy(b"d/notes-PC.md", b"e/notes.md"));
        assert!(!is_cloud_conflict_copy(b"d/notes-PC.md", b"notes.md"));
        assert!(!is_cloud_conflict_copy(b"notes-PC.md", b"d/notes.md"));
        assert!(!is_cloud_conflict_copy(b"a/d/notes-PC.md", b"d/notes.md"));
        assert!(is_cloud_conflict_copy(b"a/d/notes-PC.md", b"a/d/notes.md"));
    }

    #[test]
    fn rule_order() {
        let none: [&[u8]; 0] = [];
        assert_eq!(
            never_candidate(b"a/plan.md.bak", b"a/plan.md", false, none),
            Some(NeverCandidate::Pattern(6))
        );
        assert_eq!(
            never_candidate(b"b/plan.md2", b"a/plan.md", false, none),
            Some(NeverCandidate::OldNamePlusSuffix)
        );
        let sib: [&[u8]; 2] = [b"c/other.md", b"c/notes.md"];
        assert_eq!(
            never_candidate(b"c/notes-PC.md", b"a/plan.md", true, sib),
            Some(NeverCandidate::CloudConflictCopy)
        );
        assert_eq!(
            never_candidate(b"c/notes-PC.md", b"a/plan.md", false, sib),
            None
        );
        assert_eq!(never_candidate(b"c/new.md", b"a/plan.md", true, sib), None);
        let elsewhere: [&[u8]; 1] = [b"d/notes.md"];
        assert_eq!(
            never_candidate(b"c/notes-PC.md", b"a/plan.md", true, elsewhere),
            None
        );
    }
}
