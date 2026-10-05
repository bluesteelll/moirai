//! P1–P12 of [OS/path] over data (X-F7, X-F9; I-F8): the `RelPath` and `AbsPath` grammars with their `PathError`s, the
//! lexical form of a machine-local absolute path, the CLI boundary, P7's `origin_path`, P5's portability issues,
//! representability per OS, P9's keys and lookup order, P11 (a) query file names and P11 (b) ref names. P6 is
//! [`crate::r4::fold`]; P8's symbolic-link `oid` is [`crate::r4::text::symlink_oid`]; P10 is an OS behaviour with no
//! data function.
//!
//! P3's NFC for untracked names on a normalization-insensitive volume is git's precomposition ([OS/path] open point 2);
//! it needs a composition table that the two pinned UCD files cannot give (canonical composition excludes the
//! characters `CompositionExclusions.txt` lists, which is not pinned), so [`stored_untracked_name`] reports that case
//! instead of guessing.

use crate::r4::fold::fold_v1;

/// The `PathError` of [OS/path §11], one meaning per variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    /// An empty segment.
    Empty,
    /// A malformed `AbsPath` prefix.
    BadSegment,
    /// A segment that is exactly `.` or `..`.
    DotSegment,
    /// A `/` inside a single segment.
    Separator,
    /// A C0 control in a `RelPath` segment, or U+0000 in an `AbsPath`.
    Control,
    /// A `\` in a `RelPath` segment.
    Backslash,
    /// Input that is not valid Unicode.
    NotUtf8,
    /// A value that must be absolute and is not, or could not be made absolute.
    NotAbsolute,
    /// A Windows `X:rel` or bare `X:` argument.
    DriveRelative,
    /// A Windows device-form argument (`//./…`, `//?/…`).
    DevicePath,
    /// A CLI argument outside the tree.
    OutsideRoot,
}

impl PathError {
    /// The variant's name as [OS/path §11] spells it.
    pub fn name(self) -> &'static str {
        match self {
            PathError::Empty => "Empty",
            PathError::BadSegment => "BadSegment",
            PathError::DotSegment => "DotSegment",
            PathError::Separator => "Separator",
            PathError::Control => "Control",
            PathError::Backslash => "Backslash",
            PathError::NotUtf8 => "NotUtf8",
            PathError::NotAbsolute => "NotAbsolute",
            PathError::DriveRelative => "DriveRelative",
            PathError::DevicePath => "DevicePath",
            PathError::OutsideRoot => "OutsideRoot",
        }
    }
}

/// An OS tag ([OS/proc §2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Os {
    /// Windows.
    Windows,
    /// Linux.
    Linux,
    /// macOS.
    Macos,
}

impl Os {
    /// The tag's name.
    pub fn name(self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::Macos => "macos",
        }
    }

    /// The tag of a name.
    pub fn from_name(s: &str) -> Option<Os> {
        match s {
            "windows" => Some(Os::Windows),
            "linux" => Some(Os::Linux),
            "macos" => Some(Os::Macos),
            _ => None,
        }
    }
}

/// One `RelPath` segment: non-empty, not `.` or `..`, no `\`, no C0 control (P1, P4).
fn rel_segment(seg: &str) -> Result<(), PathError> {
    if seg.is_empty() {
        return Err(PathError::Empty);
    }
    if seg == "." || seg == ".." {
        return Err(PathError::DotSegment);
    }
    for c in seg.chars() {
        if c == '\\' {
            return Err(PathError::Backslash);
        }
        if (c as u32) < 0x20 {
            return Err(PathError::Control);
        }
    }
    Ok(())
}

/// `rel-path` of [OS/path §2.1]: the empty value (the root) or segments joined by `/`; the text on success.
// spec: [OS/path §2.1]; [OS/path §3] P1, P4
pub fn relpath(b: &[u8]) -> Result<&str, PathError> {
    let s = std::str::from_utf8(b).map_err(|_| PathError::NotUtf8)?;
    if s.is_empty() {
        return Ok(s);
    }
    for seg in s.split('/') {
        rel_segment(seg)?;
    }
    Ok(s)
}

/// A stored path of a root other than `abs`: a non-empty `rel-path` ([F18 §2.8]).
// spec: [F18 §2.8] I-F8
pub fn stored_rel_path(s: &str) -> Result<(), PathError> {
    if s.is_empty() {
        return Err(PathError::Empty);
    }
    relpath(s.as_bytes()).map(|_| ())
}

