//! The normalised anchor text N, streamed ([F20 §2.5]; [40 §4.5] "the scan streams through the fixed buffer").
//!
//! The two-pass reader hands its [`LineSink`] the lines of the anchor text `atext(b)` in pieces ([`crate::text`]):
//! BOM and CR LF already removed, no `0A`. [`NStream`] turns them into `N(t) = nl(l1) ‖ 0A ‖ … ‖ 0A ‖ nl(lm)` for an
//! [`NSink`]: it drops each line's leading `WS`, holds its trailing `WS` until a later byte of the line shows it is
//! interior, and writes the `0A` that separates two lines when the second one starts. No pass holds N or a line.
//!
//! **Trailing whitespace.** A run of `WS` after a non-`WS` byte is interior when another non-`WS` byte of the line
//! follows and is dropped when the line ends. Up to [`HOLD`] bytes of a run are held; past that the sink is cloned
//! once (the state before the run), the run is fed to the sink, and at the line's end the clone is restored. So
//! memory is fixed — the hold buffer and at most one copy of the sink — and a sink sees exactly the bytes of N.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use super::Budget;
use crate::text::{LineSink, is_ws};

/// The whitespace held back before the sink is forked: one cache line's worth of runs covers indentation and
/// alignment, and a fork costs a clone of the sink once per longer run.
pub(crate) const HOLD: usize = 256;

/// A consumer of N ([F20 §2.5]): its bytes in order, with the lines' boundaries. Line numbers start at 1 and equal
/// those of the raw file. Every `0A` of N is a separator: [`NSink::bytes`] receives it after the line it ends and
/// before [`NSink::start_line`] of the next line. A sink must be [`Clone`] for the trailing-whitespace fork.
///
/// A sink's fixed part — its tail of N, its hash states — is charged to the budget before the pass; what grows with
/// the content it reports through [`NSink::retained`] and [`NSink::work`], which a [`Metered`] pass charges as it grows.
pub(crate) trait NSink: Clone {
    /// Line `line` starts at offset `at` of N.
    fn start_line(&mut self, line: u64, at: u64);
    /// Bytes of N at offset `at`.
    fn bytes(&mut self, at: u64, b: &[u8]);
    /// Line `line` ends at offset `at` of N (`end(line)` of [F20 §2.5]).
    fn end_line(&mut self, line: u64, at: u64);
    /// The bytes the sink holds now beyond its fixed part: queues of hits that wait, searches that run, a window of N
    /// that a decision still needs. Computed in constant time, or in the time of one callback.
    fn retained(&self) -> usize {
        0
    }
    /// The bytes of N the sink has searched again beyond the pass itself since it was built; never decreases.
    fn work(&self) -> u64 {
        0
    }
}

/// The sink that wants nothing.
impl NSink for () {
    fn start_line(&mut self, _line: u64, _at: u64) {}
    fn bytes(&mut self, _at: u64, _b: &[u8]) {}
    fn end_line(&mut self, _line: u64, _at: u64) {}
}

/// Two sinks fed the same N.
impl<A: NSink, B: NSink> NSink for (A, B) {
    fn start_line(&mut self, line: u64, at: u64) {
        self.0.start_line(line, at);
        self.1.start_line(line, at);
    }
    fn bytes(&mut self, at: u64, b: &[u8]) {
        self.0.bytes(at, b);
        self.1.bytes(at, b);
    }
    fn end_line(&mut self, line: u64, at: u64) {
        self.0.end_line(line, at);
        self.1.end_line(line, at);
    }
    fn retained(&self) -> usize {
        self.0.retained().saturating_add(self.1.retained())
    }
    fn work(&self) -> u64 {
        self.0.work().saturating_add(self.1.work())
    }
}

/// A sink that may be absent.
impl<A: NSink> NSink for Option<A> {
    fn start_line(&mut self, line: u64, at: u64) {
        if let Some(a) = self {
            a.start_line(line, at);
        }
    }
    fn bytes(&mut self, at: u64, b: &[u8]) {
        if let Some(a) = self {
            a.bytes(at, b);
        }
    }
    fn end_line(&mut self, line: u64, at: u64) {
        if let Some(a) = self {
            a.end_line(line, at);
        }
    }
    fn retained(&self) -> usize {
        self.as_ref().map_or(0, NSink::retained)
    }
    fn work(&self) -> u64 {
        self.as_ref().map_or(0, NSink::work)
    }
}

