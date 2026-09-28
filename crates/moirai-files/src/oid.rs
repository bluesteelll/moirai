//! `oid`: the content identity of a file ([F20 §2.3]; [40 §2.5]; [40 §2.11] R-1), the git object-format registry
//! ([F01 §7.5]) and the three-valued comparison of `oid` values.
//!
//! `oid_H(b) = H("blob " ‖ dec(len(norm(b))) ‖ 00 ‖ norm(b))` with H the root's object format A(R), SHA-1 or
//! SHA-256. The normalisation `norm` and the two-pass streaming reader are [`crate::text`]'s; this module owns the
//! value type, the hash over a known normalised length ([`OidHasher`]) and the comparison rules.

use core::fmt;

use sha1::{Digest, Sha1};
use sha2::Sha256;

/// A value of the git object-format registry ([F01 §7.5]): the `algo` byte of every stored `oid` and git object id.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Algo {
    /// 0 `none`: no object id ("oid-or-empty", [F01 §7.5]).
    None = 0,
    /// 1 `sha1`: SHA-1, 20 digest bytes.
    Sha1 = 1,
    /// 2 `sha256`: SHA-256, 32 digest bytes.
    Sha256 = 2,
}

impl Algo {
    /// The registry value of byte `b`; values 3–255 are reserved and invalid in format v1 ([F01 §7.5]).
    #[must_use]
    pub const fn from_u8(b: u8) -> Option<Algo> {
        match b {
            0 => Some(Algo::None),
            1 => Some(Algo::Sha1),
            2 => Some(Algo::Sha256),
            _ => None,
        }
    }

    /// The registry byte.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    /// The number of digest bytes: 0, 20 or 32 ([F01 §7.5]).
    #[must_use]
    pub const fn digest_len(self) -> usize {
        match self {
            Algo::None => 0,
            Algo::Sha1 => 20,
            Algo::Sha256 => 32,
        }
    }

    /// The registry name, git's `extensions.objectFormat` value; text forms use the name, never the number
    /// ([F01 §7.5]).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Algo::None => "none",
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
        }
    }

    /// The registry value named `name` (exact, lower-case).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Algo> {
        match name {
            "none" => Some(Algo::None),
            "sha1" => Some(Algo::Sha1),
            "sha256" => Some(Algo::Sha256),
            _ => None,
        }
    }
}

/// A hash function that computes `oid` values: the object format A(R) of a root ([F20 §2.3] "Algorithm of a root",
/// [40 §2.5], review S-18). The `project` root uses the store's repository format as read at `init` (`sha1` without
/// a repository); every named root and `abs` use `sha1`. A(R) is an `init`-fixed parameter the caller supplies.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ObjectFormat {
    /// SHA-1 (`sha1`).
    Sha1,
    /// SHA-256 (`sha256`).
    Sha256,
}

impl ObjectFormat {
    /// The registry value that tags the `oid` values this format computes.
    #[must_use]
    pub const fn algo(self) -> Algo {
        match self {
            ObjectFormat::Sha1 => Algo::Sha1,
            ObjectFormat::Sha256 => Algo::Sha256,
        }
    }

    /// The format of registry value `algo`; `none` computes nothing.
    #[must_use]
    pub const fn from_algo(algo: Algo) -> Option<ObjectFormat> {
        match algo {
            Algo::None => None,
            Algo::Sha1 => Some(ObjectFormat::Sha1),
            Algo::Sha256 => Some(ObjectFormat::Sha256),
        }
    }
}

/// An `oid` value: an `algo` byte and exactly that algorithm's digest bytes ([F01 §7.5], [40 §2.11] R-1, [F08 §5.2]).
///
/// The `none` value (`algo` 0, no digest) is the design's "oid-or-empty". `Eq` here is plain value equality, used for
/// storage and tests; content comparison follows [F20 §2.3] and is [`Oid::relation`] and [`oid_in`].
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct Oid {
    algo: Algo,
    /// The digest in its first `algo.digest_len()` bytes; the rest is zero.
    bytes: [u8; 32],
}

/// Why bytes are not an `oid` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidError {
    /// The `algo` byte is reserved (3–255, [F01 §7.5]).
    ReservedAlgo(u8),
    /// The digest length does not match the algorithm's.
    DigestLength {
        /// The algorithm the value claims.
        algo: Algo,
        /// The number of digest bytes present.
        len: usize,
    },
    /// A fixed 32-byte slot has non-zero reserved bytes ([F01 §7.5], §10).
    ReservedBytes,
}

impl fmt::Display for OidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            OidError::ReservedAlgo(b) => write!(f, "reserved oid algorithm byte {b}"),
            OidError::DigestLength { algo, len } => {
                write!(
                    f,
                    "{} digest of {len} bytes, expected {}",
                    algo.name(),
                    algo.digest_len()
                )
            }
            OidError::ReservedBytes => f.write_str("non-zero reserved bytes in an oid slot"),
        }
    }
}

