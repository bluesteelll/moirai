//! Anchors: capture and the anchor cascade of R4 ([40 §2.7, §4.5]; [F20 §6]; the record of [F08 §10.3] and the
//! derivations of [F08 §11.4]; the scanners of \[F21\] through [`crate::scan`]).
//!
//! # What this module does
//!
//! - **Capture** ([F20 §6.1]; [`Anchors::capture`]): from an authoring form and the captured content, the selectors
//!   of a new anchor — kind, quote, `end`, prefix, suffix, the window, the hint, `span_hash`, `blob`, `git`,
//!   `captured`, `resolver` — with the authoring forms' refusals ([F19 §10.2] `anchor_spec`), the BOM and U+FFFD rules
//!   of quote input, the span's trivial-line skips and the uniqueness ladder. [`identify`] then reuses an anchor with
//!   equal current selectors or derives the new anchor's uid ([F08 §11.4] steps 1–4).
//! - **Resolve** ([F20 §6.2]–§6.6; [`Anchors::resolve`]): the cascade over the current content — hint, exact quote
//!   with the context, window and occurrence tie-breaks, fuzzy quote by Myers with its score and margins, scope only,
//!   `lines` anchors, `text-unavailable` anchors, the watch rules, binary content, and `unverified` when an input is
//!   unavailable.
//!
//! Path resolution, `PATHIDX`, `ProjectFs` and the file cascade are not here: the caller resolves the file and hands
//! its bytes in as a [`ByteSource`] (M2/M6). The module opens nothing (PLAN §6.2 R18; GT20 (d)).
//!
//! # Streaming and memory
//!
//! No step holds the file or its normalised text N ([40 §2.5], \[71\] RAM-M4). Every step is a pass of the reader
//! ([`ContentReader`]) whose line hook streams N to a sink (module `nstream`): the first read is the two-pass read that
//! also yields `oid`, the statistics and the capped line-hash array that the window steps read ([F20 §2.4]); every
//! later read is one pass that must see the same bytes (raw length, raw XXH3-64 and the handle's snapshots,
//! [`ContentReader::reread`]), else the anchor is `unverified (budget)` (the `unstable` reason, [F20 §1.5]). Each pass
//! is charged to the budget before it starts. A step keeps a fixed tail of N — a few KiB for the quotes a capture
//! writes (≤ 128 bytes, context ≤ 64; an imported anchor with longer texts is `unverified (budget)` before any quote
//! step) — the line-hash array (≤ 136 KiB at the default `files.max-line-hashes`) and the reader's 128 KiB buffer.
//! What grows with the content grows with its matches: `range` start hits waiting for their end hits within the
//! spread, and those end hits; the end searches of a `range` anchor's fuzzy start candidates (one per candidate inside
//! the spread) and the bytes they search; and a `symbol` header's window, its current line or an unterminated literal
//! or block comment it is inside (module `header`). A pass charges that growth to the budget as it grows, after every
//! slice of N (module `nstream`, `Metered`), so a hint far longer than the text, a long range over repeated lines or a
//! literal left open to the end of the file ends as `unverified (budget)` under a finite budget, never as another
//! target ([40 §4.1] P7).
//!
//! # Exactness
//!
//! Scores are exact rationals ([`Score`]; [F20 §1.2]). Constants are [`crate::r14`]'s, holes at their draft values.
//! [F20 §6.1]'s interim scanner rule is [`crate::r14::INTERIM_SCANNER_RULE`]; [`ScannerRule::CURRENT`] follows it,
//! and [`ScannerRule::Lifted`] runs the scanner steps of [F21 §2.4]–§2.6 and §6 that apply once review lifts it.

mod capture;
mod exact;
mod fuzzy;
mod header;
mod nstream;
mod resolve;
mod score;
mod window;

use std::num::NonZeroU16;