/// A `pathmove` directory prefix: a non-empty `rel-path` followed by exactly one `/` ([F18 §2.8]).
// spec: [F18 §2.8] pathmove prefixes
pub fn dir_prefix(s: &str) -> Result<(), PathError> {
    match s.strip_suffix('/') {
        Some(p) => stored_rel_path(p),
        None => Err(PathError::Empty),
    }
}

/// `abs-seg`: non-empty, not `.` or `..`, no U+0000 (`/` never reaches a segment).
fn abs_segment(seg: &str) -> Result<(), PathError> {
    if seg.is_empty() {
        return Err(PathError::Empty);
    }
    if seg == "." || seg == ".." {
        return Err(PathError::DotSegment);
    }
    if seg.contains('\0') {
        return Err(PathError::Control);
    }
    Ok(())
}

fn abs_rel(rest: &str) -> Result<(), PathError> {
    if rest.is_empty() {
        return Ok(());
    }
    rest.split('/').try_for_each(abs_segment)
}

/// `abs-path` of [OS/path §2.2] (P12): `X:/…` with an upper-case drive, `//server/share[/…]`, or `/…`.
// spec: [OS/path §2.2]; [OS/path §3] P12
pub fn abspath(s: &str) -> Result<(), PathError> {
    let b = s.as_bytes();
    if let Some(rest) = s.strip_prefix("//") {
        let mut it = rest.splitn(3, '/');
        let server = it.next().unwrap_or("");
        let share = it.next().unwrap_or("");
        if server.is_empty() || share.is_empty() {
            return Err(PathError::BadSegment);
        }
        abs_segment(server)?;
        abs_segment(share)?;
        return match it.next() {
            Some(r) => {
                if r.is_empty() {
                    Err(PathError::Empty)
                } else {
                    abs_rel(r)
                }
            }
            None => Ok(()),
        };
    }
    if let Some(rest) = s.strip_prefix('/') {
        return abs_rel(rest);
    }
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        if !b[0].is_ascii_uppercase() || b.get(2) != Some(&b'/') {
            return Err(PathError::BadSegment);
        }
        return abs_rel(&s[3..]);
    }
    Err(PathError::NotAbsolute)
}

/// The prefix of a Windows absolute path (`X:` upper-cased, or `//server/share`) and the rest, `/`-separated.
fn win_prefix(s: &str) -> Option<(String, &str)> {
    let b = s.as_bytes();
    if let Some(rest) = s.strip_prefix("//") {
        let mut it = rest.splitn(3, '/');
        let server = it.next()?;
        let share = it.next()?;
        if server.is_empty() || share.is_empty() {
            return None;
        }
        return Some((format!("//{server}/{share}"), it.next().unwrap_or("")));
    }
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/' {
        return Some((format!("{}:", b[0].to_ascii_uppercase() as char), &s[3..]));
    }
    None
}

/// Lexical normalisation of [OS/path §5] step 2 over segments: empty and `.` dropped, `..` removing the previous one
/// (never the prefix), joined with `/`.
fn normalise_segments(rest: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out
}

/// A lexically normalised absolute path: the prefix, then the segments.
fn join_abs(windows: bool, prefix: &str, segs: &[&str]) -> String {
    if windows {
        if prefix.starts_with("//") {
            if segs.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}/{}", segs.join("/"))
            }
        } else {
            format!("{prefix}/{}", segs.join("/"))
        }
    } else {
        format!("/{}", segs.join("/"))
    }
}

/// [OS/path §5] step 2 for a path that does not exist: made absolute against `cwd` (the canonical current directory)
/// when relative, then normalised lexically; the drive letter upper-cased. A Windows argument with `\` separators is
/// read with `/`. A relative Windows argument is joined to `cwd` as a Unix one is (the rewriting `GetFullPathNameW`
/// adds is not specified, fixture gap G-3).
// spec: [OS/path §5] step 2; [OS/path §3] P12
pub fn canonical_abs_lexical(
    windows: bool,
    cwd: Option<&str>,
    text: &str,
) -> Result<String, PathError> {
    if windows {
        let t = text.replace('\\', "/");
        let full = match win_prefix(&t) {
            Some(_) => t,
            None => match cwd {
                Some(c) => format!("{c}/{t}"),
                None => return Err(PathError::NotAbsolute),
            },
        };
        let (prefix, rest) = win_prefix(&full).ok_or(PathError::NotAbsolute)?;
        let out = join_abs(true, &prefix, &normalise_segments(rest));
        abspath(&out)?;
        Ok(out)
    } else {
        let full = if text.starts_with('/') {
            text.to_string()
        } else {
            match cwd {
                Some(c) => format!("{c}/{text}"),
                None => return Err(PathError::NotAbsolute),
            }
        };
        let out = join_abs(false, "", &normalise_segments(&full[1..]));
        abspath(&out)?;
        Ok(out)
    }
}

