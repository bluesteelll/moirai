//! HD against a literal transcription of [F12 §7.5] written here (and a plain per-box scan for mid sizes), under every
//! heap cap and chain choice, plus the diff3 walk of [F12 §7.5] as M3 will write it on this API, and complexity and
//! memory checks on large and worst-case inputs.

use std::cmp::Reverse;
use std::collections::HashMap;

use super::{
    Area, Differ, Entry, Inherit, MAX_LEN, Match, NO_KEY, Run, Stats, Tuning, UNBUILT, UNMATCHED,
    dense, diff, diff_lines, fits, heapify, hunks, keyed, line_of, pack, partners, rarity_of,
    sift_down, slot, unpack,
};
use crate::{Interner, LengthError, lines};
use proptest::prelude::*;

/// HD(P, Q) of [F12 §7.5], literally: every maximal region of every box is enumerated.
fn naive(p: &[u32], q: &[u32]) -> Vec<Match> {
    let mut out = Vec::new();
    naive_box(p, q, [0, p.len(), 0, q.len()], &mut out);
    out.sort_unstable();
    out
}

fn pair(i: usize, j: usize) -> Match {
    Match {
        p: i as u32,
        q: j as u32,
    }
}

fn naive_box(p: &[u32], q: &[u32], bx: [usize; 4], out: &mut Vec<Match>) {
    let [mut p0, mut p1, mut q0, mut q1] = bx;
    while p0 < p1 && q0 < q1 && p[p0] == q[q0] {
        out.push(pair(p0, q0));
        p0 += 1;
        q0 += 1;
    }
    while p0 < p1 && q0 < q1 && p[p1 - 1] == q[q1 - 1] {
        out.push(pair(p1 - 1, q1 - 1));
        p1 -= 1;
        q1 -= 1;
    }
    if p0 == p1 || q0 == q1 {
        return;
    }
    let mut cnt: HashMap<u32, usize> = HashMap::new();
    for &v in &p[p0..p1] {
        *cnt.entry(v).or_default() += 1;
    }
    let mut best: Option<(usize, Reverse<usize>, usize, usize)> = None;
    for i in p0..p1 {
        for j in q0..q1 {
            if p[i] != q[j] || (i > p0 && j > q0 && p[i - 1] == q[j - 1]) {
                continue;
            }
            let mut len = 0;
            while i + len < p1 && j + len < q1 && p[i + len] == q[j + len] {
                len += 1;
            }
            let rarity = (i..i + len).map(|x| cnt[&p[x]]).min().unwrap_or(usize::MAX);
            if rarity > 64 {
                continue;
            }
            let c = (rarity, Reverse(len), j, i);
            if best.is_none_or(|b| c < b) {
                best = Some(c);
            }
        }
    }
    let Some((_, Reverse(len), j, i)) = best else {
        return;
    };
    naive_box(p, q, [p0, i, q0, j], out);
    out.extend((0..len).map(|k| pair(i + k, j + k)));
    naive_box(p, q, [i + len, p1, j + len, q1], out);
}

/// Heap caps from one entry (a trim and a rescan at nearly every step, a group heap compaction at nearly every push)
/// to the default, and every choice of the child that continues a chain: all must give the definition's pairs.
const TUNINGS: [Tuning; 8] = [
    Tuning::DEFAULT,
    Tuning {
        inherit: Inherit::Smaller,
        ..Tuning::DEFAULT
    },
    Tuning {
        inherit: Inherit::Left,
        ..Tuning::DEFAULT
    },
    Tuning {
        inherit: Inherit::Right,
        ..Tuning::DEFAULT
    },
    Tuning {
        min_cap: 1,
        cap_div: usize::MAX,
        group_cap: 1,
        inherit: Inherit::Larger,
    },
    Tuning {
        min_cap: 2,
        cap_div: usize::MAX,
        group_cap: 2,
        inherit: Inherit::Smaller,
    },
    Tuning {
        min_cap: 3,
        cap_div: usize::MAX,
        group_cap: 1,
        inherit: Inherit::Left,
    },
    Tuning {
        min_cap: 1,
        cap_div: usize::MAX,
        group_cap: 3,
        inherit: Inherit::Right,
    },
];

fn run(tuning: Tuning, p: &[u32], q: &[u32]) -> (Vec<Match>, Stats) {
    let mut d = Differ::with_tuning(tuning);
    let mut out = Vec::new();
    d.diff(p, q, &mut out).unwrap();
    (out, d.engine.stats)
}

fn check_all_tunings(p: &[u32], q: &[u32]) -> Result<(), TestCaseError> {
    let want = naive(p, q);
    for t in TUNINGS {
        let (got, _) = run(t, p, q);
        prop_assert_eq!(&got, &want, "tuning {:?}", t);
    }
    Ok(())
}

/// A sequence deterministic in `seed` over ids below `alphabet` (xorshift).
fn seq(seed: u64, len: usize, alphabet: u32) -> Vec<u32> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % u64::from(alphabet)) as u32
        })
        .collect()
}

/// Applies `edits` (position, kind, id) to `base`: 0 deletes, 1 inserts, 2 replaces.
fn edited(base: &[u32], edits: &[(usize, u8, u32)]) -> Vec<u32> {
    let mut v = base.to_vec();
    for &(at, kind, id) in edits {
        let at = if v.is_empty() { 0 } else { at % (v.len() + 1) };
        match kind % 3 {
            0 if at < v.len() => {
                v.remove(at);
            }
            1 => v.insert(at, id),
            _ if at < v.len() => v[at] = id,
            _ => v.push(id),
        }
    }
    v
}

#[test]
fn trivial_inputs() {
    assert!(diff(&[], &[]).unwrap().is_empty());
    assert!(diff(&[1, 2], &[]).unwrap().is_empty());
    assert!(diff(&[], &[1, 2]).unwrap().is_empty());
    assert_eq!(
        diff(&[5, 6, 7], &[5, 6, 7]).unwrap(),
        [pair(0, 0), pair(1, 1), pair(2, 2)]
    );
    assert!(diff(&[1, 2, 3], &[4, 5, 6]).unwrap().is_empty());
    // Ids of Q beyond every id of P never match.
    assert_eq!(diff(&[0, 1], &[9, 1, 7]).unwrap(), [pair(1, 1)]);
}

