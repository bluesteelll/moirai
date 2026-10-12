//! RS-007, the model's hierarchy rule ([RULES/merge-table] RS-007; [AR §11] OQ-A-13: K2 with the exactness fixes F1 to
//! F5, 13.2 (a), 13.3 (b), 13.4 (a) and 13.5 (b)): a three-way merge of the hierarchy keys by state, with a cycle
//! repair, read from the three states, the origins of the sides' values and the effective strict.
//!
//! - **Scope.** Computed once per merge, `sync`, virtual merge (VM-7), revert and cherry-pick, after the re-key
//!   (PR-007) and over the re-keyed states, for every merged node: a node live in the result with no existence policy.
//!   Its inputs are b, o and t (for a revert or a cherry-pick, those of its DM row), the effective strict and the
//!   origins.
//! - **Values (F3, 13.2 (a)).** Values compare canonically. On a side where the node is not live the key holds
//!   `absent`, which equals no live value: a live root with no order is not `absent`. A key is touched on side S when
//!   S's value differs from b's.
//! - **Three-way.** o's value when o = t (`untouched` when it is also b's, else `both-same`); t's when o = b
//!   (`only-t`); o's when t = b (`only-o`). Otherwise the key is two-sided (`both-diff`), and every other key is
//!   determined.
//! - **13.5 (b).** When b, o and t are live and exactly one side's value is order-only by state (its parent is b's, its
//!   order is not), a two-sided key takes the other side's value, whatever the origins; a revert and a cherry-pick
//!   included (R-15 Q3).
//! - **MR-040.** Otherwise the side whose value has the later origin wins, t's on equal origins.
//! - **Origin (13.4 (a)).** The origin of a value is the (hlc, commit id) of the commit that produced it
//!   ([`crate::dag::Dag::origin`]): the walk from a side's tip passes to the first parent over a commit whose net
//!   changeset leaves the key's (live, flat) value unchanged, and to the second parent at a two-parent commit that
//!   changed the key and holds its second parent's value; any other commit that changed the key produced it; ε gives
//!   (0, 0). The commit a `merge --continue` lands is walked by the same rules, whatever its resolutions' choices
//!   (13.4 (a) read by state, R-20). For a revert or a cherry-pick of C, src's origin is C's. In a virtual merge dst's
//!   is the latest among the folded LCAs that hold the value, (0, 0) when none does. A both-same key's origin is the
//!   later of its two sides' (F2). A merge with no history ([`super::Ctx::origin`] is `None`) gives every value
//!   (0, 0).
//! - **Cycle repair, participants.** The result must be a forest. A node an existence policy fixed, and a tombstone,
//!   keeps its copied value and is never moved; a parent that is no result node ends a chain. Only a key whose value
//!   and fallback differ in parent takes part: order alone closes no cycle. The fallback is the other side's value for
//!   a two-sided key and b's for a determined key.
//! - **Step (1) (F1, F4).** While no choice of the two-sided keys gives a forest, let U be the nodes from which every
//!   choice leads into a cycle. Among the determined keys of U that lie on a cycle of the option graph restricted to U
//!   (F1), one is reset to b, by these tie-breaks in order: for a revert or a cherry-pick, a key only C changed; a
//!   one-sided key before a both-same one; a key whose b value's parent is a root or outside U; the latest origin,
//!   src's on equal origins; the least uid. That key is MR-039's (CS-013), silently inside a virtual merge. A cycle
//!   with no such key (only fixed nodes, untouched keys, keys whose value and fallback have the same parent, two-sided
//!   keys and keys already reset) is left as it is, for VA-005 to stage (F4).
//! - **Step (2).** Then the two-sided keys are taken in ascending origin of their three-way value, least uid on a tie.
//!   Each keeps that value unless it leaves a node unable to reach a root that could before; otherwise it takes the
//!   other side's value. Outside strict, a key whose three-way value has a parent not live in the result, while the
//!   other side's value has a live parent or none, tries the other side's value first.
//! - **13.3 (b).** A two-sided key that ends at the other side's value this way lands there, with one `MoveSkipped`
//!   hint (HT-004) naming the key, the side whose value landed and the reason (`cycle` or `dead-parent`); none inside a
//!   virtual merge (VB-019). Under strict (`--strict` or `merge.strict`; never a revert, a cherry-pick or a virtual
//!   merge, R-15 Q2) a `cycle` key is MR-039's instead, the candidate holding the other side's value, and the
//!   dead-parent preference is off, so the three-way value stays and VA-004 stages its `DanglingEdge`.
//! - **Guarantees (F5, 13.2 (a)).** Every key lands at its three-way value or, for a two-sided key, at the other side's
//!   value. A merge stages a `HierarchyCycle` on a key untouched on one side only when no forest keeps every such key
//!   at the other side's value (I25′). A merge whose base's commit is tip(dst) lands t's hierarchy, and a revert or a
//!   cherry-pick of a commit with no hierarchy entry lands o's. A key equal in b, o and t keeps its value and is never
//!   reset ([F12 §7.2]).

