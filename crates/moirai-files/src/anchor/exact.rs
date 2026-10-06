//! The exact-quote step of the anchor cascade ([F20 §6.2] steps 3–6), run as N streams: every hit of the quote
//! (overlapping hits included), the pairing of a `range` anchor's start and end hits, and for each hit the context
//! score, the window score and its index, aggregated so that the decision needs no list of hits.
//!
//! Capture runs the same step on the captured content for the uniqueness ladder ([F20 §6.1] step 8), with two context
//! widths at once (`CONTEXT` and `CONTEXT_MAX`) and the captured hit's index for `occurrence`.
//!
//! # `range` pairing (WP-64 reading of [F20 §6.2] step 4)
//!
//! A start hit h pairs with one end hit h_e among those with `h_e ≥ h`, `h_e + len(end) ≥ h + len(exact)` (the end
//! quote may overlap the start quote: a span shorter than `2 × QUOTE_DEFAULT` gives overlapping quotes, and one of at
//! most `QUOTE_DEFAULT` bytes gives `exact = end`) and a last line within the spread: the one whose quote span's line
//! count is nearest the captured count `h2 − h1 + 1`, the earlier end on a tie. [F20 §6.2] as written pairs with the
//! first end hit at or after `h + len(exact)`, which never pairs overlapping quotes and truncates a range whose end
//! text recurs inside it (spec finding of WP-64).
//!
//! End hits arrive in offset order, so their last lines never decrease: among a start's candidates the nearest count
//! is the first end at or after the target line or the first end on the last line before it, and a start is decided
//! once an end at or after the target has arrived, the stream has passed the target by the distance of the best one
//! below it, or it has left the spread. No line after N's last exists, so the target and the spread's last line are
//! clamped to N's line count m (from the first read), which changes no pairing and decides a start whose captured
//! count reaches past N as soon as an end on line m arrives. Starts are decided in offset order; each end hit's suffix
//! agreement is taken when its bytes arrive, so a start decided long after its end hit needs no bytes behind the tail.
//!
//! # Memory
//!
//! The start hits that wait and the end hits they may pair with grow with the content (a hint far longer than the
//! text, or a long range over repeated lines, keeps every hit until the end of N): the sink reports them through
//! [`NSink::retained`] and the pass charges them to the budget as they grow ([`super::nstream::Metered`]; [F20 §1.3]
//! input (e)), so their growth ends as `Unavailable(budget)`, never as another target. A decided pair is aggregated at
//! once.

use std::collections::VecDeque;

use moirai_diff::{Pattern, Searcher};

use super::nstream::{LineMap, NSink, Tail};
use super::score::Top2;
use super::window::{Window, WindowCache};
use crate::r14;
use crate::text::LineHashes;

/// A hit of the exact step: quote bytes `[h, z)` of N (for a `range`, from the start quote's first byte to the end
/// quote's last) and its quote span, the lines `[qs, qe]` they cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) h: u64,
    pub(crate) z: u64,
    pub(crate) qs: u64,
    pub(crate) qe: u64,
}

/// One prefix and suffix pair whose context score is computed for every hit ([F20 §6.2] step 6.1).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Context<'a> {
    pub(crate) prefix: &'a [u8],
    pub(crate) suffix: &'a [u8],
}

impl Context<'_> {
    /// The context score's denominator `2 · P′ · S′`, with an empty prefix or suffix counting as 1 over 1.
    // spec: [F20 §6.2] step 6.1 (an empty prefix or suffix gives 1)
    pub(crate) fn den(&self) -> u128 {
        2 * self.prefix.len().max(1) as u128 * self.suffix.len().max(1) as u128
    }
}

/// A `range` anchor's end quote ([F20 §6.2] step 4).
#[derive(Clone, Copy, Debug)]
pub(crate) struct RangeEnd<'a> {
    /// The end quote, preprocessed; not empty.
    pub(crate) pattern: &'a Pattern,
    /// Its last byte, for the region test.
    pub(crate) last: u8,
    /// The captured line count `h2 − h1 + 1`, at least 1.
    pub(crate) lines: u64,
    /// N's line count m (from the first read): the pairing's target and limit are clamped to it.
    pub(crate) m: u64,
}

