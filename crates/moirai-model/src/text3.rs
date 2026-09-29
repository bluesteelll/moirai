//! The text rule of the typed merge ([F12 §7.5]): lines, the line diff HD, diff3 and the removed-text guard, written from
//! the specification's text (the model shares no code with `moirai-diff`, [60 §4.5]). HD is computed by its definition:
//! strip the common prefix and suffix, then split at the maximal matching region of least (rarity, −length, j, i), with
//! rarity at most `DIFF_MAX_RARITY`; every region is enumerated, which is quadratic and fine at model scale.

use std::collections::{BTreeMap, HashMap};

/// `DIFF_MAX_RARITY`, part of format v1 ([F12 §7.5]).
pub const DIFF_MAX_RARITY: usize = 64;

/// The bound of a text value and of a body in bytes ([F08 §5.3], [F08 §7.2]; [F12 §7.5] "Length").
pub const MAX_TEXT: usize = 65_536;

/// lines(x): the maximal runs of bytes ending with LF, and a final run without LF ([F12 §7.5] "Lines").
// spec: [F12 §7.5] Lines
pub fn lines(x: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in x.bytes().enumerate() {
        if b == b'\n' {
            out.push(&x[start..=i]);
            start = i + 1;
        }
    }
    if start < x.len() {
        out.push(&x[start..]);
    }
    out
}

/// HD(P, Q): the matched pairs (i, j), strictly increasing in both coordinates ([F12 §7.5]).
// spec: [F12 §7.5] HD
pub fn hd(p: &[&str], q: &[&str]) -> Vec<(usize, usize)> {
    let mut qpos: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, l) in q.iter().enumerate() {
        qpos.entry(*l).or_default().push(j);
    }
    let mut out = Vec::new();
    let mut work = vec![(0usize, p.len(), 0usize, q.len())];
    while let Some((mut p0, mut p1, mut q0, mut q1)) = work.pop() {
        while p0 < p1 && q0 < q1 && p[p0] == q[q0] {
            out.push((p0, q0));
            p0 += 1;
            q0 += 1;
        }
        while p0 < p1 && q0 < q1 && p[p1 - 1] == q[q1 - 1] {
            out.push((p1 - 1, q1 - 1));
            p1 -= 1;
            q1 -= 1;
        }
        if p0 == p1 || q0 == q1 {
            continue;
        }
        let mut cnt: HashMap<&str, usize> = HashMap::new();
        for l in &p[p0..p1] {
            *cnt.entry(*l).or_default() += 1;
        }
        // The candidate with the least (rarity, −len, j, i).
        let mut best: Option<(usize, std::cmp::Reverse<usize>, usize, usize)> = None;
        for i in p0..p1 {
            let Some(js) = qpos.get(p[i]) else { continue };
            for &j in js.iter().filter(|j| **j >= q0 && **j < q1) {
                if i > p0 && j > q0 && p[i - 1] == q[j - 1] {
                    continue; // not maximal to the left
                }
                let mut len = 0;
                let mut rarity = usize::MAX;
                while i + len < p1 && j + len < q1 && p[i + len] == q[j + len] {
                    rarity = rarity.min(cnt[p[i + len]]);
                    len += 1;
                }
                if rarity > DIFF_MAX_RARITY {
                    continue;
                }
                let key = (rarity, std::cmp::Reverse(len), j, i);
                if best.is_none_or(|b| key < b) {
                    best = Some(key);
                }
            }
        }
        let Some((_, std::cmp::Reverse(len), j, i)) = best else {
            continue;
        };
        for k in 0..len {
            out.push((i + k, j + k));
        }
        work.push((p0, i, q0, j));
        work.push((i + len, p1, j + len, q1));
    }
    out.sort_unstable();
    out
}

/// One chunk of diff3's walk.
#[derive(Debug, PartialEq, Eq)]
enum Chunk<'a> {
    Stable(&'a [&'a str]),
    Unstable(&'a [&'a str], &'a [&'a str], &'a [&'a str]),
}

