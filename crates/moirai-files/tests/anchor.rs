//! WP-64: anchor capture and the anchor cascade ([F20 §6]; [F08 §10.3, §11.4]).
//!
//! - [`rf`] is a naive reference written in this file from [F20 §1.2, §2.5, §2.7, §2.8, §6.1–§6.5]: N held whole,
//!   every hit and every fuzzy end offset found by brute force (dynamic programming), the candidate selection, the
//!   scores and the margins evaluated by their definitions with exact rationals. The property tests compare the
//!   streaming product with it on generated texts and edits, under the interim scanner rule. Where the spec is open,
//!   it follows WP-64's readings (spec findings): the `range` pairing by the nearest line count with overlapping
//!   quotes allowed, the fuzzy end search from the start candidate, the quote input's own non-trivial lines, and texts
//!   longer than a capture writes leaving the quote steps `unverified (budget)`. The model's brute-force resolver
//!   (WP-92, P11 in WP-77) is the independent oracle; this one checks the streaming against the definitions.
//! - The scenario tests show each rule of the cascade and each capture refusal on a small text.
//! - The lifted-rule tests run the scanner steps of [F21 §2.4]–§2.6 and §6 that apply once review lifts [F20 §6.1]'s
//!   interim rule.
//!
//! Every text is synthetic and written in the test (PLAN R18).

use std::num::NonZeroU16;

use moirai_files::anchor::{
    Anchor, AnchorKind, AnchorState, Anchors, Budget, CaptureError, CaptureInput, Form, LineSpan,
    Mode, ReadParams, Refusal, Resolution, ResolveInput, ScannerRule, Score, SliceSource, Texts,
    Unlimited, UnverifiedReason, Watch, Window, capture_planned, parse_spec, quote_text,
};
use moirai_files::oid::{ObjectFormat, Oid};
use moirai_files::text::{ByteSource, Snapshot, Unavailable};
use moirai_files::uid::Uid;
use proptest::prelude::*;
use xxhash_rust::xxh3::xxh3_64;

const FILE: Uid = Uid([7; 16]);

fn params() -> ReadParams {
    ReadParams {
        format: ObjectFormat::Sha1,
        max_read_bytes: 1 << 24,
        max_line_hashes: 1 << 16,
    }
}

fn input<'a>(form: Form<'a>, path: &'a [u8], rule: ScannerRule) -> CaptureInput<'a> {
    CaptureInput {
        path,
        form,
        mode: Mode::Live,
        watch: None,
        file_uid: &FILE,
        git: None,
        read: params(),
        scanners: rule,
    }
}

fn cap(b: &[u8], form: Form<'_>) -> Result<Anchor, CaptureError> {
    Anchors::new()
        .capture(
            &mut SliceSource::new(b),
            &input(form, b"a.txt", ScannerRule::CURRENT),
            &mut Unlimited,
        )
        .map(|c| c.anchor)
}

fn span(first: u32, last: u32) -> LineSpan {
    LineSpan { first, last }
}

fn res(a: &Anchor, b: &[u8]) -> Resolution {
    res_with(a, b, b"a.txt", ScannerRule::CURRENT, params())
}

fn res_with(a: &Anchor, b: &[u8], path: &[u8], rule: ScannerRule, p: ReadParams) -> Resolution {
    let ri = ResolveInput {
        path,
        read: p,
        scanners: rule,
    };
    Anchors::new().resolve(&mut SliceSource::new(b), a, &ri, &mut Unlimited)
}

/// A budget of so many units.
struct Tight(u64);

impl Budget for Tight {
    fn spend(&mut self, units: u64) -> bool {
        if units > self.0 {
            return false;
        }
        self.0 -= units;
        true
    }
}

/// A source whose every read fails, with the size of `0` bytes it claims.
struct Broken(u64);

impl ByteSource for Broken {
    type Error = ();
    type Stamp = ();
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
        Err(())
    }
    fn rewind(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn snapshot(&self) -> Result<Snapshot<()>, Self::Error> {
        Ok(Snapshot {
            size: self.0,
            mtime: (),
        })
    }
}

/// A source that hands out the bytes in chunks of cycling sizes.
struct Chunky<'a> {
    data: &'a [u8],
    pos: usize,
    sizes: Vec<usize>,
    k: usize,
}

impl ByteSource for Chunky<'_> {
    type Error = std::convert::Infallible;
    type Stamp = u8;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let want = self.sizes[self.k % self.sizes.len()].max(1);
        self.k += 1;
        let n = want.min(buf.len()).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }
    fn snapshot(&self) -> Result<Snapshot<u8>, Self::Error> {
        Ok(Snapshot {
            size: self.data.len() as u64,
            mtime: 0,
        })
    }
}

// --- the naive reference -------------------------------------------------------------------------------------------

mod rf {
    use super::*;

    pub const WIN: usize = 16;

    pub fn is_ws(b: u8) -> bool {
        matches!(b, 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20)
    }

    /// [F20 §2.5] "Trivial line" over the bytes of a normalised line.
    pub fn trivial_bytes(l: &[u8]) -> bool {
        l.iter().all(|&b| is_ws(b) || b"{}()[];,".contains(&b))
    }

    pub fn nl(l: &[u8]) -> &[u8] {
        let a = l.iter().position(|&b| !is_ws(b)).unwrap_or(l.len());
        let z = l.iter().rposition(|&b| !is_ws(b)).map_or(a, |z| z + 1);
        &l[a..z.max(a)]
    }

    /// The normalised anchor text of a content ([F20 §2.5]).
    pub struct T {
        pub n: Vec<u8>,
        pub starts: Vec<usize>,
        pub lines: Vec<Vec<u8>>,
    }

