//! Generated work trees for the ignore differential (`tests/ignore_git.rs`): directories and files with realistic and
//! awkward names, and `.gitignore` files, `info/exclude` and a `core.excludesFile` whose patterns are mostly derived
//! from the tree's own paths, so that they match often, and then edited towards git's corners.
//!
//! **Names** are valid on every target and distinct under case folding within a directory ([`sanitize`], and the
//! dedup in [`build`]), so a tree is the same on a case-insensitive file system; they include brackets, `!`, `#`,
//! spaces, a leading dot, upper case and non-ASCII letters. **Virtual files** (`Tree::virtual_files`, asked about but
//! never written) are made from the tree's names with a `*` or `?` inserted or a trailing space or dot appended, in
//! the same directory, wherever `Tree::virtual_file_problem` allows.
//!
//! **Patterns** ([`PatternSpec`]) take a target path of the tree (virtual files included), relative to the source's
//! directory (its last component when it lies elsewhere), in one of the [`Form`]s — basename, relative, anchored
//! (`/`), `**/` before, `/**` after, `/**/` between, a `*` component, a `**` glued to a name (`a**/b`, which is no
//! `**` component), the parent directory, a trailing `*` — then apply up to two [`Edit`]s to its name bytes: `?`,
//! `*`, bracket expressions (sets, complements with `!` and `^`, ranges in both cases, classes, an unknown class, an
//! unclosed bracket, a member in the other case, an escaped member), a `\` escape, a case flip (case variants: both
//! `core.ignorecase` values are checked), a deleted or doubled byte. A literal pattern escapes every name byte that
//! gitignore reads specially (`*`, `?`, `[`, a space, `!`, `#`), so that it matches the target's bytes. A pattern may
//! be negated, directory-only and followed by trailing spaces, an escaped space, a tab or a lone `\`.
//!
//! **Lines** ([`LineSpec`]) are patterns, noise built from glob tokens, comments, blank lines, and patterns cut by a
//! `00` byte; a source ends its lines in LF, CR LF or both, may start with a UTF-8 BOM and may lack a final line
//! end.

use moirai_replay::ignorediff::{Tree, WINDOWS_DEVICE_NAMES};
use proptest::prelude::*;
use proptest::sample::Index;

/// Names that real trees have, and names with the bytes gitignore treats specially.
const POOL: &[&str] = &[
    "a",
    "b",
    "ab",
    "ba",
    "A",
    "B",
    "Ab",
    "aB",
    "a.log",
    "b.log",
    "keep.log",
    "X.LOG",
    "x.tmp",
    "y.tmp",
    "q.bak",
    "a.b.c",
    ".hidden",
    ".a",
    "build",
    "Build",
    "out",
    "src",
    "deep",
    "node_modules",
    "target",
    "Makefile",
    "README.md",
    "x y",
    "[a]",
    "a[b]",
    "[ab]",
    "#c",
    "!x",
    "!",
    "#",
    "é",
    "É",
    "ä.txt",
    "Ä",
    "-a",
    "{a}",
    "a,b",
    "a'b",
    "a=b",
    "$x",
    "%x",
    "^a",
    "a&b",
    "@a",
    "a;b",
    "a+b",
];

/// `name` made a valid file name on every target: no byte Windows forbids (`\ / : * ? " < > |` and controls), no
/// leading space, no trailing `.` or space, no `~` before a digit (which can alias an 8.3 short name), not `.`, `..`,
/// `.git`, `.gitignore` or a reserved device name, compared without case.
pub fn sanitize(name: &str) -> String {
    let mut s = String::with_capacity(name.len() + 2);
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
            s.push('x');
        } else {
            s.push(c);
            if c == '~' && chars.peek().is_some_and(char::is_ascii_digit) {
                s.push('x');
            }
        }
    }
    if s.is_empty() || s.starts_with(' ') {
        s.insert(0, 'a');
    }
    if s.ends_with(['.', ' ']) {
        s.push('a');
    }
    let lower = s.to_lowercase();
    let stem = lower.split('.').next().unwrap_or_default();
    if matches!(lower.as_str(), ".git" | ".gitignore") || WINDOWS_DEVICE_NAMES.contains(&stem) {
        s.insert(0, 'x');
    }
    s
}

/// The key under which two names are the same file on a case-insensitive file system.
pub fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// A file or directory name.
pub fn name() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => prop::sample::select(POOL).prop_map(str::to_string),
        3 => "[abAB][abAB.]{0,3}",
        1 => "[abx!#\\[\\] é.-]{1,4}",
    ]
    .prop_map(|n| sanitize(&n))
}