pub use capture::{
    CaptureError, CaptureInput, Captured, Form, Identity, Refusal, Spec, capture_planned, identify,
    parse_spec, quote_text,
};
pub use header::{HeaderKind, header};
pub use resolve::{AnchorState, Resolution, ResolveInput, UnverifiedReason};
pub use score::Score;
pub use window::{WIN, Window, WindowError};

use crate::oid::{ObjectFormat, Oid};
use crate::r14;
use crate::text::{ByteSource, ContentReader, Snapshot};
pub use crate::uid::AnchorKind;
use crate::uid::Uid;

/// `mode` ([F08 §10.3]): a `live` anchor is re-resolved; a `pinned` one is a historical citation, never re-resolved
/// ([40 §2.7]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Mode {
    /// 1 `live`.
    Live = 1,
    /// 2 `pinned`.
    Pinned = 2,
}

/// `watch` ([F08 §10.3]; [40 §2.7], §4.5 "Watch semantics").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Watch {
    /// 1 `header`: a changed body keeps the anchor `fresh`.
    Header = 1,
    /// 2 `span`: a changed span is `edited`; for a `file` anchor, a content pin.
    Span = 2,
}

impl Watch {
    /// The default watch of a kind: `header` for `file`, `heading` and `symbol`, `span` for the others ([F08 §10.3]).
    #[must_use]
    // spec: [F08 §10.3] (`watch` defaults)
    pub const fn default_for(kind: AnchorKind) -> Watch {
        match kind {
            AnchorKind::File | AnchorKind::Heading | AnchorKind::Symbol => Watch::Header,
            AnchorKind::Quote | AnchorKind::Range | AnchorKind::Lines => Watch::Span,
        }
    }
}

/// Lines `[first, last]`, 1-based and inclusive: an anchor's hint ([F08 §10.3] `hint_first`, `hint_last`) or a
/// resolved span.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineSpan {
    /// The first line, ≥ 1.
    pub first: u32,
    /// The last line, ≥ `first`.
    pub last: u32,
}

impl LineSpan {
    /// The number of lines; 0 for a span whose last line is before its first.
    #[must_use]
    pub const fn len(&self) -> u64 {
        (self.last as u64 + 1).saturating_sub(self.first as u64)
    }

    /// Whether the span is valid ([F08 §10.3]: `hint_last` ≥ `hint_first` ≥ 1).
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.first >= 1 && self.last >= self.first
    }

    /// Whether the span is empty (never, for a valid span).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.last < self.first
    }

    pub(crate) fn of(first: u64, last: u64) -> LineSpan {
        LineSpan {
            first: u32::try_from(first).unwrap_or(u32::MAX),
            last: u32::try_from(last).unwrap_or(u32::MAX),
        }
    }
}

/// An anchor's quote texts ([F08 §10.3] orders 12–19): held, or only their digests (`text_unavailable`, [40 §5.7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Texts {
    /// The exact bytes of N ([F20 §2.5, §6.1]).
    Held {
        /// `quote.exact` (for a `range`, the start quote); non-empty (I-F9).
        quote: Vec<u8>,
        /// The prefix as widened at capture; may be empty.
        prefix: Vec<u8>,
        /// The suffix; may be empty.
        suffix: Vec<u8>,
        /// A `range` anchor's end quote.
        end: Option<Vec<u8>>,
    },
    /// BLAKE3-128 of each text: an anchor imported without its text.
    Digests {
        /// `quote_h`.
        quote: [u8; 16],
        /// `prefix_h`.
        prefix: [u8; 16],
        /// `suffix_h`.
        suffix: [u8; 16],
        /// `end_h`, for a `range`.
        end: Option<[u8; 16]>,
    },
}

/// BLAKE3-128 of a text ([F08 §10.3] `quote_h`, `prefix_h`, `suffix_h`, `end_h`).
// spec: [F08 §10.3] (`quote_h`, `prefix_h`, `suffix_h`, `end_h`: BLAKE3-128)
fn digest128(x: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&blake3::hash(x).as_bytes()[..16]);
    out
}

