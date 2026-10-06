//! The anchor cascade ([F20 §6.2]–§6.6; [40 §4.5]): hint, exact quote, fuzzy quote, scope only, `lines` anchors,
//! `text-unavailable` anchors and the watch rules, over the current content as the caller's [`ByteSource`] supplies
//! it, read in passes that each stream N.

use std::collections::VecDeque;

use moirai_diff::Pattern;
use xxhash_rust::xxh3::Xxh3Default;

use super::exact::{Context, Exact, ExactSink, ExactSpec, RangeEnd, Set};
use super::fuzzy::{Fuzzy, FuzzyEnd, FuzzySink, FuzzySpec};
use super::header::{HeaderAcc, HeaderKind};
use super::nstream::{Both, Meter, Metered, NSink, NStream};
use super::score::{at_least, unique_by};
use super::window::{WIN, Window};
use super::{
    Anchor, AnchorKind, Budget, LineSpan, Mode, ReadParams, ScannerRule, Score, Texts, Watch,
};
use crate::oid::{Tri, oid_in};
use crate::r14;
use crate::scan::{Items, Lang, ScanFailed, Scanner, Scope};
use crate::text::{
    ByteSource, Content, ContentReader, LineHashes, LineSink, ReadOptions, Unavailable,
};

/// The state of one anchor ([F18 §4.3]), with its code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnchorState {
    /// 1 `fresh`: the span hash matches at the hint (for `header` watch, the header).
    Fresh = 1,
    /// 2 `moved`: identical text, unique, found elsewhere.
    Moved = 2,
    /// 3 `edited`: a fuzzy match, a changed span under `span` watch, or only the scope survived.
    Edited = 3,
    /// 4 `ambiguous`: two or more candidates within the margin.
    Ambiguous = 4,
    /// 5 `orphaned`: nothing found.
    Orphaned = 5,
    /// 6 `unverified`: an input was unavailable.
    Unverified = 6,
    /// 7 `unresolved`: the cascade did not run (a `pinned` anchor; LQ only, never stored).
    Unresolved = 7,
}

impl AnchorState {
    /// The frozen string ([F18 §4.3]).
    #[must_use]
    // spec: [F18 §4.3]
    pub const fn as_str(self) -> &'static str {
        match self {
            AnchorState::Fresh => "fresh",
            AnchorState::Moved => "moved",
            AnchorState::Edited => "edited",
            AnchorState::Ambiguous => "ambiguous",
            AnchorState::Orphaned => "orphaned",
            AnchorState::Unverified => "unverified",
            AnchorState::Unresolved => "unresolved",
        }
    }
}

/// Why an anchor is `unverified` ([F20 §1.5]; [F18 §4.6] codes 53–61).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnverifiedReason {
    /// An unavailable input; `unstable` renders `budget` ([F20 §1.5]).
    Unavailable(Unavailable),
    /// A `file` anchor with `span` watch whose `blob` has another algorithm than the current content (code 61).
    OidAlgorithm,
}

/// The result of the cascade for one anchor ([F20 §6.6]: a state and, for `fresh`, `moved` and `edited`, a span).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// The anchor state.
    pub state: AnchorState,
    /// The lines the anchor resolved to: its quote span (for `symbol` and `heading`, from the header line over the
    /// hint's length), or the scope's range for `edited (scope only)`.
    pub span: Option<LineSpan>,
    /// The fuzzy score, when the fuzzy step decided (`edited <score>`, [F18 §4.6] code 62).
    pub score: Option<Score>,
    /// `edited (scope only)` ([F18 §4.6] code 63).
    pub scope_only: bool,
    /// Why the anchor is `unverified`.
    pub reason: Option<UnverifiedReason>,
}

impl Resolution {
    const fn of(state: AnchorState) -> Resolution {
        Resolution {
            state,
            span: None,
            score: None,
            scope_only: false,
            reason: None,
        }
    }

    const fn at(state: AnchorState, span: LineSpan) -> Resolution {
        Resolution {
            span: Some(span),
            ..Resolution::of(state)
        }
    }

    const fn unverified(reason: UnverifiedReason) -> Resolution {
        Resolution {
            reason: Some(reason),
            ..Resolution::of(AnchorState::Unverified)
        }
    }

    const fn unavailable(r: Unavailable) -> Resolution {
        Resolution::unverified(UnverifiedReason::Unavailable(r))
    }
}

/// The inputs of a resolution besides the anchor and the content.
#[derive(Clone, Copy, Debug)]
pub struct ResolveInput<'a> {
    /// The file's current root-relative path: its last component decides the scanner's language ([F21 §1.3]).
    pub path: &'a [u8],
    /// The parameters of the reads.
    pub read: ReadParams,
    /// [`ScannerRule::CURRENT`].
    pub scanners: ScannerRule,
}

