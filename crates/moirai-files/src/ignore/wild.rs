//! git's `wildmatch`: the glob matcher behind every gitignore pattern that is not a plain name, a plain path or a
//! `*` followed by a plain suffix ([F20 §4.4]: "the matcher has git's semantics").
//!
//! This follows the behaviour of git's `dowild` (semantics only; no git code is used), every rule included, because
//! WP-74's differential compares this matcher with `git check-ignore`:
//!
//! - **Case folding** (`core.ignorecase`) lowers ASCII upper-case letters of the text and of plain pattern bytes. A
//!   byte after `\` and a byte inside a bracket expression is not lowered, so under folding `\A` and `[A]` match
//!   neither `a` nor `A`; a range also tests the upper-case form of a lower-case text byte, and `[:upper:]` also
//!   accepts a lower-case byte. Bytes `80`–`FF` never fold.
//! - **Character classes** use git's own ASCII tables: `[:space:]` is `09`, `0A`, `0D` and `20` (not `0B` or `0C`),
//!   `[:blank:]` is `09` and `20`; an unknown class name makes the whole match fail, as does an unterminated `[`.
//! - **`**`** (under WM_PATHNAME) matches across `/` only when it fills a whole component of the whole pattern: it
//!   starts the pattern or follows `/`, and ends the pattern or precedes `/` or `\/`. Elsewhere it is `*`, also when a
//!   caller has already compared a literal prefix before it (git's own matcher behaves so: `a**/b` does not match
//!   `a/x/b`). `**/` also matches no directory at all. Without WM_PATHNAME (basename patterns) every `*` run, `**`
//!   included, matches any bytes, `/` too, and has no component meaning (git's plain pathspecs, which use this mode
//!   on whole paths, show it: `x**y` matches `x/y`, and `**/c` does not match `c`).
//! - **Aborts.** A call stops the whole match when the text runs out before the pattern does, when a literal after a
//!   `*` is absent from the rest of the text, or on a malformed bracket expression; a `*` that cannot cross `/` stops
//!   the enclosing `*` runs back to the nearest `**`. These only cut branches that cannot match: the result is
//!   whether some choice of runs for the `*`s matches the whole text (the tests check this against a naive port of
//!   `dowild`).
//!
//! # Bounded work
//!
//! git's matcher recurses once per `*` run. Its time is exponential in the number of nested `**/` units, and a long
//! enough line overflows its stack. Three changes of form keep its results and bound the cost:
//!
//! - the recursion is an explicit stack ([`Frames`]), so no pattern deepens the call stack;
//! - consecutive whole-component `**/` units match as one: "zero or more directories" twice is "zero or more
//!   directories" (the second unit's matches from a position are a subset of the first's), so a nested call consumes
//!   text before the chain of pending calls grows by more than a few frames, and the chain is bounded by the text;
//! - once a match has taken more than [`budget`] steps, it starts again as a simulation ([`simulate`]): the set of
//!   pattern positions that the text read so far can reach, advanced one text byte at a time (Thompson's
//!   construction over the pattern as git reads it). It takes at most about |pattern| × |text| steps and holds two
//!   bit sets of 2 × (|pattern| + 1) bits, whatever the input, and it decides the same thing: whether some choice
//!   of runs matches.
//!
//! The pattern never contains `00` (the parser cuts a line at its first `00`, as git's string handling does). A `00`
//! in the text is an ordinary byte; a path never holds one ([OS/path]).

/// The match flags of one call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WildFlags {
    /// git's `WM_PATHNAME`: `*`, `?` and bracket expressions never match `/`, and `**` has its component meaning.
    pub(crate) pathname: bool,
    /// git's `WM_CASEFOLD` (`core.ignorecase`).
    pub(crate) casefold: bool,
}

// The result codes of one call: a match, a mismatch that lets an enclosing `*` try a longer run, and the two aborts.
const MATCH: i8 = 0;
const NOMATCH: i8 = 1;
const ABORT_ALL: i8 = -1;
const ABORT_TO_STARSTAR: i8 = -2;

/// A suspended call, waiting for the result of the call it made.
#[derive(Clone, Copy, Debug)]
enum Frame {
    /// The `**/` probe: the call tried the pattern after the `/` (at `p + 1`) against the text at `t`, to let `**/`
    /// match no directory. On a match the call matches; otherwise its `*` loop runs from `t` over `p` with `**`'s
    /// meaning.
    Probe { p: usize, t: usize },
    /// The `*` loop over pattern position `p`, which has called the pattern from `p` against the text from `t`.
    Loop {
        p: usize,
        t: usize,
        match_slash: bool,
    },
}

/// Frames held without an allocation: one per `*` run in the deepest pending chain.
const INLINE: usize = 8;

/// The frames of the explicit call stack: the first [`INLINE`] in place, the rest on the heap, so an ordinary pattern
/// allocates nothing.
struct Frames {
    inline: [Frame; INLINE],
    len: usize,
    spill: Vec<Frame>,
}

