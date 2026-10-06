//! The scalar built-ins with LQ-specific meaning ([LQ/std §2.10]; [50 §2.6]): the hierarchy sets, the scoping
//! predicates over `applies_to` and `path_globs` ([F08 §5.4.3], §5.4.6; [RULES/pack-classes] PT-012, PT-013, PT-020),
//! `fits_role` over [RULES/role-write-policy] `role-status`, glob matching and glob overlap, and the time texts.

use super::val::V;
use super::view::Ev;
use crate::lq::lexer::{datetime_ms, duration_ms, iso_datetime};
use crate::value::{Nid, Value};

/// `YYYY-MM-DDTHH:MM:SSZ` of milliseconds since the epoch (UTC, whole seconds).
pub fn iso(ms: i64) -> String {
    let s = ms.div_euclid(1000);
    let days = s.div_euclid(86_400);
    let rem = s.rem_euclid(86_400);
    // Civil date of a day number (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// `datetime(s)` and `date(s)`: an ISO 8601 date or date-time text ([LQ/lexical §7.5]) in milliseconds; absent for any
/// other text ([50 §2.6]).
// spec: [LQ/std §2.10] datetime
pub fn parse_time(s: &str) -> V {
    iso_datetime(s).map_or(V::Absent, |n| V::Time(datetime_ms(&n)))
}

/// `duration(s)`: `<int><s|m|h|d|w>` ([LQ/lexical §5.6]) or ISO 8601 `P[nW][nD][T[nH][nM][nS]]`, in milliseconds;
/// absent for any other text.
// spec: [LQ/std §2.10] duration
pub fn parse_duration(s: &str) -> V {
    if let Some(ms) = duration_ms(s) {
        return V::Dur(ms);
    }
    let Some(rest) = s.strip_prefix('P') else {
        return V::Absent;
    };
    let mut total: i64 = 0;
    let mut time = false;
    let mut num = String::new();
    let mut any = false;
    for c in rest.chars() {
        match c {
            'T' if !time && num.is_empty() => time = true,
            '0'..='9' => num.push(c),
            _ => {
                let Ok(n) = num.parse::<i64>() else {
                    return V::Absent;
                };
                let per = match (c, time) {
                    ('W', false) => 604_800_000,
                    ('D', false) => 86_400_000,
                    ('H', true) => 3_600_000,
                    ('M', true) => 60_000,
                    ('S', true) => 1_000,
                    _ => return V::Absent,
                };
                let Some(t) = n.checked_mul(per).and_then(|x| total.checked_add(x)) else {
                    return V::Absent;
                };
                total = t;
                num.clear();
                any = true;
            }
        }
    }
    if any && num.is_empty() {
        V::Dur(total)
    } else {
        V::Absent
    }
}

/// One character position of a glob segment ([F08 §5.4.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    /// A literal scalar value.
    Lit(char),
    /// `?`.
    One,
    /// `*`.
    Star,
    /// A class: ranges, negated.
    Class(Vec<(char, char)>, bool),
}

/// The tokens of one segment pattern; `None` for `**`.
fn seg_tokens(s: &str) -> Option<Vec<Tok>> {
    if s == "**" {
        return None;
    }
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        match cs[i] {
            '*' => out.push(Tok::Star),
            '?' => out.push(Tok::One),
            '[' => match cs[i + 1..].iter().position(|&c| c == ']') {
                Some(close) if close > 0 => {
                    let body = &cs[i + 1..i + 1 + close];
                    let (neg, body) = match body.first() {
                        Some('!') => (true, &body[1..]),
                        _ => (false, body),
                    };
                    let mut ranges = Vec::new();
                    let mut k = 0;
                    while k < body.len() {
                        if k + 2 < body.len() && body[k + 1] == '-' {
                            ranges.push((body[k], body[k + 2]));
                            k += 3;
                        } else {
                            ranges.push((body[k], body[k]));
                            k += 1;
                        }
                    }
                    out.push(Tok::Class(ranges, neg));
                    i += close + 1;
                }
                _ => out.push(Tok::Lit('[')),
            },
            c => out.push(Tok::Lit(c)),
        }
        i += 1;
    }
    Some(out)
}