#[test]
fn choice_order_rarity_then_length_then_j_then_i() {
    // Prefix and suffix differ, so the middle regions compete. Line 1 occurs twice in P: its lone second copy has
    // rarity 2, while the region 1 2 3 through the unique lines 2 and 3 has rarity 1.
    let p = [9, 1, 2, 3, 1, 8];
    let q = [7, 1, 2, 3, 6];
    assert_eq!(diff(&p, &q).unwrap(), naive(&p, &q));
    assert_eq!(diff(&p, &q).unwrap(), [pair(1, 1), pair(2, 2), pair(3, 3)]);
    // Equal rarity: the longer region wins.
    let p = [9, 1, 5, 2, 3, 8];
    let q = [7, 2, 3, 5, 1, 6];
    assert_eq!(diff(&p, &q).unwrap(), [pair(3, 1), pair(4, 2)]);
    // Equal rarity and length: the least j, then the least i.
    let p = [9, 1, 2, 8];
    let q = [7, 2, 1, 6];
    assert_eq!(diff(&p, &q).unwrap(), [pair(2, 1)]);
}

#[test]
fn rarity_limit_is_64() {
    // A line with 64 copies in the box is a candidate; with 65 it is not.
    for (copies, matched) in [(64usize, true), (65, false)] {
        let mut p = vec![100];
        p.extend(std::iter::repeat_n(1, copies));
        p.push(101);
        let q = vec![102, 1, 103];
        let got = diff(&p, &q).unwrap();
        assert_eq!(got, naive(&p, &q));
        assert_eq!(!got.is_empty(), matched, "copies = {copies}");
        if matched {
            // The least (j, i): the first copy.
            assert_eq!(got, [pair(1, 1)]);
        }
    }
    // A region is a candidate when any one of its lines is rare: common line 1 next to a unique line 2.
    let mut p = vec![100];
    p.extend(std::iter::repeat_n(1, 70));
    p.extend([2, 1, 101]);
    let q = vec![102, 1, 2, 1, 103];
    assert_eq!(diff(&p, &q).unwrap(), naive(&p, &q));
    assert_eq!(
        diff(&p, &q).unwrap(),
        [pair(70, 1), pair(71, 2), pair(72, 3)]
    );
}

#[test]
fn counts_are_taken_in_the_box() {
    // In the whole of P line 1 has 66 copies, but after the first split the right box holds only 2: it becomes
    // rare there.
    let mut p = vec![50];
    p.extend(std::iter::repeat_n(1, 64));
    p.extend([7, 60, 1, 61, 1, 62]);
    let q = vec![51, 7, 63, 1, 64];
    assert_eq!(diff(&p, &q).unwrap(), naive(&p, &q));
}

#[test]
fn a_count_falling_to_64_makes_new_candidates() {
    // Line v (id 1) has 73 copies in the first box, so it is no candidate there, and the box's candidates are C
    // (ids 5 6 7) and D. Choosing C leaves 3 copies of v in the left box and 70 in the right. Whichever child
    // continues the chain, v's regions in the left box become candidates there and must match.
    let v = 1;
    let (d, e) = (8, 9);
    let f = |base: u32| (0..50u32).map(move |k| base + k);
    let mut p = vec![2, v];
    p.extend(f(100));
    p.push(d);
    p.extend(f(200));
    p.extend([v, v, 20, 5, 6, 7]);
    p.extend(std::iter::repeat_n(v, 70));
    p.push(e);
    let mut q = vec![3, v];
    q.extend(f(300));
    q.push(d);
    q.extend(f(400));
    q.extend([v, v, 21, 5, 6, 7, 10, 11]);
    let want = naive(&p, &q);
    assert!(
        want.contains(&pair(1, 1))
            && want.contains(&pair(103, 103))
            && want.contains(&pair(104, 104))
    );
    for t in TUNINGS {
        assert_eq!(run(t, &p, &q).0, want, "tuning {t:?}");
    }
}

#[test]
fn counts_cover_only_the_current_box() {
    // The root chooses C0 and its left box X has 67 copies of v. X chooses C1; its right box holds one copy of v,
    // the root's right box 70 more. Only the copy inside the box counts: v is rare there and must match.
    let v = 1;
    let fill = |base: u32| (0..80u32).map(move |k| base + k);
    let (c1, d2, c0) = ([10, 11], 12, [20, 21, 22, 23]);
    let mut p = vec![100];
    p.extend(std::iter::repeat_n(v, 66));
    p.extend(c1);
    p.extend([101, d2, 102]);
    let pv = p.len();
    p.extend([v, 103]);
    p.extend(fill(1000));
    p.extend(c0);
    p.extend(std::iter::repeat_n(v, 70));
    p.push(104);
    let mut q = vec![200];
    q.extend(c1);
    q.extend([201, d2, 202]);
    let qv = q.len();
    q.extend([v, 203]);
    q.extend(fill(2000));
    q.extend(c0);
    q.push(204);
    let want = naive(&p, &q);
    assert!(want.contains(&pair(pv, qv)));
    for t in TUNINGS {
        assert_eq!(run(t, &p, &q).0, want, "tuning {t:?}");
    }
}

#[test]
fn informative_diff3_example() {
    // [F12 §7.5] (Informative): b = a b c, o = a B c, t = a b c d; MA pairs lines 0 and 2, MB lines 0-2.
    let (b, o, t) = (&b"a\nb\nc\n"[..], &b"a\nB\nc\n"[..], &b"a\nb\nc\nd\n"[..]);
    assert_eq!(diff_lines(b, o).unwrap(), [pair(0, 0), pair(2, 2)]);
    assert_eq!(
        diff_lines(b, t).unwrap(),
        [pair(0, 0), pair(1, 1), pair(2, 2)]
    );
    assert_eq!(diff3(b, o, t), Some(b"a\nB\nc\nd\n".to_vec()));
    // Both sides change the same line differently: a conflicting chunk.
    assert_eq!(diff3(b"x\ny\nz\n", b"x\n1\nz\n", b"x\n2\nz\n"), None);
    // One side deletes, the other keeps: the deletion wins.
    assert_eq!(
        diff3(b"x\ny\nz\n", b"x\nz\n", b"x\ny\nz\n"),
        Some(b"x\nz\n".to_vec())
    );
}

