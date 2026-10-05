//! Link states ([F18 §4.4]; [40 §2.9]): what an agent sees for one anchor, the file state refined by the anchor state.
//!
//! | file state | anchor | link state |
//! |---|---|---|
//! | `ok` or `moved-auto` | `fresh` or `moved`; a `file` anchor with `watch = header`; a `pinned` anchor | the file state |
//! | `ok` or `moved-auto` | `edited`, `ambiguous` or `orphaned` | `stale-anchor`, the anchor state as principal detail |
//! | `ok` or `moved-auto` | `unverified` | `unverified`, with the anchor's reason |
//! | any other | not computed | the file state |
//!
//! The anchor cascade never runs on `replaced` content nor for a `pinned` anchor ([40 §4.5], [40 §2.7]); a `file` anchor
//! with `header` watch keeps the file state whatever its content, with detail 4 when the content changed since capture.

use crate::r4::anchor::{AResult, AState, Anchor, Kind, Mode, Watch};
use crate::r4::cascade::{Detail, FileResult};
use crate::r4::strings::State;

/// Whether the anchor cascade runs for a link: the file resolved `ok` or `moved-auto` (so never on `replaced` content)
/// and the anchor is `live` ([F18 §4.4]).
// spec: [F18 §4.4]; [40 §4.5]
pub fn anchor_runs(file: &FileResult, anchor: &Anchor) -> bool {
    matches!(file.state, State::Ok | State::MovedAuto) && anchor.mode == Mode::Live
}