    pub fn text(b: &[u8]) -> Option<T> {
        let t = moirai_files::text::atext(b)?;
        let mut pieces: Vec<&[u8]> = t.split(|&x| x == b'\n').collect();
        if pieces.last().is_some_and(|p| p.is_empty()) {
            pieces.pop();
        }
        let lines: Vec<Vec<u8>> = pieces.iter().map(|p| nl(p).to_vec()).collect();
        let mut n = Vec::new();
        let mut starts = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if i > 0 {
                n.push(b'\n');
            }
            starts.push(n.len());
            n.extend_from_slice(l);
        }
        Some(T { n, starts, lines })
    }

    impl T {
        pub fn m(&self) -> usize {
            self.lines.len()
        }
        pub fn start(&self, i: usize) -> usize {
            self.starts[i - 1]
        }
        pub fn end(&self, i: usize) -> usize {
            self.starts[i - 1] + self.lines[i - 1].len()
        }
        pub fn st(&self, s: usize, e: usize) -> &[u8] {
            &self.n[self.start(s)..self.end(e)]
        }
        pub fn line_at(&self, o: usize) -> usize {
            self.starts.partition_point(|&x| x <= o).max(1)
        }
        pub fn trivial(&self, i: usize) -> bool {
            trivial_bytes(&self.lines[i - 1])
        }
        pub fn wh(&self, i: usize) -> u16 {
            (xxh3_64(&self.lines[i - 1]) & 0xFFFF) as u16
        }
        /// `before(qs)` and `after(qe)` ([F20 §2.7.2]).
        pub fn window(&self, qs: usize, qe: usize) -> (Vec<u16>, Vec<u16>) {
            let mut before: Vec<u16> = (1..qs.min(self.m() + 1))
                .filter(|&i| !self.trivial(i))
                .map(|i| self.wh(i))
                .collect();
            if before.len() > WIN {
                before.drain(..before.len() - WIN);
            }
            let after: Vec<u16> = (qe + 1..=self.m())
                .filter(|&i| !self.trivial(i))
                .map(|i| self.wh(i))
                .take(WIN)
                .collect();
            (before, after)
        }
    }

    pub fn lcs(a: &[u16], b: &[u16]) -> usize {
        let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for i in 1..=a.len() {
            for j in 1..=b.len() {
                d[i][j] = if a[i - 1] == b[j - 1] {
                    d[i - 1][j - 1] + 1
                } else {
                    d[i - 1][j].max(d[i][j - 1])
                };
            }
        }
        d[a.len()][b.len()]
    }

    pub fn lev(a: &[u8], b: &[u8]) -> usize {
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        for i in 1..=a.len() {
            let mut cur = vec![i; b.len() + 1];
            for j in 1..=b.len() {
                cur[j] = (prev[j - 1] + usize::from(a[i - 1] != b[j - 1]))
                    .min(prev[j] + 1)
                    .min(cur[j - 1] + 1);
            }
            prev = cur;
        }
        prev[b.len()]
    }

    fn cont(b: u8) -> bool {
        (0x80..=0xBF).contains(&b)
    }

    pub fn cutp(x: &[u8], n: usize) -> &[u8] {
        (0..=n.min(x.len()))
            .rev()
            .find(|&k| k == x.len() || !cont(x[k]))
            .map_or(&x[..0], |k| &x[..k])
    }

    pub fn cuts(x: &[u8], n: usize) -> &[u8] {
        let lo = x.len().saturating_sub(n);
        (lo..=x.len())
            .find(|&s| s == x.len() || !cont(x[s]))
            .map_or(&x[x.len()..], |s| &x[s..])
    }

    /// An exact non-negative rational.
    #[derive(Clone, Copy, Debug)]
    pub struct Q(pub u128, pub u128);

    impl Q {
        pub fn ge(self, o: Q) -> bool {
            self.0 * o.1 >= o.0 * self.1
        }
        pub fn gt(self, o: Q) -> bool {
            self.0 * o.1 > o.0 * self.1
        }
        /// `self − o ≥ m`, for `self ≥ o`.
        pub fn beats(self, o: Q, m: Q) -> bool {
            self.gt(o) && (self.0 * o.1 - o.0 * self.1) * m.1 >= m.0 * self.1 * o.1
        }
        pub fn add(self, o: Q) -> Q {
            Q(self.0 * o.1 + o.0 * self.1, self.1 * o.1)
        }
        pub fn scale(self, w: u128) -> Q {
            Q(self.0 * w, self.1)
        }
    }

    pub fn wscore(stored: &(Vec<u16>, Vec<u16>), cand: &(Vec<u16>, Vec<u16>)) -> Q {
        let den = stored.0.len() + stored.1.len();
        if den == 0 {
            return Q(0, 1);
        }
        Q(
            (lcs(&stored.0, &cand.0) + lcs(&stored.1, &cand.1)) as u128,
            den as u128,
        )
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Hit {
        pub h: usize,
        pub z: usize,
        pub qs: usize,
        pub qe: usize,
    }

    fn occurrences(n: &[u8], q: &[u8]) -> Vec<usize> {
        (0..=n.len().saturating_sub(q.len()))
            .filter(|&h| n.len() >= q.len() && &n[h..h + q.len()] == q)
            .collect()
    }

    /// [F20 §6.2] step 4; for a `range`, `end` is the end quote and the captured line count, and a start pairs with the
    /// end hit at or after it (`h_e ≥ h`, `h_e + len(end) ≥ h + len(exact)`) within the spread whose line count is
    /// nearest the captured one, the earlier on a tie.
    pub fn hits(t: &T, quote: &[u8], end: Option<(&[u8], usize)>) -> Vec<Hit> {
        let starts = occurrences(&t.n, quote);
        match end {
            None => starts
                .into_iter()
                .map(|h| Hit {
                    h,
                    z: h + quote.len(),
                    qs: t.line_at(h),
                    qe: t.line_at(h + quote.len() - 1),
                })
                .collect(),
            Some((e, lines)) => {
                let ends = occurrences(&t.n, e);
                starts
                    .into_iter()
                    .filter_map(|h| {
                        let z = h + quote.len();
                        let qs = t.line_at(h);
                        let target = qs + lines - 1;
                        let (he, last) = ends
                            .iter()
                            .copied()
                            .filter(|&he| he >= h && he + e.len() >= z)
                            .map(|he| (he, t.line_at(he + e.len() - 1)))
                            .filter(|&(_, last)| last < qs + 2 * lines)
                            .min_by_key(|&(he, last)| (last.abs_diff(target), he))?;
                        Some(Hit {
                            h,
                            z: he + e.len(),
                            qs,
                            qe: last,
                        })
                    })
                    .collect()
            }
        }
    }

    fn agreement(a: usize, len: usize) -> Q {
        if len == 0 {
            Q(1, 1)
        } else {
            Q(a as u128, len as u128)
        }
    }

    pub fn ctx(t: &T, h: &Hit, prefix: &[u8], suffix: &[u8]) -> Q {
        let before = &t.n[h.h.saturating_sub(prefix.len())..h.h];
        let pa = prefix
            .iter()
            .rev()
            .zip(before.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let after = &t.n[h.z..(h.z + suffix.len()).min(t.n.len())];
        let sa = suffix.iter().zip(after).take_while(|(a, b)| a == b).count();
        let s = agreement(pa, prefix.len()).add(agreement(sa, suffix.len()));
        Q(s.0, s.1 * 2)
    }

    /// The unique best of `(score, item)` pairs with margin `m`.
    fn unique<X: Copy>(v: &[(Q, X)], m: Q) -> Option<X> {
        let mut order: Vec<usize> = (0..v.len()).collect();
        order.sort_by(|&a, &b| {
            if v[a].0.gt(v[b].0) {
                std::cmp::Ordering::Less
            } else if v[b].0.gt(v[a].0) {
                std::cmp::Ordering::Greater
            } else {
                a.cmp(&b)
            }
        });
        let best = v[order[0]];
        match order.get(1) {
            None => Some(best.1),
            Some(&s) => best.0.beats(v[s].0, m).then_some(best.1),
        }
    }

    /// [F20 §6.2] steps 5–6: `None` without a hit, `Some(None)` for `ambiguous`.
    pub fn decide(
        t: &T,
        hs: &[Hit],
        prefix: &[u8],
        suffix: &[u8],
        window: &(Vec<u16>, Vec<u16>),
        occurrence: Option<usize>,
    ) -> Option<Option<Hit>> {
        match hs.len() {
            0 => return None,
            1 => return Some(Some(hs[0])),
            _ => {}
        }
        let c: Vec<(Q, Hit)> = hs.iter().map(|h| (ctx(t, h, prefix, suffix), *h)).collect();
        if let Some(h) = unique(&c, Q(1, 10)) {
            return Some(Some(h));
        }
        if !window.0.is_empty() || !window.1.is_empty() {
            let w: Vec<(Q, Hit)> = hs
                .iter()
                .map(|h| (wscore(window, &t.window(h.qs, h.qe)), *h))
                .collect();
            if let Some(h) = unique(&w, Q(15, 100)) {
                return Some(Some(h));
            }
        }
        if let Some(o) = occurrence
            && let Some(h) = hs.get(o - 1)
        {
            return Some(Some(*h));
        }
        Some(None)
    }

    /// The capture of `path:L-M` ([F20 §6.1]).
    pub fn capture_lines(b: &[u8], l: usize, m: usize) -> Result<Anchor, &'static str> {
        let t = text(b).ok_or("binary")?;
        if l == 0 || l > m || m > t.m() {
            return Err("range");
        }
        let nt: Vec<usize> = (l..=m).filter(|&i| !t.trivial(i)).collect();
        let (Some(&s), Some(&e)) = (nt.first(), nt.last()) else {
            let window = t.window(l, m);
            if window.0.is_empty() && window.1.is_empty() {
                return Err("no-window");
            }
            let w = Window::new(&window.0, &window.1).unwrap();
            let wb = w.to_bytes();
            let c = moirai_files::uid::captured(&moirai_files::uid::Capture {
                file_uid: &FILE,
                kind: AnchorKind::Lines,
                scope: &[],
                quote: &[],
                prefix: &[],
                suffix: &[],
                end: &[],
                occurrence: None,
                window: &wb,
            })
            .unwrap();
            return Ok(Anchor {
                kind: AnchorKind::Lines,
                mode: Mode::Live,
                watch: Watch::Span,
                resolver: 1,
                captured: c,
                pred: None,
                hint: Some(span(l as u32, m as u32)),
                scope: None,
                texts: None,
                occurrence: None,
                window: Some(w),
                span_hash: Some(xxh3_64(t.st(l, m))),
                blob: moirai_files::text::oid_of(ObjectFormat::Sha1, b),
                git: None,
                marker: None,
            });
        };
        let st = t.st(s, e).to_vec();
        let kind = if nt.len() <= 4 && st.len() <= 128 {
            AnchorKind::Quote
        } else {
            AnchorKind::Range
        };
        Ok(finish(b, &t, kind, &st, t.start(s), t.end(e), (s, e)))
    }

    /// The capture of a quote input ([F20 §6.1] step 3), its occurrence within the lines `within` when given.
    pub fn capture_quote(
        b: &[u8],
        input: &[u8],
        within: Option<(usize, usize)>,
    ) -> Result<Anchor, &'static str> {
        let q = quote_text(input).map_err(|r| r.case())?;
        let t = text(b).ok_or("binary")?;
        let at = |h: usize| (t.line_at(h), t.line_at(h + q.len() - 1));
        let h = occurrences(&t.n, &q)
            .into_iter()
            .find(|&h| within.is_none_or(|(w1, w2)| at(h).0 >= w1 && at(h).1 <= w2))
            .ok_or("not-found")?;
        let (s, e) = at(h);
        let nt = q
            .split(|&x| x == b'\n')
            .filter(|l| !trivial_bytes(l))
            .count();
        let kind = if nt <= 4 && q.len() <= 128 {
            AnchorKind::Quote
        } else {
            AnchorKind::Range
        };
        Ok(finish(b, &t, kind, &q, h, h + q.len(), (s, e)))
    }

    fn finish(
        b: &[u8],
        t: &T,
        kind: AnchorKind,
        text: &[u8],
        o: usize,
        o2: usize,
        (s, e): (usize, usize),
    ) -> Anchor {
        let (quote, end) = if kind == AnchorKind::Range {
            (cutp(text, 64).to_vec(), Some(cuts(text, 64).to_vec()))
        } else {
            (text.to_vec(), None)
        };
        let ctxw = |n: usize| (cuts(&t.n[..o], n).to_vec(), cutp(&t.n[o2..], n).to_vec());
        let window = t.window(s, e);
        let hs = hits(t, &quote, end.as_deref().map(|x| (x, e - s + 1)));
        let is_captured = |p: &[u8], q: &[u8]| matches!(decide(t, &hs, p, q, &window, None), Some(Some(h)) if h.h == o);
        let (p32, s32) = ctxw(32);
        let (p64, s64) = ctxw(64);
        let (prefix, suffix, occurrence) = if is_captured(&p32, &s32) {
            (p32, s32, None)
        } else if is_captured(&p64, &s64) {
            (p64, s64, None)
        } else {
            let idx = hs
                .iter()
                .position(|h| h.h == o)
                .expect("the captured position is a hit");
            (p64, s64, NonZeroU16::new(idx as u16 + 1))
        };
        let c = moirai_files::uid::captured(&moirai_files::uid::Capture {
            file_uid: &FILE,
            kind,
            scope: &[],
            quote: &quote,
            prefix: &prefix,
            suffix: &suffix,
            end: end.as_deref().unwrap_or(&[]),
            occurrence,
            window: &[],
        })
        .unwrap();
        Anchor {
            kind,
            mode: Mode::Live,
            watch: Watch::Span,
            resolver: 1,
            captured: c,
            pred: None,
            hint: Some(span(s as u32, e as u32)),
            scope: None,
            texts: Some(Texts::Held {
                quote,
                prefix,
                suffix,
                end,
            }),
            occurrence,
            window: Some(Window::new(&window.0, &window.1).unwrap()),
            span_hash: Some(xxh3_64(t.st(s, e))),
            blob: moirai_files::text::oid_of(ObjectFormat::Sha1, b),
            git: None,
            marker: None,
        }
    }

    /// `d(e)` for every end offset of the region `[lo, hi)`: the least distance of `pat` to a substring
    /// `N[s .. e)` with `s ≥ lo` ([F20 §6.4]); entry j is `d(lo + j)`.
    fn dists(n: &[u8], pat: &[u8], lo: usize, hi: usize) -> Vec<usize> {
        let m = pat.len();
        let mut col: Vec<usize> = (0..=m).collect();
        let mut out = vec![m];
        for &c in &n[lo..hi] {
            let mut new = vec![0usize; m + 1];
            for i in 1..=m {
                new[i] = (col[i - 1] + usize::from(pat[i - 1] != c))
                    .min(col[i] + 1)
                    .min(new[i - 1] + 1);
            }
            out.push(new[m]);
            col = new;
        }
        out
    }

    /// The candidates of a region ([F20 §6.4] "Candidates in a region R"), with ends at least `min_end`: `(s, e, d)`.
    pub fn candidates(
        n: &[u8],
        pat: &[u8],
        k: usize,
        (lo, hi): (usize, usize),
        min_end: usize,
    ) -> Vec<(usize, usize, usize)> {
        let d = dists(n, pat, lo, hi);
        let mut e: Vec<(usize, usize)> = d
            .iter()
            .enumerate()
            .filter(|&(j, &x)| x <= k && lo + j >= min_end)
            .map(|(j, &x)| (x, lo + j))
            .collect();
        e.sort_unstable();
        let mut kept: Vec<(usize, usize)> = Vec::new();
        for (x, end) in e {
            if kept.iter().all(|&(_, y)| y.abs_diff(end) >= pat.len()) {
                kept.push((x, end));
            }
        }
        kept.into_iter()
            .map(|(x, end)| {
                // A substring longer than len(pat) + x is more than x away, so no start lies further back.
                let first = lo.max(end.saturating_sub(pat.len() + x));
                let s = (first..=end)
                    .rev()
                    .find(|&s| lev(pat, &n[s..end]) == x)
                    .unwrap();
                (s, end, x)
            })
            .collect()
    }

    /// The cascade ([F20 §6.2]–§6.5), interim scanner rule.
    pub fn resolve(a: &Anchor, b: &[u8]) -> Resolution {
        let of = |state| Resolution {
            state,
            span: None,
            score: None,
            scope_only: false,
            reason: None,
        };
        let at = |state, s: usize, e: usize| Resolution {
            span: Some(span(s as u32, e as u32)),
            ..of(state)
        };
        let Some(t) = text(b) else {
            return of(AnchorState::Orphaned);
        };
        let h = a.hint.unwrap();
        let (h1, h2) = (h.first as usize, h.last as usize);
        let sh = a.span_hash.unwrap();
        if h2 <= t.m() && xxh3_64(t.st(h1, h2)) == sh {
            return at(AnchorState::Fresh, h1, h2);
        }
        let w = a.window.unwrap();
        let stored = (w.before().to_vec(), w.after().to_vec());
        if a.kind == AnchorKind::Lines {
            let len = h2 - h1 + 1;
            if t.m() < len {
                return of(AnchorState::Orphaned);
            }
            let v: Vec<(Q, usize)> = (1..=t.m() - len + 1)
                .map(|j| (wscore(&stored, &t.window(j, j + len - 1)), j))
                .collect();
            let best = v
                .iter()
                .copied()
                .fold(None::<(Q, usize)>, |acc, x| match acc {
                    Some(b) if !x.0.gt(b.0) => Some(b),
                    _ => Some(x),
                })
                .unwrap();
            let second = v.iter().filter(|x| x.1 != best.1).map(|x| x.0).fold(
                None::<Q>,
                |acc, x| match acc {
                    Some(b) if !x.gt(b) => Some(b),
                    _ => Some(x),
                },
            );
            let den = (stored.0.len() + stored.1.len()).max(1) as u128;
            let ok = best.0.ge(Q(1, 2)) && second.is_none_or(|s| best.0.beats(s, Q(15, 100)));
            let _ = den;
            if !ok {
                return of(AnchorState::Orphaned);
            }
            let j = best.1;
            return if xxh3_64(t.st(j, j + len - 1)) != sh {
                of(AnchorState::Orphaned)
            } else if j == h1 {
                at(AnchorState::Fresh, j, j + len - 1)
            } else {
                at(AnchorState::Moved, j, j + len - 1)
            };
        }
        let Some(Texts::Held {
            quote,
            prefix,
            suffix,
            end,
        }) = &a.texts
        else {
            panic!("held texts");
        };
        if quote.len() > 128
            || end.as_ref().is_some_and(|e| e.len() > 128)
            || prefix.len() > 64
            || suffix.len() > 64
        {
            return Resolution {
                reason: Some(UnverifiedReason::Unavailable(Unavailable::Budget)),
                ..of(AnchorState::Unverified)
            };
        }
        let lines = h2 - h1 + 1;
        let spread = 2 * lines;
        let hs = hits(&t, quote, end.as_deref().map(|e| (e, lines)));
        match decide(
            &t,
            &hs,
            prefix,
            suffix,
            &stored,
            a.occurrence.map(|o| usize::from(o.get())),
        ) {
            Some(Some(hit)) => {
                let (s, e) = (hit.qs, hit.qe);
                return if e > t.m() || xxh3_64(t.st(s, e)) != sh {
                    at(AnchorState::Edited, s, e)
                } else if (s, e) == (h1, h2) {
                    at(AnchorState::Fresh, s, e)
                } else {
                    at(AnchorState::Moved, s, e)
                };
            }
            Some(None) => return of(AnchorState::Ambiguous),
            None => {}
        }
        // The fuzzy quote.
        let len = t.n.len();
        let mut regions = Vec::new();
        if h1 <= t.m() {
            let x = t.start(h1);
            let z = t.end(h2.min(t.m()));
            regions.push((x.saturating_sub(16_384), (z + 16_384).min(len)));
        }
        regions.push((0, len));
        let k = quote.len() / 4;
        let p = prefix.len();
        let s_len = suffix.len();
        let score = |q: Q, s: usize, e: usize, qs: usize, qe: usize| -> Q {
            let ps = if p == 0 {
                Q(1, 1)
            } else {
                Q(
                    (p - lev(prefix, &t.n[s.saturating_sub(p)..s])) as u128,
                    p as u128,
                )
            };
            let ss = if s_len == 0 {
                Q(1, 1)
            } else {
                let after = &t.n[e..(e + s_len).min(len)];
                Q((s_len - lev(suffix, after)) as u128, s_len as u128)
            };
            let ws = wscore(&stored, &t.window(qs, qe));
            let sum = q
                .scale(50)
                .add(ps.scale(20))
                .add(ss.scale(20))
                .add(ws.scale(10));
            Q(sum.0, sum.1 * 100)
        };
        for (lo, hi) in regions {
            let mut cands: Vec<(Q, usize, usize, usize, usize)> = Vec::new();
            for (s, e, d) in candidates(&t.n, quote, k, (lo, hi), 0) {
                let q1 = Q((quote.len() - d) as u128, quote.len() as u128);
                if !q1.ge(Q(3, 4)) {
                    continue;
                }
                match end {
                    None => {
                        let (qs, qe) = (t.line_at(s), t.line_at(e - 1));
                        cands.push((score(q1, s, e, qs, qe), s, e, qs, qe));
                    }
                    Some(endq) => {
                        let qs = t.line_at(s);
                        let limit = qs + spread - 1;
                        if t.line_at(e - 1) > limit {
                            continue;
                        }
                        let ehi = if limit >= t.m() { len } else { t.end(limit) };
                        let k2 = endq.len() / 4;
                        // From the start candidate's first byte, ending at or after its end; the best by (distance,
                        // line-count distance, start).
                        let best = candidates(&t.n, endq, k2, (s, ehi), e)
                            .into_iter()
                            .min_by_key(|&(s2, e2, d2)| {
                                (d2, (t.line_at(e2 - 1) + 1 - qs).abs_diff(lines), s2)
                            });
                        if let Some((_, e2, d2)) = best {
                            let q2 = Q((endq.len() - d2) as u128, endq.len() as u128);
                            if !q2.ge(Q(3, 4)) {
                                continue;
                            }
                            let q = if q1.ge(q2) { q2 } else { q1 };
                            let qe = t.line_at(e2 - 1);
                            cands.push((score(q, s, e2, qs, qe), s, e2, qs, qe));
                        }
                    }
                }
            }
            if cands.is_empty() {
                continue;
            }
            cands.sort_by(|a, b| {
                if a.0.gt(b.0) {
                    std::cmp::Ordering::Less
                } else if b.0.gt(a.0) {
                    std::cmp::Ordering::Greater
                } else {
                    a.1.cmp(&b.1)
                }
            });
            let best = cands[0];
            let ok = cands.len() == 1 || best.0.beats(cands[1].0, Q(2, 100));
            return if ok {
                Resolution {
                    score: Some(Score {
                        num: best.0.0,
                        den: best.0.1,
                    }),
                    ..at(AnchorState::Edited, best.3, best.4)
                }
            } else {
                of(AnchorState::Ambiguous)
            };
        }
        of(AnchorState::Orphaned)
    }
}

