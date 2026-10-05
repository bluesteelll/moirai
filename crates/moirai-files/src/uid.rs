//! The derived identities of R4 ([F08 §11]; [40 §2.3, §2.7]; [40 §2.11] R-3, R-5): file uids with their predecessor
//! order and the dead-uid rule, root-node uids, and anchor uids with the capture digest `captured`.
//!
//! Every derivation is BLAKE3-128 ([F01 §7.1]) over `lp()`-framed arguments ([F01 §6.3]):
//! `lp(x) = u32-le(len(x)) ‖ x`, so no two argument lists collide by concatenation. Uids derive from exact bytes and
//! are never case-folded ([40 §2.3]). The view lookups the procedures need (which uids a view holds) are the caller's,
//! passed in as functions, so the derivation is the same for the engine and for every store that reads the same view.

use core::fmt;
use core::num::NonZeroU16;

/// A node uid: 16 bytes, compared bytewise, written as 32 lower-case hexadecimal digits ([F08 §2.2], [F01 §6.4]).
#[derive(Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct Uid(pub [u8; 16]);

impl Uid {
    /// The all-zero value, which is never a node's uid ([F08 §2.2]).
    pub const ZERO: Uid = Uid([0; 16]);

    /// Whether this is the all-zero value.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 16]
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uid({self})")
    }
}

/// The domain string of file uids ([F08 §11.1]).
pub const FILE_DOMAIN: &str = "moirai-file-v1";
/// The domain string of root-node uids ([F08 §11.1]).
pub const ROOT_DOMAIN: &str = "moirai-root-v1";
/// The domain string of anchor uids ([F08 §11.1]).
pub const ANCHOR_DOMAIN: &str = "moirai-anchor-v1";

/// Why a derivation is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UidError {
    /// An argument of 2^32 bytes or more cannot be framed by `lp()` ([F01 §6.3]).
    ArgumentTooLong,
    /// The derivation gave the all-zero uid: refused as a collision ([F08 §2.2], [F19 §10.2] `uid_collision`).
    Zero,
    /// A dead-uid loop ran more times than the number of nodes of the view it could name, tombstones included (for an
    /// anchor, the anchors on the edge): only a BLAKE3 collision causes it; an internal error (exit 1, [F08 §11.2]
    /// step 4, §11.4 step 3).
    LoopBound,
}

impl fmt::Display for UidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UidError::ArgumentTooLong => "a uid derivation argument of 4 GiB or more",
            UidError::Zero => "a uid derivation gave the all-zero uid (collision)",
            UidError::LoopBound => "a uid derivation loop exceeded its bound (collision)",
        })
    }
}

impl std::error::Error for UidError {}

/// A BLAKE3 hasher fed with `lp()`-framed arguments.
struct LpHasher(blake3::Hasher);

impl LpHasher {
    fn new() -> LpHasher {
        LpHasher(blake3::Hasher::new())
    }

    fn lp(&mut self, x: &[u8]) -> Result<&mut LpHasher, UidError> {
        let len = u32::try_from(x.len()).map_err(|_| UidError::ArgumentTooLong)?;
        self.0.update(&len.to_le_bytes());
        self.0.update(x);
        Ok(self)
    }

    /// BLAKE3-128: the first 16 bytes of the BLAKE3 output ([F01 §7.1]).
    fn finish128(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out.copy_from_slice(&self.0.finalize().as_bytes()[..16]);
        out
    }

    fn finish_uid(&self) -> Result<Uid, UidError> {
        let u = Uid(self.finish128());
        if u.is_zero() {
            Err(UidError::Zero)
        } else {
            Ok(u)
        }
    }
}

fn opt(u: Option<&Uid>) -> &[u8] {
    u.map_or(&[], |u| &u.0[..])
}