/// How a pattern is cut from its target path (module documentation).
#[derive(Clone, Copy, Debug)]
pub enum Form {
    Basename,
    Rel,
    Anchored,
    AnyDepth,
    Inside,
    Between,
    StarParent,
    Glued,
    Parent,
    TrailingStar,
}

/// A byte-level edit of a pattern's name bytes (module documentation).
#[derive(Clone, Copy, Debug)]
pub enum Edit {
    Question,
    Star,
    StarRun,
    Bracket(u8),
    Escape,
    FlipCase,
    Delete,
    Double,
}

/// One pattern line before rendering.
#[derive(Clone, Debug)]
pub struct PatternSpec {
    target: Index,
    form: Form,
    edits: Vec<(Edit, Index)>,
    negate: bool,
    dir_only: bool,
    literal: bool,
    tail: u8,
}

/// One line of a source before rendering.
#[derive(Clone, Debug)]
pub enum LineSpec {
    Pattern(PatternSpec),
    Noise(Vec<Index>),
    Comment(Vec<Index>),
    Blank(u8),
    Nul(PatternSpec, Index),
}

/// One pattern source before rendering.
#[derive(Clone, Debug)]
pub struct SourceSpec {
    lines: Vec<LineSpec>,
    eol: u8,
    bom: bool,
    final_eol: bool,
}

/// One tree before building ([`build`]).
#[derive(Clone, Debug)]
pub struct TreeSpec {
    dirs: Vec<(Index, String)>,
    files: Vec<(Index, String)>,
    virtuals: Vec<(Index, Index, u8)>,
    root: Option<SourceSpec>,
    nested: Vec<(Index, SourceSpec)>,
    info: Option<SourceSpec>,
    excludes: Option<SourceSpec>,
}

fn form() -> impl Strategy<Value = Form> {
    prop_oneof![
        4 => Just(Form::Basename),
        4 => Just(Form::Rel),
        2 => Just(Form::Anchored),
        2 => Just(Form::AnyDepth),
        2 => Just(Form::Inside),
        2 => Just(Form::Between),
        1 => Just(Form::StarParent),
        1 => Just(Form::Glued),
        2 => Just(Form::Parent),
        1 => Just(Form::TrailingStar),
    ]
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        2 => Just(Edit::Question),
        2 => Just(Edit::Star),
        1 => Just(Edit::StarRun),
        4 => (0u8..16).prop_map(Edit::Bracket),
        2 => Just(Edit::Escape),
        4 => Just(Edit::FlipCase),
        1 => Just(Edit::Delete),
        1 => Just(Edit::Double),
    ]
}

fn pattern() -> impl Strategy<Value = PatternSpec> {
    (
        any::<Index>(),
        form(),
        prop::collection::vec((edit(), any::<Index>()), 0..3),
        prop::bool::weighted(0.25),
        prop::bool::weighted(0.25),
        prop::bool::weighted(0.3),
        prop_oneof![10 => Just(0u8), 1 => 1u8..8],
    )
        .prop_map(
            |(target, form, edits, negate, dir_only, literal, tail)| PatternSpec {
                target,
                form,
                edits,
                negate,
                dir_only,
                literal,
                tail,
            },
        )
}

fn line() -> impl Strategy<Value = LineSpec> {
    prop_oneof![
        10 => pattern().prop_map(LineSpec::Pattern),
        1 => prop::collection::vec(any::<Index>(), 1..6).prop_map(LineSpec::Noise),
        1 => prop::collection::vec(any::<Index>(), 0..3).prop_map(LineSpec::Comment),
        1 => (0u8..4).prop_map(LineSpec::Blank),
        1 => (pattern(), any::<Index>()).prop_map(|(p, at)| LineSpec::Nul(p, at)),
    ]
}

fn source(lines: std::ops::Range<usize>) -> impl Strategy<Value = SourceSpec> {
    (
        prop::collection::vec(line(), lines),
        prop_oneof![3 => Just(0u8), 1 => Just(1u8), 1 => Just(2u8)],
        prop::bool::weighted(0.15),
        prop::bool::weighted(0.85),
    )
        .prop_map(|(lines, eol, bom, final_eol)| SourceSpec {
            lines,
            eol,
            bom,
            final_eol,
        })
}

