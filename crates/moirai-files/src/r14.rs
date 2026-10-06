//! The R-14 constant module: every constant of resolver version 1 ([F20 §7]), under the names chapter 20 gives them.
//!
//! [F20 §1.3]: any change to a value here is a new resolver version, except the fill of a named hole before the
//! `format-v1` tag. [F20 §1.4]: the values an M0 measurement decides are named holes; until WP-81a fills one, its draft
//! value is the one the code uses, and each such constant names its hole id (`HOLE(F20-…)`) in its doc comment. The one
//! hole without a draft value, [`SKEW`], is `None` until it is filled. No value here is a configuration key, an
//! environment variable or a flag ([F20 §1.1], [AR §13] "Never a key").
//!
//! Scores, ratios and thresholds are exact non-negative rationals ([F20 §1.2] "Rationals"): they are [`Ratio`]
//! values, compared by cross-multiplication in integer arithmetic, never as floating point.

use core::cmp::Ordering;

/// An exact non-negative rational number `num / den` ([F20 §1.2] "Rationals").
///
/// Equality and order are those of the rational values (`1/2 == 2/4`), evaluated by cross-multiplication in `u128`,
/// so no comparison rounds. A decimal threshold written in chapter 20 denotes the exact rational (0.29 is 29/100).
#[derive(Clone, Copy, Debug)]
pub struct Ratio {
    num: u64,
    den: u64,
}

impl Ratio {
    /// The rational `num / den`. `den` must be positive; a zero denominator panics (at compile time in a `const`).
    #[must_use]
    pub const fn new(num: u64, den: u64) -> Ratio {
        assert!(den != 0, "a ratio needs a positive denominator");
        Ratio { num, den }
    }

    /// The rational 0.
    pub const ZERO: Ratio = Ratio::new(0, 1);

    /// The rational 1.
    pub const ONE: Ratio = Ratio::new(1, 1);

    /// The numerator as given.
    #[must_use]
    pub const fn num(self) -> u64 {
        self.num
    }

    /// The denominator as given (positive).
    #[must_use]
    pub const fn den(self) -> u64 {
        self.den
    }

    /// `x / den`, or 0 when `den` is 0: chapter 20's rule for a measure whose denominator is 0 ([F20 §2.10.1],
    /// §2.10.2, §2.10.3, §2.9).
    #[must_use]
    pub const fn or_zero(num: u64, den: u64) -> Ratio {
        if den == 0 {
            Ratio::ZERO
        } else {
            Ratio::new(num, den)
        }
    }

    /// `1 − self`, for `self ≤ 1`; saturates at 0 above 1.
    #[must_use]
    pub const fn complement(self) -> Ratio {
        if self.num >= self.den {
            Ratio::ZERO
        } else {
            Ratio::new(self.den - self.num, self.den)
        }
    }
}

impl PartialEq for Ratio {
    fn eq(&self, other: &Ratio) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Ratio {}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, other: &Ratio) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ratio {
    fn cmp(&self, other: &Ratio) -> Ordering {
        (u128::from(self.num) * u128::from(other.den))
            .cmp(&(u128::from(other.num) * u128::from(self.den)))
    }
}

/// How far a volume's file creation time can be trusted: `VolumeCaps.btime` ([OS/project §4.2], [80 §2.11.1]), as
/// copy-rule line 2 ([F20 §5.9]) reads it.
///
/// The same name, variants, order and discriminants as `moirai-vfs`'s `BtimeTrust`, which this crate cannot depend on
/// (PLAN §2.2), so a caller maps one onto the other variant by variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum BtimeTrust {
    /// The volume records no creation time.
    Absent = 0,
    /// A rename keeps the creation time, a copy gets a new one, and NTFS tunnelling may restore an old one.
    TunneledNotCopied = 1,
    /// No tool can give a new file an old creation time.
    Unforgeable = 2,
    /// A clone copies the creation time (APFS).
    CopiedByClones = 3,
}

// --- §1.3: the resolver version -------------------------------------------------------------------------------

/// `RESOLVER_VERSION` = 1: the resolver version this module defines ([F20 §1.3]; [40 §2.6, §2.7]). 0 is invalid.
pub const RESOLVER_VERSION: u16 = 1;

// --- §1.2, §2.5: byte classes ----------------------------------------------------------------------------------

/// `WS`: the whitespace bytes HT, LF, VT, FF, CR and space ([F20 §1.2]).
pub const WS: [u8; 6] = [0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20];