impl std::error::Error for OidError {}

impl Oid {
    /// The `none` value.
    pub const NONE: Oid = Oid {
        algo: Algo::None,
        bytes: [0; 32],
    };

    /// The value `algo` + `digest`; the length must be the algorithm's.
    ///
    /// # Errors
    /// [`OidError::DigestLength`] when `digest` has another length.
    pub fn new(algo: Algo, digest: &[u8]) -> Result<Oid, OidError> {
        if digest.len() != algo.digest_len() {
            return Err(OidError::DigestLength {
                algo,
                len: digest.len(),
            });
        }
        let mut bytes = [0u8; 32];
        bytes[..digest.len()].copy_from_slice(digest);
        Ok(Oid { algo, bytes })
    }

    /// The `algo` registry value.
    #[must_use]
    pub const fn algo(&self) -> Algo {
        self.algo
    }

    /// The digest bytes (empty for `none`).
    #[must_use]
    pub fn digest(&self) -> &[u8] {
        &self.bytes[..self.algo.digest_len()]
    }

    /// Whether this is the `none` value.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self.algo, Algo::None)
    }

    /// The variable-width form ([F01 §7.5]): the `algo` byte, then exactly the digest bytes (1, 21 or 33 bytes),
    /// appended to `out`.
    pub fn encode_var(&self, out: &mut Vec<u8>) {
        out.push(self.algo.to_u8());
        out.extend_from_slice(self.digest());
    }

    /// Decodes the variable-width form at the start of `bytes`; returns the value and the bytes it used.
    ///
    /// # Errors
    /// [`OidError::ReservedAlgo`] for a reserved `algo` byte; [`OidError::DigestLength`] when `bytes` ends inside the
    /// digest (including an empty input, reported as `none` with length 0 missing its `algo` byte).
    pub fn decode_var(bytes: &[u8]) -> Result<(Oid, usize), OidError> {
        let Some((&a, rest)) = bytes.split_first() else {
            return Err(OidError::DigestLength {
                algo: Algo::None,
                len: 0,
            });
        };
        let algo = Algo::from_u8(a).ok_or(OidError::ReservedAlgo(a))?;
        let n = algo.digest_len();
        if rest.len() < n {
            return Err(OidError::DigestLength {
                algo,
                len: rest.len(),
            });
        }
        Ok((Oid::new(algo, &rest[..n])?, 1 + n))
    }

    /// The fixed 32-byte slot form ([F01 §7.5]): `sha1` digests in bytes 0–19 with 20–31 zero, `sha256` digests in
    /// all 32 bytes, `none` all zero. The algorithm travels in a separate `algo` field.
    #[must_use]
    pub const fn to_slot32(&self) -> [u8; 32] {
        self.bytes
    }

    /// The value of a fixed 32-byte slot with algorithm `algo`.
    ///
    /// # Errors
    /// [`OidError::ReservedBytes`] when the bytes past the digest (all of them for `none`) are not zero.
    pub fn from_slot32(algo: Algo, slot: &[u8; 32]) -> Result<Oid, OidError> {
        let n = algo.digest_len();
        if slot[n..].iter().any(|&b| b != 0) {
            return Err(OidError::ReservedBytes);
        }
        Ok(Oid { algo, bytes: *slot })
    }

    /// The content relation of two stored values ([F20 §2.3] "Comparison"): equal iff the `algo` bytes and the
    /// digests are equal; two values of the same algorithm with different digests are different; a pair of
    /// different algorithms is content unknown, and `none` equals nothing, so any pair with `none` is unknown.
    #[must_use]
    pub fn relation(&self, other: &Oid) -> OidRelation {
        if self.is_none() || other.is_none() || self.algo != other.algo {
            OidRelation::Unknown
        } else if self.digest() == other.digest() {
            OidRelation::Equal
        } else {
            OidRelation::Different
        }
    }
}

impl fmt::Display for Oid {
    /// Lower-case hexadecimal of the digest bytes only: 40 digits for `sha1`, 64 for `sha256`, none for `none`
    /// ([F01 §6.4, §7.5]).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.digest() {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{self}", self.algo.name())
    }
}

/// The relation of two `oid` values ([F20 §2.3]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidRelation {
    /// Same algorithm, same digest.
    Equal,
    /// Same algorithm, different digests.
    Different,
    /// Different algorithms, or a `none` value: content unknown, never equal and never different.
    Unknown,
}

/// A three-valued truth value ([F20 §2.3]): a rule that needs "∈" holds only on `True`, a rule that needs "∉" only
/// on `False`; `Unknown` makes the evidence line contribute nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tri {
    /// Holds.
    True,
    /// Does not hold.
    False,
    /// Cannot be decided from the values.
    Unknown,
}