// --- generated texts and edits -------------------------------------------------------------------------------------

/// Lines of the generated texts: short code lines (five of them make a `range` whose quotes overlap), long lines
/// (ranges whose quotes do not), and lines of two-, three- and four-byte characters, so that the cuts at 32, 64 and
/// 128 bytes fall at every phase of a character.
const VOCAB: &[&str] = &[
    "    let total = compute_the_sum(alpha_values, beta_values, gamma_values);",
    "// Grüße, ação, €uro and 😀 — a line of mixed widths for the cuts",
    "    печать(\"данные\"); // комментарий к этой строке",
    "€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€€",
    "x😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀😀",
    "        assert_eq!(first_long_identifier_name, second_long_identifier_name);",
    "fn alpha() {",
    "    let x = 1;",
    "}",
    "",
    "    beta(x);",
    "// note on alpha",
    "  ",
    "x",
    "let y = 2;",
    "    return x;",
    "});",
    "## Heading",
    "    let x = 1; // again",
    "fn alpha() { beta(); }",
];

fn texts() -> impl Strategy<Value = Vec<String>> {
    proptest::collection::vec(
        proptest::sample::select(VOCAB.to_vec()).prop_map(str::to_owned),
        3..40,
    )
}

#[derive(Clone, Debug)]
enum Edit {
    Insert(usize, String),
    Delete(usize),
    Byte(usize, usize, u8),
    Copy(usize, usize, usize),
}

fn edits() -> impl Strategy<Value = Vec<Edit>> {
    proptest::collection::vec(
        prop_oneof![
            (any::<usize>(), proptest::sample::select(VOCAB.to_vec()))
                .prop_map(|(p, s)| Edit::Insert(p, s.to_owned())),
            any::<usize>().prop_map(Edit::Delete),
            (
                any::<usize>(),
                any::<usize>(),
                proptest::sample::select(vec![b'z', b' ', b'(', b'1'])
            )
                .prop_map(|(l, c, b)| Edit::Byte(l, c, b)),
            (any::<usize>(), 1usize..4, any::<usize>()).prop_map(|(f, n, t)| Edit::Copy(f, n, t)),
        ],
        0..6,
    )
}

fn apply(lines: &[String], es: &[Edit]) -> Vec<String> {
    let mut v = lines.to_vec();
    for e in es {
        match e {
            Edit::Insert(p, s) => {
                let p = p % (v.len() + 1);
                v.insert(p, s.clone());
            }
            Edit::Delete(p) if !v.is_empty() => {
                let p = p % v.len();
                v.remove(p);
            }
            Edit::Byte(l, c, b) if !v.is_empty() => {
                let l = l % v.len();
                let mut bytes = v[l].clone().into_bytes();
                if bytes.is_empty() {
                    bytes.push(*b);
                } else {
                    let c = c % bytes.len();
                    bytes[c] = *b;
                }
                // A byte inside a multi-byte character leaves U+FFFD: content may hold it (only quote input may not).
                v[l] = String::from_utf8_lossy(&bytes).into_owned();
            }
            Edit::Copy(f, n, t) if !v.is_empty() => {
                let f = f % v.len();
                let n = (*n).min(v.len() - f);
                let block: Vec<String> = v[f..f + n].to_vec();
                let t = t % (v.len() + 1);
                for (i, s) in block.into_iter().enumerate() {
                    v.insert(t + i, s);
                }
            }
            _ => {}
        }
    }
    v
}

fn join(lines: &[String], crlf: bool) -> Vec<u8> {
    let sep = if crlf { "\r\n" } else { "\n" };
    let mut s = lines.join(sep);
    s.push_str(sep);
    s.into_bytes()
}

