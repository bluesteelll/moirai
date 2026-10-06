//! Anchor capture ([F20 §6.1]; [40 §2.7] "Capture", steps 3–5): the authoring forms, the quote input rules, the
//! selectors, the uniqueness ladder, and the anchor's identity ([F08 §11.4]).

use std::collections::VecDeque;
use std::num::NonZeroU16;

use moirai_diff::{Pattern, Searcher};
use xxhash_rust::xxh3::Xxh3Default;

use super::exact::{Context, Exact, ExactSink, ExactSpec, RangeEnd, Set};
use super::header::{HeaderAcc, HeaderKind};
use super::nstream::{LineMap, NSink, Tail};
use super::resolve::{Again, ScanFeed, SpanHash, read_first};
use super::window::{WIN, Window};
use super::{Anchor, AnchorKind, Budget, LineSpan, Mode, ReadParams, ScannerRule, Texts, Watch};
use crate::oid::Oid;
use crate::r14;
use crate::scan::{
    FormKind, FormOutcome, Items, Lang, ScanFailed, Scope, Selector, SelectorError, find_form,
    split_heading, split_symbol,
};
use crate::text::{ByteSource, Content, ContentReader, Unavailable, cutp, cuts, is_ws, lines, nl};
use crate::uid::{Capture, Uid, UidError, captured, derive_anchor_uid};

/// An authoring form ([40 §2.7] "Authoring forms"; [F21 §6.1]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form<'a> {
    /// `path`: a `file` anchor.
    File,
    /// `path:L` or `path:L-M`: a `quote`, `range` or `lines` anchor on those lines.
    Lines(LineSpan),
    /// Quote text from `--quote-file`, stdin or the `quote` parameter ([F20 §6.1] step 3), with the lines of a
    /// `path:L-M` spec when one was given (its occurrence must lie within them).
    Quote {
        /// The raw input bytes.
        text: &'a [u8],
        /// Lines the occurrence must lie within.
        within: Option<LineSpan>,
    },
    /// `path::S1/…/Sk` ([F21 §6.2], §6.3): the selector after the split.
    Symbol(&'a str),
    /// `path#H1/…/Hk` ([F21 §6.4]): the selector after the split.
    Heading(&'a str),
}

/// A parsed anchor spec: the path text, the form, and the commit of a `path@<commit>:L-M` form (`mode = pinned`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec<'a> {
    /// The path as written; resolving it is the caller's ([40 §2.7] step 1).
    pub path: &'a str,
    /// The form.
    pub form: Form<'a>,
    /// The commit of a pinned form.
    pub commit: Option<&'a str>,
}

/// `L` or `L-M` in decimal; a line number beyond `u32` saturates, so the capture refuses it as out of range.
// spec: [40 §2.7] "Authoring forms" (`path:L`, `path:L-M`)
fn line_range(s: &str) -> Option<LineSpan> {
    let num = |t: &str| -> Option<u32> {
        (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())).then(|| {
            t.parse::<u64>()
                .map_or(u32::MAX, |v| u32::try_from(v).unwrap_or(u32::MAX))
        })
    };
    match s.split_once('-') {
        Some((l, m)) => Some(LineSpan {
            first: num(l)?,
            last: num(m)?,
        }),
        None => {
            let l = num(s)?;
            Some(LineSpan { first: l, last: l })
        }
    }
}

/// Whether `c` can be the `<commit>` of a `path@<commit>:L-M` form: 4 to 64 hexadecimal digits (an abbreviated or
/// full git commit id, or a moirai commit literal, `c` and hexadecimal digits, [F12 §3.8]). [40 §2.7] gives no syntax;
/// this reading keeps a path whose name holds `@` a path (spec finding of WP-64).
// spec: [40 §2.7] "Authoring forms" (`path@<commit>:L-M`; WP-64 reading of `<commit>`)
fn commit_text(c: &str) -> bool {
    (4..=64).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Splits an anchor spec into its path and form ([40 §2.7] "Authoring forms"; [F21 §6.1] for the `::` and `#`
/// splits). In order: the `symbol` split, the `heading` split, a trailing `:L` or `:L-M` (with `@<commit>` before it
/// for a pinned form, the commit 4 to 64 hexadecimal digits), else the whole spec is a path (`file`). A quote input is
/// the caller's: it pairs this result with [`Form::Quote`].
// spec: [F21 §6.1], [40 §2.7] "Authoring forms"
#[must_use]
pub fn parse_spec(spec: &str) -> Spec<'_> {
    if let Some((path, sel)) = split_symbol(spec) {
        return Spec {
            path,
            form: Form::Symbol(sel),
            commit: None,
        };
    }
    if let Some((path, sel)) = split_heading(spec) {
        return Spec {
            path,
            form: Form::Heading(sel),
            commit: None,
        };
    }
    if let Some((head, tail)) = spec.rsplit_once(':')
        && let Some(span) = line_range(tail)
        && !head.is_empty()
    {
        if let Some((path, commit)) = head.rsplit_once('@')
            && !path.is_empty()
            && commit_text(commit)
        {
            return Spec {
                path,
                form: Form::Lines(span),
                commit: Some(commit),
            };
        }
        return Spec {
            path: head,
            form: Form::Lines(span),
            commit: None,
        };
    }
    Spec {
        path: spec,
        form: Form::File,
        commit: None,
    }
}

/// The inputs of a capture.
#[derive(Clone, Copy, Debug)]
pub struct CaptureInput<'a> {
    /// The file's root-relative path: its last component decides the scanner's language ([F21 §1.3]).
    pub path: &'a [u8],
    /// The form.
    pub form: Form<'a>,
    /// `live`, or `pinned` for a `path@<commit>:L-M` form (the caller supplies that commit's content).
    pub mode: Mode,
    /// `--watch`, or the kind's default.
    pub watch: Option<Watch>,
    /// The file node's uid ([F08 §11.4] `captured`).
    pub file_uid: &'a Uid,
    /// The observed git commit, when the tree has one.
    pub git: Option<Oid>,
    /// The parameters of the reads.
    pub read: ReadParams,
    /// [`ScannerRule::CURRENT`].
    pub scanners: ScannerRule,
}

