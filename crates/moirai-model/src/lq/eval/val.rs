//! The evaluator's values ([50 §3.3], §3.5; [LQ/std §2.11], §2.12): the absent value, the scalar types of LQ, nodes,
//! edges, revisions, lists, maps and integer ranges, with the three relations the semantics needs — the comparison of
//! `=` and of the ordered operators under the absent-value rule (two-valued, [50 §3.3]), the grouping equality of keys,
//! `DISTINCT` and set operations (all absent values equal), and the total order of results ([50 §3.5]: natural orders,
//! absent last).

use crate::value::{Nid, Uid};
use std::cmp::Ordering;

/// An enumeration value: its declared name, its `sort_rank` in the view's schema ([50 §3.5]), and the field it is a
/// value of, so that a text compared with it by an ordered operator ranks in that field ([50 §3.5]: enumerations order
/// by declared rank, whatever expression produced the operands).
#[derive(Clone, Debug)]
pub struct EnumV {
    /// The declared name.
    pub name: String,
    /// The value's rank (F2 `sort_rank`).
    pub rank: u32,
    /// The kind whose field the value belongs to.
    pub kind: String,
    /// The field.
    pub field: String,
}

impl PartialEq for EnumV {
    /// Two enumeration values are the same value when their names and ranks are.
    fn eq(&self, o: &EnumV) -> bool {
        self.name == o.name && self.rank == o.rank
    }
}

impl Eq for EnumV {}

/// An edge: its key ([50 §3.4] rule 1: `(src, kind, dst)` plus the anchor discriminator of an `at` edge).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeV {
    /// The source.
    pub src: Nid,
    /// The stored kind name.
    pub kind: String,
    /// The destination.
    pub dst: Nid,
    /// The anchor uid of an `at` edge.
    pub disc: Option<Uid>,
}

/// A value of the evaluator.
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    /// The absent value ([50 §3.3]).
    Absent,
    /// `bool`.
    Bool(bool),
    /// `int`.
    Int(i64),
    /// `float`: never a NaN.
    Float(f64),
    /// `text` (also kind names and symbols).
    Text(String),
    /// An enumeration value.
    Enum(Box<EnumV>),
    /// A timestamp: milliseconds since the Unix epoch, UTC.
    Time(i64),
    /// A duration in milliseconds.
    Dur(i64),
    /// A node by `#N`.
    Node(Nid),
    /// An edge.
    Edge(Box<EdgeV>),
    /// A revision: a commit by its store sequence number.
    Rev(u64),
    /// A list (sets are lists in their natural order).
    List(Vec<V>),
    /// A map with its members in order (`lease`, the structured values of `diff` rows).
    Map(Vec<(String, V)>),
    /// `range<int>`: absent ends are open.
    Range(Option<i64>, Option<i64>),
}

impl V {
    /// A text value.
    pub fn text(s: impl Into<String>) -> V {
        V::Text(s.into())
    }

    /// Whether the value is absent.
    pub fn is_absent(&self) -> bool {
        matches!(self, V::Absent)
    }

    /// The node of a node value.
    pub fn node(&self) -> Option<Nid> {
        match self {
            V::Node(n) => Some(*n),
            _ => None,
        }
    }

    /// The text of a text or enumeration value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            V::Text(s) => Some(s),
            V::Enum(e) => Some(&e.name),
            _ => None,
        }
    }

    /// The boolean of a bool value; absent and every other value are `None`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            V::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The integer of an int value.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            V::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The elements of a list; a single non-list value is a one-element list, absent is empty.
    pub fn elems(&self) -> &[V] {
        match self {
            V::List(v) => v,
            V::Absent => &[],
            other => std::slice::from_ref(other),
        }
    }

    /// The member of a map.
    pub fn member(&self, k: &str) -> V {
        match self {
            V::Map(m) => m
                .iter()
                .find(|(n, _)| n == k)
                .map_or(V::Absent, |(_, v)| v.clone()),
            _ => V::Absent,
        }
    }

    /// The type tag of the total order: values of different types order by it ([50 §3.5]; absent last).
    fn tag(&self) -> u8 {
        match self {
            V::Bool(_) => 1,
            V::Int(_) | V::Float(_) => 2,
            V::Time(_) => 3,
            V::Dur(_) => 4,
            V::Enum(_) => 5,
            V::Text(_) => 6,
            V::Node(_) => 7,
            V::Edge(_) => 8,
            V::Rev(_) => 9,
            V::Range(..) => 10,
            V::List(_) => 11,
            V::Map(_) => 12,
            V::Absent => 255,
        }
    }
}