/// The diff3 chunk walk of [F12 §7.5], written on this crate's API as M3 will: `None` when a chunk conflicts.
fn diff3(base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let mut int = Interner::new();
    let (mut o, mut a, mut b) = (Vec::new(), Vec::new(), Vec::new());
    int.intern_lines(base, &mut o).unwrap();
    int.intern_lines(ours, &mut a).unwrap();
    int.intern_lines(theirs, &mut b).unwrap();
    let text: Vec<&[u8]> = lines(base)
        .chain(lines(ours))
        .chain(lines(theirs))
        .collect();
    let empty: &[u8] = &[];
    let mut by_id = vec![empty; int.len()];
    for (line, id) in text.iter().zip(o.iter().chain(&a).chain(&b)) {
        by_id[*id as usize] = line;
    }
    let (mut ma, mut mb) = (Vec::new(), Vec::new());
    partners(&diff(&o, &a).unwrap(), o.len(), &mut ma);
    partners(&diff(&o, &b).unwrap(), o.len(), &mut mb);
    let (mut p, mut q, mut r) = (0usize, 0usize, 0usize);
    let mut res: Vec<u32> = Vec::new();
    loop {
        if p == o.len() && q == a.len() && r == b.len() {
            break;
        }
        let mut s = 0;
        while p + s < o.len() && ma[p + s] as usize == q + s && mb[p + s] as usize == r + s {
            s += 1;
        }
        if s > 0 {
            res.extend_from_slice(&o[p..p + s]);
            (p, q, r) = (p + s, q + s, r + s);
            continue;
        }
        let j = (p..o.len()).find(|&j| ma[j] != UNMATCHED && mb[j] != UNMATCHED);
        let (oe, ae, be) = j.map_or((o.len(), a.len(), b.len()), |j| {
            (j, ma[j] as usize, mb[j] as usize)
        });
        let (co, ca, cb) = (&o[p..oe], &a[q..ae], &b[r..be]);
        if ca == co {
            res.extend_from_slice(cb);
        } else if cb == co || ca == cb {
            res.extend_from_slice(ca);
        } else {
            return None;
        }
        (p, q, r) = (oe, ae, be);
        if j.is_none() {
            break;
        }
    }
    Some(
        res.iter()
            .flat_map(|&id| by_id[id as usize].iter().copied())
            .collect(),
    )
}

#[test]
fn hunks_and_partners() {
    let m = [pair(1, 0), pair(2, 2)];
    let h: Vec<_> = hunks(&m, 5, 3).collect();
    assert_eq!(h.len(), 3);
    assert_eq!((h[0].p_range(), h[0].q_range()), (0..1, 0..0));
    assert_eq!((h[1].p_range(), h[1].q_range()), (2..2, 1..2));
    assert_eq!((h[2].p_range(), h[2].q_range()), (3..5, 3..3));
    assert_eq!(hunks(&[], 0, 0).count(), 0);
    assert_eq!(hunks(&[pair(0, 0)], 1, 1).count(), 0);
    let mut out = Vec::new();
    partners(&m, 4, &mut out);
    assert_eq!(out, [UNMATCHED, 0, 2, UNMATCHED]);
    // Malformed input never panics.
    let bad = [
        pair(3, 3),
        pair(1, 1),
        Match {
            p: u32::MAX,
            q: u32::MAX,
        },
    ];
    assert!(hunks(&bad, 2, 2).count() <= 4);
    partners(&bad, 2, &mut out);
    assert_eq!(out, [UNMATCHED, 1]);
}

#[test]
fn limits() {
    assert_eq!(MAX_LEN, 4_294_967_294);
    assert!(fits(0) && fits(MAX_LEN) && !fits(MAX_LEN + 1));
    assert!(dense(4104, 2) && !dense(4105, 2) && dense(4096, 0) && !dense(4097, 0));
    assert!(dense(usize::MAX, usize::MAX));
    // The packed order is (rarity, -len, j, i), and packing round-trips.
    let key = pack(3, 17, 5, 9);
    assert_eq!(unpack(key), (5, 9, 17));
    assert!(pack(1, 1, 0, 0) < pack(2, 100, 0, 0));
    assert!(pack(1, 5, 9, 9) < pack(1, 4, 0, 0));
    assert!(pack(1, 5, 9, 1) < pack(1, 5, 1, 2));
    assert!(pack(1, 5, 1, 2) < pack(1, 5, 2, 2));
    assert_eq!(unpack(pack(64, MAX_LEN, MAX_LEN, 0)), (MAX_LEN, 0, MAX_LEN));
    assert_eq!(rarity_of(pack(64, 1, 2, 3)), 64);
    // A slot orders by (-len, j, i), then y; keyed gives its region's key with a count as the rarity.
    let s = slot(5, 9, 17, 3);
    assert_eq!(
        (unpack(keyed(7, s)), rarity_of(keyed(7, s)), line_of(s)),
        ((5, 9, 17), 7, 3)
    );
    assert_eq!(
        keyed(64, slot(MAX_LEN, 0, MAX_LEN, MAX_LEN)),
        pack(64, MAX_LEN, MAX_LEN, 0)
    );
    assert!(slot(0, 0, 5, 9) < slot(0, 0, 4, 0));
    assert!(slot(9, 1, 5, 9) < slot(1, 2, 5, 0));
    assert!(slot(1, 2, 5, 9) < slot(2, 2, 5, 0));
    assert!(slot(2, 2, 5, 0) < slot(2, 2, 5, 1));
    assert!(slot(MAX_LEN, MAX_LEN, 1, MAX_LEN) < NO_KEY);
    // A box's floor orders before every slot of the box, and keyed, before every key of that rarity there.
    let a = Area {
        p0: 3,
        p1: 10,
        q0: 0,
        q1: 4,
    };
    assert!(a.floor() < slot(0, 0, 4, 0) && a.floor() > slot(9, 9, 6, 9));
    // Its diagonals x + |Q| − y run from its pair (3, 3) to its pair (9, 0), for a Q of 8 lines.
    assert_eq!(a.diagonals(8), 8..18);
    assert!(keyed(2, a.floor()) < pack(2, 4, 0, 0) && keyed(2, a.floor()) > pack(1, 9, 9, 9));
    let whole = Area {
        p0: 0,
        p1: MAX_LEN,
        q0: 0,
        q1: MAX_LEN,
    };
    assert_eq!(unpack(keyed(1, whole.floor())), (0, 0, MAX_LEN + 1));
    // Group entries order by key, then id, and give their key back.
    let e = Entry::new(pack(7, 6, 4, 5), 9);
    assert_eq!((e.key(), e.v), (pack(7, 6, 4, 5), 9));
    assert!(Entry::new(pack(6, 1, 0, 0), 0) < e && e < Entry::new(pack(7, 6, 4, 5), 10));
    assert!(e < Entry::new(pack(7, 6, 5, 5), 0) && Entry::new(pack(7, 7, 9, 9), 99) < e);
    assert!(LengthError.to_string().contains("u32"));
}