impl Frames {
    fn new() -> Frames {
        Frames {
            inline: [Frame::Probe { p: 0, t: 0 }; INLINE],
            len: 0,
            spill: Vec::new(),
        }
    }

    fn push(&mut self, f: Frame) {
        if self.len < INLINE {
            self.inline[self.len] = f;
        } else {
            self.spill.push(f);
        }
        self.len += 1;
    }

    fn pop(&mut self) -> Option<Frame> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        if self.len < INLINE {
            Some(self.inline[self.len])
        } else {
            self.spill.pop()
        }
    }
}

/// What a piece of a call produced: its result, or a nested call to make (and the frame that resumes this call).
enum Step {
    Ret(i8),
    Call { p: usize, t: usize, resume: Frame },
}

/// The steps a match takes before it switches to [`simulate`]: linear in the input, more than any pattern without
/// nested `*` runs needs.
const fn budget(pat_len: usize, text_len: usize) -> usize {
    64 + 4 * (pat_len + text_len)
}

/// git's glob special bytes: `*`, `?`, `[`, `\` (its `is_glob_special`).
#[inline]
pub(crate) const fn is_glob_special(c: u8) -> bool {
    matches!(c, b'*' | b'?' | b'[' | b'\\')
}

/// The length of the leading run of `s` that holds no glob special byte (git's `simple_length`).
#[inline]
pub(crate) fn simple_length(s: &[u8]) -> usize {
    s.iter()
        .position(|&c| is_glob_special(c))
        .unwrap_or(s.len())
}

#[inline]
const fn fold(c: u8, casefold: bool) -> u8 {
    if casefold { c.to_ascii_lowercase() } else { c }
}

/// The pattern byte at `i`, or `00` past its end (git reads the terminating NUL there).
#[inline]
fn at(pat: &[u8], i: usize) -> u8 {
    pat.get(i).copied().unwrap_or(0)
}

/// What a run of `*` means.
#[derive(Clone, Copy, Debug)]
struct StarRun {
    /// The position after the run.
    end: usize,
    /// The run may match `/`.
    match_slash: bool,
    /// The run is a whole-component `**` followed by `/`: the `**/` unit, which also matches no directory.
    dir_unit: bool,
}

/// The `*` run that starts at `p` (the run counts from `p` on; the byte before `p` decides whether a `**` starts a
/// component).
// spec: [F20 §4.4] (git's semantics: `*` never crosses `/` under WM_PATHNAME; `**` as a whole component)
fn star_run(pat: &[u8], p: usize, pathname: bool) -> StarRun {
    let mut end = p;
    while at(pat, end) == b'*' {
        end += 1;
    }
    if !pathname {
        // Without WM_PATHNAME every run matches `/`, and `**` is `*`.
        return StarRun {
            end,
            match_slash: true,
            dir_unit: false,
        };
    }
    if end - p < 2 {
        return StarRun {
            end,
            match_slash: false,
            dir_unit: false,
        };
    }
    let next = at(pat, end);
    let at_end = end >= pat.len();
    let whole = (p == 0 || pat[p - 1] == b'/')
        && (at_end || next == b'/' || (next == b'\\' && at(pat, end + 1) == b'/'));
    StarRun {
        end,
        match_slash: whole,
        dir_unit: whole && !at_end && next == b'/',
    }
}

/// True iff `pat[start..]` matches the whole of `text` under `flags`, as git's `wildmatch` decides it. The bytes
/// before `start` are a literal prefix the caller has already compared; they count only as the bytes before a `**`.
// spec: [F20 §4.4] (git's semantics: wildmatch, with WM_PATHNAME for patterns that hold a `/`)
pub(crate) fn wildmatch(pat: &[u8], start: usize, text: &[u8], flags: WildFlags) -> bool {
    run(pat, start, text, flags, budget(pat.len(), text.len()))
}

/// [`wildmatch`] by git's backtracking, switching to [`simulate`] after `limit` steps.
fn run(pat: &[u8], start: usize, text: &[u8], flags: WildFlags, limit: usize) -> bool {
    debug_assert!(!pat.contains(&0), "a pattern holds no 00 byte");
    let m = Machine {
        pat,
        text,
        f: flags,
    };
    let mut work = 0usize;
    let mut frames = Frames::new();
    let mut step = m.body(start.min(pat.len()), 0);
    loop {
        work += 1;
        if work > limit {
            return simulate(pat, start, text, flags);
        }
        match step {
            Step::Call { p, t, resume } => {
                frames.push(resume);
                step = m.body(p, t);
            }
            Step::Ret(r) => match frames.pop() {
                None => return r == MATCH,
                Some(frame) => step = m.resume(frame, r),
            },
        }
    }
}

/// The inputs of one match.
struct Machine<'a> {
    pat: &'a [u8],
    text: &'a [u8],
    f: WildFlags,
}