/// The bytes besides `WS` that a trivial line may hold: `{ } ( ) [ ] ; ,` ([F20 §2.5] "Trivial line", [40 §2.7],
/// open point 4).
pub const TRIVIAL_LINE_EXTRA: [u8; 8] = *b"{}()[];,";

// --- §2.4: reading project content -----------------------------------------------------------------------------

/// `READ_RETRIES` = 1: an unstable two-pass read is repeated once from the start ([F20 §2.4], review A1P-05).
pub const READ_RETRIES: u32 = 1;

// --- §2.6: fingerprints ----------------------------------------------------------------------------------------

/// `FP_MIN_CHARS` = 3: a collapsed line of at most 3 characters is not a fingerprint line ([F20 §2.6.1],
/// [40 §2.5]).
pub const FP_MIN_CHARS: usize = 3;

/// `SKETCH_K` = 64: the sketch holds the 64 smallest sketch line hashes ([F20 §2.6.3], [40 §2.5] "bottom-64").
pub const SKETCH_K: usize = 64;

/// `SKETCH_BITS` = 32: the sketch line hash is the low 32 bits of XXH3-64 ([F20 §2.6.2], [40 §2.5] "u32").
pub const SKETCH_BITS: u32 = 32;

/// `TINY_LINES` = 5: text with fewer normalised lines is tiny ([F20 §2.6.5], [40 §4.4]).
pub const TINY_LINES: u64 = 5;

/// `TINY_BYTES` = 64: content with fewer (normalised) bytes is tiny ([F20 §2.6.5], [40 §4.4]).
pub const TINY_BYTES: u64 = 64;

// --- §2.7: the window ------------------------------------------------------------------------------------------

/// `WINDOW_BITS` = 16: the window hash is the low 16 bits of XXH3-64 ([F20 §2.7.1], [40 §2.7] "u16 hashes").
pub const WINDOW_BITS: u32 = 16;

/// `WIN` (`WINDOW_LINES`): non-trivial lines hashed on each side of an anchor's quote span ([F20 §2.7.2]).
///
/// HOLE(F20-window-lines), decided by replay row 2 (WP-76); draft 16 ([40 §2.7]). Constraint: `4 + 4 × WIN ≤ 68`.
pub const WINDOW_LINES: u16 = 16;

// --- §2.9, §2.10: winnowing and similarity -------------------------------------------------------------------

/// `WINNOW_K`: the winnowing k-gram length in tokens ([F20 §2.9], [10 §5.8b]).
///
/// HOLE(F20-winnow-k), decided by WP-66 with WP-76; draft 5.
pub const WINNOW_K: usize = 5;

/// `WINNOW_W`: the winnowing window in k-grams ([F20 §2.9], [10 §5.8b]).
///
/// HOLE(F20-winnow-w), decided by WP-66 with WP-76; draft 4.
pub const WINNOW_W: usize = 4;

/// `EXACT_LIMIT` = 65,536: the most fingerprint lines (or winnowing values) per side for the exact measures
/// ([F20 §2.10.5]); a resolver constant, never the key `files.max-line-hashes`.
pub const EXACT_LIMIT: u32 = 65_536;

// --- §4: candidate eligibility ---------------------------------------------------------------------------------

/// The never-candidate basename patterns of resolver version 1 ([F20 §4.7.2]; [40 §4.3], [80 §2.11.4] rule 5), in
/// the chapter's order. Syntax ([F20 §4.7.1]): whole-name match of the last component; `*` any byte sequence, `?`
/// one byte, every other byte ASCII case-insensitively; no other metacharacter. The contextual rules of
/// [F20 §4.7.3] are not patterns.
pub const NEVER_CANDIDATE_PATTERNS: [&[u8]; 21] = [
    b"*.tmp",
    b"*.tmp.*",
    b"*___jb_tmp___",
    b"*___jb_old___",
    b"*~",
    b"*.bak",
    b"*.orig",
    b"*.old",
    b"*.rej",
    b"*.swp",
    b"*.swo",
    b"4913",
    b".#*",
    b"~$*",
    b"sed??????",
    b"._*",
    b".DS_Store",
    b".fuse_hidden*",
    b".nfs*",
    b".goutputstream-*",
    b".~lock.*#",
];

/// `LIST_MAX` = 3: an `ambiguous` state lists at most 3 candidates and `FILEOBS` keeps at most 3 proposals
/// ([F20 §4.1], [40 §2.6, §4.4]).
pub const LIST_MAX: usize = 3;