/// `uid_file(r, p, q) = BLAKE3-128(lp("moirai-file-v1") ‖ lp(r) ‖ lp(p) ‖ lp(q or empty))` ([F08 §11.2],
/// [40 §2.3]): r the root name, p the registration path (exact bytes, without its root), q the predecessor uid.
///
/// # Errors
/// [`UidError::ArgumentTooLong`] for an argument of 4 GiB or more; [`UidError::Zero`] for an all-zero result.
pub fn uid_file(root: &str, path: &str, pred: Option<&Uid>) -> Result<Uid, UidError> {
    LpHasher::new()
        .lp(FILE_DOMAIN.as_bytes())?
        .lp(root.as_bytes())?
        .lp(path.as_bytes())?
        .lp(opt(pred))?
        .finish_uid()
}

/// `uid_root(r) = BLAKE3-128(lp("moirai-root-v1") ‖ lp(r))` ([F08 §11.3], [40 §2.3], R-5).
///
/// # Errors
/// [`UidError::ArgumentTooLong`] for a name of 4 GiB or more; [`UidError::Zero`] for an all-zero result.
pub fn uid_root(root: &str) -> Result<Uid, UidError> {
    LpHasher::new()
        .lp(ROOT_DOMAIN.as_bytes())?
        .lp(root.as_bytes())?
        .finish_uid()
}

/// The predecessor term of a file registration ([F08 §11.2] step 2, [40 §2.3], review S-01): among the candidates —
/// the artifacts of the view with root r that once held p and do not hold it now (live `removed` nodes whose `path`
/// is p, and live nodes whose `aliases` contain p; never tombstones) — the one with the bytewise greatest uid, or
/// none.
#[must_use]
pub fn predecessor<'a>(candidates: impl IntoIterator<Item = &'a Uid>) -> Option<Uid> {
    candidates.into_iter().max().copied()
}

/// The result of a file-uid derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileKey {
    /// The uid of the new file node.
    pub uid: Uid,
    /// Its `origin_pred`: the last predecessor term used, absent when empty ([F08 §11.2] step 5).
    pub origin_pred: Option<Uid>,
}

/// Steps 2–4 of a file registration on a view V ([F08 §11.2]; [40 §2.3]).
///
/// Step 1 is the caller's: when V holds a live artifact of root r with `path` = p and status `present` or `planned`,
/// that node is the file node and nothing is derived. Otherwise: q = [`predecessor`] of `candidates`;
/// u = `uid_file(r, p, q)`; while u names any node of V (`names_node`: live at another path, `removed`, or a
/// tombstone), q = u and u is derived again. The loop reads V alone, so every store derives the same uid for the same
/// file, path and view.
///
/// `nameable_nodes` is the bound of [F08 §11.2] step 4: the number of nodes of V that u could name, tombstones
/// included; a loop that would run more times is refused. Each round names a node of V that no earlier round named
/// unless BLAKE3 collides, so without a collision the loop ends within the bound. A caller may pass any count that
/// covers the nodes `names_node` can report, such as the number of nodes of V.
///
/// # Errors
/// [`UidError::LoopBound`] when the loop would run more than `nameable_nodes` times; the errors of [`uid_file`].
pub fn derive_file_uid<'a>(
    root: &str,
    path: &str,
    candidates: impl IntoIterator<Item = &'a Uid>,
    mut names_node: impl FnMut(&Uid) -> bool,
    nameable_nodes: u64,
) -> Result<FileKey, UidError> {
    let mut q = predecessor(candidates);
    let mut u = uid_file(root, path, q.as_ref())?;
    let mut rounds = 0u64;
    while names_node(&u) {
        if rounds >= nameable_nodes {
            return Err(UidError::LoopBound);
        }
        rounds += 1;
        q = Some(u);
        u = uid_file(root, path, q.as_ref())?;
    }
    Ok(FileKey {
        uid: u,
        origin_pred: q,
    })
}

/// The anchor kinds of the anchor record ([F08 §10.3]); their names enter `captured` ([F08 §11.1]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum AnchorKind {
    /// 1 `file`.
    File = 1,
    /// 2 `heading`.
    Heading = 2,
    /// 3 `symbol`.
    Symbol = 3,
    /// 4 `quote`.
    Quote = 4,
    /// 5 `range`.
    Range = 5,
    /// 6 `lines`.
    Lines = 6,
}