/// Whether some scalar value (other than `/`) matches both one-character tokens.
fn chars_meet(a: &Tok, b: &Tok) -> bool {
    let in_class =
        |c: char, r: &[(char, char)], neg: bool| r.iter().any(|(x, y)| *x <= c && c <= *y) != neg;
    match (a, b) {
        (Tok::Lit(x), Tok::Lit(y)) => x == y,
        (Tok::Lit(c), Tok::One) | (Tok::One, Tok::Lit(c)) => *c != '/',
        (Tok::Lit(c), Tok::Class(r, n)) | (Tok::Class(r, n), Tok::Lit(c)) => in_class(*c, r, *n),
        (Tok::One, Tok::One) => true,
        (Tok::One, Tok::Class(r, n)) | (Tok::Class(r, n), Tok::One) => *n || !r.is_empty(),
        (Tok::Class(r1, n1), Tok::Class(r2, n2)) => match (n1, n2) {
            (true, true) => true,
            (false, false) => r1
                .iter()
                .any(|(a, b)| r2.iter().any(|(c, d)| a.max(c) <= b.min(d))),
            _ => {
                let (pos, neg) = if *n1 { (r2, r1) } else { (r1, r2) };
                // A point of a positive range outside every negated range: a range start, or one past a negated
                // range's end, inside the positive range.
                pos.iter().any(|&(lo, hi)| {
                    let mut cands = vec![lo];
                    for &(_, e) in neg {
                        if let Some(c) = char::from_u32(e as u32 + 1) {
                            cands.push(c);
                        }
                    }
                    cands
                        .into_iter()
                        .any(|c| lo <= c && c <= hi && !in_class(c, neg, false))
                })
            }
        },
        _ => false,
    }
}

/// Whether two segment patterns match a common segment.
fn seg_meet(p: &[Tok], q: &[Tok]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![(0usize, 0usize)];
    while let Some((a, b)) = stack.pop() {
        if !seen.insert((a, b)) {
            continue;
        }
        if a == p.len() && b == q.len() {
            return true;
        }
        let pa = p.get(a);
        let qb = q.get(b);
        if pa == Some(&Tok::Star) {
            stack.push((a + 1, b));
            if b < q.len() {
                stack.push((a, b + 1));
            }
        }
        if qb == Some(&Tok::Star) {
            stack.push((a, b + 1));
            if a < p.len() {
                stack.push((a + 1, b));
            }
        }
        if let (Some(x), Some(y)) = (pa, qb)
            && *x != Tok::Star
            && *y != Tok::Star
            && chars_meet(x, y)
        {
            stack.push((a + 1, b + 1));
        }
    }
    false
}

/// Glob overlap ([50 §2.6] `applies`; [RULES/pack-classes] PT-020): whether some path matches both globs (a path is a
/// glob without wildcards). `**` stands for zero or more whole segments ([F08 §5.4.3]).
// spec: [F08 §5.4.3]
pub fn glob_overlap(a: &str, b: &str) -> bool {
    let g: Vec<Option<Vec<Tok>>> = a.split('/').map(seg_tokens).collect();
    let h: Vec<Option<Vec<Tok>>> = b.split('/').map(seg_tokens).collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![(0usize, 0usize)];
    while let Some((i, j)) = stack.pop() {
        if !seen.insert((i, j)) {
            continue;
        }
        if i == g.len() && j == h.len() {
            return true;
        }
        let gi = g.get(i);
        let hj = h.get(j);
        if let Some(None) = gi {
            stack.push((i + 1, j));
            if j < h.len() {
                stack.push((i, j + 1));
            }
        }
        if let Some(None) = hj {
            stack.push((i, j + 1));
            if i < g.len() {
                stack.push((i + 1, j));
            }
        }
        if let (Some(Some(p)), Some(Some(q))) = (gi, hj)
            && seg_meet(p, q)
        {
            stack.push((i + 1, j + 1));
        }
    }
    false
}

/// The tagged elements of a node's `applies_to` ([F08 §5.4.6]); empty when absent.
fn applies_to(ev: &Ev<'_>, n: Nid) -> Option<Vec<String>> {
    let x = ev.node(n)?;
    ev.st().schema.field(&x.kind, "applies_to")?;
    Some(match x.fields.get("applies_to") {
        Some(v) => v
            .elems()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    })
}

/// `applies(k, glob)` ([50 §2.6]): `k.applies_to` (an empty one means `*`; its `path:` globs) or an area's
/// `path_globs` overlaps the glob; false for kinds with neither field.
// spec: [50 §2.6] applies
pub fn applies(ev: &Ev<'_>, n: Nid, glob: &str) -> bool {
    if let Some(tags) = applies_to(ev, n) {
        return tags.is_empty()
            || tags.iter().any(|t| {
                t == "*"
                    || t.strip_prefix("path:")
                        .is_some_and(|g| glob_overlap(g, glob))
            });
    }
    match ev.node(n) {
        Some(x) if x.kind == "area" => x.fields.get("path_globs").is_some_and(|v| {
            v.elems()
                .iter()
                .filter_map(Value::as_str)
                .any(|g| glob_overlap(g, glob))
        }),
        _ => false,
    }
}