fn chunky_resolve(a: &Anchor, b: &[u8], sizes: Vec<usize>) -> Resolution {
    let ri = ResolveInput {
        path: b"a.txt",
        read: params(),
        scanners: ScannerRule::CURRENT,
    };
    let mut src = Chunky {
        data: b,
        pos: 0,
        sizes,
        k: 0,
    };
    Anchors::new().resolve(&mut src, a, &ri, &mut Unlimited)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    /// Capture of `path:L-M` equals the reference's, and resolving the captured content gives `fresh` at the hint.
    #[test]
    fn capture_matches_the_reference(lines in texts(), l in 1usize..40, len in 0usize..8, crlf in any::<bool>()) {
        let b = join(&lines, crlf);
        let m = l + len;
        let want = rf::capture_lines(&b, l, m);
        let got = cap(&b, Form::Lines(span(l as u32, m as u32)));
        match (&want, &got) {
            (Ok(w), Ok(g)) => {
                prop_assert_eq!(w, g);
                let r = res(g, &b);
                prop_assert_eq!(r.state, AnchorState::Fresh);
                prop_assert_eq!(r.span, g.hint);
            }
            (Err(w), Err(CaptureError::Refused(r))) => prop_assert_eq!(*w, r.case()),
            _ => prop_assert!(false, "reference {:?}, product {:?}", want, got),
        }
    }

    /// The cascade over an edited content equals the reference's, whatever the chunking of the reads.
    #[test]
    fn resolve_matches_the_reference(lines in texts(), l in 1usize..40, len in 0usize..8, es in edits(),
                                     crlf in any::<bool>(),
                                     sizes in proptest::collection::vec(1usize..300, 1..6)) {
        let b = join(&lines, crlf);
        let m = l + len;
        let Ok(a) = cap(&b, Form::Lines(span(l as u32, m as u32))) else { return Ok(()) };
        let b2 = join(&apply(&lines, &es), crlf);
        let want = rf::resolve(&a, &b2);
        let got = res(&a, &b2);
        prop_assert_eq!(got, want);
        prop_assert_eq!(chunky_resolve(&a, &b2, sizes), want);
    }

    /// Quote-input captures — whole lines, or text that starts and ends inside a line, with or without `within` —
    /// equal the reference's and resolve like it after edits.
    #[test]
    fn quote_input_matches_the_reference(lines in texts(), from in 0usize..40, n in 1usize..6,
                                         cut in (0usize..80, 0usize..80),
                                         within in proptest::option::of((1usize..40, 0usize..12)), es in edits()) {
        let b = join(&lines, false);
        let Some(t) = rf::text(&b) else { return Ok(()) };
        if t.m() == 0 { return Ok(()) }
        let s = from % t.m() + 1;
        let e = (s + n - 1).min(t.m());
        // Cut inside the first and the last line; the cuts 0 give whole lines.
        let (a, z) = (t.start(s), t.end(e));
        let a2 = a + cut.0 % (t.end(s) - a + 1);
        let z2 = z - cut.1 % (z - t.start(e) + 1);
        let (a2, z2) = if a2 < z2 { (a2, z2) } else { (a, z) };
        let mut q = b"\xEF\xBB\xBF".to_vec();
        q.extend_from_slice(&t.n[a2..z2]);
        let w = within.map(|(w1, len)| (w1, w1 + len));
        let want = rf::capture_quote(&b, &q, w);
        let wspan = w.map(|(w1, w2)| span(w1 as u32, w2 as u32));
        let got = cap(&b, Form::Quote { text: &q, within: wspan });
        match (&want, &got) {
            (Ok(w), Ok(g)) => {
                prop_assert_eq!(w, g);
                let b2 = join(&apply(&lines, &es), false);
                prop_assert_eq!(res(g, &b2), rf::resolve(w, &b2));
            }
            (Err(w), Err(CaptureError::Refused(r))) => prop_assert_eq!(*w, r.case()),
            _ => prop_assert!(false, "reference {:?}, product {:?}", want, got),
        }
    }
}

// --- scenarios: capture --------------------------------------------------------------------------------------------

const DOC: &[u8] = b"# Lock\n\nThe writer byte is taken with LockFileEx.\nA holder that dies leaves the byte to the OS.\n\n## Timeouts\n\nWaiters retry every 5 ms for at most 2 s.\n";

#[test]
fn spans_skip_trivial_lines_and_choose_the_kind() {
    // Lines 2–4: line 2 is blank, so the quote starts on line 3; two lines of 88 bytes are a quote.
    let a = cap(DOC, Form::Lines(span(2, 4))).unwrap();
    assert_eq!(a.kind, AnchorKind::Quote);
    assert_eq!(a.hint, Some(span(3, 4)));
    let a = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    assert_eq!(a.kind, AnchorKind::Quote);
    let Some(Texts::Held {
        quote,
        prefix,
        suffix,
        end,
    }) = &a.texts
    else {
        panic!()
    };
    assert_eq!(quote, b"The writer byte is taken with LockFileEx.");
    assert_eq!(prefix, b"# Lock\n\n");
    assert_eq!(suffix, b"\nA holder that dies leaves the b");
    assert_eq!(end, &None);
    assert_eq!(a.watch, Watch::Span);
    assert_eq!(a.resolver, 1);
    assert_eq!(a.window.unwrap().before().len(), 1);
    assert_eq!(a.window.unwrap().after().len(), 3);
    assert_eq!(
        a.span_hash,
        Some(xxh3_64(b"The writer byte is taken with LockFileEx."))
    );
    // A range: start and end quotes of at most 64 bytes.
    let a = cap(DOC, Form::Lines(span(3, 8))).unwrap();
    assert_eq!(a.kind, AnchorKind::Range);
    let Some(Texts::Held { quote, end, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!(quote.len(), 64);
    assert_eq!(end.as_deref().map(<[u8]>::len), Some(64));
    assert!(
        end.as_deref()
            .unwrap()
            .ends_with(b"\n\nWaiters retry every 5 ms for at most 2 s.")
    );
    // Only trivial lines: a `lines` anchor with the window around them.
    let a = cap(DOC, Form::Lines(span(5, 5))).unwrap();
    assert_eq!(a.kind, AnchorKind::Lines);
    assert_eq!(a.texts, None);
    assert!(!a.window.unwrap().is_empty());
}

#[test]
fn capture_refusals() {
    let r = |b: &[u8], f| match cap(b, f) {
        Err(CaptureError::Refused(r)) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(r(DOC, Form::Lines(span(0, 1))), Refusal::Range { lines: 8 });
    assert_eq!(r(DOC, Form::Lines(span(4, 9))), Refusal::Range { lines: 8 });
    assert_eq!(r(DOC, Form::Lines(span(3, 2))), Refusal::Range { lines: 8 });
    assert_eq!(r(b"a\0b\n", Form::Lines(span(1, 1))), Refusal::Binary);
    assert_eq!(
        r(
            DOC,
            Form::Quote {
                text: b"x\xEF\xBF\xBDy",
                within: None
            }
        ),
        Refusal::Fffd
    );
    assert_eq!(
        r(
            DOC,
            Form::Quote {
                text: b" \r\n\t\n",
                within: None
            }
        ),
        Refusal::Empty
    );
    assert_eq!(
        r(
            DOC,
            Form::Quote {
                text: b"not in the file",
                within: None
            }
        ),
        Refusal::NotFound
    );
    assert_eq!(r(DOC, Form::Symbol("Lock")), Refusal::NoScanner);
    assert_eq!(r(DOC, Form::Heading("Lock")), Refusal::NoScanner);
    assert_eq!(r(b"{\n}\n", Form::Lines(span(1, 2))), Refusal::NoWindow);
    let mut i = input(Form::Lines(span(3, 3)), b"a.txt", ScannerRule::CURRENT);
    i.watch = Some(Watch::Header);
    let e = Anchors::new().capture(&mut SliceSource::new(DOC), &i, &mut Unlimited);
    assert_eq!(e.unwrap_err(), CaptureError::Refused(Refusal::HeaderWatch));
    assert_eq!(Refusal::NoWindow.case(), "no-window");
}

#[test]
fn quote_input_rules() {
    assert_eq!(
        quote_text(b"\xEF\xBB\xBF  a \r\n\r\n  b  \n\n").unwrap(),
        b"a\n\nb"
    );
    // A capture by quote text equals the capture of the lines it covers when the text is those lines.
    let by_text = cap(
        DOC,
        Form::Quote {
            text: b"\xEF\xBB\xBFThe writer byte is taken with LockFileEx.\r\n",
            within: None,
        },
    )
    .unwrap();
    let by_lines = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    assert_eq!(by_text, by_lines);
    // A quote inside a line: its lines are the hint, and `span_hash` covers the whole line.
    let a = cap(
        DOC,
        Form::Quote {
            text: b"LockFileEx",
            within: None,
        },
    )
    .unwrap();
    assert_eq!(a.hint, Some(span(3, 3)));
    assert_eq!(
        a.span_hash,
        Some(xxh3_64(b"The writer byte is taken with LockFileEx."))
    );
    // `within` restricts the occurrence.
    let b = b"x = 1\ny = 2\nx = 1\n";
    let a = cap(
        b,
        Form::Quote {
            text: b"x = 1",
            within: Some(span(2, 3)),
        },
    )
    .unwrap();
    assert_eq!(a.hint, Some(span(3, 3)));
}

#[test]
fn the_uniqueness_ladder() {
    // Two hits with equal 32-byte contexts and no usable window (the array is capped): rung 1 widens the context,
    // and the 64-byte prefix tells them apart.
    let long = "0123456789abcdefghijklmnopqrstuvwxyz0123";
    let tail = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
    let b = format!("one\n{long}\nsame line\n{tail}\ntwo\n{long}\nsame line\n{tail}\n");
    let mut i = input(Form::Lines(span(7, 7)), b"a.txt", ScannerRule::CURRENT);
    i.read.max_line_hashes = 2;
    let a = Anchors::new()
        .capture(&mut SliceSource::new(b.as_bytes()), &i, &mut Unlimited)
        .unwrap()
        .anchor;
    let Some(Texts::Held { prefix, suffix, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!((prefix.len(), suffix.len()), (64, 41));
    assert_eq!(a.occurrence, None);
    // With the window available, the window step decides at the first width.
    let a = cap(b.as_bytes(), Form::Lines(span(7, 7))).unwrap();
    let Some(Texts::Held { prefix, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!(prefix.len(), 32);
    // Identical lines everywhere: nothing but the occurrence tells the hits apart.
    let b: String = "same line\n".repeat(60);
    let a = cap(b.as_bytes(), Form::Lines(span(30, 30))).unwrap();
    assert_eq!(a.occurrence, NonZeroU16::new(30));
    assert_eq!(rf::capture_lines(b.as_bytes(), 30, 30).unwrap(), a);
    assert_eq!(res(&a, b.as_bytes()).state, AnchorState::Fresh);
    // The hint line changed: the occurrence decides among the 59 hits.
    let mut lines: Vec<String> = b.lines().map(str::to_owned).collect();
    lines[29] = "changed".to_owned();
    let b2 = join(&lines, false);
    let r = res(&a, &b2);
    assert_eq!(r, rf::resolve(&a, &b2));
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(31, 31))));
}

#[test]
fn file_and_planned_anchors() {
    let a = cap(DOC, Form::File).unwrap();
    assert_eq!(
        (a.kind, a.watch, a.hint, a.window, a.span_hash),
        (AnchorKind::File, Watch::Header, None, None, None)
    );
    assert_eq!(a.blob, moirai_files::text::oid_of(ObjectFormat::Sha1, DOC));
    assert_eq!(res(&a, b"anything").state, AnchorState::Fresh);
    // A header-watched file anchor follows the file: its content is not read.
    let ri = ResolveInput {
        path: b"a.txt",
        read: params(),
        scanners: ScannerRule::CURRENT,
    };
    let r = Anchors::new().resolve(&mut Broken(9), &a, &ri, &mut Tight(0));
    assert_eq!(r.state, AnchorState::Fresh);
    let mut pin = a.clone();
    pin.watch = Watch::Span;
    assert_eq!(res(&pin, DOC).state, AnchorState::Fresh);
    assert_eq!(res(&pin, b"changed\n").state, AnchorState::Edited);
    // A pair of algorithms is content unknown: unverified (oid algorithm differs).
    let p = ReadParams {
        format: ObjectFormat::Sha256,
        ..params()
    };
    let r = res_with(&pin, DOC, b"a.txt", ScannerRule::CURRENT, p);
    assert_eq!(
        (r.state, r.reason),
        (
            AnchorState::Unverified,
            Some(UnverifiedReason::OidAlgorithm)
        )
    );
    let planned = capture_planned(&FILE, None);
    assert_eq!((planned.kind, planned.blob), (AnchorKind::File, Oid::NONE));
    // A span-watched one reads: an unreadable content leaves it unverified (unreadable).
    let r = Anchors::new().resolve(&mut Broken(9), &pin, &ri, &mut Unlimited);
    assert_eq!(
        r.reason,
        Some(UnverifiedReason::Unavailable(Unavailable::Unreadable))
    );
}

#[test]
fn budgets_are_charged_before_each_read() {
    // A budget below the file's size: refused before a byte is read (the source fails every read).
    let i = input(Form::Lines(span(3, 3)), b"a.txt", ScannerRule::CURRENT);
    let e = Anchors::new().capture(&mut Broken(DOC.len() as u64), &i, &mut Tight(10));
    assert_eq!(
        e.unwrap_err(),
        CaptureError::Unavailable(Unavailable::Budget)
    );
    // Enough for the first read but not for the second: unavailable (budget).
    let e = Anchors::new().capture(
        &mut SliceSource::new(DOC),
        &i,
        &mut Tight(DOC.len() as u64 + 10),
    );
    assert_eq!(
        e.unwrap_err(),
        CaptureError::Unavailable(Unavailable::Budget)
    );
    // Enough for all of a capture's reads.
    let ok = Anchors::new().capture(&mut SliceSource::new(DOC), &i, &mut Tight(1 << 20));
    assert!(ok.is_ok());
    // A file over files.max-read-bytes is unavailable (size) before the budget is charged.
    let mut small = input(Form::Lines(span(3, 3)), b"a.txt", ScannerRule::CURRENT);
    small.read.max_read_bytes = 10;
    let e = Anchors::new().capture(&mut Broken(DOC.len() as u64), &small, &mut Tight(0));
    assert_eq!(e.unwrap_err(), CaptureError::Unavailable(Unavailable::Size));
    // Resolve: the first read is charged before it starts.
    let a = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    let ri = ResolveInput {
        path: b"a.txt",
        read: params(),
        scanners: ScannerRule::CURRENT,
    };
    let r = Anchors::new().resolve(&mut Broken(DOC.len() as u64), &a, &ri, &mut Tight(10));
    assert_eq!(
        r.reason,
        Some(UnverifiedReason::Unavailable(Unavailable::Budget))
    );
}

#[test]
fn short_ranges_with_overlapping_quotes_resolve() {
    // Five short non-trivial lines (54 bytes): a `range` whose start and end quotes are both the whole span text.
    let lines: Vec<String> = [
        "intro",
        "alpha one",
        "beta two",
        "gamma three",
        "delta four",
        "epsilon five",
        "outro",
    ]
    .map(str::to_owned)
    .to_vec();
    let b = join(&lines, false);
    let a = cap(&b, Form::Lines(span(2, 6))).unwrap();
    assert_eq!(a, rf::capture_lines(&b, 2, 6).unwrap());
    assert_eq!(a.kind, AnchorKind::Range);
    let Some(Texts::Held { quote, end, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!(quote.len(), 54);
    assert_eq!(Some(quote), end.as_ref());
    assert_eq!(a.occurrence, None);
    assert_eq!(res(&a, &b).state, AnchorState::Fresh);
    // One line inserted above: moved.
    let mut moved = lines.clone();
    moved.insert(0, "new".to_owned());
    let b2 = join(&moved, false);
    let r = res(&a, &b2);
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(3, 7))));
    assert_eq!(r, rf::resolve(&a, &b2));
    // A byte of the span changed as well: the fuzzy step pairs the overlapping quotes.
    let mut edited = moved.clone();
    edited[3] = "beta 2wo".to_owned();
    let b3 = join(&edited, false);
    let r = res(&a, &b3);
    assert_eq!((r.state, r.span), (AnchorState::Edited, Some(span(3, 7))));
    assert_eq!(r, rf::resolve(&a, &b3));
    // A span of 75 bytes: the quotes of 64 bytes overlap without being equal.
    let lines: Vec<String> = [
        "intro",
        "alpha number one",
        "beta number two",
        "gamma number three",
        "delta four",
        "epsilon five",
        "outro",
    ]
    .map(str::to_owned)
    .to_vec();
    let b = join(&lines, false);
    let a = cap(&b, Form::Lines(span(2, 6))).unwrap();
    let Some(Texts::Held { quote, end, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!((quote.len(), end.as_ref().map(Vec::len)), (64, Some(64)));
    assert_ne!(Some(quote), end.as_ref());
    let mut moved = lines.clone();
    moved.insert(0, "new".to_owned());
    let b2 = join(&moved, false);
    let r = res(&a, &b2);
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(3, 7))));
    assert_eq!(r, rf::resolve(&a, &b2));
    assert_eq!(chunky_resolve(&a, &b2, vec![1, 5, 300]), r);
}

#[test]
fn a_range_whose_end_text_recurs_inside_it() {
    // The end quote (the last 64 bytes of the span) lies inside line 6 and inside its copy on line 3.
    let same = "let checksum_value = combine_parts(first_part, second_part, third_part);";
    let lines: Vec<String> = [
        "intro",
        "fn start_of_the_range() {",
        same,
        "a middle line of the range",
        "another middle line",
        same,
        "outro",
    ]
    .map(str::to_owned)
    .to_vec();
    let b = join(&lines, false);
    let a = cap(&b, Form::Lines(span(2, 6))).unwrap();
    assert_eq!(a.kind, AnchorKind::Range);
    assert_eq!(a, rf::capture_lines(&b, 2, 6).unwrap());
    assert_eq!(res(&a, &b).state, AnchorState::Fresh);
    // Moved by one line and unchanged: moved over the whole range, not edited over its first two lines.
    let mut moved = lines.clone();
    moved.insert(0, "new".to_owned());
    let b2 = join(&moved, false);
    let r = res(&a, &b2);
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(3, 7))));
    assert_eq!(r, rf::resolve(&a, &b2));
    // A line removed inside: the nearest end is still the last one.
    let mut shorter = moved.clone();
    shorter.remove(4);
    let b3 = join(&shorter, false);
    let r = res(&a, &b3);
    assert_eq!((r.state, r.span), (AnchorState::Edited, Some(span(3, 6))));
    assert_eq!(r, rf::resolve(&a, &b3));
}