// --- §5: the file cascade --------------------------------------------------------------------------------------

/// `SKEW`: the margin, in nanoseconds, of every comparison between a file timestamp, an `hlc` and a git committer time
/// ([F20 §5.1], review S-17): the largest difference between a file timestamp and the `hlc` wall time of the same
/// moment on one machine.
///
/// HOLE(F20-clock-skew), decided by measurements 15 and 22 (WP-55, WP-52): the smallest whole number of milliseconds at
/// least the measured maximum plus the volume's granularity. Chapter 20 gives it **no draft value**, so it is `None`
/// until WP-81a fills it, and no value is invented here; until then code that compares across clock domains takes the
/// margin as a parameter (at M0 only WP-92's model does: chapter 20's "Holes" table).
pub const SKEW: Option<u64> = None;

/// The `VolumeCaps.btime` class of NTFS volumes on Windows 11, which copy-rule line 2 reads ([F20 §5.9]).
///
/// HOLE(F20-btime-ntfs), decided by measurement 15 (WP-55, with the agent-machine copy paths of pass 1, S1-31); draft
/// [`BtimeTrust::TunneledNotCopied`] ([80 §2.11.1]); the other candidates are [`BtimeTrust::Unforgeable`] and
/// [`BtimeTrust::Absent`]. Constraint: `TunneledNotCopied` only if no measured tool gives a new file its source's
/// creation time; if one does, `Absent`.
pub const NTFS_BTIME: BtimeTrust = BtimeTrust::TunneledNotCopied;

/// Whether copy-rule line 2 requires q's ChangeTime clearly after the effective `verified_at` ([F20 §5.9], review
/// S-16).
///
/// HOLE(F20-ctime-rename), decided by measurement 15 (WP-55); draft `true` ("required"). Filled `false`, line 2 is
/// never `exact` on Windows.
pub const CTIME_RENAME_REQUIRED: bool = true;

/// `REPLACED_MAX` = 29/100, strict: both containment estimates below it make a changed file `replaced`
/// ([F20 §5.4.1], [40 §4.4]).
pub const REPLACED_MAX: Ratio = Ratio::new(29, 100);

/// `REPLACED_MIN_LINES` = 5: the `replaced` test needs both contents text with at least 5 normalised lines
/// ([F20 §5.4.1], [40 §4.4]).
pub const REPLACED_MIN_LINES: u64 = 5;

/// `PREFIX_MIN_NODES` = 2: nodes needed for sibling inference and an `observed` `path_moves` entry
/// ([F20 §5.10, §5.16], [40 §4.3, §4.4]).
pub const PREFIX_MIN_NODES: u32 = 2;

/// `E6_MAX_COMMITS` = 2,000: the longest E6 window, a resolver constant, not a budget ([F20 §5.11.2], [40 §4.3]).
pub const E6_MAX_COMMITS: u32 = 2_000;

/// `E6_SLACK_MS` = 86,400,000 (1 day): the slack of the time-bounded E6 window ([F20 §5.11.2], [40 §4.3]).
pub const E6_SLACK_MS: u64 = 86_400_000;

/// `SPLIT_MIN_PIECES` = 2: a split needs at least 2 pieces ([F20 §5.11.4] row 2, [40 §4.4]).
pub const SPLIT_MIN_PIECES: u32 = 2;

/// `SPLIT_NIO` = 4/5: a split piece's new-in-old containment ([F20 §5.11.4] row 2, [40 §4.4]).
pub const SPLIT_NIO: Ratio = Ratio::new(4, 5);

/// `SPLIT_SUM` = 3/5: the least sum of the pieces' old-in-new containments ([F20 §5.11.4] row 2, [40 §4.4]).
pub const SPLIT_SUM: Ratio = Ratio::new(3, 5);

/// `GS_CHUNK` = 64: the most bytes of one span of the git pair score's span counts; a span also ends at every `0A`
/// ([F20 §5.11.4] "The git pair score"; git 2.54.0 `diffcore-delta.c`).
pub const GS_CHUNK: u32 = 64;

/// `GS_HASHBASE` = 107,927: the modulus of the git pair score's span hash ([F20 §5.11.4]; git 2.54.0
/// `diffcore-delta.c`).
pub const GS_HASHBASE: u32 = 107_927;

