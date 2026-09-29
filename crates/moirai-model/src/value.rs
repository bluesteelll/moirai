//! Identifiers and the closed value set of the reference model ([F08 §2], [F08 §5]; [API §5.1]–§5.2).
//!
//! The model keeps store-local identities as the store does (`#N`, [F08 §2.1]) and names every enumeration value,
//! kind and field by its name: symbol ids, enumeration integers and root ids are the engine's storage and never the
//! model's ([60 §4.3]). A `ref` value is a `#N`, which denotes the node's uid through the store-wide allocation
//! ([F08 §5.1] type 9).

use std::cmp::Ordering;
use std::fmt;

/// A node's store-local identity `#N`, 1 … 2^32 − 1 ([F08 §2.1]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Nid(pub u32);

impl fmt::Display for Nid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A uid: 16 bytes, compared bytewise, never all zero on a node ([F08 §2.2]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uid(pub [u8; 16]);

impl Uid {
    /// The all-zero value, which no node carries.
    pub const ZERO: Uid = Uid([0; 16]);

    /// The 32 lower-case hexadecimal digits ([F01 §6.4]).
    pub fn hex(&self) -> String {
        hex(&self.0)
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#u:{}", self.hex())
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#u:{}", self.hex())
    }
}

/// Lower-case hexadecimal of bytes.
pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}

/// `lp(x) = u32-le(len(x)) ‖ x` ([F01 §6.3]), appended to `out`.
pub fn lp(out: &mut Vec<u8>, x: &[u8]) {
    let n = u32::try_from(x.len()).expect("an lp() operand is below 2^32 bytes ([F01 §6.3])");
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(x);
}

/// BLAKE3-128: the first 16 bytes of BLAKE3-256 ([F01 §7.1]) over `lp()`-framed operands.
pub fn blake3_128(parts: &[&[u8]]) -> [u8; 16] {
    let mut buf = Vec::new();
    for p in parts {
        lp(&mut buf, p);
    }
    let h = blake3::hash(&buf);
    let mut out = [0u8; 16];
    out.copy_from_slice(&h.as_bytes()[..16]);
    out
}

/// An `f64` value: never NaN or infinite, `−0.0` stored as `+0.0` ([F08 §5.3]). Ordered numerically.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct F64(u64);

impl F64 {
    /// The value, or `None` for NaN and the infinities (refused as `bad_value`).
    pub fn new(x: f64) -> Option<F64> {
        if !x.is_finite() {
            return None;
        }
        let x = if x == 0.0 { 0.0 } else { x };
        Some(F64(x.to_bits()))
    }

    /// The number.
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl PartialOrd for F64 {
    fn partial_cmp(&self, other: &F64) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for F64 {
    fn cmp(&self, other: &F64) -> Ordering {
        self.get().total_cmp(&other.get())
    }
}

impl fmt::Debug for F64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get())
    }
}

/// The git object-format algorithm of an `oid` ([F01 §7.5]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Algo {
    /// SHA-1, 20 digest bytes.
    Sha1,
    /// SHA-256, 32 digest bytes.
    Sha256,
}

impl Algo {
    /// The algorithm's name.
    pub fn name(self) -> &'static str {
        match self {
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
        }
    }

    /// The digest length in bytes.
    pub fn digest_len(self) -> usize {
        match self {
            Algo::Sha1 => 20,
            Algo::Sha256 => 32,
        }
    }
}

/// An `oid` value: an algorithm and exactly its digest bytes ([F08 §5.2]). The empty `oid` is an absent value.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Oid {
    /// The algorithm.
    pub algo: Algo,
    /// The digest, `algo.digest_len()` bytes.
    pub digest: Vec<u8>,
}

/// A `path` value: a root name and the path text ([F08 §5.2], §5.4.1).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PathVal {
    /// The root name (`project`, `abs` or a named root).
    pub root: String,
    /// The path text, exact bytes.
    pub text: String,
}

/// The class of a `pathmove` entry ([F08 §5.2]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum MoveClass {
    /// 1 `explicit`.
    Explicit,
    /// 2 `confirmed`.
    Confirmed,
    /// 3 `committed`.
    Committed,
    /// 4 `observed`.
    Observed,
}

/// A `pathmove` value ([F08 §5.2]).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PathMove {
    /// The writer's HLC when the candidate was computed.
    pub hlc: u64,
    /// The class.
    pub class: MoveClass,
    /// The source directory prefix.
    pub from: PathVal,
    /// The destination directory prefix.
    pub to: PathVal,
    /// The git commit it was observed in, or `None`.
    pub git: Option<Oid>,
}

impl PartialOrd for PathMove {
    fn partial_cmp(&self, o: &PathMove) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for PathMove {
    /// (`hlc`, `from`, `to`, `class`, `git`) ([API §5.5]).
    fn cmp(&self, o: &PathMove) -> Ordering {
        (self.hlc, &self.from, &self.to, self.class, &self.git)
            .cmp(&(o.hlc, &o.from, &o.to, o.class, &o.git))
    }
}

/// A value of the closed type set ([F08 §5.1]). Enumeration values are held by name; `text` and `sym` are one
/// logical type. A set holds its elements strictly ascending ([API §5.5]) and is never empty: the empty set, the empty
/// text and the empty `oid` are absent values ([F08 §5.3]).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Value {
    /// `bool`.
    Bool(bool),
    /// `int`.
    Int(i64),
    /// `counter`: the total of every increment.
    Counter(i64),
    /// `f64`.
    F64(F64),
    /// An enumeration value, by name.
    Enum(String),
    /// A text (`text` or `sym`).
    Text(String),
    /// A set, strictly ascending.
    Set(Vec<Value>),
    /// A node reference.
    Ref(Nid),
    /// A moirai commit id (32 bytes).
    Commit([u8; 32]),
    /// A path.
    Path(PathVal),
    /// A git object id.
    Oid(Oid),
    /// A directory move.
    PathMove(Box<PathMove>),
}

impl Hash for F64 {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.0.hash(h);
    }
}

use std::hash::Hash;

impl Value {
    /// A set value from elements: sorted, duplicates kept out; `None` for an empty set (absent).
    pub fn set(mut elems: Vec<Value>) -> Option<Value> {
        elems.sort();
        elems.dedup();
        if elems.is_empty() {
            None
        } else {
            Some(Value::Set(elems))
        }
    }

    /// The text of a text or enumeration value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) | Value::Enum(s) => Some(s),
            _ => None,
        }
    }

    /// The elements of a set value.
    pub fn elems(&self) -> &[Value] {
        match self {
            Value::Set(v) => v,
            _ => std::slice::from_ref(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f64_refuses_nan_and_folds_negative_zero() {
        assert!(F64::new(f64::NAN).is_none());
        assert!(F64::new(f64::INFINITY).is_none());
        assert_eq!(F64::new(-0.0), F64::new(0.0));
        assert!(F64::new(-1.5) < F64::new(0.0));
    }

    #[test]
    fn sets_are_sorted_without_duplicates_and_never_empty() {
        let s = Value::set(vec![Value::Int(5), Value::Int(1), Value::Int(5)]).unwrap();
        assert_eq!(s, Value::Set(vec![Value::Int(1), Value::Int(5)]));
        assert_eq!(Value::set(vec![]), None);
    }

    #[test]
    fn lp_frames_with_a_little_endian_length() {
        let mut v = Vec::new();
        lp(&mut v, b"moirai-file-v1");
        assert_eq!(&v[..4], &[0x0E, 0, 0, 0]);
        assert_eq!(hex(&[0xAB, 0x01]), "ab01");
    }
}