impl AnchorKind {
    /// The record's `kind` byte.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The kind of record byte `b`.
    #[must_use]
    pub const fn from_code(b: u8) -> Option<AnchorKind> {
        match b {
            1 => Some(AnchorKind::File),
            2 => Some(AnchorKind::Heading),
            3 => Some(AnchorKind::Symbol),
            4 => Some(AnchorKind::Quote),
            5 => Some(AnchorKind::Range),
            6 => Some(AnchorKind::Lines),
            _ => None,
        }
    }

    /// The name, whose ASCII bytes are the argument of `lp(kind)` ([F08 §11.1], [F01 §6.3]).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            AnchorKind::File => "file",
            AnchorKind::Heading => "heading",
            AnchorKind::Symbol => "symbol",
            AnchorKind::Quote => "quote",
            AnchorKind::Range => "range",
            AnchorKind::Lines => "lines",
        }
    }

    /// Whether the kind carries a quote, a prefix and a suffix: `heading`, `symbol`, `quote` and `range` ([F08 §10.3]
    /// orders 12–17, I-F9).
    #[must_use]
    pub const fn has_quote(self) -> bool {
        matches!(
            self,
            AnchorKind::Heading | AnchorKind::Symbol | AnchorKind::Quote | AnchorKind::Range
        )
    }

    /// Whether the kind carries an end quote: `range` only ([F08 §10.3] orders 18–19).
    #[must_use]
    pub const fn has_end(self) -> bool {
        matches!(self, AnchorKind::Range)
    }
}

/// The inputs of the capture digest `captured` ([F08 §11.1, §11.4]; [40 §2.7]).
///
/// The text fields and the window enter only for the kinds that carry them ([F08 §11.1]): [`captured`] reads `quote`,
/// `prefix` and `suffix` for a kind with [`AnchorKind::has_quote`], `end` for `range` and `window` for `lines`, and
/// enters every other one as empty, whatever it holds.
#[derive(Clone, Copy, Debug)]
pub struct Capture<'a> {
    /// The file node's uid at capture.
    pub file_uid: &'a Uid,
    /// The anchor kind.
    pub kind: AnchorKind,
    /// The scope value's bytes ([F08 §10.3.1]); empty when the anchor has none.
    pub scope: &'a [u8],
    /// `quote.exact`: bytes of N ([F20 §6.1]); it enters for `heading`, `symbol`, `quote` and `range` (the start
    /// quote).
    pub quote: &'a [u8],
    /// `prefix.exact`, as widened by the uniqueness ladder; may be empty. It enters as `quote` does.
    pub prefix: &'a [u8],
    /// `suffix.exact`, as widened by the uniqueness ladder; may be empty. It enters as `quote` does.
    pub suffix: &'a [u8],
    /// `end.exact`, the end quote; it enters for `range` only.
    pub end: &'a [u8],
    /// The 1-based occurrence index, when recorded ([F20 §6.1] step 8.3). [F08 §10.3] requires `occurrence` ≥ 1, so
    /// the type rules out a 0 that no valid anchor record can carry.
    pub occurrence: Option<NonZeroU16>,
    /// The window value W ([F20 §2.7.3]); it enters only for a `lines` anchor.
    pub window: &'a [u8],
}