#[test]
fn imported_texts_longer_than_a_capture_writes_are_unverified() {
    let a = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    let mut b2 = b"new\n".to_vec();
    b2.extend_from_slice(DOC);
    let budget = (
        AnchorState::Unverified,
        Some(UnverifiedReason::Unavailable(Unavailable::Budget)),
    );
    let long = |n: usize| b"LockFileEx ".repeat(n / 11 + 1)[..n].to_vec();
    let cases = [
        (long(129), Vec::new(), Vec::new(), None),
        (long(1 << 20), Vec::new(), Vec::new(), None),
        (b"LockFileEx".to_vec(), long(65), Vec::new(), None),
        (b"LockFileEx".to_vec(), Vec::new(), long(65), None),
        (
            b"LockFileEx".to_vec(),
            Vec::new(),
            Vec::new(),
            Some(long(129)),
        ),
    ];
    for (quote, prefix, suffix, end) in cases {
        let mut x = a.clone();
        if end.is_some() {
            x.kind = AnchorKind::Range;
        }
        x.texts = Some(Texts::Held {
            quote,
            prefix,
            suffix,
            end,
        });
        let r = res(&x, &b2);
        assert_eq!((r.state, r.reason), budget);
        assert_eq!(r, rf::resolve(&x, &b2));
    }
    // At the bounds, the steps run.
    let mut x = a.clone();
    x.texts = Some(Texts::Held {
        quote: b"The writer byte is taken with LockFileEx.".to_vec(),
        prefix: long(64),
        suffix: long(64),
        end: None,
    });
    assert_eq!(res(&x, &b2).state, AnchorState::Moved);
}

/// A resolution under the interim rule with a budget of so many units.
fn res_tight(a: &Anchor, b: &[u8], units: u64) -> Resolution {
    let ri = ResolveInput {
        path: b"a.txt",
        read: params(),
        scanners: ScannerRule::CURRENT,
    };
    Anchors::new().resolve(&mut SliceSource::new(b), a, &ri, &mut Tight(units))
}

/// An imported anchor ([F08 §10.3] bounds neither its hint nor its texts' content): the kind, hint and held texts
/// given, no context, no occurrence and an empty window.
fn imported(kind: AnchorKind, hint: LineSpan, quote: &[u8], end: Option<&[u8]>) -> Anchor {
    let mut a = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    a.kind = kind;
    a.hint = Some(hint);
    a.occurrence = None;
    a.window = Some(Window::EMPTY);
    a.texts = Some(Texts::Held {
        quote: quote.to_vec(),
        prefix: Vec::new(),
        suffix: Vec::new(),
        end: end.map(<[u8]>::to_vec),
    });
    a
}

const BUDGET: (AnchorState, Option<UnverifiedReason>) = (
    AnchorState::Unverified,
    Some(UnverifiedReason::Unavailable(Unavailable::Budget)),
);

#[test]
fn range_hits_that_wait_are_charged_as_they_grow() {
    // A hint [1, u32::MAX] with quote and end "e": every start's target lies past N, so over a long run of "e" lines
    // every start and end hit waits for N's last line. A budget that covers the reads but not those hits:
    // unverified (budget).
    let a = imported(AnchorKind::Range, span(1, u32::MAX), b"e", Some(b"e"));
    let lines = "e\n".repeat(100_000);
    let reads = 2 * lines.len() as u64;
    let r = res_tight(&a, lines.as_bytes(), reads + (64 << 10));
    assert_eq!((r.state, r.reason), BUDGET);
    // Unlimited, the same text resolves: every start pairs with the end on the last line.
    assert_eq!(res(&a, lines.as_bytes()).state, AnchorState::Ambiguous);
    // On one line the target is clamped to N's line count: each start pairs with the end hit at its own offset at
    // once, and the same budget suffices.
    let one = "e".repeat(200_000);
    let r = res_tight(&a, one.as_bytes(), reads + (64 << 10));
    assert_eq!(r.state, AnchorState::Ambiguous);
}

#[test]
fn a_long_range_over_repeated_lines_is_charged_as_it_grows() {
    let m = 20_000u32;
    let b = "x = 1;\n".repeat(m as usize);
    let size = b.len() as u64;
    let a = cap(b.as_bytes(), Form::Lines(span(1, m))).unwrap();
    assert_eq!(a.kind, AnchorKind::Range);
    assert_eq!(a.occurrence, NonZeroU16::new(1));
    // Moved by one line: every start waits for the end on N's last line; unlimited, the occurrence decides.
    let b2 = format!("// top\n{b}");
    let r = res(&a, b2.as_bytes());
    assert_eq!(
        (r.state, r.span),
        (AnchorState::Moved, Some(span(2, m + 1)))
    );
    // A budget that covers the reads but not the waiting hits: unverified (budget), at capture and at resolve.
    let i = input(Form::Lines(span(1, m)), b"a.txt", ScannerRule::CURRENT);
    let e = Anchors::new().capture(
        &mut SliceSource::new(b.as_bytes()),
        &i,
        &mut Tight(3 * size + (64 << 10)),
    );
    assert_eq!(
        e.unwrap_err(),
        CaptureError::Unavailable(Unavailable::Budget)
    );
    let r = res_tight(&a, b2.as_bytes(), 2 * (size + 7) + (64 << 10));
    assert_eq!((r.state, r.reason), BUDGET);
}

#[test]
fn fuzzy_end_searches_are_charged_as_they_search() {
    // A short imported quote with a hint far longer than the text: no exact hit, a fuzzy start candidate on every line,
    // and an end that occurs nowhere, so each candidate searches its end to the end of N.
    let a = imported(AnchorKind::Range, span(1, u32::MAX), b"abcd", Some(b"wxyz"));
    let small = "abce\n".repeat(2_000);
    assert_eq!(res(&a, small.as_bytes()).state, AnchorState::Orphaned);
    let large = "abce\n".repeat(20_000);
    let r = res_tight(&a, large.as_bytes(), 3 * large.len() as u64 + (1 << 20));
    assert_eq!((r.state, r.reason), BUDGET);
}

