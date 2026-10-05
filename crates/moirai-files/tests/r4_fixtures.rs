//! WP-61 and WP-62: conformance with the golden fixtures of R4's pure functions, `fixtures/r4/` (format:
//! `fixtures/r4/INDEX.md` §2), and with the derived uids in use in `fixtures/canonical/` (`anchors.cases`,
//! `checkpoint.cases`; format: `fixtures/canonical/INDEX.md` §3).
//!
//! What runs here, against this crate's functions:
//!
//! | Fixture | Function | Spec |
//! |---|---|---|
//! | `fold.cases` (all 37) | [`fold_v1`], [`ceq`] | [F20 §3.1], §3.4 |
//! | `derivations.cases` (all 21) | [`uid_file`], [`uid_root`], [`captured`], [`uid_anchor`], with their hashed bytes | [F08 §11.1]–§11.4 |
//! | `predecessor.cases` (all 15) | [`derive_file_uid`] (registration, steps 2–5), [`captured`] and [`derive_anchor_uid`] (capture, steps 2–4) | [F08 §11.2], §11.4 |
//! | `paths.cases` P5 | [`portable_issues`], [`representable`] | [OS/path §8.1], §8.2 |
//! | `paths.cases` P8 | [`blob_oid`] over the target text | [F20 §2.3], [OS/path §3] P8 |
//! | `paths.cases` P11 (b) | [`is_device_name`]: the RN-4 decision of every case that reaches RN-4 | [F12 §2.4], [OS/path §3] P11 (b) |
//! | `canonical/` | the file, root and anchor uids and the `captured` digests of the states | [F08 §11.2]–§11.5 |
//!
//! The parts of a procedure that read a view — step 1 of a registration (the node that already holds the path), the
//! candidate set of step 2, step 1 of a capture (de-duplication by current selectors) and the lookups of the loops —
//! are the caller's ([`moirai_files::uid`]); the harness runs them over the fixture's stated view exactly as
//! [F08 §11.2] and §11.4 word them, and the product supplies every derivation, the predecessor order and the loops.
//!
//! The `paths.cases` functions whose home is another crate ([OS/path §1]) are listed in [`ELSEWHERE`] with that home;
//! a function the fixtures add later fails [`every_paths_case_is_run_or_placed`] until it is placed.
//!
//! The files are compiled in (`include_str!`): a product crate's tests open no file ([OS/README §2.5], PLAN §6.2 R18).
//! Every failing check of a file is collected and reported together, each with its case id and line.

use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::str::CharIndices;

use moirai_files::fold::{ceq, fold_eq, fold_matches, fold_v1};
use moirai_files::oid::{ObjectFormat, blob_oid};
use moirai_files::path::{Os, is_device_name, portable_issues, representable};
use moirai_files::uid::{
    AnchorKind, Capture, Uid, UidError, captured, derive_anchor_uid, derive_file_uid, uid_anchor,
    uid_file, uid_root,
};

const FOLD: &str = include_str!("../../../fixtures/r4/cases/fold.cases");
const DERIVATIONS: &str = include_str!("../../../fixtures/r4/cases/derivations.cases");
const PREDECESSOR: &str = include_str!("../../../fixtures/r4/cases/predecessor.cases");
const PATHS: &str = include_str!("../../../fixtures/r4/cases/paths.cases");
const CANONICAL_ANCHORS: &str = include_str!("../../../fixtures/canonical/cases/anchors.cases");
const CANONICAL_CHECKPOINT: &str =
    include_str!("../../../fixtures/canonical/cases/checkpoint.cases");

// --- the case format (fixtures/r4/INDEX.md §2.1, fixtures/lq/INDEX.md §2.1) ----------------------------------

/// One case: its line directives (`%% <name> <value>`) and block directives (`%% <name>` and the lines below it).
struct Case {
    file: &'static str,
    id: &'static str,
    line: usize,
    lines: Vec<(&'static str, &'static str)>,
    blocks: Vec<(&'static str, Vec<&'static str>)>,
}

impl Case {
    /// `file:line id`, for messages.
    fn at(&self) -> String {
        format!("{}:{} {}", self.file, self.line, self.id)
    }

    fn all(&self, name: &str) -> Vec<&'static str> {
        self.lines
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| *v)
            .collect()
    }

    fn opt(&self, name: &str) -> Option<&'static str> {
        let v = self.all(name);
        assert!(
            v.len() <= 1,
            "{}: `{name}` given {} times",
            self.at(),
            v.len()
        );
        v.first().copied()
    }

    fn one(&self, name: &str) -> &'static str {
        self.opt(name)
            .unwrap_or_else(|| panic!("{}: no `{name}` directive", self.at()))
    }

    fn block(&self, name: &str) -> Option<&[&'static str]> {
        self.blocks
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_slice())
    }

    fn function(&self) -> &'static str {
        self.one("function")
    }

    /// The values of the repeatable `arg <name> <value>` lines named `name`.
    fn args(&self, name: &str) -> Vec<&'static str> {
        self.all("arg")
            .into_iter()
            .filter_map(|a| {
                let (n, v) = a.split_once(' ').unwrap_or((a, ""));
                (n == name).then_some(v)
            })
            .collect()
    }

    fn arg(&self, name: &str) -> &'static str {
        let v = self.args(name);
        assert_eq!(
            v.len(),
            1,
            "{}: `arg {name}` given {} times",
            self.at(),
            v.len()
        );
        v[0]
    }

    /// An argument written `-` when absent ([INDEX §2.3]).
    fn opt_arg(&self, name: &str) -> Option<&'static str> {
        Some(self.arg(name)).filter(|v| *v != "-")
    }
}

