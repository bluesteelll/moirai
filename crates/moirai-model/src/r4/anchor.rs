//! Anchors by definition ([40 §2.7], §4.5; [F08 §10.3], §11.4; [F20 §6]): the anchor record with its texts, capture
//! over a stated content ([F20 §6.1]), and the **brute-force anchor resolver** of [40 §8.3.2] P11: it enumerates every
//! exact occurrence of a quote, computes every fuzzy candidate's edit distance by the plain dynamic programme (FL-1
//! uses Myers' bit-parallel matcher), scores every candidate exactly, and applies the cascade's rules to the full
//! lists. The P11 differential (WP-77) compares FL-1's resolver with [`resolve`] by the order of [`at_least_as_conservative`].
//!
//! The anchor constants that are named holes of [F20 §7] are a [`Consts`] value whose [`Consts::DRAFT`] holds the draft
//! values, so WP-76 and WP-81a can evaluate the model at candidate values. No scope is recorded and no scanner step
//! runs while [F20 §6.1]'s interim scanner rule holds: the `symbol` and `heading` forms are refused, and an imported
//! anchor of those kinds is matched by its header quote as a quote.

use crate::err::Refusal;
use crate::r4::text::{
    NText, Ratio, atext, cutp, cuts, header, is_text, oid, oid_in, parse_window, window_value, xxh3,
};
use crate::r4::uid::{self, CaptureId, EdgeAnchor, Selectors};
use crate::value::{Algo, Oid, Uid, blake3_128};

/// The anchor kinds ([F08 §10.3] `kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Kind {
    /// 1 `file`.
    #[default]
    File,
    /// 2 `heading`.
    Heading,
    /// 3 `symbol`.
    Symbol,
    /// 4 `quote`.
    Quote,
    /// 5 `range`.
    Range,
    /// 6 `lines`.
    Lines,
}

impl Kind {
    /// The kind's name ([F08 §11.1]: it enters `captured` as its ASCII name).
    pub fn name(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Heading => "heading",
            Kind::Symbol => "symbol",
            Kind::Quote => "quote",
            Kind::Range => "range",
            Kind::Lines => "lines",
        }
    }

    /// The kind of a name.
    pub fn from_name(s: &str) -> Option<Kind> {
        match s {
            "file" => Some(Kind::File),
            "heading" => Some(Kind::Heading),
            "symbol" => Some(Kind::Symbol),
            "quote" => Some(Kind::Quote),
            "range" => Some(Kind::Range),
            "lines" => Some(Kind::Lines),
            _ => None,
        }
    }

    /// Whether the kind carries a quote (`heading`, `symbol`, `quote`, `range`; I-F9).
    pub fn has_quote(self) -> bool {
        matches!(
            self,
            Kind::Heading | Kind::Symbol | Kind::Quote | Kind::Range
        )
    }
}

/// `mode` ([F08 §10.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 1 `live`.
    Live,
    /// 2 `pinned`: a historical citation, never re-resolved.
    Pinned,
}

/// `watch` ([F08 §10.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    /// 1 `header`.
    Header,
    /// 2 `span`.
    Span,
}

/// The anchor record of [F08 §10.3] with its texts. When `text_unavailable` is set the texts are empty and the
/// digests are the stored ones; otherwise the digests are computed from the texts ([`Anchor::digest`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// The anchor uid.
    pub uid: Uid,
    /// `kind`.
    pub kind: Kind,
    /// `mode`.
    pub mode: Mode,
    /// `watch`.
    pub watch: Watch,
    /// `resolver`.
    pub resolver: u16,
    /// `captured`.
    pub captured: [u8; 16],
    /// `pred`.
    pub pred: Option<Uid>,
    /// `hint_first`, `hint_last` (1-based, inclusive); `None` for a `file` anchor.
    pub hint: Option<(u32, u32)>,
    /// The scope value's bytes; empty without a scope.
    pub scope: Vec<u8>,
    /// `text_unavailable`.
    pub text_unavailable: bool,
    /// `quote` (for `range`, the start quote).
    pub quote: Vec<u8>,
    /// `prefix`.
    pub prefix: Vec<u8>,
    /// `suffix`.
    pub suffix: Vec<u8>,
    /// `end` (a `range` anchor's end quote).
    pub end: Vec<u8>,
    /// The stored digests of an anchor without its text: `quote_h`, `prefix_h`, `suffix_h`, `end_h`.
    pub stored_digests: [Option<[u8; 16]>; 4],
    /// `occurrence`.
    pub occurrence: Option<u16>,
    /// `window`: the window value W; empty for a `file` anchor.
    pub window: Vec<u8>,
    /// `span_hash`; `None` for a `file` anchor.
    pub span_hash: Option<u64>,
    /// `blob`: the file's `oid` at capture; `None` (`algo` `none`) for a planned target.
    pub blob: Option<Oid>,
    /// `git`.
    pub git: Option<Oid>,
    /// `marker`; empty when none.
    pub marker: String,
}

impl Anchor {
    /// A digest of [F07 §8.2] (index 0 `quote_h`, 1 `prefix_h`, 2 `suffix_h`, 3 `end_h`): BLAKE3-128 of the held text,
    /// or the stored digest of a text-unavailable anchor; `None` where the kind has no such text.
    // spec: [F08 §10.3] orders 13, 15, 17, 19; [40 §2.11] R-10
    pub fn digest(&self, i: usize) -> Option<[u8; 16]> {
        let applies = if i == 3 {
            self.kind == Kind::Range
        } else {
            self.kind.has_quote()
        };
        if !applies {
            return None;
        }
        if self.text_unavailable {
            return self.stored_digests[i];
        }
        let t = [&self.quote, &self.prefix, &self.suffix, &self.end][i];
        Some(blake3_128(&[t]))
    }

    /// The current selectors ([F08 §11.4] step 1).
    pub fn selectors(&self) -> Selectors {
        Selectors {
            kind: self.kind,
            scope: self.scope.clone(),
            quote: self.quote.clone(),
            prefix: self.prefix.clone(),
            suffix: self.suffix.clone(),
            end: self.end.clone(),
            occurrence: self.occurrence,
            window: self.window.clone(),
        }
    }

    /// The anchor as capture's identity steps read it on (s, f).
    pub fn edge_anchor(&self) -> EdgeAnchor {
        EdgeAnchor {
            uid: self.uid,
            captured: self.captured,
            pred: self.pred,
            current: self.selectors(),
            digests: self.text_unavailable.then(|| {
                [
                    self.digest(0),
                    self.digest(1),
                    self.digest(2),
                    self.digest(3),
                ]
            }),
        }
    }

    /// The hashed selector fields the canonical form carries ([F07 §8.2]).
    pub fn to_canon(&self) -> crate::canon::Anchor {
        crate::canon::Anchor {
            kind: self.kind.name().into(),
            mode: match self.mode {
                Mode::Live => "live",
                Mode::Pinned => "pinned",
            }
            .into(),
            watch: match self.watch {
                Watch::Header => "header",
                Watch::Span => "span",
            }
            .into(),
            scope: self.scope.clone(),
            quote_h: self.digest(0),
            prefix_h: self.digest(1),
            suffix_h: self.digest(2),
            end_h: self.digest(3),
            occurrence: self.occurrence,
            hint: self.hint,
            window: self.window.clone(),
            span_hash: self.span_hash,
            blob: self.blob.clone(),
            git: self.git.clone(),
            captured: self.captured,
            pred: self.pred.map(|p| p.0),
            marker: self.marker.clone(),
            resolver: self.resolver,
            text: (!self.text_unavailable).then(|| {
                Box::new(crate::canon::AnchorText {
                    quote: self.quote.clone(),
                    prefix: self.prefix.clone(),
                    suffix: self.suffix.clone(),
                    end: self.end.clone(),
                })
            }),
        }
    }