/// `cli_path(arg, cwd, tree)` of [OS/path §7]: a path argument turned into a root-relative path of the tree.
// spec: [OS/path §7]
pub fn cli_path(windows: bool, tree: &str, cwd: &str, arg: &str) -> Result<String, PathError> {
    let full = if windows {
        let t = arg.replace('\\', "/");
        let b = t.as_bytes();
        if t.starts_with("//./") || t.starts_with("//?/") || t == "//." || t == "//?" {
            return Err(PathError::DevicePath);
        }
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b.get(2) != Some(&b'/') {
            return Err(PathError::DriveRelative);
        }
        if t.starts_with("//") {
            if win_prefix(&t).is_none() {
                return Err(PathError::NotAbsolute);
            }
            t
        } else if let Some(rest) = t.strip_prefix('/') {
            // The root of the current directory's drive; a UNC current directory has none.
            let cb = cwd.as_bytes();
            if cb.len() >= 2 && cb[0].is_ascii_alphabetic() && cb[1] == b':' {
                format!("{}:/{rest}", cb[0] as char)
            } else {
                return Err(PathError::NotAbsolute);
            }
        } else if win_prefix(&t).is_some() {
            t
        } else {
            format!("{cwd}/{t}")
        }
    } else if arg.starts_with('/') {
        arg.to_string()
    } else {
        format!("{cwd}/{arg}")
    };
    let norm = canonical_abs_lexical(windows, None, &full)?;
    if norm == tree {
        return Ok(String::new());
    }
    let prefix = if tree.ends_with('/') {
        tree.to_string()
    } else {
        format!("{tree}/")
    };
    let rest = norm.strip_prefix(&prefix).ok_or(PathError::OutsideRoot)?;
    relpath(rest.as_bytes()).map(str::to_string)
}

/// P3's stored spelling of an untracked name: the enumerated spelling, except on a normalization-insensitive volume
/// (`norm_insensitive_always`) with `core.precomposeUnicode = true` or without git, where it is git's precomposition —
/// `None` then, since the model does not derive NFC (module documentation).
// spec: [OS/path §3] P3
pub fn stored_untracked_name(
    text: &str,
    norm_insensitive_always: bool,
    git: bool,
    precompose_unicode: bool,
) -> Option<String> {
    if norm_insensitive_always && (precompose_unicode || !git) {
        None
    } else {
        Some(text.to_string())
    }
}

/// P7: `origin_path` follows P2 (a tracked file: git's HEAD-tree spelling) and P3 (an untracked file); `None` where
/// P3 needs git's precomposition.
// spec: [OS/path §3] P7, P2
pub fn origin_path(
    tracked: Option<&str>,
    enumerated: &str,
    norm_insensitive_always: bool,
    git: bool,
    precompose_unicode: bool,
) -> Option<String> {
    match tracked {
        Some(g) => Some(g.to_string()),
        None => stored_untracked_name(enumerated, norm_insensitive_always, git, precompose_unicode),
    }
}

/// One portability issue of [OS/path §8.2].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortableIssue {
    /// `device-name`.
    DeviceName,
    /// `trailing-dot-or-space`.
    TrailingDotOrSpace,
    /// `reserved-char`, citing the segment's first reserved character.
    ReservedChar(char),
    /// `too-long`.
    TooLong,
    /// `fold-sibling`, citing the smallest such sibling in byte order.
    FoldSibling(String),
}

/// Whether a segment's stem (the part before its first `.`, trailing ASCII spaces removed) is a Windows device name,
/// ignoring ASCII case.
// spec: [OS/path §8.2] device-name
pub fn device_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or(seg).trim_end_matches(' ');
    let up = stem.to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&up.as_str()) {
        return true;
    }
    for p in ["COM", "LPT"] {
        if let Some(rest) = up.strip_prefix(p) {
            let mut cs = rest.chars();
            if let (Some(c), None) = (cs.next(), cs.next())
                && (c.is_ascii_digit() || matches!(c, '\u{00B9}' | '\u{00B2}' | '\u{00B3}'))
            {
                return true;
            }
        }
    }
    false
}