#[test]
fn slot_heaps() {
    // heapify and sift_down keep the least at the top, through every removal.
    for seed in 1..30u64 {
        let mut h: Vec<u128> = seq(seed, (seed * 7 % 40) as usize, 50)
            .into_iter()
            .map(u128::from)
            .collect();
        let mut want = h.clone();
        want.sort_unstable();
        heapify(&mut h);
        let mut got = Vec::new();
        while let Some(&top) = h.first() {
            got.push(top);
            let n = h.len();
            h[0] = h[n - 1];
            h.truncate(n - 1);
            sift_down(&mut h, 0);
        }
        assert_eq!(got, want, "seed {seed}");
    }
    sift_down(&mut [], 0);
    let mut one = [5u128];
    sift_down(&mut one, 0);
    heapify(&mut one);
    assert_eq!(one, [5]);
}

#[test]
fn dense_ids_index_directly_and_sparse_ones_are_renumbered() {
    // Four lines in all: ids up to 4 × 4 + 4096 index the tables directly.
    let mut d = Differ::default();
    let mut out = Vec::new();
    // Two single-line regions of rarity 1: the least j wins.
    d.diff(&[4111, 1], &[1, 4111], &mut out).unwrap();
    assert!(d.ids.is_empty());
    assert_eq!(out, [pair(1, 0)]);
    d.diff(&[4112, 1], &[1, 4112], &mut out).unwrap();
    assert_eq!(d.ids, [1, 4112]);
    assert_eq!(d.p_ids, [1, 0]);
    assert_eq!(d.q_ids, [0, 1]);
    assert_eq!(out, [pair(1, 0)]);
    d.diff(&[4112, 1], &[7, 4112], &mut out).unwrap();
    assert_eq!(d.q_ids, [2, 1]);
    assert_eq!(out, [pair(0, 1)]);
    // Five lines in all (three and two): ids up to 4 × 5 + 4096 = 4116 index directly.
    for (top, sparse) in [(4115, false), (4116, true)] {
        let mut d = Differ::new();
        d.diff(&[top, 1, 2], &[1, 2], &mut out).unwrap();
        assert_eq!(!d.ids.is_empty(), sparse, "top id {top}");
        assert_eq!(out, [pair(1, 0), pair(2, 1)]);
    }
}

#[test]
fn scratch_bytes_counts_every_buffer() {
    // Sparse ids fill the renumbering buffers; repeated lines give steps, waiting boxes, groups and their builds.
    let base = seq(4, 500, 40);
    let q = edited(
        &base,
        &[(7, 2, 41), (100, 1, 42), (260, 0, 0), (400, 2, 43)],
    );
    let spread = |v: &[u32]| v.iter().map(|&x| x * 1_000_000).collect::<Vec<_>>();
    let mut d = Differ::new();
    let mut out = Vec::new();
    d.diff(&spread(&base), &spread(&q), &mut out).unwrap();
    assert_eq!(out, plain(&base, &q));
    let e = &d.engine;
    let words = [
        e.p_pos.capacity(),
        e.q_pos.capacity(),
        e.part.capacity(),
        e.affected.capacity(),
        e.built.capacity(),
        d.ids.capacity(),
        d.p_ids.capacity(),
        d.q_ids.capacity(),
    ];
    let others = [
        e.rows.capacity(),
        e.runs.capacity(),
        e.slots.capacity(),
        e.pending.capacity(),
        e.heap.capacity(),
        e.groups.capacity(),
    ];
    assert!(
        words.iter().chain(&others).all(|&c| c > 1),
        "{words:?} {others:?}"
    );
    let want = 4 * words.iter().sum::<usize>()
        + 16 * others[0]
        + 8 * others[1]
        + 16 * others[2]
        + 16 * others[3]
        + 16 * others[4]
        + 24 * others[5]
        + 8 * e.lines.capacity();
    assert!(e.lines.capacity() > 1);
    assert_eq!(d.scratch_bytes(), want);
}

#[test]
fn sparse_ids_are_renumbered() {
    let p = seq(1, 300, 20);
    let q = edited(&p, &[(5, 0, 3), (40, 1, 7), (90, 2, 19), (150, 1, 25)]);
    // Multiplication by an odd constant is a bijection of u32: equal ids stay equal.
    let spread = |v: &[u32]| {
        v.iter()
            .map(|&x| x.wrapping_mul(2_654_435_761))
            .collect::<Vec<_>>()
    };
    let mut d = Differ::new();
    let (mut dense, mut sparse) = (Vec::new(), Vec::new());
    d.diff(&p, &q, &mut dense).unwrap();
    d.diff(&spread(&p), &spread(&q), &mut sparse).unwrap();
    assert_eq!(dense, sparse);
    assert_eq!(dense, naive(&p, &q));
    // A Q id absent from sparse P stays unmatched.
    let got = diff(&[u32::MAX, 4_000_000_000], &[4_000_000_000, 17, u32::MAX]).unwrap();
    assert_eq!(got, [pair(1, 0)]);
    assert!(d.scratch_bytes() > 0);
    d.release();
    assert_eq!(d.scratch_bytes(), 0);
}