#[test]
fn a_header_window_left_open_is_charged_as_it_grows() {
    let a = lifted_cap(RUST, b"src/lock.rs", Form::Symbol("LockFile/acquire")).unwrap();
    // The hint's header line now opens a string literal that never closes: the header runs to the end of N.
    let mut b = String::from_utf8(RUST.to_vec())
        .unwrap()
        .replace("fn acquire(&self) -> u32 {", "fn acquire(&self, s = \"");
    b.push_str(&"text { ; ( ] text\n".repeat(20_000));
    let ri = ResolveInput {
        path: b"src/lock.rs",
        read: params(),
        scanners: ScannerRule::Lifted,
    };
    // A budget that covers the reads but not the window the first read keeps: unverified (budget).
    let size = b.len() as u64;
    let mut tight = Tight(3 * size + (64 << 10));
    let r = Anchors::new().resolve(&mut SliceSource::new(b.as_bytes()), &a, &ri, &mut tight);
    assert_eq!((r.state, r.reason), BUDGET);
    // Unlimited, the cascade runs past the hint.
    let r = Anchors::new().resolve(&mut SliceSource::new(b.as_bytes()), &a, &ri, &mut Unlimited);
    assert_ne!(r.state, AnchorState::Unverified);
}

#[test]
fn an_occurrence_above_u16_is_refused() {
    let b = "x\n".repeat(70_000);
    let e = cap(b.as_bytes(), Form::Lines(span(66_000, 66_000)));
    assert_eq!(e.unwrap_err(), CaptureError::Refused(Refusal::Occurrence));
    let a = cap(b.as_bytes(), Form::Lines(span(65_535, 65_535))).unwrap();
    assert_eq!(a.occurrence, NonZeroU16::new(65_535));
}

#[test]
fn specs_parse_into_forms() {
    let s = parse_spec("docs/a.md:3-5");
    assert_eq!(
        (s.path, s.form, s.commit),
        ("docs/a.md", Form::Lines(span(3, 5)), None)
    );
    let s = parse_spec("docs/a.md:7");
    assert_eq!(s.form, Form::Lines(span(7, 7)));
    let s = parse_spec("docs/plan/storage.md@c4410:12-18");
    assert_eq!((s.path, s.commit), ("docs/plan/storage.md", Some("c4410")));
    let s = parse_spec("crates/engine/src/lock.rs::LockFile/acquire");
    assert_eq!(
        (s.path, s.form),
        (
            "crates/engine/src/lock.rs",
            Form::Symbol("LockFile/acquire")
        )
    );
    let s = parse_spec("docs/C#/intro.md#Setup");
    assert_eq!(
        (s.path, s.form),
        ("docs/C#/intro.md", Form::Heading("Setup"))
    );
    assert_eq!(parse_spec("C:/work/x.txt").form, Form::File);
    assert_eq!(parse_spec("C:/work/x.txt:9").path, "C:/work/x.txt");
    assert_eq!(parse_spec("a.txt:x").form, Form::File);
    // `@` before a line form names a commit only when 4 to 64 hexadecimal digits follow it.
    let s = parse_spec("docs/a@b.md:3");
    assert_eq!(
        (s.path, s.form, s.commit),
        ("docs/a@b.md", Form::Lines(span(3, 3)), None)
    );
    assert_eq!(parse_spec("x.md@HEAD:3").path, "x.md@HEAD");
    assert_eq!(parse_spec("x.md@abc:3").commit, None);
    let full = format!("x.md@{}:3", "0123456789abcdef".repeat(4));
    assert_eq!(parse_spec(&full).commit.map(str::len), Some(64));
    let long = format!("x.md@{}:3", "a".repeat(65));
    assert_eq!(parse_spec(&long).commit, None);
    assert_eq!(parse_spec("x.md@C4410FF:3").commit, Some("C4410FF"));
}

// --- scenarios: resolve --------------------------------------------------------------------------------------------

#[test]
fn moved_edited_ambiguous_orphaned() {
    let a = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    // Lines inserted above: moved.
    let mut b2 = b"new\nlines\n".to_vec();
    b2.extend_from_slice(DOC);
    let r = res(&a, &b2);
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(5, 5))));
    // The quote edited: the fuzzy step, with its score.
    let b3 = String::from_utf8(DOC.to_vec())
        .unwrap()
        .replace("LockFileEx", "LockFileEx2")
        .into_bytes();
    let r = res(&a, &b3);
    assert_eq!(r.state, AnchorState::Edited);
    assert_eq!(r.span, Some(span(3, 3)));
    assert!(r.score.is_some());
    // The quote gone: orphaned.
    let r = res(&a, b"# Lock\n\nnothing like it\n");
    assert_eq!(r.state, AnchorState::Orphaned);
    // Duplicates with the same context and window: ambiguous.
    let q = cap(b"k\nv\nk\nv\n", Form::Lines(span(2, 2))).unwrap();
    let mut q2 = q.clone();
    q2.occurrence = None;
    q2.texts = Some(Texts::Held {
        quote: b"v".to_vec(),
        prefix: Vec::new(),
        suffix: Vec::new(),
        end: None,
    });
    q2.window = Some(Window::EMPTY);
    assert_eq!(res(&q2, b"v\nw\nv\n").state, AnchorState::Ambiguous);
    // The content binary: orphaned.
    assert_eq!(res(&a, b"x\0y").state, AnchorState::Orphaned);
    // Pinned anchors are not resolved.
    let mut p = a.clone();
    p.mode = Mode::Pinned;
    assert_eq!(res(&p, &b2).state, AnchorState::Unresolved);
}

#[test]
fn the_watch_rule_on_a_moved_range() {
    let a = cap(DOC, Form::Lines(span(3, 8))).unwrap();
    assert_eq!(a.kind, AnchorKind::Range);
    // Moved with the middle unchanged: moved.
    let mut b2 = b"intro\n".to_vec();
    b2.extend_from_slice(DOC);
    assert_eq!(res(&a, &b2).state, AnchorState::Moved);
    // Moved with a byte between the two quotes changed: the span changed under span watch, so edited.
    let b3 = String::from_utf8(b2)
        .unwrap()
        .replace("the byte to", "the lock to")
        .into_bytes();
    let r = res(&a, &b3);
    assert_eq!(
        (r.state, r.span, r.score),
        (AnchorState::Edited, Some(span(4, 9)), None)
    );
}

#[test]
fn lines_anchors_align_their_window() {
    let b = b"one\ntwo\nthree\n}\n\nfour\nfive\nsix\n";
    let a = cap(b, Form::Lines(span(4, 5))).unwrap();
    assert_eq!(a.kind, AnchorKind::Lines);
    let mut b2 = b"zero\n".to_vec();
    b2.extend_from_slice(b);
    let r = res(&a, &b2);
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(5, 6))));
    // The trivial lines changed: aligned but the span hash differs.
    let b3 = b"zero\none\ntwo\nthree\n};\n\nfour\nfive\nsix\n";
    assert_eq!(res(&a, b3).state, AnchorState::Orphaned);
    // A capped line-hash array leaves a window-only anchor unverified (size).
    let p = ReadParams {
        max_line_hashes: 2,
        ..params()
    };
    let r = res_with(&a, &b2, b"a.txt", ScannerRule::CURRENT, p);
    assert_eq!(
        r.reason,
        Some(UnverifiedReason::Unavailable(Unavailable::Size))
    );
}

#[test]
fn text_unavailable_anchors_skip_the_quote_steps() {
    let b = b"a1\na2\nTHE LINE\nb1\nb2\n";
    let a = cap(b, Form::Lines(span(3, 3))).unwrap();
    let mut tu = a.clone();
    tu.texts = Some(Texts::Digests {
        quote: [1; 16],
        prefix: [2; 16],
        suffix: [3; 16],
        end: None,
    });
    assert_eq!(res(&tu, b).state, AnchorState::Fresh);
    let mut b2 = b"new\n".to_vec();
    b2.extend_from_slice(b);
    let r = res(&tu, &b2);
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(4, 4))));
    // The line edited: the window aligns, the span hash does not match, and no quote step runs.
    let b3 = b"new\na1\na2\nTHE LINE!\nb1\nb2\n";
    assert_eq!(res(&tu, b3).state, AnchorState::Orphaned);
    assert_eq!(res(&a, b3).state, AnchorState::Edited);
    // Next to a trivial line the alignment ties and decides nothing (spec finding of WP-64).
    let c = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    let mut tu = c.clone();
    tu.texts = Some(Texts::Digests {
        quote: [1; 16],
        prefix: [2; 16],
        suffix: [3; 16],
        end: None,
    });
    let mut d2 = b"new\n".to_vec();
    d2.extend_from_slice(DOC);
    assert_eq!(res(&tu, &d2).state, AnchorState::Orphaned);
}