/// The link state of one anchor and its detail parts in [F18 §4.7] rule-1 order ([F18 §4.4]), from the file's result
/// and the anchor's result on the content at the file's path. When the cascade does not run for the link
/// ([`anchor_runs`]), the anchor result is not read and the link is the file state with the file's details.
///
/// - `fresh` or `moved`, a `file` anchor with `header` watch, or a `pinned` anchor: the file state, with the file's
///   details followed by the anchor's 4 (`body changed since capture`) and 67 (`text-unavailable`);
/// - `edited`, `ambiguous` or `orphaned`: `stale-anchor` with the principal detail 62 (`edited`, with the fuzzy score
///   when the fuzzy step decided), 64 (`ambiguous`) or 65 (`orphaned`), then 66 with the captured quote (for a
///   `range`, its start quote) when the kind has a quote (a `file` or `lines` anchor has none, and part 66 is
///   optional), or 67 instead for an anchor without its text ([F18 §4.7] rule 5). Detail 63
///   (`edited (scope only)`) needs the scope-only step, which does not run while [F20 §6.1]'s interim scanner rule
///   holds;
/// - `unverified` (reason r): `unverified` with detail r, then 67 for an anchor without its text.
// spec: [F18 §4.4]; [F18 §4.6]; [F18 §4.7]
pub fn link_state(file: &FileResult, anchor: &Anchor, result: &AResult) -> (State, Vec<Detail>) {
    if !anchor_runs(file, anchor) {
        return (file.state, file.details.clone());
    }
    let keeps_file_state = (anchor.kind == Kind::File && anchor.watch == Watch::Header)
        || matches!(
            result.state,
            AState::Fresh | AState::Moved | AState::Unresolved
        );
    if keeps_file_state {
        let mut d = file.details.clone();
        d.extend(result.details.iter().map(|&c| Detail::code(c)));
        return (file.state, d);
    }
    let unavailable = anchor.text_unavailable;
    match result.state {
        AState::Edited | AState::Ambiguous | AState::Orphaned => {
            let principal = match (result.state, result.score) {
                (AState::Edited, Some(s)) => Detail::score(62, s),
                (AState::Edited, None) => Detail::code(62),
                (AState::Ambiguous, _) => Detail::code(64),
                _ => Detail::code(65),
            };
            let mut d = vec![principal];
            if unavailable {
                d.push(Detail::code(67));
            } else if anchor.kind.has_quote() {
                d.push(Detail {
                    quote: anchor.quote.clone(),
                    ..Detail::code(66)
                });
            }
            (State::StaleAnchor, d)
        }
        AState::Unverified(reason) => {
            let mut d = vec![Detail::code(reason)];
            if unavailable {
                d.push(Detail::code(67));
            }
            (State::Unverified, d)
        }
        AState::Fresh | AState::Moved | AState::Unresolved => {
            unreachable!("kept the file state above")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r4::anchor::{Consts, Content, Form, capture, resolve};
    use crate::r4::strings::valid_parts;
    use crate::r4::text::Ratio;
    use crate::value::{Algo, Uid};

    fn file(state: State, details: &[u8]) -> FileResult {
        FileResult::of(state, details.iter().map(|&c| Detail::code(c)).collect())
    }

    const TEXT: &str = "fn main() {\n    let x = compute();\n    println!(\"{x}\");\n}\n";

    fn quote_anchor() -> Anchor {
        capture(
            Uid([2; 16]),
            Uid([7; 16]),
            &Form::Lines(2, 2),
            Some(TEXT.as_bytes()),
            None,
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap()
        .0
    }

    fn ar(state: AState) -> AResult {
        AResult {
            state,
            span: None,
            score: None,
            details: Vec::new(),
        }
    }

    fn codes(d: &[Detail]) -> Vec<u8> {
        d.iter().map(|x| x.code).collect()
    }

    /// Every row of [F18 §4.4]'s table, each result's parts valid under [F18 §4.7] rule 1.
    #[test]
    fn every_row_of_the_link_state_table() {
        let a = quote_anchor();
        let ok = file(State::Ok, &[]);
        let moved = file(State::MovedAuto, &[5, 7]);
        // Row 1: fresh or moved keep the file state.
        for f in [&ok, &moved] {
            for s in [AState::Fresh, AState::Moved] {
                let (st, d) = link_state(f, &a, &ar(s));
                assert_eq!((st, codes(&d)), (f.state, codes(&f.details)));
                assert!(valid_parts(st, &codes(&d)));
            }
        }
        // Row 1: a `file` anchor with `header` watch keeps the file state even when its content changed (detail 4).
        let (fa, _) = capture(
            Uid([2; 16]),
            Uid([7; 16]),
            &Form::File,
            Some(b"v1\n"),
            None,
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap();
        let r = resolve(&fa, Content::Bytes(b"v2\n"), Algo::Sha1, &Consts::DRAFT);
        assert_eq!((r.state, r.details.clone()), (AState::Fresh, vec![4]));
        let (st, d) = link_state(&moved, &fa, &r);
        assert_eq!((st, codes(&d)), (State::MovedAuto, vec![5, 7, 4]));
        assert!(valid_parts(st, &codes(&d)));
        // Row 1: a pinned anchor is never re-resolved; the link is the file state.
        let mut pinned = a.clone();
        pinned.mode = Mode::Pinned;
        let r = resolve(
            &pinned,
            Content::Bytes(b"other\n"),
            Algo::Sha1,
            &Consts::DRAFT,
        );
        assert_eq!(r.state, AState::Unresolved);
        assert_eq!(link_state(&ok, &pinned, &r).0, State::Ok);
        // Row 2: edited, ambiguous and orphaned are `stale-anchor` with the anchor state as principal detail and the
        // captured quote.
        let mut edited = ar(AState::Edited);
        edited.score = Some(Ratio::new(43, 50));
        for (res, principal) in [
            (edited, 62u8),
            (ar(AState::Ambiguous), 64),
            (ar(AState::Orphaned), 65),
        ] {
            let (st, d) = link_state(&ok, &a, &res);
            assert_eq!((st, codes(&d)), (State::StaleAnchor, vec![principal, 66]));
            assert_eq!(d[1].quote, a.quote);
            assert!(valid_parts(st, &codes(&d)));
        }
        let (_, d) = link_state(&ok, &a, &{
            let mut e = ar(AState::Edited);
            e.score = Some(Ratio::new(43, 50));
            e
        });
        assert_eq!(d[0].scores, vec![Ratio::new(43, 50)]);
        // Row 2 for a `file` anchor with `span` watch (a content pin): no quote, so no part 66.
        let (pin, _) = capture(
            Uid([2; 16]),
            Uid([7; 16]),
            &Form::File,
            Some(b"v1\n"),
            Some(Watch::Span),
            None,
            Algo::Sha1,
            &[],
            &Consts::DRAFT,
        )
        .unwrap();
        let r = resolve(&pin, Content::Bytes(b"v2\n"), Algo::Sha1, &Consts::DRAFT);
        let (st, d) = link_state(&ok, &pin, &r);
        assert_eq!((st, codes(&d)), (State::StaleAnchor, vec![62]));
        assert!(valid_parts(st, &codes(&d)));
        // Row 3: an unverified anchor makes the link unverified with the anchor's reason.
        let (st, d) = link_state(&moved, &a, &ar(AState::Unverified(58)));
        assert_eq!((st, codes(&d)), (State::Unverified, vec![58]));
        assert!(valid_parts(st, &codes(&d)));
        // Row 4: any other file state is the link state, and the cascade does not run (not on `replaced` content).
        for s in [
            State::Replaced,
            State::Missing,
            State::Ambiguous,
            State::MovedNeedsConfirm,
            State::Pending,
        ] {
            let f = file(s, &[]);
            assert!(!anchor_runs(&f, &a));
            assert_eq!(link_state(&f, &a, &ar(AState::Orphaned)).0, s);
        }
    }

    /// An anchor imported without its text: every result carries detail 67, and `stale-anchor` shows 67 instead of the
    /// quote ([F18 §4.7] rule 5).
    #[test]
    fn text_unavailable_anchors_carry_detail_67() {
        let mut a = quote_anchor();
        a.stored_digests = [a.digest(0), a.digest(1), a.digest(2), a.digest(3)];
        a.text_unavailable = true;
        a.quote.clear();
        a.prefix.clear();
        a.suffix.clear();
        let r = resolve(
            &a,
            Content::Bytes(TEXT.as_bytes()),
            Algo::Sha1,
            &Consts::DRAFT,
        );
        assert_eq!((r.state, r.details.clone()), (AState::Fresh, vec![67]));
        let (st, d) = link_state(&file(State::Ok, &[]), &a, &r);
        assert_eq!((st, codes(&d)), (State::Ok, vec![67]));
        assert!(valid_parts(st, &codes(&d)));
        let gone = resolve(
            &a,
            Content::Bytes(b"nothing here\n"),
            Algo::Sha1,
            &Consts::DRAFT,
        );
        let (st, d) = link_state(&file(State::Ok, &[]), &a, &gone);
        assert_eq!(st, State::StaleAnchor);
        assert_eq!(codes(&d)[1], 67);
        assert!(valid_parts(st, &codes(&d)));
        let (st, d) = link_state(
            &file(State::Ok, &[]),
            &a,
            &resolve(&a, Content::Unavailable(59), Algo::Sha1, &Consts::DRAFT),
        );
        assert_eq!((st, codes(&d)), (State::Unverified, vec![59, 67]));
    }
}