impl Texts {
    /// The digests of the texts, held or not.
    #[must_use]
    // spec: [F08 §10.3], [F07 §8.2] (texts enter as digests)
    pub fn digests(&self) -> ([u8; 16], [u8; 16], [u8; 16], Option<[u8; 16]>) {
        match self {
            Texts::Held {
                quote,
                prefix,
                suffix,
                end,
            } => (
                digest128(quote),
                digest128(prefix),
                digest128(suffix),
                end.as_deref().map(digest128),
            ),
            Texts::Digests {
                quote,
                prefix,
                suffix,
                end,
            } => (*quote, *prefix, *suffix, *end),
        }
    }
}

/// The selectors of an anchor record ([F08 §10.3]) that capture writes and the cascade reads. The byte layout is
/// \[F08\]'s, the canonical selector block \[F07\]'s and the image line \[F14\]'s; `uid`, `aN` and the edge are the
/// caller's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// `kind`.
    pub kind: AnchorKind,
    /// `mode`.
    pub mode: Mode,
    /// `watch`.
    pub watch: Watch,
    /// The resolver version at capture ([F20 §1.3]), ≥ 1.
    pub resolver: u16,
    /// The capture digest ([F08 §11.4]); never changes.
    pub captured: [u8; 16],
    /// The predecessor term of the uid, when the derivation loop ran ([F08 §11.4] step 4).
    pub pred: Option<Uid>,
    /// The hint; absent for a `file` anchor.
    pub hint: Option<LineSpan>,
    /// The scope value ([F08 §10.3.1]), when one was recorded.
    pub scope: Option<Vec<u8>>,
    /// The quote texts of `heading`, `symbol`, `quote` and `range` anchors.
    pub texts: Option<Texts>,
    /// The occurrence index ([F20 §6.1] step 8.3).
    pub occurrence: Option<NonZeroU16>,
    /// The window ([F20 §2.7.3]); absent for a `file` anchor.
    pub window: Option<Window>,
    /// `span_hash` ([F20 §2.8]); absent for a `file` anchor.
    pub span_hash: Option<u64>,
    /// The file's `oid` at capture; `none` for a planned target.
    pub blob: Oid,
    /// The observed git commit at capture.
    pub git: Option<Oid>,
    /// The opt-in in-file marker id ([40 §9.2] decision 3).
    pub marker: Option<String>,
}

impl Anchor {
    /// Whether the texts are held only as digests (`text_unavailable`, [F08 §10.3] `aflags` bit 5).
    #[must_use]
    // spec: [F08 §10.3] (`aflags` bit 5), [40 §5.7]
    pub fn text_unavailable(&self) -> bool {
        matches!(self.texts, Some(Texts::Digests { .. }))
    }

    /// Whether `self`'s current selectors equal `other`'s for capture de-duplication ([F08 §11.4] step 1): equal
    /// `kind`, `scope`, `quote`, `prefix`, `suffix`, `end`, `occurrence` and, for `lines`, `window`. A text held on one
    /// side and only digested on the other compares by digest.
    // spec: [F08 §11.4] step 1, [40 §2.7] "Capture de-duplication"
    #[must_use]
    pub fn same_selectors(&self, other: &Anchor) -> bool {
        let texts = match (&self.texts, &other.texts) {
            (None, None) => true,
            (Some(a @ Texts::Held { .. }), Some(b @ Texts::Held { .. })) => a == b,
            (Some(a), Some(b)) => a.digests() == b.digests(),
            _ => false,
        };
        self.kind == other.kind
            && self.scope == other.scope
            && texts
            && self.occurrence == other.occurrence
            && (self.kind != AnchorKind::Lines || self.window == other.window)
    }
}

/// Whether [F20 §6.1]'s interim scanner rule applies to a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScannerRule {
    /// The interim rule: no scope recorded, `symbol` and `heading` forms refused, no scanner step at resolve.
    Interim,
    /// The rule lifted: the scanner steps of [F21 §2.4]–§2.6 and §6.
    Lifted,
}