/// A tree: up to 6 directories (at most 3 levels), 1 to 9 files, up to 3 virtual files, a root `.gitignore` in most
/// trees, up to 3 nested ones, and `info/exclude` and `core.excludesFile` in about a third of the trees each.
pub fn tree_spec() -> impl Strategy<Value = TreeSpec> {
    (
        prop::collection::vec((any::<Index>(), name()), 0..7),
        prop::collection::vec((any::<Index>(), name()), 1..10),
        prop::collection::vec((any::<Index>(), any::<Index>(), 0u8..6), 0..4),
        prop::option::weighted(0.85, source(1..8)),
        prop::collection::vec((any::<Index>(), source(1..6)), 0..4),
        prop::option::weighted(0.35, source(1..5)),
        prop::option::weighted(0.35, source(1..5)),
    )
        .prop_map(
            |(dirs, files, virtuals, root, nested, info, excludes)| TreeSpec {
                dirs,
                files,
                virtuals,
                root,
                nested,
                info,
                excludes,
            },
        )
}

/// A virtual file's name made from the real name `name` (module documentation): kind 0 inserts `*` and kind 1 `?`
/// before the character at `at` (at the end when `at` is past it); kinds 2 to 5 append a space, a dot, two spaces,
/// or a dot and a space.
fn virtual_name(name: &str, at: usize, kind: u8) -> String {
    match kind % 6 {
        k @ (0 | 1) => {
            let chars: Vec<char> = name.chars().collect();
            let at = at.min(chars.len());
            let mut s: String = chars[..at].iter().collect();
            s.push(if k == 0 { '*' } else { '?' });
            s.extend(&chars[at..]);
            s
        }
        k => format!("{name}{}", [" ", ".", "  ", ". "][usize::from(k - 2)]),
    }
}

/// `dir/name`, or `name` for the root.
fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// The tree a [`TreeSpec`] describes: directories under parents at most 2 levels deep, files in directories, each
/// name skipped when its directory already holds it under case folding; virtual files made from those names, each
/// skipped unless `Tree::virtual_file_problem` allows it and it is new; then the sources, rendered against the
/// tree's paths.
pub fn build(spec: &TreeSpec) -> Tree {
    let mut dirs: Vec<String> = vec![String::new()];
    let mut taken: Vec<(String, String)> = Vec::new();
    for (parent, name) in &spec.dirs {
        let parent = dirs[parent.index(dirs.len())].clone();
        if parent.split('/').count() >= 3 {
            continue;
        }
        let key = (parent.clone(), fold(name));
        if !taken.contains(&key) {
            taken.push(key);
            dirs.push(join(&parent, name));
        }
    }
    let mut files = Vec::new();
    for (dir, name) in &spec.files {
        let dir = dirs[dir.index(dirs.len())].clone();
        let key = (dir.clone(), fold(name));
        if !taken.contains(&key) {
            taken.push(key);
            files.push(join(&dir, name));
        }
    }
    let mut ignore_dirs: Vec<String> = Vec::new();
    if spec.root.is_some() {
        ignore_dirs.push(String::new());
    }
    let mut nested = Vec::new();
    for (dir, src) in &spec.nested {
        let dir = dirs[dir.index(dirs.len())].clone();
        if !ignore_dirs.contains(&dir) {
            ignore_dirs.push(dir.clone());
            nested.push((dir, src));
        }
    }
    // The `.gitignore` files are paths of the tree before their contents exist.
    let mut tree = Tree {
        dirs: dirs[1..].to_vec(),
        files,
        gitignores: ignore_dirs
            .iter()
            .map(|d| (d.clone(), Vec::new()))
            .collect(),
        ..Tree::default()
    };
    let real: Vec<String> = tree.dirs.iter().chain(&tree.files).cloned().collect();
    for (target, at, kind) in spec.virtuals.iter().filter(|_| !real.is_empty()) {
        let path = &real[target.index(real.len())];
        let (parent, name) = match path.rfind('/') {
            Some(i) => (&path[..i], &path[i + 1..]),
            None => ("", path.as_str()),
        };
        let at = at.index(name.chars().count() + 1);
        let v = join(parent, &virtual_name(name, at, *kind));
        if !tree.virtual_files.contains(&v) && tree.virtual_file_problem(&v).is_none() {
            tree.virtual_files.push(v);
        }
    }
    let mut targets: Vec<String> = real;
    targets.extend(tree.virtual_files.iter().cloned());
    targets.extend(ignore_dirs.iter().map(|d| join(d, ".gitignore")));
    targets.sort();
    let mut gitignores = Vec::new();
    if let Some(src) = &spec.root {
        gitignores.push((String::new(), render_source(src, "", &targets)));
    }
    for (dir, src) in nested {
        let bytes = render_source(src, &dir, &targets);
        gitignores.push((dir, bytes));
    }
    tree.gitignores = gitignores;
    tree.info_exclude = spec.info.as_ref().map(|s| render_source(s, "", &targets));
    tree.excludes_file = spec
        .excludes
        .as_ref()
        .map(|s| render_source(s, "", &targets));
    tree
}