/// HD by the recursion of [F12 §7.5] with one plain scan per box: the rare pairs of the box, each measured to its
/// maximal region. Quadratic on long chains of splits, but fast enough past the naive reference's reach.
fn plain(p: &[u32], q: &[u32]) -> Vec<Match> {
    let mut out = Vec::new();
    let mut boxes = vec![[0, p.len(), 0, q.len()]];
    while let Some([mut p0, mut p1, mut q0, mut q1]) = boxes.pop() {
        while p0 < p1 && q0 < q1 && p[p0] == q[q0] {
            out.push(pair(p0, q0));
            p0 += 1;
            q0 += 1;
        }
        while p0 < p1 && q0 < q1 && p[p1 - 1] == q[q1 - 1] {
            out.push(pair(p1 - 1, q1 - 1));
            p1 -= 1;
            q1 -= 1;
        }
        if p0 == p1 || q0 == q1 {
            continue;
        }
        let mut at: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, &v) in p.iter().enumerate().take(p1).skip(p0) {
            at.entry(v).or_default().push(i);
        }
        let mut best: Option<(usize, Reverse<usize>, usize, usize)> = None;
        for j in q0..q1 {
            let Some(occ) = at.get(&q[j]) else { continue };
            if occ.len() > 64 {
                continue;
            }
            for &i in occ {
                if i > p0 && j > q0 && p[i - 1] == q[j - 1] {
                    continue;
                }
                let mut len = 0;
                while i + len < p1 && j + len < q1 && p[i + len] == q[j + len] {
                    len += 1;
                }
                let rarity = (i..i + len)
                    .map(|x| at[&p[x]].len())
                    .min()
                    .unwrap_or(usize::MAX);
                let c = (rarity, Reverse(len), j, i);
                if best.is_none_or(|b| c < b) {
                    best = Some(c);
                }
            }
        }
        if let Some((_, Reverse(len), j, i)) = best {
            out.extend((0..len).map(|k| pair(i + k, j + k)));
            boxes.push([p0, i, q0, j]);
            boxes.push([i + len, p1, j + len, q1]);
        }
    }
    out.sort_unstable();
    out
}

#[test]
fn plain_reference_equals_the_naive_one() {
    for seed in 1..40u64 {
        let base = seq(seed, 60, 1 + (seed % 9) as u32);
        let edits: Vec<(usize, u8, u32)> = seq(seed + 7, 30, 60)
            .chunks(3)
            .map(|c| (c[0] as usize, c[1] as u8, c[2] % 12))
            .collect();
        let q = edited(&base, &edits);
        assert_eq!(plain(&base, &q), naive(&base, &q), "seed {seed}");
    }
}

#[test]
fn every_tuning_agrees_on_mid_sizes() {
    // Beyond the naive reference's reach: small alphabets make counts cross 64 inside the boxes.
    for (seed, alphabet) in [(1u64, 400u32), (2, 150), (3, 60), (4, 25), (5, 1000)] {
        let base = seq(seed, 3000, alphabet);
        let edits: Vec<(usize, u8, u32)> = seq(seed + 100, 900, 3000)
            .chunks(3)
            .map(|c| (c[0] as usize, c[1] as u8, c[2] % (alphabet + 50)))
            .collect();
        let q = edited(&base, &edits);
        let want = plain(&base, &q);
        let back = plain(&q, &base);
        for t in TUNINGS {
            assert_eq!(run(t, &base, &q).0, want, "seed {seed}, tuning {t:?}");
            assert_eq!(
                run(t, &q, &base).0,
                back,
                "seed {seed}, tuning {t:?}, swapped"
            );
        }
    }
}

#[test]
fn small_caps_trim_and_rescan() {
    let base = seq(9, 400, 30);
    let q = edited(
        &base,
        &[
            (3, 0, 1),
            (50, 1, 31),
            (99, 2, 32),
            (200, 1, 7),
            (320, 0, 0),
        ],
    );
    let want = plain(&base, &q);
    let tiny = Tuning {
        min_cap: 1,
        cap_div: usize::MAX,
        group_cap: 1,
        inherit: Inherit::Larger,
    };
    let (got, st) = run(tiny, &base, &q);
    assert_eq!(got, want);
    assert!(st.trims > 0 && st.compactions > 0, "{st:?}");
    let (got, st) = run(Tuning::DEFAULT, &base, &q);
    assert_eq!(got, want);
    assert_eq!(
        (st.scans, st.trims, st.compactions),
        (st.chains, 0, 0),
        "{st:?}"
    );
    // Unique lines with every other one changed: a cap of one keeps one entry, and the chains must trim and scan
    // their boxes again.
    let (p, q) = interleaved(300, 1, 2, 9);
    let want = plain(&p, &q);
    let (got, st) = run(tiny, &p, &q);
    assert_eq!(got, want);
    assert!(st.trims > 0 && st.scans > st.chains, "{st:?}");
    // The default cap keeps 4,096 entries and trims only above 8,192: about 6,000 regions of two unrelated
    // sequences fit without a trim.
    let (a, b) = (seq(31, 500, 40), seq(32, 500, 40));
    let (got, st) = run(Tuning::DEFAULT, &a, &b);
    assert_eq!(got, plain(&a, &b));
    assert!(st.regions > 4200, "{st:?}");
    assert_eq!((st.trims, st.scans), (0, st.chains), "{st:?}");
}

#[test]
fn a_count_falling_to_64_builds_the_group_once() {
    // v has 65 copies in P, so it is no candidate at the root. The root matches B, and the right box continues the
    // chain without the first copy: v falls to 64 and gets a group entry; its slots (300 lines of Q, 64 pairs each)
    // are built once, and each of the 64 steps after that evaluates few of them again.
    let (v, b) = (1u32, 2u32);
    let mut p = vec![10, v, b];
    for k in 0..64 {
        p.extend([v, 100 + k]);
    }
    p.push(11);
    let mut q = vec![20, b];
    for k in 0..300 {
        q.extend([v, 1000 + k]);
    }
    q.push(21);
    let want = plain(&p, &q);
    assert!(want.len() > 2, "{want:?}");
    let (got, st) = run(Tuning::DEFAULT, &p, &q);
    assert_eq!(got, want);
    assert_eq!(st.builds, 1, "{st:?}");
    assert!(st.lookups <= 64 * 300 + 64 * st.evals, "{st:?}");
    assert!(st.evals <= 4 * 64, "{st:?}");
    for t in TUNINGS {
        assert_eq!(run(t, &p, &q).0, want, "tuning {t:?}");
    }
}

/// A differ over `p` against itself with the scratch of a run in place and every count of P set, as `chain` does.
fn engine_on(p: &[u32]) -> Differ {
    let mut d = Differ::new();
    let e = &mut d.engine;
    let bound = p.iter().max().map_or(0, |&m| m as usize + 1);
    e.index(p, p, bound);
    e.runs.resize(2 * p.len(), Run::default());
    for &x in p {
        e.rows[x as usize].cnt += 1;
    }
    d
}