    /// The record of a stored anchor ([F08 §10.3]): the edge key's discriminator as its uid and the fields the edge
    /// holds. An anchor held without its text is `text_unavailable`, with the stored digests.
    pub fn from_canon(uid: Uid, c: &crate::canon::Anchor) -> Anchor {
        let t = c.text.as_deref();
        Anchor {
            uid,
            kind: Kind::from_name(&c.kind).unwrap_or_default(),
            mode: if c.mode == "pinned" {
                Mode::Pinned
            } else {
                Mode::Live
            },
            watch: if c.watch == "span" {
                Watch::Span
            } else {
                Watch::Header
            },
            resolver: c.resolver,
            captured: c.captured,
            pred: c.pred.map(Uid),
            hint: c.hint,
            scope: c.scope.clone(),
            text_unavailable: t.is_none(),
            quote: t.map(|t| t.quote.clone()).unwrap_or_default(),
            prefix: t.map(|t| t.prefix.clone()).unwrap_or_default(),
            suffix: t.map(|t| t.suffix.clone()).unwrap_or_default(),
            end: t.map(|t| t.end.clone()).unwrap_or_default(),
            stored_digests: [c.quote_h, c.prefix_h, c.suffix_h, c.end_h],
            occurrence: c.occurrence,
            window: c.window.clone(),
            span_hash: c.span_hash,
            blob: c.blob.clone(),
            git: c.git.clone(),
            marker: c.marker.clone(),
        }
    }
}

/// The anchor constants of [F20 §7] that are named holes, at the values a run evaluates.
#[derive(Clone, Copy, Debug)]
pub struct Consts {
    /// `WIN` (HOLE F20-window-lines).
    pub win: usize,
    /// `QUOTE_LINES` (HOLE F20-quote-lines).
    pub quote_lines: usize,
    /// `QUOTE_MAX` (HOLE F20-quote-max).
    pub quote_max: usize,
    /// `QUOTE_DEFAULT` (HOLE F20-quote-default).
    pub quote_default: usize,
    /// `CONTEXT` (HOLE F20-context).
    pub context: usize,
    /// `CONTEXT_MAX` (HOLE F20-context-max).
    pub context_max: usize,
    /// `CONTEXT_MARGIN` (HOLE F20-context-margin).
    pub context_margin: (u128, u128),
    /// `WINDOW_MARGIN` (HOLE F20-window-margin).
    pub window_margin: (u128, u128),
    /// `RANGE_SPREAD` (HOLE F20-range-spread).
    pub range_spread: usize,
    /// `FUZZY_BUDGET` (HOLE F20-fuzzy-budget).
    pub fuzzy_budget: (u128, u128),
    /// `SPAN` (HOLE F20-fuzzy-span), bytes on each side of the hint.
    pub span: usize,
    /// The fuzzy weights (w1, w2, w3, w4) (HOLE F20-fuzzy-weights).
    pub weights: (u128, u128, u128, u128),
    /// `FUZZY_ACCEPT` (HOLE F20-fuzzy-accept).
    pub fuzzy_accept: (u128, u128),
    /// `FUZZY_MARGIN` (HOLE F20-fuzzy-margin).
    pub fuzzy_margin: (u128, u128),
    /// `HEADER_MARGIN` (HOLE F20-header-margin).
    pub header_margin: (u128, u128),
    /// `LINES_MIN` (HOLE F20-lines-min).
    pub lines_min: (u128, u128),
    /// `files.max-line-hashes` ([CFG]): the line-hash cap of [F20 §2.4]; `None` for no cap.
    pub max_line_hashes: Option<usize>,
    /// `files.max-read-bytes` ([CFG]): content larger than this is `Unavailable(size)` ([F20 §2.4] item 1), so every
    /// anchor on it is `unverified (size)` ([F20 §1.5]); `None` for no limit.
    pub max_read_bytes: Option<u64>,
}

impl Consts {
    /// The draft values of [F20 §7] and the defaults of `files.max-line-hashes` (65,536) and `files.max-read-bytes`
    /// (16 MiB).
    pub const DRAFT: Consts = Consts {
        win: 16,
        quote_lines: 4,
        quote_max: 128,
        quote_default: 64,
        context: 32,
        context_max: 64,
        context_margin: (1, 10),
        window_margin: (15, 100),
        range_spread: 2,
        fuzzy_budget: (1, 4),
        span: 16_384,
        weights: (50, 20, 20, 10),
        fuzzy_accept: (3, 4),
        fuzzy_margin: (2, 100),
        header_margin: (1, 10),
        lines_min: (1, 2),
        max_line_hashes: Some(65_536),
        max_read_bytes: Some(16 << 20),
    };
}

fn r(p: (u128, u128)) -> Ratio {
    Ratio::new(p.0, p.1)
}

/// An authoring form ([40 §2.7] authoring table) as capture takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Form {
    /// `path`: a `file` anchor.
    File,
    /// `path:L-M` (`path:L` with M = L).
    Lines(u32, u32),
    /// `path@<commit>:L-M`: a `quote` (or `range`, `lines`) anchor with mode `pinned`, captured on the commit's
    /// content.
    Pinned(Oid, u32, u32),
    /// `--quote-file` or stdin: a quote from literal text.
    QuoteText(Vec<u8>),
    /// `path::A/B`: refused while the interim scanner rule holds.
    Symbol(String),
    /// `path#H`: refused while the interim scanner rule holds.
    Heading(String),
}

fn spec_refusal(case: &str, msg: String) -> Refusal {
    Refusal::new("anchor_spec", 2, msg).key("case", case)
}

/// The quote text of a quote-file form ([F20 §6.1] step 3): one leading BOM removed, U+FFFD refused, CR LF → LF, each
/// line `nl`-trimmed, leading and trailing empty lines dropped, joined by `0A`.
fn quote_text(input: &[u8]) -> Result<Vec<u8>, Refusal> {
    let b = input.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(input);
    if b.windows(3).any(|w| w == [0xEF, 0xBF, 0xBD]) {
        return Err(spec_refusal(
            "fffd",
            "the quote text contains U+FFFD (a replacement character)".into(),
        ));
    }
    let mut v = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x0D && b.get(i + 1) == Some(&0x0A) {
            v.push(0x0A);
            i += 2;
        } else {
            v.push(b[i]);
            i += 1;
        }
    }
    let ls: Vec<&[u8]> = crate::r4::text::lines(&v)
        .into_iter()
        .map(crate::r4::text::nl)
        .collect();
    let first = ls.iter().position(|l| !l.is_empty());
    let last = ls.iter().rposition(|l| !l.is_empty());
    let (Some(f), Some(l)) = (first, last) else {
        return Err(spec_refusal("empty", "the quote text is empty".into()));
    };
    Ok(ls[f..=l].join(&0x0A))
}

/// The byte ranges of a located quote in N: `quote` (start, end) and for `range` the end quote's (start, end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hit {
    /// The quote's first byte.
    h: usize,
    /// After the quote (for `range`, after the end quote).
    z: usize,
}

/// The exact hits of an anchor's quote in N ([F20 §6.2] step 4), in offset order: every offset, overlapping hits
/// included; for `range`, each start hit h paired with one end hit within the spread by [`pair_end`], unpaired ones
/// dropped.
// spec: [F20 §6.2] step 4
fn exact_hits(nt: &NText, a: &Anchor, c: &Consts) -> Vec<Hit> {
    let n = &nt.n;
    let q = &a.quote;
    if q.is_empty() || q.len() > n.len() {
        return Vec::new();
    }
    let starts = (0..=n.len() - q.len()).filter(|&h| &n[h..h + q.len()] == q.as_slice());
    if a.kind != Kind::Range {
        return starts.map(|h| Hit { h, z: h + q.len() }).collect();
    }
    starts
        .filter_map(|h| pair_end(nt, a, c, h, h + q.len()).map(|z| Hit { h, z }))
        .collect()
}

/// The offset of N after which a `range` anchor's end quote may not end, for a start at h ([F20 §6.2] step 4): an end
/// hit's last byte lies on a line ≤ `line(h) + RANGE_SPREAD × (h2 − h1 + 1) − 1` iff it ends at most just after the
/// `0A` that ends that line.
fn end_bound(nt: &NText, a: &Anchor, c: &Consts, h: usize) -> usize {
    let hint_len = a.hint.map_or(1, |(f, l)| (l - f + 1) as usize);
    let lim = nt.line_of(h) + c.range_spread * hint_len - 1;
    if lim >= nt.len() {
        nt.n.len()
    } else {
        nt.end(lim) + 1
    }
}