const RESERVED: [char; 7] = ['<', '>', ':', '"', '|', '?', '*'];

/// `portable_issues(segment, siblings)` of [OS/path §8.2], in the table's order; `siblings` are the directory's names
/// after the operation other than the segment.
// spec: [OS/path §8.2]; [OS/path §3] P5
pub fn portable_issues(seg: &str, siblings: &[&str]) -> Vec<PortableIssue> {
    let mut out = Vec::new();
    if device_name(seg) {
        out.push(PortableIssue::DeviceName);
    }
    if seg.ends_with('.') || seg.ends_with(' ') {
        out.push(PortableIssue::TrailingDotOrSpace);
    }
    if let Some(c) = seg.chars().find(|c| RESERVED.contains(c)) {
        out.push(PortableIssue::ReservedChar(c));
    }
    if seg.len() > 255 {
        out.push(PortableIssue::TooLong);
    }
    let f = fold_v1(seg);
    let mut sibs: Vec<&str> = siblings
        .iter()
        .copied()
        .filter(|s| *s != seg && fold_v1(s) == f)
        .collect();
    sibs.sort_unstable();
    if let Some(s) = sibs.first() {
        out.push(PortableIssue::FoldSibling((*s).to_string()));
    }
    out
}

/// `representable(os, segment)` of [OS/path §8.1].
// spec: [OS/path §8.1]
pub fn representable(os: Os, seg: &str) -> bool {
    match os {
        Os::Windows => {
            !device_name(seg)
                && !seg.ends_with('.')
                && !seg.ends_with(' ')
                && !seg.chars().any(|c| RESERVED.contains(&c))
                && seg.encode_utf16().count() <= 255
        }
        Os::Linux | Os::Macos => seg.len() <= 255,
    }
}

/// `blake3_16`: the first 16 bytes of BLAKE3-256 of bytes ([F01 §7.1]).
pub fn blake3_16(b: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&blake3::hash(b).as_bytes()[..16]);
    out
}

/// The `TREES` key and the directory-binding key: `blake3_16` of the canonical text's bytes.
// spec: [OS/path §4.5]
pub fn trees_key(canonical: &str) -> [u8; 16] {
    blake3_16(canonical.as_bytes())
}

/// A root id as [OS/path §4.4] compares it: a kind (0 = no trusted id) and the object's identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootId {
    /// The kind; 0 is `none`.
    pub kind: u8,
    /// The volume key and id, compared whole.
    pub id: Vec<u8>,
}

/// A stored `TREES` or binding row as [OS/path §4.4] looks it up.
#[derive(Clone, Debug)]
pub struct TreeRow {
    /// The canonical text.
    pub text: String,
    /// The root id.
    pub root_id: RootId,
    /// The OS tag of the process that wrote it.
    pub os: Os,
}

/// Lookup by id first ([OS/path §4.4], P9): the index of the row that is the tree, or `None` for a new tree. A row
/// written under another OS tag has an uninterpretable id and is found by spelling only.
// spec: [OS/path §4.4]; [OS/path §3] P9
pub fn lookup_tree(rows: &[TreeRow], text: &str, root_id: &RootId, os: Os) -> Option<usize> {
    if root_id.kind != 0
        && let Some(i) = rows
            .iter()
            .position(|r| r.os == os && r.root_id.kind != 0 && r.root_id == *root_id)
    {
        return Some(i);
    }
    let key = trees_key(text);
    rows.iter().position(|r| trees_key(&r.text) == key)
}

/// P11 (a): `schema/queries/<q>.moi`, q the 32 lower-case hex digits of the first 16 bytes of BLAKE3-256 over the
/// query name's UTF-8 bytes.
// spec: [OS/path §3] P11 (a)
pub fn query_file_name(name: &str) -> String {
    format!(
        "schema/queries/{}.moi",
        crate::value::hex(&blake3_16(name.as_bytes()))
    )
}