use super::{HInput, HOutput, MoveKey, Op, parent_of};
use crate::state::KVal;
use crate::value::Nid;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// A merged key's class under the three-way rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Class {
    /// o = t = b.
    Untouched,
    /// o = b ≠ t.
    OnlyT,
    /// t = b ≠ o.
    OnlyO,
    /// o = t ≠ b.
    BothSame,
    /// o ≠ t, both ≠ b.
    BothDiff,
}

/// One merged key: its class, its three-way value (the MR-040 winner for `both-diff`) and its fallback (the other
/// side's value for `both-diff`, b's otherwise).
struct K {
    class: Class,
    chosen: Option<KVal>,
    fallback: Option<KVal>,
    /// The side whose value `chosen` is (1 o, 2 t; 0 b's, for `untouched`).
    side: usize,
}

/// `threeway`'s hierarchy decision for one merge.
pub(crate) fn hierarchy(x: &HInput<'_>) -> HOutput {
    threeway(x)
}

/// The origins of the sides' values, memoised per merge: the caller's [`super::Ctx::origin`], or (0, 0) for a merge
/// with no history.
struct Origins<'x, 'a> {
    x: &'x HInput<'a>,
    memo: RefCell<BTreeMap<(usize, Nid), MoveKey>>,
}

impl Origins<'_, '_> {
    fn side_value(&self, side: usize, n: Nid) -> Option<KVal> {
        match side {
            1 => self.x.o[&n].clone(),
            _ => self.x.t[&n].clone(),
        }
    }

    /// The origin time of side `side`'s (1 o, 2 t) value of `n`'s key.
    fn of(&self, side: usize, n: Nid) -> MoveKey {
        if let Some(k) = self.memo.borrow().get(&(side, n)) {
            return *k;
        }
        let v = self.side_value(side, n);
        let k = match self.x.origin {
            Some(f) => f(side, n, !self.x.dead[side].contains(&n), &v),
            None => (0, [0; 32]),
        };
        self.memo.borrow_mut().insert((side, n), k);
        k
    }

    /// The origin of a key's three-way value: its side's, the later of both for `both-same`, (0, 0) for `untouched`.
    fn chosen(&self, n: Nid, k: &K) -> MoveKey {
        match k.class {
            Class::Untouched => (0, [0; 32]),
            Class::BothSame => self.of(1, n).max(self.of(2, n)),
            _ => self.of(k.side, n),
        }
    }
}

/// The three-way classes and values of the merged keys.
fn classify(x: &HInput<'_>, og: &Origins<'_, '_>) -> BTreeMap<Nid, K> {
    let mut out = BTreeMap::new();
    for n in x.merged {
        let (b, o, t) = (&x.b[n], &x.o[n], &x.t[n]);
        // A side where the node is not live holds `absent`, which differs from a live root with no order although
        // both read `None` (a node a side revived, or deleted, changed the key there).
        let [db, dob, dt] = [0, 1, 2].map(|i| x.dead[i].contains(n));
        let eq =
            |v: &Option<KVal>, dv: bool, w: &Option<KVal>, dw: bool| dv == dw && (dv || v == w);
        let (ot, ob, tb) = (eq(o, dob, t, dt), eq(o, dob, b, db), eq(t, dt, b, db));
        let k = if ot {
            K {
                class: if ob {
                    Class::Untouched
                } else {
                    Class::BothSame
                },
                chosen: o.clone(),
                fallback: b.clone(),
                side: if ob { 0 } else { 1 },
            }
        } else if ob {
            K {
                class: Class::OnlyT,
                chosen: t.clone(),
                fallback: b.clone(),
                side: 2,
            }
        } else if tb {
            K {
                class: Class::OnlyO,
                chosen: o.clone(),
                fallback: b.clone(),
                side: 1,
            }
        } else if takes_t(og, *n, (b, o, t), [db, dob, dt]) {
            K {
                class: Class::BothDiff,
                chosen: t.clone(),
                fallback: o.clone(),
                side: 2,
            }
        } else {
            K {
                class: Class::BothDiff,
                chosen: o.clone(),
                fallback: t.clone(),
                side: 1,
            }
        };
        out.insert(*n, k);
    }
    out
}