/// `GS_TEXT_PREFIX` = 8,000: the leading bytes of a blob tested for `00` to decide whether the git pair score skips its
/// CR-before-LF bytes ([F20 §5.11.4]; git 2.54.0 `buffer_is_binary`). Only this score uses git's diff heuristic; the
/// content functions use `is_text` over the whole content ([F20 §2.1]).
pub const GS_TEXT_PREFIX: usize = 8_000;

/// `E6_STRONG` = 90: the git similarity index (%) of a `strong` E6 pair ([F20 §5.11.4] row 3, [40 §4.4]).
pub const E6_STRONG: u32 = 90;

/// `E6_WEAK`: a `weak` E6 pair has a git similarity index (%) from 20 to below 50 ([F20 §5.11.4] row 6,
/// [40 §4.4] "git pair 20–49 %"), confirmed by [`E6_WEAK_CONTAINMENT`].
pub const E6_WEAK: core::ops::Range<u32> = 20..50;

/// The containment that confirms a `weak` E6 pair: `max(oin, nio) ≥ 4/5` ([F20 §5.11.4] row 6, §7 `E6_WEAK`).
pub const E6_WEAK_CONTAINMENT: Ratio = Ratio::new(4, 5);

/// `E8_MIN` = 4/5, both directions: the containment estimates of an E8 candidate ([F20 §5.13], [40 §4.3]).
pub const E8_MIN: Ratio = Ratio::new(4, 5);

/// `STAGE1_TOP` = 10: candidates kept by stage 1 of the similarity search ([F20 §5.14], [40 §4.3]).
pub const STAGE1_TOP: usize = 10;

/// `SIM_STRONG` = 1/2: the least pair score of a `strong` similarity candidate ([F20 §5.15], [40 §4.4]).
pub const SIM_STRONG: Ratio = Ratio::new(1, 2);

/// `SIM_MARGIN` = 1/5: the least margin over the runner-up ([F20 §5.15], [40 §4.4]).
pub const SIM_MARGIN: Ratio = Ratio::new(1, 5);

/// `SIM_WEAK` = 3/10: the least pair score of a `weak` candidate ([F20 §5.15], [40 §4.4]).
pub const SIM_WEAK: Ratio = Ratio::new(3, 10);

/// `MERGED_OIN` = 4/5: the old-in-new containment of a merge into a host file ([F20 §5.15], [40 §4.4]).
pub const MERGED_OIN: Ratio = Ratio::new(4, 5);

/// `MERGED_NIO_MAX` = 1/2, strict: the new-in-old containment of a merge stays below it ([F20 §5.15],
/// [40 §4.4]).
pub const MERGED_NIO_MAX: Ratio = Ratio::new(1, 2);

/// `QUIESCE_NS` = 50,000,000 (50 ms) on the process's monotonic clock ([F20 §5.17], [40 §4.1] P10).
pub const QUIESCE_NS: u64 = 50_000_000;

// --- §6: anchors -----------------------------------------------------------------------------------------------

/// `QUOTE_LINES`: the most non-trivial lines of a `quote` span ([F20 §6.1] step 2).
///
/// HOLE(F20-quote-lines), decided by replay row 2 (WP-76); draft 4 ([40 §2.7]).
pub const QUOTE_LINES: u32 = 4;

/// `QUOTE_MAX`: the most bytes of a `quote` span and of a header quote ([F20 §6.1] steps 2–3).
///
/// HOLE(F20-quote-max), decided by replay row 2 (WP-76); draft 128 ([40 §2.7]).
pub const QUOTE_MAX: usize = 128;

/// `QUOTE_DEFAULT`: the length of a `range` anchor's start and end quotes ([F20 §6.1] step 3, open point 19).
///
/// HOLE(F20-quote-default), decided by replay row 2 (WP-76); draft 64 ([40 §2.7]).
pub const QUOTE_DEFAULT: usize = 64;

/// `CONTEXT`: prefix and suffix length at capture ([F20 §6.1] step 5).
///
/// HOLE(F20-context), decided by replay row 2 (WP-76); draft 32 ([40 §2.7]).
pub const CONTEXT: usize = 32;

/// `CONTEXT_MAX`: the widened prefix and suffix length of the uniqueness ladder ([F20 §6.1] step 8).
///
/// HOLE(F20-context-max), decided by replay row 2 (WP-76); draft 64 ([40 §2.7]). Constraint: `≥ CONTEXT`.
pub const CONTEXT_MAX: usize = 64;