/// P11 (b): the ref-name rules of a complete name ([F12 §2.4]; no namespace completion), RN-1 to RN-8 in order, `live`
/// the live refs (each with whether it takes part in RN-8); the first failing rule. IN-1's NFC is observationally
/// inert here: no non-ASCII character composes to a character RN-1 admits, so RN-1 refuses every non-ASCII name
/// whether or not it was composed first.
// spec: [OS/path §3] P11 (b); [F12 §2.4]
pub fn ref_name_check(name: &str, live: &[(&str, bool)]) -> Result<(), &'static str> {
    if let Err(e) = crate::dag::check_rules(name, name) {
        return Err(match e.get_str("rule") {
            Some("RN-1") => "RN-1",
            Some("RN-2") => "RN-2",
            Some("RN-3") => "RN-3",
            Some("RN-4") => "RN-4",
            Some("RN-5") => "RN-5",
            Some("RN-6") => "RN-6",
            other => panic!("check_rules refused with {other:?}"),
        });
    }
    match crate::dag::unique_rule(name, live.iter().copied()) {
        Some((rule, _)) => Err(rule),
        None => Ok(()),
    }
}

/// One glob segment against one path segment, by scalar values ([F08 §5.4.3]): `*` any run inside the segment, `?`
/// one scalar value, a class one scalar value in (with `!`, not in) its items.
fn seg_match(g: &[char], p: &[char]) -> bool {
    match g.first() {
        None => p.is_empty(),
        Some('*') => (0..=p.len()).any(|i| seg_match(&g[1..], &p[i..])),
        Some('?') => !p.is_empty() && seg_match(&g[1..], &p[1..]),
        Some('[') => {
            let Some(close) = g.iter().skip(2).position(|c| *c == ']').map(|i| i + 2) else {
                return !p.is_empty() && p[0] == '[' && seg_match(&g[1..], &p[1..]);
            };
            if p.is_empty() {
                return false;
            }
            let (neg, items) = match g.get(1) {
                Some('!') => (true, &g[2..close]),
                _ => (false, &g[1..close]),
            };
            let mut hit = false;
            let mut i = 0;
            while i < items.len() {
                if i + 2 < items.len() && items[i + 1] == '-' {
                    if (items[i]..=items[i + 2]).contains(&p[0]) {
                        hit = true;
                    }
                    i += 3;
                } else {
                    if items[i] == p[0] {
                        hit = true;
                    }
                    i += 1;
                }
            }
            hit != neg && seg_match(&g[close + 1..], &p[1..])
        }
        Some(c) => !p.is_empty() && p[0] == *c && seg_match(&g[1..], &p[1..]),
    }
}

/// Whether a glob rooted at `project` matches a `RelPath` ([F08 §5.4.3]): segments in order, `**` zero or more whole
/// segments.
// spec: [F08 §5.4.3]
pub fn glob_match(glob: &str, path: &str) -> bool {
    fn segs(g: &[&str], p: &[&str]) -> bool {
        match g.first() {
            None => p.is_empty(),
            Some(&"**") => (0..=p.len()).any(|k| segs(&g[1..], &p[k..])),
            Some(s) => {
                !p.is_empty()
                    && seg_match(
                        &s.chars().collect::<Vec<_>>(),
                        &p[0].chars().collect::<Vec<_>>(),
                    )
                    && segs(&g[1..], &p[1..])
            }
        }
    }
    let g: Vec<&str> = glob.split('/').collect();
    let p: Vec<&str> = path.split('/').collect();
    segs(&g, &p)
}

