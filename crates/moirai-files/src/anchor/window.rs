//! The window of an anchor ([F20 §2.7]): the window hashes of the non-trivial lines around a span, the window value W
//! that the `window` selector stores (§2.7.3), and the window score that aligns two windows (§6.3).

use crate::r14;
use crate::text::LineHashes;

/// `WIN`: non-trivial lines hashed on each side of a span ([F20 §2.7.2]; HOLE(F20-window-lines), draft 16).
pub const WIN: usize = r14::WINDOW_LINES as usize;

/// A window: up to [`WIN`] window hashes before a span and up to [`WIN`] after it, each in file order ([F20 §2.7.2]).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Window {
    n_before: u8,
    n_after: u8,
    /// `before` in `h[..n_before]`, `after` in `h[WIN..WIN + n_after]`.
    h: [u16; 2 * WIN],
}

impl core::fmt::Debug for Window {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Window")
            .field("before", &self.before())
            .field("after", &self.after())
            .finish()
    }
}

/// A byte string that is not a valid window value ([F20 §2.7.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowError {
    /// Shorter than its header or than its counts say, or longer.
    Length,
    /// A count above [`WIN`].
    Count,
}

impl core::fmt::Display for WindowError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            WindowError::Length => "window value of the wrong length",
            WindowError::Count => "window value with a count above WIN",
        })
    }
}

impl std::error::Error for WindowError {}

impl Window {
    /// The empty window: no hashes on either side.
    pub const EMPTY: Window = Window {
        n_before: 0,
        n_after: 0,
        h: [0; 2 * WIN],
    };

    /// The window with these hashes, each side in file order; `None` when a side has more than [`WIN`].
    #[must_use]
    // spec: [F20 §2.7.2] (at most WIN hashes a side)
    pub fn new(before: &[u16], after: &[u16]) -> Option<Window> {
        if before.len() > WIN || after.len() > WIN {
            return None;
        }
        let mut w = Window::EMPTY;
        w.h[..before.len()].copy_from_slice(before);
        w.h[WIN..WIN + after.len()].copy_from_slice(after);
        // Both counts are at most WIN ≤ 16.
        w.n_before = before.len() as u8;
        w.n_after = after.len() as u8;
        Some(w)
    }

    /// `before(s)`: the hashes before the span, in file order.
    #[must_use]
    pub fn before(&self) -> &[u16] {
        &self.h[..usize::from(self.n_before)]
    }

    /// `after(e)`: the hashes after the span, in file order.
    #[must_use]
    pub fn after(&self) -> &[u16] {
        &self.h[WIN..WIN + usize::from(self.n_after)]
    }

    /// The number of hashes, `len(B) + len(A)`: the window score's denominator ([F20 §6.3]).
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.n_before) + usize::from(self.n_after)
    }

    /// Whether the window holds no hash ([F18 §2.9]: a `lines` anchor must carry a non-empty window).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The window value W ([F20 §2.7.3]): `u16 n_before ‖ u16 n_after ‖ before ‖ after`, little-endian, `4 + 2 ×
    /// (n_before + n_after)` bytes.
    // spec: [F20 §2.7.3]
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + 2 * self.len());
        out.extend_from_slice(&u16::from(self.n_before).to_le_bytes());
        out.extend_from_slice(&u16::from(self.n_after).to_le_bytes());
        for h in self.before().iter().chain(self.after()) {
            out.extend_from_slice(&h.to_le_bytes());
        }
        out
    }

    /// Decodes a window value W ([F20 §2.7.3]).
    ///
    /// # Errors
    /// [`WindowError::Count`] for a count above [`WIN`]; [`WindowError::Length`] when the length is not `4 + 2 ×
    /// (n_before + n_after)`.
    // spec: [F20 §2.7.3] (a value with a count above WIN is invalid)
    pub fn from_bytes(b: &[u8]) -> Result<Window, WindowError> {
        let [b0, b1, a0, a1, rest @ ..] = b else {
            return Err(WindowError::Length);
        };
        let nb = usize::from(u16::from_le_bytes([*b0, *b1]));
        let na = usize::from(u16::from_le_bytes([*a0, *a1]));
        if nb > WIN || na > WIN {
            return Err(WindowError::Count);
        }
        if rest.len() != 2 * (nb + na) {
            return Err(WindowError::Length);
        }
        let mut hs = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c));
        let before: Vec<u16> = hs.by_ref().take(nb).collect();
        let after: Vec<u16> = hs.collect();
        Window::new(&before, &after).ok_or(WindowError::Count)
    }

    /// The window score of a candidate's window against this stored window ([F20 §6.3]): `(LCS(B_s, B_c) + LCS(A_s,
    /// A_c)) / (len(B_s) + len(A_s))`, as (numerator, denominator); before-hashes align only with before-hashes and
    /// after-hashes only with after-hashes. A stored window with no hash gives 0 over 1 for every candidate.
    // spec: [F20 §6.3]
    #[must_use]
    pub fn score(&self, cand: &Window) -> (u32, u32) {
        let den = self.len();
        if den == 0 {
            return (0, 1);
        }
        let num = moirai_diff::lcs_len(self.before(), cand.before())
            + moirai_diff::lcs_len(self.after(), cand.after());
        // Both are at most 2 × WIN.
        (num as u32, den as u32)
    }
}

