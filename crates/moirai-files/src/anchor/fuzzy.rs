//! The fuzzy-quote step of the anchor cascade ([F20 §6.4]), run as N streams: in each region (around the hint, the
//! scope, the whole text) the k-bounded Myers search with `k = ⌊FUZZY_BUDGET × len(exact)⌋`, the candidates selected
//! as [F20 §6.4] defines them (moirai-diff's [`Selector`]), and each candidate's score from its quote, prefix,
//! suffix and window agreement; a `range` anchor's start candidates each search their end quote within the spread.
//! Candidates are scored as they are decided and only the best two of each region are kept. Once a region is
//! complete and holds a candidate, it decides, and the searches of the later regions stop.
//!
//! **`range` end search** (WP-64 reading of [F20 §6.4] "`range` anchors", with the pairing of [`super::exact`]): the
//! end quote is searched from the start candidate's first byte, and an end candidate counts when it starts at or after
//! the start candidate and ends at or after its end (the quotes of a short range overlap); among the end candidates
//! the best is the least by (distance, line-count distance to the captured count, start). An end quote's q below
//! `FUZZY_ACCEPT` discards the pair, whose q is the smaller of the two. The spread's last line is clamped to N's line
//! count m, after which no end can lie.
//!
//! **Memory and work.** Each start candidate of a `range` keeps an end search until its spread ends: their number, what
//! they hold and the bytes of N they search again grow with the candidates and the spread (a short imported quote with
//! a hint far longer than the text searches to the end of N once per candidate). The sink reports them through
//! [`NSink::retained`] and [`NSink::work`] and the pass charges them to the budget as they grow
//! ([`super::nstream::Metered`]; [F20 §1.3] input (e)), so their growth ends as `Unavailable(budget)`.

use moirai_diff::{Hit as DiffHit, Pattern, Searcher, Selector, levenshtein};

use super::nstream::{LineMap, NSink, Tail};
use super::score::{Score, cmp_products};
use super::window::{Window, WindowCache};
use crate::r14::{self, Ratio};
use crate::text::LineHashes;

/// The bytes of N processed between two rounds of candidate decisions.
const SLICE: usize = 256;

/// A `range` anchor's end quote for the fuzzy step ([F20 §6.4] "`range` anchors").
#[derive(Clone, Copy, Debug)]
pub(crate) struct FuzzyEnd<'a> {
    /// The end quote, preprocessed; not empty.
    pub(crate) pattern: &'a Pattern,
    /// Its own error budget.
    pub(crate) k: usize,
    /// The captured line count `h2 − h1 + 1`, at least 1.
    pub(crate) lines: u64,
    /// N's line count m (from the first read): the spread's last line is clamped to it.
    pub(crate) m: u64,
}

/// What the fuzzy step searches for and scores.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FuzzySpec<'a> {
    /// The quote (for a `range`, the start quote) and its error budget k.
    pub(crate) quote: &'a Pattern,
    pub(crate) k: usize,
    /// A `range` anchor's end quote.
    pub(crate) end: Option<FuzzyEnd<'a>>,
    pub(crate) prefix: &'a [u8],
    pub(crate) suffix: &'a [u8],
    pub(crate) window: &'a Window,
    /// The complete line-hash array, or `None` when the window score is unavailable ([F20 §2.4]).
    pub(crate) lines: Option<&'a LineHashes>,
    /// The regions `[start, end)` of N in their order (R1, R2, R3; an absent one left out).
    pub(crate) regions: &'a [(u64, u64)],
    /// The same-kind header restriction ([F20 §6.4], [F21 §2.6]): the byte ranges of the item headers of the
    /// anchor's kind, sorted by start; `None` when the restriction does not apply.
    pub(crate) headers: Option<&'a [(u64, u64)]>,
}

/// The decided candidate of a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) s: u64,
    pub(crate) e: u64,
    pub(crate) qs: u64,
    pub(crate) qe: u64,
    pub(crate) score: Score,
}