#[test]
fn window_tie_break_and_unavailable_window() {
    // Two equal quotes whose 32-byte contexts are equal, told apart by their windows.
    let mk = |tag: &str| {
        format!(
            "{tag} header\ncommon line number one is long\nfoo();\ncommon trailing line is long xx\n{tag} footer\n"
        )
    };
    let b = format!("{}{}", mk("left"), mk("right"));
    let a = cap(b.as_bytes(), Form::Lines(span(8, 8))).unwrap();
    let Some(Texts::Held { prefix, suffix, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!(
        (prefix.as_slice(), suffix.as_slice()),
        (
            &b"\ncommon line number one is long\n"[..],
            &b"\ncommon trailing line is long xx"[..]
        )
    );
    assert_eq!(a.occurrence, None);
    let b2 = format!("x\ny\n{b}");
    let r = res(&a, b2.as_bytes());
    assert_eq!((r.state, r.span), (AnchorState::Moved, Some(span(10, 10))));
    assert_eq!(r, rf::resolve(&a, b2.as_bytes()));
    // The same with a capped array: the window step is needed and unavailable.
    let p = ReadParams {
        max_line_hashes: 3,
        ..params()
    };
    let r = res_with(&a, b2.as_bytes(), b"a.txt", ScannerRule::CURRENT, p);
    assert_eq!(
        r.reason,
        Some(UnverifiedReason::Unavailable(Unavailable::Size))
    );
}

#[test]
fn reads_that_disagree_and_budgets() {
    // A source whose bytes change after the first read: unverified (unstable).
    struct Changing {
        reads: usize,
        pos: usize,
        a: Vec<u8>,
        b: Vec<u8>,
    }
    impl ByteSource for Changing {
        type Error = std::convert::Infallible;
        type Stamp = usize;
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let v = if self.reads < 4 { &self.a } else { &self.b };
            let n = buf.len().min(v.len() - self.pos.min(v.len()));
            buf[..n].copy_from_slice(&v[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
        fn rewind(&mut self) -> Result<(), Self::Error> {
            self.reads += 1;
            self.pos = 0;
            Ok(())
        }
        fn snapshot(&self) -> Result<Snapshot<usize>, Self::Error> {
            let v = if self.reads < 4 { &self.a } else { &self.b };
            Ok(Snapshot {
                size: v.len() as u64,
                mtime: usize::from(self.reads >= 4),
            })
        }
    }
    let a = cap(DOC, Form::Lines(span(3, 3))).unwrap();
    let mut moved = b"x\n".to_vec();
    moved.extend_from_slice(DOC);
    let mut other = b"y\n".to_vec();
    other.extend_from_slice(DOC);
    let mut src = Changing {
        reads: 0,
        pos: 0,
        a: moved,
        b: other,
    };
    let ri = ResolveInput {
        path: b"a.txt",
        read: params(),
        scanners: ScannerRule::CURRENT,
    };
    let r = Anchors::new().resolve(&mut src, &a, &ri, &mut Unlimited);
    assert_eq!(
        r.reason,
        Some(UnverifiedReason::Unavailable(Unavailable::Unstable))
    );
    // A budget that runs out: unverified (budget).
    let mut b2 = b"new\n".to_vec();
    b2.extend_from_slice(DOC);
    let r = Anchors::new().resolve(&mut SliceSource::new(&b2), &a, &ri, &mut Tight(200));
    assert_eq!(
        r.reason,
        Some(UnverifiedReason::Unavailable(Unavailable::Budget))
    );
    // Too large for files.max-read-bytes: unverified (size).
    let p = ReadParams {
        max_read_bytes: 10,
        ..params()
    };
    let r = res_with(&a, DOC, b"a.txt", ScannerRule::CURRENT, p);
    assert_eq!(
        r.reason,
        Some(UnverifiedReason::Unavailable(Unavailable::Size))
    );
}

#[test]
fn long_whitespace_runs_stream_exactly() {
    // Interior and trailing runs longer than the stream's hold.
    let mut b = b"start\n".to_vec();
    b.extend_from_slice(b"key");
    b.extend(std::iter::repeat_n(b' ', 1000));
    b.extend_from_slice(b"= value");
    b.extend(std::iter::repeat_n(b'\t', 900));
    b.extend_from_slice(b"\nend\n");
    let a = cap(&b, Form::Lines(span(2, 2))).unwrap();
    let Some(Texts::Held { quote, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!(a.kind, AnchorKind::Range);
    assert_eq!(quote.len(), 64);
    assert_eq!(rf::capture_lines(&b, 2, 2).unwrap(), a);
    let mut b2 = b"moved\n".to_vec();
    b2.extend_from_slice(&b);
    assert_eq!(res(&a, &b2), rf::resolve(&a, &b2));
    assert_eq!(
        chunky_resolve(&a, &b2, vec![1, 7, 300]),
        rf::resolve(&a, &b2)
    );
}

// --- the lifted scanner rule ([F21 §2.4]–§2.6, §6) -----------------------------------------------------------------

const RUST: &[u8] = b"mod lock {\n    pub struct LockFile;\n    impl LockFile {\n        pub fn acquire(&self) -> u32 {\n            let x = 1;\n            x\n        }\n        pub fn acquire_shared(&self) -> u32 {\n            2\n        }\n    }\n}\n";

fn lifted_cap(b: &[u8], path: &[u8], form: Form<'_>) -> Result<Anchor, CaptureError> {
    Anchors::new()
        .capture(
            &mut SliceSource::new(b),
            &input(form, path, ScannerRule::Lifted),
            &mut Unlimited,
        )
        .map(|c| c.anchor)
}

#[test]
fn lifted_symbol_form() {
    let a = lifted_cap(RUST, b"src/lock.rs", Form::Symbol("LockFile/acquire")).unwrap();
    assert_eq!(a.kind, AnchorKind::Symbol);
    assert_eq!(a.watch, Watch::Header);
    assert_eq!(a.hint, Some(span(4, 7)));
    let Some(Texts::Held { quote, .. }) = &a.texts else {
        panic!()
    };
    assert_eq!(quote, b"pub fn acquire(&self) -> u32");
    assert_eq!(a.span_hash, Some(xxh3_64(b"pub fn acquire(&self) -> u32")));
    assert!(a.scope.is_some());
    let r = |b: &[u8]| res_with(&a, b, b"src/lock.rs", ScannerRule::Lifted, params());
    assert_eq!(r(RUST).state, AnchorState::Fresh);
    // A changed body keeps a header-watched anchor fresh.
    let body = String::from_utf8(RUST.to_vec())
        .unwrap()
        .replace("let x = 1;", "let x = 7;");
    assert_eq!(r(body.as_bytes()).state, AnchorState::Fresh);
    // Moved inside its scope: moved, with the hint's length.
    let moved = String::from_utf8(RUST.to_vec())
        .unwrap()
        .replace("impl LockFile {\n", "impl LockFile {\n        // c\n");
    let m = r(moved.as_bytes());
    assert_eq!((m.state, m.span), (AnchorState::Moved, Some(span(5, 8))));
    // Renamed: the scope no longer resolves; the fuzzy step, restricted to `fn` headers, finds the renamed item.
    let renamed = String::from_utf8(RUST.to_vec())
        .unwrap()
        .replace("fn acquire(", "fn acquire2(");
    let e = r(renamed.as_bytes());
    assert_eq!((e.state, e.span), (AnchorState::Edited, Some(span(4, 7))));
    assert!(e.score.is_some());
    // Under the interim rule the same anchor resolves without scanner steps.
    let i = res_with(
        &a,
        moved.as_bytes(),
        b"src/lock.rs",
        ScannerRule::Interim,
        params(),
    );
    assert_eq!(i.state, AnchorState::Moved);
}

#[test]
fn lifted_form_refusals_and_scopes() {
    let e = |b: &[u8], path: &[u8], f| match lifted_cap(b, path, f) {
        Err(CaptureError::Refused(r)) => r.case(),
        other => panic!("{other:?}"),
    };
    assert_eq!(e(RUST, b"src/lock.rs", Form::Symbol("Missing")), "no-scope");
    assert_eq!(e(RUST, b"src/lock.rs", Form::Symbol("LockFile/")), "syntax");
    assert_eq!(
        e(b"fn a() {}\nfn a() {}\n", b"x.rs", Form::Symbol("a")),
        "several"
    );
    // A `path:L-M` capture records the enclosing scope.
    let a = lifted_cap(RUST, b"src/lock.rs", Form::Lines(span(5, 5))).unwrap();
    assert!(a.scope.is_some());
    // The heading form on Markdown.
    let md = b"# Design\n\n## 3.2 Recovery\n\nText.\n";
    let h = lifted_cap(md, b"docs/a.md", Form::Heading("Recovery")).unwrap();
    assert_eq!(h.kind, AnchorKind::Heading);
    let Some(Texts::Held { quote, .. }) = &h.texts else {
        panic!()
    };
    assert_eq!(quote, b"## 3.2 Recovery");
    assert_eq!(h.hint, Some(span(3, 5)));
}

#[test]
fn text_unavailable_symbol_anchors_watch_their_header() {
    // A one-line item, so that its hint and its quote span are the same lines ([F20 §6.5] aligns the window over the
    // hint's length; for a longer item the two differ, a spec finding of WP-64).
    let src = "mod m {\n    fn one() { 1 }\n    fn two() { 2 }\n    fn three() { 3 }\n    fn four() { 4 }\n}\n";
    let a = lifted_cap(src.as_bytes(), b"src/m.rs", Form::Symbol("m/two")).unwrap();
    assert_eq!(
        (a.kind, a.watch, a.hint),
        (AnchorKind::Symbol, Watch::Header, Some(span(3, 3)))
    );
    let mut tu = a.clone();
    tu.texts = Some(Texts::Digests {
        quote: [1; 16],
        prefix: [2; 16],
        suffix: [3; 16],
        end: None,
    });
    let r = |b: &str| {
        res_with(
            &tu,
            b.as_bytes(),
            b"src/m.rs",
            ScannerRule::Interim,
            params(),
        )
    };
    assert_eq!(r(src).state, AnchorState::Fresh);
    // A changed body keeps a header-watched anchor fresh at its hint.
    let body = src.replace("{ 2 }", "{ 22 }");
    assert_eq!(r(&body).state, AnchorState::Fresh);
    // Moved with its body changed: the window aligns and the header at the new line hashes to `span_hash`.
    let moved = format!("// c\n{body}");
    let m = r(&moved);
    assert_eq!((m.state, m.span), (AnchorState::Moved, Some(span(4, 4))));
    // Moved and renamed: no quote step runs for a text-unavailable anchor, so it is orphaned.
    let renamed = moved.replace("fn two(", "fn two2(");
    assert_eq!(r(&renamed).state, AnchorState::Orphaned);
}

#[test]
fn lifted_scope_only() {
    let a = lifted_cap(RUST, b"src/lock.rs", Form::Symbol("LockFile/acquire")).unwrap();
    // Header rewritten beyond recognition while the item keeps its name path: edited (scope only).
    let gone = String::from_utf8(RUST.to_vec())
        .unwrap()
        .replace("pub fn acquire(&self) -> u32 {", "fn acquire() {");
    let r = res_with(
        &a,
        gone.as_bytes(),
        b"src/lock.rs",
        ScannerRule::Lifted,
        params(),
    );
    assert!(r.state == AnchorState::Edited, "{r:?}");
}

#[test]
fn range_fuzzy_with_a_distant_end() {
    // A range over 50 long lines: its end quote lies far beyond the fuzzy step's tail when the start candidate is
    // decided, so the end search and the end candidate's suffix run over many slices.
    let mut seed = 12345u32;
    let lines: Vec<String> = (0..60)
        .map(|i| {
            let words: String = (0..100)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    char::from(b'a' + ((seed >> 16) % 26) as u8)
                })
                .collect();
            format!("line {i:02} {words}")
        })
        .collect();
    let b = join(&lines, false);
    let a = cap(&b, Form::Lines(span(5, 55))).unwrap();
    assert_eq!(a.kind, AnchorKind::Range);
    let mut edited = lines.clone();
    edited[4] = edited[4].replacen("line 04", "line 4X", 1);
    edited.insert(0, "inserted".to_owned());
    let b2 = join(&edited, false);
    let r = res(&a, &b2);
    assert_eq!(r, rf::resolve(&a, &b2));
    assert_eq!((r.state, r.span), (AnchorState::Edited, Some(span(6, 56))));
    assert_eq!(chunky_resolve(&a, &b2, vec![3, 1000, 17]), r);
}

/// `n` distinct filler lines of 46 bytes from a seed.
fn filler(n: usize, seed: u64) -> Vec<String> {
    let mut x = seed | 1;
    (0..n)
        .map(|i| {
            let w: String = (0..40)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    char::from(b'a' + (x % 26) as u8)
                })
                .collect();
            format!("f{i:04} {w}")
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 10, ..ProptestConfig::default() })]

    /// Texts longer than 2 × SPAN with the captured lines moved far from the hint: R1 holds no candidate and R3
    /// decides; with a more damaged copy left near the hint, R1 decides first. The result equals the reference's.
    #[test]
    fn far_moves_follow_the_region_order(seed in any::<u64>(), l in 2usize..30, len in 0usize..6,
                                         dest in 500usize..760, damage in proptest::option::of(0usize..46),
                                         near in any::<bool>()) {
        let lines = filler(800, seed);
        let b = join(&lines, false);
        prop_assert!(b.len() > 2 * 16_384);
        let m = l + len;
        let a = cap(&b, Form::Lines(span(l as u32, m as u32))).unwrap();
        let mut moved = lines.clone();
        let mut block: Vec<String> = moved.drain(l - 1..m).collect();
        if near {
            let mut copy = block.clone();
            copy[0].replace_range(7..9, "##");
            moved.splice(l - 1..l - 1, copy);
        }
        if let Some(c) = damage {
            block[0].replace_range(c..=c, "#");
        }
        moved.splice(dest..dest, block);
        let b2 = join(&moved, false);
        let r = res(&a, &b2);
        prop_assert_eq!(r, rf::resolve(&a, &b2));
        prop_assert_eq!(chunky_resolve(&a, &b2, vec![700, 13]), r);
    }
}

// --- fixtures ------------------------------------------------------------------------------------------------------

const CANONICAL: [(&str, &str); 3] = [
    (
        "anchors.cases",
        include_str!("../../../fixtures/canonical/cases/anchors.cases"),
    ),
    (
        "checkpoint.cases",
        include_str!("../../../fixtures/canonical/cases/checkpoint.cases"),
    ),
    (
        "values.cases",
        include_str!("../../../fixtures/canonical/cases/values.cases"),
    ),
];

/// The `key=value` attributes of an anchor line (`fixtures/canonical/INDEX.md` §3.4): a value is a bare token or a
/// JSON string, returned decoded.
fn attrs(line: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        let k0 = i;
        while i < b.len() && b[i] != b'=' && b[i] != b' ' {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            continue;
        }
        let key = line[k0..i].to_owned();
        i += 1;
        let mut v = Vec::new();
        if b.get(i) == Some(&b'"') {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                    match b[i] {
                        b'n' => v.push(b'\n'),
                        b't' => v.push(b'\t'),
                        b'r' => v.push(b'\r'),
                        b'u' => {
                            let cp = u32::from_str_radix(&line[i + 1..i + 5], 16).unwrap();
                            let mut buf = [0u8; 4];
                            v.extend_from_slice(
                                char::from_u32(cp).unwrap().encode_utf8(&mut buf).as_bytes(),
                            );
                            i += 4;
                        }
                        c => v.push(c),
                    }
                } else {
                    v.push(b[i]);
                }
                i += 1;
            }
            i += 1;
        } else {
            let v0 = i;
            while i < b.len() && b[i] != b' ' {
                i += 1;
            }
            v.extend_from_slice(&b[v0..i]);
        }
        out.push((key, v));
    }
    out
}