#[test]
fn a_build_measures_each_diagonal_run_once() {
    // v a v a v against itself: v's 9 pairs lie on 5 diagonal runs (lengths 5, 3, 3, 1, 1), each measured once, and
    // each line of Q holding v has the whole diagonal as its slot.
    let (v, a) = (0u32, 1u32);
    let p = [v, a, v, a, v];
    let mut d = engine_on(&p);
    let e = &mut d.engine;
    let all = Area {
        p0: 0,
        p1: 5,
        q0: 0,
        q1: 5,
    };
    e.affected = vec![v];
    e.build(&p, &p, all);
    assert_eq!((e.stats.lookups, e.stats.runs, e.stats.walked), (9, 5, 13));
    assert_eq!(e.rows[v as usize].live, 3);
    assert_eq!(e.built, [v]);
    let mut slots: Vec<u128> = e.slots[..3].to_vec();
    slots.sort_unstable();
    assert_eq!(slots, [0, 2, 4].map(|y| slot(0, 0, 5, y)));
    // A box that cuts the runs: the cached runs clip to it, and nothing is measured again.
    let inner = Area {
        p0: 1,
        p1: 5,
        q0: 0,
        q1: 3,
    };
    // A built group is not built again.
    e.build(&p, &p, inner);
    assert_eq!((e.stats.builds, e.stats.lookups), (1, 9));
    e.rows[v as usize].live = UNBUILT;
    e.build(&p, &p, inner);
    assert_eq!((e.stats.lookups, e.stats.runs), (13, 5));
    let mut slots: Vec<u128> = e.slots[..2].to_vec();
    slots.sort_unstable();
    assert_eq!(slots, [slot(2, 0, 3, 0), slot(2, 0, 3, 2)]);
    // A fresh engine measuring in the inner box gives the same slots, from its 3 runs.
    let mut fresh = engine_on(&p);
    let f = &mut fresh.engine;
    f.affected = vec![v];
    f.build(&p, &p, inner);
    let mut again: Vec<u128> = f.slots[..2].to_vec();
    again.sort_unstable();
    assert_eq!(again, slots);
    assert_eq!(f.stats.runs, 3);
}

#[test]
fn slots_leave_the_heap_with_their_lines() {
    // v a v a … against itself (20 copies of v): every line of Q holding v has the whole diagonal as its slot, so
    // the heap orders them by line. In a box that drops the first 7 of them, those leave the heap from the top one
    // by one, and exactly the other 13 stay.
    let (v, a) = (0u32, 1u32);
    let p: Vec<u32> = (0..40).map(|k| if k % 2 == 0 { v } else { a }).collect();
    let mut d = engine_on(&p);
    let e = &mut d.engine;
    let all = Area {
        p0: 0,
        p1: 40,
        q0: 0,
        q1: 40,
    };
    e.affected = vec![v];
    e.build(&p, &p, all);
    assert_eq!(e.rows[v as usize].live, 20);
    let inner = Area {
        p0: 0,
        p1: 40,
        q0: 14,
        q1: 40,
    };
    let top = e.least(&p, &p, inner, v as usize);
    // Through line 14, the longest regions run to the end of Q (26 lines); the least i is P's first line. Every
    // slot left was the whole diagonal, below its true value: each is evaluated once, and the top once more.
    assert_eq!(top, Some(slot(0, 14, 26, 14)));
    assert_eq!(e.stats.evals, 14);
    let live = e.rows[v as usize].live as usize;
    let s = e.rows[v as usize].q as usize;
    let mut lines: Vec<usize> = e.slots[s..s + live].iter().map(|&x| line_of(x)).collect();
    lines.sort_unstable();
    assert_eq!(lines, (14..40).step_by(2).collect::<Vec<_>>());
}

#[test]
fn slots_are_evaluated_again_as_the_box_shrinks() {
    // v a v a v against itself, built in the whole box. In a box that cuts the runs, v's least slot sinks to its
    // true value and the lines of Q outside the box leave the heap.
    let (v, a) = (0u32, 1u32);
    let p = [v, a, v, a, v];
    let mut d = engine_on(&p);
    let e = &mut d.engine;
    let all = Area {
        p0: 0,
        p1: 5,
        q0: 0,
        q1: 5,
    };
    e.affected = vec![v];
    e.build(&p, &p, all);
    assert_eq!(e.least(&p, &p, all, v as usize), Some(slot(0, 0, 5, 0)));
    assert_eq!(e.stats.evals, 1);
    // Only y = 2 lies in P[1..5) × Q[1..3): its pairs (2, 2) and (4, 2) give the clipped runs (1, 1, 2) and
    // (2, 0, 3) → (3, 1, 2); the least is (1, 1, 2).
    let inner = Area {
        p0: 1,
        p1: 5,
        q0: 1,
        q1: 3,
    };
    let got = e.least(&p, &p, inner, v as usize);
    assert_eq!(got, Some(slot(1, 1, 2, 2)));
    assert_eq!(e.rows[v as usize].live, 1);
    // No line of v in Q's part of the box: the heap empties.
    let none = Area {
        p0: 1,
        p1: 5,
        q0: 3,
        q1: 4,
    };
    assert_eq!(e.least(&p, &p, none, v as usize), None);
    assert_eq!(e.rows[v as usize].live, 0);
}

fn is_valid(p: &[u32], q: &[u32], m: &[Match]) -> bool {
    m.windows(2).all(|w| w[0].p < w[1].p && w[0].q < w[1].q)
        && m.iter().all(|x| p[x.p as usize] == q[x.q as usize])
}

/// All the work of a run, in line, region and pair visits.
fn work(s: &Stats) -> usize {
    s.scanned + s.regions + s.removed + s.pops + s.evals + s.lookups + s.walked
}

/// Every `every`-th line of P (from line 1) changed in Q; the lines of P have about `copies` copies each.
fn interleaved(n: usize, copies: u32, every: usize, seed: u64) -> (Vec<u32>, Vec<u32>) {
    let alphabet = (n as u32 / copies).max(1);
    let p = seq(seed, n, alphabet);
    let q = p
        .iter()
        .enumerate()
        .map(|(k, &v)| {
            if k % every == 1 {
                10_000_000 + k as u32
            } else {
                v
            }
        })
        .collect();
    (p, q)
}

fn log2(n: usize) -> usize {
    n.ilog2() as usize + 1
}

