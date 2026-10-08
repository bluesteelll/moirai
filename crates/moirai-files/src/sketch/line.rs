//! The fingerprint-line hasher that every sink of the module shares ([F20 §2.6.1], §2.6.2).

use xxhash_rust::xxh3::Xxh3Default;

use crate::r14::FP_MIN_CHARS;
use crate::text::{chars, is_ws};

/// Streams `f = collapse(nl(l))` of one line over its pieces, in constant state ([F20 §2.6.1]): `nl` drops the leading
/// and trailing `WS` bytes, and `collapse` replaces every maximal run of `WS` bytes by one `20`.
///
/// A `WS` run after a byte outside `WS` only marks a space as pending; the next byte outside `WS` first hashes that
/// one `20`, and a pending space at the end of the line is dropped, so the line is never buffered and a piece boundary
/// anywhere (inside a `WS` run or a UTF-8 sequence) changes nothing.
#[derive(Clone)]
pub(crate) struct LineHasher {
    /// XXH3-64 of the bytes of f so far.
    hash: Xxh3Default,
    /// `len(f)` so far, without the pending space.
    len: u64,
    /// `chars(f)` so far, without the pending space ([F20 §1.2]).
    chars: usize,
    /// A byte outside `WS` was hashed: a `WS` run is now inside the line, not leading.
    started: bool,
    /// A `WS` run followed the last hashed byte.
    pending: bool,
}

impl LineHasher {
    /// A hasher at the start of a line.
    pub(crate) const fn new() -> LineHasher {
        LineHasher {
            hash: Xxh3Default::new(),
            len: 0,
            chars: 0,
            started: false,
            pending: false,
        }
    }

    /// Forgets the current line, a half-received one included.
    pub(crate) fn reset(&mut self) {
        self.hash.reset();
        self.len = 0;
        self.chars = 0;
        self.started = false;
        self.pending = false;
    }

    /// The next bytes of the current line.
    pub(crate) fn piece(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            if is_ws(bytes[i]) {
                while i < bytes.len() && is_ws(bytes[i]) {
                    i += 1;
                }
                // A leading run is `nl`'s; a run after a hashed byte is one space if a byte outside `WS` follows.
                self.pending |= self.started;
                continue;
            }
            let start = i;
            while i < bytes.len() && !is_ws(bytes[i]) {
                i += 1;
            }
            if self.pending {
                self.pending = false;
                self.add(b" ");
            }
            self.add(&bytes[start..i]);
            self.started = true;
        }
    }

    fn add(&mut self, run: &[u8]) {
        self.hash.update(run);
        self.len += run.len() as u64;
        self.chars = self.chars.saturating_add(chars(run));
    }

    /// Ends the current line: `Some((fh(f), len(f)))` when f is a fingerprint line, `chars(f) > FP_MIN_CHARS`
    /// ([F20 §2.6.1], §2.6.2), and `None` otherwise. The hasher is then at the start of the next line.
    pub(crate) fn end_line(&mut self) -> Option<(u64, u64)> {
        let out = (self.chars > FP_MIN_CHARS).then(|| (self.hash.digest(), self.len));
        self.reset();
        out
    }
}

impl Default for LineHasher {
    fn default() -> LineHasher {
        LineHasher::new()
    }
}