/// The test "`oid(q) ∈ S`" for a set S of stored values ([F20 §2.3]), with `q` computed under the root's format
/// A(R): `True` if q equals an element of S; `False` if it equals none and every element of S has q's algorithm
/// (so an empty S gives `False`); `Unknown` otherwise.
#[must_use]
pub fn oid_in<'a>(q: &Oid, set: impl IntoIterator<Item = &'a Oid>) -> Tri {
    let mut all_different = true;
    for s in set {
        match q.relation(s) {
            OidRelation::Equal => return Tri::True,
            OidRelation::Different => {}
            OidRelation::Unknown => all_different = false,
        }
    }
    if all_different {
        Tri::False
    } else {
        Tri::Unknown
    }
}

/// Streaming `H("blob " ‖ dec(len) ‖ 00 ‖ bytes)` for a length known before the first byte ([F20 §2.3]; [40 §2.5]
/// "Streaming, bounded memory": the header needs the normalised length, hence pass 1).
///
/// The caller feeds exactly `len` bytes; [`OidHasher::fed`] tells how many it fed, which the two-pass reader
/// compares with pass 1's count ([F20 §2.4]).
#[derive(Clone)]
pub struct OidHasher {
    inner: Inner,
    fed: u64,
}

#[derive(Clone)]
enum Inner {
    Sha1(Sha1),
    Sha256(Sha256),
}

impl OidHasher {
    /// A hasher for format `format` over content of `len` bytes; the header is hashed here.
    #[must_use]
    pub fn new(format: ObjectFormat, len: u64) -> OidHasher {
        let mut header = [0u8; 5 + 20 + 1];
        header[..5].copy_from_slice(b"blob ");
        let digits = write_dec(len, &mut header[5..25]);
        header[5 + digits] = 0;
        let header = &header[..5 + digits + 1];
        let inner = match format {
            ObjectFormat::Sha1 => Inner::Sha1(Sha1::new_with_prefix(header)),
            ObjectFormat::Sha256 => Inner::Sha256(Sha256::new_with_prefix(header)),
        };
        OidHasher { inner, fed: 0 }
    }

    /// Hashes the next content bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.fed += bytes.len() as u64;
        match &mut self.inner {
            Inner::Sha1(h) => h.update(bytes),
            Inner::Sha256(h) => h.update(bytes),
        }
    }

    /// The number of content bytes fed so far (the header excluded).
    #[must_use]
    pub const fn fed(&self) -> u64 {
        self.fed
    }

    /// The `oid` value, tagged with the format's `algo`.
    #[must_use]
    pub fn finish(self) -> Oid {
        let mut bytes = [0u8; 32];
        let algo = match self.inner {
            Inner::Sha1(h) => {
                bytes[..20].copy_from_slice(h.finalize().as_slice());
                Algo::Sha1
            }
            Inner::Sha256(h) => {
                bytes.copy_from_slice(h.finalize().as_slice());
                Algo::Sha256
            }
        };
        Oid { algo, bytes }
    }
}

/// `H("blob " ‖ dec(len(bytes)) ‖ 00 ‖ bytes)` over bytes taken as they are, with no normalisation: git's blob id of
/// `bytes`. For a symbolic link, `bytes` is its target text as [OS/path] reads it, and this is its `oid`
/// ([F20 §2.3] "Symbolic links", [80 §2.10] P8). For file content use [`crate::text::analyse`] or the two-pass
/// reader, which apply `norm`.
#[must_use]
pub fn blob_oid(format: ObjectFormat, bytes: &[u8]) -> Oid {
    let mut h = OidHasher::new(format, bytes.len() as u64);
    h.update(bytes);
    h.finish()
}