/// Every other (or third) line changed: splits peel one region at a time, and rescanning the rest at each split is
/// O(n²).
#[test]
fn interleaved_changes_stay_near_n_log_n() {
    let n = 20_000usize;
    for (copies, every, per) in [(1, 2, 2), (2, 2, 2), (5, 2, 2), (20, 3, 4), (60, 3, 8)] {
        let (p, q) = interleaved(n, copies, every, u64::from(copies) * 7 + every as u64);
        for (a, b) in [(&p, &q), (&q, &p)] {
            let (m, st) = run(Tuning::DEFAULT, a, b);
            assert!(is_valid(a, b, &m));
            // From P to Q the changed lines are new; backwards, a line of P changed in Q may still occur elsewhere
            // in P, and HD can pair it far off the diagonal.
            assert!(a != &p || m.len() >= n / every / 2, "{}", m.len());
            assert!(
                work(&st) <= per * n * log2(n),
                "copies {copies}, every {every}: {st:?}"
            );
        }
    }
    // Unique lines with a shared blank line between them: its count stays above 64 until the last splits.
    let (p, q) = interleaved(n / 2, 1, 2, 5);
    let blank = 20_000_001;
    let p2: Vec<u32> = p.iter().flat_map(|&x| [x, blank]).collect();
    let q2: Vec<u32> = q.iter().flat_map(|&x| [x, blank]).collect();
    let (m2, st2) = run(Tuning::DEFAULT, &p2, &q2);
    assert!(is_valid(&p2, &q2, &m2));
    assert!(m2.len() >= n / 2, "{}", m2.len());
    assert!(work(&st2) <= 2 * n * log2(n), "{st2:?}");
    // Continuing on the smaller child instead leaves the large one to a fresh scan at every split: quadratic.
    let small = 2_000usize;
    let (p, q) = interleaved(small, 2, 2, 11);
    let (m, slow) = run(
        Tuning {
            inherit: Inherit::Smaller,
            ..Tuning::DEFAULT
        },
        &p,
        &q,
    );
    let (fast_m, fast) = run(Tuning::DEFAULT, &p, &q);
    assert!(slow.scanned >= small * small / 8, "{slow:?}");
    assert_eq!(fast_m, m);
    assert!(fast.scanned * 20 <= slow.scanned, "{fast:?} {slow:?}");
}

/// Scratch bytes of a fresh differ after one diff: at most 40 per line of input, plus 1 MiB.
fn bounded_scratch(d: &Differ, lines: usize) -> bool {
    d.scratch_bytes() <= 40 * lines + (1 << 20)
}

#[test]
fn large_inputs_bounded_work_and_memory() {
    // 200,000 lines with a few scattered edits: prefix, suffix and small middle boxes.
    let n = 200_000usize;
    let p: Vec<u32> = (0..n as u32).collect();
    let edits: Vec<(usize, u8, u32)> = (0..40)
        .map(|k| (k * 4999 + 17, (k % 3) as u8, n as u32 + k as u32))
        .collect();
    let q = edited(&p, &edits);
    let mut d = Differ::new();
    let mut m = Vec::new();
    d.diff(&p, &q, &mut m).unwrap();
    assert!(is_valid(&p, &q, &m));
    assert!(m.len() >= n - 40);
    assert!(bounded_scratch(&d, 2 * n), "{}", d.scratch_bytes());
    let st = d.engine.stats;
    assert!(work(&st) <= 3 * (p.len() + q.len()), "{st:?}");
    // Many repeated lines (counts far above 64): no candidate after the prefix and suffix, one scan.
    let p = seq(7, 100_000, 100);
    let q = seq(8, 100_000, 100);
    let (m, st) = run(Tuning::DEFAULT, &p, &q);
    assert!(is_valid(&p, &q, &m));
    assert_eq!(st.scans, 1);
    assert!(work(&st) <= 2 * (p.len() + q.len()), "{st:?}");
    // Rare lines everywhere under a heavy edit load, and a long queue of waiting boxes.
    let mut m = Vec::new();
    for (copies, every, seed) in [(3, 3, 13), (1, 2, 17)] {
        let (p, q) = interleaved(100_000, copies, every, seed);
        let mut d = Differ::new();
        d.diff(&p, &q, &mut m).unwrap();
        assert!(is_valid(&p, &q, &m));
        assert!(bounded_scratch(&d, 200_000), "{}", d.scratch_bytes());
    }
}

/// A cycle of `ids` ids repeated `copies` times, and its reversal: every count is `copies`, and each step of HD lowers
/// almost every count by one.
fn cycle_and_reversal(ids: u32, copies: usize) -> (Vec<u32>, Vec<u32>) {
    let p: Vec<u32> = (0..copies).flat_map(|_| 0..ids).collect();
    let mut q = p.clone();
    q.reverse();
    (p, q)
}

#[test]
fn every_count_at_the_limit() {
    let (p, q) = cycle_and_reversal(20, 64);
    let want = plain(&p, &q);
    for t in TUNINGS {
        assert_eq!(run(t, &p, &q).0, want, "tuning {t:?}");
    }
    // The review's worst case for the id entries of the first engine: 341 ids with 64 copies each, 21,824 lines, took
    // about 4,096 · n region measurements. Each id's slots are now built once, and the steps evaluate few slots.
    let (p, q) = cycle_and_reversal(341, 64);
    let (m, st) = run(Tuning::DEFAULT, &p, &q);
    assert!(is_valid(&p, &q, &m));
    assert_eq!(st.builds, 341, "{st:?}");
    assert!(st.lookups <= 64 * q.len() + 64 * st.evals, "{st:?}");
    assert!(st.evals <= 8 * 64, "{st:?}");
    assert!(work(&st) <= 4 * 64 * q.len(), "{st:?}");
}

/// P: 64 copies of `unit`, each followed by a unique line; Q: `unit` repeated `reps` times.
fn rare_units(unit: &[u32], reps: usize) -> (Vec<u32>, Vec<u32>) {
    let mut p = Vec::new();
    for k in 0..64 {
        p.extend_from_slice(unit);
        p.push(100 + k);
    }
    let q = unit
        .iter()
        .copied()
        .cycle()
        .take(unit.len() * reps)
        .collect();
    (p, q)
}

#[test]
fn a_rare_line_facing_many_copies() {
    // Every step lowers the count of v (and w), whose slots cover all of Q: they are built once.
    for unit in [&[1u32][..], &[1, 2]] {
        let (p, q) = rare_units(unit, 300);
        let want = plain(&p, &q);
        for t in TUNINGS {
            assert_eq!(run(t, &p, &q).0, want, "tuning {t:?}");
        }
        let (p, q) = rare_units(unit, 32_768 / unit.len());
        let (m, st) = run(Tuning::DEFAULT, &p, &q);
        assert!(is_valid(&p, &q, &m));
        assert_eq!(m.len(), 64 * unit.len());
        assert_eq!(st.builds, unit.len(), "{st:?}");
        assert!(st.lookups <= 64 * q.len() + 64 * st.evals, "{st:?}");
        assert!(st.evals <= 8 * 64, "{st:?}");
        // One run per pair of v: w's pairs lie on v's runs.
        assert!(
            st.runs <= 64 * q.len() / unit.len() + 64 * st.evals,
            "{st:?}"
        );
    }
}