impl Machine<'_> {
    /// Continues a suspended call with the result `r` of the call it made.
    fn resume(&self, frame: Frame, r: i8) -> Step {
        match frame {
            Frame::Probe { p, t } => {
                if r == MATCH {
                    Step::Ret(MATCH)
                } else {
                    self.star_loop(p, t, true)
                }
            }
            Frame::Loop { p, t, match_slash } => {
                if r != NOMATCH {
                    if !match_slash || r != ABORT_TO_STARSTAR {
                        return Step::Ret(r);
                    }
                } else if !match_slash && self.text.get(t) == Some(&b'/') {
                    return Step::Ret(ABORT_TO_STARSTAR);
                }
                self.star_loop(p, t + 1, match_slash)
            }
        }
    }

    /// One call from pattern position `p` and text position `t`, up to its result or its first nested call.
    // spec: [F20 §4.4] (git's semantics: literals, `\`, `?`, bracket expressions, `*` and `**`, case folding)
    fn body(&self, mut p: usize, mut t: usize) -> Step {
        let (pat, text, f) = (self.pat, self.text, self.f);
        loop {
            let Some(&pc_raw) = pat.get(p) else {
                return Step::Ret(if t < text.len() { NOMATCH } else { MATCH });
            };
            let tc_opt = text.get(t).copied();
            if tc_opt.is_none() && pc_raw != b'*' {
                return Step::Ret(ABORT_ALL);
            }
            let tc = fold(tc_opt.unwrap_or(0), f.casefold);
            let pc = fold(pc_raw, f.casefold);
            match pc {
                b'\\' => {
                    // A literal match with the next byte, which is not folded.
                    p += 1;
                    if p >= pat.len() || pat[p] != tc {
                        return Step::Ret(NOMATCH);
                    }
                }
                b'?' => {
                    if f.pathname && tc == b'/' {
                        return Step::Ret(NOMATCH);
                    }
                }
                b'*' => {
                    let run = star_run(pat, p, f.pathname);
                    p = run.end;
                    if run.dir_unit {
                        // Whole-component `**/` units that follow match as this one does.
                        loop {
                            let next = star_run(pat, p + 1, f.pathname);
                            if !next.dir_unit {
                                break;
                            }
                            p = next.end;
                        }
                        // `**/` may match no directory: try the rest after the `/` here first.
                        return Step::Call {
                            p: p + 1,
                            t,
                            resume: Frame::Probe { p, t },
                        };
                    }
                    let match_slash = run.match_slash;
                    if p >= pat.len() {
                        // A trailing `**` matches everything; a trailing `*` only a rest without `/`.
                        if !match_slash && text[t..].contains(&b'/') {
                            return Step::Ret(NOMATCH);
                        }
                        return Step::Ret(MATCH);
                    }
                    if !match_slash && pat[p] == b'/' {
                        // One `*` before a `/` matches up to the next `/` of the text, which the `/` then consumes.
                        let Some(i) = text[t..].iter().position(|&c| c == b'/') else {
                            return Step::Ret(NOMATCH);
                        };
                        t += i;
                    } else {
                        return self.star_loop(p, t, match_slash);
                    }
                }
                b'[' => {
                    if let Some(r) = class(pat, &mut p, tc, f) {
                        return Step::Ret(r);
                    }
                }
                _ => {
                    if tc != pc {
                        return Step::Ret(NOMATCH);
                    }
                }
            }
            p += 1;
            t += 1;
        }
    }

    /// The top of the `*` loop over pattern position `p` (not the pattern's end): the run of the `*` ends at text
    /// position `t`.
    // spec: [F20 §4.4] (git's semantics: the runs a `*` tries, and git's aborts)
    fn star_loop(&self, p: usize, mut t: usize, match_slash: bool) -> Step {
        let (pat, text, f) = (self.pat, self.text, self.f);
        if t >= text.len() {
            return Step::Ret(ABORT_ALL);
        }
        let lead = pat[p];
        if !is_glob_special(lead) {
            // A literal follows the `*`: skip to its next occurrence (never past a `/` when the run cannot hold one).
            let pc = fold(lead, f.casefold);
            let mut found = false;
            while let Some(&c) = text.get(t) {
                if !match_slash && c == b'/' {
                    break;
                }
                if fold(c, f.casefold) == pc {
                    found = true;
                    break;
                }
                t += 1;
            }
            if !found {
                return Step::Ret(if match_slash {
                    ABORT_ALL
                } else {
                    ABORT_TO_STARSTAR
                });
            }
        }
        Step::Call {
            p,
            t,
            resume: Frame::Loop { p, t, match_slash },
        }
    }
}

/// Where one state of [`simulate`] leads on one text byte.
enum Advance {
    /// To this state.
    To(usize),
    /// Nowhere: this branch fails.
    Stop,
    /// A malformed bracket expression: the whole match fails (every branch must cross it).
    Abort,
}