/// `applies_role(k, role)` and `applies_phase(k, phase)` ([RULES/pack-classes] PT-013): `applies_to` is empty, or holds
/// `*`, or holds `<tag>:<value>`.
// spec: [RULES/pack-classes] PT-013
pub fn applies_tag(ev: &Ev<'_>, n: Nid, tag: &str, value: &str) -> bool {
    applies_to(ev, n).is_some_and(|tags| {
        tags.is_empty()
            || tags.iter().any(|t| {
                t == "*"
                    || t.strip_prefix(tag)
                        .and_then(|r| r.strip_prefix(':'))
                        .is_some_and(|v| v == value)
            })
    })
}

/// `fits_role(t, role)` ([LQ/std §2.10]): a `role-status` row for the role whose kind is t's kind or `*` and whose target
/// status is `done` or `*`.
// spec: [LQ/std §2.10] fits_role
pub fn fits_role(ev: &Ev<'_>, n: Nid, role: &str) -> bool {
    let Some(x) = ev.node(n) else { return false };
    crate::rules::rules()
        .table("role-status")
        .rows
        .iter()
        .any(|r| {
            r.tok("role") == role
                && (r.tok("kind") == x.kind || r.tok("kind") == "*")
                && (r.tok("to") == "done" || r.tok("to") == "*")
        })
}

/// `subtree(n [, depth])`, `descendants`: n (or not) and its `CHILD_OF` descendants within `depth` levels (unbounded
/// when absent), ascending by id ([LQ/std §2.10]).
// spec: [LQ/std §2.10] subtree
pub fn subtree(ev: &Ev<'_>, n: Nid, depth: Option<i64>, inclusive: bool) -> Vec<Nid> {
    let mut out = Vec::new();
    if ev.st().live(n).is_none() {
        return out;
    }
    let mut level = vec![n];
    let mut d = 0;
    if inclusive {
        out.push(n);
    }
    // The hierarchy is a forest (I4), so each node is reached once; budgets are not modelled ([60 §4.2]).
    let mut seen = std::collections::BTreeSet::from([n]);
    while !level.is_empty() && depth.is_none_or(|m| d < m) {
        let mut next = Vec::new();
        for p in &level {
            if let Some(cs) = ev.ix().children.get(p) {
                next.extend(cs.iter().copied().filter(|c| seen.insert(*c)));
            }
        }
        d += 1;
        out.extend(next.iter().copied());
        level = next;
    }
    out.sort();
    out
}

/// `ancestors(n)`: the strict `parent` chain ([LQ/std §2.10]), ascending by id.
// spec: [LQ/std §2.10] ancestors
pub fn ancestors(ev: &Ev<'_>, n: Nid) -> Vec<Nid> {
    let mut v = ev.ix().ancestors(n);
    v.sort();
    v
}

/// `children(n)`: the direct live children, ascending by id ([LQ/std §2.10]).
// spec: [LQ/std §2.10] children
pub fn children(ev: &Ev<'_>, n: Nid) -> Vec<Nid> {
    let mut v = ev.ix().children.get(&n).cloned().unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trips_datetime_ms() {
        let t = datetime_ms("2026-09-25T12:03:00Z");
        assert_eq!(iso(t), "2026-09-25T12:03:00Z");
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn durations_parse_in_both_forms() {
        assert!(matches!(parse_duration("15m"), V::Dur(900_000)));
        assert!(matches!(parse_duration("P1DT2H"), V::Dur(93_600_000)));
        assert!(parse_duration("P").is_absent());
        assert!(parse_duration("1x").is_absent());
    }

    #[test]
    fn globs_overlap_by_a_common_path() {
        assert!(glob_overlap("crates/ecs/**", "crates/ecs/world.rs"));
        assert!(glob_overlap("a/*/x", "a/y/*"));
        assert!(glob_overlap("**", "docs/x.md"));
        assert!(!glob_overlap("crates/ecs/**", "docs/**"));
        assert!(glob_overlap("src/[a-c]*.rs", "src/b?.rs"));
        assert!(!glob_overlap("src/[a-c]x", "src/[!a-z]x"));
        assert!(glob_overlap("src/[!a]x", "src/[a-b]x"));
        assert!(!glob_overlap("a/b", "a/b/c"));
        assert!(glob_overlap("a/**/c", "a/c"));
    }
}