fn render_source(src: &SourceSpec, dir: &str, targets: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    if src.bom {
        out.extend_from_slice(b"\xef\xbb\xbf");
    }
    for (i, line) in src.lines.iter().enumerate() {
        out.extend_from_slice(&render_line(line, dir, targets));
        if i + 1 < src.lines.len() || src.final_eol {
            let crlf = src.eol == 1 || (src.eol == 2 && i % 2 == 0);
            out.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
        }
    }
    out
}

/// Glob and literal pieces that noise and comment lines are made of.
const NOISE: &[&str] = &[
    "a",
    "b",
    "A",
    "*",
    "**",
    "?",
    "/",
    "[ab]",
    "[!a]",
    "[a-b]",
    "[[:alpha:]]",
    "\\a",
    "[",
    "]",
    "\\",
    "!",
    "#",
    " ",
    ".",
    "é",
    "**/",
    "/**",
    "\\*",
    "[]",
    "[]a]",
];

fn render_line(line: &LineSpec, dir: &str, targets: &[String]) -> Vec<u8> {
    match line {
        LineSpec::Pattern(p) => render_pattern(p, dir, targets).into_bytes(),
        LineSpec::Noise(pieces) => pieces
            .iter()
            .map(|i| NOISE[i.index(NOISE.len())])
            .collect::<String>()
            .into_bytes(),
        LineSpec::Comment(pieces) => {
            let mut s = String::from("#");
            s.extend(pieces.iter().map(|i| NOISE[i.index(NOISE.len())]));
            s.into_bytes()
        }
        LineSpec::Blank(k) => [&b""[..], b" ", b"\t", b"  "][usize::from(*k) % 4].to_vec(),
        LineSpec::Nul(p, at) => {
            let mut b = render_pattern(p, dir, targets).into_bytes();
            let at = at.index(b.len() + 1);
            b.splice(at..at, *b"\0x");
            b
        }
    }
}

/// A piece of a pattern: a name byte that edits may change, or fixed text.
#[derive(Clone, Debug)]
enum Tok {
    Lit(char),
    Raw(String),
}

fn lits(s: &str) -> impl Iterator<Item = Tok> + '_ {
    s.chars().map(Tok::Lit)
}

fn raw(s: &str) -> Tok {
    Tok::Raw(s.to_string())
}

/// `c` with its case flipped (ASCII letters, and the two non-ASCII pairs the names use); any other character as it
/// is.
pub fn flip(c: char) -> char {
    match c {
        'é' => 'É',
        'É' => 'é',
        'ä' => 'Ä',
        'Ä' => 'ä',
        c if c.is_ascii_lowercase() => c.to_ascii_uppercase(),
        c => c.to_ascii_lowercase(),
    }
}

/// A bracket expression built around the byte `c` (module documentation).
fn bracket(c: char, kind: u8) -> String {
    match kind % 16 {
        0 => format!("[{c}]"),
        1 => format!("[x{c}]"),
        2 => format!("[!{c}]"),
        3 => format!("[^{c}]"),
        4 => match c {
            'a'..='z' => "[a-z]".into(),
            'A'..='Z' => "[A-Z]".into(),
            '0'..='9' => "[0-9]".into(),
            _ => format!("[{c}-{c}]"),
        },
        5 => match c {
            'a'..='z' => "[A-Z]".into(),
            'A'..='Z' => "[a-z]".into(),
            _ => "[a-c]".into(),
        },
        6 => "[[:alpha:]]".into(),
        7 => "[[:upper:]]".into(),
        8 => "[[:lower:]]".into(),
        9 => "[[:punct:]]".into(),
        10 => "[[:digit:][:alpha:]]".into(),
        11 => "[[:nope:]]".into(),
        12 => format!("[{c}"),
        13 => format!("[{}]", flip(c)),
        14 => format!("[\\{c}]"),
        _ => format!("[{c}-]"),
    }
}