/// [`wildmatch`] as a simulation: after each text byte, the set of states that some choice of runs reaches, over the
/// pattern as git reads it ([`star_run`], [`class`]). State `2p` is the start of the element at pattern position
/// `p`: a literal, an escaped byte, `?`, a bracket expression, a `*` run, or the end. A `*` run stays put on a byte
/// it may match and moves past itself at no cost. A `**/` unit instead moves at no cost either past its `/` (no
/// directory) or into its loop, state `2p + 1`, which stays put on any byte and moves to the `/` at no cost. The text
/// matches iff the end is in the set after its last byte. Memory: two sets of 2 × (|pattern| + 1) bits.
// spec: [F20 §4.4] (git's semantics, decided in time |pattern| × |text| and memory linear in the pattern)
fn simulate(pat: &[u8], start: usize, text: &[u8], f: WildFlags) -> bool {
    let len = pat.len();
    let words = (2 * len + 1) / 64 + 1;
    let mut buf = vec![0u64; 2 * words];
    let (mut cur, mut next) = buf.split_at_mut(words);
    let first = 2 * start.min(len);
    insert(cur, first);
    let mut lo = first;
    let mut hi = close(pat, cur, first, first, f);
    for &c in text {
        let tc = fold(c, f.casefold);
        let (mut nlo, mut nhi) = (usize::MAX, 0usize);
        for (w, word) in cur.iter_mut().enumerate().take(hi / 64 + 1).skip(lo / 64) {
            let mut bits = std::mem::take(word);
            while bits != 0 {
                let s = w * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let q = match advance(pat, s, c, tc, f) {
                    Advance::To(q) => q,
                    Advance::Stop => continue,
                    Advance::Abort => return false,
                };
                insert(next, q);
                nlo = nlo.min(q);
                nhi = nhi.max(q);
            }
        }
        if nlo > nhi {
            return false;
        }
        lo = nlo;
        hi = close(pat, next, nlo, nhi, f);
        std::mem::swap(&mut cur, &mut next);
    }
    let end = 2 * len;
    cur[end / 64] & (1u64 << (end % 64)) != 0
}

#[inline]
fn insert(set: &mut [u64], s: usize) {
    set[s / 64] |= 1u64 << (s % 64);
}

/// Adds to `set` (whose members lie in `lo..=hi`) every state its members reach at no cost; returns the new highest
/// member. Those moves only go to higher states, so one ascending pass reaches them all.
fn close(pat: &[u8], set: &mut [u64], lo: usize, mut hi: usize, f: WildFlags) -> usize {
    let mut w = lo / 64;
    while w <= hi / 64 {
        let mut seen = 0u64;
        loop {
            let pending = set[w] & !seen;
            if pending == 0 {
                break;
            }
            let b = pending.trailing_zeros();
            seen |= 1u64 << b;
            let s = w * 64 + b as usize;
            let p = s / 2;
            if at(pat, p) != b'*' {
                continue;
            }
            let run = star_run(pat, p, f.pathname);
            let (to, also) = if s % 2 == 1 {
                // The loop of a `**/` unit, to its `/`.
                (2 * run.end, None)
            } else if run.dir_unit {
                (s + 1, Some(2 * (run.end + 1)))
            } else {
                (2 * run.end, None)
            };
            insert(set, to);
            hi = hi.max(to);
            if let Some(to) = also {
                insert(set, to);
                hi = hi.max(to);
            }
        }
        w += 1;
    }
    hi
}

/// Where state `s` leads on the text byte `c` (folded: `tc`).
fn advance(pat: &[u8], s: usize, c: u8, tc: u8, f: WildFlags) -> Advance {
    let p = s / 2;
    let Some(&pc) = pat.get(p) else {
        return Advance::Stop;
    };
    let to = |ok: bool, q: usize| if ok { Advance::To(q) } else { Advance::Stop };
    if s % 2 == 1 {
        // The loop of a `**/` unit takes any byte.
        return Advance::To(s);
    }
    match pc {
        b'*' => {
            let run = star_run(pat, p, f.pathname);
            to(!run.dir_unit && (run.match_slash || c != b'/'), s)
        }
        b'\\' => to(pat.get(p + 1) == Some(&tc), 2 * (p + 2)),
        b'?' => to(!(f.pathname && c == b'/'), 2 * (p + 1)),
        b'[' => {
            let mut q = p;
            match class(pat, &mut q, tc, f) {
                None => Advance::To(2 * (q + 1)),
                Some(NOMATCH) => Advance::Stop,
                Some(_) => Advance::Abort,
            }
        }
        _ => to(fold(pc, f.casefold) == tc, 2 * (p + 1)),
    }
}