impl RangeEnd<'_> {
    /// `RANGE_SPREAD × (h2 − h1 + 1)` lines.
    // spec: [F20 §6.2] step 4 (`RANGE_SPREAD`)
    pub(crate) fn spread(&self) -> u64 {
        u64::from(r14::RANGE_SPREAD).saturating_mul(self.lines.max(1))
    }
}

/// What the exact step searches for and scores.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExactSpec<'a> {
    /// The quote (for a `range`, the start quote), preprocessed.
    pub(crate) quote: &'a Pattern,
    /// Its last byte, for the region test.
    pub(crate) quote_last: u8,
    /// A `range` anchor's end quote.
    pub(crate) end: Option<RangeEnd<'a>>,
    /// One or two context pairs; the first is the anchor's.
    pub(crate) contexts: &'a [Context<'a>],
    /// The stored window.
    pub(crate) window: &'a Window,
    /// The complete line-hash array, or `None` when the window step is unavailable ([F20 §2.4]).
    pub(crate) lines: Option<&'a LineHashes>,
    /// The lines `[ys, ye]` of a scope that resolves uniquely ([F21 §2.5]).
    pub(crate) region: Option<(u64, u64)>,
    /// The recorded occurrence (1-based).
    pub(crate) occurrence: Option<u64>,
    /// At capture: the captured hit's first byte.
    pub(crate) captured_at: Option<u64>,
}

/// The most context pairs scored at once.
const WIDTHS: usize = 2;

/// The bytes of N searched between two completions: the tail holds this much beyond a quote, its prefix and its
/// suffix, so no hit's context leaves the tail before it is read.
const SLICE: usize = 256;

/// The prefix or suffix agreements of a hit with each context pair: at most the context's length, which is far below
/// `u32::MAX` (a capture writes at most `CONTEXT_MAX` bytes and the cascade bounds an imported anchor's texts).
type Agree = [u32; WIDTHS];

/// A start hit of a `range` anchor waiting for its end hit ([F20 §6.2] step 4): its first byte h (it ends at
/// `h + len(exact)`), its first line and its prefix agreements.
#[derive(Clone, Copy, Debug)]
struct Start {
    h: u64,
    qs: u64,
    pa: Agree,
}

/// An end hit of a `range` anchor: its end ze (it starts at `ze − len(end)`), its last line, and its suffix
/// agreements, valid once the bytes after it have arrived (the leading `ends_done` entries of the queue).
#[derive(Clone, Copy, Debug)]
struct EndHit {
    ze: u64,
    line: u64,
    sa: Agree,
}

/// A hit waiting for the bytes after it (the suffix agreement), or a `range` pair whose agreement is known.
#[derive(Clone, Copy, Debug)]
struct Pend {
    hit: Hit,
    pa: Agree,
    sa: Option<Agree>,
    win: Option<u32>,
    in_region: bool,
    idx: [u64; 2],
}

/// The aggregate of one search region: the hits' count, the first hit, the best two of each score, the hit at the
/// recorded occurrence and the captured hit's index.
#[derive(Clone, Debug)]
struct Agg {
    count: u64,
    first: Option<Hit>,
    ctx: [Top2<Hit>; WIDTHS],
    win: Top2<Hit>,
    occ: Option<Hit>,
    captured_index: Option<u64>,
}

impl Agg {
    const fn new() -> Agg {
        Agg {
            count: 0,
            first: None,
            ctx: [Top2::new(), Top2::new()],
            win: Top2::new(),
            occ: None,
            captured_index: None,
        }
    }
}

/// The region whose hits decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Set {
    /// The whole of N.
    Whole,
    /// The scope's range when it holds a hit, else the whole of N ([F20 §6.2] step 3).
    Scope,
}

/// The outcome of the exact step for one region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exact {
    /// No hit: the cascade goes on to the fuzzy step.
    NoHit,
    /// One hit, or the hit the scores or the occurrence decide.
    Hit(Hit),
    /// Several hits and nothing decides.
    Ambiguous,
    /// The window step was needed and the line-hash array is capped ([F20 §2.4]).
    WindowUnavailable,
}

