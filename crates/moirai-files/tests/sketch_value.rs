//! WP-66: the fingerprint value ([F20 §2.6.4]): its sequence table against a little-endian encoder written from the
//! table alone, the committed fixture value, every invalid case of the section refused, other resolver versions, and
//! the tiny test of a fingerprint ([F20 §2.6.5]).
//!
//! Every content here is synthetic and in memory.

use moirai_files::oid::ObjectFormat;
use moirai_files::r14::{TINY_BYTES, TINY_LINES};
use moirai_files::sketch::{Fingerprint, FingerprintError, FingerprintSink};
use moirai_files::text::analyse;
use proptest::prelude::*;

// --- the table of [F20 §2.6.4], field by field ----------------------------------------------------------------------

fn le16(out: &mut Vec<u8>, v: u16) {
    out.push((v & 0xFF) as u8);
    out.push((v >> 8) as u8);
}

fn le32(out: &mut Vec<u8>, v: u32) {
    for k in 0..4 {
        out.push(((v >> (8 * k)) & 0xFF) as u8);
    }
}

/// The fields of one value, in table order; `n_sketch` is `sketch.len()`.
#[derive(Clone)]
struct Value {
    ver: u16,
    flags: u8,
    nlines: u32,
    nbytes: u32,
    weight: u32,
    distinct: u32,
    sketch: Vec<u32>,
}

impl Value {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        le16(&mut out, self.ver);
        out.push(self.sketch.len() as u8);
        out.push(self.flags);
        for v in [self.nlines, self.nbytes, self.weight, self.distinct] {
            le32(&mut out, v);
        }
        for &v in &self.sketch {
            le32(&mut out, v);
        }
        out
    }
}

fn value_of(fp: &Fingerprint) -> Value {
    Value {
        ver: 1,
        flags: u8::from(fp.distinct_estimated()),
        nlines: fp.nlines(),
        nbytes: fp.nbytes(),
        weight: fp.weight(),
        distinct: fp.distinct(),
        sketch: fp.sketch().to_vec(),
    }
}

/// `fixtures/hex/store-a/blobs.9.hex` lines 64–65, the BLOBDATA bytes of `fprint_lockmd` ([F10 §3.2]): ver 1,
/// n_sketch 3, flags 0, nlines 5, nbytes 161, weight 150, distinct 3, sketch `0a1b2c3d 5e6f7081 92a3b4c5`.
const FIXTURE: [u8; 32] = [
    0x01, 0x00, 0x03, 0x00, 0x05, 0x00, 0x00, 0x00, 0xa1, 0x00, 0x00, 0x00, 0x96, 0x00, 0x00,
    0x00, //
    0x03, 0x00, 0x00, 0x00, 0x3d, 0x2c, 0x1b, 0x0a, 0x81, 0x70, 0x6f, 0x5e, 0xc5, 0xb4, 0xa3, 0x92,
];

fn fixture() -> Value {
    Value {
        ver: 1,
        flags: 0,
        nlines: 5,
        nbytes: 161,
        weight: 150,
        distinct: 3,
        sketch: vec![0x0a1b_2c3d, 0x5e6f_7081, 0x92a3_b4c5],
    }
}

/// A valid value with a full sketch of 64 values; `estimated` sets the flag with `distinct` 1,000.
fn full(estimated: bool) -> Value {
    Value {
        ver: 1,
        flags: u8::from(estimated),
        nlines: 400,
        nbytes: 20_000,
        weight: 15_000,
        distinct: if estimated { 1_000 } else { 64 },
        sketch: (0..64).map(|i| 1_000 + 7 * i).collect(),
    }
}

// --- the property test -------------------------------------------------------------------------------------------

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

/// A text content: an optional BOM, 0–200 lines of 0–4 words from a pool of 300 (so a content often has more than 64
/// distinct lines, and sometimes a short or repeated one), LF or CR LF, a final terminator or not.
fn text() -> impl Strategy<Value = Vec<u8>> {
    (
        any::<bool>(),
        prop::collection::vec(
            (prop::collection::vec(0u32..300, 0..5), any::<bool>()),
            0..200,
        ),
        any::<bool>(),
    )
        .prop_map(|(bom, lines, final_eol)| {
            let mut t = if bom {
                b"\xEF\xBB\xBF".to_vec()
            } else {
                Vec::new()
            };
            let n = lines.len();
            for (i, (words, crlf)) in lines.into_iter().enumerate() {
                let l: Vec<String> = words.iter().map(|k| format!("w{k}")).collect();
                t.extend_from_slice(l.join(" \t").as_bytes());
                if i + 1 < n || final_eol {
                    t.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
                }
            }
            t
        })
}

proptest! {
    #![proptest_config(test_config(64))]

    /// `to_bytes` is the table, little-endian, `20 + 4 × n_sketch` bytes long, and `from_bytes` gives the fingerprint
    /// back.
    #[test]
    fn value_round_trips(t in text()) {
        let mut sink = FingerprintSink::new();
        let c = analyse(&t, ObjectFormat::Sha1, None, &mut sink);
        let fp = sink.finish(&c.stats).unwrap();
        let bytes = fp.to_bytes();
        prop_assert_eq!(bytes.len(), 20 + 4 * fp.sketch().len());
        prop_assert_eq!(&bytes, &value_of(&fp).encode());
        prop_assert_eq!(Fingerprint::from_bytes(&bytes), Ok(fp));
    }
}