/// The cases of a `.cases` file. Lines outside a case are comments; inside a block, lines starting with `;` are
/// comments ([INDEX §2.3]: `input-hex`; the views' `; (empty view)`). Block lines keep their indentation.
fn parse(file: &'static str, text: &'static str) -> Vec<Case> {
    let mut cases = Vec::new();
    let mut cur: Option<Case> = None;
    let mut in_block = false;
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        match (&mut cur, line.strip_prefix("%% ")) {
            (None, Some(rest)) => {
                let id = rest
                    .strip_prefix("case ")
                    .unwrap_or_else(|| panic!("{file}:{n}: a directive outside a case"));
                cur = Some(Case {
                    file,
                    id,
                    line: n,
                    lines: Vec::new(),
                    blocks: Vec::new(),
                });
                in_block = false;
            }
            (None, None) => {}
            (Some(_), Some("end")) => {
                cases.extend(cur.take());
            }
            (Some(c), Some(rest)) => {
                assert!(
                    !rest.starts_with("case "),
                    "{file}:{n}: a case inside a case"
                );
                match rest.split_once(' ') {
                    Some((name, value)) => {
                        c.lines.push((name, value));
                        in_block = false;
                    }
                    None => {
                        c.blocks.push((rest, Vec::new()));
                        in_block = true;
                    }
                }
            }
            (Some(c), None) => {
                if line.starts_with(';') || line.trim().is_empty() {
                    continue;
                }
                assert!(in_block, "{file}:{n}: a line outside any block: {line:?}");
                if let Some((_, v)) = c.blocks.last_mut() {
                    v.push(line);
                }
            }
        }
    }
    assert!(cur.is_none(), "{file}: the last case has no `%% end`");
    cases
}

/// Splits a line at spaces outside JSON strings: `key="a b"` and `"x","y"` stay one token each.
fn tokens(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut start, mut in_str) = (0, None, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
        } else if c == b' ' {
            if let Some(st) = start.take() {
                out.push(&s[st..i]);
            }
        } else {
            start.get_or_insert(i);
            in_str = c == b'"';
        }
        i += 1;
    }
    assert!(!in_str, "an unterminated string in {s:?}");
    out.extend(start.map(|st| &s[st..]));
    out
}

/// `key=value` tokens as pairs (the value undecoded); other tokens are refused.
fn attrs<'a>(toks: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    toks.iter()
        .map(|t| {
            t.split_once('=')
                .unwrap_or_else(|| panic!("not an attribute: {t:?}"))
        })
        .collect()
}

fn attr<'a>(a: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    a.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn hex4(chars: &mut CharIndices<'_>) -> u32 {
    (0..4).fold(0, |acc, _| {
        let d = chars.next().and_then(|(_, c)| c.to_digit(16));
        acc * 16 + d.expect("four hexadecimal digits after \\u")
    })
}

/// The JSON string (RFC 8259) at the start of `s`, decoded, and the rest of `s`.
fn json_prefix(s: &str) -> (String, &str) {
    let mut chars = s.char_indices();
    assert_eq!(chars.next().map(|c| c.1), Some('"'), "a JSON string: {s:?}");
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return (out, &s[i + 1..]),
            '\\' => {
                let e = chars.next().expect("an escape").1;
                let ch = match e {
                    '"' | '\\' | '/' => e,
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let hi = hex4(&mut chars);
                        let cp = if (0xD800..0xDC00).contains(&hi) {
                            let esc: String = chars.by_ref().take(2).map(|c| c.1).collect();
                            assert_eq!(
                                esc, "\\u",
                                "a high surrogate without its low half in {s:?}"
                            );
                            let lo = hex4(&mut chars);
                            assert!(
                                (0xDC00..0xE000).contains(&lo),
                                "not a low surrogate in {s:?}"
                            );
                            0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                        } else {
                            hi
                        };
                        char::from_u32(cp).unwrap_or_else(|| panic!("a lone surrogate in {s:?}"))
                    }
                    _ => panic!("a bad escape in {s:?}"),
                };
                out.push(ch);
            }
            c if u32::from(c) < 0x20 => panic!("a control character in the JSON string {s:?}"),
            c => out.push(c),
        }
    }
    panic!("an unterminated JSON string: {s:?}")
}