/// diff3(base, ours, theirs) ([F12 §7.5]): the merged text when every chunk resolves, else `None` for a
/// conflicting chunk.
// spec: [F12 §7.5] diff3
pub fn diff3(b: &str, o: &str, t: &str) -> Option<String> {
    let (ol, al, bl) = (lines(b), lines(o), lines(t));
    let ma: BTreeMap<usize, usize> = hd(&ol, &al).into_iter().collect();
    let mb: BTreeMap<usize, usize> = hd(&ol, &bl).into_iter().collect();
    let (mut p, mut q, mut r) = (0usize, 0usize, 0usize);
    let mut chunks = Vec::new();
    loop {
        if p == ol.len() && q == al.len() && r == bl.len() {
            break;
        }
        let mut s = 0;
        while p + s < ol.len()
            && ma.get(&(p + s)) == Some(&(q + s))
            && mb.get(&(p + s)) == Some(&(r + s))
        {
            s += 1;
        }
        if s > 0 {
            chunks.push(Chunk::Stable(&ol[p..p + s]));
            p += s;
            q += s;
            r += s;
            continue;
        }
        let j = (p..ol.len()).find(|j| ma.contains_key(j) && mb.contains_key(j));
        match j {
            Some(j) => {
                let (qa, rb) = (ma[&j], mb[&j]);
                chunks.push(Chunk::Unstable(&ol[p..j], &al[q..qa], &bl[r..rb]));
                p = j;
                q = qa;
                r = rb;
            }
            None => {
                chunks.push(Chunk::Unstable(&ol[p..], &al[q..], &bl[r..]));
                break;
            }
        }
    }
    let mut out = String::new();
    for c in chunks {
        match c {
            Chunk::Stable(x) => out.extend(x.iter().copied()),
            Chunk::Unstable(oc, ac, bc) => {
                let pick = if ac == oc {
                    bc
                } else if bc == oc || ac == bc {
                    ac
                } else {
                    return None;
                };
                out.extend(pick.iter().copied());
            }
        }
    }
    Some(out)
}

/// The removed-text guard fails for the diff3 result `r` ([RULES/merge-table] CS-012; [F12 §7.5]): for S = ours or
/// theirs, the multiset lines(S) − lines(r) is not contained in lines(base).
// spec: [F12 §7.5] guard
pub fn guard_fails(b: &str, o: &str, t: &str, r: &str) -> bool {
    let count = |x: &str| -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for l in lines(x) {
            *m.entry(l.to_string()).or_default() += 1;
        }
        m
    };
    let (bc, rc) = (count(b), count(r));
    [o, t].iter().any(|s| {
        count(s).into_iter().any(|(l, n)| {
            let removed = n.saturating_sub(rc.get(&l).copied().unwrap_or(0));
            removed > bc.get(&l).copied().unwrap_or(0)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn lines_split_after_each_lf() {
        assert_eq!(lines(""), Vec::<&str>::new());
        assert_eq!(lines("a\nb"), vec!["a\n", "b"]);
        assert_eq!(lines("a\n\n"), vec!["a\n", "\n"]);
    }

    /// [F12 §7.5]'s informative example.
    #[test]
    fn the_example_of_section_7_5() {
        assert_eq!(
            diff3("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\nd\n").as_deref(),
            Some("a\nB\nc\nd\n")
        );
    }

    #[test]
    fn a_line_both_sides_change_differently_conflicts() {
        assert_eq!(diff3("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n"), None);
        assert_eq!(
            diff3("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n").as_deref(),
            Some("a\nX\nc\n")
        );
        // Both sides append at the end: the unstable tail conflicts.
        assert_eq!(diff3("a\n", "a\nx\n", "a\ny\n"), None);
    }

    #[test]
    fn hd_prefers_the_rarest_longest_region() {
        let p = ["x\n", "a\n", "b\n", "x\n"];
        let q = ["a\n", "b\n", "y\n"];
        assert_eq!(hd(&p, &q), vec![(1, 0), (2, 1)]);
        // No common line: no pair.
        assert!(hd(&["a\n"], &["b\n"]).is_empty());
    }

    #[test]
    fn the_guard_catches_text_the_merge_dropped() {
        assert!(!guard_fails("a\nb\n", "a\n", "a\nb\nc\n", "a\nc\n"));
        assert!(guard_fails("a\n", "a\nz\n", "a\n", "a\n"));
    }

    fn text() -> impl Strategy<Value = String> {
        proptest::collection::vec(prop_oneof!["a\n", "b\n", "c\n", "d"], 0..8)
            .prop_map(|v| v.concat())
    }

    proptest! {
        /// HD's pairs are matching lines, strictly increasing in both coordinates.
        #[test]
        fn hd_is_a_common_subsequence(a in text(), b in text()) {
            let (p, q) = (lines(&a), lines(&b));
            let m = hd(&p, &q);
            for w in m.windows(2) {
                prop_assert!(w[0].0 < w[1].0 && w[0].1 < w[1].1);
            }
            for (i, j) in m {
                prop_assert_eq!(p[i], q[j]);
            }
        }

        /// A one-sided change merges to that side; equal sides merge to either.
        #[test]
        fn one_sided_changes_merge_to_the_changed_side(b in text(), x in text()) {
            prop_assert_eq!(diff3(&b, &b, &x), Some(x.clone()));
            prop_assert_eq!(diff3(&b, &x, &b), Some(x.clone()));
            prop_assert_eq!(diff3(&b, &x, &x), Some(x.clone()));
        }
    }
}