/// A capture's result: the anchor's selectors, and the content read (its `oid` and statistics, for the file node).
#[derive(Clone, Debug)]
pub struct Captured {
    /// The anchor; `pred` is set by [`identify`].
    pub anchor: Anchor,
    /// The content of the first read.
    pub content: Content,
}

/// A refused form ([F19 §10.2] `anchor_spec`, exit 2), in [F19 §10.3]'s case order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `range`: the lines are outside the file (it has `lines` lines).
    Range {
        /// The file's line count.
        lines: u64,
    },
    /// `binary`: a span anchor on content that is not text.
    Binary,
    /// `fffd`: quote input with U+FFFD.
    Fffd,
    /// `empty`: quote input empty after trimming.
    Empty,
    /// `not-found`: the quote does not occur.
    NotFound,
    /// `no-scanner`: a `symbol` or `heading` form while the interim scanner rule holds.
    NoScanner,
    /// `no-scope`: no item matches the `symbol` or `heading` form.
    NoScope,
    /// `several`: several items match, or the match's name path names several; their scope texts.
    Several(Vec<String>),
    /// `not-recordable`: the match's name path cannot be stored ([F21 §2.3]).
    NotRecordable(String),
    /// `scan-failed`: the file's scan failed ([F21 §2.7]).
    ScanFailed,
    /// `syntax`: a malformed selector.
    Syntax(SelectorError),
    /// `no-window`: a span of trivial lines with an empty window ([F18 §2.9]).
    NoWindow,
    /// `header` watch asked of a kind with no header (`quote`, `range`, `lines`); [F19 §10.2] has no case for it
    /// (spec finding of WP-64).
    HeaderWatch,
    /// Only the occurrence tells the captured hit apart, and its index is above 65,535, which [F08 §10.3]'s `u16`
    /// cannot hold; [F19 §10.2] has no case for it (spec finding of WP-64).
    Occurrence,
}

impl Refusal {
    /// The JSON `case` ([F19 §10.3]).
    #[must_use]
    // spec: [F19 §10.3] (`anchor_spec` cases)
    pub const fn case(&self) -> &'static str {
        match self {
            Refusal::Range { .. } => "range",
            Refusal::Binary => "binary",
            Refusal::Fffd => "fffd",
            Refusal::Empty => "empty",
            Refusal::NotFound => "not-found",
            Refusal::NoScanner => "no-scanner",
            Refusal::NoScope => "no-scope",
            Refusal::Several(_) => "several",
            Refusal::NotRecordable(_) => "not-recordable",
            Refusal::ScanFailed => "scan-failed",
            Refusal::Syntax(_) => "syntax",
            Refusal::NoWindow => "no-window",
            Refusal::HeaderWatch => "watch",
            Refusal::Occurrence => "occurrence",
        }
    }
}

/// Why a capture produced no anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The form is refused.
    Refused(Refusal),
    /// The content could not be read, or the budget ran out ([F20 §2.4], §1.5).
    Unavailable(Unavailable),
    /// The uniqueness ladder's exact step did not find the captured position among its hits ([F20 §6.1] step 8): a
    /// defect, never an anchor that is neither unique nor has an occurrence ([F19 §7] exit 1).
    Internal,
}

impl From<Unavailable> for CaptureError {
    fn from(u: Unavailable) -> CaptureError {
        CaptureError::Unavailable(u)
    }
}

impl From<Refusal> for CaptureError {
    fn from(r: Refusal) -> CaptureError {
        CaptureError::Refused(r)
    }
}

/// The identity of a captured anchor on the edge (s, `at`, f) ([F08 §11.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    /// Step 1: an anchor on (s, f) has equal current selectors and is reused.
    Reuse(Uid),
    /// Steps 2–4: a new anchor with this uid and predecessor term.
    New {
        /// The anchor uid.
        uid: Uid,
        /// The stored `pred`.
        pred: Option<Uid>,
    },
}

/// The identity of `cap`, captured by the referrer `src`, among `existing`, the anchors on (s, f) in the view with
/// their current selectors ([F08 §11.4] steps 1–4; [40 §2.7] "Capture de-duplication").
///
/// # Errors
/// [`UidError`] when the derivation loop exceeds its bound (only a BLAKE3 collision causes it).
// spec: [F08 §11.4] steps 1–4
pub fn identify<'a, I>(src: &Uid, cap: &Anchor, existing: I) -> Result<Identity, UidError>
where
    I: IntoIterator<Item = (&'a Uid, &'a Anchor)> + Clone,
{
    if let Some((u, _)) = existing
        .clone()
        .into_iter()
        .find(|(_, x)| x.same_selectors(cap))
    {
        return Ok(Identity::Reuse(*u));
    }
    let count = existing.clone().into_iter().count() as u64;
    let key = derive_anchor_uid(
        src,
        &cap.captured,
        |u| existing.clone().into_iter().any(|(x, _)| x == u),
        count,
    )?;
    Ok(Identity::New {
        uid: key.uid,
        pred: key.pred,
    })
}