/// Whether a two-sided key takes t's value (its three-way value, the MR-040 winner): when b, o and t are all live
/// and exactly one side's value is order-only by state (its parent is b's, its order not), the other side's
/// reparent, whatever their origins (13.5 (b); R-15 Q3, revert and cherry-pick included); otherwise the side whose
/// value has the later origin, t on equal origins (MR-040).
fn takes_t(
    og: &Origins<'_, '_>,
    n: Nid,
    (b, o, t): (&Option<KVal>, &Option<KVal>, &Option<KVal>),
    dead: [bool; 3],
) -> bool {
    if dead == [false; 3] {
        let o_reorder = parent_of(o) == parent_of(b);
        let t_reorder = parent_of(t) == parent_of(b);
        if o_reorder != t_reorder {
            return o_reorder;
        }
    }
    og.of(2, n) >= og.of(1, n)
}

/// `threeway` over one merge's inputs.
fn threeway(x: &HInput<'_>) -> HOutput {
    let og = Origins {
        x,
        memo: RefCell::new(BTreeMap::new()),
    };
    let keys = classify(x, &og);
    let mut values: BTreeMap<Nid, Option<KVal>> = x.init.clone();
    for (n, k) in &keys {
        values.insert(*n, k.chosen.clone());
    }
    let (skipped, hinted) = exact(x, &og, &keys, &mut values);
    values.retain(|n, _| x.merged.contains(n));
    HOutput {
        values,
        skipped,
        hinted,
    }
}

/// A key the repair may change: its value's parent differs from its fallback's (an order-only difference cannot break a
/// cycle).
fn repairable(k: &K) -> bool {
    k.class != Class::Untouched && parent_of(&k.chosen) != parent_of(&k.fallback)
}