/// An int against a float by their exact values ([50 §3.3]: numbers compare numerically): `Equal` exactly when the
/// float is integral and equals the integer, so `2 = 2.0`, `2 <= 2.0` and `2 >= 2.0` all hold. An LQ float is never
/// NaN (every operator refuses a non-finite result); one would sort after every int.
// spec: [50 §3.3] equality
fn cmp_int_float(i: i64, f: f64) -> Ordering {
    if f.is_nan() {
        return Ordering::Less;
    }
    // 2^63 is exactly representable; every float in [-2^63, 2^63) truncates to an i64 without rounding.
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    if f >= TWO63 {
        return Ordering::Less;
    }
    if f < -TWO63 {
        return Ordering::Greater;
    }
    let t = f.trunc();
    match i.cmp(&(t as i64)) {
        Ordering::Equal => float_cmp(0.0, f - t),
        o => o,
    }
}

/// Two floats numerically, `0.0` and `-0.0` equal ([50 §3.3]); the IEEE total order otherwise.
fn float_cmp(x: f64, y: f64) -> Ordering {
    if x == y {
        Ordering::Equal
    } else {
        x.total_cmp(&y)
    }
}

/// The total order of results ([50 §3.5]): natural orders within a type — numbers numerically (an int and a float of
/// the same value, and `0.0` and `-0.0`, are equal, as `=` makes them, [50 §3.3]), enumerations by rank then name, text
/// by bytes, nodes by id, edges by edge key, revisions by sequence number, lists and maps element by element — and
/// absent last. Values of different types order by type.
// spec: [50 §3.5] total order
pub fn cmp_total(a: &V, b: &V) -> Ordering {
    match (a, b) {
        (V::Absent, V::Absent) => Ordering::Equal,
        (V::Absent, _) => Ordering::Greater,
        (_, V::Absent) => Ordering::Less,
        (V::Bool(x), V::Bool(y)) => x.cmp(y),
        (V::Int(x), V::Int(y)) => x.cmp(y),
        (V::Float(x), V::Float(y)) => float_cmp(*x, *y),
        (V::Int(x), V::Float(y)) => cmp_int_float(*x, *y),
        (V::Float(x), V::Int(y)) => cmp_int_float(*y, *x).reverse(),
        (V::Time(x), V::Time(y)) | (V::Dur(x), V::Dur(y)) => x.cmp(y),
        (V::Enum(x), V::Enum(y)) => x.rank.cmp(&y.rank).then_with(|| x.name.cmp(&y.name)),
        (V::Text(x), V::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (V::Node(x), V::Node(y)) => x.cmp(y),
        (V::Edge(x), V::Edge(y)) => x.cmp(y),
        (V::Rev(x), V::Rev(y)) => x.cmp(y),
        (V::Range(a1, b1), V::Range(a2, b2)) => (a1, b1).cmp(&(a2, b2)),
        (V::List(x), V::List(y)) => {
            for (p, q) in x.iter().zip(y) {
                let o = cmp_total(p, q);
                if o != Ordering::Equal {
                    return o;
                }
            }
            x.len().cmp(&y.len())
        }
        (V::Map(x), V::Map(y)) => {
            for ((kp, p), (kq, q)) in x.iter().zip(y) {
                let o = kp.cmp(kq).then_with(|| cmp_total(p, q));
                if o != Ordering::Equal {
                    return o;
                }
            }
            x.len().cmp(&y.len())
        }
        _ => a.tag().cmp(&b.tag()),
    }
}

/// A sort key's order: `desc` reverses it, and absent sorts last in both directions ([50 §3.3]).
// spec: [50 §3.5] sort keys
pub fn cmp_key(a: &V, b: &V, desc: bool) -> Ordering {
    match (a.is_absent(), b.is_absent()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ if desc => cmp_total(b, a),
        _ => cmp_total(a, b),
    }
}

/// Grouping equality ([50 §3.3]: grouping keys, `DISTINCT` and set operations): it differs from `=` only in that all
/// absent values are equal to each other; numbers group by numeric value (`2` and `2.0` are one group), and the group's
/// first row in binding order is its representative.
// spec: [50 §3.3] grouping
pub fn same(a: &V, b: &V) -> bool {
    cmp_total(a, b) == Ordering::Equal
}

/// `a = b` with present operands: numbers numerically, an enumeration against a text by name, lists element by
/// element; `None` when either side is absent (the caller applies the absent-value rule).
// spec: [50 §3.3] equality
pub fn eq(a: &V, b: &V) -> Option<bool> {
    Some(match (a, b) {
        (V::Absent, _) | (_, V::Absent) => return None,
        (V::Enum(x), V::Text(y)) | (V::Text(y), V::Enum(x)) => x.name == *y,
        (V::Enum(x), V::Enum(y)) => x.name == y.name,
        (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => cmp_total(a, b) == Ordering::Equal,
        (V::List(x), V::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| eq(p, q) == Some(true))
        }
        _ => a.tag() == b.tag() && cmp_total(a, b) == Ordering::Equal,
    })
}

/// An ordered comparison of present operands of comparable types ([50 §3.3]): numbers numerically, `Equal` on equal
/// values; `None` when either side is absent or the types do not compare. An enumeration and a text compare only once
/// the text is ranked in the enumeration's field (the evaluator's comparison does that); here they do not compare.
// spec: [50 §3.3] comparison
pub fn ord(a: &V, b: &V) -> Option<Ordering> {
    match (a, b) {
        (V::Absent, _) | (_, V::Absent) => None,
        _ if a.tag() == b.tag() => Some(cmp_total(a, b)),
        _ => None,
    }
}

/// Sorts values in the natural order ([50 §3.5]; `collect()` and set-valued fields).
pub fn sort_natural(v: &mut [V]) {
    v.sort_by(cmp_total);
}

/// `round(x, digits)` half-even at `digits` decimals of the exact binary value of `x`: the rounding of `search()`'s
/// score ([LQ/envelope §5.2]). The decimal expansion of an f64 is exact, so a tie is a value exactly halfway in binary
/// (`0.0625` gives `0.062`), and `0.0125`, whose binary value lies above the half, gives `0.013`; a negative zero
/// result is `0`.
// spec: [LQ/envelope §5.2]
pub fn round_half_even(x: f64, digits: i32) -> f64 {
    let d = usize::try_from(digits.max(0)).expect("a non-negative digit count");
    let out: f64 = format!("{x:.d$}")
        .parse()
        .expect("a formatted float parses");
    if out == 0.0 { 0.0 } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en(name: &str, rank: u32) -> V {
        V::Enum(Box::new(EnumV {
            name: name.into(),
            rank,
            kind: "task".into(),
            field: "status".into(),
        }))
    }

    #[test]
    fn absent_sorts_last_in_both_directions() {
        let mut v = [V::Int(3), V::Absent, V::Int(1)];
        v.sort_by(|a, b| cmp_key(a, b, false));
        assert!(matches!(v, [V::Int(1), V::Int(3), V::Absent]));
        v.sort_by(|a, b| cmp_key(a, b, true));
        assert!(matches!(v, [V::Int(3), V::Int(1), V::Absent]));
    }

    #[test]
    fn enumerations_order_by_rank_and_equal_text_by_name() {
        assert_eq!(cmp_total(&en("critical", 0), &en("low", 3)), Ordering::Less);
        assert_eq!(eq(&en("open", 1), &V::text("open")), Some(true));
        assert_eq!(eq(&V::Absent, &V::text("open")), None);
        assert!(same(&V::Absent, &V::Absent));
    }

    #[test]
    fn numbers_compare_across_int_and_float() {
        assert_eq!(eq(&V::Int(2), &V::Float(2.0)), Some(true));
        assert_eq!(ord(&V::Int(2), &V::Float(2.5)), Some(Ordering::Less));
        assert_eq!(ord(&V::Int(2), &V::Float(2.0)), Some(Ordering::Equal));
        assert_eq!(ord(&V::Float(2.0), &V::Int(2)), Some(Ordering::Equal));
        assert_eq!(cmp_total(&V::Int(2), &V::Float(2.0)), Ordering::Equal);
        assert!(same(&V::Float(0.0), &V::Float(-0.0)));
        assert_eq!(eq(&V::Float(0.0), &V::Float(-0.0)), Some(true));
        // Beyond 2^53 the float is compared with the exact integer, not with the integer's rounding.
        assert_eq!(
            ord(
                &V::Int(9_007_199_254_740_993),
                &V::Float(9_007_199_254_740_992.0)
            ),
            Some(Ordering::Greater)
        );
        assert_eq!(
            ord(&V::Int(i64::MAX), &V::Float(9.3e18)),
            Some(Ordering::Less)
        );
        assert_eq!(
            ord(&V::Int(i64::MIN), &V::Float(-9.3e18)),
            Some(Ordering::Greater)
        );
        assert_eq!(ord(&V::Int(-3), &V::Float(-2.5)), Some(Ordering::Less));
    }

    #[test]
    fn an_enumeration_and_a_text_do_not_compare_unranked() {
        assert_eq!(ord(&en("low", 3), &V::text("high")), None);
        assert_eq!(ord(&V::text("high"), &en("low", 3)), None);
    }

    #[test]
    fn half_even_rounding_at_three_decimals() {
        assert_eq!(round_half_even(7.4125, 3), 7.412);
        assert_eq!(round_half_even(28.57142, 1), 28.6);
        assert_eq!(round_half_even(-0.0001, 3), 0.0);
        assert!(round_half_even(-0.0001, 3).is_sign_positive());
        // The exact binary value decides: 0.0125 lies above the half, 0.0625 is an exact tie.
        assert_eq!(round_half_even(0.0125, 3), 0.013);
        assert_eq!(round_half_even(0.0625, 3), 0.062);
        assert_eq!(round_half_even(2.5, 0), 2.0);
    }
}