/// The outcome of the fuzzy step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fuzzy {
    /// No region holds a candidate.
    Nothing,
    /// The first region with a candidate accepted its best: `edited`.
    Edited(Found),
    /// The first region with a candidate has two within the margin.
    Ambiguous,
    /// A score of the deciding region needed the window while the line-hash array is capped.
    WindowUnavailable,
    /// A score's common denominator does not fit 128 bits (only an anchor far outside what a capture writes).
    Overflow,
}

/// A scored candidate's place in a region's order (score descending, s ascending).
#[derive(Clone, Copy, Debug)]
struct Ranked {
    num: u128,
    found: Found,
}

/// The candidates of one region.
#[derive(Clone, Debug)]
struct RegionRun<'a> {
    start: u64,
    end: u64,
    search: Searcher<'a>,
    sel: Selector,
    done: bool,
    count: u64,
    best: Option<Ranked>,
    second: Option<u128>,
    window_unavailable: bool,
}

/// A `range` start candidate `[start, e1)` searching its end quote in `[start, end)` ([F20 §6.4] "`range` anchors").
#[derive(Clone, Debug)]
struct EndRun<'a> {
    region: usize,
    /// The start candidate's first byte: the search region's start and the least start of an end candidate.
    start: u64,
    /// The start candidate's end: the least end of an end candidate.
    e1: u64,
    qs1: u64,
    q1: u64,
    psn: u128,
    limit: u64,
    end: Option<u64>,
    fed: u64,
    search: Searcher<'a>,
    sel: Selector,
    /// The best end candidate by (distance, line-count distance, start): (d, line distance, s, e, qe).
    best: Option<(usize, u64, u64, u64, u64)>,
    /// The bytes after the best end candidate seen so far, for its suffix score.
    best_sfx: Vec<u8>,
}

/// A candidate (or pair) collecting the bytes after its end, `N[e .. e + len(suffix))`, as they arrive.
#[derive(Clone, Debug)]
struct Pend {
    region: usize,
    s: u64,
    e: u64,
    qs: u64,
    qe: u64,
    qn: u128,
    psn: u128,
    wsn: Option<u128>,
    sfx: Vec<u8>,
}

/// The fuzzy step as an [`NSink`].
#[derive(Clone, Debug)]
pub(crate) struct FuzzySink<'a> {
    spec: FuzzySpec<'a>,
    l1: u128,
    l2: u128,
    p: u128,
    s: u128,
    w: u128,
    /// The common denominator of every score, or `None` when it overflows.
    den: Option<u128>,
    tail: Tail,
    map: LineMap,
    line: u64,
    regions: Vec<RegionRun<'a>>,
    /// Whether a complete region with a candidate has stopped the later ones.
    decisive: bool,
    ends: Vec<EndRun<'a>>,
    /// The heap bytes the end searches held when last counted, with those started since.
    ends_heap: usize,
    /// The bytes of N fed to end searches so far.
    searched: u64,
    pending: Vec<Pend>,
    wcache: WindowCache,
    cands: Vec<(usize, DiffHit)>,
    found: Vec<DiffHit>,
    tmp: Vec<u8>,
}

impl FuzzySpec<'_> {
    /// The bytes of N the step holds to locate and score candidates: the decision lag of each search, the quotes, k,
    /// the prefix and the suffix, and a slice. A caller charges them to its budget before it builds the sink
    /// ([F20 §1.3] input (e)); the lag grows as k × (len − 1), so the caller first bounds the quote lengths.
    // spec: [F20 §1.3] input (e) (bytes held, charged before the pass)
    pub(crate) fn held(&self) -> usize {
        let lag1 = self.k.saturating_mul(self.quote.len().saturating_sub(1));
        let (lag2, len2, k2) = self.end.map_or((0, 0, 0), |e| {
            (
                e.k.saturating_mul(e.pattern.len().saturating_sub(1)),
                e.pattern.len(),
                e.k,
            )
        });
        let need = (lag1 + self.quote.len() + self.k + self.prefix.len())
            .max(lag2 + len2 + k2)
            .max(lag1.max(lag2) + self.suffix.len());
        need + SLICE
    }
}

