//! WP-66: tiny content and the tiny file node ([F20 §2.6.5]), against the rule written from the section alone: text
//! is tiny below 5 lines or 64 normalised bytes, binary content below 64 bytes whatever its lines, and a file node by
//! its reference fingerprint, else by `F.bytes < 64`.
//!
//! Every content here is synthetic and in memory.

use moirai_files::oid::ObjectFormat;
use moirai_files::sketch::{Fingerprint, FingerprintSink, content_is_tiny, node_is_tiny};
use moirai_files::text::{TextStats, analyse, stats};
use proptest::prelude::*;

/// [F20 §2.6.5]'s text form, written out: `nlines < 5` or `nbytes < 64`.
fn naive_text_tiny(nlines: u64, nbytes: u64) -> bool {
    nlines < 5 || nbytes < 64
}

/// The statistics of a text of `nlines` LF-terminated lines and `nbytes` bytes.
fn text_stats(nlines: u64, nbytes: u64) -> TextStats {
    TextStats {
        len: nbytes,
        crlf: 0,
        lonecr: 0,
        lonelf: nlines,
        nul: 0,
        printable: nbytes - nlines,
        nonprintable: 0,
        last: Some(b'\n'),
    }
}

/// A text of `nlines` LF-terminated lines and `nbytes` bytes: `a` lines, the last one filling up the length.
fn text_of(nlines: usize, nbytes: usize) -> Vec<u8> {
    let mut t = b"a\n".repeat(nlines - 1);
    t.extend(std::iter::repeat_n(b'a', nbytes - t.len() - 1));
    t.push(b'\n');
    t
}

fn fingerprint_of(t: &[u8]) -> Option<Fingerprint> {
    let mut sink = FingerprintSink::new();
    let c = analyse(t, ObjectFormat::Sha1, None, &mut sink);
    sink.finish(&c.stats)
}

/// Text at 4, 5 and 6 lines, each at 63, 64 and 65 normalised bytes: tiny exactly when below 5 lines or 64 bytes,
/// from synthetic statistics and from real bytes.
#[test]
fn content_tiny_follows_the_text_rule() {
    for nlines in [4u64, 5, 6] {
        for nbytes in [63u64, 64, 65] {
            let want = naive_text_tiny(nlines, nbytes);
            let synthetic = text_stats(nlines, nbytes);
            assert!(synthetic.is_text());
            assert_eq!(
                content_is_tiny(&synthetic),
                want,
                "{nlines} lines, {nbytes} bytes"
            );
            let b = text_of(nlines as usize, nbytes as usize);
            let real = stats(&b);
            assert!(real.is_text());
            assert_eq!((real.nlines(), real.norm_len()), (nlines, nbytes));
            assert_eq!(
                content_is_tiny(&real),
                want,
                "{nlines} lines, {nbytes} bytes"
            );
        }
    }
    // The empty content is text with no line: tiny.
    assert!(content_is_tiny(&stats(b"")));
}

/// A text's bytes are those of `norm(b)`: CR LF counts one byte. 10 lines `abcde` CR LF are 70 raw bytes but 60
/// normalised, so tiny; 10 lines `abcdefg` CR LF are 90 raw and 80 normalised, so not.
#[test]
fn content_tiny_counts_normalised_bytes() {
    let crlf = b"abcde\r\n".repeat(10);
    let s = stats(&crlf);
    assert!(s.is_text());
    assert_eq!((s.len, s.norm_len(), s.nlines()), (70, 60, 10));
    assert!(content_is_tiny(&s));

    let crlf = b"abcdefg\r\n".repeat(10);
    let s = stats(&crlf);
    assert_eq!((s.len, s.norm_len(), s.nlines()), (90, 80, 10));
    assert!(!content_is_tiny(&s));

    // 8 lines: 72 raw bytes and 64 normalised, at the threshold, so not tiny; one byte less is tiny.
    let at = b"abcdefg\r\n".repeat(8);
    let s = stats(&at);
    assert_eq!((s.len, s.norm_len()), (72, 64));
    assert!(!content_is_tiny(&s));
    let below = [&b"abcdef\r\n"[..], &b"abcdefg\r\n".repeat(7)].concat();
    let s = stats(&below);
    assert_eq!((s.len, s.norm_len()), (71, 63));
    assert!(content_is_tiny(&s));
}

