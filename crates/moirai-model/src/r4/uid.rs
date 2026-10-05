//! The derived identities of R4 ([F08 §11]; [40 §2.3], §2.7; R-3, R-5): `uid_file`, `uid_root`, `captured` and
//! `uid_anchor` over `lp()`-framed arguments, registration of a file node on a view with its predecessor order and the
//! dead-uid rule, `#N` reuse by the store-wide allocation, and the anchor capture's de-duplication and predecessor term.

use crate::err::Refusal;
use crate::r4::anchor::Kind;
use crate::value::{Uid, blake3_128};

/// `uid_file(r, p, q) = BLAKE3-128(lp("moirai-file-v1") ‖ lp(r) ‖ lp(p) ‖ lp(q or empty))`.
// spec: [F08 §11.2]; [40 §2.3]
pub fn uid_file(root: &str, path: &str, q: Option<Uid>) -> Uid {
    let q = q.map(|u| u.0.to_vec()).unwrap_or_default();
    Uid(blake3_128(&[
        b"moirai-file-v1",
        root.as_bytes(),
        path.as_bytes(),
        &q,
    ]))
}

/// `uid_root(r) = BLAKE3-128(lp("moirai-root-v1") ‖ lp(r))`.
// spec: [F08 §11.3]; [40 §2.3]
pub fn uid_root(root: &str) -> Uid {
    Uid(blake3_128(&[b"moirai-root-v1", root.as_bytes()]))
}

/// The selector inputs of a capture ([F08 §11.1], §11.4): the current selectors an anchor is compared by, and the
/// capture digest's arguments. Absent texts are empty.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Selectors {
    /// The anchor kind.
    pub kind: Kind,
    /// The scope value's bytes; empty without a scope.
    pub scope: Vec<u8>,
    /// `quote.exact`.
    pub quote: Vec<u8>,
    /// `prefix.exact`.
    pub prefix: Vec<u8>,
    /// `suffix.exact`.
    pub suffix: Vec<u8>,
    /// `end.exact`.
    pub end: Vec<u8>,
    /// The occurrence index.
    pub occurrence: Option<u16>,
    /// The window value W.
    pub window: Vec<u8>,
}