/// The window around lines [`qs`, `qe`] of a text from its complete line-hash array ([F20 §2.4], §2.7.2):
/// `before(qs)` holds the window hashes of the last `WIN` non-trivial lines before `qs` and `after(qe)` those of the
/// first `WIN` non-trivial lines after `qe`, in file order. The caller passes a complete array; a capped one makes
/// the window steps `Unavailable(size)` before this is called.
// spec: [F20 §2.7.2], [F20 §2.4] (the window steps read the line-hash array)
#[must_use]
pub(crate) fn around(lh: &LineHashes, qs: u64, qe: u64) -> Window {
    let total = lh.len() as u64;
    let mut before = [0u16; WIN];
    let mut nb = 0;
    let mut i = qs.min(total + 1);
    while nb < WIN && i > 1 {
        i -= 1;
        if let Some(wh) = lh.get((i - 1) as usize).and_then(|e| e.window_hash()) {
            before[nb] = wh;
            nb += 1;
        }
    }
    before[..nb].reverse();
    let mut after = [0u16; WIN];
    let mut na = 0;
    let mut j = qe;
    while na < WIN && j < total {
        j += 1;
        if let Some(wh) = lh.get((j - 1) as usize).and_then(|e| e.window_hash()) {
            after[na] = wh;
            na += 1;
        }
    }
    Window::new(&before[..nb], &after[..na]).unwrap_or(Window::EMPTY)
}

/// Windows around spans computed over a complete line-hash array, remembering the last `before` and `after` side so
/// that hits on the same lines, the common case of duplicate quotes, do not scan the array again.
#[derive(Clone, Debug)]
pub(crate) struct WindowCache {
    before: Option<(u64, [u16; WIN], usize)>,
    after: Option<(u64, [u16; WIN], usize)>,
}

impl WindowCache {
    pub(crate) const fn new() -> WindowCache {
        WindowCache {
            before: None,
            after: None,
        }
    }