/// The nodes that reach no root under any choice of their options: every option of each leads to another of them. A
/// parent that is no node of `opts` (none, or a node outside the result) is a root.
fn unsafe_set(opts: &BTreeMap<Nid, Vec<Option<KVal>>>) -> BTreeSet<Nid> {
    let mut safe: BTreeSet<Nid> = BTreeSet::new();
    loop {
        let mut grew = false;
        for (n, os) in opts {
            if safe.contains(n) {
                continue;
            }
            let ok = os.iter().any(|w| match parent_of(w) {
                None => true,
                Some(p) => !opts.contains_key(&p) || safe.contains(&p),
            });
            if ok {
                safe.insert(*n);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    opts.keys().copied().filter(|n| !safe.contains(n)).collect()
}

/// The members of `u` that lie on a cycle of the option graph restricted to `u` (some choice of options inside `u`
/// leads back to them).
fn on_option_cycle(opts: &BTreeMap<Nid, Vec<Option<KVal>>>, u: &BTreeSet<Nid>) -> BTreeSet<Nid> {
    let succ = |n: Nid| -> Vec<Nid> {
        opts[&n]
            .iter()
            .filter_map(parent_of)
            .filter(|p| u.contains(p))
            .collect()
    };
    let mut out = BTreeSet::new();
    for s in u {
        let mut seen: BTreeSet<Nid> = BTreeSet::new();
        let mut stack = succ(*s);
        while let Some(m) = stack.pop() {
            if m == *s {
                out.insert(*s);
                break;
            }
            if seen.insert(m) {
                stack.extend(succ(m));
            }
        }
    }
    out
}

/// The skipped keys (MR-039) and the hinted keys (13.3 (b)) of [`exact`].
type Exact = (BTreeSet<Nid>, BTreeMap<Nid, (usize, &'static str)>);

/// K2-b: the exact repair.
fn exact(
    x: &HInput<'_>,
    og: &Origins<'_, '_>,
    keys: &BTreeMap<Nid, K>,
    values: &mut BTreeMap<Nid, Option<KVal>>,
) -> Exact {
    // Every result node's allowed values: one for a determined key and for a node an existence policy fixed, two for a
    // `both-diff` key (its MR-040 winner first).
    let mut opts: BTreeMap<Nid, Vec<Option<KVal>>> =
        values.iter().map(|(n, w)| (*n, vec![w.clone()])).collect();
    // A value whose parent is not live in the result would stage a `DanglingEdge` (V04): a `both-diff` key whose MR-040
    // winner has such a parent takes the other side's value first when that one's parent is live (or a root); not under
    // strict, which stages the winner instead (13.3 (b); R-15 Q2).
    let dangles = |w: &Option<KVal>| parent_of(w).is_some_and(|p| !x.live.contains(&p));
    let mut preferred: BTreeSet<Nid> = BTreeSet::new();
    for (n, k) in keys {
        if k.class == Class::BothDiff {
            if !x.strict && dangles(&k.chosen) && !dangles(&k.fallback) {
                opts.insert(*n, vec![k.fallback.clone(), k.chosen.clone()]);
                preferred.insert(*n);
            } else {
                opts.insert(*n, vec![k.chosen.clone(), k.fallback.clone()]);
            }
        }
    }
    // While no choice gives a forest, demote a determined key of a cycle of the unavoidable part to b.
    let pick_op = matches!(x.op, Op::Revert | Op::CherryPick);
    let mut skipped: BTreeSet<Nid> = BTreeSet::new();
    loop {
        let u = unsafe_set(&opts);
        if u.is_empty() {
            break;
        }
        let cyc = on_option_cycle(&opts, &u);
        // The order of the candidates: for a revert or a cherry-pick, a key only C changed (src's) before one dst
        // changed, since C's change is the one applied on dst's forest; a key one side changed before one both sides
        // set alike; a key whose reset to b leaves the unavoidable part (its b parent is a root, no result node, or a
        // node some choice saves) before one whose reset does not; then the latest origin, src's on equal origins;
        // then the least uid.
        let rank = |n: Nid, k: &K| {
            let src_first = u8::from(pick_op && k.class == Class::OnlyT);
            let one_sided = u8::from(k.class != Class::BothSame);
            let escapes = u8::from(match parent_of(&k.fallback) {
                None => true,
                Some(p) => !u.contains(&p),
            });
            // On equal origins src's value is the later one, as a `both-diff` tie goes to t.
            let src_tie = u8::from(k.side == 2);
            (
                src_first,
                one_sided,
                escapes,
                og.chosen(n, k),
                src_tie,
                std::cmp::Reverse((x.uid)(n)),
            )
        };
        let pick = keys
            .iter()
            .filter(|(n, k)| {
                cyc.contains(n)
                    && k.class != Class::BothDiff
                    && !skipped.contains(n)
                    && repairable(k)
            })
            .max_by_key(|(n, k)| rank(**n, k))
            .map(|(n, _)| *n);
        // A cycle with no demotable key (only fixed nodes, untouched keys, keys whose value and fallback have the
        // same parent, two-sided keys and keys already demoted on it) is left to the validators' cycle check (V05,
        // I37′), which stages it.
        let Some(n) = pick else { break };
        opts.insert(n, vec![keys[&n].fallback.clone()]);
        skipped.insert(n);
    }
    // The `both-diff` keys, in ascending origin of their MR-040 winner: each takes its winner unless that leaves a
    // node that can reach no root, else the other side's value (a dead-parent key tries the other side's value first).
    // A key that ends at the other side's value this way lands there with a `MoveSkipped` hint naming the reason the
    // winner was skipped (13.3 (b); R-15 Q1): `cycle`, or `dead-parent` for a key the preference kept at it. Under
    // strict a `cycle` key is MR-039's instead (CS-013: V01 stages a `HierarchyCycle`, the candidate holding
    // the other side's value), and a dead-parent winner stays, which V04 stages (R-15 Q2). A preferred key whose other
    // value would leave a node that can reach no root lands at its winner, with no hint, and V04 stages it.
    let base_u = unsafe_set(&opts).len();
    let mut xs: Vec<Nid> = keys
        .iter()
        .filter(|(_, k)| k.class == Class::BothDiff)
        .map(|(n, _)| *n)
        .collect();
    xs.sort_by_key(|n| (og.chosen(*n, &keys[n]), (x.uid)(*n)));
    let mut hinted: BTreeMap<Nid, (usize, &'static str)> = BTreeMap::new();
    for n in xs {
        let both = opts[&n].clone();
        // The side whose value is the fallback: the MR-040 winner's other side.
        let fallback_side = 3 - keys[&n].side;
        opts.insert(n, vec![both[0].clone()]);
        if unsafe_set(&opts).len() > base_u {
            opts.insert(n, vec![both[1].clone()]);
            if !preferred.contains(&n) {
                if x.strict {
                    skipped.insert(n);
                } else {
                    hinted.insert(n, (fallback_side, "cycle"));
                }
            }
        } else if preferred.contains(&n) {
            hinted.insert(n, (fallback_side, "dead-parent"));
        }
    }
    for (n, os) in opts {
        values.insert(n, os[0].clone());
    }
    (skipped, hinted)
}

/// Pins of `threeway`'s exactness fixes (OQ-A-13 F1, F2 and F4) and of MR-040's tie, on hand-built merges.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::Origin;
    use crate::value::Uid;

    fn uid(n: Nid) -> Uid {
        let mut b = [0u8; 16];
        b[12..].copy_from_slice(&n.0.to_be_bytes());
        Uid(b)
    }

    /// The value of a node under `p` (`None`: the root), with no order.
    fn under(p: Option<u32>) -> Option<KVal> {
        p.map(|p| KVal::Hierarchy {
            parent: Some(Nid(p)),
            order: None,
        })
    }

    /// Parents by node, #1 first, as values.
    fn values(ps: &[Option<u32>]) -> BTreeMap<Nid, Option<KVal>> {
        ps.iter()
            .enumerate()
            .map(|(i, p)| (Nid(i as u32 + 1), under(*p)))
            .collect()
    }

    /// The parents of b's, o's and t's nodes, #1 first (`None`: the root).
    type Sides<'s> = (&'s [Option<u32>], &'s [Option<u32>], &'s [Option<u32>]);

    /// A merge of nodes #1..=#n, all merged and live in b, o and t, given as parents.
    fn input<'a>(
        merged: &'a BTreeSet<Nid>,
        (b, o, t): Sides<'_>,
        origin: Option<Origin<'a>>,
    ) -> HInput<'a> {
        HInput {
            op: Op::Merge,
            merged,
            b: values(b),
            o: values(o),
            t: values(t),
            init: values(b),
            uid: &uid,
            origin,
            dead: Default::default(),
            live: merged.clone(),
            strict: false,
        }
    }

    fn origins<'x, 'a>(x: &'x HInput<'a>) -> Origins<'x, 'a> {
        Origins {
            x,
            memo: RefCell::new(BTreeMap::new()),
        }
    }

    /// F1 (ALT-7): "lies on a cycle within U" is the cycle of the option graph restricted to U. b holds a (#1), b (#2)
    /// and d (#3) at the root; o puts d under a and b under d; t puts d under b and a under d; every origin is (0, 0).
    /// Every choice of d's two values leads into a cycle, so U = {a, b, d}, and both determined keys lie on a cycle of
    /// the option graph: a (t's, through d's o value) and b (o's, through d's t value). The tie-breaks reset a (src's
    /// on equal origins); d then takes ours, a, with a `cycle` hint, and b stays under d. Reading the cycle on the
    /// current values (d at its MR-040 winner, under b) would reset b instead.
    #[test]
    fn f1_the_reset_key_lies_on_a_cycle_of_the_option_graph() {
        let merged: BTreeSet<Nid> = (1..=3).map(Nid).collect();
        let x = input(
            &merged,
            (
                &[None, None, None],
                &[None, Some(3), Some(1)],
                &[Some(3), None, Some(2)],
            ),
            None,
        );
        let out = threeway(&x);
        assert_eq!(out.skipped, BTreeSet::from([Nid(1)]), "a is reset");
        assert_eq!(
            out.values,
            values(&[None, Some(3), Some(1)]),
            "a at the root, b under d, d under a"
        );
        assert_eq!(out.hinted, BTreeMap::from([(Nid(3), (1, "cycle"))]));
    }

    /// F2: a both-same key's origin, which orders it among step (1)'s tie-breaks, is the later of both sides' origins,
    /// whichever side that is. #1 and #2 are both under #3 on o and on t; #1's o value is the older, #2's the newer.
    #[test]
    fn f2_a_both_same_keys_origin_is_the_later_of_its_sides() {
        let merged: BTreeSet<Nid> = (1..=3).map(Nid).collect();
        let og_fn = |side: usize, n: Nid, _live: bool, _v: &Option<KVal>| {
            let older_side = if n == Nid(1) { 1 } else { 2 };
            if side == older_side {
                (5, [1; 32])
            } else {
                (9, [2; 32])
            }
        };
        let x = input(
            &merged,
            (
                &[None, None, None],
                &[Some(3), Some(3), None],
                &[Some(3), Some(3), None],
            ),
            Some(&og_fn),
        );
        let og = origins(&x);
        let keys = classify(&x, &og);
        for n in [Nid(1), Nid(2)] {
            assert_eq!(keys[&n].class, Class::BothSame);
            assert_eq!(og.chosen(n, &keys[&n]), (9, [2; 32]), "{n}");
        }
    }

    /// MR-040's tie: a two-sided key whose sides' values have equal origins takes t's value. o puts #1 under #2, t
    /// under #3; neither is order-only.
    #[test]
    fn a_two_sided_tie_goes_to_t() {
        let merged: BTreeSet<Nid> = (1..=3).map(Nid).collect();
        let x = input(
            &merged,
            (
                &[None, None, None],
                &[Some(2), None, None],
                &[Some(3), None, None],
            ),
            None,
        );
        let og = origins(&x);
        let keys = classify(&x, &og);
        assert_eq!(
            (keys[&Nid(1)].class, keys[&Nid(1)].side),
            (Class::BothDiff, 2)
        );
        assert_eq!(threeway(&x).values[&Nid(1)], under(Some(3)));
    }

    /// F4: a cycle that holds no repairable key is left as it is, for the validators' cycle check (V05, I37′): #1, a
    /// node an existence policy fixed, is under #2 as copied, and #2's merged key is under #1 in b, o and t.
    /// `threeway` skips nothing, hints nothing and changes no value; #3, which o puts under #2, cannot reach a root
    /// either but lies on no cycle, so it keeps o's value.
    #[test]
    fn f4_a_cycle_with_no_repairable_key_is_left_to_the_validators() {
        let merged = BTreeSet::from([Nid(2), Nid(3)]);
        let side = |three: Option<u32>| -> BTreeMap<Nid, Option<KVal>> {
            BTreeMap::from([(Nid(2), under(Some(1))), (Nid(3), under(three))])
        };
        let x = HInput {
            init: values(&[Some(2), Some(1), None]),
            b: side(None),
            o: side(Some(2)),
            t: side(None),
            live: BTreeSet::from([Nid(1), Nid(2), Nid(3)]),
            ..input(&merged, (&[], &[], &[]), None)
        };
        let out = threeway(&x);
        assert!(
            out.skipped.is_empty() && out.hinted.is_empty(),
            "{:?} {:?}",
            out.skipped,
            out.hinted
        );
        assert_eq!(
            out.values,
            BTreeMap::from([(Nid(2), under(Some(1))), (Nid(3), under(Some(2)))])
        );
    }

    /// Order alone closes no cycle (RS-007's participants; OQ-A-11 11.2): a key whose value and fallback have the same
    /// parent is never reset, even when it lies on a cycle no other key can break. #1, a node an existence policy
    /// fixed, is under #2 as copied; #2's merged key is under #1 in b and o, and t gives it a new order there
    /// (`only-t`, order-only). The cycle #1 ↔ #2 holds no repairable key, so it is left to the validators (F4):
    /// `threeway` resets nothing, hints nothing, and #2 keeps t's order. A repair that admitted the order-only key
    /// would reset it to b and make it MR-039's.
    #[test]
    fn an_order_only_key_is_never_reset() {
        let merged = BTreeSet::from([Nid(2)]);
        let reordered = Some(KVal::Hierarchy {
            parent: Some(Nid(1)),
            order: Some("V".into()),
        });
        let x = HInput {
            init: values(&[Some(2), None]),
            b: BTreeMap::from([(Nid(2), under(Some(1)))]),
            o: BTreeMap::from([(Nid(2), under(Some(1)))]),
            t: BTreeMap::from([(Nid(2), reordered.clone())]),
            live: BTreeSet::from([Nid(1), Nid(2)]),
            ..input(&merged, (&[], &[], &[]), None)
        };
        let out = threeway(&x);
        assert!(
            out.skipped.is_empty() && out.hinted.is_empty(),
            "{:?} {:?}",
            out.skipped,
            out.hinted
        );
        assert_eq!(out.values, BTreeMap::from([(Nid(2), reordered)]));
    }
}