fn unhex(s: &[u8]) -> Vec<u8> {
    s.chunks(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}

/// Every anchor window of `fixtures/canonical/` is a valid window value ([F20 §2.7.3]) that re-encodes to its bytes,
/// and every hash-only anchor's digests are those of its full twin's texts ([F08 §10.3]): capture de-duplication
/// compares them so ([`Anchor::same_selectors`]). These are the complete checks: the content-derived selectors of
/// `anchors.cases` and `checkpoint.cases` are stated values (`fixtures/canonical/INDEX.md` §3.4), and the
/// `values.cases` `selector-*` cases encode a selector block ([F07 §8.2]), so no capture or resolve check applies to
/// any of them.
#[test]
fn canonical_anchor_records() {
    let mut windows = 0;
    let mut full: Vec<(String, Texts)> = Vec::new();
    let mut hashed: Vec<(String, Texts)> = Vec::new();
    for (file, text) in CANONICAL {
        for line in text.lines() {
            let Some(at) = line.find("kind=") else {
                continue;
            };
            let a = attrs(&line[at..]);
            let get = |k: &str| a.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
            if let Some(w) = get("window") {
                let bytes = unhex(&w);
                let win =
                    Window::from_bytes(&bytes).unwrap_or_else(|e| panic!("{file}: {line}: {e}"));
                assert_eq!(win.to_bytes(), bytes, "{file}: {line}");
                windows += 1;
            }
            let captured = String::from_utf8(get("captured").unwrap_or_default()).unwrap();
            if let Some(q) = get("quote") {
                full.push((
                    captured,
                    Texts::Held {
                        quote: q,
                        prefix: get("prefix").unwrap_or_default(),
                        suffix: get("suffix").unwrap_or_default(),
                        end: get("end"),
                    },
                ));
            } else if let Some(q) = get("quote_h") {
                let h = |v: Vec<u8>| -> [u8; 16] { unhex(&v).try_into().unwrap() };
                hashed.push((
                    captured,
                    Texts::Digests {
                        quote: h(q),
                        prefix: h(get("prefix_h").unwrap()),
                        suffix: h(get("suffix_h").unwrap()),
                        end: get("end_h").map(h),
                    },
                ));
            }
        }
    }
    assert!(windows >= 10, "the canonical cases hold anchor windows");
    let mut twins = 0;
    for (c, t) in &hashed {
        if let Some((_, f)) = full.iter().find(|(fc, _)| fc == c) {
            assert_eq!(f.digests(), t.digests(), "captured {c}");
            twins += 1;
        }
    }
    assert!(
        twins >= 2,
        "anchor-full and anchor-hash-only share their anchors"
    );
}

const RUST_VOCAB: [&str; 12] = [
    "mod m {",
    "fn f() {",
    "fn g(a: u8) -> u8 {",
    "}",
    "    let x = 1;",
    "struct S;",
    "impl S {",
    "pub fn h(&self) {",
    "// fn f() {",
    "    x",
    "",
    "const K: u8 = 1;",
];

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Under the lifted rule, a `symbol` capture resolves `fresh` on its own content, and every resolution after an
    /// edit is the same whatever the chunking of the reads.
    #[test]
    fn lifted_symbol_anchors_stream_exactly(
        lines in proptest::collection::vec(proptest::sample::select(RUST_VOCAB.to_vec()).prop_map(str::to_owned), 2..30),
        name in proptest::sample::select(vec!["f", "g", "h", "S", "K", "m"]),
        es in edits(),
        sizes in proptest::collection::vec(1usize..200, 1..5),
    ) {
        let b = join(&lines, false);
        let Ok(a) = lifted_cap(&b, b"src/x.rs", Form::Symbol(name)) else { return Ok(()) };
        let ri = ResolveInput { path: b"src/x.rs", read: params(), scanners: ScannerRule::Lifted };
        let fresh = Anchors::new().resolve(&mut SliceSource::new(&b), &a, &ri, &mut Unlimited);
        prop_assert_eq!(fresh.state, AnchorState::Fresh);
        let b2 = join(&apply(&lines, &es), false);
        let whole = Anchors::new().resolve(&mut SliceSource::new(&b2), &a, &ri, &mut Unlimited);
        let mut src = Chunky { data: &b2, pos: 0, sizes, k: 0 };
        let chunked = Anchors::new().resolve(&mut src, &a, &ri, &mut Unlimited);
        prop_assert_eq!(whole, chunked);
    }
}

/// The header texts of [F21 §8] (Rust construct 11, Markdown M1, TOML T1 and T4) through [`header`].
#[test]
fn f21_golden_header_texts() {
    use moirai_files::anchor::{HeaderKind, header};
    use moirai_files::text::NormalisedText;
    let h = |src: &str, l: usize, k: HeaderKind| {
        let n = NormalisedText::new(src.as_bytes()).unwrap();
        String::from_utf8(header(&n, l, k).unwrap().to_vec()).unwrap()
    };
    let rust = "/// Doc comment.
#[derive(Debug)]
#[cfg(test)]
pub struct S {
    x: u8,
}

// A plain comment.
#[inline]
pub(crate)
unsafe fn f(
    a: u8,
) -> u8 {
    a
}

/** Block doc. */
mod m;

#[macro_export]
macro_rules! mac (
    () => {}
);
";
    assert_eq!(h(rust, 4, HeaderKind::Symbol), "pub struct S");
    assert_eq!(
        h(rust, 10, HeaderKind::Symbol),
        "pub(crate)
unsafe fn f(
a: u8,
) -> u8"
    );
    assert_eq!(h(rust, 18, HeaderKind::Symbol), "mod m");
    assert_eq!(
        h(rust, 21, HeaderKind::Symbol),
        "macro_rules! mac (
() => {}
)"
    );
    let md = "# Title

## 1. Scope

text

### 1.1 What   it  fixes ###

## 2 Layout
";
    assert_eq!(h(md, 3, HeaderKind::Heading), "## 1. Scope");
    assert_eq!(
        h(md, 7, HeaderKind::Heading),
        "### 1.1 What   it  fixes ###"
    );
    let toml = "name = \"top\"
[package]
name = \"x\"
version = \"0.1.0\"

# comment
[dependencies]
serde = { version = \"1\", features = [\"derive\"] }
";
    assert_eq!(h(toml, 2, HeaderKind::Symbol), "[package]");
    assert_eq!(h(toml, 3, HeaderKind::Symbol), "name = \"x\"");
    assert_eq!(h(toml, 8, HeaderKind::Symbol), "serde =");
    let t4 = "features = [
  \"a\",
  [1],
  \"b\", # comment ]
]
next = 1
";
    assert_eq!(
        h(t4, 1, HeaderKind::Symbol),
        "features = [
\"a\",
[1],
\"b\", # comment ]"
    );
}

fn any_kind() -> impl Strategy<Value = AnchorKind> {
    proptest::sample::select(vec![
        AnchorKind::File,
        AnchorKind::Heading,
        AnchorKind::Symbol,
        AnchorKind::Quote,
        AnchorKind::Range,
        AnchorKind::Lines,
    ])
}

fn bytes_of(alphabet: &'static [u8], max: usize) -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(proptest::sample::select(alphabet.to_vec()), 0..max)
}

const ALPHABET: &[u8] = b"ab {}()\n\r\t;\0\xC3\xA9\xEF\xBB\xBF\xFF#'\"/*!";

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// The cascade is total: any record a decoder could hand it (invalid hints included) and any content, binary,
    /// lone CRs and invalid UTF-8 included, resolve without a panic and identically in any chunking, under both
    /// scanner rules.
    #[test]
    fn resolution_is_total(
        content in bytes_of(ALPHABET, 300),
        kind in any_kind(),
        watch in proptest::sample::select(vec![Watch::Header, Watch::Span]),
        hint in (0u32..12, 0u32..12),
        quote in prop_oneof![4 => bytes_of(ALPHABET, 12), 1 => bytes_of(ALPHABET, 300)],
        prefix in prop_oneof![4 => bytes_of(ALPHABET, 8), 1 => bytes_of(ALPHABET, 100)],
        suffix in prop_oneof![4 => bytes_of(ALPHABET, 8), 1 => bytes_of(ALPHABET, 100)],
        end in proptest::option::of(prop_oneof![4 => bytes_of(ALPHABET, 12), 1 => bytes_of(ALPHABET, 300)]),
        digests in any::<bool>(),
        occurrence in proptest::option::of(1u16..5),
        before in proptest::collection::vec(any::<u16>(), 0..4),
        after in proptest::collection::vec(any::<u16>(), 0..4),
        span_hash in any::<u64>(),
        scope in proptest::option::of(bytes_of(b"\x01\x02\x03ab", 10)),
        lifted in any::<bool>(),
        sizes in proptest::collection::vec(1usize..64, 1..4),
    ) {
        let texts = if digests {
            Texts::Digests { quote: [1; 16], prefix: [2; 16], suffix: [3; 16], end: end.as_ref().map(|_| [4; 16]) }
        } else {
            Texts::Held { quote, prefix, suffix, end }
        };
        let a = Anchor {
            kind,
            mode: Mode::Live,
            watch,
            resolver: 1,
            captured: [0; 16],
            pred: None,
            hint: Some(span(hint.0, hint.1)),
            scope,
            texts: Some(texts),
            occurrence: occurrence.and_then(NonZeroU16::new),
            window: Window::new(&before, &after),
            span_hash: Some(span_hash),
            blob: Oid::NONE,
            git: None,
            marker: None,
        };
        let rule = if lifted { ScannerRule::Lifted } else { ScannerRule::Interim };
        let ri = ResolveInput { path: b"x.rs", read: params(), scanners: rule };
        let whole = Anchors::new().resolve(&mut SliceSource::new(&content), &a, &ri, &mut Unlimited);
        let mut src = Chunky { data: &content, pos: 0, sizes, k: 0 };
        let chunked = Anchors::new().resolve(&mut src, &a, &ri, &mut Unlimited);
        prop_assert_eq!(whole, chunked);
    }
}