/// Writes `v` as decimal text ([F01 §6.5]: no sign, no leading zeros, `0` for zero) at the start of `out` (at least
/// 20 bytes) and returns the number of digits.
fn write_dec(mut v: u64, out: &mut [u8]) -> usize {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    loop {
        i -= 1;
        // `v % 10` is below 10, so the cast keeps every bit.
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let n = tmp.len() - i;
    out[..n].copy_from_slice(&tmp[i..]);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn registry_round_trips() {
        for a in [Algo::None, Algo::Sha1, Algo::Sha256] {
            assert_eq!(Algo::from_u8(a.to_u8()), Some(a));
            assert_eq!(Algo::from_name(a.name()), Some(a));
        }
        assert_eq!(Algo::from_u8(3), None);
        assert_eq!(Algo::from_u8(255), None);
        assert_eq!(Algo::from_name("SHA1"), None);
        assert_eq!(ObjectFormat::from_algo(Algo::None), None);
        assert_eq!(
            ObjectFormat::from_algo(Algo::Sha256),
            Some(ObjectFormat::Sha256)
        );
    }

    #[test]
    fn decimal_text() {
        let mut b = [0u8; 20];
        let n = write_dec(0, &mut b);
        assert_eq!(&b[..n], b"0");
        let n = write_dec(1_234_567_890, &mut b);
        assert_eq!(&b[..n], b"1234567890");
        let n = write_dec(u64::MAX, &mut b);
        assert_eq!(&b[..n], b"18446744073709551615");
    }

    #[test]
    fn blob_ids_match_git() {
        // Git's well-known ids: the empty blob and "hello\n".
        assert_eq!(
            blob_oid(ObjectFormat::Sha1, b"").to_string(),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
        assert_eq!(
            blob_oid(ObjectFormat::Sha1, b"hello\n").to_string(),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert_eq!(
            blob_oid(ObjectFormat::Sha256, b"hello\n").to_string(),
            "2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4"
        );
        // Header and content hashed in pieces equal one piece.
        let mut h = OidHasher::new(ObjectFormat::Sha1, 6);
        h.update(b"hel");
        h.update(b"");
        h.update(b"lo\n");
        assert_eq!(h.fed(), 6);
        assert_eq!(h.finish(), blob_oid(ObjectFormat::Sha1, b"hello\n"));
    }

    #[test]
    fn encodings_round_trip() {
        let a = blob_oid(ObjectFormat::Sha1, b"x");
        let b = blob_oid(ObjectFormat::Sha256, b"x");
        let mut v = Vec::new();
        a.encode_var(&mut v);
        b.encode_var(&mut v);
        Oid::NONE.encode_var(&mut v);
        assert_eq!(v.len(), 21 + 33 + 1);
        let (a2, n1) = Oid::decode_var(&v).unwrap();
        let (b2, n2) = Oid::decode_var(&v[n1..]).unwrap();
        let (c2, n3) = Oid::decode_var(&v[n1 + n2..]).unwrap();
        assert_eq!((a2, b2, c2, n1 + n2 + n3), (a, b, Oid::NONE, v.len()));
        assert_eq!(Oid::decode_var(&[7]), Err(OidError::ReservedAlgo(7)));
        assert_eq!(
            Oid::decode_var(&[1, 0, 0]),
            Err(OidError::DigestLength {
                algo: Algo::Sha1,
                len: 2
            })
        );
        assert!(Oid::decode_var(&[]).is_err());

        let slot = a.to_slot32();
        assert!(slot[20..].iter().all(|&x| x == 0));
        assert_eq!(Oid::from_slot32(Algo::Sha1, &slot), Ok(a));
        assert_eq!(Oid::from_slot32(Algo::Sha256, &b.to_slot32()), Ok(b));
        assert_eq!(Oid::from_slot32(Algo::None, &[0; 32]), Ok(Oid::NONE));
        assert_eq!(
            Oid::from_slot32(Algo::Sha1, &b.to_slot32()),
            Err(OidError::ReservedBytes)
        );
        assert_eq!(
            Oid::new(Algo::Sha1, &[0; 32]),
            Err(OidError::DigestLength {
                algo: Algo::Sha1,
                len: 32
            })
        );
        assert_eq!(
            Oid::new(Algo::Sha1, &hex("ce013625030ba8dba906f756967f9e9ca394464a")),
            Ok(blob_oid(ObjectFormat::Sha1, b"hello\n"))
        );
        assert_eq!(format!("{:?}", Oid::NONE), "none:");
    }

    #[test]
    fn comparison_is_three_valued() {
        let x1 = blob_oid(ObjectFormat::Sha1, b"x");
        let y1 = blob_oid(ObjectFormat::Sha1, b"y");
        let x2 = blob_oid(ObjectFormat::Sha256, b"x");
        assert_eq!(x1.relation(&x1), OidRelation::Equal);
        assert_eq!(x1.relation(&y1), OidRelation::Different);
        assert_eq!(x1.relation(&x2), OidRelation::Unknown);
        assert_eq!(x1.relation(&Oid::NONE), OidRelation::Unknown);
        assert_eq!(Oid::NONE.relation(&Oid::NONE), OidRelation::Unknown);

        assert_eq!(oid_in(&x1, [&y1, &x1]), Tri::True);
        assert_eq!(oid_in(&x1, [&x2, &x1]), Tri::True);
        assert_eq!(oid_in(&x1, [&y1]), Tri::False);
        assert_eq!(oid_in(&x1, []), Tri::False);
        assert_eq!(oid_in(&x1, [&y1, &x2]), Tri::Unknown);
        assert_eq!(oid_in(&x1, [&Oid::NONE]), Tri::Unknown);
    }
}