/// The exact end hit a `range` start pairs with ([F20 §6.2] step 4; the exact end search of §6.4's `range` bullet):
/// for a start at h whose start quote ends at `qz`, the offset after the chosen end hit, or `None` when none pairs.
///
/// **Which end hits pair.** Every offset x with `N[x .. x + len(end)) = end`, x ≥ h, `x + len(end) ≥ qz` and the last
/// byte within [`end_bound`]. An end hit may overlap the start quote: a range whose span text is shorter than its two
/// quotes together has overlapping quotes (both are cut from one span text, [F20 §6.1] step 3), and the literal
/// "h_e ≥ h + len(exact)" would never pair it, not even on its own content (WP-92 spec finding A1, pending the
/// R-SPEC-R sync of [F20 §6.2] step 4).
///
/// **Which one is taken.** The last whose last line is `line(h) + (h2 − h1)` — the captured span's length, so a
/// shifted range pairs as it was captured — and, when there is none, the first. Pairing with the first end hit alone
/// picks a too-early end whenever the end quote recurs inside the span (repeated lines), and a result with a shorter
/// span is never at least as conservative as the moved range ([F20 §6.6]) (WP-92 spec finding A1). The exact and the
/// fuzzy step pair exact end hits by this one rule.
// spec: [F20 §6.2] step 4; [F20 §6.4] `range` anchors
fn pair_end(nt: &NText, a: &Anchor, c: &Consts, h: usize, qz: usize) -> Option<usize> {
    let (n, e) = (&nt.n, &a.end);
    if e.is_empty() || e.len() > n.len() {
        return None;
    }
    let want = nt.line_of(h) + a.hint.map_or(0, |(f, l)| (l - f) as usize);
    let upto = end_bound(nt, a, c, h);
    let mut first = None;
    let mut preferred = None;
    let mut x = qz.saturating_sub(e.len()).max(h);
    while x + e.len() <= upto {
        if &n[x..x + e.len()] == e.as_slice() {
            first.get_or_insert(x);
            if nt.line_of(x + e.len() - 1) == want {
                preferred = Some(x);
            }
        }
        x += 1;
    }
    preferred.or(first).map(|x| x + e.len())
}

/// The 1-based line span of bytes [h, z) of N.
fn span_of(nt: &NText, h: usize, z: usize) -> (u32, u32) {
    let last = if z > h { z - 1 } else { h };
    (nt.line_of(h) as u32, nt.line_of(last) as u32)
}

/// The length of the longest common suffix of `a` and `b`.
fn lcsuffix(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// The length of the longest common prefix of `a` and `b`.
fn lcprefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// The context score of a hit ([F20 §6.2] step 6.1): `(pa + sa) / 2`.
// spec: [F20 §6.2] step 6.1
fn context_score(n: &[u8], a: &Anchor, hit: Hit) -> Ratio {
    let pa = if a.prefix.is_empty() {
        Ratio::int(1)
    } else {
        let lo = hit.h.saturating_sub(a.prefix.len());
        Ratio::new(
            lcsuffix(&a.prefix, &n[lo..hit.h]) as u128,
            a.prefix.len() as u128,
        )
    };
    let sa = if a.suffix.is_empty() {
        Ratio::int(1)
    } else {
        let hi = (hit.z + a.suffix.len()).min(n.len());
        Ratio::new(
            lcprefix(&a.suffix, &n[hit.z..hi]) as u128,
            a.suffix.len() as u128,
        )
    };
    let s = pa + sa;
    Ratio::new(s.num, s.den * 2)
}

/// The length of the longest common subsequence of two token sequences (two row buffers).
fn lcs(a: &[u16], b: &[u16]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for x in a {
        cur[0] = 0;
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j] + 1
            } else {
                prev[j + 1].max(cur[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The window score ([F20 §6.3]) of a stored window against the window around lines [s, e] of the current text;
/// `None` when the window step is unavailable (the line-hash array is capped, [F20 §2.4]) or the stored value is not
/// valid.
// spec: [F20 §6.3]
fn window_score(nt: &NText, a: &Anchor, s: usize, e: usize, c: &Consts) -> Option<Ratio> {
    if c.max_line_hashes.is_some_and(|m| nt.len() > m) {
        return None;
    }
    let (bs, as_) = parse_window(&a.window, c.win)?;
    let total = bs.len() + as_.len();
    if total == 0 {
        return Some(Ratio::int(0));
    }
    let bc = nt.before(s, c.win);
    let ac = nt.after(e, c.win);
    Some(Ratio::new(
        (lcs(&bs, &bc) + lcs(&as_, &ac)) as u128,
        total as u128,
    ))
}

/// The unique best of scored items with a margin over the runner-up: its index, when the list is non-empty and the
/// best exceeds the next by at least `margin` (a single item is the unique best).
fn unique_best(scores: &[Ratio], margin: Ratio) -> Option<usize> {
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&x, &y| scores[y].cmp(&scores[x]).then(x.cmp(&y)));
    let best = *idx.first()?;
    match idx.get(1) {
        None => Some(best),
        Some(&second) => (scores[best].saturating_sub(scores[second]) >= margin
            && scores[best] > scores[second])
            .then_some(best),
    }
}

/// The outcome of the exact step over the hits ([F20 §6.2] steps 5–6): the chosen hit, or `Err(true)` for
/// `ambiguous`, `Err(false)` when there is no hit.
// spec: [F20 §6.2] steps 5, 6
fn choose_exact(nt: &NText, a: &Anchor, hits: &[Hit], c: &Consts) -> Result<Hit, bool> {
    match hits.len() {
        0 => Err(false),
        1 => Ok(hits[0]),
        _ => {
            let ctx: Vec<Ratio> = hits.iter().map(|&h| context_score(&nt.n, a, h)).collect();
            if let Some(i) = unique_best(&ctx, r(c.context_margin)) {
                return Ok(hits[i]);
            }
            let ws: Option<Vec<Ratio>> = hits
                .iter()
                .map(|&h| {
                    let (s, e) = span_of(nt, h.h, h.z);
                    window_score(nt, a, s as usize, e as usize, c)
                })
                .collect();
            if let Some(ws) = ws
                && let Some(i) = unique_best(&ws, r(c.window_margin))
            {
                return Ok(hits[i]);
            }
            if let Some(o) = a.occurrence
                && let Some(h) = hits.get(usize::from(o).wrapping_sub(1))
            {
                return Ok(*h);
            }
            Err(true)
        }
    }
}

/// The resolution of one anchor ([F18 §4.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AState {
    /// 1 `fresh`.
    Fresh,
    /// 2 `moved`.
    Moved,
    /// 3 `edited`.
    Edited,
    /// 4 `ambiguous`.
    Ambiguous,
    /// 5 `orphaned`.
    Orphaned,
    /// 6 `unverified`, with the reason's detail code of [F18 §4.6] (53–61).
    Unverified(u8),
    /// 7 `unresolved`: the cascade did not run (a pinned anchor); LQ only, never stored.
    Unresolved,
}

impl AState {
    /// The frozen string ([F18 §4.3]).
    pub fn name(self) -> &'static str {
        match self {
            AState::Fresh => "fresh",
            AState::Moved => "moved",
            AState::Edited => "edited",
            AState::Ambiguous => "ambiguous",
            AState::Orphaned => "orphaned",
            AState::Unverified(_) => "unverified",
            AState::Unresolved => "unresolved",
        }
    }
}

/// An anchor result: the state, the span it names (1-based lines), for a fuzzy match its score, and the detail codes
/// the anchor adds to its link ([F18 §4.6]: 4 `body-changed`, 67 `text-unavailable`), in [F18 §4.7] rule-1 order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AResult {
    /// The state.
    pub state: AState,
    /// The span, for `fresh`, `moved` and `edited`.
    pub span: Option<(u32, u32)>,
    /// The fuzzy score when the fuzzy step decided.
    pub score: Option<Ratio>,
    /// Detail codes 4 and 67.
    pub details: Vec<u8>,
}

impl AResult {
    fn of(state: AState) -> AResult {
        AResult {
            state,
            span: None,
            score: None,
            details: Vec::new(),
        }
    }

    fn at(state: AState, span: (u32, u32)) -> AResult {
        AResult {
            span: Some(span),
            ..AResult::of(state)
        }
    }
}