/// A bracket expression at `pat[*p] == '['` against the (folded) text byte `tc`. On return without a result `*p` is
/// at the closing `]`; `Some(r)` ends the call with `r`. Whether it returns [`ABORT_ALL`] (a malformed expression)
/// depends on the pattern only.
// spec: [F20 §4.4] (git's semantics: bracket expressions, ranges, git's character classes, unfolded members)
fn class(pat: &[u8], p: &mut usize, tc: u8, f: WildFlags) -> Option<i8> {
    *p += 1;
    let mut pc = at(pat, *p);
    if pc == b'^' {
        pc = b'!';
    }
    let negated = pc == b'!';
    if negated {
        *p += 1;
        pc = at(pat, *p);
    }
    let mut prev: u8 = 0;
    let mut matched = false;
    loop {
        if pc == 0 {
            return Some(ABORT_ALL);
        }
        if pc == b'\\' {
            *p += 1;
            pc = at(pat, *p);
            if pc == 0 {
                return Some(ABORT_ALL);
            }
            if tc == pc {
                matched = true;
            }
        } else if pc == b'-' && prev != 0 && at(pat, *p + 1) != 0 && at(pat, *p + 1) != b']' {
            *p += 1;
            pc = at(pat, *p);
            if pc == b'\\' {
                *p += 1;
                pc = at(pat, *p);
                if pc == 0 {
                    return Some(ABORT_ALL);
                }
            }
            if tc <= pc && tc >= prev {
                matched = true;
            } else if f.casefold && tc.is_ascii_lowercase() {
                let up = tc.to_ascii_uppercase();
                if up <= pc && up >= prev {
                    matched = true;
                }
            }
            pc = 0;
        } else if pc == b'[' && at(pat, *p + 1) == b':' {
            *p += 2;
            let s = *p;
            while at(pat, *p) != 0 && at(pat, *p) != b']' {
                *p += 1;
            }
            if at(pat, *p) == 0 {
                return Some(ABORT_ALL);
            }
            if *p == s || pat[*p - 1] != b':' {
                // No `:]`: the `[` is an ordinary member.
                *p = s - 2;
                pc = b'[';
                if tc == pc {
                    matched = true;
                }
            } else {
                let hit = match &pat[s..*p - 1] {
                    b"alnum" => tc.is_ascii_alphanumeric(),
                    b"alpha" => tc.is_ascii_alphabetic(),
                    b"blank" => tc == b' ' || tc == b'\t',
                    b"cntrl" => tc < 0x20 || tc == 0x7f,
                    b"digit" => tc.is_ascii_digit(),
                    b"graph" => (0x21..=0x7e).contains(&tc),
                    b"lower" => tc.is_ascii_lowercase(),
                    b"print" => (0x20..=0x7e).contains(&tc),
                    b"punct" => tc.is_ascii_punctuation(),
                    b"space" => matches!(tc, b'\t' | b'\n' | b'\r' | b' '),
                    b"upper" => tc.is_ascii_uppercase() || (f.casefold && tc.is_ascii_lowercase()),
                    b"xdigit" => tc.is_ascii_hexdigit(),
                    _ => return Some(ABORT_ALL),
                };
                if hit {
                    matched = true;
                }
                pc = 0;
            }
        } else if tc == pc {
            matched = true;
        }
        prev = pc;
        *p += 1;
        pc = at(pat, *p);
        if pc == b']' {
            break;
        }
    }
    if matched == negated || (f.pathname && tc == b'/') {
        return Some(NOMATCH);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: WildFlags = WildFlags {
        pathname: true,
        casefold: false,
    };
    const B: WildFlags = WildFlags {
        pathname: false,
        casefold: false,
    };
    const PI: WildFlags = WildFlags {
        pathname: true,
        casefold: true,
    };

    fn m(pat: &str, text: &str, f: WildFlags) -> bool {
        wildmatch(pat.as_bytes(), 0, text.as_bytes(), f)
    }

    /// git's `dowild` read naively: plain recursion, every `**/` unit on its own, no step budget and no simulation.
    /// It returns git's four result codes.
    fn dowild_ref(pat: &[u8], mut p: usize, text: &[u8], mut t: usize, f: WildFlags) -> i8 {
        loop {
            let Some(&pc_raw) = pat.get(p) else {
                return if t < text.len() { NOMATCH } else { MATCH };
            };
            if t >= text.len() && pc_raw != b'*' {
                return ABORT_ALL;
            }
            let tc = fold(at(text, t), f.casefold);
            match fold(pc_raw, f.casefold) {
                b'\\' => {
                    p += 1;
                    if p >= pat.len() || pat[p] != tc {
                        return NOMATCH;
                    }
                }
                b'?' => {
                    if f.pathname && tc == b'/' {
                        return NOMATCH;
                    }
                }
                b'[' => {
                    if let Some(r) = class(pat, &mut p, tc, f) {
                        return r;
                    }
                }
                b'*' => {
                    p += 1;
                    let match_slash;
                    if at(pat, p) == b'*' {
                        let prev_ok = p < 2 || pat[p - 2] == b'/';
                        while at(pat, p) == b'*' {
                            p += 1;
                        }
                        let next = at(pat, p);
                        if !f.pathname {
                            match_slash = true;
                        } else if prev_ok
                            && (next == 0
                                || next == b'/'
                                || (next == b'\\' && at(pat, p + 1) == b'/'))
                        {
                            if next == b'/' && dowild_ref(pat, p + 1, text, t, f) == MATCH {
                                return MATCH;
                            }
                            match_slash = true;
                        } else {
                            match_slash = false;
                        }
                    } else {
                        match_slash = !f.pathname;
                    }
                    if p >= pat.len() {
                        return if !match_slash && text[t..].contains(&b'/') {
                            NOMATCH
                        } else {
                            MATCH
                        };
                    }
                    if !match_slash && pat[p] == b'/' {
                        match text[t..].iter().position(|&c| c == b'/') {
                            Some(i) => t += i,
                            None => return NOMATCH,
                        }
                    } else {
                        loop {
                            if t >= text.len() {
                                return ABORT_ALL;
                            }
                            if !is_glob_special(pat[p]) {
                                let pc = fold(pat[p], f.casefold);
                                while t < text.len()
                                    && (match_slash || text[t] != b'/')
                                    && fold(text[t], f.casefold) != pc
                                {
                                    t += 1;
                                }
                                if t >= text.len() || fold(text[t], f.casefold) != pc {
                                    return if match_slash {
                                        ABORT_ALL
                                    } else {
                                        ABORT_TO_STARSTAR
                                    };
                                }
                            }
                            let r = dowild_ref(pat, p, text, t, f);
                            if r != NOMATCH {
                                if !match_slash || r != ABORT_TO_STARSTAR {
                                    return r;
                                }
                            } else if !match_slash && text[t] == b'/' {
                                return ABORT_TO_STARSTAR;
                            }
                            t += 1;
                        }
                    }
                }
                pc => {
                    if tc != pc {
                        return NOMATCH;
                    }
                }
            }
            p += 1;
            t += 1;
        }
    }

    #[test]
    fn literals_and_single_wildcards() {
        assert!(m("foo", "foo", P));
        assert!(!m("foo", "fo", P));
        assert!(!m("foo", "fooo", P));
        assert!(m("f?o", "fxo", P));
        assert!(!m("f?o", "f/o", P));
        assert!(m("f?o", "f/o", B));
        assert!(m("*", "", P));
        assert!(m("*", "abc", P));
        assert!(!m("*", "a/b", P));
        assert!(m("*", "a/b", B));
        assert!(m("a*c", "abbbc", P));
        assert!(!m("a*c", "ab/bc", P));
        assert!(m("a*c", "ab/bc", B));
        assert!(!m("a*b", "a", P));
        assert!(m("\\*", "*", P));
        assert!(!m("\\*", "x", P));
        assert!(!m("abc\\", "abc", P));
        assert!(!m("abc\\", "abc\\", P));
        assert!(!m("", "a", P));
        assert!(m("", "", P));
    }

    #[test]
    fn star_star() {
        assert!(m("**", "a/b/c", P));
        assert!(m("**/c", "c", P));
        assert!(m("**/c", "a/b/c", P));
        assert!(!m("**/c", "a/bc", P));
        assert!(m("a/**", "a/b", P));
        assert!(m("a/**", "a/b/c", P));
        assert!(!m("a/**", "a", P));
        assert!(m("a/**/b", "a/b", P));
        assert!(m("a/**/b", "a/x/b", P));
        assert!(m("a/**/b", "a/x/y/b", P));
        assert!(!m("a/**/b", "a/xb", P));
        // Not a whole component: `**` is `*`.
        assert!(m("a**b", "axxb", P));
        assert!(!m("a**b", "ax/xb", P));
        assert!(!m("**b", "a/b", P));
        assert!(m("**b", "ab", P));
        assert!(!m("a/b**", "a/b/c", P));
        // `**\/` counts as a whole component without the zero-directory probe.
        assert!(m("**\\/b", "a/b", P));
        assert!(!m("**\\/b", "b", P));
        assert!(m("***/c", "a/c", P));
        // Without WM_PATHNAME `**` is `*`: it matches `/` anywhere, and `**/` needs its `/`.
        assert!(m("x**y", "x/y", B));
        assert!(m("**c", "a/c", B));
        assert!(m("**/c", "a/c", B));
        assert!(!m("**/c", "c", B));
        assert!(m("b**d", "b/c/d", B));
    }

    #[test]
    fn literal_prefix_does_not_make_a_component() {
        // `a**/b` with the prefix `a` already compared: the `**` still follows `a`.
        assert!(!wildmatch(b"a**/b", 1, b"/x/b", P));
        assert!(wildmatch(b"a**/b", 1, b"x/b", P));
        assert!(wildmatch(b"a/**/b", 2, b"x/y/b", P));
        assert!(wildmatch(b"ab", 2, b"", P));
        assert!(!wildmatch(b"ab", 2, b"c", P));
    }

    #[test]
    fn one_star_before_slash() {
        assert!(m("a/*/c", "a/b/c", P));
        assert!(!m("a/*/c", "a/b/x/c", P));
        assert!(m("*/c", "b/c", P));
        assert!(!m("*/c", "c", P));
        assert!(m("a*/c", "abc/c", P));
    }

    #[test]
    fn classes() {
        assert!(m("[abc]", "b", P));
        assert!(!m("[abc]", "d", P));
        assert!(m("[!abc]", "d", P));
        assert!(m("[^abc]", "d", P));
        assert!(!m("[^abc]", "a", P));
        assert!(m("[a-c]", "b", P));
        assert!(!m("[a-c]", "d", P));
        assert!(m("[]a]", "]", P));
        assert!(m("[!]a]", "b", P));
        assert!(m("[a-]", "-", P));
        assert!(m("[-a]", "-", P));
        assert!(!m("[a-c", "b", P));
        assert!(!m("[/]", "/", P));
        assert!(m("[/]", "/", B));
        assert!(!m("[!a]", "/", P));
        assert!(m("[\\]]", "]", P));
        assert!(m("[a\\-c]", "-", P));
        assert!(!m("[a\\-c]", "b", P));
        assert!(m("[[:digit:]]", "7", P));
        assert!(!m("[[:digit:]]", "x", P));
        assert!(m("[[:alpha:][:digit:]]", "7", P));
        assert!(m("[[:space:]]", "\t", P));
        assert!(m("[[:space:]]", "\r", P));
        assert!(!m("[[:space:]]", "\x0b", P));
        assert!(!m("[[:space:]]", "\x0c", P));
        assert!(m("[[:blank:]]", " ", P));
        assert!(!m("[[:blank:]]", "\n", P));
        assert!(m("[[:punct:]]", "_", P));
        assert!(m("[[:xdigit:]]", "F", P));
        assert!(m("[[:graph:]]", "~", P));
        assert!(!m("[[:graph:]]", " ", P));
        assert!(m("[[:print:]]", " ", P));
        assert!(m("[[:cntrl:]]", "\x7f", P));
        // An unknown class aborts the match.
        assert!(!m("[[:nope:]]", "n", P));
        assert!(!m("*[[:nope:]]", "n", P));
        // `[[:` without `:]` is an ordinary `[`.
        assert!(m("[[:a]", "[", P));
        assert!(m("[[:a]", ":", P));
        // `[[:]` is a set of `[` and `:`; the second `]` is a literal.
        assert!(m("[[:]]", "[]", B));
        assert!(m("[[:]]", ":]", B));
        assert!(!m("[[:]]", "[", B));
        assert!(!m("[[::]]", ":", P));
    }

    #[test]
    fn casefold() {
        assert!(m("ABC", "abc", PI));
        assert!(m("abc", "ABC", PI));
        assert!(m("[a-z]", "Q", PI));
        assert!(m("[A-Z]", "q", PI));
        assert!(m("[B-Za]", "A", PI));
        assert!(m("[[:upper:]]", "a", PI));
        assert!(m("[[:lower:]]", "A", PI));
        // Escaped letters and plain bracket members are not folded (git's behaviour).
        assert!(!m("\\A", "a", PI));
        assert!(!m("\\A", "A", PI));
        assert!(!m("[A]", "a", PI));
        assert!(!m("[A]", "A", PI));
        assert!(m("[a]", "A", PI));
        // Only ASCII folds.
        assert!(!m("\u{c0}", "\u{e0}", PI));
        assert!(m("*X", "abx", PI));
    }

    #[test]
    fn deep_patterns_run_in_bounded_depth_and_time() {
        let pat = "*a".repeat(20_000) + "b";
        let text = "a".repeat(30_000);
        assert!(!m(&pat, &text, P));
        let pat2 = "*?".repeat(10_000);
        let text2 = "x".repeat(10_000);
        assert!(m(&pat2, &text2, P));
        assert!(!m(&pat2, &text2[1..], P));
        let pat3 = "**/".repeat(5_000) + "z";
        let text3 = "d/".repeat(3_000) + "z";
        assert!(m(&pat3, &text3, P));
        // Exponential for git's matcher; polynomial here.
        let pat4 = "**/".repeat(200) + "z";
        let text4 = "d/".repeat(200) + "q";
        assert!(!m(&pat4, &text4, P));
        let pat5 = "*/**/".repeat(100) + "z";
        assert!(!m(&pat5, &text4, P));
        // Consecutive `**/` units cost one unit.
        let pat7 = "/**".repeat(100_000) + "/z";
        assert!(m(&pat7[1..], "z", P));
        assert!(m(&pat7[1..], &text3, P));
        assert!(!m(&pat7[1..], &text4, P));
        for text in ["z", "a/z", "a/b/z", "az", "a/zz", "z/a", ""] {
            for k in 1..5 {
                let pat = "**/".repeat(k) + "z";
                assert_eq!(m(&pat, text, P), m("**/z", text, P), "{pat} {text}");
                let pat = "a/".to_owned() + &"**/".repeat(k) + "**";
                assert_eq!(m(&pat, text, P), m("a/**", text, P), "{pat} {text}");
            }
        }
        let pat6 = "a*".repeat(300) + "[[:nope:]]";
        assert!(!m(&pat6, &"a".repeat(400), B));
    }

    #[test]
    fn nested_units_switch_to_the_simulation() {
        // Units that no merge removes: `**/x/` repeated. Backtracking would take time exponential in the units; the
        // budget hands the match to the simulation, whose memory is two bit sets of the pattern's length.
        let pat = "**/x/".repeat(400) + "z";
        let text = "x/".repeat(2_000);
        let hit = text.clone() + "z";
        let miss = text + "q";
        assert!(m(&pat, &hit, P));
        assert!(!m(&pat, &miss, P));
        assert!(m(&pat, &hit, PI));
        assert!(!m(&pat, &("x/".repeat(399) + "z"), P));
        assert!(m(&pat, &("x/".repeat(400) + "z"), P));
        let pat = "**/a*/".repeat(60) + "*b";
        let text = "aa/".repeat(80);
        assert!(m(&pat, &(text.clone() + "xb"), P));
        assert!(!m(&pat, &(text + "x/b"), P));
    }

    #[test]
    fn simulation_gives_the_same_results() {
        let cases: &[(&str, &str, bool)] = &[
            ("**/**/x", "a/b/x", true),
            ("**/**/x", "x", true),
            ("**/a/**/b", "a/b", true),
            ("**/a/**/b", "q/a/r/s/b", true),
            ("**/a/**/b", "q/a/r/s/c", false),
            ("*a*b*c", "xaybzc", true),
            ("*a*b*c", "xaybz/c", false),
            ("a*/**/c", "ab/x/c", true),
            ("a/*/c", "a/b/c", true),
            ("a/*/c", "a/b/x/c", false),
            ("*", "a/b", false),
            ("**", "a/b", true),
            ("**\\/b", "b", false),
            ("**\\/b", "a/b", true),
            ("[a-c", "b", false),
            ("x*[[:nope:]]", "xyz", false),
            ("abc\\", "abc", false),
            ("", "", true),
            ("", "a", false),
        ];
        for &(pat, text, want) in cases {
            let (pb, tb) = (pat.as_bytes(), text.as_bytes());
            assert_eq!(m(pat, text, P), want, "{pat} {text}");
            assert_eq!(run(pb, 0, tb, P, 0), want, "switched {pat} {text}");
            assert_eq!(simulate(pb, 0, tb, P), want, "simulated {pat} {text}");
        }
        assert!(!simulate(b"a**/b", 1, b"/x/b", P));
        assert!(simulate(b"a**/b", 1, b"x/b", P));
        assert!(simulate(b"*", 0, b"a/b", B));
        assert!(!simulate(b"[A]", 0, b"a", PI));
        assert!(simulate(b"[a]", 0, b"A", PI));
    }

    #[test]
    fn agrees_with_a_naive_port_of_dowild() {
        // A deterministic generator over a small alphabet: literals of both cases, `/`, `*`, `**`, `?`, brackets,
        // escapes, and `**/` units next to `*/`, `\/` and brackets.
        const PAT: &[&[u8]] = &[
            b"a", b"b", b"A", b"/", b"*", b"**", b"?", b"[ab]", b"[!a]", b"[a/]", b"[A-Z]", b"\\*",
            b"\\/", b"\\a", b"**/", b"**/", b"*/", b"/**",
        ];
        const TXT: &[u8] = b"aAb//";
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let mut pat = Vec::new();
            for _ in 0..(next() % 8) {
                pat.extend_from_slice(PAT[(next() % PAT.len() as u64) as usize]);
            }
            let text: Vec<u8> = (0..(next() % 9))
                .map(|_| TXT[(next() % TXT.len() as u64) as usize])
                .collect();
            for start in [0, simple_length(&pat)] {
                for f in [P, B, PI] {
                    let want = dowild_ref(&pat, start, &text, 0, f) == MATCH;
                    let show = || {
                        format!(
                            "{} @{start} {} {f:?}",
                            String::from_utf8_lossy(&pat),
                            String::from_utf8_lossy(&text)
                        )
                    };
                    assert_eq!(wildmatch(&pat, start, &text, f), want, "{}", show());
                    assert_eq!(
                        run(&pat, start, &text, f, usize::MAX),
                        want,
                        "backtracking {}",
                        show()
                    );
                    assert_eq!(
                        simulate(&pat, start, &text, f),
                        want,
                        "simulated {}",
                        show()
                    );
                }
            }
        }
    }
}