/// Several sinks of one kind.
impl<A: NSink> NSink for Vec<A> {
    fn start_line(&mut self, line: u64, at: u64) {
        for a in self {
            a.start_line(line, at);
        }
    }
    fn bytes(&mut self, at: u64, b: &[u8]) {
        for a in self {
            a.bytes(at, b);
        }
    }
    fn end_line(&mut self, line: u64, at: u64) {
        for a in self {
            a.end_line(line, at);
        }
    }
    fn retained(&self) -> usize {
        self.iter().fold(self.capacity() * size_of::<A>(), |n, a| {
            n.saturating_add(a.retained())
        })
    }
    fn work(&self) -> u64 {
        self.iter().fold(0, |n, a| n.saturating_add(a.work()))
    }
}

/// The budget a pass charges the growth of its sinks to ([F20 §1.3] input (e)): the bytes they hold beyond their fixed
/// part, at their peak, and the bytes of N they search again. Once a charge is refused the meter stays exhausted and
/// the pass is `Unavailable(budget)` ([F20 §1.5]), which a budget may always make of an answer ([40 §4.1] P7).
pub(crate) struct Meter<'b> {
    budget: RefCell<&'b mut dyn Budget>,
    out: Cell<bool>,
}

impl<'b> Meter<'b> {
    pub(crate) fn new(budget: &'b mut dyn Budget) -> Meter<'b> {
        Meter {
            budget: RefCell::new(budget),
            out: Cell::new(false),
        }
    }

    /// Spends `units`; `false`, now and from then on, once the budget refuses.
    // spec: [F20 §1.3] input (e) (a budget that runs out turns the answer into `unverified (budget)`)
    fn spend(&self, units: u64) -> bool {
        if self.out.get() {
            return false;
        }
        if units > 0 && !self.budget.borrow_mut().spend(units) {
            self.out.set(true);
            return false;
        }
        true
    }

    /// Whether a charge was refused.
    pub(crate) fn exhausted(&self) -> bool {
        self.out.get()
    }
}

/// The bytes of N a [`Metered`] sink receives between two charges.
const METER_SLICE: usize = 256;

/// A sink whose growth is charged to a [`Meter`] after every callback, the bytes of N handed on in slices of
/// [`METER_SLICE`], so that a charge trails the growth by at most one slice's hits (and one doubling of a queue). A
/// copy — [`NStream`]'s fork before a long whitespace run — is charged what the sink holds before it is made. Once the
/// meter is exhausted the sink is dropped and receives nothing more.
pub(crate) struct Metered<'m, 'b, K> {
    inner: Option<K>,
    meter: &'m Meter<'b>,
    /// The peak of [`NSink::retained`] charged so far.
    peak: usize,
    /// The [`NSink::work`] charged so far.
    worked: u64,
}

impl<'m, 'b, K: NSink> Metered<'m, 'b, K> {
    pub(crate) fn new(inner: K, meter: &'m Meter<'b>) -> Metered<'m, 'b, K> {
        Metered {
            inner: Some(inner),
            meter,
            peak: 0,
            worked: 0,
        }
    }

    /// The sink, or `None` when the meter ran out.
    pub(crate) fn into_inner(self) -> Option<K> {
        if self.meter.exhausted() {
            None
        } else {
            self.inner
        }
    }

    /// Charges the growth since the last charge; drops the sink when the meter refuses, or ran out for another copy.
    // spec: [F20 §1.3] input (e) (what a pass holds and searches is charged as it grows)
    fn charge(&mut self) {
        let Some(k) = &self.inner else {
            return;
        };
        let held = k.retained();
        let work = k.work();
        let units = (held.saturating_sub(self.peak) as u64)
            .saturating_add(work.saturating_sub(self.worked));
        if self.meter.spend(units) {
            self.peak = self.peak.max(held);
            self.worked = self.worked.max(work);
        } else {
            self.inner = None;
        }
    }
}

impl<K: NSink> Clone for Metered<'_, '_, K> {
    // spec: [F20 §1.3] input (e) (a copy of what a sink holds is held too)
    fn clone(&self) -> Self {
        let held = self.inner.as_ref().map_or(0, NSink::retained);
        let inner = if self.meter.spend(held as u64) {
            self.inner.clone()
        } else {
            None
        };
        Metered {
            inner,
            meter: self.meter,
            peak: self.peak,
            worked: self.worked,
        }
    }
}

impl<K: NSink> NSink for Metered<'_, '_, K> {
    fn start_line(&mut self, line: u64, at: u64) {
        if let Some(k) = &mut self.inner {
            k.start_line(line, at);
            self.charge();
        }
    }