/// The file anchor of a planned target ([F08 §10.3] order 23): `blob` is `none`, since the target has no content.
// spec: [F08 §10.3] (`blob` none for a planned target), [40 §3.2]
#[must_use]
pub fn capture_planned(file_uid: &Uid, git: Option<Oid>) -> Anchor {
    file_anchor(file_uid, Watch::Header, Oid::NONE, git, Mode::Live)
}

// spec: [F20 §6.1] step 9, [F08 §10.3] (a `file` anchor: no hint, window or span hash)
fn file_anchor(file_uid: &Uid, watch: Watch, blob: Oid, git: Option<Oid>, mode: Mode) -> Anchor {
    let c = Capture {
        file_uid,
        kind: AnchorKind::File,
        scope: &[],
        quote: &[],
        prefix: &[],
        suffix: &[],
        end: &[],
        occurrence: None,
        window: &[],
    };
    Anchor {
        kind: AnchorKind::File,
        mode,
        watch,
        resolver: r14::RESOLVER_VERSION,
        // A file anchor's arguments are 16 + 4 bytes and the empty strings: no argument reaches 4 GiB.
        captured: captured(&c).unwrap_or([0; 16]),
        pred: None,
        hint: None,
        scope: None,
        texts: None,
        occurrence: None,
        window: None,
        span_hash: None,
        blob,
        git,
        marker: None,
    }
}

/// The quote input rules ([F20 §6.1] step 3): one leading BOM removed; U+FFFD refused; CR LF → LF; each line
/// `nl`-trimmed; leading and trailing empty lines dropped; the rest joined by `0A`; empty refused.
///
/// # Errors
/// [`Refusal::Fffd`], [`Refusal::Empty`].
// spec: [F20 §6.1] step 3 (quote text from `--quote-file` or stdin)
pub fn quote_text(input: &[u8]) -> Result<Vec<u8>, Refusal> {
    let x = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
    if x.windows(3).any(|w| w == b"\xEF\xBF\xBD") {
        return Err(Refusal::Fffd);
    }
    let mut norm = Vec::with_capacity(x.len());
    let mut i = 0;
    while i < x.len() {
        if x[i] == b'\r' && x.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        norm.push(x[i]);
        i += 1;
    }
    let ls: Vec<&[u8]> = lines(&norm).map(nl).collect();
    let a = ls.iter().position(|l| !l.is_empty());
    let z = ls.iter().rposition(|l| !l.is_empty());
    let (Some(a), Some(z)) = (a, z) else {
        return Err(Refusal::Empty);
    };
    Ok(ls[a..=z].join(&b'\n'))
}

// --- sinks ---------------------------------------------------------------------------------------------------------

/// Whether every byte of a line of N is trivial ([F20 §2.5]): `WS` or `{ } ( ) [ ] ; ,`.
// spec: [F20 §2.5] "Trivial line"
fn trivial_bytes(b: &[u8]) -> bool {
    b.iter()
        .all(|&x| is_ws(x) || r14::TRIVIAL_LINE_EXTRA.contains(&x))
}

/// Read 1 of a `path:L-M` capture: the first and last non-trivial line of `[L, M]` with `start(s)`, `end(e)` and the
/// count of non-trivial lines ([F20 §6.1] steps 1–2).
#[derive(Clone, Debug)]
struct SpanFacts {
    l: u64,
    m: u64,
    line: u64,
    start: u64,
    trivial: bool,
    s: Option<(u64, u64)>,
    e: Option<(u64, u64)>,
    nontrivial: u64,
}

impl NSink for SpanFacts {
    fn start_line(&mut self, line: u64, at: u64) {
        self.line = line;
        self.start = at;
        self.trivial = true;
    }
    fn bytes(&mut self, _at: u64, b: &[u8]) {
        if self.trivial && self.line >= self.l && self.line <= self.m && b != b"\n" {
            self.trivial = trivial_bytes(b);
        }
    }
    // spec: [F20 §6.1] step 1 ("spans skip lines that are blank or contain only braces")
    fn end_line(&mut self, line: u64, at: u64) {
        if line >= self.l && line <= self.m && !self.trivial {
            if self.s.is_none() {
                self.s = Some((line, self.start));
            }
            self.e = Some((line, at));
            self.nontrivial += 1;
        }
    }
}

/// Read 1 of a quote-input capture: the first occurrence of the text in N, within the given lines when any.
#[derive(Clone, Debug)]
struct FindFirst<'a> {
    search: Searcher<'a>,
    len: u64,
    within: Option<LineSpan>,
    map: LineMap,
    found: Option<(u64, u64, u64)>,
    hits: Vec<u64>,
}