/// Binary content is tiny below 64 bytes, and its lines are never counted: 63 bytes are tiny, also as 62 line
/// feeds; 64 bytes are not; 100 bytes on one line are not.
#[test]
fn binary_tiny_counts_bytes_not_lines() {
    let binary = |rest: &[u8]| [&[0u8][..], rest].concat();
    let cases: [(Vec<u8>, bool); 5] = [
        (binary(&[b'\n'; 62]), true),
        (binary(&[b'x'; 62]), true),
        (binary(&[b'\n'; 63]), false),
        (binary(&[b'x'; 63]), false),
        (binary(&[b'x'; 99]), false),
    ];
    for (b, want) in cases {
        let s = stats(&b);
        assert!(!s.is_text());
        assert_eq!(content_is_tiny(&s), want, "{} bytes", b.len());
    }
    // A binary content with a final CR LF counts raw bytes: `00`, 61 × `x`, CR LF is 64 bytes, not tiny.
    let b = [&[0u8][..], &[b'x'; 61], b"\r\n"].concat();
    let s = stats(&b);
    assert_eq!(s.len, 64);
    assert!(!content_is_tiny(&s));
}

/// The proptest configuration of a suite whose tier-`pr` case count is `base`: `MOIRAI_TEST_TIER` = `nightly` runs 16
/// times as many and `exit` 64 times as many (PLAN §2.1 test tiers); no failure persistence (the seed is printed), so a
/// failing case never writes a regressions file into the repository.
fn test_config(base: u32) -> ProptestConfig {
    let cases = match std::env::var("MOIRAI_TEST_TIER").as_deref() {
        Ok("nightly") => base * 16,
        Ok("exit") => base * 64,
        _ => base,
    };
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// A short line of a pool of 8, so that texts lie near both thresholds.
fn pool_line(k: u8) -> &'static [u8] {
    [
        &b"x"[..],
        b"ab",
        b"let v = 1;",
        b"  spaced  out  ",
        b"\xD0\xB6\xD0\xB6",
        b"fn item() {}",
        b"",
        b"a longer line of text",
    ][usize::from(k % 8)]
}

/// A text of 0 to 12 pool lines, LF or CR LF, a final terminator or not.
fn short_text() -> impl Strategy<Value = Vec<u8>> {
    (
        prop::collection::vec((any::<u8>(), any::<bool>()), 0..12),
        any::<bool>(),
    )
        .prop_map(|(lines, final_eol)| {
            let mut t = Vec::new();
            let n = lines.len();
            for (i, (k, crlf)) in lines.into_iter().enumerate() {
                t.extend_from_slice(pool_line(k));
                if i + 1 < n || final_eol {
                    t.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
                }
            }
            t
        })
}

/// Binary content of 1 to 140 bytes: random bytes with a NUL somewhere.
fn binary() -> impl Strategy<Value = Vec<u8>> {
    (
        prop::collection::vec(any::<u8>(), 0..140),
        any::<prop::sample::Index>(),
    )
        .prop_map(|(mut b, at)| {
            let i = at.index(b.len() + 1);
            b.insert(i, 0);
            b
        })
}

proptest! {
    #![proptest_config(test_config(256))]

    /// For text, the content test equals `Fingerprint::is_tiny`; binary content has no fingerprint, and its test is
    /// `len < 64`.
    #[test]
    fn content_tiny_agrees_with_the_fingerprint(b in prop_oneof![short_text(), binary()]) {
        let s = stats(&b);
        match fingerprint_of(&b) {
            Some(fp) => {
                prop_assert!(s.is_text());
                prop_assert_eq!(content_is_tiny(&s), fp.is_tiny());
                prop_assert_eq!(
                    content_is_tiny(&s),
                    naive_text_tiny(u64::from(fp.nlines()), u64::from(fp.nbytes()))
                );
            }
            None => {
                prop_assert!(!s.is_text());
                prop_assert_eq!(content_is_tiny(&s), (b.len() as u64) < 64);
            }
        }
    }
}

/// A file node is tiny by its reference fingerprint whatever `F.bytes` says, and by `F.bytes < 64` only without one.
#[test]
fn node_tiny_prefers_the_reference_fingerprint() {
    let tiny = fingerprint_of(b"alpha beta\ngamma delta\n").unwrap();
    assert!(tiny.is_tiny());
    let full = fingerprint_of(&text_of(6, 80)).unwrap();
    assert!(!full.is_tiny());

    assert!(node_is_tiny(Some(&tiny), 1_000));
    assert!(node_is_tiny(Some(&tiny), u64::MAX));
    assert!(!node_is_tiny(Some(&full), 10));
    assert!(!node_is_tiny(Some(&full), 0));

    assert!(node_is_tiny(None, 63));
    assert!(!node_is_tiny(None, 64));
    assert!(!node_is_tiny(None, 65));
    assert!(node_is_tiny(None, 0));
    assert!(!node_is_tiny(None, u64::MAX));
}