impl Selectors {
    /// The selectors as `captured` and de-duplication read them: quote, prefix and suffix only for the kinds that
    /// carry a quote, `end` only for `range`, the window only for `lines` ([F08 §11.1]).
    pub fn masked(&self) -> Selectors {
        let quoted = self.kind.has_quote();
        Selectors {
            kind: self.kind,
            scope: self.scope.clone(),
            quote: if quoted {
                self.quote.clone()
            } else {
                Vec::new()
            },
            prefix: if quoted {
                self.prefix.clone()
            } else {
                Vec::new()
            },
            suffix: if quoted {
                self.suffix.clone()
            } else {
                Vec::new()
            },
            end: if self.kind == Kind::Range {
                self.end.clone()
            } else {
                Vec::new()
            },
            occurrence: self.occurrence,
            window: if self.kind == Kind::Lines {
                self.window.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// The digests of quote, prefix, suffix and end as the hashed selector block holds them ([F07 §8.2]): BLAKE3-128
    /// of each text, `None` where the kind carries no such text (the same applicability as [`Selectors::masked`]).
    // spec: [F07 §8.2]; [F08 §10.3] orders 13, 15, 17, 19
    pub fn digests(&self) -> [Option<[u8; 16]>; 4] {
        let quoted = self.kind.has_quote();
        let d = |t: &Vec<u8>, applies: bool| applies.then(|| crate::value::blake3_128(&[t]));
        [
            d(&self.quote, quoted),
            d(&self.prefix, quoted),
            d(&self.suffix, quoted),
            d(&self.end, self.kind == Kind::Range),
        ]
    }
}

/// `captured = BLAKE3-128(lp(file uid) ‖ lp(kind) ‖ lp(scope) ‖ lp(quote) ‖ lp(prefix) ‖ lp(suffix) ‖ lp(end) ‖
/// lp(occurrence) ‖ lp(window if kind = lines, else empty))`.
// spec: [F08 §11.4]; [F08 §11.1]; [40 §2.7]
pub fn captured(file: Uid, sel: &Selectors) -> [u8; 16] {
    let m = sel.masked();
    let occ = m
        .occurrence
        .map(|o| o.to_le_bytes().to_vec())
        .unwrap_or_default();
    blake3_128(&[
        &file.0,
        m.kind.name().as_bytes(),
        &m.scope,
        &m.quote,
        &m.prefix,
        &m.suffix,
        &m.end,
        &occ,
        &m.window,
    ])
}

/// `uid_anchor(s, c, p) = BLAKE3-128(lp("moirai-anchor-v1") ‖ lp(s) ‖ lp(c) ‖ lp(p or empty))`.
// spec: [F08 §11.4]; [40 §2.7]
pub fn uid_anchor(src: Uid, captured: [u8; 16], pred: Option<Uid>) -> Uid {
    let p = pred.map(|u| u.0.to_vec()).unwrap_or_default();
    Uid(blake3_128(&[b"moirai-anchor-v1", &src.0, &captured, &p]))
}

/// The status of a file node as registration reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileStatus {
    /// `planned`.
    Planned,
    /// `present`.
    Present,
    /// `removed`.
    Removed,
}

impl FileStatus {
    /// The status of a name.
    pub fn from_name(s: &str) -> Option<FileStatus> {
        match s {
            "planned" => Some(FileStatus::Planned),
            "present" => Some(FileStatus::Present),
            "removed" => Some(FileStatus::Removed),
            _ => None,
        }
    }

    /// The name.
    pub fn name(self) -> &'static str {
        match self {
            FileStatus::Planned => "planned",
            FileStatus::Present => "present",
            FileStatus::Removed => "removed",
        }
    }
}

/// A node of a view as registration reads it ([F08 §11.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewNode {
    /// A live artifact (file node).
    File {
        /// Its uid.
        uid: Uid,
        /// Its `root` field.
        root: String,
        /// Its `path`, exact bytes.
        path: String,
        /// Its status.
        status: FileStatus,
        /// Its `aliases`.
        aliases: Vec<String>,
    },
    /// A tombstone of any kind.
    Tomb {
        /// Its uid.
        uid: Uid,
    },
    /// Any other live node.
    Other {
        /// Its uid.
        uid: Uid,
    },
}

impl ViewNode {
    /// The node's uid.
    pub fn uid(&self) -> Uid {
        match self {
            ViewNode::File { uid, .. } | ViewNode::Tomb { uid } | ViewNode::Other { uid } => *uid,
        }
    }
}

/// The outcome of registering a file ([F08 §11.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registration {
    /// Step 1: the view already holds the key with status `present` or `planned`; nothing is created.
    Existing(Uid),
    /// Steps 2–5: a new node with this uid and `origin_pred`.
    New {
        /// The derived uid.
        uid: Uid,
        /// `origin_pred`; `None` when empty.
        pred: Option<Uid>,
    },
}

/// The predecessor of a registration at (r, p) on V: among the live artifacts of root r that once held p and do not
/// hold it now — status `removed` with `path` = p, or `p ∈ aliases` — the bytewise greatest uid; tombstones are no
/// candidates.
// spec: [F08 §11.2] step 2; [40 §2.3] predecessor
pub fn predecessor(view: &[ViewNode], root: &str, path: &str) -> Option<Uid> {
    view.iter()
        .filter_map(|n| match n {
            ViewNode::File {
                uid,
                root: r,
                path: p,
                status,
                aliases,
            } if r == root
                && ((*status == FileStatus::Removed && p == path)
                    || aliases.iter().any(|a| a == path)) =>
            {
                Some(*uid)
            }
            _ => None,
        })
        .max()
}