impl NSink for FindFirst<'_> {
    fn start_line(&mut self, line: u64, at: u64) {
        if self.found.is_none() {
            self.map.start_line(line, at);
        }
    }
    // spec: [F20 §6.1] step 3 (the quote text must occur in N, within the given lines; the lines it covers)
    fn bytes(&mut self, at: u64, b: &[u8]) {
        if self.found.is_some() {
            return;
        }
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        self.search.feed(b, |h| hits.push(h.end as u64));
        for &z in &hits {
            let h = z - self.len;
            let (qs, qe) = (self.map.line_at(h), self.map.line_at(z - 1));
            let ok = self
                .within
                .is_none_or(|w| qs >= u64::from(w.first) && qe <= u64::from(w.last));
            if ok {
                self.found = Some((h, qs, qe));
                break;
            }
        }
        self.hits = hits;
        // Keep only the lines a later hit can start on: none starts before `at + len(b) − len`.
        self.map
            .prune((at + b.len() as u64).saturating_sub(self.len));
    }
    fn end_line(&mut self, _line: u64, _at: u64) {}
}

/// The bytes kept from the quote's start: a whole `quote` span (`QUOTE_MAX`) and one more, which `cutp` reads.
const HEAD: usize = r14::QUOTE_MAX + 1;

/// Where a capture's quote lies: its span `[qs, qe]` and its bytes `[o, o')`, `o` defaulting to `start(qs)` and `o'`
/// to `o + qlen` or else `end(qe)`.
#[derive(Clone, Copy, Debug)]
struct Target {
    qs: u64,
    qe: u64,
    o: Option<u64>,
    o2: Option<u64>,
    qlen: Option<u64>,
}

/// Read 2 of a capture: around the quote bytes `[o, o′)` and the quote span `[qs, qe]` — the widened prefix, the
/// quote's first and last bytes, the widened suffix, the window and `ST` of the hint ([F20 §6.1] steps 3–7, 9). `o`
/// and `o′` default to `start(qs)` and `end(qe)`.
#[derive(Clone)]
struct Collector {
    qs: u64,
    qe: u64,
    o: Option<u64>,
    o2: Option<u64>,
    qlen: Option<u64>,
    ring: Tail,
    prefix: Option<Vec<u8>>,
    head: Vec<u8>,
    head_on: bool,
    tail: Option<Vec<u8>>,
    suffix: Vec<u8>,
    suffix_on: bool,
    in_line: bool,
    line: u64,
    lh: Xxh3Default,
    trivial: bool,
    ring_wh: VecDeque<u16>,
    before: Option<Vec<u16>>,
    after: Vec<u16>,
    span: SpanHash,
}

impl core::fmt::Debug for Collector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Collector")
            .field("qs", &self.qs)
            .field("qe", &self.qe)
            .field("o", &self.o)
            .field("o2", &self.o2)
            .finish_non_exhaustive()
    }
}

impl Collector {
    fn new(at: Target, hint: (u64, u64)) -> Collector {
        Collector {
            qs: at.qs,
            qe: at.qe,
            o: at.o,
            o2: at.o2,
            qlen: at.qlen,
            ring: Tail::new(r14::CONTEXT_MAX.max(r14::QUOTE_DEFAULT)),
            prefix: None,
            head: Vec::new(),
            head_on: false,
            tail: None,
            suffix: Vec::new(),
            suffix_on: false,
            in_line: false,
            line: 0,
            lh: Xxh3Default::new(),
            trivial: true,
            ring_wh: VecDeque::with_capacity(WIN + 1),
            before: None,
            after: Vec::new(),
            span: SpanHash::new(hint.0, hint.1),
        }
    }

    /// The position `p` of N has been reached: take the snapshots that start there — the widened prefix
    /// `cuts(N[0 .. o), CONTEXT_MAX)` at o, and at o′ the quote's last `QUOTE_DEFAULT` bytes (a `range` end quote).
    // spec: [F20 §6.1] steps 3 and 5 (`end = cuts(ST, QUOTE_DEFAULT)`, `prefix = cuts(N[0 .. o), CTX)`)
    fn mark(&mut self, p: u64) {
        let mut tmp = Vec::new();
        if self.prefix.is_none() && self.o == Some(p) {
            self.ring
                .copy(p.saturating_sub(r14::CONTEXT_MAX as u64), p, &mut tmp);
            self.prefix = Some(tmp.clone());
            self.head_on = true;
        }
        if self.tail.is_none() && self.o2 == Some(p) {
            let from = p
                .saturating_sub(r14::QUOTE_DEFAULT as u64)
                .max(self.o.unwrap_or(0));
            self.ring.copy(from, p, &mut tmp);
            self.tail = Some(tmp);
            self.suffix_on = true;
        }
    }

    /// The bytes after o (the quote's first `QUOTE_MAX + 1`) and after o′ (the widened suffix and one more byte).
    // spec: [F20 §6.1] steps 3 and 5 (`exact = cutp(ST, ·)`, `suffix = cutp(N[o′ ..), CTX)`)
    fn take(&mut self, b: &[u8]) {
        if self.head_on && self.head.len() < HEAD {
            let n = (HEAD - self.head.len()).min(b.len());
            self.head.extend_from_slice(&b[..n]);
        }
        if self.suffix_on && self.suffix.len() <= r14::CONTEXT_MAX {
            let n = (r14::CONTEXT_MAX + 1 - self.suffix.len()).min(b.len());
            self.suffix.extend_from_slice(&b[..n]);
        }
    }

    /// The end of N: positions at its end take their snapshots.
    fn finish(&mut self, len: u64) {
        self.mark(len);
    }
}