// --- sinks ---------------------------------------------------------------------------------------------------------

/// `XXH3-64(ST(first, last))` as N streams ([F20 §2.8]), with the byte range in N of the lines of `[first, last]` that
/// exist ([F20 §6.4] region R1).
#[derive(Clone)]
pub(crate) struct SpanHash {
    first: u64,
    last: u64,
    h: Xxh3Default,
    active: bool,
    hash: Option<u64>,
    a: Option<u64>,
    z: Option<u64>,
}

impl core::fmt::Debug for SpanHash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SpanHash")
            .field("first", &self.first)
            .field("last", &self.last)
            .field("hash", &self.hash)
            .finish_non_exhaustive()
    }
}

impl SpanHash {
    pub(crate) fn new(first: u64, last: u64) -> SpanHash {
        SpanHash {
            first,
            last,
            h: Xxh3Default::new(),
            active: false,
            hash: None,
            a: None,
            z: None,
        }
    }

    /// The hash, once line `last` has ended.
    pub(crate) fn hash(&self) -> Option<u64> {
        self.hash
    }

    /// `[start(first) .. end(min(last, m)))` when line `first` exists.
    pub(crate) fn range(&self) -> Option<(u64, u64)> {
        Some((self.a?, self.z?))
    }
}

impl NSink for SpanHash {
    // spec: [F20 §2.8] (`span_hash` = XXH3-64(ST(h1, h2))), [F20 §6.4] (R1: the byte range of the hint lines)
    fn start_line(&mut self, line: u64, at: u64) {
        if line == self.first {
            self.active = true;
            self.a = Some(at);
        }
    }

    // spec: [F20 §2.5] `ST(s, e)`, [F20 §2.8]
    fn bytes(&mut self, _at: u64, b: &[u8]) {
        if self.active {
            self.h.update(b);
        }
    }

    // spec: [F20 §2.8] (the hash over the hint lines, once line h2 has ended)
    fn end_line(&mut self, line: u64, at: u64) {
        if self.active {
            self.z = Some(at);
            if line == self.last {
                self.hash = Some(self.h.digest());
                self.active = false;
            }
        }
    }
}

/// `start(ys)` and `end(ye)`: the byte range of a scope's lines in N ([F21 §2.5]).
#[derive(Clone, Copy, Debug)]
struct LineBounds {
    ys: u64,
    ye: u64,
    a: Option<u64>,
    z: Option<u64>,
}

impl NSink for LineBounds {
    // spec: [F21 §2.5] (the scope's byte range `[start(ys) .. end(ye))`), [F20 §6.2] step 3
    fn start_line(&mut self, line: u64, at: u64) {
        if line == self.ys {
            self.a = Some(at);
        }
    }
    fn bytes(&mut self, _at: u64, _b: &[u8]) {}
    fn end_line(&mut self, line: u64, at: u64) {
        if line == self.ye {
            self.z = Some(at);
        }
    }
}

/// The byte ranges in N of the headers of the items that start on `lines` ([F21 §2.6]: `[start(start(y)) ..
/// start(start(y)) + len(header))`).
#[derive(Clone, Debug)]
struct HeaderRanges {
    lines: Vec<u64>,
    kind: HeaderKind,
    next: usize,
    active: Vec<(u64, HeaderAcc)>,
    done: Vec<(u64, u64)>,
}

impl HeaderRanges {
    fn new(mut lines: Vec<u64>, kind: HeaderKind) -> HeaderRanges {
        lines.sort_unstable();
        lines.dedup();
        HeaderRanges {
            lines,
            kind,
            next: 0,
            active: Vec::new(),
            done: Vec::new(),
        }
    }