/// `captured = BLAKE3-128(lp(file uid at capture) ‖ lp(kind) ‖ lp(scope) ‖ lp(quote.exact) ‖ lp(prefix.exact) ‖
/// lp(suffix.exact) ‖ lp(end.exact) ‖ lp(occurrence) ‖ lp(window if kind = lines, else empty))` ([F08 §11.4],
/// [40 §2.7], review S-03). The occurrence enters as its `u16` little-endian (2 bytes), or empty when absent.
///
/// [F08 §11.1]: `quote.exact`, `prefix.exact`, `suffix.exact` and `end.exact` are "empty when the kind has none", and
/// the window is empty for a kind other than `lines`; a field the kind does not carry enters as `lp("")` whatever
/// [`Capture`] holds ([`AnchorKind::has_quote`], [`AnchorKind::has_end`]).
///
/// # Errors
/// [`UidError::ArgumentTooLong`] for an argument of 4 GiB or more.
pub fn captured(c: &Capture<'_>) -> Result<[u8; 16], UidError> {
    /// `v` when the kind carries the field, else the empty argument.
    fn pick(carried: bool, v: &[u8]) -> &[u8] {
        if carried { v } else { &[] }
    }
    let occ = c.occurrence.map(|o| o.get().to_le_bytes());
    let occ: &[u8] = occ.as_ref().map_or(&[], |b| &b[..]);
    let texts = c.kind.has_quote();
    let window = pick(c.kind == AnchorKind::Lines, c.window);
    Ok(LpHasher::new()
        .lp(&c.file_uid.0)?
        .lp(c.kind.name().as_bytes())?
        .lp(c.scope)?
        .lp(pick(texts, c.quote))?
        .lp(pick(texts, c.prefix))?
        .lp(pick(texts, c.suffix))?
        .lp(pick(c.kind.has_end(), c.end))?
        .lp(occ)?
        .lp(window)?
        .finish128())
}

/// `uid_anchor(s, c, p) = BLAKE3-128(lp("moirai-anchor-v1") ‖ lp(s) ‖ lp(c) ‖ lp(p or empty))` ([F08 §11.4],
/// [40 §2.7]): s the referrer's uid, c = `captured`, p the predecessor term.
///
/// # Errors
/// [`UidError::Zero`] for an all-zero result.
pub fn uid_anchor(src: &Uid, captured: &[u8; 16], pred: Option<&Uid>) -> Result<Uid, UidError> {
    LpHasher::new()
        .lp(ANCHOR_DOMAIN.as_bytes())?
        .lp(&src.0)?
        .lp(captured)?
        .lp(opt(pred))?
        .finish_uid()
}

/// The result of an anchor-uid derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnchorKey {
    /// The anchor uid.
    pub uid: Uid,
    /// The stored `pred`: present when the loop ran ([F08 §11.4] step 4).
    pub pred: Option<Uid>,
}