impl NSink for Collector {
    // spec: [F20 §2.7.2] (`before(s)` at the quote span's first line), [F20 §6.1] step 5 (o = start(qs))
    fn start_line(&mut self, line: u64, at: u64) {
        self.span.start_line(line, at);
        self.line = line;
        self.in_line = true;
        self.lh = Xxh3Default::new();
        self.trivial = true;
        if line == self.qs {
            if self.o.is_none() {
                self.o = Some(at);
                if let Some(n) = self.qlen {
                    self.o2 = Some(at + n);
                }
            }
            self.before = Some(self.ring_wh.iter().copied().collect());
        }
        self.mark(at);
    }

    // spec: [F20 §2.5] "Trivial line", [F20 §2.7.1] (the line's hash input `nl(l)`)
    fn bytes(&mut self, at: u64, b: &[u8]) {
        self.span.bytes(at, b);
        if self.in_line {
            self.lh.update(b);
            if self.trivial {
                self.trivial = trivial_bytes(b);
            }
        }
        // Split at the snapshot positions inside the bytes.
        let end = at + b.len() as u64;
        let inside = |x: Option<u64>| x.filter(|&p| p > at && p < end).unwrap_or(end);
        let (c1, c2) = (inside(self.o), inside(self.o2));
        let mut p = at;
        for c in [c1.min(c2), c1.max(c2), end] {
            if c > p {
                let part = &b[(p - at) as usize..(c - at) as usize];
                self.mark(p);
                self.take(part);
                self.ring.push(p, part);
                p = c;
            }
        }
        self.mark(end);
    }

    // spec: [F20 §2.7.1] (`wh(l)` of a non-trivial line), [F20 §2.7.2] (`before(s)`, `after(e)`), [F20 §6.1] step 6
    fn end_line(&mut self, line: u64, at: u64) {
        self.span.end_line(line, at);
        self.in_line = false;
        if line == self.qe && self.o2.is_none() {
            self.o2 = Some(at);
            self.mark(at);
        }
        if !self.trivial {
            let wh = (self.lh.digest() & 0xFFFF) as u16;
            if line > self.qe && self.after.len() < WIN {
                self.after.push(wh);
            }
            self.ring_wh.push_back(wh);
            if self.ring_wh.len() > WIN {
                self.ring_wh.pop_front();
            }
        }
    }
}

// --- the capture ---------------------------------------------------------------------------------------------------

/// What the selectors of a span capture are built from.
struct Parts {
    kind: AnchorKind,
    quote: Vec<u8>,
    end: Option<Vec<u8>>,
    /// The widened context: `cuts(N[0 .. o), CONTEXT_MAX)` and the first `CONTEXT_MAX + 1` bytes after `o′`.
    prefix64: Vec<u8>,
    suffix65: Vec<u8>,
    o: u64,
    hint: LineSpan,
    window: Window,
    span_hash: u64,
    scope: Option<Scope>,
    /// N's line count m.
    m: u64,
}