/// A token that is exactly one JSON string.
fn json(s: &str) -> String {
    let (v, rest) = json_prefix(s);
    assert!(rest.is_empty(), "text after the JSON string {s:?}");
    v
}

/// `"a","b",…`: one or more JSON strings separated by commas.
fn json_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        let (v, r) = json_prefix(rest);
        out.push(v);
        match r.strip_prefix(',') {
            Some(r) => rest = r,
            None => {
                assert!(r.is_empty(), "text after the JSON list {s:?}");
                return out;
            }
        }
    }
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(
        s.len().is_multiple_of(2) && s.bytes().all(|b| b.is_ascii_hexdigit()),
        "not hexadecimal bytes: {s:?}"
    );
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn b16(s: &str) -> [u8; 16] {
    unhex(s)
        .try_into()
        .unwrap_or_else(|_| panic!("not 16 bytes: {s:?}"))
}

fn uid(s: &str) -> Uid {
    Uid(b16(s))
}

fn opt_uid(u: Option<&Uid>) -> String {
    u.map_or_else(|| "-".to_owned(), Uid::to_string)
}

/// BLAKE3-128 ([F01 §7.1]): the first 16 bytes of BLAKE3.
fn b3_128(bytes: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&blake3::hash(bytes).as_bytes()[..16]);
    out
}

/// `U+0050 U+006C …` as a string.
fn cps(s: &str) -> String {
    s.split_whitespace()
        .map(|t| {
            let h = t
                .strip_prefix("U+")
                .unwrap_or_else(|| panic!("not a code point: {t:?}"));
            char::from_u32(u32::from_str_radix(h, 16).expect("hex")).expect("a scalar value")
        })
        .collect()
}

const KINDS: [AnchorKind; 6] = [
    AnchorKind::File,
    AnchorKind::Heading,
    AnchorKind::Symbol,
    AnchorKind::Quote,
    AnchorKind::Range,
    AnchorKind::Lines,
];

fn kind(name: &str) -> AnchorKind {
    KINDS
        .into_iter()
        .find(|k| k.name() == name)
        .unwrap_or_else(|| panic!("not an anchor kind: {name:?}"))
}

/// The failing checks of one file, reported together.
struct Report(Vec<String>);

impl Report {
    fn new() -> Report {
        Report(Vec::new())
    }

    fn check(&mut self, c: &Case, ok: bool, what: impl FnOnce() -> String) {
        if !ok {
            self.0.push(format!("{}: {}", c.at(), what()));
        }
    }