/// `CONTEXT_MARGIN`: the context-score margin that breaks a duplicate-quote tie ([F20 §6.2] step 6.1).
///
/// HOLE(F20-context-margin), decided by replay row 2 (WP-76); draft 1/10 ([40 §4.5]).
pub const CONTEXT_MARGIN: Ratio = Ratio::new(1, 10);

/// `WINDOW_MARGIN`: the window-score margin for duplicate quotes and `lines` anchors ([F20 §6.2] step 6.2, §6.5).
///
/// HOLE(F20-window-margin), decided by replay row 2 (WP-76); draft 15/100 ([40 §4.5]).
pub const WINDOW_MARGIN: Ratio = Ratio::new(15, 100);

/// `RANGE_SPREAD`: how far after its start quote a range's end quote may lie, in multiples of the captured hint
/// length in lines ([F20 §6.2] step 4, open point 20).
///
/// HOLE(F20-range-spread), decided by replay row 2 (WP-76); draft 2 ([40 §4.5]).
pub const RANGE_SPREAD: u32 = 2;

/// `FUZZY_BUDGET`: the Myers error budget per quote byte, `k = ⌊FUZZY_BUDGET × len(exact)⌋` ([F20 §6.4]).
///
/// HOLE(F20-fuzzy-budget), decided by replay row 2 (WP-76); draft 1/4 ([40 §4.5]). Constraint: `≤ 1/4`.
pub const FUZZY_BUDGET: Ratio = Ratio::new(1, 4);

/// `SPAN`: bytes of N′ searched on each side of the hint lines by the first fuzzy region ([F20 §6.4], open
/// point 21).
///
/// HOLE(F20-fuzzy-span), decided by replay row 2 (WP-76); draft 16,384 ([40 §4.5] "±16 KB").
pub const SPAN: usize = 16_384;

/// The fuzzy-score weights (quote, prefix, suffix, window) `(w1, w2, w3, w4)` ([F20 §6.4]).
///
/// HOLE(F20-fuzzy-weights), decided by replay row 2 (WP-76); draft (50, 20, 20, 10) ([40 §4.5]). Constraint:
/// non-negative integers with a positive sum.
pub const FUZZY_WEIGHTS: [u32; 4] = [50, 20, 20, 10];

/// `FUZZY_ACCEPT`: the least quote similarity `q` of a fuzzy candidate ([F20 §6.4]).
///
/// HOLE(F20-fuzzy-accept), decided by replay row 2 (WP-76); draft 3/4 ([40 §4.5]). Constraint:
/// `≥ 1 − FUZZY_BUDGET`.
pub const FUZZY_ACCEPT: Ratio = Ratio::new(3, 4);

/// `FUZZY_MARGIN`: the top-2 score margin of an accepted fuzzy match ([F20 §6.4]).
///
/// HOLE(F20-fuzzy-margin), decided by replay row 2 (WP-76); draft 2/100 ([40 §4.5]).
pub const FUZZY_MARGIN: Ratio = Ratio::new(2, 100);

/// `HEADER_MARGIN`: the top-2 margin of a same-kind header match when the scope did not resolve ([F20 §6.4]).
///
/// HOLE(F20-header-margin), decided by replay row 2 (WP-76); draft 1/10 ([40 §4.5], [41 m8]).
pub const HEADER_MARGIN: Ratio = Ratio::new(1, 10);

/// `LINES_MIN`: the least window score that aligns a `lines` anchor ([F20 §6.5], open point 23).
///
/// HOLE(F20-lines-min), decided by replay row 2 (WP-76) and P11's generated cases (WP-77); draft 1/2.
pub const LINES_MIN: Ratio = Ratio::new(1, 2);

/// Whether [F20 §6.1]'s **interim scanner rule** holds: no capture records a scope, the `symbol` and `heading` forms
/// are refused, and an anchor that carries a scope or is of kind `symbol` or `heading` resolves without scanner steps
/// (no scope region, no same-kind header restriction, no scope-only step).
///
/// `true` for resolver version 1 as the chapter stands: OQ-R-2 (decided 2026-09-28) keeps the rule until review accepts
/// \[F21\] (\[F20\] open point 30, [F21 §1.4] and its open point 1). Lifting it before the `format-v1` tag is part of
/// what version 1 says at the tag ([F20 §1.3]); after the tag it is a new resolver version. The anchor module's
/// scanner steps ([F21 §2.4]–§2.6, §6) are built and tested for both values.
pub const INTERIM_SCANNER_RULE: bool = true;