/// Captures an anchor ([F20 §6.1]).
// spec: [F20 §6.1]
pub(crate) fn capture<S: ByteSource, B: Budget>(
    rd: &mut ContentReader,
    src: &mut S,
    input: &CaptureInput<'_>,
    budget: &mut B,
) -> Result<Captured, CaptureError> {
    let lifted = input.scanners == ScannerRule::Lifted;
    // The interim rule refuses the scanner forms before anything is read ([F20 §6.1]; [F21 §6.5]).
    if !lifted && matches!(input.form, Form::Symbol(_) | Form::Heading(_)) {
        return Err(Refusal::NoScanner.into());
    }
    let params = &input.read;
    let lang = if lifted {
        Lang::of_path(input.path)
    } else {
        None
    };
    match input.form {
        Form::File => {
            let (_, content) = read_first(rd, src, params, budget, (), None)?;
            let watch = input.watch.unwrap_or(Watch::Header);
            let anchor = file_anchor(input.file_uid, watch, content.oid, input.git, input.mode);
            Ok(Captured { anchor, content })
        }
        Form::Lines(span) => {
            let mut scan = lang.map(ScanFeed::new);
            let facts = SpanFacts {
                l: u64::from(span.first),
                m: u64::from(span.last),
                line: 0,
                start: 0,
                trivial: true,
                s: None,
                e: None,
                nontrivial: 0,
            };
            let (pass, content) = read_first(rd, src, params, budget, facts, scan.as_mut())?;
            if !content.is_text() {
                return Err(Refusal::Binary.into());
            }
            if span.first == 0 || span.first > span.last || u64::from(span.last) > pass.lines {
                return Err(Refusal::Range { lines: pass.lines }.into());
            }
            let items = scan.map(|s| s.finish(content.stats.last == Some(b'\n')));
            let f = pass.sink;
            let (l, m) = (u64::from(span.first), u64::from(span.last));
            let mut again = Again {
                rd,
                src,
                params,
                first: &content,
                budget,
            };
            let anchor = match (f.s, f.e) {
                (Some((s, os)), Some((e, oe))) => {
                    let kind = if f.nontrivial <= u64::from(r14::QUOTE_LINES)
                        && oe - os <= r14::QUOTE_MAX as u64
                    {
                        AnchorKind::Quote
                    } else {
                        AnchorKind::Range
                    };
                    let scope = items.as_ref().and_then(|it| capture_scope(it, s, e));
                    let at = Target {
                        qs: s,
                        qe: e,
                        o: Some(os),
                        o2: Some(oe),
                        qlen: None,
                    };
                    let c = collect(&mut again, at, (s, e))?;
                    let parts = span_parts(kind, &c, None, (s, e), scope);
                    finish_span(&mut again, input, parts, items, false)?
                }
                _ => {
                    // No non-trivial line in [L, M]: a `lines` anchor ([F20 §6.1] step 1).
                    let scope = items.as_ref().and_then(|it| capture_scope(it, l, m));
                    let at = Target {
                        qs: l,
                        qe: m,
                        o: None,
                        o2: None,
                        qlen: None,
                    };
                    let c = collect(&mut again, at, (l, m))?;
                    lines_anchor(input, &content, &c, (l, m), scope)?
                }
            };
            Ok(Captured { anchor, content })
        }
        Form::Quote { text, within } => {
            let q = quote_text(text)?;
            let pat = Pattern::new(&q);
            let mut scan = lang.map(ScanFeed::new);
            let find = FindFirst {
                search: pat.searcher(0),
                len: q.len() as u64,
                within,
                map: LineMap::default(),
                found: None,
                hits: Vec::new(),
            };
            let (pass, content) = read_first(rd, src, params, budget, find, scan.as_mut())?;
            if !content.is_text() {
                return Err(Refusal::Binary.into());
            }
            let Some((h, s, e)) = pass.sink.found else {
                return Err(Refusal::NotFound.into());
            };
            let items = scan.map(|s| s.finish(content.stats.last == Some(b'\n')));
            let o2 = h + q.len() as u64;
            let at = Target {
                qs: s,
                qe: e,
                o: Some(h),
                o2: Some(o2),
                qlen: None,
            };
            let mut again = Again {
                rd,
                src,
                params,
                first: &content,
                budget,
            };
            let c = collect(&mut again, at, (s, e))?;
            // Step 2 with the text as the span text: its own non-trivial lines and its length (WP-64 reading; a text
            // that starts or ends inside a line is judged by its own bytes there).
            let nontrivial = lines(&q).filter(|l| !trivial_bytes(l)).count() as u64;
            let kind = if nontrivial <= u64::from(r14::QUOTE_LINES) && q.len() <= r14::QUOTE_MAX {
                AnchorKind::Quote
            } else {
                AnchorKind::Range
            };
            let parts = span_parts(kind, &c, Some(&q), (s, e), None);
            let anchor = finish_span(&mut again, input, parts, items, true)?;
            Ok(Captured { anchor, content })
        }
        Form::Symbol(sel) | Form::Heading(sel) => {
            let fk = if matches!(input.form, Form::Symbol(_)) {
                FormKind::Symbol
            } else {
                FormKind::Heading
            };
            let selector = Selector::parse(fk, sel).map_err(Refusal::Syntax)?;
            let Some(lang) = lang else {
                return Err(Refusal::NoScope.into());
            };
            let mut scan = Some(ScanFeed::new(lang));
            let (_, content) = read_first(rd, src, params, budget, (), scan.as_mut())?;
            if !content.is_text() {
                return Err(Refusal::Binary.into());
            }
            let items = scan.map(|s| s.finish(content.stats.last == Some(b'\n')));
            let scanned: Result<Items, ScanFailed> = items.unwrap_or(Err(ScanFailed));
            let (item_start, item_end, scope) = match find_form(&scanned, &selector) {
                FormOutcome::Found(i) => {
                    let it = scanned.as_ref().map_err(|_| Refusal::ScanFailed)?;
                    let y = it.get(i).ok_or(Refusal::NoScope)?;
                    (y.start, y.end, it.scope_of(i))
                }
                FormOutcome::NotFound => return Err(Refusal::NoScope.into()),
                FormOutcome::Several(v) => {
                    let names = scanned
                        .as_ref()
                        .map(|it| v.iter().map(|&i| it.path_text(i)).collect())
                        .unwrap_or_default();
                    return Err(Refusal::Several(names).into());
                }
                FormOutcome::NotRecordable(i) => {
                    let name = scanned
                        .as_ref()
                        .map(|it| it.path_text(i))
                        .unwrap_or_default();
                    return Err(Refusal::NotRecordable(name).into());
                }
                FormOutcome::FailedScan => return Err(Refusal::ScanFailed.into()),
            };
            let hk = if lang == Lang::Markdown {
                HeaderKind::Heading
            } else {
                HeaderKind::Symbol
            };
            let mut again = Again {
                rd,
                src,
                params,
                first: &content,
                budget,
            };
            // Read 2: the header of the item's header line ([F20 §2.8]).
            let mut acc = again.read(0, || HeaderAcc::new(item_start, hk))?.sink;
            acc.finish();
            let Some(head) = acc.header() else {
                return Err(Refusal::NoScope.into());
            };
            let exact = cutp(head.head(), r14::QUOTE_MAX).to_vec();
            let header_hash = head.hash;
            if exact.is_empty() {
                return Err(Refusal::NoScope.into());
            }
            let m = exact.iter().filter(|&&b| b == b'\n').count() as u64;
            let (qs, qe) = (item_start, item_start + m);
            let kind = if fk == FormKind::Symbol {
                AnchorKind::Symbol
            } else {
                AnchorKind::Heading
            };
            let at = Target {
                qs,
                qe,
                o: None,
                o2: None,
                qlen: Some(exact.len() as u64),
            };
            let c = collect(&mut again, at, (item_start, item_end))?;
            let mut parts = span_parts(kind, &c, Some(&exact), (qs, qe), scope);
            parts.hint = LineSpan::of(item_start, item_end);
            let watch = input.watch.unwrap_or(Watch::Header);
            if watch == Watch::Header {
                parts.span_hash = header_hash;
            }
            let anchor = finish_span(&mut again, input, parts, Some(scanned), false)?;
            Ok(Captured { anchor, content })
        }
    }
}