/// The exact step as an [`NSink`].
#[derive(Clone, Debug)]
pub(crate) struct ExactSink<'a> {
    spec: ExactSpec<'a>,
    qsearch: Searcher<'a>,
    esearch: Option<Searcher<'a>>,
    qlen: u64,
    elen: u64,
    smax: u64,
    tail: Tail,
    map: LineMap,
    line: u64,
    starts: VecDeque<Start>,
    ends: VecDeque<EndHit>,
    /// The leading entries of `ends` whose suffix agreements are known.
    ends_done: usize,
    pending: VecDeque<Pend>,
    formed: [u64; 2],
    sets: [Agg; 2],
    wcache: WindowCache,
    hits_q: Vec<u64>,
    hits_e: Vec<u64>,
    tmp: Vec<u8>,
}

impl ExactSpec<'_> {
    /// The longest suffix of the context pairs.
    fn smax(&self) -> usize {
        self.contexts
            .iter()
            .map(|c| c.suffix.len())
            .max()
            .unwrap_or(0)
    }

    /// The bytes of N the step holds for the context: the quotes, the longest prefix and suffix, and a slice. A caller
    /// charges them to its budget before it builds the sink ([F20 §1.3] input (e)).
    // spec: [F20 §1.3] input (e) (bytes held, charged before the pass)
    pub(crate) fn held(&self) -> usize {
        let pmax = self
            .contexts
            .iter()
            .map(|c| c.prefix.len())
            .max()
            .unwrap_or(0);
        let elen = self.end.map_or(0, |e| e.pattern.len());
        self.quote.len().max(elen) + pmax + self.smax() + SLICE
    }
}