    fn bytes(&mut self, at: u64, b: &[u8]) {
        let mut at = at;
        for part in b.chunks(METER_SLICE) {
            let Some(k) = &mut self.inner else {
                return;
            };
            k.bytes(at, part);
            self.charge();
            at += part.len() as u64;
        }
    }

    fn end_line(&mut self, line: u64, at: u64) {
        if let Some(k) = &mut self.inner {
            k.end_line(line, at);
            self.charge();
        }
    }
}

/// Two line sinks fed the same lines: the N stream and the scope scanner's raw text.
pub(crate) struct Both<'x, A, B>(pub(crate) &'x mut A, pub(crate) &'x mut B);

impl<A: LineSink, B: LineSink> LineSink for Both<'_, A, B> {
    fn begin(&mut self) {
        self.0.begin();
        self.1.begin();
    }
    fn piece(&mut self, bytes: &[u8]) {
        self.0.piece(bytes);
        self.1.piece(bytes);
    }
    fn end_line(&mut self) {
        self.0.end_line();
        self.1.end_line();
    }
}

/// The [`LineSink`] that feeds N to an [`NSink`] ([F20 §2.5] `nl`, `N(t)`).
#[derive(Clone, Debug)]
pub(crate) struct NStream<S> {
    init: S,
    sink: S,
    line: u64,
    pos: u64,
    in_line: bool,
    seen: bool,
    hold: [u8; HOLD],
    held: usize,
    snap: Option<(S, u64)>,
}

impl<S: NSink> NStream<S> {
    pub(crate) fn new(sink: S) -> NStream<S> {
        NStream {
            init: sink.clone(),
            sink,
            line: 0,
            pos: 0,
            in_line: false,
            seen: false,
            hold: [0; HOLD],
            held: 0,
            snap: None,
        }
    }

    /// The number of lines of t seen.
    pub(crate) fn lines(&self) -> u64 {
        self.line
    }

    /// `len(N)` so far.
    pub(crate) fn len(&self) -> u64 {
        self.pos
    }

    pub(crate) fn into_sink(self) -> S {
        self.sink
    }

    // spec: [F20 §2.5] (the `0A` between two lines of N)
    fn open(&mut self) {
        if self.line > 0 {
            self.sink.bytes(self.pos, b"\n");
            self.pos += 1;
        }
        self.line += 1;
        self.sink.start_line(self.line, self.pos);
        self.in_line = true;
        self.seen = false;
        self.held = 0;
    }

    // spec: [F20 §2.5] `nl` (leading WS dropped; trailing WS held, interior WS kept)
    fn ws(&mut self, run: &[u8]) {
        if !self.seen {
            return;
        }
        if self.snap.is_some() {
            self.sink.bytes(self.pos, run);
            self.pos += run.len() as u64;
        } else if self.held + run.len() <= HOLD {
            self.hold[self.held..self.held + run.len()].copy_from_slice(run);
            self.held += run.len();
        } else {
            self.snap = Some((self.sink.clone(), self.pos));
            let held = self.held;
            self.sink.bytes(self.pos, &self.hold[..held]);
            self.pos += held as u64;
            self.held = 0;
            self.sink.bytes(self.pos, run);
            self.pos += run.len() as u64;
        }
    }

    // spec: [F20 §2.5] `nl` (whitespace before a byte of the line is interior)
    fn word(&mut self, run: &[u8]) {
        if self.snap.take().is_none() && self.held > 0 {
            let held = self.held;
            self.sink.bytes(self.pos, &self.hold[..held]);
            self.pos += held as u64;
        }
        self.held = 0;
        self.sink.bytes(self.pos, run);
        self.pos += run.len() as u64;
        self.seen = true;
    }
}

impl<S: NSink> LineSink for NStream<S> {
    fn begin(&mut self) {
        self.sink = self.init.clone();
        self.line = 0;
        self.pos = 0;
        self.in_line = false;
        self.seen = false;
        self.held = 0;
        self.snap = None;
    }