/// A pattern's text (module documentation).
fn render_pattern(p: &PatternSpec, dir: &str, targets: &[String]) -> String {
    let target = if targets.is_empty() {
        "a"
    } else {
        targets[p.target.index(targets.len())].as_str()
    };
    let rel = match target.strip_prefix(dir) {
        _ if dir.is_empty() => target,
        Some(rest) if rest.starts_with('/') => &rest[1..],
        _ => target.rsplit('/').next().unwrap_or(target),
    };
    let comps: Vec<&str> = rel.split('/').collect();
    let last = comps[comps.len() - 1];
    let first = comps[0];
    let deep = comps.len() >= 2;
    let mut toks: Vec<Tok> = Vec::new();
    match p.form {
        Form::Basename => toks.extend(lits(last)),
        Form::Rel => toks.extend(lits(rel)),
        Form::Anchored => {
            toks.push(raw("/"));
            toks.extend(lits(rel));
        }
        Form::AnyDepth => {
            toks.push(raw("**/"));
            toks.extend(lits(last));
        }
        Form::Inside => {
            toks.extend(lits(if deep { first } else { rel }));
            toks.push(raw("/**"));
        }
        Form::Between if deep => {
            toks.extend(lits(first));
            toks.push(raw("/**/"));
            toks.extend(lits(last));
        }
        Form::Between => {
            toks.push(raw("**/"));
            toks.extend(lits(last));
            toks.push(raw("/**"));
        }
        Form::StarParent => {
            for c in &comps[..comps.len().saturating_sub(2)] {
                toks.extend(lits(c));
                toks.push(raw("/"));
            }
            toks.push(raw("*/"));
            toks.extend(lits(last));
        }
        Form::Glued if deep => {
            toks.extend(lits(first));
            toks.push(raw("**/"));
            toks.extend(lits(last));
        }
        Form::Glued => {
            toks.extend(lits(last));
            toks.push(raw("**"));
        }
        Form::Parent if deep => toks.extend(lits(&rel[..rel.len() - last.len() - 1])),
        Form::Parent => toks.extend(lits(rel)),
        Form::TrailingStar => {
            toks.extend(lits(rel));
            toks.push(raw("*"));
        }
    }
    for (e, at) in &p.edits {
        let pos: Vec<usize> = toks
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t, Tok::Lit(c) if *c != '/'))
            .map(|(i, _)| i)
            .collect();
        if pos.is_empty() {
            break;
        }
        let i = pos[at.index(pos.len())];
        let Tok::Lit(c) = toks[i] else { continue };
        match e {
            Edit::Question => toks[i] = raw("?"),
            Edit::Star => toks[i] = raw("*"),
            Edit::StarRun => {
                let mut j = i;
                while matches!(toks.get(j), Some(Tok::Lit(c)) if *c != '/') {
                    j += 1;
                }
                toks.splice(i..j, [raw("*")]);
            }
            Edit::Bracket(k) => toks[i] = Tok::Raw(bracket(c, *k)),
            Edit::Escape => toks[i] = Tok::Raw(format!("\\{c}")),
            Edit::FlipCase => toks[i] = Tok::Lit(flip(c)),
            Edit::Delete => {
                toks.remove(i);
            }
            Edit::Double => toks.insert(i, Tok::Lit(c)),
        }
    }
    let mut s = String::new();
    if p.negate {
        s.push('!');
    }
    for t in &toks {
        match t {
            Tok::Lit(c) => {
                if p.literal && matches!(c, '*' | '?' | '[' | ' ' | '!' | '#') {
                    s.push('\\');
                }
                s.push(*c);
            }
            Tok::Raw(r) => s.push_str(r),
        }
    }
    if p.dir_only {
        s.push('/');
    }
    s.push_str(["", " ", "   ", "\\ ", "\\  ", "\t", "\\", " \t "][usize::from(p.tail) % 8]);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize("a."), "a.a");
        assert_eq!(sanitize(" a "), "a a a");
        assert_eq!(sanitize("x~1"), "x~x1");
        assert_eq!(sanitize(".GIT"), "x.GIT");
        assert_eq!(sanitize("Nul.txt"), "xNul.txt");
        assert_eq!(sanitize("a*b?"), "axbx");
        assert_eq!(sanitize(""), "a");
    }

    #[test]
    fn virtual_names_hold_a_wildcard_or_trailing_spaces_and_dots() {
        assert_eq!(virtual_name("é.b", 0, 0), "*é.b");
        assert_eq!(virtual_name("é.b", 1, 1), "é?.b");
        assert_eq!(virtual_name("é.b", 9, 0), "é.b*");
        assert_eq!(virtual_name("a", 0, 2), "a ");
        assert_eq!(virtual_name("a", 0, 3), "a.");
        assert_eq!(virtual_name("a", 0, 4), "a  ");
        assert_eq!(virtual_name("a", 0, 5), "a. ");
    }
}