impl ScannerRule {
    /// The rule of resolver version 1 as [`r14::INTERIM_SCANNER_RULE`] states it. Callers pass this one; the other
    /// value exists for the tests of the lifted rule and for the issue that lifts it.
    pub const CURRENT: ScannerRule = if r14::INTERIM_SCANNER_RULE {
        ScannerRule::Interim
    } else {
        ScannerRule::Lifted
    };
}

/// The parameters of every read of one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadParams {
    /// A(R), the object format of the file's root ([F20 §2.3]).
    pub format: ObjectFormat,
    /// `files.max-read-bytes` ([CFG §10.4]).
    pub max_read_bytes: u64,
    /// `files.max-line-hashes` ([CFG §10.4]): beyond it the window steps are `Unavailable(size)`.
    pub max_line_hashes: u32,
}

/// A command's budget ([F20 §1.3] input (e)), charged in units of bytes read, held or searched again: before each pass
/// (the first read with the size the handle reports, before any byte is read; a later read with the content's length
/// and its sink's fixed part, before the sink is built) and during it, as its sinks grow (the peak bytes they hold
/// beyond their fixed part and the bytes of N they search again). A budget that runs out turns the answer into
/// `unverified (budget)`, never into another target ([40 §4.1] P7).
pub trait Budget {
    /// Spends `units`; `false` when the budget is exhausted.
    fn spend(&mut self, units: u64) -> bool;
}

/// The budget that never runs out.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unlimited;

impl Budget for Unlimited {
    fn spend(&mut self, _units: u64) -> bool {
        true
    }
}

/// A [`ByteSource`] over bytes already in memory (a git blob, a test text): it cannot change between passes.
#[derive(Clone, Copy, Debug)]
pub struct SliceSource<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> SliceSource<'a> {
    /// A source at the start of `bytes`.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> SliceSource<'a> {
        SliceSource { bytes, pos: 0 }
    }
}

impl ByteSource for SliceSource<'_> {
    type Error = core::convert::Infallible;
    type Stamp = ();

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let n = buf.len().min(self.bytes.len() - self.pos);
        buf[..n].copy_from_slice(&self.bytes[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn rewind(&mut self) -> Result<(), Self::Error> {
        self.pos = 0;
        Ok(())
    }

    fn snapshot(&self) -> Result<Snapshot<()>, Self::Error> {
        Ok(Snapshot {
            size: self.bytes.len() as u64,
            mtime: (),
        })
    }
}

/// Capture and resolution with one reader: keep one per thread and reuse it ([`ContentReader`]'s buffer).
#[derive(Default)]
pub struct Anchors {
    reader: ContentReader,
}

impl Anchors {
    /// A new instance with its reader.
    #[must_use]
    pub fn new() -> Anchors {
        Anchors::default()
    }

    /// Captures an anchor ([F20 §6.1]) on the content `src` supplies.
    ///
    /// # Errors
    /// [`CaptureError::Refused`] for a form [F19 §10.2] `anchor_spec` refuses; [`CaptureError::Unavailable`] when the
    /// content cannot be read ([F20 §2.4]) or the budget runs out.
    pub fn capture<S: ByteSource, B: Budget>(
        &mut self,
        src: &mut S,
        input: &CaptureInput<'_>,
        budget: &mut B,
    ) -> Result<Captured, CaptureError> {
        capture::capture(&mut self.reader, src, input, budget)
    }

    /// Resolves an anchor ([F20 §6.2]–§6.6) against the current content `src` supplies. The file cascade has
    /// resolved the file (`ok` or `moved-auto`) and its content is not `replaced` ([40 §4.5]).
    pub fn resolve<S: ByteSource, B: Budget>(
        &mut self,
        src: &mut S,
        anchor: &Anchor,
        input: &ResolveInput<'_>,
        budget: &mut B,
    ) -> Resolution {
        resolve::resolve(&mut self.reader, src, anchor, input, budget)
    }
}