    // spec: [F20 §2.5] `nl`, `N(t)` (each line trimmed, the lines joined by `0A`)
    fn piece(&mut self, bytes: &[u8]) {
        if !self.in_line {
            self.open();
        }
        // Up to the piece's last non-`WS` byte everything is N: whitespace before it is interior. A run that starts
        // the piece continues the run held before it; one that ends the piece is held.
        let Some(z) = bytes.iter().rposition(|&b| !is_ws(b)) else {
            self.ws(bytes);
            return;
        };
        let a = bytes.iter().position(|&b| !is_ws(b)).unwrap_or(z);
        if a > 0 {
            self.ws(&bytes[..a]);
        }
        self.word(&bytes[a..=z]);
        if z + 1 < bytes.len() {
            self.ws(&bytes[z + 1..]);
        }
    }

    // spec: [F20 §2.5] `nl` (trailing WS dropped), `end(i)`
    fn end_line(&mut self) {
        if !self.in_line {
            self.open();
        }
        if let Some((s, p)) = self.snap.take() {
            self.sink = s;
            self.pos = p;
        }
        self.held = 0;
        self.sink.end_line(self.line, self.pos);
        self.in_line = false;
    }
}

/// The last bytes of N, in a ring of fixed capacity, for the context and the location of a hit behind the stream.
#[derive(Clone, Debug)]
pub(crate) struct Tail {
    buf: Box<[u8]>,
    head: u64,
}

impl Tail {
    pub(crate) fn new(cap: usize) -> Tail {
        Tail {
            buf: vec![0; cap.max(1)].into_boxed_slice(),
            head: 0,
        }
    }

    /// Appends the bytes at offset `at`, which must be the end of what was pushed.
    pub(crate) fn push(&mut self, at: u64, b: &[u8]) {
        debug_assert_eq!(at, self.head, "N arrives in order");
        let cap = self.buf.len();
        let b = if b.len() > cap {
            self.head += (b.len() - cap) as u64;
            &b[b.len() - cap..]
        } else {
            b
        };
        let mut i = (self.head % cap as u64) as usize;
        for &x in b {
            self.buf[i] = x;
            i += 1;
            if i == cap {
                i = 0;
            }
        }
        self.head += b.len() as u64;
    }

    /// The offset of the oldest byte held.
    pub(crate) fn start(&self) -> u64 {
        self.head.saturating_sub(self.buf.len() as u64)
    }

    /// The offset after the newest byte.
    pub(crate) fn end(&self) -> u64 {
        self.head
    }

    /// The byte at offset `p`, which must be held (`start() ≤ p < end()`).
    pub(crate) fn byte(&self, p: u64) -> u8 {
        debug_assert!(p >= self.start() && p < self.head);
        self.buf[(p % self.buf.len() as u64) as usize]
    }

    /// Copies `N[from .. to)` into `out` (replacing its contents). `from` is clamped to the oldest byte held and `to`
    /// to the newest; returns the offset the copy starts at.
    pub(crate) fn copy(&self, from: u64, to: u64, out: &mut Vec<u8>) -> u64 {
        out.clear();
        let from = from.max(self.start());
        let to = to.min(self.head);
        let cap = self.buf.len() as u64;
        let mut p = from;
        while p < to {
            out.push(self.buf[(p % cap) as usize]);
            p += 1;
        }
        from
    }
}

/// The lines of the bytes behind the stream: `line_at(o)`, the line of N that holds offset o, an offset on a
/// separating `0A` belonging to the line before it ([F20 §2.5]).
#[derive(Clone, Debug, Default)]
pub(crate) struct LineMap {
    starts: VecDeque<(u64, u64)>,
}

impl LineMap {
    pub(crate) fn start_line(&mut self, line: u64, at: u64) {
        self.starts.push_back((line, at));
    }

    /// Forgets lines that end before offset `min`, keeping the line that holds it.
    pub(crate) fn prune(&mut self, min: u64) {
        while self.starts.len() > 1 && self.starts[1].1 <= min {
            self.starts.pop_front();
        }
    }

    /// `start(line)`, when that line is kept.
    pub(crate) fn start_of(&self, line: u64) -> Option<u64> {
        let k = self.starts.partition_point(|&(l, _)| l < line);
        self.starts
            .get(k)
            .filter(|&&(l, _)| l == line)
            .map(|&(_, s)| s)
    }