/// The display form of a name ([OS/path §9]): UTF-8 as is, each byte of an ill-formed sequence as `\xNN`.
// spec: [OS/path §9]
pub fn display_name(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                break;
            }
            Err(e) => {
                let (ok, bad) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(ok).expect("valid prefix"));
                let n = e.error_len().unwrap_or(bad.len());
                for b in &bad[..n] {
                    out.push_str(&format!("\\x{b:02x}"));
                }
                rest = &bad[n..];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canon::fixtures::json;
    use crate::r4::tests::cases;
    use crate::value::Algo;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    fn arg<'a>(c: &'a crate::canon::fixtures::Case, name: &str) -> Option<&'a str> {
        c.lines.get("arg").and_then(|v| {
            v.iter()
                .find_map(|a| a.strip_prefix(name).and_then(|r| r.strip_prefix(' ')))
        })
    }

    fn args<'a>(c: &'a crate::canon::fixtures::Case, name: &str) -> Vec<&'a str> {
        c.lines.get("arg").map_or(Vec::new(), |v| {
            v.iter()
                .filter_map(|a| a.strip_prefix(name).and_then(|r| r.strip_prefix(' ')))
                .collect()
        })
    }

    fn expect_path(r: Result<String, PathError>) -> String {
        match r {
            Ok(s) => format!("\"{}\"", s),
            Err(e) => format!("error {}", e.name()),
        }
    }

    fn root_id(tok: &str) -> RootId {
        if tok == "0" {
            RootId {
                kind: 0,
                id: Vec::new(),
            }
        } else {
            let (kind, rest) = tok.split_once(':').expect("kind:vol:id");
            RootId {
                kind: match kind {
                    "ntfs" => 1,
                    "refs" => 2,
                    "ext4" => 3,
                    "apfs" => 4,
                    _ => 5,
                },
                id: rest.as_bytes().to_vec(),
            }
        }
    }

    /// Every case of `fixtures/r4/cases/paths.cases` whose function the model implements; the P3 cases that need
    /// git's precomposition report `None`, which the harness counts and lists.
    #[test]
    fn paths_cases_pass() {
        let all = cases("paths.cases");
        assert!(all.len() >= 137, "{} path cases", all.len());
        let mut not_modelled = Vec::new();
        for c in &all {
            let want = c.line("expect").unwrap_or("");
            let got: String = match c.line("function").expect("function") {
                "relpath" => {
                    let bytes = arg(c, "hex").map(unhex).unwrap_or_default();
                    if let Some(t) = arg(c, "text") {
                        assert_eq!(json(t).as_bytes(), bytes.as_slice(), "{}", c.id);
                    }
                    match relpath(&bytes) {
                        Ok(_) => "ok".into(),
                        Err(e) => format!("error {}", e.name()),
                    }
                }
                "abspath" => match abspath(&json(arg(c, "text").unwrap())) {
                    Ok(()) => "ok".into(),
                    Err(e) => format!("error {}", e.name()),
                },
                "canonical_abs_lexical" => {
                    let windows = arg(c, "os") == Some("windows");
                    let cwd = arg(c, "cwd").map(json);
                    let r = canonical_abs_lexical(
                        windows,
                        cwd.as_deref(),
                        &json(arg(c, "text").unwrap()),
                    );
                    expect_path(r)
                }
                "cli_path" => {
                    let windows = arg(c, "os") == Some("windows");
                    expect_path(cli_path(
                        windows,
                        &json(arg(c, "tree").unwrap()),
                        &json(arg(c, "cwd").unwrap()),
                        &json(arg(c, "text").unwrap()),
                    ))
                }
                "stored_untracked_name" => {
                    let b = |n: &str| arg(c, n) == Some("true");
                    match stored_untracked_name(
                        &json(arg(c, "text").unwrap()),
                        b("norm_insensitive_always"),
                        b("git"),
                        b("precompose_unicode"),
                    ) {
                        Some(s) => format!("\"{s}\""),
                        None => {
                            not_modelled.push(c.id.clone());
                            continue;
                        }
                    }
                }
                "origin_path" => {
                    let tracked = arg(c, "tracked") == Some("true");
                    let g = arg(c, "git_spelling").map(json);
                    match origin_path(
                        if tracked { g.as_deref() } else { None },
                        &json(arg(c, "enumerated").unwrap()),
                        arg(c, "norm_insensitive_always") == Some("true"),
                        true,
                        false,
                    ) {
                        Some(s) => format!("\"{s}\""),
                        None => {
                            not_modelled.push(c.id.clone());
                            continue;
                        }
                    }
                }
                "portable_issues" => {
                    let seg = json(arg(c, "segment").unwrap());
                    let sibs: Vec<String> = args(c, "sibling").into_iter().map(json).collect();
                    let sref: Vec<&str> = sibs.iter().map(String::as_str).collect();
                    let want_issues: Vec<PortableIssue> = if want == "none" {
                        Vec::new()
                    } else {
                        want.split(" , ")
                            .map(|t| match t.split_once(' ') {
                                None if t == "device-name" => PortableIssue::DeviceName,
                                None if t == "trailing-dot-or-space" => {
                                    PortableIssue::TrailingDotOrSpace
                                }
                                None if t == "too-long" => PortableIssue::TooLong,
                                Some(("reserved-char", v)) => PortableIssue::ReservedChar(
                                    json(v).chars().next().expect("a char"),
                                ),
                                Some(("fold-sibling", v)) => PortableIssue::FoldSibling(json(v)),
                                _ => panic!("{}: issue {t}", c.id),
                            })
                            .collect()
                    };
                    assert_eq!(portable_issues(&seg, &sref), want_issues, "{}", c.id);
                    continue;
                }
                "representable" => {
                    let os = Os::from_name(arg(c, "os").unwrap()).expect("os");
                    for row in c.block("rows") {
                        let row = row.trim();
                        if row.is_empty() {
                            continue;
                        }
                        let (seg, v) = row.rsplit_once(' ').expect("<json> true|false");
                        assert_eq!(
                            representable(os, &json(seg)),
                            v == "true",
                            "{}: representable({}, {seg})",
                            c.id,
                            os.name()
                        );
                    }
                    continue;
                }
                "symlink_oid" => {
                    let algo = match arg(c, "algo") {
                        Some("sha256") => Algo::Sha256,
                        _ => Algo::Sha1,
                    };
                    let o = crate::r4::text::symlink_oid(
                        algo,
                        json(arg(c, "target").unwrap()).as_bytes(),
                    );
                    crate::value::hex(&o.digest)
                }
                "trees_key" => crate::value::hex(&trees_key(&json(arg(c, "text").unwrap()))),
                "lookup_tree" => {
                    let mut names = Vec::new();
                    let mut rows = Vec::new();
                    for l in c.block("view") {
                        let Some(rest) = l.strip_prefix("row ") else {
                            continue;
                        };
                        let mut it = rest.split(' ');
                        names.push(it.next().unwrap().to_string());
                        let mut text = String::new();
                        let mut rid = RootId {
                            kind: 0,
                            id: Vec::new(),
                        };
                        let mut os = Os::Windows;
                        for kv in it {
                            let (k, v) = kv.split_once('=').unwrap();
                            match k {
                                "text" => text = json(v),
                                "root_id" => rid = root_id(v),
                                "os" => os = Os::from_name(v).unwrap(),
                                _ => panic!("{}: row field {k}", c.id),
                            }
                        }
                        rows.push(TreeRow {
                            text,
                            root_id: rid,
                            os,
                        });
                    }
                    match lookup_tree(
                        &rows,
                        &json(arg(c, "text").unwrap()),
                        &root_id(arg(c, "root_id").unwrap()),
                        Os::from_name(arg(c, "os").unwrap()).unwrap(),
                    ) {
                        Some(i) => names[i].clone(),
                        None => "new".into(),
                    }
                }
                "query_file_name" => {
                    format!("\"{}\"", query_file_name(&json(arg(c, "name").unwrap())))
                }
                "ref_name_check" => {
                    let live: Vec<String> = args(c, "live").into_iter().map(json).collect();
                    let lv: Vec<(&str, bool)> = live.iter().map(|n| (n.as_str(), true)).collect();
                    match ref_name_check(&json(arg(c, "name").unwrap()), &lv) {
                        Ok(()) => "ok".into(),
                        Err(r) => format!("refused {r}"),
                    }
                }
                f => panic!("{}: unknown function {f}", c.id),
            };
            if want.starts_with('"') {
                assert_eq!(got, format!("\"{}\"", json(want)), "{}", c.id);
            } else {
                assert_eq!(got, want, "{}", c.id);
            }
        }
        assert_eq!(
            not_modelled,
            ["p3-01", "p3-03", "p3-06"],
            "the P3 cases that need git's precomposition"
        );
    }

    #[test]
    fn globs_match_by_segments() {
        assert!(glob_match("src/**/*.rs", "src/a/b/c.rs"));
        assert!(glob_match("src/**/*.rs", "src/c.rs"));
        assert!(!glob_match("src/*.rs", "src/a/c.rs"));
        assert!(glob_match("docs/[a-c]?.md", "docs/b1.md"));
        assert!(!glob_match("docs/[!a-c]?.md", "docs/b1.md"));
        assert!(glob_match("**", "any/thing"));
    }

    #[test]
    fn display_escapes_ill_formed_bytes() {
        assert_eq!(display_name(b"a\xffb"), "a\\xffb");
        assert_eq!(display_name("é".as_bytes()), "é");
        assert_eq!(dir_prefix("docs/"), Ok(()));
        assert_eq!(dir_prefix("docs"), Err(PathError::Empty));
        assert_eq!(stored_rel_path(""), Err(PathError::Empty));
    }
}