    fn finish(self) {
        assert!(
            self.0.is_empty(),
            "{} failing checks:\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

/// The number of cases per function, sorted by function name.
fn tally<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<(&'a str, usize)> {
    let mut v: Vec<(&str, usize)> = Vec::new();
    for n in names {
        match v.iter_mut().find(|(m, _)| *m == n) {
            Some((_, k)) => *k += 1,
            None => v.push((n, 1)),
        }
    }
    v.sort_unstable();
    v
}

// --- fold.cases: fold_v1 and ceq ([F20 §3.1], §3.4) -----------------------------------------------------------

#[test]
fn fold_cases() {
    let cases = parse("fold.cases", FOLD);
    assert_eq!(
        tally(cases.iter().map(Case::function)),
        [("ceq", 5), ("fold_v1", 32)],
        "INDEX.md §1: 37 cases"
    );
    let mut r = Report::new();
    for c in &cases {
        let x = json(c.one("input-text"));
        match c.function() {
            "fold_v1" => {
                // The fixture's three spellings of the input and its two of the output agree.
                r.check(c, cps(c.one("input-cps")) == x, || {
                    "input-cps differs from input-text".into()
                });
                r.check(c, unhex(c.one("input-utf8")) == x.as_bytes(), || {
                    "input-utf8 differs from input-text".into()
                });
                let want = String::from_utf8(unhex(c.one("output-utf8"))).expect("UTF-8");
                r.check(c, cps(c.one("output-cps")) == want, || {
                    "output-cps differs from output-utf8".into()
                });
                let got = fold_v1(&x);
                r.check(c, got == want, || {
                    format!(
                        "fold_v1 gave {}, expected {}",
                        hex(got.as_bytes()),
                        hex(want.as_bytes())
                    )
                });
                r.check(c, fold_eq(&x, &want) && fold_matches(&got, &x), || {
                    "the fold predicates disagree with fold_v1".into()
                });
            }
            "ceq" => {
                let y = json(c.one("input2-text"));
                let want = match c.one("output") {
                    "true" => true,
                    "false" => false,
                    o => panic!("{}: output {o:?}", c.at()),
                };
                r.check(c, ceq(&x, &y) == want && ceq(&y, &x) == want, || {
                    format!("ceq is not {want}")
                });
            }
            f => panic!("{}: unknown function {f:?}", c.at()),
        }
    }
    r.finish();
}

// --- derivations.cases: uid_file, uid_root, captured, uid_anchor ([F08 §11]) ---------------------------------

/// The text argument `name`, absent (`-`) as empty.
fn text_arg(c: &Case, name: &str) -> Vec<u8> {
    c.opt_arg(name)
        .map(|v| json(v).into_bytes())
        .unwrap_or_default()
}

#[test]
fn derivation_cases() {
    let cases = parse("derivations.cases", DERIVATIONS);
    assert_eq!(
        tally(cases.iter().map(Case::function)),
        [
            ("captured", 8),
            ("uid_anchor", 3),
            ("uid_file", 7),
            ("uid_root", 3)
        ],
        "INDEX.md §1: 21 cases"
    );
    let mut r = Report::new();
    for c in &cases {
        let want = b16(c.one("output"));
        // The fixture's hashed bytes: their stated length and their digest.
        let input: Vec<u8> = c
            .block("input-hex")
            .unwrap_or_else(|| panic!("{}: no input-hex", c.at()))
            .iter()
            .flat_map(|l| unhex(l.trim()))
            .collect();
        let len: usize = c.one("input-length").parse().expect("a length");
        r.check(c, input.len() == len, || {
            format!(
                "input-hex has {} bytes, input-length says {len}",
                input.len()
            )
        });
        r.check(c, b3_128(&input) == want, || {
            "BLAKE3-128 of input-hex is not the output".into()
        });
        let got: Result<[u8; 16], UidError> = match c.function() {
            "uid_file" => uid_file(
                &json(c.arg("root")),
                &json(c.arg("path")),
                c.opt_arg("pred").map(uid).as_ref(),
            )
            .map(|u| u.0),
            "uid_root" => uid_root(&json(c.arg("root"))).map(|u| u.0),
            "captured" => {
                let file = uid(c.arg("file_uid"));
                let scope = c.opt_arg("scope").map(unhex).unwrap_or_default();
                let window = c.opt_arg("window").map(unhex).unwrap_or_default();
                let (quote, prefix, suffix, end) = (
                    text_arg(c, "quote"),
                    text_arg(c, "prefix"),
                    text_arg(c, "suffix"),
                    text_arg(c, "end"),
                );
                captured(&Capture {
                    file_uid: &file,
                    kind: kind(c.arg("kind")),
                    scope: &scope,
                    quote: &quote,
                    prefix: &prefix,
                    suffix: &suffix,
                    end: &end,
                    occurrence: c
                        .opt_arg("occurrence")
                        .map(|o| NonZeroU16::new(o.parse().expect("a u16")).expect("≥ 1")),
                    window: &window,
                })
            }
            "uid_anchor" => uid_anchor(
                &uid(c.arg("src")),
                &b16(c.arg("captured")),
                c.opt_arg("pred").map(uid).as_ref(),
            )
            .map(|u| u.0),
            f => panic!("{}: unknown function {f:?}", c.at()),
        };
        r.check(c, got == Ok(want), || {
            format!(
                "{} gave {:?}, expected {}",
                c.function(),
                got.map(|g| hex(&g)),
                hex(&want)
            )
        });
    }
    r.finish();
}

// --- predecessor.cases: registration and capture over a stated view ([F08 §11.2], §11.4) ---------------------

/// A node of a `register` view ([INDEX §2.4]).
struct ViewNode {
    uid: Uid,
    /// For a live artifact (`file`): root, path, status and aliases. Tombstones and other nodes only have a uid
    /// that the dead-uid loop can name.
    file: Option<(String, String, String, Vec<String>)>,
}

fn register_view(c: &Case) -> Vec<ViewNode> {
    c.block("view")
        .unwrap_or_else(|| panic!("{}: no view", c.at()))
        .iter()
        .map(|l| {
            let t = tokens(l);
            let a = attrs(&t[2..]);
            let file = match t[0] {
                "file" => Some((
                    attr(&a, "root").expect("root").to_owned(),
                    json(attr(&a, "path").expect("path")),
                    attr(&a, "status").expect("status").to_owned(),
                    attr(&a, "aliases").map(json_list).unwrap_or_default(),
                )),
                "tombstone" | "node" => None,
                k => panic!("{}: a view row {k:?}", c.at()),
            };
            ViewNode {
                uid: uid(t[1]),
                file,
            }
        })
        .collect()
}

/// A registration of (r, p) on view V ([F08 §11.2]): step 1 and step 2's candidate set read V as the section words
/// them; the product supplies the predecessor order, the derivation and the dead-uid loop (steps 2–4) and the
/// node's `origin_pred` (step 5).
fn register(view: &[ViewNode], r: &str, p: &str) -> Result<String, UidError> {
    let holder = view.iter().find(|n| {
        n.file.as_ref().is_some_and(|(root, path, status, _)| {
            root == r && path == p && (status == "present" || status == "planned")
        })
    });
    if let Some(n) = holder {
        return Ok(format!("existing {}", n.uid));
    }
    let candidates: Vec<Uid> = view
        .iter()
        .filter(|n| {
            n.file
                .as_ref()
                .is_some_and(|(root, path, status, aliases)| {
                    root == r
                        && ((status == "removed" && path == p) || aliases.iter().any(|a| a == p))
                })
        })
        .map(|n| n.uid)
        .collect();
    // Every node of V can be named by the loop, tombstones included (step 4's bound).
    let k = derive_file_uid(
        r,
        p,
        &candidates,
        |u| view.iter().any(|n| n.uid == *u),
        view.len() as u64,
    )?;
    Ok(format!(
        "new {} pred {}",
        k.uid,
        opt_uid(k.origin_pred.as_ref())
    ))
}

/// The current selectors of an anchor ([F08 §11.4] step 1): `kind`, `scope`, `quote`, `prefix`, `suffix`, `end`,
/// `occurrence` and, for `lines`, `window`.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Selectors {
    kind: AnchorKind,
    scope: Vec<u8>,
    quote: Option<String>,
    prefix: Option<String>,
    suffix: Option<String>,
    end: Option<String>,
    occurrence: Option<NonZeroU16>,
    window: Option<Vec<u8>>,
}

impl Selectors {
    fn from_attrs(a: &[(&str, &str)]) -> Selectors {
        let k = kind(attr(a, "kind").expect("kind"));
        let text = |key: &str| attr(a, key).map(json);
        Selectors {
            kind: k,
            scope: attr(a, "scope").map(unhex).unwrap_or_default(),
            quote: text("quote"),
            prefix: text("prefix"),
            suffix: text("suffix"),
            end: text("end"),
            occurrence: attr(a, "occurrence")
                .map(|o| NonZeroU16::new(o.parse().expect("a u16")).expect("≥ 1")),
            window: attr(a, "window")
                .filter(|_| k == AnchorKind::Lines)
                .map(unhex),
        }
    }

    /// Whether the record holds its texts (not `text_unavailable`): `captured` needs them.
    fn has_texts(&self) -> bool {
        match self.kind {
            AnchorKind::File | AnchorKind::Lines => true,
            _ => self.quote.is_some(),
        }
    }

    /// `captured` of these selectors on file node `file` ([F08 §11.4]).
    fn captured(&self, file: &Uid) -> Result<[u8; 16], UidError> {
        fn b(t: &Option<String>) -> &[u8] {
            t.as_deref().unwrap_or("").as_bytes()
        }
        captured(&Capture {
            file_uid: file,
            kind: self.kind,
            scope: &self.scope,
            quote: b(&self.quote),
            prefix: b(&self.prefix),
            suffix: b(&self.suffix),
            end: b(&self.end),
            occurrence: self.occurrence,
            window: self.window.as_deref().unwrap_or(&[]),
        })
    }
}

/// An anchor of a `capture` view.
struct ViewAnchor {
    uid: Uid,
    src: Uid,
    dst: Uid,
    selectors: Selectors,
}

/// A capture of an anchor with selectors `sel` on the edge (s, `at`, f) of view V ([F08 §11.4]): step 1 compares
/// current selectors as the section words it; the product supplies `captured`, the derivation and the loop
/// (steps 2–4).
fn capture(view: &[ViewAnchor], s: Uid, f: Uid, sel: &Selectors) -> Result<String, UidError> {
    let on_edge: Vec<&ViewAnchor> = view.iter().filter(|a| a.src == s && a.dst == f).collect();
    if let Some(a) = on_edge.iter().find(|a| a.selectors == *sel) {
        return Ok(format!("reuse {}", a.uid));
    }
    let c = sel.captured(&f)?;
    let k = derive_anchor_uid(
        &s,
        &c,
        |u| on_edge.iter().any(|a| a.uid == *u),
        on_edge.len() as u64,
    )?;
    Ok(format!(
        "new {} captured {} pred {}",
        k.uid,
        hex(&c),
        opt_uid(k.pred.as_ref())
    ))
}

#[test]
fn predecessor_cases() {
    let cases = parse("predecessor.cases", PREDECESSOR);
    assert_eq!(
        tally(cases.iter().map(|c| c.one("operation"))),
        [("capture", 5), ("register", 10)],
        "INDEX.md §1: 15 cases"
    );
    let mut r = Report::new();
    for c in &cases {
        let got = match c.one("operation") {
            "register" => register(
                &register_view(c),
                &json(c.arg("root")),
                &json(c.arg("path")),
            ),
            "capture" => {
                let view: Vec<ViewAnchor> = c
                    .block("view")
                    .unwrap_or(&[])
                    .iter()
                    .map(|l| {
                        let t = tokens(l);
                        assert_eq!(t[0], "anchor", "{}: a view row {l:?}", c.at());
                        let a = attrs(&t[2..]);
                        let captured = b16(attr(&a, "captured").expect("captured"));
                        let pred = attr(&a, "pred").filter(|p| *p != "-").map(uid);
                        let u = uid(t[1]);
                        let src = uid(attr(&a, "src").expect("src"));
                        // The view's own anchors follow [F08 §11.4]: uid = uid_anchor(src, captured, pred).
                        assert_eq!(
                            uid_anchor(&src, &captured, pred.as_ref()),
                            Ok(u),
                            "{}: the view's anchor {u}",
                            c.at()
                        );
                        ViewAnchor {
                            uid: u,
                            src,
                            dst: uid(attr(&a, "dst").expect("dst")),
                            selectors: Selectors::from_attrs(&a),
                        }
                    })
                    .collect();
                let sel = Selectors::from_attrs(&attrs(&tokens(c.arg("selectors"))));
                capture(&view, uid(c.arg("src")), uid(c.arg("dst")), &sel)
            }
            o => panic!("{}: operation {o:?}", c.at()),
        };
        let want = c
            .one("result")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        r.check(c, got.as_deref() == Ok(want.as_str()), || {
            format!("gave {got:?}, expected {want:?}")
        });
    }
    r.finish();
}

// --- paths.cases --------------------------------------------------------------------------------------------

/// The `paths.cases` functions this file runs, with their case counts.
const RUN_HERE: [(&str, usize); 4] = [
    ("portable_issues", 22),
    ("ref_name_check", 27),
    ("representable", 3),
    ("symlink_oid", 3),
];

/// The `paths.cases` functions whose home is not this crate ([OS/path §1], §3), with their case counts and homes.
const ELSEWHERE: [(&str, usize, &str); 9] = [
    ("abspath", 19, "moirai-vfs `AbsPath` ([OS/path §2.2], P12)"),
    (
        "canonical_abs_lexical",
        7,
        "moirai-os `path::canonical_abs` ([OS/path §5])",
    ),
    (
        "cli_path",
        17,
        "moirai-os `path`, the CLI boundary ([OS/path §7])",
    ),
    (
        "lookup_tree",
        5,
        "moirai-os `path::canonical_root` lookups ([OS/path §4.4], P9)",
    ),
    ("origin_path", 3, "the link layer, M6 ([OS/path §3] P7)"),
    (
        "query_file_name",
        3,
        "the image layer ([F14 §7.2], P11 (a))",
    ),
    (
        "relpath",
        19,
        "moirai-vfs `RelPath` ([OS/path §2.1], P1, P4)",
    ),
    (
        "stored_untracked_name",
        6,
        "moirai-files in the port phase ([OS/path §3] P3: macOS; needs NFC, which no M0 table holds)",
    ),
    ("trees_key", 3, "moirai-os `path` keys ([OS/path §4.5], P9)"),
];

#[test]
fn every_paths_case_is_run_or_placed() {
    let cases = parse("paths.cases", PATHS);
    assert_eq!(cases.len(), 137, "INDEX.md §1: 137 cases");
    let mut want: Vec<(&str, usize)> = RUN_HERE.to_vec();
    want.extend(ELSEWHERE.iter().map(|e| (e.0, e.1)));
    want.sort_unstable();
    for (f, _, home) in ELSEWHERE {
        assert!(!home.is_empty(), "{f}: no home");
    }
    assert_eq!(tally(cases.iter().map(Case::function)), want);
    for c in &cases {
        assert!(
            c.one("rule").starts_with('P') || c.one("rule") == "CLI",
            "{}: rule {:?}",
            c.at(),
            c.one("rule")
        );
    }
}

/// `portable_issues`' result in the fixture's spelling ([INDEX §2.5]): the issues in [OS/path §8.2]'s order, each
/// with its detail (`reserved-char` its first such character, `fold-sibling` the smallest such sibling).
fn issues_of(segment: &str, siblings: &[String]) -> Vec<(String, Option<String>)> {
    let i = portable_issues(segment, siblings.iter().map(String::as_str));
    i.iter()
        .map(|issue| {
            let detail = match issue.name() {
                "reserved-char" => i.reserved_char.map(String::from),
                "fold-sibling" => i.fold_sibling.map(str::to_owned),
                _ => None,
            };
            (issue.name().to_owned(), detail)
        })
        .collect()
}

/// The `expect` of a `portable_issues` case: `none`, or `issue [<json>] , issue [<json>] …`.
fn expected_issues(c: &Case) -> Vec<(String, Option<String>)> {
    let e = c.one("expect");
    if e == "none" {
        return Vec::new();
    }
    let mut out = Vec::new();
    let t = tokens(e);
    let mut k = 0;
    while k < t.len() {
        let name = t[k].to_owned();
        k += 1;
        let detail = t.get(k).filter(|d| d.starts_with('"')).map(|d| json(d));
        k += usize::from(detail.is_some());
        out.push((name, detail));
        if k < t.len() {
            assert_eq!(t[k], ",", "{}: expect {e:?}", c.at());
            k += 1;
        }
    }
    out
}

#[test]
fn paths_p5_portable_issues() {
    let cases = parse("paths.cases", PATHS);
    let mut r = Report::new();
    let mut n = 0;
    for c in cases.iter().filter(|c| c.function() == "portable_issues") {
        n += 1;
        let segment = json(c.arg("segment"));
        let siblings: Vec<String> = c.args("sibling").into_iter().map(json).collect();
        let got = issues_of(&segment, &siblings);
        let want = expected_issues(c);
        r.check(c, got == want, || {
            format!("gave {got:?}, expected {want:?}")
        });
    }
    assert_eq!(n, 22);
    r.finish();
}

#[test]
fn paths_p5_representable() {
    let cases = parse("paths.cases", PATHS);
    let mut r = Report::new();
    let mut rows = [0usize; 3];
    for c in cases.iter().filter(|c| c.function() == "representable") {
        let os = match c.arg("os") {
            "windows" => Os::Windows,
            "linux" => Os::Linux,
            "macos" => Os::MacOs,
            o => panic!("{}: os {o:?}", c.at()),
        };
        for row in c.block("rows").unwrap_or(&[]) {
            let (segment, rest) = json_prefix(row);
            let want = match rest.trim() {
                "true" => true,
                "false" => false,
                w => panic!("{}: row {row:?} expects {w:?}", c.at()),
            };
            rows[usize::from(os.tag() - 1)] += 1;
            let got = representable(os, &segment);
            r.check(c, got == want, || {
                format!(
                    "representable({os:?}, {:?}…, {} bytes) is {got}",
                    segment.chars().take(12).collect::<String>(),
                    segment.len()
                )
            });
        }
    }
    // Windows: 32 device names × 6 spellings, 2 trailing, 7 reserved, 4 lengths, 2 ordinary names.
    assert_eq!(rows, [32 * 6 + 2 + 7 + 4 + 2, 7, 5]);
    r.finish();
}

#[test]
fn paths_p8_symlink_oid() {
    let cases = parse("paths.cases", PATHS);
    let mut r = Report::new();
    let mut n = 0;
    for c in cases.iter().filter(|c| c.function() == "symlink_oid") {
        n += 1;
        let format = match c.arg("algo") {
            "sha1" => ObjectFormat::Sha1,
            "sha256" => ObjectFormat::Sha256,
            a => panic!("{}: algo {a:?}", c.at()),
        };
        let target = json(c.arg("target"));
        let got = blob_oid(format, target.as_bytes());
        let want = c.one("expect");
        r.check(
            c,
            got.algo() == format.algo() && got.to_string() == want,
            || format!("gave {got:?}, expected {want}"),
        );
    }
    assert_eq!(n, 3);
    r.finish();
}

/// P11 (b)'s device list ([OS/path §3]; [F12 §2.4] RN-4) is [`is_device_name`]. RN-1 to RN-8 are checked in order
/// and the first failing rule is refused, so every case refused by RN-4 has a segment that is a device name, and
/// every case accepted or refused by RN-5 to RN-8 has none; a case refused by RN-1 to RN-3 never reaches RN-4. The
/// other rules are [F12]'s, built with the ref store. RN-4 applies to the user part; the fixed words of [F12 §2.3]
/// are no device names, so every segment is tested.
#[test]
fn paths_p11b_device_names() {
    let cases = parse("paths.cases", PATHS);
    let mut r = Report::new();
    let mut decided = 0;
    for c in cases.iter().filter(|c| c.function() == "ref_name_check") {
        let name = json(c.arg("name"));
        let device = name.split('/').any(is_device_name);
        let want = match c.one("expect") {
            "ok" | "refused RN-5" | "refused RN-6" | "refused RN-7" | "refused RN-8" => false,
            "refused RN-4" => true,
            "refused RN-1" | "refused RN-2" | "refused RN-3" => continue,
            e => panic!("{}: expect {e:?}", c.at()),
        };
        decided += 1;
        r.check(c, device == want, || {
            format!("a device-name segment in {name:?}: {device}, expected {want}")
        });
    }
    // 27 cases, of which 10 are refused by RN-1 to RN-3.
    assert_eq!(decided, 17);
    r.finish();
}

// --- fixtures/canonical: the derived uids in use ([F08 §11.2]–§11.5) -----------------------------------------

/// The derivations checked across the canonical files, each once: `(what, uid)`.
type Seen = BTreeSet<(&'static str, String)>;

/// One node of a canonical state block, as far as the derivations need it.
#[derive(Default)]
struct StateNode {
    uid: Option<Uid>,
    kind: String,
    tombstone: bool,
    root: Option<String>,
    origin: Option<(String, String)>,
    origin_pred: Option<Uid>,
}

/// Checks a finished node: an artifact's uid is `uid_file(root, origin_path, origin_pred)`, a root node's (an `area`
/// with a `root` field) is `uid_root(root)` ([F08 §11.5]).
fn check_node(c: &Case, n: &StateNode, r: &mut Report, seen: &mut Seen) {
    let Some(u) = n.uid else { return };
    if n.tombstone {
        return;
    }
    match (n.kind.as_str(), &n.root, &n.origin) {
        ("artifact", Some(root), Some((origin_root, path))) => {
            r.check(c, origin_root == root, || {
                format!("{u}: origin_path's root {origin_root:?} is not root {root:?}")
            });
            let got = uid_file(root, path, n.origin_pred.as_ref());
            r.check(c, got == Ok(u), || {
                format!(
                    "file node {u}: uid_file({root:?}, {path:?}, {:?}) gave {got:?}",
                    n.origin_pred
                )
            });
            seen.insert(("uid_file", u.to_string()));
        }
        ("area", Some(root), _) => {
            let got = uid_root(root);
            r.check(c, got == Ok(u), || {
                format!("root node {u}: uid_root({root:?}) gave {got:?}")
            });
            seen.insert(("uid_root", u.to_string()));
        }
        _ => {}
    }
}

/// An `at` line of node `src`: the anchor uid is `uid_anchor(src, captured, pred)` ([F08 §11.5]). With `fresh` (the
/// case captures its anchors with the selectors they hold, `anchors.cases`), an anchor without a predecessor term
/// whose texts are held is also checked as `captured` of those selectors on its target. Otherwise `captured` is
/// trusted as stored ([F08 §11.5]): a repin keeps it while the selectors change ([F08 §11.4]), and an imported anchor
/// keeps it as written, so it is not recomputed from the current selectors.
fn check_anchor(c: &Case, src: Uid, toks: &[&str], fresh: bool, r: &mut Report, seen: &mut Seen) {
    let dst = uid(toks[1]);
    let u = uid(toks[2]);
    let a = attrs(&toks[3..]);
    let cap = b16(attr(&a, "captured").expect("captured"));
    let pred = attr(&a, "pred").map(uid);
    let got = uid_anchor(&src, &cap, pred.as_ref());
    r.check(c, got == Ok(u), || {
        format!(
            "anchor {u}: uid_anchor({src}, {}, {pred:?}) gave {got:?}",
            hex(&cap)
        )
    });
    seen.insert(("uid_anchor", u.to_string()));
    let sel = Selectors::from_attrs(&a);
    if fresh && pred.is_none() && sel.has_texts() {
        let got = sel.captured(&dst);
        r.check(c, got == Ok(cap), || {
            format!(
                "anchor {u}: captured of its selectors on {dst} gave {:?}, the record says {}",
                got.map(|g| hex(&g)),
                hex(&cap)
            )
        });
        seen.insert(("captured", u.to_string()));
    }
}

fn canonical_derivations(
    file: &'static str,
    text: &'static str,
    fresh: bool,
    r: &mut Report,
    seen: &mut Seen,
) {
    for c in &parse(file, text) {
        for (name, lines) in &c.blocks {
            if !name.ends_with("-state") {
                continue;
            }
            let mut node = StateNode::default();
            for l in lines {
                let t = tokens(l);
                if !l.starts_with(' ') {
                    check_node(c, &node, r, seen);
                    node = StateNode::default();
                    if t[0] == "node" {
                        node.uid = Some(uid(t[1]));
                        node.kind = t[2].to_owned();
                        node.tombstone = t.get(3) == Some(&"deleted");
                    }
                    continue;
                }
                if l.starts_with("    ") {
                    // A conflict side's lines.
                    continue;
                }
                match t.as_slice() {
                    ["field", "root", "text", v] => node.root = Some(json(v)),
                    ["field", "origin_path", "path", root, p] => {
                        node.origin = Some(((*root).to_owned(), json(p)));
                    }
                    ["field", "origin_pred", "ref", u] => node.origin_pred = Some(uid(u)),
                    ["at", ..] => {
                        let src = node.uid.expect("an at line belongs to a node");
                        check_anchor(c, src, &t, fresh, r, seen);
                    }
                    _ => {}
                }
            }
            check_node(c, &node, r, seen);
        }
    }
}

#[test]
fn canonical_derived_uids() {
    let mut r = Report::new();
    let mut seen = Seen::new();
    // `anchors.cases` registers the file and captures both anchors in the case's commit ("captured and the anchor
    // uids follow [F08 §11.4]"). `checkpoint.cases` imports its anchors: the first one's selectors differ from the
    // inputs of its `captured` (a repin), which is the digest of the quote "## 3. Storage" with the second anchor's
    // prefix and suffix and no occurrence; the second anchor, whose predecessor term is the first, adds `occurrence`.
    canonical_derivations(
        "canonical/anchors.cases",
        CANONICAL_ANCHORS,
        true,
        &mut r,
        &mut seen,
    );
    canonical_derivations(
        "canonical/checkpoint.cases",
        CANONICAL_CHECKPOINT,
        false,
        &mut r,
        &mut seen,
    );
    let count = |what: &str| seen.iter().filter(|(w, _)| *w == what).count();
    // Two file nodes (docs/storage/lock.md, docs/plans/storage.md), one root node (project), four anchors, and the
    // `captured` of the two fresh captures of `anchors.cases`.
    assert_eq!(
        [
            count("uid_file"),
            count("uid_root"),
            count("uid_anchor"),
            count("captured")
        ],
        [2, 1, 4, 2],
        "{seen:?}"
    );
    r.finish();
}
