//! The candidate slot of the RS-007 evaluation harness (`suite::rs007eval`; test builds only), filled with the
//! candidate **K2, `threeway`**: a state-based three-way merge of the hierarchy keys with a cycle repair, and no replay.
//!
//! Each merged node's hierarchy key (parent, order) is decided from the three states, as every other key class is
//! ([AR §2.7] T7, "one side equals base → take the other"):
//! - o = t: that value (`untouched` when it is also b's, else `both-same`);
//! - o = b: t (`only-t`); t = b: o (`only-o`);
//! - otherwise (`both-diff`): the side whose value has the later origin time wins (MR-040), t on a tie.
//!
//! The origin time of side S's value of a key is the (hlc, commit id) of the commit that produced it
//! ([`crate::dag::Dag::origin`]): the walk from S's tip passes over every commit whose net changeset against its first
//! parent leaves the key's value as it was, and over a two-parent commit that holds its second parent's value (a
//! `sync` that takes `main`'s move does not re-time it; a resolution to `ours` that keeps the side's own move keeps its
//! original time); the first other commit that changed it produced it; ε gives (0, 0). For a revert or a cherry-pick of
//! C, src's value is C's (its origin is C's (hlc, commit id)). For a virtual merge's dst, a fold of LCAs, it is the
//! latest origin among the folded LCAs that hold the value, and (0, 0) when none does.
//!
//! Then the cycle repair, over the result nodes (merged nodes at their value, nodes an existence policy fixed at their
//! copied value as immovable context; a parent that is no result node ends a chain). Only a key whose value and
//! fallback have different parents can break a cycle (order cannot close one, OQ-A-11 11.2), so only such a key is ever
//! repaired. The variant `MOIRAI_K2_VARIANT` selects the repair (default `exact`, the final candidate):
//! - `greedy` (K2-a, the first statement): while a cycle exists, the repairable key on a cycle whose value has the
//!   latest origin is demoted to its fallback (the other side's value for `both-diff`, else b's), `both-same` keys
//!   only when no other repairable key is on the cycle; every demoted key is `kleppmann-skipped` (stages);
//! - `greedy-silent` (K2-a2): the same, `both-diff` keys first, and a demoted `both-diff` key lands at the other
//!   side's value (skipped and logged, not staged);
//! - `exact` (K2-b): a `both-diff` key has two allowed values; first, while no choice of them gives a forest, a
//!   determined key on a cycle of the unavoidable part (the nodes from which every choice leads into a cycle) is
//!   demoted to b and `kleppmann-skipped`: for a revert or a cherry-pick a key only C changed first; a key one side
//!   changed before one both sides set alike; a key whose reset leaves the unavoidable part first; then the latest
//!   origin (src's on equal origins); then the least uid. Then the `both-diff` keys are fixed in ascending origin of
//!   their MR-040 winner, each to its winner unless that leaves no forest, else to the other side's value (the later
//!   move that closes a cycle is skipped, as Kleppmann's replay skips it, and logged, not staged). A `both-diff` key
//!   whose MR-040 winner has a parent that is not live in the result (a `DanglingEdge`, V04) while the other side's
//!   value has a live parent or none tries the other side's value first, as a cycle-closing winner does.
//!
//! Each demotion and each `both-diff` key that takes the other side's value counts one [`Counter::Repair`].

use super::{Counter, HInput, HOutput, MoveKey, Op, bump, parent_of};
use crate::state::KVal;
use crate::value::Nid;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// OQ-A-12 (b): unused by `threeway`, which never replays.
pub(crate) const KEPT_KEYS: bool = false;
/// OQ-A-12 (b)'s backstop: unused.
pub(crate) const BACKSTOP: bool = false;
/// OQ-A-12 (c): `threeway` reads no step keys, so none are derived.
pub(crate) const RESOLVED_KEYS: bool = false;
/// The replay from B: unused.
pub(crate) const FROM_B: bool = false;
/// `threeway` reads no replay input: no replay start, no step, no step key ([`super::no_replay`]).
pub(crate) const NO_REPLAY: bool = true;

/// The repair `MOIRAI_K2_VARIANT` selects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Variant {
    /// K2-a: greedy, latest origin first, every demotion staged.
    Greedy,
    /// K2-a2: greedy, `both-diff` first, a `both-diff` demotion lands.
    GreedySilent,
    /// K2-b: the exact repair (the final candidate).
    Exact,
}

/// The variant of this process: `MOIRAI_K2_VARIANT` = `greedy`, `greedy-silent` or `exact` (default).
pub(crate) fn variant() -> Variant {
    static V: std::sync::OnceLock<Variant> = std::sync::OnceLock::new();
    *V.get_or_init(|| match std::env::var("MOIRAI_K2_VARIANT").as_deref() {
        Ok("greedy") => Variant::Greedy,
        Ok("greedy-silent") => Variant::GreedySilent,
        Ok("exact") | Ok("") | Err(_) => Variant::Exact,
        Ok(v) => panic!("MOIRAI_K2_VARIANT={v:?}: expected greedy, greedy-silent or exact"),
    })
}

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
pub(crate) fn hierarchy(x: &HInput<'_>) -> Option<HOutput> {
    Some(threeway(x, variant()))
}