/// [F21 §2.4] for a quote span: the scope recorded, as a scope value.
// spec: [F21 §2.4], [F20 §6.1] (the scope of a `path:L-M` form once the interim rule is lifted)
fn capture_scope(items: &Result<Items, ScanFailed>, qs: u64, qe: u64) -> Option<Scope> {
    items.as_ref().ok()?.capture_scope(qs, qe)
}

/// What read 2 collected.
struct Collected {
    prefix64: Vec<u8>,
    head: Vec<u8>,
    tail: Vec<u8>,
    suffix65: Vec<u8>,
    window: Window,
    span_hash: u64,
    o: u64,
    o2: u64,
    /// N's line count m.
    m: u64,
}

/// Read 2 of a span capture ([`Collector`]).
// spec: [F20 §6.1] steps 5–7, 9
fn collect<S: ByteSource, B: Budget>(
    again: &mut Again<'_, S, B>,
    at: Target,
    hint: (u64, u64),
) -> Result<Collected, CaptureError> {
    let p = again.read(0, || Collector::new(at, hint))?;
    let mut c = p.sink;
    c.finish(p.len);
    let window = Window::new(c.before.as_deref().unwrap_or(&[]), &c.after).unwrap_or(Window::EMPTY);
    Ok(Collected {
        prefix64: c.prefix.unwrap_or_default(),
        head: c.head,
        tail: c.tail.unwrap_or_default(),
        suffix65: c.suffix,
        window,
        span_hash: c.span.hash().unwrap_or(0),
        o: c.o.unwrap_or(0),
        o2: c.o2.unwrap_or(0),
        m: p.lines,
    })
}

/// The quote texts of a span capture ([F20 §6.1] step 3): a `quote` is the span text (or the input text); a
/// `range` keeps `cutp(·, QUOTE_DEFAULT)` and `cuts(·, QUOTE_DEFAULT)`; a header quote is given whole.
// spec: [F20 §6.1] steps 2–3, 6, 7, 9
fn span_parts(
    kind: AnchorKind,
    c: &Collected,
    text: Option<&[u8]>,
    (qs, qe): (u64, u64),
    scope: Option<Scope>,
) -> Parts {
    // `N[o .. o')`, as far as the head holds it: all of a `quote` span, which is at most QUOTE_MAX bytes.
    let st = &c.head[..c.head.len().min(c.o2.saturating_sub(c.o) as usize)];
    let (quote, end) = match (kind, text) {
        (AnchorKind::Range, Some(t)) => (
            cutp(t, r14::QUOTE_DEFAULT).to_vec(),
            Some(cuts(t, r14::QUOTE_DEFAULT).to_vec()),
        ),
        (AnchorKind::Range, None) => (
            cutp(st, r14::QUOTE_DEFAULT).to_vec(),
            Some(cuts(&c.tail, r14::QUOTE_DEFAULT).to_vec()),
        ),
        (_, Some(t)) => (t.to_vec(), None),
        (_, None) => (st.to_vec(), None),
    };
    Parts {
        kind,
        quote,
        end,
        prefix64: c.prefix64.clone(),
        suffix65: c.suffix65.clone(),
        o: c.o,
        hint: LineSpan::of(qs, qe),
        window: c.window,
        span_hash: c.span_hash,
        scope,
        m: c.m,
    }
}

/// A `lines` anchor ([F20 §6.1] step 1): hint `[L, M]` and the window around it, which must not be empty.
// spec: [F20 §6.1] step 1, [F18 §2.9] (a `lines` capture with an empty window is refused)
fn lines_anchor(
    input: &CaptureInput<'_>,
    content: &Content,
    c: &Collected,
    (l, m): (u64, u64),
    scope: Option<Scope>,
) -> Result<Anchor, CaptureError> {
    let watch = input.watch.unwrap_or(Watch::Span);
    if watch == Watch::Header {
        return Err(Refusal::HeaderWatch.into());
    }
    if c.window.is_empty() {
        return Err(Refusal::NoWindow.into());
    }
    let wbytes = c.window.to_bytes();
    let scope_bytes = scope.as_ref().map(|s| s.as_bytes().to_vec());
    let cap = Capture {
        file_uid: input.file_uid,
        kind: AnchorKind::Lines,
        scope: scope_bytes.as_deref().unwrap_or(&[]),
        quote: &[],
        prefix: &[],
        suffix: &[],
        end: &[],
        occurrence: None,
        window: &wbytes,
    };
    let anchor = Anchor {
        kind: AnchorKind::Lines,
        mode: input.mode,
        watch,
        resolver: r14::RESOLVER_VERSION,
        captured: captured(&cap).map_err(|_| Unavailable::Size)?,
        pred: None,
        hint: Some(LineSpan::of(l, m)),
        scope: scope_bytes,
        texts: None,
        occurrence: None,
        window: Some(c.window),
        span_hash: Some(c.span_hash),
        blob: content.oid,
        git: input.git,
        marker: None,
    };
    Ok(anchor)
}