impl<'a> FuzzySink<'a> {
    pub(crate) fn new(spec: FuzzySpec<'a>) -> FuzzySink<'a> {
        let l1 = spec.quote.len() as u128;
        let l2 = spec.end.map_or(1, |e| e.pattern.len() as u128);
        let p = spec.prefix.len().max(1) as u128;
        let s = spec.suffix.len().max(1) as u128;
        let w = spec.window.len().max(1) as u128;
        let sumw: u128 = r14::FUZZY_WEIGHTS.iter().map(|&x| u128::from(x)).sum();
        let lq = l1.checked_mul(l2);
        let den = lq
            .and_then(|x| x.checked_mul(p))
            .and_then(|x| x.checked_mul(s))
            .and_then(|x| x.checked_mul(w))
            .and_then(|x| x.checked_mul(sumw));
        let regions = spec
            .regions
            .iter()
            .map(|&(start, end)| RegionRun {
                start,
                end,
                search: spec.quote.searcher(spec.k),
                sel: Selector::new(spec.quote.len(), spec.k),
                done: start >= end,
                count: 0,
                best: None,
                second: None,
                window_unavailable: false,
            })
            .collect();
        FuzzySink {
            spec,
            l1,
            l2,
            p,
            s,
            w,
            den,
            tail: Tail::new(spec.held()),
            map: LineMap::default(),
            line: 0,
            regions,
            decisive: false,
            ends: Vec::new(),
            ends_heap: 0,
            searched: 0,
            pending: Vec::new(),
            wcache: WindowCache::new(),
            cands: Vec::new(),
            found: Vec::new(),
            tmp: Vec::new(),
        }
    }

    /// The largest start with distance `d` of a match ending at `e`, with no start before `lo` ([F20 §6.4]: "s is the
    /// largest start with distance d(e)").
    // spec: [F20 §6.4] "Candidates in a region R"
    fn locate(&mut self, pat: &Pattern, e: u64, d: usize, lo: u64) -> Option<u64> {
        let from = e.saturating_sub((pat.len() + d) as u64).max(lo);
        let base = self.tail.copy(from, e, &mut self.tmp);
        let loc = pat.locate(&self.tmp, self.tmp.len(), 0, d)?;
        (loc.distance == d).then_some(base + loc.start as u64)
    }

    /// `ps`'s numerator over `P′`: `P − lev(prefix, N[max(0, s − P) .. s))`, or 1 over 1 for an empty prefix.
    // spec: [F20 §6.4] "Score of a candidate" (`ps`)
    fn prefix_score(&mut self, s: u64) -> u128 {
        if self.spec.prefix.is_empty() {
            return 1;
        }
        self.tail.copy(
            s.saturating_sub(self.spec.prefix.len() as u64),
            s,
            &mut self.tmp,
        );
        let d = levenshtein(self.spec.prefix, &self.tmp) as u128;
        self.p.saturating_sub(d)
    }

    /// `ws`'s numerator over `W′` around lines `[qs, qe]`; `None` when it needs the capped array.
    // spec: [F20 §6.4] (`ws`), [F20 §6.3]
    fn window_score(&mut self, qs: u64, qe: u64) -> Option<u128> {
        if self.spec.window.is_empty() {
            return Some(0);
        }
        let lh = self.spec.lines?;
        let cand = self.wcache.around(lh, qs, qe);
        Some(u128::from(self.spec.window.score(&cand).0))
    }

    /// The heap bytes of one end search beyond its own record: the searcher's three columns of 64-bit blocks and,
    /// when given, the ends its selector holds and the bytes after its best end candidate.
    fn run_heap(&self, run: Option<&EndRun<'_>>) -> usize {
        let blocks = self.spec.end.map_or(0, |e| e.pattern.len().div_ceil(64));
        3 * 8 * blocks
            + run.map_or(0, |r| {
                r.sel.held() * size_of::<usize>() + r.best_sfx.capacity()
            })
    }

    /// Whether `[s, e)` lies inside a same-kind item header ([F21 §2.6]), or no restriction applies.
    // spec: [F20 §6.4] "Same-kind headers", [F21 §2.6]
    fn header_ok(&self, s: u64, e: u64) -> bool {
        self.spec.headers.is_none_or(|hs| {
            let k = hs.partition_point(|&(a, _)| a <= s);
            hs[..k].iter().any(|&(_, z)| e <= z)
        })
    }

    /// A start candidate of region `r` ending at `e` with distance `d`.
    // spec: [F20 §6.4] "Candidates in a region R", "Acceptance" (q < FUZZY_ACCEPT discarded)
    fn candidate(&mut self, r: usize, e: u64, d: usize) {
        let lo = self.regions[r].start;
        let Some(s) = self.locate(self.spec.quote, e, d, lo) else {
            return;
        };
        let l1 = self.spec.quote.len() as u64;
        let q1 = l1 - d as u64;
        if !super::score::at_least(u128::from(q1), u128::from(l1), r14::FUZZY_ACCEPT) {
            return;
        }
        if !self.header_ok(s, e) {
            return;
        }
        let qs = self.map.line_at(s);
        let psn = self.prefix_score(s);
        if let Some(fe) = self.spec.end {
            let spread = u64::from(r14::RANGE_SPREAD).saturating_mul(fe.lines.max(1));
            let limit = qs.saturating_add(spread - 1).min(fe.m.max(qs));
            if self.map.line_at(e.saturating_sub(1)) > limit {
                return;
            }
            let end = self.map.start_of(limit + 1).map(|x| x - 1);
            self.ends.push(EndRun {
                region: r,
                start: s,
                e1: e,
                qs1: qs,
                q1,
                psn,
                limit,
                end,
                fed: s,
                search: fe.pattern.searcher(fe.k),
                sel: Selector::new(fe.pattern.len(), fe.k),
                best: None,
                best_sfx: Vec::new(),
            });
            self.ends_heap += self.run_heap(None);
            return;
        }
        let qe = self.map.line_at(e - 1);
        let wsn = self.window_score(qs, qe);
        let mut sfx = Vec::new();
        self.fill(&mut sfx, e);
        self.pending.push(Pend {
            region: r,
            s,
            e,
            qs,
            qe,
            qn: u128::from(q1),
            psn,
            wsn,
            sfx,
        });
    }

    /// Appends to `sfx` the bytes of `N[e + len(sfx) .. e + len(suffix))` that have arrived.
    // spec: [F20 §6.4] (`ss` over `N[e .. e + len(suffix))`)
    fn fill(&self, sfx: &mut Vec<u8>, e: u64) {
        let want = self.spec.suffix.len();
        if sfx.len() >= want {
            return;
        }
        let from = e + sfx.len() as u64;
        let to = (e + want as u64).min(self.tail.end());
        let mut i = from.max(self.tail.start());
        while i < to {
            sfx.push(self.tail.byte(i));
            i += 1;
        }
    }

    /// Feeds the end searches the bytes they have not seen, and finishes those whose region is complete (every one
    /// at the end of N).
    // spec: [F20 §6.4] "`range` anchors" (the end quote within the spread; WP-64 reading: from the start candidate's
    // first byte, ending at or after its end, the best by (q descending, line-count distance, offset ascending)),
    // "Acceptance" (the pair's q = min(q1, q2) below FUZZY_ACCEPT is discarded), [F20 §1.3] input (e) (the bytes
    // searched and held, charged as they grow)
    fn run_ends(&mut self, all: bool) {
        let Some(fe) = self.spec.end else {
            return;
        };
        let pat = fe.pattern;
        let lcount = fe.lines.max(1);
        let top = self.tail.end();
        let mut found = std::mem::take(&mut self.found);
        let mut i = 0;
        while i < self.ends.len() {
            found.clear();
            let stop = self.ends[i].end.unwrap_or(top).min(top);
            if stop > self.ends[i].fed {
                let from = self.ends[i].fed;
                self.searched = self.searched.saturating_add(stop - from);
                self.tail.copy(from, stop, &mut self.tmp);
                let tmp = &self.tmp;
                let EndRun {
                    search,
                    sel,
                    fed,
                    start,
                    e1,
                    ..
                } = &mut self.ends[i];
                // Only ends at or after the start candidate's end are end candidates.
                let min_end = *e1 - *start;
                search.feed(tmp, |h| {
                    if h.end as u64 >= min_end {
                        sel.push(h, |c| found.push(c));
                    }
                });
                sel.advance(search.position(), |c| found.push(c));
                *fed = stop;
            }
            let finished = all || self.ends[i].end.is_some_and(|z| self.ends[i].fed >= z);
            if finished {
                self.ends[i].sel.finish(|c| found.push(c));
            }
            if let Some((_, _, _, e2, _)) = self.ends[i].best {
                let mut sfx = std::mem::take(&mut self.ends[i].best_sfx);
                self.fill(&mut sfx, e2);
                self.ends[i].best_sfx = sfx;
            }
            for c in found.iter().copied() {
                let start = self.ends[i].start;
                let e2 = start + c.end as u64;
                if let Some(s2) = self.locate(pat, e2, c.distance, start) {
                    let qe = self.map.line_at(e2 - 1);
                    let count = qe + 1 - self.ends[i].qs1;
                    let key = (c.distance, count.abs_diff(lcount), s2);
                    let better = self.ends[i]
                        .best
                        .is_none_or(|(bd, bl, bs, _, _)| key < (bd, bl, bs));
                    if better {
                        let mut sfx = Vec::new();
                        self.fill(&mut sfx, e2);
                        let run = &mut self.ends[i];
                        run.best = Some((key.0, key.1, s2, e2, qe));
                        run.best_sfx = sfx;
                    }
                }
            }
            if finished {
                let run = self.ends.swap_remove(i);
                let l2 = pat.len() as u64;
                let accepted = run.best.filter(|b| {
                    super::score::at_least(
                        u128::from(l2 - b.0 as u64),
                        u128::from(l2),
                        r14::FUZZY_ACCEPT,
                    )
                });
                if let Some((d2, _, _, e2, qe)) = accepted {
                    let l2 = pat.len() as u128;
                    let qn = (u128::from(run.q1) * l2).min((l2 - d2 as u128) * self.l1);
                    let wsn = self.window_score(run.qs1, qe);
                    self.pending.push(Pend {
                        region: run.region,
                        s: run.start,
                        e: e2,
                        qs: run.qs1,
                        qe,
                        qn,
                        psn: run.psn,
                        wsn,
                        sfx: run.best_sfx,
                    });
                }
            } else {
                i += 1;
            }
        }
        self.found = found;
        self.ends_heap = self
            .ends
            .iter()
            .fold(0, |n, r| n.saturating_add(self.run_heap(Some(r))));
    }

    /// Scores the pending candidates whose suffix bytes have all arrived, or every one at the end of N.
    // spec: [F20 §6.4] "Score of a candidate", "Acceptance" (the order (score descending, s ascending))
    fn complete(&mut self, all: bool) {
        let want = self.spec.suffix.len();
        let mut pending = std::mem::take(&mut self.pending);
        let mut i = 0;
        while i < pending.len() {
            let e = pending[i].e;
            self.fill(&mut pending[i].sfx, e);
            if all || pending[i].sfx.len() >= want {
                let c = pending.swap_remove(i);
                self.score(&c);
            } else {
                i += 1;
            }
        }
        self.pending = pending;
    }

    /// Scores one candidate into its region's order.
    // spec: [F20 §6.4] "Score of a candidate", "Acceptance"
    fn score(&mut self, c: &Pend) {
        let ssn = if self.spec.suffix.is_empty() {
            1
        } else {
            self.s
                .saturating_sub(levenshtein(self.spec.suffix, &c.sfx) as u128)
        };
        let region = &mut self.regions[c.region];
        region.count += 1;
        let Some(wsn) = c.wsn else {
            region.window_unavailable = true;
            return;
        };
        let lq = self.l1 * self.l2;
        let [w1, w2, w3, w4] = r14::FUZZY_WEIGHTS.map(u128::from);
        let (p, s, w) = (self.p, self.s, self.w);
        let num = (|| {
            let a = w1
                .checked_mul(c.qn)?
                .checked_mul(p)?
                .checked_mul(s)?
                .checked_mul(w)?;
            let b = w2
                .checked_mul(c.psn)?
                .checked_mul(lq)?
                .checked_mul(s)?
                .checked_mul(w)?;
            let cc = w3
                .checked_mul(ssn)?
                .checked_mul(lq)?
                .checked_mul(p)?
                .checked_mul(w)?;
            let d = w4
                .checked_mul(wsn)?
                .checked_mul(lq)?
                .checked_mul(p)?
                .checked_mul(s)?;
            a.checked_add(b)?.checked_add(cc)?.checked_add(d)
        })();
        let (Some(num), Some(den)) = (num, self.den) else {
            self.den = None;
            return;
        };
        let found = Found {
            s: c.s,
            e: c.e,
            qs: c.qs,
            qe: c.qe,
            score: Score { num, den },
        };
        let ranked = Ranked { num, found };
        let region = &mut self.regions[c.region];
        let better =
            |x: &Ranked, y: &Ranked| x.num > y.num || (x.num == y.num && x.found.s < y.found.s);
        match region.best {
            None => region.best = Some(ranked),
            Some(b) if better(&ranked, &b) => {
                region.second = Some(b.num);
                region.best = Some(ranked);
            }
            Some(_) => {
                if region.second.is_none_or(|s2| num > s2) {
                    region.second = Some(num);
                }
            }
        }
    }

    /// The end of N.
    pub(crate) fn finish(&mut self) {
        let mut cands = std::mem::take(&mut self.cands);
        cands.clear();
        for (r, run) in self.regions.iter_mut().enumerate() {
            if !run.done {
                run.done = true;
                run.sel.finish(|c| cands.push((r, c)));
            }
        }
        for &(r, c) in &cands {
            let e = self.regions[r].start + c.end as u64;
            self.candidate(r, e, c.distance);
        }
        self.cands = cands;
        self.run_ends(true);
        self.complete(true);
    }

    /// The decision ([F20 §6.4] "Acceptance"): the first region with a candidate decides; its best is accepted when it
    /// is the only one or exceeds the second by the margin (`HEADER_MARGIN` under the same-kind restriction,
    /// `FUZZY_MARGIN` otherwise).
    // spec: [F20 §6.4] "Acceptance", "Same-kind headers"
    pub(crate) fn decide(&self) -> Fuzzy {
        let Some(den) = self.den else {
            return Fuzzy::Overflow;
        };
        let margin: Ratio = if self.spec.headers.is_some() {
            r14::HEADER_MARGIN
        } else {
            r14::FUZZY_MARGIN
        };
        for r in &self.regions {
            if r.count == 0 {
                continue;
            }
            if r.window_unavailable {
                return Fuzzy::WindowUnavailable;
            }
            let Some(best) = r.best else {
                return Fuzzy::Overflow;
            };
            let ok = match r.second {
                None => r.count == 1,
                Some(s2) => {
                    best.num > s2
                        && cmp_products(
                            best.num - s2,
                            u128::from(margin.den()),
                            u128::from(margin.num()),
                            den,
                        ) != core::cmp::Ordering::Less
                }
            };
            return if ok {
                Fuzzy::Edited(best.found)
            } else {
                Fuzzy::Ambiguous
            };
        }
        Fuzzy::Nothing
    }

    /// Once a complete region holds a candidate it decides ([F20 §6.4] "Acceptance": the first region that holds at
    /// least one candidate), so the later regions stop searching and their end searches and candidates are dropped.
    // spec: [F20 §6.4] "Acceptance" (the first region with a candidate decides)
    fn stop_later_regions(&mut self) {
        if self.decisive {
            return;
        }
        for r in 0..self.regions.len() {
            let holds = self.regions[r].count > 0 || self.pending.iter().any(|p| p.region == r);
            if self.regions[r].done && holds {
                for later in &mut self.regions[r + 1..] {
                    later.done = true;
                }
                self.ends.retain(|e| e.region <= r);
                self.pending.retain(|p| p.region <= r);
                self.decisive = true;
                return;
            }
        }
    }

    /// At most [`SLICE`] bytes of N.
    // spec: [F20 §6.4] "Regions", "Candidates in a region R" (each region's end offsets with d(e) ≤ k, selected)
    fn slice(&mut self, at: u64, b: &[u8]) {
        self.tail.push(at, b);
        self.map.prune(self.tail.start());
        let to = at + b.len() as u64;
        let mut cands = std::mem::take(&mut self.cands);
        cands.clear();
        for (r, run) in self.regions.iter_mut().enumerate() {
            if run.done {
                continue;
            }
            let lo = at.max(run.start);
            let hi = to.min(run.end);
            if lo < hi {
                let part = &b[(lo - at) as usize..(hi - at) as usize];
                let RegionRun { search, sel, .. } = run;
                search.feed(part, |h| sel.push(h, |c| cands.push((r, c))));
                sel.advance(search.position(), |c| cands.push((r, c)));
            }
            if to >= run.end {
                run.done = true;
                run.sel.finish(|c| cands.push((r, c)));
            }
        }
        for &(r, c) in &cands {
            let e = self.regions[r].start + c.end as u64;
            self.candidate(r, e, c.distance);
        }
        self.cands = cands;
        self.run_ends(false);
        self.complete(false);
        self.stop_later_regions();
    }
}

impl NSink for FuzzySink<'_> {
    fn start_line(&mut self, line: u64, at: u64) {
        self.line = line;
        self.map.start_line(line, at);
    }

    fn bytes(&mut self, at: u64, b: &[u8]) {
        let mut at = at;
        for part in b.chunks(SLICE) {
            self.slice(at, part);
            at += part.len() as u64;
        }
    }

    // spec: [F20 §1.3] input (e) (the end searches and the pending candidates, charged as they grow)
    fn retained(&self) -> usize {
        self.ends.capacity() * size_of::<EndRun<'_>>()
            + self.ends_heap
            + self.pending.capacity() * (size_of::<Pend>() + self.spec.suffix.len())
    }

    // spec: [F20 §1.3] input (e) (the bytes the end searches read again, charged as they grow)
    fn work(&self) -> u64 {
        self.searched
    }

    // spec: [F20 §6.4] "`range` anchors" (the end search stops after the spread's last line)
    fn end_line(&mut self, line: u64, at: u64) {
        let mut any = false;
        for run in &mut self.ends {
            if run.end.is_none() && run.limit <= line {
                run.end = Some(at);
                any = true;
            }
        }
        if any {
            self.run_ends(false);
            self.complete(false);
            self.stop_later_regions();
        }
    }
}