impl<'a> ExactSink<'a> {
    pub(crate) fn new(spec: ExactSpec<'a>) -> ExactSink<'a> {
        debug_assert!(!spec.contexts.is_empty() && spec.contexts.len() <= WIDTHS);
        debug_assert!(spec.end.is_none_or(|e| !e.pattern.is_empty()));
        let qlen = spec.quote.len() as u64;
        let elen = spec.end.map_or(0, |e| e.pattern.len() as u64);
        ExactSink {
            qsearch: spec.quote.searcher(0),
            esearch: spec.end.map(|e| e.pattern.searcher(0)),
            qlen,
            elen,
            smax: spec.smax() as u64,
            tail: Tail::new(spec.held()),
            map: LineMap::default(),
            line: 0,
            starts: VecDeque::new(),
            ends: VecDeque::new(),
            ends_done: 0,
            pending: VecDeque::new(),
            formed: [0; 2],
            sets: [Agg::new(), Agg::new()],
            wcache: WindowCache::new(),
            hits_q: Vec::new(),
            hits_e: Vec::new(),
            tmp: Vec::new(),
            spec,
        }
    }

    /// The prefix agreements `pa` of a hit starting at `h`: the longest common suffix of each prefix and
    /// `N[max(0, h − P) .. h)` ([F20 §6.2] step 6.1).
    // spec: [F20 §6.2] step 6.1 (`pa`)
    fn prefix_agreement(&mut self, h: u64) -> Agree {
        let mut pa = [0u32; WIDTHS];
        for (w, c) in self.spec.contexts.iter().enumerate() {
            let p = c.prefix.len() as u64;
            self.tail.copy(h.saturating_sub(p), h, &mut self.tmp);
            let n = c
                .prefix
                .iter()
                .rev()
                .zip(self.tmp.iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            pa[w] = u32::try_from(n).unwrap_or(u32::MAX);
        }
        pa
    }

    /// Whether the hit's bytes `[h, z)` lie in the scope's range `[start(ys) .. end(ye))` ([F21 §2.5]).
    // spec: [F21 §2.5], [F20 §6.2] step 3
    fn in_region(&self, qs: u64, qe: u64, last: u8) -> bool {
        self.spec
            .region
            .is_some_and(|(ys, ye)| qs >= ys && (qe < ye || (qe == ye && last != b'\n')))
    }

    /// The suffix agreements `sa` of a hit ending at `z`: the longest common prefix of each suffix and
    /// `N[z .. z + S)`, as far as N goes ([F20 §6.2] step 6.1).
    // spec: [F20 §6.2] step 6.1 (`sa`)
    fn suffix_agreement(&mut self, z: u64) -> Agree {
        let mut sa = [0u32; WIDTHS];
        for (w, c) in self.spec.contexts.iter().enumerate() {
            let s = c.suffix.len() as u64;
            self.tail.copy(z, z + s, &mut self.tmp);
            let n = c
                .suffix
                .iter()
                .zip(self.tmp.iter())
                .take_while(|(a, b)| a == b)
                .count();
            sa[w] = u32::try_from(n).unwrap_or(u32::MAX);
        }
        sa
    }

    // spec: [F20 §6.2] step 4 (hits), step 6 (the scores of each hit), [F20 §6.3]
    fn form(&mut self, hit: Hit, pa: Agree, sa: Option<Agree>, last: u8) {
        let in_region = self.in_region(hit.qs, hit.qe, last);
        self.formed[0] += 1;
        if in_region {
            self.formed[1] += 1;
        }
        let win = self.spec.lines.map(|lh| {
            let cand = self.wcache.around(lh, hit.qs, hit.qe);
            self.spec.window.score(&cand).0
        });
        self.pending.push_back(Pend {
            hit,
            pa,
            sa,
            win,
            in_region,
            idx: self.formed,
        });
    }

    /// Completes the pending hits whose suffix bytes have all arrived, or every one at the end of N.
    // spec: [F20 §6.2] step 6.1 (`sa`, `ctx`)
    fn complete(&mut self, all: bool) {
        let end = self.tail.end();
        while let Some(p) = self.pending.front().copied() {
            if p.sa.is_none() && !all && p.hit.z + self.smax > end {
                break;
            }
            self.pending.pop_front();
            let sa = match p.sa {
                Some(sa) => sa,
                None => self.suffix_agreement(p.hit.z),
            };
            let mut ctx = [0u128; WIDTHS];
            for (w, c) in self.spec.contexts.iter().enumerate() {
                let sa = u128::from(sa[w]);
                let (pn, pd) = if c.prefix.is_empty() {
                    (1, 1)
                } else {
                    (u128::from(p.pa[w]), c.prefix.len() as u128)
                };
                let (sn, sd) = if c.suffix.is_empty() {
                    (1, 1)
                } else {
                    (sa, c.suffix.len() as u128)
                };
                ctx[w] = pn * sd + sn * pd;
            }
            let agg = |a: &mut Agg, idx: u64, spec: &ExactSpec<'_>| {
                a.count += 1;
                if a.first.is_none() {
                    a.first = Some(p.hit);
                }
                for (top, &v) in a.ctx.iter_mut().zip(&ctx).take(spec.contexts.len()) {
                    top.push(v, p.hit);
                }
                if let Some(v) = p.win {
                    a.win.push(u128::from(v), p.hit);
                }
                if spec.occurrence == Some(idx) {
                    a.occ = Some(p.hit);
                }
                if spec.captured_at == Some(p.hit.h) {
                    a.captured_index = Some(idx);
                }
            };
            let spec = self.spec;
            agg(&mut self.sets[0], p.idx[0], &spec);
            if p.in_region {
                agg(&mut self.sets[1], p.idx[1], &spec);
            }
        }
    }

    /// The end of N.
    pub(crate) fn finish(&mut self) {
        self.fill_ends(true);
        self.settle(true);
        self.complete(true);
    }

    fn set(&self, which: Set) -> &Agg {
        match which {
            Set::Scope if self.sets[1].count > 0 => &self.sets[1],
            _ => &self.sets[0],
        }
    }

    /// The number of hits of a region.
    #[cfg(test)]
    pub(crate) fn count(&self, which: Set) -> u64 {
        self.set(which).count
    }

    /// The captured hit's 1-based index among the region's hits ([F20 §6.1] step 8.3).
    pub(crate) fn captured_index(&self, which: Set) -> Option<u64> {
        self.set(which).captured_index
    }

    /// The decision of [F20 §6.2] steps 5–6 over a region, with context pair `width`, consulting the occurrence when
    /// `occurrence` is set. Nearest-to-hint is never a tie-break (step 7).
    ///
    /// When several hits remain after the context score and the window score is needed while the line-hash array is
    /// capped, the outcome is [`Exact::WindowUnavailable`]: the window step could have decided a different hit than
    /// the occurrence, and a key may only turn an answer into `unverified`, never into another target ([F20 §1.3],
    /// [40 §4.1] P7).
    // spec: [F20 §6.2] steps 5–7
    pub(crate) fn decide(&self, which: Set, width: usize, occurrence: bool) -> Exact {
        let a = self.set(which);
        match a.count {
            0 => return Exact::NoHit,
            1 => return a.first.map_or(Exact::NoHit, Exact::Hit),
            _ => {}
        }
        let c = &self.spec.contexts[width];
        if let Some(h) = a.ctx[width].decide(c.den(), r14::CONTEXT_MARGIN) {
            return Exact::Hit(h);
        }
        if !self.spec.window.is_empty() {
            if self.spec.lines.is_none() {
                return Exact::WindowUnavailable;
            }
            if let Some(h) = a
                .win
                .decide(self.spec.window.len() as u128, r14::WINDOW_MARGIN)
            {
                return Exact::Hit(h);
            }
        }
        if occurrence && let Some(h) = a.occ {
            return Exact::Hit(h);
        }
        Exact::Ambiguous
    }

    /// Takes the suffix agreements of the end hits whose suffix bytes have all arrived, or of every one at the end of N.
    // spec: [F20 §6.2] step 6.1 (`sa` of a `range` pair: after its end quote)
    fn fill_ends(&mut self, all: bool) {
        let end = self.tail.end();
        while self.ends_done < self.ends.len() {
            let ze = self.ends[self.ends_done].ze;
            if !all && ze + self.smax > end {
                break;
            }
            let sa = self.suffix_agreement(ze);
            self.ends[self.ends_done].sa = sa;
            self.ends_done += 1;
        }
    }

    /// The least start of an end hit that may pair with a start hit at `h`: `h_e ≥ h` and `h_e + len(end) ≥ h +
    /// len(exact)`.
    // spec: [F20 §6.2] step 4 (WP-64 reading: an end hit starts at or after the start hit and ends at or after its end)
    fn threshold(&self, h: u64) -> u64 {
        h.max((h + self.qlen).saturating_sub(self.elen))
    }

    /// Pairs the waiting start hits, in offset order, that can be decided (module doc, "`range` pairing"); `eof` at the
    /// end of N. A start without a candidate is dropped; a decided pair is aggregated at once.
    // spec: [F20 §6.2] step 4 (`range` pairing, WP-64 reading: the end hit at or after the start whose line count is
    // nearest the captured one within the spread, the earlier on a tie; no line after N's m exists)
    fn settle(&mut self, eof: bool) {
        let Some(end) = self.spec.end else {
            return;
        };
        let spread = end.spread();
        let elen = self.elen;
        while let Some(s) = self.starts.front().copied() {
            // Candidates: h_e ≥ h and h_e + len(end) ≥ z, i.e. h_e ≥ max(h, z − len(end)); their lines never decrease.
            let thr = self.threshold(s.h);
            let m = end.m.max(s.qs);
            let limit = s.qs.saturating_add(spread - 1).min(m);
            let target = s.qs.saturating_add(end.lines.max(1) - 1).min(m);
            let i0 = self.ends.partition_point(|x| x.ze - elen < thr);
            let lim = self.ends.partition_point(|x| x.line <= limit).max(i0);
            let i1 = self
                .ends
                .partition_point(|x| x.line < target)
                .clamp(i0, lim);
            let above = (i1 < lim).then_some(i1);
            let below = (i1 > i0).then(|| {
                let lb = self.ends[i1 - 1].line;
                self.ends.partition_point(|x| x.line < lb).max(i0)
            });
            // Every end hit still to come ends on the current line or later.
            let decided = above.is_some()
                || eof
                || self.line > limit
                || below.is_some_and(|b| {
                    let d = target - self.ends[b].line;
                    self.line >= target.saturating_add(d)
                });
            if !decided {
                break;
            }
            let best = match (below, above) {
                (Some(b), Some(a)) => {
                    if target - self.ends[b].line <= self.ends[a].line - target {
                        Some(b)
                    } else {
                        Some(a)
                    }
                }
                (b, a) => b.or(a),
            };
            match best {
                Some(i) => {
                    // Its suffix agreements are not known before the bytes after it have arrived.
                    if i >= self.ends_done {
                        break;
                    }
                    let e = self.ends[i];
                    self.starts.pop_front();
                    let hit = Hit {
                        h: s.h,
                        z: e.ze,
                        qs: s.qs,
                        qe: e.line,
                    };
                    self.form(hit, s.pa, Some(e.sa), end.last);
                    self.complete(false);
                }
                None => {
                    self.starts.pop_front();
                }
            }
        }
        // End hits before every remaining start's threshold are no candidate of any start, waiting or to come.
        let k = match self.starts.front() {
            Some(n) => {
                let thr = self.threshold(n.h);
                self.ends.partition_point(|x| x.ze - elen < thr)
            }
            None => self.ends.len(),
        };
        self.ends.drain(..k);
        self.ends_done = self.ends_done.saturating_sub(k);
    }
}

impl ExactSink<'_> {
    /// At most [`SLICE`] bytes of N: searched, their hits formed, the pending hits completed.
    // spec: [F20 §6.2] step 4 (every offset h with `N′[h .. h + len(exact)) = exact`, overlapping hits included)
    fn slice(&mut self, at: u64, b: &[u8]) {
        self.tail.push(at, b);
        self.map.prune(self.tail.start());
        let mut hq = std::mem::take(&mut self.hits_q);
        let mut he = std::mem::take(&mut self.hits_e);
        hq.clear();
        he.clear();
        self.qsearch.feed(b, |hit| hq.push(hit.end as u64));
        if let Some(s) = &mut self.esearch {
            s.feed(b, |hit| he.push(hit.end as u64));
        }
        for &z in &hq {
            let h = z - self.qlen;
            let qs = self.map.line_at(h);
            let pa = self.prefix_agreement(h);
            if self.spec.end.is_some() {
                self.starts.push_back(Start { h, qs, pa });
            } else {
                let qe = self.map.line_at(z - 1);
                let last = self.spec.quote_last;
                self.form(Hit { h, z, qs, qe }, pa, None, last);
            }
        }
        for &ze in &he {
            // An end hit is kept only while a start waits: none to come can pair with it (module doc).
            if !self.starts.is_empty() {
                let line = self.map.line_at(ze - 1);
                self.ends.push_back(EndHit {
                    ze,
                    line,
                    sa: [0; WIDTHS],
                });
            }
        }
        self.hits_q = hq;
        self.hits_e = he;
        self.fill_ends(false);
        self.settle(false);
        self.complete(false);
    }
}