/// The origin times of the sides' values, memoised per merge: the caller's [`super::Ctx::origin`], or for a hand-built
/// merge with no history the key of the side's last step that sets the side's value ((0, 0) when none does).
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
            Some(f) => f(side, n, &v),
            None => self
                .x
                .steps
                .iter()
                .rev()
                .filter(|s| s.side + 1 == side)
                .find_map(|s| {
                    s.moves
                        .iter()
                        .find(|(m, _)| *m == n)
                        .map(|(_, w)| (s.key, w))
                })
                .filter(|(_, w)| **w == v)
                .map(|(key, _)| key)
                .unwrap_or((0, [0; 32])),
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
        } else if og.of(2, *n) >= og.of(1, *n) {
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

/// Whether `n` is its own ancestor in the parent map `p` (a parent outside the map ends the chain).
fn cyclic(p: &BTreeMap<Nid, Option<Nid>>, n: Nid) -> bool {
    let mut x = p.get(&n).copied().flatten();
    for _ in 0..=p.len() {
        match x {
            None => return false,
            Some(y) if y == n => return true,
            Some(y) => x = p.get(&y).copied().flatten(),
        }
    }
    false
}

/// `threeway` over one merge's inputs.
fn threeway(x: &HInput<'_>, v: Variant) -> HOutput {
    let og = Origins {
        x,
        memo: RefCell::new(BTreeMap::new()),
    };
    let keys = classify(x, &og);
    let mut values: BTreeMap<Nid, Option<KVal>> = x.init.clone();
    for (n, k) in &keys {
        values.insert(*n, k.chosen.clone());
    }
    let skipped = match v {
        Variant::Greedy | Variant::GreedySilent => greedy(x, &og, &keys, &mut values, v),
        Variant::Exact => exact(x, &og, &keys, &mut values),
    };
    values.retain(|n, _| x.merged.contains(n));
    HOutput { values, skipped }
}

/// A key the repair may change: its value's parent differs from its fallback's (an order-only difference cannot break a
/// cycle).
fn repairable(k: &K) -> bool {
    k.class != Class::Untouched && parent_of(&k.chosen) != parent_of(&k.fallback)
}

/// K2-a and K2-a2: while a cycle exists, demote the repairable key on a cycle with the highest rank, then the latest
/// origin, then the least uid.
fn greedy(
    x: &HInput<'_>,
    og: &Origins<'_, '_>,
    keys: &BTreeMap<Nid, K>,
    values: &mut BTreeMap<Nid, Option<KVal>>,
    v: Variant,
) -> BTreeSet<Nid> {
    let mut demoted: BTreeSet<Nid> = BTreeSet::new();
    let mut skipped = BTreeSet::new();
    loop {
        let parents: BTreeMap<Nid, Option<Nid>> =
            values.iter().map(|(n, w)| (*n, parent_of(w))).collect();
        let rank = |k: &K| match (v, k.class) {
            (_, Class::BothSame) => 0,
            (Variant::GreedySilent, Class::BothDiff) => 2,
            _ => 1,
        };
        let pick = keys
            .iter()
            .filter(|(n, k)| !demoted.contains(n) && repairable(k) && cyclic(&parents, **n))
            .max_by_key(|(n, k)| (rank(k), og.chosen(**n, k), std::cmp::Reverse((x.uid)(**n))))
            .map(|(n, _)| *n);
        let Some(n) = pick else { break };
        let k = &keys[&n];
        values.insert(n, k.fallback.clone());
        demoted.insert(n);
        if v == Variant::Greedy || k.class != Class::BothDiff {
            skipped.insert(n);
        }
        bump(Counter::Repair);
    }
    skipped
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

/// K2-b: the exact repair.
fn exact(
    x: &HInput<'_>,
    og: &Origins<'_, '_>,
    keys: &BTreeMap<Nid, K>,
    values: &mut BTreeMap<Nid, Option<KVal>>,
) -> BTreeSet<Nid> {
    // Every result node's allowed values: one for a determined key and for a node an existence policy fixed, two for a
    // `both-diff` key (its MR-040 winner first).
    let mut opts: BTreeMap<Nid, Vec<Option<KVal>>> =
        values.iter().map(|(n, w)| (*n, vec![w.clone()])).collect();
    // A value whose parent is not live in the result would stage a `DanglingEdge` (V04): a `both-diff` key whose MR-040
    // winner has such a parent takes the other side's value first when that one's parent is live (or a root).
    let dangles = |w: &Option<KVal>| parent_of(w).is_some_and(|p| !x.live.contains(&p));
    for (n, k) in keys {
        if k.class == Class::BothDiff {
            if dangles(&k.chosen) && !dangles(&k.fallback) {
                opts.insert(*n, vec![k.fallback.clone(), k.chosen.clone()]);
                bump(Counter::Repair);
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
        // A cycle with no demotable key (only fixed nodes, untouched keys and keys already demoted on it) is left to
        // the validators' cycle check (V05, I37′), which stages it.
        let Some(n) = pick else { break };
        opts.insert(n, vec![keys[&n].fallback.clone()]);
        skipped.insert(n);
        bump(Counter::Repair);
    }
    // The `both-diff` keys, in ascending origin of their MR-040 winner: each takes its winner unless that leaves a
    // node that can reach no root, else the other side's value.
    let base_u = unsafe_set(&opts).len();
    let mut xs: Vec<Nid> = keys
        .iter()
        .filter(|(_, k)| k.class == Class::BothDiff)
        .map(|(n, _)| *n)
        .collect();
    xs.sort_by_key(|n| (og.chosen(*n, &keys[n]), (x.uid)(*n)));
    for n in xs {
        let both = opts[&n].clone();
        opts.insert(n, vec![both[0].clone()]);
        if unsafe_set(&opts).len() > base_u {
            opts.insert(n, vec![both[1].clone()]);
            bump(Counter::Repair);
        }
    }
    for (n, os) in opts {
        values.insert(n, os[0].clone());
    }
    skipped
}