/// Registration of the file at (root r, path p) on view V ([F08 §11.2] steps 1–5): the existing node of step 1, or the
/// new uid with its predecessor, re-derived while it names any node of V (dead uids are never re-created). A loop that
/// runs more times than V has nodes is an internal error (exit 1).
// spec: [F08 §11.2]; [40 §2.3]; [F18 §2.2] I-F2; [F18 §2.14] I-F14
pub fn register(view: &[ViewNode], root: &str, path: &str) -> Result<Registration, Refusal> {
    if let Some(n) = view.iter().find(|n| {
        matches!(n, ViewNode::File { root: r, path: p, status, .. }
            if r == root && p == path && *status != FileStatus::Removed)
    }) {
        return Ok(Registration::Existing(n.uid()));
    }
    let mut q = predecessor(view, root, path);
    let mut u = uid_file(root, path, q);
    let mut steps = 0usize;
    while view.iter().any(|n| n.uid() == u) {
        steps += 1;
        if steps > view.len() {
            return Err(Refusal::new(
                "internal",
                1,
                "internal error: the uid derivation loop exceeded its bound ([F08 §11.2] step 4)",
            ));
        }
        q = Some(u);
        u = uid_file(root, path, q);
    }
    Ok(Registration::New { uid: u, pred: q })
}

/// The `#N` of a registered uid ([F08 §11.2] step 5; [40 §2.3] "`#N` reuse"; I-F2): the store-wide `UIDX` gives a
/// uid the store already knows, on any branch, its `#N`; a new uid takes `next_id`. Returns the number and whether it
/// is new. A re-keyed node's uid′ is new, so it gets a new `#N` ([RULES/link-merge-rules] RK-008).
// spec: [F08 §11.2] step 5; [F18 §2.2] I-F2
pub fn number(
    uidx: &std::collections::BTreeMap<Uid, crate::value::Nid>,
    next_id: u32,
    uid: Uid,
) -> (crate::value::Nid, bool) {
    match uidx.get(&uid) {
        Some(n) => (*n, false),
        None => (crate::value::Nid(next_id), true),
    }
}

/// An anchor on (s, f) as capture reads it: its uid, `captured`, `pred` and current selectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeAnchor {
    /// The anchor uid.
    pub uid: Uid,
    /// Its `captured`.
    pub captured: [u8; 16],
    /// Its `pred`.
    pub pred: Option<Uid>,
    /// Its current selectors (empty texts for an anchor held without its text).
    pub current: Selectors,
    /// For an anchor held without its text (`text_unavailable`, a hash-only import), its stored digests of quote,
    /// prefix, suffix and end ([F07 §8.2]); `None` for an anchor with its text.
    pub digests: Option<[Option<[u8; 16]>; 4]>,
}

impl EdgeAnchor {
    /// Whether the anchor's current selectors equal `sel` ([F08 §11.4] step 1): kind, scope, occurrence and (for
    /// `lines`) window bytewise, and quote, prefix, suffix and end by text, or — when the anchor holds no text — by
    /// their BLAKE3-128 digests, the way anchor values compare after a hash-only import ([RULES/link-merge-rules] §2;
    /// [40 §2.11] R-10).
    // spec: [F08 §11.4] step 1; [F18 §2.3] I-F3
    pub fn same_selectors(&self, sel: &Selectors) -> bool {
        let (a, b) = (self.current.masked(), sel.masked());
        match &self.digests {
            None => a == b,
            Some(d) => {
                a.kind == b.kind
                    && a.scope == b.scope
                    && a.occurrence == b.occurrence
                    && a.window == b.window
                    && *d == b.digests()
            }
        }
    }
}

/// The outcome of a capture's identity steps ([F08 §11.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureId {
    /// Step 1: an anchor on (s, f) with equal current selectors is reused.
    Reuse(Uid),
    /// Steps 2–4: a new anchor.
    New {
        /// Its uid.
        uid: Uid,
        /// Its `captured`.
        captured: [u8; 16],
        /// Its `pred`, present when step 3 ran.
        pred: Option<Uid>,
    },
}