/// `copies` copies of a block of `len` distinct ids, each followed by a separator id `sep + copy`.
fn blocks(len: u32, copies: u32, sep: u32) -> Vec<u32> {
    (0..copies)
        .flat_map(|c| (0..len).chain([sep + c]))
        .collect()
}

#[test]
fn repeated_blocks_share_their_runs() {
    // Every step matches one copy of the block and lowers the counts of all its ids, whose regions are the same runs:
    // the ids affected together are built in one pass, which measures each run once.
    let (p, q) = (blocks(10, 16, 1000), blocks(10, 16, 2000));
    let want = plain(&p, &q);
    for t in TUNINGS {
        assert_eq!(run(t, &p, &q).0, want, "tuning {t:?}");
    }
    let (p, q) = (blocks(100, 64, 1000), blocks(100, 64, 2000));
    let (m, st) = run(Tuning::DEFAULT, &p, &q);
    assert!(is_valid(&p, &q, &m));
    assert_eq!(m.len(), 64 * 100);
    // 100 ids with 63 × 63 pairs each, on 63 × 63 runs of 100 lines.
    assert_eq!(st.builds, 100, "{st:?}");
    assert!(st.runs <= 63 * 63 + 64 * st.evals, "{st:?}");
    assert!(st.walked <= 100 * (63 * 63 + 64 * st.evals), "{st:?}");
}

#[test]
fn exact_work_on_a_fixed_input() {
    // Speed-only choices (when to trim or compact, how much to keep, which child continues) leave the pairs unchanged
    // but not the work: the counters of a fixed input pin them.
    let base = seq(9, 400, 30);
    let q = edited(
        &base,
        &[
            (3, 0, 1),
            (50, 1, 31),
            (99, 2, 32),
            (200, 1, 7),
            (320, 0, 0),
        ],
    );
    let small = Tuning {
        min_cap: 3,
        cap_div: usize::MAX,
        group_cap: 3,
        inherit: Inherit::Larger,
    };
    let common = Stats {
        chains: 2,
        scans: 2,
        scanned: 636,
        regions: 3148,
        removed: 269,
        pops: 23,
        builds: 14,
        evals: 4,
        lookups: 25,
        runs: 8,
        walked: 247,
        ..Stats::default()
    };
    let (m, st) = run(Tuning::DEFAULT, &base, &q);
    assert_eq!(m, plain(&base, &q));
    assert_eq!(st, common);
    // Caps of 3: five trims and six compactions, with fewer pops for the entries they let go.
    let (m, st) = run(small, &base, &q);
    assert_eq!(m, plain(&base, &q));
    assert_eq!(
        st,
        Stats {
            pops: 20,
            trims: 5,
            compactions: 6,
            ..common
        }
    );
    // Unique lines with every other one changed: rescans after the trims.
    let (p2, q2) = interleaved(300, 1, 2, 9);
    let (m, st) = run(small, &p2, &q2);
    assert_eq!(m, plain(&p2, &q2));
    assert_eq!(
        st,
        Stats {
            chains: 150,
            scans: 180,
            scanned: 8344,
            regions: 3330,
            removed: 420,
            pops: 250,
            trims: 47,
            compactions: 4,
            builds: 42,
            evals: 0,
            lookups: 42,
            runs: 41,
            walked: 41,
        }
    );
}

fn small_seq(max: usize) -> impl Strategy<Value = Vec<u32>> {
    prop_oneof![
        prop::collection::vec(0u32..3, 0..max),
        prop::collection::vec(0u32..8, 0..max),
        prop::collection::vec(0u32..40, 0..max),
    ]
}

fn edit_list() -> impl Strategy<Value = Vec<(usize, u8, u32)>> {
    prop::collection::vec((0usize..200, 0u8..3, 0u32..60), 0..25)
}

proptest! {
    #![proptest_config(crate::test_config(256))]

    #[test]
    fn random_pairs_equal_the_definition(p in small_seq(50), q in small_seq(50)) {
        check_all_tunings(&p, &q)?;
    }

    #[test]
    fn edited_copies_equal_the_definition(base in small_seq(120), edits in edit_list()) {
        let q = edited(&base, &edits);
        check_all_tunings(&base, &q)?;
        check_all_tunings(&q, &base)?;
    }

    #[test]
    fn counts_around_the_limit(zeros in 58usize..72, others in prop::collection::vec(0usize..200, 20..45), edits in edit_list()) {
        // One id with about 64 copies spread among other lines: its rarity crosses the limit inside sub-boxes.
        let mut p = vec![0u32; zeros];
        for (k, at) in others.into_iter().enumerate() {
            p.insert(at % (p.len() + 1), 10 + (k % 23) as u32);
        }
        let q = edited(&p, &edits);
        check_all_tunings(&p, &q)?;
        check_all_tunings(&q, &p)?;
    }

    #[test]
    fn hunks_partition_both_sides(base in small_seq(80), edits in edit_list()) {
        let q = edited(&base, &edits);
        let m = diff(&base, &q).unwrap();
        prop_assert!(is_valid(&base, &q, &m));
        let (mut seen_p, mut seen_q) = (vec![0u8; base.len()], vec![0u8; q.len()]);
        for x in &m {
            seen_p[x.p as usize] += 1;
            seen_q[x.q as usize] += 1;
        }
        let mut last_end = (0usize, 0usize);
        for h in hunks(&m, base.len(), q.len()) {
            prop_assert!(!h.p_range().is_empty() || !h.q_range().is_empty());
            prop_assert!(h.p_start as usize >= last_end.0 && h.q_start as usize >= last_end.1);
            last_end = (h.p_end as usize, h.q_end as usize);
            for i in h.p_range() { seen_p[i] += 1; }
            for j in h.q_range() { seen_q[j] += 1; }
        }
        prop_assert!(seen_p.iter().all(|&c| c == 1));
        prop_assert!(seen_q.iter().all(|&c| c == 1));
    }
}