/// The content an anchor resolves against: the current bytes, or why they are unavailable (a detail code of
/// [F18 §4.6], 53–59, [F20 §1.5]).
#[derive(Clone, Copy, Debug)]
pub enum Content<'a> {
    /// The current bytes.
    Bytes(&'a [u8]),
    /// Unavailable, with the reason's detail code.
    Unavailable(u8),
}

/// Levenshtein distance over bytes with unit costs (two row buffers).
fn lev(a: &[u8], b: &[u8]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j + 1] + 1)
                .min(cur[j] + 1)
                .min(prev[j] + usize::from(x != y));
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// d(e) for every end offset e of [lo, hi]: the least distance between `p` and a substring `t[s..e)` with s ≥ lo, by
/// the dynamic programme with a free start ([F20 §6.4]), one column per text byte in two buffers.
fn distances(p: &[u8], t: &[u8], lo: usize, hi: usize) -> Vec<usize> {
    let m = p.len();
    let mut col: Vec<usize> = (0..=m).collect();
    let mut next = vec![0usize; m + 1];
    let mut out = Vec::with_capacity(hi - lo + 1);
    out.push(col[m]);
    for &y in &t[lo..hi] {
        next[0] = 0;
        for i in 1..=m {
            next[i] = (col[i] + 1)
                .min(next[i - 1] + 1)
                .min(col[i - 1] + usize::from(p[i - 1] != y));
        }
        std::mem::swap(&mut col, &mut next);
        out.push(col[m]);
    }
    out
}