impl NSink for ExactSink<'_> {
    fn start_line(&mut self, line: u64, at: u64) {
        self.map.start_line(line, at);
        self.line = line;
        if self.spec.end.is_some() && !self.starts.is_empty() {
            self.settle(false);
            self.complete(false);
        }
    }

    fn bytes(&mut self, at: u64, b: &[u8]) {
        let mut at = at;
        for part in b.chunks(SLICE) {
            self.slice(at, part);
            at += part.len() as u64;
        }
    }

    fn end_line(&mut self, _line: u64, _at: u64) {}

    // spec: [F20 §1.3] input (e) (the waiting start and end hits and the pending hits, charged as they grow)
    fn retained(&self) -> usize {
        self.starts.capacity() * size_of::<Start>()
            + self.ends.capacity() * size_of::<EndHit>()
            + self.pending.capacity() * size_of::<Pend>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::nstream::NStream;
    use crate::oid::ObjectFormat;
    use crate::text::analyse;

    fn run(
        text: &[u8],
        quote: &[u8],
        end: Option<(&[u8], u64)>,
        ctx: Context<'_>,
    ) -> (ExactSink<'static>, u64) {
        // N's line count, as the first read gives it.
        let m = {
            let mut ns = NStream::new(());
            let _ = analyse(text, ObjectFormat::Sha1, None, &mut ns);
            ns.lines()
        };
        // Leak the patterns for the test's 'static borrow.
        let qp: &'static Pattern = Box::leak(Box::new(Pattern::new(quote)));
        let ep = end.map(|(e, lines)| {
            let p: &'static Pattern = Box::leak(Box::new(Pattern::new(e)));
            RangeEnd {
                pattern: p,
                last: *e.last().unwrap(),
                lines,
                m,
            }
        });
        let ctxs: &'static [Context<'static>] = Box::leak(
            vec![Context {
                prefix: Box::leak(ctx.prefix.to_vec().into_boxed_slice()),
                suffix: Box::leak(ctx.suffix.to_vec().into_boxed_slice()),
            }]
            .into_boxed_slice(),
        );
        let content = analyse(text, ObjectFormat::Sha1, Some(1 << 16), &mut ());
        let lh: &'static LineHashes = Box::leak(Box::new(content.line_hashes.unwrap()));
        let w: &'static Window = Box::leak(Box::new(Window::EMPTY));
        let spec = ExactSpec {
            quote: qp,
            quote_last: *quote.last().unwrap(),
            end: ep,
            contexts: ctxs,
            window: w,
            lines: Some(lh),
            region: None,
            occurrence: Some(2),
            captured_at: None,
        };
        let mut s = NStream::new(ExactSink::new(spec));
        let _ = analyse(text, ObjectFormat::Sha1, None, &mut s);
        let n = s.len();
        let mut sink = s.into_sink();
        sink.finish();
        (sink, n)
    }

    #[test]
    fn hits_and_context_decide() {
        let none = Context {
            prefix: b"",
            suffix: b"",
        };
        let (s, _) = run(b"a x\nb x\n", b"x", None, none);
        assert_eq!(s.count(Set::Whole), 2);
        // Occurrence 2 decides when nothing else does.
        assert_eq!(
            s.decide(Set::Whole, 0, true),
            Exact::Hit(Hit {
                h: 6,
                z: 7,
                qs: 2,
                qe: 2
            })
        );
        assert_eq!(s.decide(Set::Whole, 0, false), Exact::Ambiguous);
        let pre = Context {
            prefix: b"b ",
            suffix: b"",
        };
        let (s, _) = run(b"a x\nb x\n", b"x", None, pre);
        assert_eq!(
            s.decide(Set::Whole, 0, false),
            Exact::Hit(Hit {
                h: 6,
                z: 7,
                qs: 2,
                qe: 2
            })
        );
        // Overlapping hits count.
        let (s, _) = run(b"aaaa", b"aa", None, none);
        assert_eq!(s.count(Set::Whole), 3);
    }

    #[test]
    fn range_pairs_with_the_nearest_line_count_within_the_spread() {
        let none = Context {
            prefix: b"",
            suffix: b"",
        };
        let hit = |s: &ExactSink<'_>| match s.decide(Set::Whole, 0, false) {
            Exact::Hit(h) => Some((h.qs, h.qe)),
            _ => None,
        };
        // Start "S" at lines 1 and 5, end "E" at 3 and 7. Two captured lines: spread 4, each start pairs.
        let t = b"S\nx\nE\nx\nS\nx\nE\n";
        let (s, _) = run(t, b"S", Some((b"E", 2)), none);
        assert_eq!(s.count(Set::Whole), 2);
        // One captured line: spread 2, and "E" on line 3 is past line 1 + 2 − 1.
        let (s, _) = run(t, b"S", Some((b"E", 1)), none);
        assert_eq!(s.count(Set::Whole), 0);
        // The end text recurs inside the range: the line count nearest the captured one decides, not the first end.
        let t = b"S\nE\nx\nE\n";
        let (s, _) = run(t, b"S", Some((b"E", 4)), none);
        assert_eq!(hit(&s), Some((1, 4)));
        let (s, _) = run(t, b"S", Some((b"E", 2)), none);
        assert_eq!(hit(&s), Some((1, 2)));
        // Lines 2 and 4 are both one line from the captured 3: the earlier end wins.
        let (s, _) = run(t, b"S", Some((b"E", 3)), none);
        assert_eq!(hit(&s), Some((1, 2)));
        // A captured count past N: the target is clamped to N's last line, which pairs as the unclamped rule does (the
        // end on the last line is the nearest), and decides there.
        let (s, _) = run(t, b"S", Some((b"E", 1000)), none);
        assert_eq!(hit(&s), Some((1, 4)));
        let (s, _) = run(b"S E E\n", b"S", Some((b"E", u64::from(u32::MAX))), none);
        assert_eq!(hit(&s), Some((1, 1)));
        // Overlapping quotes: the end quote equals the start quote, and pairs with itself.
        let (s, _) = run(b"q\nabc\nz\n", b"abc", Some((b"abc", 1)), none);
        assert_eq!(hit(&s), Some((2, 2)));
        // The end overlaps the start's last bytes: it only has to end after it.
        let (s, _) = run(b"abcd\n", b"abc", Some((b"bcd", 1)), none);
        assert_eq!(hit(&s), Some((1, 1)));
        // An end that starts before the start is no candidate.
        let (s, _) = run(b"xab\n", b"ab", Some((b"xab", 1)), none);
        assert_eq!(s.count(Set::Whole), 0);
    }
}