    // spec: [F21 §2.6] (an item header's range `[start(start(y)) .. start(start(y)) + len(header))`)
    fn collect(&mut self) {
        let mut i = 0;
        while i < self.active.len() {
            if let Some(h) = self.active[i].1.header() {
                let a = self.active[i].0;
                self.done.push((a, a + h.len));
                self.active.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn finish(mut self) -> Vec<(u64, u64)> {
        for (_, acc) in &mut self.active {
            acc.finish();
        }
        self.collect();
        let mut out = self.done;
        out.sort_unstable();
        out
    }
}

impl NSink for HeaderRanges {
    fn start_line(&mut self, line: u64, at: u64) {
        if self.lines.get(self.next) == Some(&line) {
            self.next += 1;
            self.active.push((at, HeaderAcc::new(line, self.kind)));
        }
        for (_, acc) in &mut self.active {
            acc.start_line(line, at);
        }
    }

    fn bytes(&mut self, at: u64, b: &[u8]) {
        for (_, acc) in &mut self.active {
            acc.bytes(at, b);
        }
    }

    fn end_line(&mut self, line: u64, at: u64) {
        for (_, acc) in &mut self.active {
            acc.end_line(line, at);
        }
        self.collect();
    }

    // spec: [F20 §1.3] input (e) (the open headers' windows, charged as they grow)
    fn retained(&self) -> usize {
        self.active.iter().fold(
            self.active.capacity() * size_of::<(u64, HeaderAcc)>()
                + self.done.capacity() * size_of::<(u64, u64)>(),
            |n, (_, acc)| n.saturating_add(acc.retained()),
        )
    }
}

/// The scope scanner fed the raw anchor text t ([F21 §1.3]) from the reader's lines: each line's `0A` is written when
/// the next line starts, and the last one only when the content ends with it.
pub(crate) struct ScanFeed {
    lang: Lang,
    scanner: Scanner,
    pending_lf: bool,
}

impl ScanFeed {
    pub(crate) fn new(lang: Lang) -> ScanFeed {
        ScanFeed {
            lang,
            scanner: Scanner::new(lang),
            pending_lf: false,
        }
    }

    /// The items of t; `ends_with_lf` says whether the content's last byte is `0A`.
    pub(crate) fn finish(mut self, ends_with_lf: bool) -> Result<Items, ScanFailed> {
        if self.pending_lf && ends_with_lf {
            self.scanner.feed(b"\n");
        }
        self.scanner.finish()
    }
}

impl LineSink for ScanFeed {
    fn begin(&mut self) {
        self.scanner = Scanner::new(self.lang);
        self.pending_lf = false;
    }

    // spec: [F21 §1.3] (the scanners read the raw anchor text t, its line feeds included)
    fn piece(&mut self, bytes: &[u8]) {
        if self.pending_lf {
            self.pending_lf = false;
            self.scanner.feed(b"\n");
        }
        self.scanner.feed(bytes);
    }

    fn end_line(&mut self) {
        if self.pending_lf {
            self.scanner.feed(b"\n");
        }
        self.pending_lf = true;
    }
}

// --- reads ---------------------------------------------------------------------------------------------------------

/// What one pass yields: the sink and the number of lines and bytes of N.
pub(crate) struct Pass<K> {
    pub(crate) sink: K,
    pub(crate) lines: u64,
    pub(crate) len: u64,
}

/// The first read of a call: statistics, `oid`, the line-hash array, N into `sink` and, when given, t into a scanner.
/// The read is charged to the budget before it starts, with the size the handle reports ([F20 §1.3] input (e)); a size
/// above `files.max-read-bytes` is `Unavailable(size)` first ([F20 §2.4] item 1). A sink of the first read keeps no
/// tail of N: a hash state, a line map or a header's window (module `header`), whose growth the pass charges as it
/// grows ([`Metered`]).
// spec: [F20 §2.4] (the two-pass read), [F20 §1.3] input (e) (charged before the pass and as the sinks grow)
pub(crate) fn read_first<S: ByteSource, K: NSink, B: Budget>(
    rd: &mut ContentReader,
    src: &mut S,
    params: &ReadParams,
    budget: &mut B,
    sink: K,
    scan: Option<&mut ScanFeed>,
) -> Result<(Pass<K>, Content), Unavailable> {
    let size = src.snapshot().map_err(|_| Unavailable::Unreadable)?.size;
    if size > params.max_read_bytes {
        return Err(Unavailable::Size);
    }
    if !budget.spend(size) {
        return Err(Unavailable::Budget);
    }
    let opts = ReadOptions {
        format: params.format,
        max_read_bytes: params.max_read_bytes,
        max_line_hashes: Some(params.max_line_hashes),
    };
    let meter = Meter::new(budget);
    let mut ns = NStream::new(Metered::new(sink, &meter));
    let content = match scan {
        Some(sf) => rd.read(src, &opts, &mut Both(&mut ns, sf)),
        None => rd.read(src, &opts, &mut ns),
    }
    .map_err(|e| e.reason())?;
    let (lines, len) = (ns.lines(), ns.len());
    let sink = ns.into_sink().into_inner().ok_or(Unavailable::Budget)?;
    Ok((Pass { sink, lines, len }, content))
}

/// A later read of the same call: N into `sink`, in one pass that must see the first read's bytes
/// ([`ContentReader::reread`]; [F20 §2.4]), else `Unavailable(unstable)`; the sink's growth is charged to `budget` as
/// it grows ([`Metered`]), else `Unavailable(budget)`.
// spec: [F20 §2.4] (every read of one resolution sees the same bytes; else `unstable`), [F20 §1.3] input (e)
fn read_again<S: ByteSource, K: NSink>(
    rd: &mut ContentReader,
    src: &mut S,
    params: &ReadParams,
    first: &Content,
    sink: K,
    budget: &mut dyn Budget,
) -> Result<Pass<K>, Unavailable> {
    let opts = ReadOptions {
        format: params.format,
        max_read_bytes: params.max_read_bytes,
        max_line_hashes: None,
    };
    let meter = Meter::new(budget);
    let mut ns = NStream::new(Metered::new(sink, &meter));
    rd.reread(src, &opts, first, &mut ns)
        .map_err(|e| e.reason())?;
    let (lines, len) = (ns.lines(), ns.len());
    let sink = ns.into_sink().into_inner().ok_or(Unavailable::Budget)?;
    Ok(Pass { sink, lines, len })
}

/// The reads of a call after its first: each must see the first read's bytes, and each is charged to the budget.
pub(crate) struct Again<'x, S, B> {
    pub(crate) rd: &'x mut ContentReader,
    pub(crate) src: &'x mut S,
    pub(crate) params: &'x ReadParams,
    pub(crate) first: &'x Content,
    pub(crate) budget: &'x mut B,
}

impl<S: ByteSource, B: Budget> Again<'_, S, B> {
    /// A read into the sink `make` builds, charged before the sink is built and the pass starts — the content's length
    /// and the `held` bytes of the sink's fixed part — and, during the pass, with what the sink holds and searches
    /// beyond it as that grows ([F20 §1.3] input (e)).
    // spec: [F20 §1.3] input (e) (budgets charged before each pass and as its sinks grow), [F20 §2.4]
    pub(crate) fn read<K: NSink>(
        &mut self,
        held: usize,
        make: impl FnOnce() -> K,
    ) -> Result<Pass<K>, Unavailable> {
        if !self
            .budget
            .spend(self.first.raw_len().saturating_add(held as u64))
        {
            return Err(Unavailable::Budget);
        }
        read_again(
            self.rd,
            self.src,
            self.params,
            self.first,
            make(),
            &mut *self.budget,
        )
    }

    /// The hash of the watched text at lines `[first, last]`: `ST` for `span` watch, the header of line `first` for
    /// `header` watch ([F20 §2.8]); `None` when the anchor watches a header it has none of, or the lines do not
    /// exist.
    // spec: [F20 §2.8], [F20 §6.5] "Watch"
    fn watched(
        &mut self,
        watch: Watch,
        hkind: Option<HeaderKind>,
        (first, last): (u64, u64),
    ) -> Result<Option<u64>, Unavailable> {
        match (watch, hkind) {
            (Watch::Span, _) => Ok(self.read(0, || SpanHash::new(first, last))?.sink.hash()),
            (Watch::Header, Some(k)) => {
                let mut acc = self.read(0, || HeaderAcc::new(first, k))?.sink;
                acc.finish();
                Ok(acc.header().map(|h| h.hash))
            }
            (Watch::Header, None) => Ok(None),
        }
    }
}

/// The header kind of an anchor kind ([F20 §2.8]): `symbol` and `heading` have one; the other kinds have no item.
// spec: [F20 §2.8], [F21 §2.1]
pub(crate) const fn header_kind(kind: AnchorKind) -> Option<HeaderKind> {
    match kind {
        AnchorKind::Symbol => Some(HeaderKind::Symbol),
        AnchorKind::Heading => Some(HeaderKind::Heading),
        _ => None,
    }
}

// --- the cascade ---------------------------------------------------------------------------------------------------

/// `k = ⌊FUZZY_BUDGET × len⌋` ([F20 §6.4]).
// spec: [F20 §6.4] "Error budget"
fn budget_k(len: usize) -> usize {
    let b = r14::FUZZY_BUDGET;
    (len as u128 * u128::from(b.num()) / u128::from(b.den())) as usize
}

/// The cascade ([F20 §6.2]–§6.5).
// spec: [F20 §6.2], [F20 §6.4], [F20 §6.5], [40 §4.5]
pub(crate) fn resolve<S: ByteSource, B: Budget>(
    rd: &mut ContentReader,
    src: &mut S,
    a: &Anchor,
    input: &ResolveInput<'_>,
    budget: &mut B,
) -> Resolution {
    // [F18 §4.4]: the cascade does not run for a pinned anchor.
    if a.mode == Mode::Pinned {
        return Resolution::of(AnchorState::Unresolved);
    }
    // A header-watched `file` anchor follows the file ([F20 §6.5] "Watch"): its content is no input, so it is not read.
    if a.kind == AnchorKind::File && a.watch == Watch::Header {
        return Resolution::of(AnchorState::Fresh);
    }
    match cascade(rd, src, a, input, budget) {
        Ok(r) => r,
        Err(u) => Resolution::unavailable(u),
    }
}

// spec: [F20 §6.2]–§6.5
fn cascade<S: ByteSource, B: Budget>(
    rd: &mut ContentReader,
    src: &mut S,
    a: &Anchor,
    input: &ResolveInput<'_>,
    budget: &mut B,
) -> Result<Resolution, Unavailable> {
    let params = &input.read;
    let lifted = input.scanners == ScannerRule::Lifted;
    let hkind = header_kind(a.kind);
    let scope = if lifted {
        a.scope.as_deref().and_then(|b| Scope::from_bytes(b).ok())
    } else {
        None
    };
    let lang = Lang::of_path(input.path);
    let mut scan = match (&scope, lang) {
        (Some(_), Some(l)) => Some(ScanFeed::new(l)),
        _ => None,
    };

    // Read 1: the content, the hint lines' hash and range, the scan.
    // A hint that breaks [F08 §10.3]'s validity is no hint: the record is a decoder's to refuse.
    let hint = a.hint.filter(LineSpan::is_valid);
    let first_sink = (
        hint.map(|h| SpanHash::new(u64::from(h.first), u64::from(h.last))),
        hint.zip(hkind)
            .filter(|_| a.watch == Watch::Header)
            .map(|(h, k)| HeaderAcc::new(u64::from(h.first), k)),
    );
    let (pass, content) = read_first(rd, src, params, budget, first_sink, scan.as_mut())?;
    let mut again = Again {
        rd,
        src,
        params,
        first: &content,
        budget,
    };

    // `file` anchors: the watch rule ([F20 §6.5] "Watch").
    if a.kind == AnchorKind::File {
        return Ok(file_anchor(a, &content));
    }
    // Binary current content: every span anchor is orphaned ([F20 §6.5]).
    if !content.is_text() {
        return Ok(Resolution::of(AnchorState::Orphaned));
    }

    // Step 1: the hint ([F20 §6.2] step 1).
    let (span_sink, mut head_sink) = pass.sink;
    if let Some(head) = &mut head_sink {
        head.finish();
    }
    if let (Some(h), Some(sh)) = (hint, a.span_hash)
        && u64::from(h.last) <= pass.lines
    {
        let got = match (a.watch, &head_sink) {
            (Watch::Span, _) => span_sink.as_ref().and_then(SpanHash::hash),
            (Watch::Header, Some(acc)) => acc.header().map(|h| h.hash),
            (Watch::Header, None) => None,
        };
        if got == Some(sh) {
            return Ok(Resolution::at(AnchorState::Fresh, h));
        }
    }
    // Step 2, the opt-in marker, has no in-file syntax in resolver version 1 and decides nothing (spec finding).

    // The scope ([F20 §6.2] step 3; [F21 §2.5]); only once the interim rule is lifted.
    let items = scan.map(|s| s.finish(content.stats.last == Some(b'\n')));
    let scope_lines = match (&scope, &items) {
        (Some(sc), Some(Ok(it))) if it.lang() == sc.lang() => {
            it.resolve(sc).map(|y| (y.start, y.end))
        }
        _ => None,
    };
    let lines_ok = content.line_hashes.as_ref().filter(|l| l.is_complete());

    if a.kind == AnchorKind::Lines {
        return lines_anchor(&mut again, a, lines_ok);
    }
    let (quote, prefix, suffix, end) = match &a.texts {
        Some(Texts::Held {
            quote,
            prefix,
            suffix,
            end,
        }) => (quote, prefix, suffix, end),
        Some(Texts::Digests { .. }) => {
            return text_unavailable(&mut again, a, lines_ok, scope_lines);
        }
        None => return Ok(Resolution::of(AnchorState::Orphaned)),
    };
    let end = match (a.kind, end) {
        (AnchorKind::Range, Some(e)) if !e.is_empty() => Some(e.as_slice()),
        (AnchorKind::Range, _) => return Ok(Resolution::of(AnchorState::Orphaned)),
        _ => None,
    };
    let Some(&quote_last) = quote.last() else {
        return Ok(Resolution::of(AnchorState::Orphaned));
    };
    if !texts_in_bounds(quote, prefix, suffix, end) {
        return Ok(Resolution::unavailable(Unavailable::Budget));
    }
    let window = a.window.unwrap_or(Window::EMPTY);
    let hint_len = hint.map_or(1, |h| h.len());

    // Steps 3–6: the exact quote ([F20 §6.2]).
    let qp = Pattern::new(quote);
    let ep = end.map(Pattern::new);
    let contexts = [Context { prefix, suffix }];
    let spec = ExactSpec {
        quote: &qp,
        quote_last,
        end: ep
            .as_ref()
            .zip(end.and_then(|e| e.last().copied()))
            .map(|(pattern, last)| RangeEnd {
                pattern,
                last,
                lines: hint_len,
                m: pass.lines,
            }),
        contexts: &contexts,
        window: &window,
        lines: lines_ok,
        region: scope_lines,
        occurrence: a.occurrence.map(|o| u64::from(o.get())),
        captured_at: None,
    };
    let restrict = lifted && hkind.is_some() && scope.is_some() && scope_lines.is_none();
    let heads = match (&items, &scope) {
        (Some(Ok(it)), Some(sc)) if restrict => {
            let k = if it.lang() == Lang::Markdown {
                HeaderKind::Heading
            } else {
                HeaderKind::Symbol
            };
            Some(HeaderRanges::new(
                it.same_kind(sc).map(|y| y.start).collect(),
                k,
            ))
        }
        _ => None,
    };
    let bounds = scope_lines.map(|(ys, ye)| LineBounds {
        ys,
        ye,
        a: None,
        z: None,
    });
    let p2 = again.read(spec.held(), || (ExactSink::new(spec), (bounds, heads)))?;
    let (mut exact, (bounds, heads)) = p2.sink;
    exact.finish();
    match exact.decide(Set::Scope, 0, true) {
        Exact::Hit(hit) => {
            let span = span_of(a, hit.qs, hit.qe);
            return verify(&mut again, a, hkind, span);
        }
        Exact::Ambiguous => return Ok(Resolution::of(AnchorState::Ambiguous)),
        Exact::WindowUnavailable => return Ok(Resolution::unavailable(Unavailable::Size)),
        Exact::NoHit => {}
    }

    // Step 4: the fuzzy quote ([F20 §6.4]).
    let mut regions = Vec::with_capacity(3);
    if let Some((x, z)) = span_sink.as_ref().and_then(SpanHash::range) {
        let span = r14::SPAN as u64;
        regions.push((x.saturating_sub(span), (z + span).min(pass.len)));
    }
    if let Some(LineBounds {
        a: Some(x),
        z: Some(z),
        ..
    }) = bounds
    {
        regions.push((x, z));
    }
    regions.push((0, pass.len));
    let header_ranges = if restrict {
        Some(heads.map(HeaderRanges::finish).unwrap_or_default())
    } else {
        None
    };
    let fspec = FuzzySpec {
        quote: &qp,
        k: budget_k(quote.len()),
        end: ep.as_ref().zip(end).map(|(pattern, e)| FuzzyEnd {
            pattern,
            k: budget_k(e.len()),
            lines: hint_len,
            m: pass.lines,
        }),
        prefix,
        suffix,
        window: &window,
        lines: lines_ok,
        regions: &regions,
        headers: header_ranges.as_deref(),
    };
    let p3 = again.read(fspec.held(), || FuzzySink::new(fspec))?;
    let mut fuzzy = p3.sink;
    fuzzy.finish();
    match fuzzy.decide() {
        Fuzzy::Edited(f) => {
            return Ok(Resolution {
                score: Some(f.score),
                ..Resolution::at(AnchorState::Edited, span_of(a, f.qs, f.qe))
            });
        }
        Fuzzy::Ambiguous => return Ok(Resolution::of(AnchorState::Ambiguous)),
        Fuzzy::WindowUnavailable => return Ok(Resolution::unavailable(Unavailable::Size)),
        Fuzzy::Overflow => return Ok(Resolution::unavailable(Unavailable::Budget)),
        Fuzzy::Nothing => {}
    }

    // Step 5: scope only ([F20 §6.5]).
    Ok(scope_only(a, scope_lines).unwrap_or(Resolution::of(AnchorState::Orphaned)))
}

/// Whether an anchor's texts are within what a capture of resolver version 1 writes ([F20 §6.1]: quotes of at most
/// `QUOTE_MAX` bytes, contexts of at most `CONTEXT_MAX`). [F08 §10.3] bounds no text, but the fuzzy step's memory and
/// time grow as k × len(quote) and len(prefix)² ([F20 §6.4]), so an imported anchor with longer texts has its quote
/// steps `Unavailable(budget)` ([F20 §1.5]; spec finding of WP-64).
// spec: [F20 §6.1] (`QUOTE_MAX`, `CONTEXT_MAX`), [F20 §1.5] (`Unavailable(budget)`)
fn texts_in_bounds(quote: &[u8], prefix: &[u8], suffix: &[u8], end: Option<&[u8]>) -> bool {
    quote.len() <= r14::QUOTE_MAX
        && end.is_none_or(|e| e.len() <= r14::QUOTE_MAX)
        && prefix.len() <= r14::CONTEXT_MAX
        && suffix.len() <= r14::CONTEXT_MAX
}

/// The span a decided quote span `[qs, qe]` gives: the quote span itself, or for a `symbol` or `heading` anchor the
/// lines from its header line over the hint's length.
// spec: [F20 §6.2] step 5 (the span of a decided hit; WP-64 reading for `symbol` and `heading`)
fn span_of(a: &Anchor, qs: u64, qe: u64) -> LineSpan {
    match (a.kind, a.hint.filter(LineSpan::is_valid)) {
        (AnchorKind::Symbol | AnchorKind::Heading, Some(h)) => {
            LineSpan::of(qs, qs + u64::from(h.last - h.first))
        }
        _ => LineSpan::of(qs, qe),
    }
}

/// `edited (scope only)` for a `symbol` or `heading` anchor whose scope resolves uniquely ([F20 §6.5]).
// spec: [F20 §6.5] "Scope only"
fn scope_only(a: &Anchor, scope_lines: Option<(u64, u64)>) -> Option<Resolution> {
    let (ys, ye) = scope_lines?;
    matches!(a.kind, AnchorKind::Symbol | AnchorKind::Heading).then(|| Resolution {
        scope_only: true,
        ..Resolution::at(AnchorState::Edited, LineSpan::of(ys, ye))
    })
}

/// The watch rule at a span the exact step decided ([F20 §6.5] "Watch"; [40 §4.5] "Watch semantics"): the watched
/// text there — `ST` of the span under `span` watch, the header under `header` watch — must hash to `span_hash`; a
/// changed watched text is `edited`. Otherwise `fresh` when the span is the hint, else `moved` ([F20 §6.2] step 5).
// spec: [F20 §6.2] step 5, [F20 §6.5] "Watch"
fn verify<S: ByteSource, B: Budget>(
    again: &mut Again<'_, S, B>,
    a: &Anchor,
    hkind: Option<HeaderKind>,
    span: LineSpan,
) -> Result<Resolution, Unavailable> {
    let same = match a.span_hash {
        Some(sh) if a.watch == Watch::Span || hkind.is_some() => {
            let lines = (u64::from(span.first), u64::from(span.last));
            again.watched(a.watch, hkind, lines)? == Some(sh)
        }
        _ => true,
    };
    Ok(if !same {
        Resolution::at(AnchorState::Edited, span)
    } else if Some(span) == a.hint {
        Resolution::at(AnchorState::Fresh, span)
    } else {
        Resolution::at(AnchorState::Moved, span)
    })
}

/// A `file` anchor with `span` watch ([F20 §6.5] "Watch"; `header` watch never reads, [`resolve`]): a content pin,
/// `edited` iff the current `oid` differs from `blob` (false membership), `unverified` for a pair of different
/// algorithms.
// spec: [F20 §6.5] "Watch", [F20 §2.3] "Comparison"
fn file_anchor(a: &Anchor, content: &Content) -> Resolution {
    match oid_in(&content.oid, [&a.blob]) {
        Tri::True => Resolution::of(AnchorState::Fresh),
        Tri::False => Resolution::of(AnchorState::Edited),
        Tri::Unknown => Resolution::unverified(UnverifiedReason::OidAlgorithm),
    }
}

/// The window alignment of a `lines` anchor ([F20 §6.5]): for every j with j + len − 1 ≤ m, the window around `[j,
/// j + len − 1]` scored against the stored window; the best j by (score descending, j ascending), accepted when its
/// score is at least `LINES_MIN` and exceeds the second best by `WINDOW_MARGIN`.
///
/// The windows slide: `before(j)` gains line j − 1 when it is non-trivial, `after(e)` loses line e and is refilled
/// from a forward pointer, so the alignment reads the array once.
// spec: [F20 §6.5] "`lines` anchors", [F20 §6.3]
pub(crate) fn align(lh: &LineHashes, w: &Window, len: u64) -> Option<u64> {
    let m = lh.len() as u64;
    if len == 0 || m < len {
        return None;
    }
    let wh = |i: u64| lh.get((i - 1) as usize).and_then(|e| e.window_hash());
    let mut before: VecDeque<u16> = VecDeque::with_capacity(WIN + 1);
    let mut after: VecDeque<(u64, u16)> = VecDeque::with_capacity(WIN + 1);
    let mut ptr = len;
    let mut best: Option<(u128, u64)> = None;
    let mut second: Option<u128> = None;
    let mut bs = [0u16; WIN];
    let mut as_ = [0u16; WIN];
    for j in 1..=m - len + 1 {
        let e = j + len - 1;
        if j > 1 {
            if let Some(h) = wh(j - 1) {
                before.push_back(h);
                if before.len() > WIN {
                    before.pop_front();
                }
            }
            if after.front().is_some_and(|&(l, _)| l == e) {
                after.pop_front();
            }
        }
        ptr = ptr.max(e);
        while after.len() < WIN && ptr < m {
            ptr += 1;
            if let Some(h) = wh(ptr) {
                after.push_back((ptr, h));
            }
        }
        for (d, s) in bs.iter_mut().zip(&before) {
            *d = *s;
        }
        for (d, s) in as_.iter_mut().zip(&after) {
            *d = s.1;
        }
        let cand = Window::new(&bs[..before.len()], &as_[..after.len()]).unwrap_or(Window::EMPTY);
        let score = u128::from(w.score(&cand).0);
        match best {
            None => best = Some((score, j)),
            Some((b, _)) if score > b => {
                second = Some(b);
                best = Some((score, j));
            }
            Some(_) => {
                if second.is_none_or(|s| score > s) {
                    second = Some(score);
                }
            }
        }
    }
    let (b, j) = best?;
    let den = u128::from(w.score(&Window::EMPTY).1);
    (at_least(b, den, r14::LINES_MIN) && unique_by(b, second, den, r14::WINDOW_MARGIN)).then_some(j)
}

/// A `lines` anchor ([F20 §6.5]): the alignment, then `XXH3-64(ST(j, j + L − 1)) = span_hash` gives `moved`
/// (`fresh` when j = h1), anything else `orphaned`; a capped line-hash array leaves it `unverified (size)`.
// spec: [F20 §6.5] "`lines` anchors"
fn lines_anchor<S: ByteSource, B: Budget>(
    again: &mut Again<'_, S, B>,
    a: &Anchor,
    lines: Option<&LineHashes>,
) -> Result<Resolution, Unavailable> {
    let (Some(h), Some(sh)) = (a.hint.filter(LineSpan::is_valid), a.span_hash) else {
        return Ok(Resolution::of(AnchorState::Orphaned));
    };
    let Some(lh) = lines else {
        return Ok(Resolution::unavailable(Unavailable::Size));
    };
    let w = a.window.unwrap_or(Window::EMPTY);
    let Some(j) = align(lh, &w, h.len()) else {
        return Ok(Resolution::of(AnchorState::Orphaned));
    };
    let span = LineSpan::of(j, j + h.len() - 1);
    let got = again.watched(Watch::Span, None, (j, j + h.len() - 1))?;
    Ok(if got != Some(sh) {
        Resolution::of(AnchorState::Orphaned)
    } else if j == u64::from(h.first) {
        Resolution::at(AnchorState::Fresh, span)
    } else {
        Resolution::at(AnchorState::Moved, span)
    })
}

/// A `text-unavailable` anchor ([F20 §6.5]): the quote steps are skipped — the hint (already tried), then the window
/// alignment of the `lines` rule over the hint's length with the watched text's hash, then scope only, else
/// `orphaned`; never `fresh` by quote.
// spec: [F20 §6.5] "`text-unavailable` anchors", [40 §5.7]
fn text_unavailable<S: ByteSource, B: Budget>(
    again: &mut Again<'_, S, B>,
    a: &Anchor,
    lines: Option<&LineHashes>,
    scope_lines: Option<(u64, u64)>,
) -> Result<Resolution, Unavailable> {
    let (Some(h), Some(sh)) = (a.hint.filter(LineSpan::is_valid), a.span_hash) else {
        return Ok(scope_only(a, scope_lines).unwrap_or(Resolution::of(AnchorState::Orphaned)));
    };
    let Some(lh) = lines else {
        return Ok(Resolution::unavailable(Unavailable::Size));
    };
    let w = a.window.unwrap_or(Window::EMPTY);
    if let Some(j) = align(lh, &w, h.len()) {
        let span = LineSpan::of(j, j + h.len() - 1);
        let got = again.watched(a.watch, header_kind(a.kind), (j, j + h.len() - 1))?;
        if got == Some(sh) {
            return Ok(if j == u64::from(h.first) {
                Resolution::at(AnchorState::Fresh, span)
            } else {
                Resolution::at(AnchorState::Moved, span)
            });
        }
    }
    Ok(scope_only(a, scope_lines).unwrap_or(Resolution::of(AnchorState::Orphaned)))
}