/// The largest start s ∈ [s_lo, e] with `lev(p, t[s..e)) = d`, by one dynamic programme over p and the text read
/// backwards from e: row i holds `lev(p[m − i ..], t[e − j .. e))` for j = 0 … e − s_lo, so the last row gives every
/// start's distance at once and the smallest j with distance d is the largest start.
fn largest_start(p: &[u8], t: &[u8], s_lo: usize, e: usize, d: usize) -> Option<usize> {
    let jmax = e - s_lo;
    let mut prev: Vec<usize> = (0..=jmax).collect();
    let mut cur = vec![0usize; jmax + 1];
    for i in 1..=p.len() {
        let x = p[p.len() - i];
        cur[0] = i;
        for j in 1..=jmax {
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + usize::from(x != t[e - j]));
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev.iter().position(|&v| v == d).map(|j| e - j)
}

/// A fuzzy candidate [s, e) with its distance.
#[derive(Clone, Copy, Debug)]
struct Fuzzy {
    s: usize,
    e: usize,
    d: usize,
}

/// The fuzzy candidates of `p` in the region [lo, hi) of `t` ([F20 §6.4] "Candidates in a region"): every e with
/// d(e) ≤ k, taken least by (d, e) while suppressing ends nearer than `len(p)`, each with the largest start of
/// distance d(e).
// spec: [F20 §6.4] candidates in a region
fn fuzzy_candidates(p: &[u8], t: &[u8], lo: usize, hi: usize, k: usize) -> Vec<Fuzzy> {
    if p.is_empty() || lo > hi {
        return Vec::new();
    }
    let d = distances(p, t, lo, hi);
    let mut es: Vec<(usize, usize)> = d
        .iter()
        .enumerate()
        .filter(|(_, x)| **x <= k)
        .map(|(i, &x)| (x, lo + i))
        .collect();
    es.sort_unstable();
    let mut picked: Vec<(usize, usize)> = Vec::new();
    let mut ends: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for (dist, e) in es {
        // Suppressed when a picked end lies within len(p) of e.
        let near = e.saturating_sub(p.len() - 1)..e + p.len();
        if ends.range(near).next().is_some() {
            continue;
        }
        ends.insert(e);
        picked.push((dist, e));
    }
    picked
        .into_iter()
        .map(|(dist, e)| {
            // A substring longer than len(p) + k is farther than k, so the optimal start is at least e − len(p) − k.
            let s_lo = lo.max(e.saturating_sub(p.len() + k));
            let s = largest_start(p, t, s_lo, e, dist).expect("d(e) is attained by some start");
            Fuzzy { s, e, d: dist }
        })
        .collect()
}

/// `1 − lev(x, y) / len(x)`, 1 for an empty x.
fn sim(x: &[u8], y: &[u8]) -> Ratio {
    if x.is_empty() {
        return Ratio::int(1);
    }
    let d = lev(x, y) as u128;
    let l = x.len() as u128;
    Ratio::new(l.saturating_sub(d), l)
}

/// A scored fuzzy candidate.
#[derive(Clone, Copy, Debug)]
struct Scored {
    s: usize,
    e: usize,
    score: Ratio,
}

/// The fuzzy step ([F20 §6.4]) over the regions in order: the accepted candidate (`Ok`) or `Err(true)` for
/// `ambiguous`, `Err(false)` for nothing.
// spec: [F20 §6.4]
fn fuzzy(nt: &NText, a: &Anchor, c: &Consts) -> Result<Scored, bool> {
    let n = &nt.n;
    let q = &a.quote;
    if q.is_empty() {
        return Err(false);
    }
    let k = (c.fuzzy_budget.0 * q.len() as u128 / c.fuzzy_budget.1) as usize;
    let mut regions: Vec<(usize, usize)> = Vec::new();
    if let Some((h1, h2)) = a.hint {
        let (h1, h2) = (h1 as usize, (h2 as usize).min(nt.len()));
        if h1 >= 1 && h1 <= nt.len() {
            let (ra, rz) = (nt.start(h1), nt.end(h2.max(h1)));
            regions.push((ra.saturating_sub(c.span), (rz + c.span).min(n.len())));
        }
    }
    // R2, the scope's range, never resolves while the interim scanner rule holds ([F20 §6.1]).
    regions.push((0, n.len()));
    let (w1, w2, w3, w4) = c.weights;
    for (lo, hi) in regions {
        let mut scored = Vec::new();
        for f in fuzzy_candidates(q, n, lo, hi, k) {
            let (s, e, qd) = if a.kind == Kind::Range {
                // The end quote within the spread, exactly by §6.2 step 4's pairing rule, then fuzzily over the same
                // bounds (an end candidate starting at or after the start candidate and ending at or after it), the
                // best by (q desc, offset asc).
                let qs = Ratio::new((q.len() - f.d.min(q.len())) as u128, q.len() as u128);
                if let Some(z) = pair_end(nt, a, c, f.s, f.e) {
                    (f.s, z, qs)
                } else {
                    let ek = (c.fuzzy_budget.0 * a.end.len() as u128 / c.fuzzy_budget.1) as usize;
                    let upto = end_bound(nt, a, c, f.s).max(f.e);
                    let best = fuzzy_candidates(&a.end, n, f.s, upto, ek)
                        .into_iter()
                        .filter(|x| x.e >= f.e)
                        .min_by(|x, y| x.d.cmp(&y.d).then(x.s.cmp(&y.s)));
                    let Some(end) = best else { continue };
                    let qe = Ratio::new(
                        (a.end.len() - end.d.min(a.end.len())) as u128,
                        a.end.len().max(1) as u128,
                    );
                    (f.s, end.e, qs.min(qe))
                }
            } else {
                (
                    f.s,
                    f.e,
                    Ratio::new((q.len() - f.d.min(q.len())) as u128, q.len() as u128),
                )
            };
            if qd < r(c.fuzzy_accept) {
                continue;
            }
            let ps = sim(&a.prefix, &n[s.saturating_sub(a.prefix.len())..s]);
            let ss = sim(&a.suffix, &n[e..(e + a.suffix.len()).min(n.len())]);
            let (ls, le) = span_of(nt, s, e);
            let ws = window_score(nt, a, ls as usize, le as usize, c).unwrap_or(Ratio::int(0));
            let num = qd.mul_int(w1) + ps.mul_int(w2) + ss.mul_int(w3) + ws.mul_int(w4);
            let score = Ratio::new(num.num, num.den * (w1 + w2 + w3 + w4));
            scored.push(Scored { s, e, score });
        }
        if scored.is_empty() {
            continue;
        }
        scored.sort_by(|x, y| y.score.cmp(&x.score).then(x.s.cmp(&y.s)));
        let margin = r(c.fuzzy_margin);
        return match scored.get(1) {
            None => Ok(scored[0]),
            Some(second)
                if scored[0].score.saturating_sub(second.score) >= margin
                    && scored[0].score > second.score =>
            {
                Ok(scored[0])
            }
            Some(_) => Err(true),
        };
    }
    Err(false)
}

/// The `lines` rule ([F20 §6.5]): the window alignment over every start line j, the best accepted at `LINES_MIN` with
/// `WINDOW_MARGIN`, then the span hash; `None` when the line-hash array is capped (`unverified (size)`).
// spec: [F20 §6.5] lines anchors
fn lines_rule(nt: &NText, a: &Anchor, len: usize, c: &Consts) -> Option<AResult> {
    if c.max_line_hashes.is_some_and(|m| nt.len() > m) {
        return None;
    }
    if len == 0 || len > nt.len() {
        return Some(AResult::of(AState::Orphaned));
    }
    let js: Vec<usize> = (1..=nt.len() + 1 - len).collect();
    let scores: Vec<Ratio> = js
        .iter()
        .map(|&j| window_score(nt, a, j, j + len - 1, c).unwrap_or(Ratio::int(0)))
        .collect();
    let Some(i) = unique_best(&scores, r(c.window_margin)) else {
        return Some(AResult::of(AState::Orphaned));
    };
    if scores[i] < r(c.lines_min) {
        return Some(AResult::of(AState::Orphaned));
    }
    let j = js[i];
    let span = (j as u32, (j + len - 1) as u32);
    if a.span_hash == Some(xxh3(nt.st(j, j + len - 1))) {
        let fresh = a.hint.is_some_and(|(h1, _)| h1 as usize == j);
        Some(AResult::at(
            if fresh { AState::Fresh } else { AState::Moved },
            span,
        ))
    } else {
        Some(AResult::of(AState::Orphaned))
    }
}

/// The hint step ([F20 §6.2] step 1): `fresh` at the hint when the span hash (for `header` watch, the header's hash)
/// matches there.
// spec: [F20 §6.2] step 1
fn hint_step(nt: &NText, a: &Anchor) -> Option<AResult> {
    let (h1, h2) = a.hint?;
    let (h1u, h2u) = (h1 as usize, h2 as usize);
    if h1u == 0 || h2u > nt.len() || h1u > h2u {
        return None;
    }
    let got = match a.watch {
        Watch::Span => xxh3(nt.st(h1u, h2u)),
        Watch::Header => xxh3(header(nt, h1u, a.kind == Kind::Symbol)),
    };
    (Some(got) == a.span_hash).then(|| AResult::at(AState::Fresh, (h1, h2)))
}

/// The span a located quote names, exactly or fuzzily ([F20 §6.5] watch): the lines from the quote's first byte to its
/// last (for `range`, the end quote's), and for a `symbol` or `heading` anchor, whose hint is the item's range, at
/// least the hint's length from the located header line (no scanner finds the item's end while [F20 §6.1]'s interim
/// rule holds), within the text — one rule for the exact and the fuzzy step (WP-92 spec finding A3, pending the
/// R-SPEC-R sync of [F20 §6.2] step 5 and §6.4: [F20] names no span for a located hit).
fn located_span(nt: &NText, a: &Anchor, h: Hit) -> (u32, u32) {
    let (s, e) = span_of(nt, h.h, h.z);
    match (a.kind, a.hint) {
        (Kind::Symbol | Kind::Heading, Some((h1, h2))) => {
            (s, e.max(s + (h2 - h1)).min(nt.len() as u32))
        }
        _ => (s, e),
    }
}

/// The brute-force anchor resolver ([F20 §6.2]–§6.5; [40 §4.5], §8.3.2 P11): the anchor state and span of `a` on the
/// current content, for a file that resolved `ok` or `moved-auto`. A pinned anchor is never re-resolved
/// (`unresolved`); a `file` anchor is `fresh` (under `header` watch with detail 4 when the content's `oid` differs from
/// `blob`), or under `span` watch `edited` when it differs; binary content orphans every span anchor; unavailable
/// content is `unverified` with its reason. Every result of an anchor imported without its text carries detail 67.
///
/// **Span watch** ([F20 §6.5]: "a changed span is `edited`"), one rule for every kind with a quote: when the located
/// span's text does not hash to `span_hash`, the result is `edited` at that span — a `range` whose middle changed, and
/// a `quote` from `--quote-file` whose line changed outside the quote. [F20 §6.2] step 5's "one hit → `moved` (or
/// `fresh` when it lies at the hint)" would call both `fresh` at the hint, against §6.5's "a changed span is `edited`"
/// (WP-92 spec finding A2, pending the R-SPEC-R sync of [F20 §6.2] steps 5–6 and §6.5 Watch).
///
/// **The marker step** ([F20 §6.2] step 2, [40 §4.5] step 2: opt-in prose, "marker found and the quote matches →
/// `fresh`, else `edited`") does not run: no chapter gives the marker syntax a file holds, so no content can be tested
/// for a marker (WP-92 spec finding A4, pending the R-SPEC-R sync of [F20 §6.2] step 2); `Anchor::marker` is carried
/// for the canonical form only.
// spec: [F20 §6.2]; [F20 §6.4]; [F20 §6.5]; [40 §4.5]; [F18 §2.10] I-F10
pub fn resolve(a: &Anchor, content: Content<'_>, algo: Algo, c: &Consts) -> AResult {
    if a.mode == Mode::Pinned {
        return AResult::of(AState::Unresolved);
    }
    let mut r = resolve_live(a, content, algo, c);
    if a.text_unavailable {
        r.details.push(67);
    }
    r
}

fn resolve_live(a: &Anchor, content: Content<'_>, algo: Algo, c: &Consts) -> AResult {
    let b = match content {
        Content::Bytes(b) if c.max_read_bytes.is_some_and(|m| b.len() as u64 > m) => {
            return AResult::of(AState::Unverified(58));
        }
        Content::Bytes(b) => b,
        Content::Unavailable(reason) => return AResult::of(AState::Unverified(reason)),
    };
    if a.kind == Kind::File {
        let Some(blob) = &a.blob else {
            return AResult::of(AState::Fresh);
        };
        // The content's `oid` under the root's algorithm; another algorithm than the blob's is `unverified` under
        // `span` watch and, under `header` watch, no body change can be told.
        let same = oid_in(&oid(algo, b), &[blob]);
        return match (a.watch, same) {
            (Watch::Header, Some(false)) => AResult {
                details: vec![4],
                ..AResult::of(AState::Fresh)
            },
            (Watch::Header, _) | (Watch::Span, Some(true)) => AResult::of(AState::Fresh),
            (Watch::Span, Some(false)) => AResult::of(AState::Edited),
            (Watch::Span, None) => AResult::of(AState::Unverified(61)),
        };
    }
    if !is_text(b) {
        return AResult::of(AState::Orphaned);
    }
    let t = atext(b).expect("text content");
    let nt = NText::of(&t);
    if let Some(x) = hint_step(&nt, a) {
        return x;
    }
    if a.kind == Kind::Lines || a.text_unavailable {
        let len = a.hint.map_or(0, |(f, l)| (l - f + 1) as usize);
        return lines_rule(&nt, a, len, c).unwrap_or(AResult::of(AState::Unverified(58)));
    }
    let hits = exact_hits(&nt, a, c);
    match choose_exact(&nt, a, &hits, c) {
        Ok(h) => {
            let span = located_span(&nt, a, h);
            let changed = a.watch == Watch::Span
                && a.span_hash != Some(xxh3(nt.st(span.0 as usize, span.1 as usize)));
            let state = if changed {
                AState::Edited
            } else if a.hint == Some(span) {
                AState::Fresh
            } else {
                AState::Moved
            };
            AResult::at(state, span)
        }
        Err(true) => AResult::of(AState::Ambiguous),
        Err(false) => match fuzzy(&nt, a, c) {
            Ok(s) => AResult {
                score: Some(s.score.reduced()),
                ..AResult::at(AState::Edited, located_span(&nt, a, Hit { h: s.s, z: s.e }))
            },
            Err(true) => AResult::of(AState::Ambiguous),
            Err(false) => AResult::of(AState::Orphaned),
        },
    }
}

/// "r1 ⊒ r2": r1 is at least as conservative as r2 ([F20 §6.6]; [40 §8.3.2] S-12): `moved` and `fresh` with one span
/// are equal; `ambiguous` and `orphaned` are ⊒ every result with a span; `edited` with span S is ⊒ `fresh` or `moved`
/// with span S; `unverified` is ⊒ everything; a result with a span is never ⊒ a result with a different span.
// spec: [F20 §6.6]; [40 §8.3.2] P11
pub fn at_least_as_conservative(r1: &AResult, r2: &AResult) -> bool {
    use AState::*;
    let exact = |s: AState| matches!(s, Fresh | Moved);
    if r1.span == r2.span && (r1.state == r2.state || (exact(r1.state) && exact(r2.state))) {
        return true;
    }
    match (r1.state, r2.state) {
        (Unverified(_), _) => true,
        (Ambiguous | Orphaned, _) => r2.span.is_some(),
        (Edited, Fresh | Moved) => r1.span == r2.span,
        _ => false,
    }
}

/// Capture of an anchor on a stated content ([F20 §6.1] steps 1–9; [F08 §11.4]): the anchor, with the capture's
/// identity (a reuse of an anchor on (s, f) with equal current selectors, or a new uid with its predecessor term).
/// `content` is the file's bytes (the commit's content for a pinned form), `None` for a planned target (a `file`
/// anchor only); `anchors` are the anchors on (s, f); `git` is the observed commit.
// spec: [F20 §6.1]; [F08 §11.4]; [40 §2.7] capture
#[allow(clippy::too_many_arguments)]
pub fn capture(
    src: Uid,
    file: Uid,
    form: &Form,
    content: Option<&[u8]>,
    watch: Option<Watch>,
    git: Option<Oid>,
    algo: Algo,
    anchors: &[EdgeAnchor],
    c: &Consts,
) -> Result<(Anchor, CaptureId), Refusal> {
    let mut a = Anchor {
        uid: Uid::ZERO,
        kind: Kind::File,
        mode: Mode::Live,
        watch: Watch::Header,
        resolver: 1,
        captured: [0; 16],
        pred: None,
        hint: None,
        scope: Vec::new(),
        text_unavailable: false,
        quote: Vec::new(),
        prefix: Vec::new(),
        suffix: Vec::new(),
        end: Vec::new(),
        stored_digests: [None; 4],
        occurrence: None,
        window: Vec::new(),
        span_hash: None,
        blob: content.map(|b| oid(algo, b)),
        git,
        marker: String::new(),
    };
    let (first, last, quote_input) = match form {
        Form::Symbol(_) | Form::Heading(_) => {
            return Err(spec_refusal(
                "no-scanner",
                "symbol and heading forms need the scope scanners; use the path:L-M form of the same lines".into(),
            ));
        }
        Form::File => {
            a.watch = watch.unwrap_or(Watch::Header);
            return finish(a, src, file, anchors);
        }
        Form::Lines(l, m) => (*l, *m, None),
        Form::Pinned(commit, l, m) => {
            a.mode = Mode::Pinned;
            a.git = Some(commit.clone());
            (*l, *m, None)
        }
        Form::QuoteText(q) => (0, 0, Some(quote_text(q)?)),
    };
    let b = content.ok_or_else(|| spec_refusal("binary", "a span anchor needs text".into()))?;
    if !is_text(b) {
        return Err(spec_refusal(
            "binary",
            "the file is not text; a span anchor needs text".into(),
        ));
    }
    let t = atext(b).expect("text");
    let nt = NText::of(&t);
    a.watch = watch.unwrap_or(Watch::Span);
    // Steps 1–4: the span, the kind, the quotes and the quote span; the quote's offsets o and o′.
    let (s, e, o, o2) = match &quote_input {
        Some(q) => {
            let Some(o) = (0..=nt.n.len().saturating_sub(q.len()))
                .find(|&h| h + q.len() <= nt.n.len() && &nt.n[h..h + q.len()] == q.as_slice())
            else {
                return Err(spec_refusal(
                    "not-found",
                    "the quote text is not in the file".into(),
                ));
            };
            let (s, e) = span_of(&nt, o, o + q.len());
            (s as usize, e as usize, o, o + q.len())
        }
        None => {
            let (l, m) = (first as usize, last as usize);
            if l < 1 || l > m || m > nt.len() {
                return Err(spec_refusal(
                    "range",
                    format!("{l}-{m} is outside the file ({} lines)", nt.len()),
                ));
            }
            let nontriv: Vec<usize> = (l..=m).filter(|&i| !nt.trivial[i - 1]).collect();
            let (Some(&s), Some(&e)) = (nontriv.first(), nontriv.last()) else {
                // A span of trivial lines: a `lines` anchor with hint [L, M] and the window around [L, M].
                a.kind = Kind::Lines;
                a.hint = Some((l as u32, m as u32));
                let (bw, aw) = (nt.before(l, c.win), nt.after(m, c.win));
                if bw.is_empty() && aw.is_empty() {
                    return Err(spec_refusal(
                        "no-window",
                        "no non-trivial line lies outside the span, so a lines anchor has no window".into(),
                    ));
                }
                a.window = window_value(&bw, &aw);
                a.span_hash = Some(xxh3(nt.st(l, m)));
                return finish(a, src, file, anchors);
            };
            (s, e, nt.start(s), nt.end(e))
        }
    };
    let span_text: Vec<u8> = match &quote_input {
        Some(q) => q.clone(),
        None => nt.st(s, e).to_vec(),
    };
    let nontrivial = (s..=e).filter(|&i| !nt.trivial[i - 1]).count();
    a.kind = if nontrivial <= c.quote_lines && span_text.len() <= c.quote_max {
        Kind::Quote
    } else {
        Kind::Range
    };
    if a.kind == Kind::Quote {
        a.quote = span_text.clone();
    } else {
        a.quote = cutp(&span_text, c.quote_default).to_vec();
        a.end = cuts(&span_text, c.quote_default).to_vec();
    }
    // Step 5: context around the quote; step 6: the window; step 7: the hint.
    a.prefix = cuts(&nt.n[..o], c.context).to_vec();
    a.suffix = cutp(&nt.n[o2..], c.context).to_vec();
    a.window = window_value(&nt.before(s, c.win), &nt.after(e, c.win));
    a.hint = Some((s as u32, e as u32));
    // Step 8: the uniqueness ladder.
    let captured_hit = Hit { h: o, z: o2 };
    let unique = |a: &Anchor| choose_exact(&nt, a, &exact_hits(&nt, a, c), c) == Ok(captured_hit);
    if !unique(&a) {
        a.prefix = cuts(&nt.n[..o], c.context_max).to_vec();
        a.suffix = cutp(&nt.n[o2..], c.context_max).to_vec();
        if !unique(&a) {
            // Rung 2 (the scope) is skipped while the interim scanner rule holds; rung 3 records the occurrence.
            // The captured start's index among the hits: a range whose end quote recurs inside the span pairs with
            // the nearer end, so the start alone identifies the captured hit.
            let hits = exact_hits(&nt, &a, c);
            if let Some(idx) = hits.iter().position(|h| h.h == captured_hit.h) {
                a.occurrence = Some((idx + 1) as u16);
            }
        }
    }
    // Step 9: the span hash over the hint lines.
    a.span_hash = Some(xxh3(nt.st(s, e)));
    finish(a, src, file, anchors)
}

/// The identity steps of a capture ([F08 §11.4]) and the record's uid, `captured` and `pred`.
fn finish(
    mut a: Anchor,
    src: Uid,
    file: Uid,
    anchors: &[EdgeAnchor],
) -> Result<(Anchor, CaptureId), Refusal> {
    let id = uid::capture_id(src, file, anchors, &a.selectors())?;
    match id {
        CaptureId::Reuse(u) => {
            a.uid = u;
            if let Some(e) = anchors.iter().find(|x| x.uid == u) {
                a.captured = e.captured;
                a.pred = e.pred;
            }
        }
        CaptureId::New {
            uid,
            captured,
            pred,
        } => {
            a.uid = uid;
            a.captured = captured;
            a.pred = pred;
        }
    }
    Ok((a, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: Uid = Uid([2; 16]);
    const FILE: Uid = Uid([7; 16]);

    fn cap(form: Form, content: &[u8]) -> Anchor {
        capture(
            SRC,
            FILE,
            &form,
            Some(content),
            None,
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap()
        .0
    }

    fn res(a: &Anchor, content: &[u8]) -> AResult {
        resolve(a, Content::Bytes(content), Algo::Sha1, &Consts::DRAFT)
    }

    const SRC_TEXT: &str = "fn main() {\n    let x = compute();\n    println!(\"{x}\");\n}\n\nfn compute() -> u32 {\n    41 + 1\n}\n";

    #[test]
    fn a_quote_is_fresh_then_moved_then_edited() {
        let a = cap(Form::Lines(2, 2), SRC_TEXT.as_bytes());
        assert_eq!(a.kind, Kind::Quote);
        assert_eq!(a.quote, b"let x = compute();");
        assert_eq!(a.hint, Some((2, 2)));
        assert_eq!(res(&a, SRC_TEXT.as_bytes()).state, AState::Fresh);
        let moved = format!("// header\n\n{SRC_TEXT}");
        let r = res(&a, moved.as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((4, 4))));
        let edited = SRC_TEXT.replace("let x = compute();", "let x = compute2();");
        let r = res(&a, edited.as_bytes());
        assert_eq!((r.state, r.span), (AState::Edited, Some((2, 2))));
        let gone = SRC_TEXT.replace("let x = compute();", "return;");
        assert_eq!(res(&a, gone.as_bytes()).state, AState::Orphaned);
        assert_eq!(res(&a, b"\x00\x01binary").state, AState::Orphaned);
    }

    #[test]
    fn duplicate_quotes_widen_context_or_record_an_occurrence() {
        let text = "a();\nretry();\nb();\nretry();\nc();\n";
        let a = cap(Form::Lines(4, 4), text.as_bytes());
        assert_eq!(a.quote, b"retry();");
        // The context tells the two apart, so no occurrence is needed.
        assert_eq!(a.occurrence, None);
        let r = res(&a, text.as_bytes());
        assert_eq!((r.state, r.span), (AState::Fresh, Some((4, 4))));
        // Twenty identical lines around each copy: neither the widened context nor the window tells them apart.
        let f = "same line\n".repeat(20);
        let same = format!("{f}retry();\n{f}retry();\n{f}");
        let a = cap(Form::Lines(42, 42), same.as_bytes());
        assert_eq!(a.occurrence, Some(2));
        assert_eq!(a.prefix.len(), 64, "the ladder widened the context first");
        assert_eq!(res(&a, same.as_bytes()).state, AState::Fresh);
        let shifted = format!("new\n{same}");
        let r = res(&a, shifted.as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((43, 43))));
        let mut no_occ = a.clone();
        no_occ.occurrence = None;
        assert_eq!(res(&no_occ, shifted.as_bytes()).state, AState::Ambiguous);
    }

    #[test]
    fn a_short_range_has_overlapping_quotes_and_still_resolves() {
        // Five non-trivial lines make a range; the span is shorter than 64 bytes, so its start and end quotes are the
        // whole span text.
        let text = "head\na1;\nb2;\nc3;\nd4;\ne5;\ntail\n";
        let a = cap(Form::Lines(2, 6), text.as_bytes());
        assert_eq!(a.kind, Kind::Range);
        assert_eq!(a.quote, a.end);
        assert_eq!(res(&a, text.as_bytes()).state, AState::Fresh);
        let shifted = format!("x\ny\n{text}");
        let r = res(&a, shifted.as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((4, 8))));
        // A long range whose middle changed under `span` watch is `edited` at the located span.
        let long: String = (0..12)
            .map(|i| format!("line number {i} of the long range\n"))
            .collect();
        let a = cap(Form::Lines(1, 12), long.as_bytes());
        assert_eq!(a.kind, Kind::Range);
        let edited = long.replace("line number 6 of", "LINE 6 of");
        let r = res(&a, format!("pre\n{edited}").as_bytes());
        assert_eq!((r.state, r.span), (AState::Edited, Some((2, 13))));
    }

    #[test]
    fn trivial_spans_are_lines_anchors() {
        let text = "one line\n}\n\ntwo line\n";
        let a = cap(Form::Lines(2, 3), text.as_bytes());
        assert_eq!(a.kind, Kind::Lines);
        assert!(!a.window.is_empty());
        assert_eq!(res(&a, text.as_bytes()).state, AState::Fresh);
        let e = capture(
            SRC,
            FILE,
            &Form::Lines(1, 1),
            Some(b"}\n"),
            None,
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap_err();
        assert_eq!(e.get_str("case"), Some("no-window"));
    }

    #[test]
    fn refusals_follow_f20_6_1() {
        let e = |form: Form, b: &[u8]| {
            capture(
                SRC,
                FILE,
                &form,
                Some(b),
                None,
                None,
                Algo::Sha1,
                &[],
                &Consts::DRAFT,
            )
            .unwrap_err()
            .get_str("case")
            .unwrap()
            .to_string()
        };
        assert_eq!(e(Form::Lines(3, 9), b"a\nb\n"), "range");
        assert_eq!(e(Form::Lines(1, 1), b"\x00"), "binary");
        assert_eq!(e(Form::QuoteText("x\u{fffd}".into()), b"x\n"), "fffd");
        assert_eq!(e(Form::QuoteText(b"\n  \n".to_vec()), b"x\n"), "empty");
        assert_eq!(e(Form::QuoteText(b"nothere".to_vec()), b"x\n"), "not-found");
        assert_eq!(e(Form::Symbol("A/b".into()), b"x\n"), "no-scanner");
    }

    #[test]
    fn a_file_anchor_under_span_watch_is_a_content_pin() {
        let (a, _) = capture(
            SRC,
            FILE,
            &Form::File,
            Some(b"v1\n"),
            Some(Watch::Span),
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap();
        assert_eq!(res(&a, b"v1\r\n").state, AState::Fresh);
        assert_eq!(res(&a, b"v2\n").state, AState::Edited);
        assert_eq!(
            resolve(&a, Content::Bytes(b"v1\n"), Algo::Sha256, &Consts::DRAFT).state,
            AState::Unverified(61)
        );
    }

    #[test]
    fn capture_reuses_equal_selectors() {
        let (a, id) = capture(
            SRC,
            FILE,
            &Form::Lines(2, 2),
            Some(SRC_TEXT.as_bytes()),
            None,
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap();
        assert!(matches!(id, CaptureId::New { pred: None, .. }));
        let (_, id2) = capture(
            SRC,
            FILE,
            &Form::Lines(2, 2),
            Some(SRC_TEXT.as_bytes()),
            None,
            None,
            Algo::Sha1,
            &[a.edge_anchor()],
            &Consts::DRAFT,
        )
        .unwrap();
        assert_eq!(id2, CaptureId::Reuse(a.uid));
    }

    #[test]
    fn the_conservativeness_order() {
        let x = |state, span| AResult {
            state,
            span,
            score: None,
            details: Vec::new(),
        };
        let fresh = x(AState::Fresh, Some((2, 2)));
        let moved = x(AState::Moved, Some((2, 2)));
        let other = x(AState::Moved, Some((5, 5)));
        let edited = x(AState::Edited, Some((2, 2)));
        assert!(at_least_as_conservative(&fresh, &moved));
        assert!(at_least_as_conservative(&moved, &fresh));
        assert!(!at_least_as_conservative(&other, &moved));
        assert!(at_least_as_conservative(&edited, &moved));
        assert!(!at_least_as_conservative(&moved, &edited));
        assert!(at_least_as_conservative(
            &x(AState::Ambiguous, None),
            &moved
        ));
        assert!(at_least_as_conservative(
            &x(AState::Orphaned, None),
            &edited
        ));
        assert!(at_least_as_conservative(
            &x(AState::Unverified(53), None),
            &other
        ));
        assert!(!at_least_as_conservative(
            &moved,
            &x(AState::Orphaned, None)
        ));
    }

    #[test]
    fn a_range_over_repeated_lines_pairs_with_the_end_of_its_own_length() {
        // Ten identical lines, the end quote recurring inside the span: a 2-line insertion moves the range whole.
        let text = format!("head a\nhead b\n{}tail\n", "same line\n".repeat(10));
        let a = cap(Form::Lines(3, 12), text.as_bytes());
        assert_eq!(a.kind, Kind::Range);
        assert_eq!(res(&a, text.as_bytes()).state, AState::Fresh);
        let r = res(&a, format!("x\ny\n{text}").as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((5, 14))));
        // With no end hit at the captured length within the spread, a start pairs with its first end hit: a shortened
        // range is never `fresh` or `moved` (here several such pairs tie, so `ambiguous`).
        let short = format!("head a\nhead b\n{}tail\n", "same line\n".repeat(8));
        let r = res(&a, short.as_bytes());
        assert!(
            matches!(r.state, AState::Edited | AState::Ambiguous),
            "{r:?}"
        );
    }

    /// The fuzzy step pairs an exact end hit by the exact step's rule ([`pair_end`]): a range whose start quote was
    /// edited and whose end quote recurs inside the span still names the captured span's length, not the first end.
    #[test]
    fn a_fuzzy_range_pairs_an_exact_end_by_the_exact_rule() {
        let block = "repeat block line x number one\nrepeat block line y number two\nrepeat block line z number three\n";
        let text =
            format!("head\nalpha beta gamma delta epsilon zeta eta theta\n{block}{block}tail\n");
        let a = cap(Form::Lines(2, 8), text.as_bytes());
        assert_eq!((a.kind, a.hint), (Kind::Range, Some((2, 8))));
        let nt = NText::of(&atext(text.as_bytes()).unwrap());
        let ends: Vec<usize> = (0..=nt.n.len() - a.end.len())
            .filter(|&x| nt.n[x..].starts_with(&a.end))
            .map(|x| nt.line_of(x + a.end.len() - 1))
            .collect();
        assert_eq!(ends, [5, 8], "the end quote also ends inside the span");
        assert_eq!(res(&a, text.as_bytes()).state, AState::Fresh);
        let moved = format!("x\ny\n{text}");
        let r = res(&a, moved.as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((4, 10))));
        let edited = moved.replace("zeta eta", "ZETA eta");
        let r = res(&a, edited.as_bytes());
        assert_eq!((r.state, r.span), (AState::Edited, Some((4, 10))));
        assert!(r.score.is_some(), "the fuzzy step decided");
    }

    #[test]
    fn span_watch_edits_a_quote_whose_line_changed_outside_it() {
        let text = "fn a() {}\nlet total = compute(x) + offset;\nfn b() {}\n";
        let a = cap(Form::QuoteText(b"compute(x)".to_vec()), text.as_bytes());
        assert_eq!((a.kind, a.hint), (Kind::Quote, Some((2, 2))));
        assert_eq!(res(&a, text.as_bytes()).state, AState::Fresh);
        // The line changed outside the quote, at the hint: `edited` there.
        let at_hint = text.replace("+ offset", "- offset");
        let r = res(&a, at_hint.as_bytes());
        assert_eq!((r.state, r.span), (AState::Edited, Some((2, 2))));
        // Edited and moved: `edited` at the moved span.
        let moved = format!("// new\n{at_hint}");
        let r = res(&a, moved.as_bytes());
        assert_eq!((r.state, r.span), (AState::Edited, Some((3, 3))));
        // Moved only: `moved`.
        let r = res(&a, format!("// new\n{text}").as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((3, 3))));
        // Under header watch the span is not compared.
        let (h, _) = capture(
            SRC,
            FILE,
            &Form::QuoteText(b"compute(x)".to_vec()),
            Some(text.as_bytes()),
            Some(Watch::Header),
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap();
        let r = res(&h, format!("// new\n{at_hint}").as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((3, 3))));
    }

    /// I-F3 ([F18 §2.3]): capture never creates a second anchor with equal current selectors, also when the existing
    /// one is held without its text (a hash-only import): the selectors then compare by their digests.
    #[test]
    fn capture_reuses_a_text_unavailable_anchor_by_its_digests() {
        let a = cap(Form::Lines(2, 2), SRC_TEXT.as_bytes());
        let mut c = a.to_canon();
        c.text = None;
        let hashed = Anchor::from_canon(a.uid, &c);
        assert!(hashed.text_unavailable && hashed.quote.is_empty());
        let again = |form: Form| {
            capture(
                SRC,
                FILE,
                &form,
                Some(SRC_TEXT.as_bytes()),
                None,
                None,
                Algo::Sha1,
                &[hashed.edge_anchor()],
                &Consts::DRAFT,
            )
            .unwrap()
            .1
        };
        assert_eq!(again(Form::Lines(2, 2)), CaptureId::Reuse(a.uid));
        // Other selectors still make a new anchor, with no predecessor (its uid differs).
        assert!(matches!(
            again(Form::Lines(7, 7)),
            CaptureId::New { pred: None, .. }
        ));
    }

    /// One span rule for a `symbol` or `heading` anchor, whichever step locates its header quote: the item's hint
    /// length from the located header line.
    #[test]
    fn exact_and_fuzzy_name_one_span_for_a_symbol_anchor() {
        let text = "// lead\nfn alpha(x: u32) -> u32 {\n    let y = x + 1;\n    y * 2\n}\n";
        let nt = NText::of(&atext(text.as_bytes()).unwrap());
        // An imported `symbol` anchor of the item at lines 2–5, matched by its header quote (the interim scanner
        // rule, [F20 §6.1]).
        let mut a = cap(Form::Lines(2, 2), text.as_bytes());
        a.kind = Kind::Symbol;
        a.watch = Watch::Header;
        a.quote = header(&nt, 2, true).to_vec();
        a.hint = Some((2, 5));
        a.span_hash = Some(xxh3(header(&nt, 2, true)));
        assert_eq!(a.quote, b"fn alpha(x: u32) -> u32");
        let r = res(&a, text.as_bytes());
        assert_eq!((r.state, r.span), (AState::Fresh, Some((2, 5))));
        let moved = format!("// one\n// two\n{text}");
        let r = res(&a, moved.as_bytes());
        assert_eq!((r.state, r.span), (AState::Moved, Some((4, 7))));
        let edited = moved.replace("x: u32", "x: u64");
        let r = res(&a, edited.as_bytes());
        assert_eq!((r.state, r.span), (AState::Edited, Some((4, 7))));
    }

    /// `files.max-read-bytes` ([F20 §2.4] item 1): content beyond it is `Unavailable(size)`, so every anchor on it is
    /// `unverified (size)`.
    #[test]
    fn content_beyond_the_read_limit_leaves_every_anchor_unverified_size() {
        let c = Consts {
            max_read_bytes: Some(8),
            ..Consts::DRAFT
        };
        for form in [Form::Lines(2, 2), Form::File] {
            let a = cap(form, SRC_TEXT.as_bytes());
            assert_eq!(res(&a, SRC_TEXT.as_bytes()).state, AState::Fresh);
            assert_eq!(
                resolve(&a, Content::Bytes(SRC_TEXT.as_bytes()), Algo::Sha1, &c).state,
                AState::Unverified(58)
            );
        }
    }

    /// The largest start of a fuzzy candidate by one reverse dynamic programme equals the definition's scan.
    #[test]
    fn the_largest_start_agrees_with_the_scan_by_definition() {
        use proptest::prelude::*;
        let strat = (
            proptest::collection::vec(0u8..4, 1..12),
            proptest::collection::vec(0u8..4, 0..40),
            any::<usize>(),
        );
        crate::r4::tests::runner(256)
            .run(&strat, |(p, t, pick)| {
                let (p, t): (Vec<u8>, Vec<u8>) = (
                    p.iter().map(|x| b'a' + x).collect(),
                    t.iter().map(|x| b'a' + x).collect(),
                );
                let e = pick % (t.len() + 1);
                let d = distances(&p, &t, 0, t.len())[e];
                let k = p.len();
                let s_lo = e.saturating_sub(p.len() + k);
                let scan = (s_lo..=e).rev().find(|&s| lev(&p, &t[s..e]) == d);
                prop_assert_eq!(largest_start(&p, &t, s_lo, e, d), scan);
                Ok(())
            })
            .unwrap();
    }
}