    /// [`around`] with the cache.
    pub(crate) fn around(&mut self, lh: &LineHashes, qs: u64, qe: u64) -> Window {
        let hit_b = self.before.filter(|b| b.0 == qs);
        let hit_a = self.after.filter(|a| a.0 == qe);
        if let (Some(b), Some(a)) = (hit_b, hit_a) {
            return Window::new(&b.1[..b.2], &a.1[..a.2]).unwrap_or(Window::EMPTY);
        }
        let w = around(lh, qs, qe);
        let mut b = [0u16; WIN];
        b[..w.before().len()].copy_from_slice(w.before());
        let mut a = [0u16; WIN];
        a[..w.after().len()].copy_from_slice(w.after());
        self.before = Some((qs, b, w.before().len()));
        self.after = Some((qe, a, w.after().len()));
        w
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::oid::ObjectFormat;
    use crate::text::analyse;

    #[test]
    fn value_bytes_round_trip_and_validity() {
        // The canonical fixture's quote-anchor window: n_before 1, n_after 2.
        let w = Window::new(&[0x1e5a], &[0xd2c7, 0x440b]).unwrap();
        let b = w.to_bytes();
        assert_eq!(
            b,
            [0x01, 0x00, 0x02, 0x00, 0x5a, 0x1e, 0xc7, 0xd2, 0x0b, 0x44]
        );
        assert_eq!(Window::from_bytes(&b), Ok(w));
        assert_eq!(Window::EMPTY.to_bytes(), [0, 0, 0, 0]);
        assert_eq!(Window::from_bytes(&[0, 0, 0]), Err(WindowError::Length));
        assert_eq!(Window::from_bytes(&[1, 0, 0, 0]), Err(WindowError::Length));
        assert_eq!(Window::from_bytes(&[17, 0, 0, 0]), Err(WindowError::Count));
        assert_eq!(
            Window::from_bytes(&[0, 0, 0, 0, 0]),
            Err(WindowError::Length)
        );
        assert!(Window::new(&[0; 17], &[]).is_none());
        // At WIN = 16 the value is at most 68 bytes.
        assert_eq!(
            Window::new(&[1; WIN], &[2; WIN]).unwrap().to_bytes().len(),
            4 + 4 * WIN
        );
    }

    #[test]
    fn score_aligns_each_side_separately() {
        let s = Window::new(&[1, 2, 3], &[4, 5]).unwrap();
        assert_eq!(s.score(&s), (5, 5));
        let c = Window::new(&[4, 5], &[1, 2, 3]).unwrap();
        assert_eq!(s.score(&c), (0, 5));
        let c = Window::new(&[9, 1, 3], &[5]).unwrap();
        assert_eq!(s.score(&c), (3, 5));
        assert_eq!(Window::EMPTY.score(&s), (0, 1));
    }

    #[test]
    fn around_skips_trivial_lines_and_clips_at_the_ends() {
        let t = b"a\n}\nb\n\nc\nd\n);\ne\n";
        let c = analyse(t, ObjectFormat::Sha1, Some(1024), &mut ());
        let lh = c.line_hashes.unwrap();
        let wh = |l: &[u8]| crate::text::window_hash(l).unwrap();
        let w = around(&lh, 5, 6);
        assert_eq!(w.before(), [wh(b"a"), wh(b"b")]);
        assert_eq!(w.after(), [wh(b"e")]);
        let w = around(&lh, 1, 1);
        assert!(w.before().is_empty());
        assert_eq!(w.after().len(), 4);
        let w = around(&lh, 8, 8);
        assert_eq!(w.before().len(), 4);
        assert!(w.after().is_empty());
    }

    proptest! {
        #[test]
        fn bytes_round_trip(before in proptest::collection::vec(any::<u16>(), 0..=WIN),
                            after in proptest::collection::vec(any::<u16>(), 0..=WIN)) {
            let w = Window::new(&before, &after).unwrap();
            prop_assert_eq!(Window::from_bytes(&w.to_bytes()), Ok(w));
            prop_assert_eq!(w.before(), &before[..]);
            prop_assert_eq!(w.after(), &after[..]);
        }

        #[test]
        fn cache_equals_direct(t in proptest::collection::vec(prop_oneof![Just(b'a'), Just(b'b'), Just(b'}'),
                                                                          Just(b'\n')], 0..80),
                               spans in proptest::collection::vec((1u64..30, 0u64..4), 1..12)) {
            let c = analyse(&t, ObjectFormat::Sha1, Some(1 << 16), &mut ());
            if let Some(lh) = c.line_hashes {
                let mut cache = WindowCache::new();
                for (qs, ext) in spans {
                    prop_assert_eq!(cache.around(&lh, qs, qs + ext), around(&lh, qs, qs + ext));
                }
            }
        }
    }
}