/// A capture of anchor A on (s, `at`, f) of a view ([F08 §11.4] steps 1–4): de-duplication by equal current selectors,
/// then `uid_anchor(s, captured, empty)` re-derived with the colliding uid as `pred` while it names an anchor on
/// (s, f); `anchors` holds the anchors on (s, f). The loop bound counts those anchors.
// spec: [F08 §11.4]; [40 §2.7]; [F18 §2.3] I-F3
pub fn capture_id(
    src: Uid,
    file: Uid,
    anchors: &[EdgeAnchor],
    sel: &Selectors,
) -> Result<CaptureId, Refusal> {
    if let Some(a) = anchors.iter().find(|a| a.same_selectors(sel)) {
        return Ok(CaptureId::Reuse(a.uid));
    }
    let c = captured(file, sel);
    let mut p = None;
    let mut u = uid_anchor(src, c, None);
    let mut steps = 0usize;
    while anchors.iter().any(|a| a.uid == u) {
        steps += 1;
        if steps > anchors.len() {
            return Err(Refusal::new(
                "internal",
                1,
                "internal error: the anchor uid loop exceeded its bound ([F08 §11.4] step 3)",
            ));
        }
        p = Some(u);
        u = uid_anchor(src, c, p);
    }
    Ok(CaptureId::New {
        uid: u,
        captured: c,
        pred: p,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canon::fixtures::{Case, hex_bytes, json};
    use crate::r4::tests::cases;

    fn uid_hex(s: &str) -> Uid {
        let b = crate::r4::tests::unhex(s);
        let mut u = [0u8; 16];
        u.copy_from_slice(&b);
        Uid(u)
    }

    fn arg<'a>(c: &'a Case, name: &str) -> &'a str {
        c.lines
            .get("arg")
            .and_then(|v| {
                v.iter()
                    .find_map(|a| a.strip_prefix(name).and_then(|r| r.strip_prefix(' ')))
            })
            .unwrap_or_else(|| panic!("{}: no arg {name}", c.id))
    }

    fn opt_text(v: &str) -> Vec<u8> {
        if v == "-" {
            Vec::new()
        } else {
            json(v).into_bytes()
        }
    }

    /// The input bytes the derivation hashes, rebuilt from the arguments, for comparison with `input-hex`.
    fn framed(parts: &[&[u8]]) -> Vec<u8> {
        let mut v = Vec::new();
        for p in parts {
            crate::value::lp(&mut v, p);
        }
        v
    }

    /// A known uid reuses its `#N` on any branch; a new one takes `next_id`.
    #[test]
    fn a_known_uid_reuses_its_number() {
        let u = uid_file("project", "docs/a.md", None);
        let mut uidx = std::collections::BTreeMap::new();
        assert_eq!(number(&uidx, 7, u), (crate::value::Nid(7), true));
        uidx.insert(u, crate::value::Nid(3));
        assert_eq!(number(&uidx, 7, u), (crate::value::Nid(3), false));
        match register(&[], "project", "docs/a.md").unwrap() {
            Registration::New { uid, .. } => {
                assert_eq!(number(&uidx, 9, uid).0, crate::value::Nid(3))
            }
            r => panic!("{r:?}"),
        }
    }

    /// Every case of `fixtures/r4/cases/derivations.cases`: the hashed bytes and the output.
    #[test]
    fn derivation_cases_pass() {
        let all = cases("derivations.cases");
        assert!(all.len() >= 21, "{} derivation cases", all.len());
        for c in &all {
            let input = hex_bytes(c.block("input-hex"));
            assert_eq!(
                input.len().to_string(),
                c.line("input-length").unwrap(),
                "{}",
                c.id
            );
            let want = c.line("output").unwrap();
            let (bytes, got) = match c.line("function").unwrap() {
                "uid_file" => {
                    let root = json(arg(c, "root"));
                    let path = json(arg(c, "path"));
                    let pred = match arg(c, "pred") {
                        "-" => None,
                        h => Some(uid_hex(h)),
                    };
                    let q = pred.map(|u| u.0.to_vec()).unwrap_or_default();
                    (
                        framed(&[b"moirai-file-v1", root.as_bytes(), path.as_bytes(), &q]),
                        uid_file(&root, &path, pred).hex(),
                    )
                }
                "uid_root" => {
                    let root = json(arg(c, "root"));
                    (
                        framed(&[b"moirai-root-v1", root.as_bytes()]),
                        uid_root(&root).hex(),
                    )
                }
                "captured" => {
                    let file = uid_hex(arg(c, "file_uid"));
                    let kind = Kind::from_name(arg(c, "kind")).expect("kind");
                    let sel = Selectors {
                        kind,
                        scope: match arg(c, "scope") {
                            "-" => Vec::new(),
                            h => crate::r4::tests::unhex(h),
                        },
                        quote: opt_text(arg(c, "quote")),
                        prefix: opt_text(arg(c, "prefix")),
                        suffix: opt_text(arg(c, "suffix")),
                        end: opt_text(arg(c, "end")),
                        occurrence: match arg(c, "occurrence") {
                            "-" => None,
                            n => Some(n.parse().unwrap()),
                        },
                        window: match arg(c, "window") {
                            "-" => Vec::new(),
                            h => crate::r4::tests::unhex(h),
                        },
                    };
                    let m = sel.masked();
                    let occ = m
                        .occurrence
                        .map(|o| o.to_le_bytes().to_vec())
                        .unwrap_or_default();
                    (
                        framed(&[
                            &file.0,
                            kind.name().as_bytes(),
                            &m.scope,
                            &m.quote,
                            &m.prefix,
                            &m.suffix,
                            &m.end,
                            &occ,
                            &m.window,
                        ]),
                        crate::value::hex(&captured(file, &sel)),
                    )
                }
                "uid_anchor" => {
                    let src = uid_hex(arg(c, "src"));
                    let cap = uid_hex(arg(c, "captured")).0;
                    let pred = match arg(c, "pred") {
                        "-" => None,
                        h => Some(uid_hex(h)),
                    };
                    let p = pred.map(|u| u.0.to_vec()).unwrap_or_default();
                    (
                        framed(&[b"moirai-anchor-v1", &src.0, &cap, &p]),
                        uid_anchor(src, cap, pred).hex(),
                    )
                }
                f => panic!("{}: unknown function {f}", c.id),
            };
            assert_eq!(bytes, input, "{}: hashed bytes", c.id);
            assert_eq!(got, want, "{}", c.id);
        }
    }

    /// A `key=value` field of a view line; values are JSON strings or tokens.
    fn fields(line: &str) -> Vec<(String, String)> {
        // Split on spaces outside JSON strings.
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_str = false;
        let mut esc = false;
        for ch in line.chars() {
            if in_str {
                cur.push(ch);
                if esc {
                    esc = false;
                } else if ch == '\\' {
                    esc = true;
                } else if ch == '"' {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '"' => {
                    in_str = true;
                    cur.push(ch);
                }
                ' ' => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(ch),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out.into_iter()
            .map(|t| match t.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (String::new(), t),
            })
            .collect()
    }

    fn get<'a>(f: &'a [(String, String)], k: &str) -> Option<&'a str> {
        f.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    fn selectors(f: &[(String, String)]) -> Selectors {
        Selectors {
            kind: Kind::from_name(get(f, "kind").unwrap()).unwrap(),
            scope: Vec::new(),
            quote: get(f, "quote")
                .map(|v| json(v).into_bytes())
                .unwrap_or_default(),
            prefix: get(f, "prefix")
                .map(|v| json(v).into_bytes())
                .unwrap_or_default(),
            suffix: get(f, "suffix")
                .map(|v| json(v).into_bytes())
                .unwrap_or_default(),
            end: get(f, "end")
                .map(|v| json(v).into_bytes())
                .unwrap_or_default(),
            occurrence: get(f, "occurrence").map(|v| v.parse().unwrap()),
            window: get(f, "window")
                .map(crate::r4::tests::unhex)
                .unwrap_or_default(),
        }
    }

    /// Every case of `fixtures/r4/cases/predecessor.cases`: registration and anchor capture over a stated view.
    #[test]
    fn predecessor_cases_pass() {
        let all = cases("predecessor.cases");
        assert!(all.len() >= 15, "{} predecessor cases", all.len());
        for c in &all {
            let want = c.line("result").unwrap();
            let got = match c.line("operation").unwrap() {
                "register" => {
                    let mut view = Vec::new();
                    for l in c.block("view") {
                        let f = fields(l);
                        let Some((_, head)) = f.first() else { continue };
                        if head.starts_with(';') {
                            continue;
                        }
                        let uid = uid_hex(&f[1].1);
                        view.push(match head.as_str() {
                            "file" => ViewNode::File {
                                uid,
                                root: get(&f, "root").unwrap().to_string(),
                                path: json(get(&f, "path").unwrap()),
                                status: FileStatus::from_name(get(&f, "status").unwrap()).unwrap(),
                                aliases: get(&f, "aliases")
                                    .map(|v| {
                                        let mut out = Vec::new();
                                        let mut rest = v;
                                        while !rest.is_empty() {
                                            let end = rest[1..].find('"').map(|i| i + 2).unwrap();
                                            out.push(json(&rest[..end]));
                                            rest = rest[end..]
                                                .strip_prefix(',')
                                                .unwrap_or(&rest[end..]);
                                        }
                                        out
                                    })
                                    .unwrap_or_default(),
                            },
                            "tombstone" => ViewNode::Tomb { uid },
                            "node" => ViewNode::Other { uid },
                            h => panic!("{}: view line {h}", c.id),
                        });
                    }
                    match register(&view, &json(arg(c, "root")), &json(arg(c, "path"))).unwrap() {
                        Registration::Existing(u) => format!("existing {}", u.hex()),
                        Registration::New { uid, pred } => format!(
                            "new {} pred {}",
                            uid.hex(),
                            pred.map_or("-".to_string(), |p| p.hex())
                        ),
                    }
                }
                "capture" => {
                    let src = uid_hex(arg(c, "src"));
                    let dst = uid_hex(arg(c, "dst"));
                    let mut anchors = Vec::new();
                    for l in c.block("view") {
                        let f = fields(l);
                        if f.first().map(|x| x.1.as_str()) != Some("anchor") {
                            continue;
                        }
                        if get(&f, "src").map(uid_hex) != Some(src)
                            || get(&f, "dst").map(uid_hex) != Some(dst)
                        {
                            continue;
                        }
                        anchors.push(EdgeAnchor {
                            uid: uid_hex(&f[1].1),
                            captured: uid_hex(get(&f, "captured").unwrap()).0,
                            pred: match get(&f, "pred").unwrap() {
                                "-" => None,
                                h => Some(uid_hex(h)),
                            },
                            current: selectors(&f),
                            digests: None,
                        });
                    }
                    let sel_line = format!("x {}", arg(c, "selectors"));
                    let sel = selectors(&fields(&sel_line));
                    match capture_id(src, dst, &anchors, &sel).unwrap() {
                        CaptureId::Reuse(u) => format!("reuse {}", u.hex()),
                        CaptureId::New {
                            uid,
                            captured,
                            pred,
                        } => format!(
                            "new {} captured {} pred {}",
                            uid.hex(),
                            crate::value::hex(&captured),
                            pred.map_or("-".to_string(), |p| p.hex())
                        ),
                    }
                }
                o => panic!("{}: operation {o}", c.id),
            };
            assert_eq!(got, want, "{}", c.id);
        }
    }
}