    /// The line that holds offset `o`; 1 for an offset before every line kept (the empty text).
    pub(crate) fn line_at(&self, o: u64) -> u64 {
        let k = self.starts.partition_point(|&(_, s)| s <= o);
        if k == 0 {
            self.starts.front().map_or(1, |&(l, _)| l)
        } else {
            self.starts[k - 1].0
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::oid::ObjectFormat;
    use crate::text::{NormalisedText, analyse, atext};

    /// A sink that records N and the line events.
    #[derive(Clone, Debug, Default)]
    struct Rec {
        n: Vec<u8>,
        starts: Vec<(u64, u64)>,
        ends: Vec<(u64, u64)>,
    }

    impl NSink for Rec {
        fn start_line(&mut self, line: u64, at: u64) {
            assert_eq!(at, self.n.len() as u64);
            self.starts.push((line, at));
        }
        fn bytes(&mut self, at: u64, b: &[u8]) {
            assert_eq!(at, self.n.len() as u64);
            self.n.extend_from_slice(b);
        }
        fn end_line(&mut self, line: u64, at: u64) {
            assert_eq!(at, self.n.len() as u64);
            self.ends.push((line, at));
        }
    }

    fn stream(b: &[u8]) -> (Rec, u64) {
        let mut s = NStream::new(Rec::default());
        let _ = analyse(b, ObjectFormat::Sha1, None, &mut s);
        let lines = s.lines();
        (s.into_sink(), lines)
    }

    #[test]
    fn n_of_small_texts() {
        assert_eq!(stream(b"").0.n, b"");
        assert_eq!(stream(b"  a  b  \r\n\t\n x\n").0.n, b"a  b\n\nx");
        assert_eq!(stream(b"\xEF\xBB\xBF  a\n").0.n, b"a");
        assert_eq!(stream(b"a\n\n").0.n, b"a\n");
        let (r, lines) = stream(b"a\n\nb");
        assert_eq!(lines, 3);
        assert_eq!(r.starts, [(1, 0), (2, 2), (3, 3)]);
        assert_eq!(r.ends, [(1, 1), (2, 2), (3, 4)]);
        // A run longer than the hold: interior, then trailing.
        let mut t = b"x".to_vec();
        t.extend(std::iter::repeat_n(b' ', 3 * HOLD));
        t.push(b'y');
        t.extend(std::iter::repeat_n(b'\t', 2 * HOLD));
        t.extend_from_slice(b"\nz");
        let mut want = b"x".to_vec();
        want.extend(std::iter::repeat_n(b' ', 3 * HOLD));
        want.extend_from_slice(b"y\nz");
        assert_eq!(stream(&t).0.n, want);
    }

    /// A sink that keeps every byte of N, so what it holds grows with the content.
    #[derive(Clone, Debug, Default)]
    struct Hoard(Vec<u8>);

    impl NSink for Hoard {
        fn start_line(&mut self, _line: u64, _at: u64) {}
        fn bytes(&mut self, _at: u64, b: &[u8]) {
            self.0.extend_from_slice(b);
        }
        fn end_line(&mut self, _line: u64, _at: u64) {}
        fn retained(&self) -> usize {
            self.0.capacity()
        }
        fn work(&self) -> u64 {
            self.0.len() as u64
        }
    }

    /// A budget of so many units.
    struct Units(u64);

    impl Budget for Units {
        fn spend(&mut self, units: u64) -> bool {
            if units > self.0 {
                return false;
            }
            self.0 -= units;
            true
        }
    }

    /// Streams `b` into a [`Hoard`] metered by a budget of `units`: the sink, if the budget held, and the units spent.
    fn hoard(b: &[u8], units: u64) -> (Option<Hoard>, u64) {
        let mut budget = Units(units);
        let meter = Meter::new(&mut budget);
        let mut s = NStream::new(Metered::new(Hoard::default(), &meter));
        let _ = analyse(b, ObjectFormat::Sha1, None, &mut s);
        let sink = s.into_sink().into_inner();
        assert_eq!(sink.is_none(), meter.exhausted());
        (sink, units - budget.0)
    }

    #[test]
    fn growth_is_charged_as_it_grows() {
        let b = b"ab  cd\n".repeat(1000);
        let n = NormalisedText::new(&atext(&b).unwrap())
            .unwrap()
            .bytes()
            .to_vec();
        // Enough: the sink is whole, and the peak it held and the bytes it searched were charged once each.
        let (sink, spent) = hoard(&b, 1 << 20);
        let sink = sink.unwrap();
        assert_eq!(sink.0, n);
        assert_eq!(spent, (sink.0.capacity() + sink.0.len()) as u64);
        // Too little: the sink is dropped and the pass is over budget.
        let (sink, _) = hoard(&b, 1000);
        assert!(sink.is_none());
        // The fork before a trailing whitespace run longer than the hold copies the sink: it is charged what the sink
        // holds then (at least the 5,000 bytes before the run), over the growth of the same N without the run.
        let with_run = |run: usize| {
            let mut b = b"x".repeat(5000);
            b.extend(std::iter::repeat_n(b' ', run));
            b.extend_from_slice(
                b"
y",
            );
            hoard(&b, 1 << 20)
        };
        let ((forked, a), (plain, b)) = (with_run(2 * HOLD), with_run(0));
        assert_eq!(forked.unwrap().0, plain.unwrap().0);
        assert!(a >= b + 5000, "{a} {b}");
    }

    #[test]
    fn tail_and_line_map() {
        let mut t = Tail::new(4);
        t.push(0, b"abcdef");
        assert_eq!((t.start(), t.end()), (2, 6));
        let mut out = Vec::new();
        assert_eq!(t.copy(0, 6, &mut out), 2);
        assert_eq!(out, b"cdef");
        t.push(6, b"g");
        assert_eq!(t.copy(2, 7, &mut out), 3);
        assert_eq!(out, b"defg");
        let mut m = LineMap::default();
        m.start_line(1, 0);
        m.start_line(2, 3);
        m.start_line(3, 4);
        assert_eq!(m.line_at(0), 1);
        assert_eq!(m.line_at(2), 1);
        assert_eq!(m.line_at(3), 2);
        assert_eq!(m.line_at(9), 3);
        m.prune(3);
        assert_eq!(m.line_at(3), 2);
    }

    /// Feeds the lines of `t` to an [`NStream`] as [`crate::text`]'s line scan would, each line cut into pieces at
    /// the offsets `cuts` gives (taken modulo the line length).
    fn stream_pieces(t: &[u8], cuts: &[usize]) -> (Rec, u64) {
        let mut s = NStream::new(Rec::default());
        s.begin();
        let mut k = 0;
        for l in crate::text::lines(t) {
            let mut rest = l;
            while !rest.is_empty() {
                let c = 1 + cuts.get(k).copied().unwrap_or(rest.len()) % rest.len();
                k += 1;
                s.piece(&rest[..c]);
                rest = &rest[c..];
            }
            s.end_line();
        }
        let lines = s.lines();
        (s.into_sink(), lines)
    }

    proptest! {
        /// N streamed in any piece split equals `N(atext(b))`, with the line offsets of [`NormalisedText`].
        #[test]
        fn streamed_n_equals_the_definition(
            b in proptest::collection::vec(prop_oneof![Just(b' '), Just(b'\t'), Just(b'\r'), Just(b'\n'),
                                                       Just(b'a'), Just(b'}'), Just(0xEF), Just(0xBB),
                                                       Just(0xBF), Just(0x0C)], 0..600),
            long in 0usize..3,
            cuts in proptest::collection::vec(0usize..400, 0..64),
        ) {
            let mut b = b;
            if long > 0 {
                let at = b.len() / 2;
                b.splice(at..at, std::iter::repeat_n(b' ', long * HOLD + 7));
            }
            let Some(t) = atext(&b) else { return Ok(()) };
            let want = NormalisedText::new(&t).unwrap();
            for (r, lines) in [stream(&b), stream_pieces(&t, &cuts)] {
                prop_assert_eq!(&r.n[..], want.bytes());
                prop_assert_eq!(lines as usize, want.line_count());
                for (i, &(l, at)) in r.starts.iter().enumerate() {
                    prop_assert_eq!(l as usize, i + 1);
                    prop_assert_eq!(Some(at as usize), want.start(i + 1));
                }
                for (i, &(_, at)) in r.ends.iter().enumerate() {
                    prop_assert_eq!(Some(at as usize), want.end(i + 1));
                }
            }
        }
    }
}