// --- the examples --------------------------------------------------------------------------------------------------

/// The committed fixture value decodes to its fields and encodes back byte for byte.
#[test]
fn fixture_value_decodes() {
    assert_eq!(fixture().encode(), FIXTURE);
    let fp = Fingerprint::from_bytes(&FIXTURE).unwrap();
    assert_eq!(
        (fp.nlines(), fp.nbytes(), fp.weight(), fp.distinct()),
        (5, 161, 150, 3)
    );
    assert!(!fp.distinct_estimated());
    assert_eq!(fp.sketch(), [0x0a1b_2c3d, 0x5e6f_7081, 0x92a3_b4c5]);
    assert!(!fp.is_tiny());
    assert_eq!(fp.to_bytes(), FIXTURE);
}

/// [F20 §2.6.4]'s invalid cases, each a valid value with exactly one defect, refused with its own error.
#[test]
fn invalid_values_are_refused() {
    let refuse = |v: &Value| Fingerprint::from_bytes(&v.encode());
    // The bases are valid.
    for base in [fixture(), full(false), full(true)] {
        assert!(refuse(&base).is_ok());
    }
    let mut edge = full(true);
    edge.distinct = 65;
    assert!(
        refuse(&edge).is_ok(),
        "a flagged full sketch with distinct 65"
    );

    // `n_sketch > 64`, with a length that agrees.
    let mut v = full(true);
    v.sketch.push(5_000);
    assert_eq!(v.encode().len(), 20 + 4 * 65);
    assert_eq!(refuse(&v), Err(FingerprintError::SketchCount(65)));

    // Not strictly ascending: equal neighbours, a descending pair.
    let mut v = fixture();
    v.sketch[1] = v.sketch[0];
    assert_eq!(refuse(&v), Err(FingerprintError::NotAscending));
    let mut v = fixture();
    v.sketch.swap(1, 2);
    assert_eq!(refuse(&v), Err(FingerprintError::NotAscending));
    let mut v = full(false);
    v.sketch[63] = v.sketch[62];
    assert_eq!(refuse(&v), Err(FingerprintError::NotAscending));

    // `n_sketch < 64` while `distinct ≠ n_sketch` or the flag is set.
    let mut v = fixture();
    v.distinct = 4;
    assert_eq!(refuse(&v), Err(FingerprintError::Distinct));
    let mut v = fixture();
    v.flags = 1;
    assert_eq!(refuse(&v), Err(FingerprintError::Distinct));
    // `n_sketch = 64` with the flag clear while `distinct ≠ 64`.
    for d in [63, 65] {
        let mut v = full(false);
        v.distinct = d;
        assert_eq!(refuse(&v), Err(FingerprintError::Distinct), "{d}");
    }
    // `n_sketch = 64` with the flag set while `distinct < 65`.
    let mut v = full(true);
    v.distinct = 64;
    assert_eq!(refuse(&v), Err(FingerprintError::Distinct));

    // A reserved bit set, each of bits 1–7 alone.
    for bit in 1..8 {
        let mut v = fixture();
        v.flags = 1 << bit;
        assert_eq!(
            refuse(&v),
            Err(FingerprintError::ReservedFlags(1 << bit)),
            "bit {bit}"
        );
    }

    // Lengths: none, 1 byte (no `ver`), 19 bytes, one byte short of `20 + 4n` and one byte over.
    let good = fixture().encode();
    for len in [0, 1, 19, good.len() - 1] {
        assert_eq!(
            Fingerprint::from_bytes(&good[..len]),
            Err(FingerprintError::Length(len)),
            "{len}"
        );
    }
    let mut long = good.clone();
    long.push(0);
    assert_eq!(
        Fingerprint::from_bytes(&long),
        Err(FingerprintError::Length(good.len() + 1))
    );
}

/// A value computed under another resolver version is absent ([F20 §1.3]): 0, 2 and `FFFF` are refused as versions,
/// before any other check.
#[test]
fn other_versions_are_absent() {
    for ver in [0, 2, 0xFFFF] {
        let mut v = fixture();
        v.ver = ver;
        let b = v.encode();
        assert_eq!(
            Fingerprint::from_bytes(&b),
            Err(FingerprintError::Version(ver))
        );
        assert_eq!(
            Fingerprint::from_bytes(&b[..2]),
            Err(FingerprintError::Version(ver))
        );
    }
}

/// [F20 §2.6.5]: text is tiny iff `nlines < TINY_LINES` or `nbytes < TINY_BYTES`.
#[test]
fn tiny_follows_lines_and_bytes() {
    for nlines in [TINY_LINES - 1, TINY_LINES] {
        for nbytes in [TINY_BYTES - 1, TINY_BYTES] {
            let v = Value {
                ver: 1,
                flags: 0,
                nlines: nlines as u32,
                nbytes: nbytes as u32,
                weight: 0,
                distinct: 0,
                sketch: Vec::new(),
            };
            let fp = Fingerprint::from_bytes(&v.encode()).unwrap();
            let want = !(nlines == TINY_LINES && nbytes == TINY_BYTES);
            assert_eq!(fp.is_tiny(), want, "nlines {nlines}, nbytes {nbytes}");
        }
    }
}