/// Steps 2–4 of an anchor capture on the edge (s, `at`, f) of a view V ([F08 §11.4]; [40 §2.7]).
///
/// Step 1 is the caller's: an anchor on (s, f) whose current selectors equal the capture's is reused and nothing is
/// derived. Otherwise u = `uid_anchor(s, captured, empty)`; while u is the uid of an anchor on (s, f) in V
/// (`names_anchor`), p = u and u is derived again.
///
/// `named_anchors` is the number of anchors on (s, f) in V: the bound of [F08 §11.2] step 4 applied to V's anchors on
/// (s, f) ([F08 §11.4] step 3).
///
/// # Errors
/// [`UidError::LoopBound`] when the loop would run more than `named_anchors` times; the errors of [`uid_anchor`].
pub fn derive_anchor_uid(
    src: &Uid,
    captured: &[u8; 16],
    mut names_anchor: impl FnMut(&Uid) -> bool,
    named_anchors: u64,
) -> Result<AnchorKey, UidError> {
    let mut p = None;
    let mut u = uid_anchor(src, captured, None)?;
    let mut rounds = 0u64;
    while names_anchor(&u) {
        if rounds >= named_anchors {
            return Err(UidError::LoopBound);
        }
        rounds += 1;
        p = Some(u);
        u = uid_anchor(src, captured, p.as_ref())?;
    }
    Ok(AnchorKey { uid: u, pred: p })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b3_128(bytes: &[u8]) -> Uid {
        let mut out = [0u8; 16];
        out.copy_from_slice(&blake3::hash(bytes).as_bytes()[..16]);
        Uid(out)
    }

    #[test]
    fn file_uid_frames_as_the_spec_shows() {
        // [F08 §11.2]'s informative input for root `project`, path `docs/a.md` and no predecessor: 46 bytes.
        let mut want = Vec::new();
        want.extend_from_slice(&[0x0E, 0, 0, 0]);
        want.extend_from_slice(b"moirai-file-v1");
        want.extend_from_slice(&[0x07, 0, 0, 0]);
        want.extend_from_slice(b"project");
        want.extend_from_slice(&[0x09, 0, 0, 0]);
        want.extend_from_slice(b"docs/a.md");
        want.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(want.len(), 46);
        assert_eq!(uid_file("project", "docs/a.md", None), Ok(b3_128(&want)));

        let q = Uid([0xAB; 16]);
        want.truncate(42);
        want.extend_from_slice(&[16, 0, 0, 0]);
        want.extend_from_slice(&q.0);
        assert_eq!(
            uid_file("project", "docs/a.md", Some(&q)),
            Ok(b3_128(&want))
        );
        // Exact bytes: case is never folded.
        assert_ne!(
            uid_file("project", "docs/A.md", None),
            uid_file("project", "docs/a.md", None)
        );
    }

    #[test]
    fn root_and_anchor_uids() {
        let mut want = Vec::new();
        want.extend_from_slice(&[0x0E, 0, 0, 0]);
        want.extend_from_slice(b"moirai-root-v1");
        want.extend_from_slice(&[5, 0, 0, 0]);
        want.extend_from_slice(b"notes");
        assert_eq!(uid_root("notes"), Ok(b3_128(&want)));

        let src = Uid([1; 16]);
        let c = [2u8; 16];
        let mut want = Vec::new();
        want.extend_from_slice(&[16, 0, 0, 0]);
        want.extend_from_slice(b"moirai-anchor-v1");
        want.extend_from_slice(&[16, 0, 0, 0]);
        want.extend_from_slice(&src.0);
        want.extend_from_slice(&[16, 0, 0, 0]);
        want.extend_from_slice(&c);
        want.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(uid_anchor(&src, &c, None), Ok(b3_128(&want)));
    }

    #[test]
    fn captured_frames_every_argument() {
        let f = Uid([9; 16]);
        let base = Capture {
            file_uid: &f,
            kind: AnchorKind::Quote,
            scope: b"",
            quote: b"return result",
            prefix: b"",
            suffix: b"",
            end: b"",
            occurrence: None,
            window: b"\x00\x00\x00\x00",
        };
        let mut want = Vec::new();
        let args: [&[u8]; 9] = [
            &f.0,
            b"quote",
            b"",
            b"return result",
            b"",
            b"",
            b"",
            b"",
            b"",
        ];
        for arg in args {
            want.extend_from_slice(&u32::try_from(arg.len()).unwrap().to_le_bytes());
            want.extend_from_slice(arg);
        }
        let got = captured(&base).unwrap();
        assert_eq!(got, b3_128(&want).0);

        // The window enters only for `lines`; the occurrence as 2 bytes little-endian.
        let lines = Capture {
            kind: AnchorKind::Lines,
            quote: b"",
            ..base
        };
        assert_ne!(
            captured(&lines).unwrap(),
            captured(&Capture {
                window: b"",
                ..lines
            })
            .unwrap()
        );
        assert_eq!(
            captured(&base).unwrap(),
            captured(&Capture {
                window: b"",
                ..base
            })
            .unwrap()
        );
        let mut want2 = want.clone();
        let occ_at = want2.len() - 8; // lp(occurrence) and lp(window) are the last two empty frames
        want2.splice(occ_at..occ_at + 4, [2, 0, 0, 0, 0x03, 0x01]);
        assert_eq!(
            captured(&Capture {
                occurrence: NonZeroU16::new(0x0103),
                ..base
            })
            .unwrap(),
            b3_128(&want2).0
        );
        // Moving bytes between arguments changes the digest (length framing).
        assert_ne!(
            captured(&Capture {
                quote: b"return",
                prefix: b" result",
                ..base
            })
            .unwrap(),
            captured(&Capture {
                quote: b"return ",
                prefix: b"result",
                ..base
            })
            .unwrap()
        );
    }

    /// [F08 §11.1]: a text field the kind does not carry, and a window of a kind other than `lines`, enter as empty.
    #[test]
    fn fields_enter_only_for_the_kinds_that_carry_them() {
        let f = Uid([4; 16]);
        let empty = |kind| Capture {
            file_uid: &f,
            kind,
            scope: b"",
            quote: b"",
            prefix: b"",
            suffix: b"",
            end: b"",
            occurrence: None,
            window: b"",
        };
        let full = |kind| Capture {
            quote: b"start",
            prefix: b"pre",
            suffix: b"suf",
            end: b"stop",
            window: b"\x01\x00\x01\x00\x11\x11",
            ..empty(kind)
        };
        for k in [
            AnchorKind::File,
            AnchorKind::Heading,
            AnchorKind::Symbol,
            AnchorKind::Quote,
            AnchorKind::Range,
            AnchorKind::Lines,
        ] {
            let carried =
                |field: Capture<'_>| captured(&field).unwrap() != captured(&empty(k)).unwrap();
            let base = empty(k);
            assert_eq!(
                carried(Capture {
                    quote: b"q",
                    ..base
                }),
                k.has_quote(),
                "{k:?} quote"
            );
            assert_eq!(
                carried(Capture {
                    prefix: b"p",
                    ..base
                }),
                k.has_quote(),
                "{k:?} prefix"
            );
            assert_eq!(
                carried(Capture {
                    suffix: b"s",
                    ..base
                }),
                k.has_quote(),
                "{k:?} suffix"
            );
            assert_eq!(
                carried(Capture { end: b"e", ..base }),
                k.has_end(),
                "{k:?} end"
            );
            assert_eq!(
                carried(Capture {
                    window: b"\x01",
                    ..base
                }),
                k == AnchorKind::Lines,
                "{k:?} window"
            );
            // Always entered: the scope and the occurrence.
            assert!(carried(Capture {
                scope: b"\x01\x01\x01\x01a\x00",
                ..base
            }));
            assert!(carried(Capture {
                occurrence: NonZeroU16::new(1),
                ..base
            }));
        }
        // A field a kind does not carry is ignored whatever it holds.
        assert_eq!(
            captured(&full(AnchorKind::File)).unwrap(),
            captured(&empty(AnchorKind::File)).unwrap()
        );
        assert_eq!(
            captured(&full(AnchorKind::Quote)).unwrap(),
            captured(&Capture {
                end: b"",
                window: b"",
                ..full(AnchorKind::Quote)
            })
            .unwrap()
        );
        assert_eq!(
            captured(&full(AnchorKind::Lines)).unwrap(),
            captured(&Capture {
                window: full(AnchorKind::Lines).window,
                ..empty(AnchorKind::Lines)
            })
            .unwrap()
        );
        assert!(!AnchorKind::File.has_quote() && !AnchorKind::Lines.has_quote());
        assert!(AnchorKind::Range.has_end() && !AnchorKind::Quote.has_end());
    }

    /// Pinned digests of store-permanent identities: a change to any derivation breaks these. The values were computed
    /// by an independent single-chunk BLAKE3 written from the BLAKE3 specification (checked against its published
    /// vectors for the empty input, `abc` and the 63-, 64-, 65-, 1,023- and 1,024-byte inputs), not by this code.
    #[test]
    fn pinned_digests() {
        let hex = |u: Uid| u.to_string();
        let u0 = uid_file("project", "docs/a.md", None).unwrap();
        assert_eq!(hex(u0), "d8253f84275a6c5b2fe5dbdb2bc28532");
        assert_eq!(
            hex(uid_file("project", "docs/a.md", Some(&u0)).unwrap()),
            "1911e72f0dc27f230f6e8a66a125dca9"
        );
        let root = uid_root("project").unwrap();
        assert_eq!(hex(root), "5bd0e29e6afc4a73557e4cbdf7d34c33");
        assert_eq!(
            hex(uid_root("notes").unwrap()),
            "780761a1ade83a9bcb0d80b111176710"
        );
        let c = captured(&Capture {
            file_uid: &u0,
            kind: AnchorKind::Quote,
            scope: b"",
            quote: b"return result",
            prefix: b"",
            suffix: b"",
            end: b"",
            occurrence: None,
            window: b"",
        })
        .unwrap();
        assert_eq!(hex(Uid(c)), "64652d9777b12e1d5ecef031a8bd1835");
        let a0 = uid_anchor(&root, &c, None).unwrap();
        assert_eq!(hex(a0), "ce9340d68ca25c35a25a4d480ef29271");
        assert_eq!(
            hex(uid_anchor(&root, &c, Some(&a0)).unwrap()),
            "8b82dc3ef4301b8d987d49a6e11524b3"
        );
    }

    #[test]
    fn predecessor_is_the_greatest_uid() {
        let a = Uid([1; 16]);
        let mut b = [1u8; 16];
        b[15] = 2;
        let b = Uid(b);
        let c = Uid([0; 16]);
        assert_eq!(predecessor([&a, &b, &c]), Some(b));
        assert_eq!(predecessor([&c, &b, &a]), Some(b));
        assert_eq!(predecessor([]), None);
    }

    #[test]
    fn dead_uids_are_never_recreated() {
        let u0 = uid_file("project", "a.rs", None).unwrap();
        let u1 = uid_file("project", "a.rs", Some(&u0)).unwrap();
        let u2 = uid_file("project", "a.rs", Some(&u1)).unwrap();
        // Nothing on the view: the plain derivation.
        assert_eq!(
            derive_file_uid("project", "a.rs", [], |_| false, 0),
            Ok(FileKey {
                uid: u0,
                origin_pred: None
            })
        );
        // u0 and u1 are dead on the view: two re-derivations.
        let dead = [u0, u1];
        assert_eq!(
            derive_file_uid("project", "a.rs", [], |u| dead.contains(u), 5),
            Ok(FileKey {
                uid: u2,
                origin_pred: Some(u1)
            })
        );
        // The predecessor starts the chain.
        let k = derive_file_uid("project", "a.rs", [&u1, &u0], |_| false, 5).unwrap();
        assert_eq!(k.origin_pred, Some(u0.max(u1)));
        // A loop past the bound is refused.
        assert_eq!(
            derive_file_uid("project", "a.rs", [], |_| true, 3),
            Err(UidError::LoopBound)
        );
        assert_eq!(
            derive_file_uid("project", "a.rs", [], |u| dead.contains(u), 1),
            Err(UidError::LoopBound)
        );
    }

    #[test]
    fn anchor_derivation_loop() {
        let s = Uid([7; 16]);
        let c = [3u8; 16];
        let a0 = uid_anchor(&s, &c, None).unwrap();
        let a1 = uid_anchor(&s, &c, Some(&a0)).unwrap();
        assert_eq!(
            derive_anchor_uid(&s, &c, |_| false, 0),
            Ok(AnchorKey {
                uid: a0,
                pred: None
            })
        );
        assert_eq!(
            derive_anchor_uid(&s, &c, |u| *u == a0, 1),
            Ok(AnchorKey {
                uid: a1,
                pred: Some(a0)
            })
        );
        assert_eq!(
            derive_anchor_uid(&s, &c, |_| true, 2),
            Err(UidError::LoopBound)
        );
    }

    #[test]
    fn kinds_and_display() {
        for k in [
            AnchorKind::File,
            AnchorKind::Heading,
            AnchorKind::Symbol,
            AnchorKind::Quote,
            AnchorKind::Range,
            AnchorKind::Lines,
        ] {
            assert_eq!(AnchorKind::from_code(k.code()), Some(k));
        }
        assert_eq!(AnchorKind::from_code(0), None);
        assert_eq!(AnchorKind::from_code(7), None);
        let mut v = [0u8; 16];
        v[0] = 0xAB;
        v[15] = 0x01;
        assert_eq!(Uid(v).to_string(), "ab000000000000000000000000000001");
        assert!(Uid::ZERO.is_zero());
    }
}