// The constraints a hole's value must meet ([F20] "Holes"), checked at compile time so that a fill breaking one does
// not build.
const _: () = {
    // F20-window-lines: `4 + 4 × WIN ≤ 68` ([40 §2.7]'s ≤ 68-byte window value).
    assert!(4 + 4 * (WINDOW_LINES as u32) <= 68);
    // F20-context-max: `≥ F20-context`.
    assert!(CONTEXT_MAX >= CONTEXT);
    // Open point 19: the start and end quotes of a range are no longer than a whole quote.
    assert!(QUOTE_DEFAULT <= QUOTE_MAX);
    // F20-fuzzy-weights: non-negative integers with a positive sum. The anchor module also needs the sum below 2^16,
    // so that a fuzzy score's common denominator fits `u128` for every quote a resolver-version-1 capture writes.
    assert!(
        FUZZY_WEIGHTS[0] as u64
            + FUZZY_WEIGHTS[1] as u64
            + FUZZY_WEIGHTS[2] as u64
            + FUZZY_WEIGHTS[3] as u64
            > 0
    );
    assert!(
        FUZZY_WEIGHTS[0] as u64
            + FUZZY_WEIGHTS[1] as u64
            + FUZZY_WEIGHTS[2] as u64
            + FUZZY_WEIGHTS[3] as u64
            <= u16::MAX as u64
    );
    // F20-fuzzy-budget: `≤ 1/4`.
    assert!(FUZZY_BUDGET.num as u128 * 4 <= FUZZY_BUDGET.den as u128);
    // F20-fuzzy-accept: `≥ 1 − F20-fuzzy-budget`.
    assert!(
        FUZZY_ACCEPT.num as u128 * FUZZY_BUDGET.den as u128
            >= (FUZZY_BUDGET.den - FUZZY_BUDGET.num) as u128 * FUZZY_ACCEPT.den as u128
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_compares_values_exactly() {
        assert_eq!(Ratio::new(1, 2), Ratio::new(2, 4));
        assert!(Ratio::new(29, 100) < Ratio::new(3, 10));
        assert!(Ratio::new(u64::MAX, u64::MAX - 1) > Ratio::ONE);
        assert!(Ratio::new(u64::MAX - 1, u64::MAX) < Ratio::ONE);
        assert_eq!(Ratio::or_zero(5, 0), Ratio::ZERO);
        assert_eq!(Ratio::or_zero(3, 6), SIM_STRONG);
        assert_eq!(FUZZY_BUDGET.complement(), FUZZY_ACCEPT);
        assert_eq!(Ratio::new(7, 5).complement(), Ratio::ZERO);
    }

    #[test]
    fn hole_drafts_meet_their_constraints() {
        // The same constraints as the compile-time block, through `Ratio`'s order.
        assert!(FUZZY_BUDGET <= Ratio::new(1, 4));
        assert!(FUZZY_ACCEPT >= FUZZY_BUDGET.complement());
    }

    #[test]
    fn btime_trust_mirrors_os_project() {
        // [OS/project §4.2]: `Absent | TunneledNotCopied | Unforgeable | CopiedByClones`, in that order.
        let all = [
            BtimeTrust::Absent,
            BtimeTrust::TunneledNotCopied,
            BtimeTrust::Unforgeable,
            BtimeTrust::CopiedByClones,
        ];
        for (i, t) in all.into_iter().enumerate() {
            assert_eq!(usize::from(t as u8), i);
        }
        assert_eq!(NTFS_BTIME, BtimeTrust::TunneledNotCopied);
    }

    #[test]
    fn fixed_values_match_the_table() {
        assert_eq!(RESOLVER_VERSION, 1);
        assert_eq!(1u64 << WINDOW_BITS, 65_536);
        assert_eq!(SKETCH_BITS, 32);
        assert_eq!(E6_WEAK.start, 20);
        assert_eq!(E6_WEAK.end, 50);
        assert!(E6_WEAK.contains(&49) && !E6_WEAK.contains(&50));
        assert!(REPLACED_MAX < SIM_WEAK && SIM_WEAK < SIM_STRONG);
        assert_eq!(NEVER_CANDIDATE_PATTERNS.len(), 21);
        assert_eq!(
            (GS_CHUNK, GS_HASHBASE, GS_TEXT_PREFIX),
            (64, 107_927, 8_000)
        );
        // WS and the trivial-line extras are disjoint.
        assert!(TRIVIAL_LINE_EXTRA.iter().all(|b| !WS.contains(b)));
    }
}