/// Rung 3 of the uniqueness ladder ([F20 §6.1] step 8.3): the captured hit's 1-based index among the region's hits.
/// The captured position is always a hit of the exact step on the captured content; when it is not, the capture is an
/// internal error rather than an anchor that is neither unique nor has an occurrence. An index above `u16` is refused.
// spec: [F20 §6.1] step 8.3, [F08 §10.3] (`occurrence` is a `u16`, ≥ 1)
fn rung3(ex: &ExactSink<'_>, set: Set) -> Result<NonZeroU16, CaptureError> {
    let i = ex.captured_index(set).ok_or(CaptureError::Internal)?;
    u16::try_from(i)
        .ok()
        .and_then(NonZeroU16::new)
        .ok_or(CaptureError::Refused(Refusal::Occurrence))
}

/// The uniqueness ladder and the record ([F20 §6.1] steps 5, 8 and 9).
// spec: [F20 §6.1] steps 5, 8, 9 (the uniqueness ladder, `captured`)
fn finish_span<S: ByteSource, B: Budget>(
    again: &mut Again<'_, S, B>,
    input: &CaptureInput<'_>,
    parts: Parts,
    items: Option<Result<Items, ScanFailed>>,
    quote_input: bool,
) -> Result<Anchor, CaptureError> {
    let watch = input.watch.unwrap_or(Watch::default_for(parts.kind));
    if watch == Watch::Header && !matches!(parts.kind, AnchorKind::Symbol | AnchorKind::Heading) {
        return Err(Refusal::HeaderWatch.into());
    }
    // Step 5: context at CONTEXT, and the widened CONTEXT_MAX of rung 1.
    let p32 = cuts(&parts.prefix64, r14::CONTEXT).to_vec();
    let s32 = cutp(&parts.suffix65, r14::CONTEXT).to_vec();
    let p64 = cuts(&parts.prefix64, r14::CONTEXT_MAX).to_vec();
    let s64 = cutp(&parts.suffix65, r14::CONTEXT_MAX).to_vec();

    // The scope the search region uses: recorded from the start (a `path:L-M`, `symbol` or `heading` form once the
    // interim rule is lifted), or the rung-2 candidate of a quote input.
    let rung2 = if quote_input && parts.scope.is_none() {
        items.as_ref().and_then(|it| {
            capture_scope(it, u64::from(parts.hint.first), u64::from(parts.hint.last))
        })
    } else {
        None
    };
    let region_scope = parts.scope.as_ref().or(rung2.as_ref());
    let region = match (region_scope, &items) {
        (Some(sc), Some(Ok(it))) => it.resolve(sc).map(|y| (y.start, y.end)),
        _ => None,
    };

    // Step 8: the exact step on the captured content, with no occurrence.
    let qp = Pattern::new(&parts.quote);
    let ep = parts.end.as_deref().map(Pattern::new);
    let contexts = [
        Context {
            prefix: &p32,
            suffix: &s32,
        },
        Context {
            prefix: &p64,
            suffix: &s64,
        },
    ];
    let first = again.first;
    let lines_ok = first.line_hashes.as_ref().filter(|l| l.is_complete());
    let spec = ExactSpec {
        quote: &qp,
        quote_last: parts.quote.last().copied().unwrap_or(0),
        end: ep
            .as_ref()
            .zip(parts.end.as_deref().and_then(|e| e.last().copied()))
            .map(|(pattern, last)| RangeEnd {
                pattern,
                last,
                lines: parts.hint.len(),
                m: parts.m,
            }),
        contexts: &contexts,
        window: &parts.window,
        lines: lines_ok,
        region,
        occurrence: None,
        captured_at: Some(parts.o),
    };
    let mut ex = again.read(spec.held(), || ExactSink::new(spec))?.sink;
    ex.finish();
    let o = parts.o;
    let unique = |set: Set, w: usize| matches!(ex.decide(set, w, false), Exact::Hit(h) if h.h == o);
    let base = if parts.scope.is_some() {
        Set::Scope
    } else {
        Set::Whole
    };
    let mut scope = parts.scope;
    let (wide, occurrence) = if unique(base, 0) {
        (false, None)
    } else if unique(base, 1) {
        (true, None)
    } else if let Some(sc) = rung2 {
        // Rung 2: the enclosing scope (quote input only, once the interim rule is lifted).
        scope = Some(sc);
        if unique(Set::Scope, 1) {
            (true, None)
        } else {
            (true, Some(rung3(&ex, Set::Scope)?))
        }
    } else {
        (true, Some(rung3(&ex, base)?))
    };
    let (prefix, suffix) = if wide { (p64, s64) } else { (p32, s32) };

    // Step 9: `captured` and the record.
    let scope_bytes = scope.as_ref().map(|s| s.as_bytes().to_vec());
    let cap = Capture {
        file_uid: input.file_uid,
        kind: parts.kind,
        scope: scope_bytes.as_deref().unwrap_or(&[]),
        quote: &parts.quote,
        prefix: &prefix,
        suffix: &suffix,
        end: parts.end.as_deref().unwrap_or(&[]),
        occurrence,
        window: &[],
    };
    let anchor = Anchor {
        kind: parts.kind,
        mode: input.mode,
        watch,
        resolver: r14::RESOLVER_VERSION,
        captured: captured(&cap).map_err(|_| Unavailable::Size)?,
        pred: None,
        hint: Some(parts.hint),
        scope: scope_bytes,
        texts: Some(Texts::Held {
            quote: parts.quote,
            prefix,
            suffix,
            end: parts.end,
        }),
        occurrence,
        window: Some(parts.window),
        span_hash: Some(parts.span_hash),
        blob: first.oid,
        git: input.git,
        marker: None,
    };
    Ok(anchor)
}
