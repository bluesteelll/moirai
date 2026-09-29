//! The crash states enumerated at one crash point: [F15 §6.4]'s obligations turned into [`CrashPlan`]s over a
//! [`CrashSurface`].
//!
//! Every choice a crash may make is an **axis**: one file's sectors and size, the slot file's sectors (the `HEAD` slots),
//! the pending namespace operations, one write in flight. Each axis has a list of alternatives. The plans are:
//! - the two **pivots** — everything at its baseline, everything at its newest ([`Resolve`]);
//! - a **single-axis sweep**: every alternative of every axis, with every other axis at each pivot (an in-flight write
//!   only against the newest pivot, where its applied bytes persist);
//! - a **bounded product** across axes ("bounded cross-file products"): every combination of the axes' corner
//!   alternatives while there are at most `cross_budget` of them, else `cross_budget` sampled combinations;
//! - in [`PlanMode::Full`], **random states** that sample intermediate versions (FM-1.1), poisoned sub-sector mixes
//!   (FM-3.3) and torn sectors on every axis at once: `random_per_point` of them, and at least `random_min` when an axis
//!   is too large to enumerate (more than `exhaustive_max` dirty sectors in a file, pending operations, or sectors of a
//!   write in flight).
//!
//! Per axis:
//! - **A file** ([`PlanMode::Prefix`], the PR tier: "per-file prefixes plus one torn sector"): its non-clean sectors in
//!   offset order, the first j at their newest content and the rest at their baseline, for every j; and each of those
//!   states with its boundary sector torn — sub-sectors 0–3 new, 4–7 old — (a dirty sector, FM-1.2) or mixed (a poisoned
//!   or dirty-over-poison sector, FM-3.3). [`PlanMode::Full`]: every subset of the dirty sectors at baseline or newest
//!   while there are at most `exhaustive_max` of them (4,096 states at 12), each intermediate version (FM-1.1), and every
//!   sector torn with all 14 prefix and suffix sub-sector mixes. Both: every size of H(f), the current one included,
//!   with the bytes beyond the old durable size as resolved and as garbage (FM-2.2, OP-4: ext4 `data=writeback` stale
//!   blocks; zeros too in `Full`).
//! - **A slot file** (the `HEAD` slots, [F15 §6.4]: "{old, new, torn} × {old, new, torn}, where old ranges over every
//!   version since the slot's durable point"): see [`slot_axis`].
//! - **The pending namespace operations** (FM-2.3: "any subset … in any order"): `Prefix` takes the issue-order prefixes,
//!   each one lost alone and each one surviving alone; `Full` every subset while there are at most `exhaustive_max`.
//! - **A write in flight** (§2.5): applied not at all, fully, only its first sector, all but its first sector; `Full`
//!   every subset of its sectors while there are at most `exhaustive_max`, half of its bytes, and a random byte mask.
//!
//! **Cost.** The plans are generated one at a time and handed to the caller as they come ([`for_each_plan`]); nothing
//! holds more than one axis's corner alternatives, and duplicates are dropped by a 128-bit structural fingerprint
//! (`Hash`), not by keeping the plans. A point with 12 dirty sectors and 10⁴ random states therefore costs the memory of
//! one plan plus 16 bytes per plan seen.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};

use crate::adversary::PartialWrite;
use crate::content::{BeyondFill, SectorKind, SectorView};
use crate::crash::{
    CrashPlan, CrashSurface, FilePlan, FileSurface, OpSurface, Resolve, SectorPick, WriteSurface,
};
use crate::rng::{Rng, splitmix};

/// Which crash-state dimension a plan belongs to (the nightly tier's state counts).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Dim {
    /// Everything at its baseline, or everything at its newest.
    Pivot,
    /// A per-file prefix of the non-clean sectors (the PR tier).
    Prefix,
    /// A subset of a file's non-clean sectors, or an intermediate version.
    Subset,
    /// A torn dirty sector (FM-1.2).
    Torn,
    /// A poisoned sector's sub-sector mix (FM-3.3).
    Poison,
    /// Another size of H(f), or the bytes beyond the old durable size (FM-2.2).
    Size,
    /// A state of the slot file's sectors (the `HEAD` slots).
    Slot,
    /// A subset of the pending namespace operations (FM-2.3).
    Namespace,
    /// A partial application of a write in flight (§2.5).
    InFlight,
    /// A combination across axes ("bounded cross-file products").
    Cross,
    /// A random state of every axis.
    Random,
}

/// How many crash states to enumerate at one crash point.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PlanMode {
    /// The two pivots only.
    Pivots,
    /// The PR tier: per-file prefixes plus one torn sector, the slot states, namespace prefixes, bounded products.
    Prefix,
    /// The nightly tier: exhaustive subsets, every torn mix, random states.
    Full,
}

/// The numbers the plan generator obeys ([F15 §6.4]; they are fixed by the design and are not holes).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PlanLimits {
    /// Enumerate every subset while there are at most this many (12).
    pub exhaustive_max: u32,
    /// At least this many random states when an axis is larger (10⁴ in the nightly tier).
    pub random_min: u32,
    /// Random states at every crash point in [`PlanMode::Full`].
    pub random_per_point: u32,
    /// The bound of the product across axes.
    pub cross_budget: u32,
}

/// Sub-sectors 0–3 new, 4–7 old: a write torn after its first half.
const HALF_NEW: u8 = 0b0000_1111;
/// Sub-sectors 0–3 old, 4–7 new.
const HALF_OLD: u8 = 0b1111_0000;

/// The 14 torn mixes of [`PlanMode::Full`]: the first j sub-sectors new (j = 1..7), and the last 8 − j new.
fn all_mixes() -> Vec<u8> {
    let mut m = Vec::with_capacity(14);
    for j in 1..8u32 {
        let prefix = ((1u16 << j) - 1) as u8;
        m.push(prefix);
        m.push(!prefix);
    }
    m
}

/// The torn mixes of `mode`: the two half mixes in `Prefix`, all 14 in `Full`.
fn mixes_of(mode: PlanMode) -> Vec<u8> {
    if mode == PlanMode::Full {
        all_mixes()
    } else {
        vec![HALF_NEW, HALF_OLD]
    }
}

/// A sub-sector pick: bit j of `mix` takes candidate `new`, else candidate `old`.
fn mixed(mix: u8, old: u64, new: u64) -> SectorPick {
    let mut s = [old; 8];
    for (j, v) in s.iter_mut().enumerate() {
        if mix & (1 << j) != 0 {
            *v = new;
        }
    }
    SectorPick::Subsectors(s)
}

fn newest(s: &SectorView) -> u64 {
    s.candidates.saturating_sub(1)
}

/// A 64-bit FNV-1a hasher: the second half of a plan's fingerprint.
struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
}

/// A 128-bit structural fingerprint of a plan: SipHash-1-3 (fixed keys) and FNV-1a over its `Hash` form. Equal plans
/// have equal fingerprints; two distinct plans of one crash point share one with a probability near 2⁻¹²⁸.
fn fingerprint(p: &CrashPlan) -> u128 {
    let mut a = std::hash::DefaultHasher::new();
    let mut b = Fnv(0xCBF2_9CE4_8422_2325);
    p.hash(&mut a);
    p.hash(&mut b);
    (u128::from(a.finish()) << 64) | u128::from(b.finish())
}

/// The sink of one crash point's plans: drops duplicates and stops once the caller has had enough.
struct Emit<'f> {
    seen: HashSet<u128>,
    f: &'f mut dyn FnMut(Dim, CrashPlan) -> bool,
    open: bool,
}

impl Emit<'_> {
    fn push(&mut self, dim: Dim, plan: CrashPlan) {
        if self.open && self.seen.insert(fingerprint(&plan)) {
            self.open = (self.f)(dim, plan);
        }
    }
}

/// One axis of the crash surface: its corner alternatives are kept (the bounded product needs them), every other
/// alternative is generated when swept.
enum Axis<'s> {
    File {
        f: &'s FileSurface,
        corners: Vec<FilePlan>,
    },
    Slot {
        f: &'s FileSurface,
        corners: Vec<FilePlan>,
    },
    Ns {
        ops: &'s [OpSurface],
        corners: Vec<BTreeSet<u64>>,
    },
    Write {
        w: &'s WriteSurface,
    },
}

/// One alternative of an axis, to lay onto a plan.
#[derive(Clone)]
enum Alt {
    File(u64, FilePlan),
    Ns(BTreeSet<u64>),
    Write(u64, PartialWrite),
}

impl Alt {
    fn apply(self, p: &mut CrashPlan) {
        match self {
            Alt::File(node, fp) => {
                p.files.insert(node, fp);
            }
            Alt::Ns(s) => p.survivors = Some(s),
            Alt::Write(id, pw) => {
                p.in_flight.insert(id, pw);
            }
        }
    }
}

/// What the generator needs besides the surface.
struct Gen<'a> {
    mode: PlanMode,
    limits: &'a PlanLimits,
    seed: u64,
}

impl Axis<'_> {
    /// The alternatives the bounded product combines.
    fn corners(&self) -> Vec<Alt> {
        match self {
            Axis::File { f, corners } | Axis::Slot { f, corners } => corners
                .iter()
                .map(|fp| Alt::File(f.node, fp.clone()))
                .collect(),
            Axis::Ns { corners, .. } => corners.iter().map(|s| Alt::Ns(s.clone())).collect(),
            Axis::Write { w } => vec![
                Alt::Write(w.id, PartialWrite::Nothing),
                Alt::Write(w.id, PartialWrite::All),
            ],
        }
    }

    /// The pivots it is swept against: a write in flight only against the newest one.
    fn pivots(&self) -> &'static [Resolve] {
        match self {
            Axis::Write { .. } => &[Resolve::Newest],
            _ => &[Resolve::Baseline, Resolve::Newest],
        }
    }

    /// Every alternative of the axis, one at a time; `emit` returns `false` to stop.
    fn for_each_alt(&self, g: &Gen<'_>, emit: &mut dyn FnMut(Dim, Alt) -> bool) {
        match self {
            Axis::File { f, corners } => {
                let mut go = |d: Dim, fp: FilePlan| emit(d, Alt::File(f.node, fp));
                for c in corners {
                    if !go(Dim::Prefix, c.clone()) {
                        return;
                    }
                }
                if file_alts(f, g, &mut go) {
                    size_alts(f, g, &mut go);
                }
            }
            Axis::Slot { f, corners } => {
                let mut go = |d: Dim, fp: FilePlan| emit(d, Alt::File(f.node, fp));
                for c in corners {
                    if !go(Dim::Slot, c.clone()) {
                        return;
                    }
                }
                size_alts(f, g, &mut go);
            }
            Axis::Ns { ops, corners } => {
                for c in corners {
                    if !emit(Dim::Namespace, Alt::Ns(c.clone())) {
                        return;
                    }
                }
                let n = ops.len();
                if g.mode == PlanMode::Full && n as u64 <= u64::from(g.limits.exhaustive_max) {
                    for mask in 0u64..(1u64 << n) {
                        let s = ops
                            .iter()
                            .enumerate()
                            .filter(|&(k, _)| mask & (1 << k) != 0)
                            .map(|(_, o)| o.id)
                            .collect();
                        if !emit(Dim::Namespace, Alt::Ns(s)) {
                            return;
                        }
                    }
                }
            }
            Axis::Write { w } => {
                for pw in write_alts(w, g) {
                    if !emit(Dim::InFlight, Alt::Write(w.id, pw)) {
                        return;
                    }
                }
            }
        }
    }
}

/// The crash plans at one crash point (see the module documentation), collected. `slots` are the nodes of the slot
/// files; `seed` drives the sampled products and the random states.
pub fn crash_plans(
    surface: &CrashSurface,
    slots: &BTreeSet<u64>,
    mode: PlanMode,
    limits: &PlanLimits,
    seed: u64,
) -> Vec<(Dim, CrashPlan)> {
    let mut out = Vec::new();
    for_each_plan(surface, slots, mode, limits, seed, &mut |d, p| {
        out.push((d, p));
        true
    });
    out
}

/// The crash plans at one crash point, handed to `f` one at a time without duplicates, in a deterministic order; `f`
/// returns `false` to stop (see the module documentation).
pub fn for_each_plan(
    surface: &CrashSurface,
    slots: &BTreeSet<u64>,
    mode: PlanMode,
    limits: &PlanLimits,
    seed: u64,
    f: &mut dyn FnMut(Dim, CrashPlan) -> bool,
) {
    let mut out = Emit {
        seen: HashSet::new(),
        f,
        open: true,
    };
    out.push(Dim::Pivot, CrashPlan::baseline());
    out.push(Dim::Pivot, CrashPlan::newest());
    if mode == PlanMode::Pivots {
        return;
    }
    let g = Gen { mode, limits, seed };
    let mut big = false;
    let mut axes = Vec::new();
    for f in &surface.files {
        if slots.contains(&f.node) {
            axes.push(Axis::Slot {
                f,
                corners: slot_states(f, mode),
            });
        } else {
            big |= f.dirty().count() as u64 > u64::from(limits.exhaustive_max);
            let n = f.sectors.len();
            axes.push(Axis::File {
                f,
                corners: (0..=n)
                    .map(|j| keep_size(f, prefix_picks(&f.sectors, j)))
                    .collect(),
            });
        }
    }
    if !surface.ops.is_empty() {
        big |= surface.ops.len() as u64 > u64::from(limits.exhaustive_max);
        axes.push(Axis::Ns {
            ops: &surface.ops,
            corners: ns_corners(&surface.ops),
        });
    }
    for w in &surface.writes {
        big |= sectors_of(w) > u64::from(limits.exhaustive_max);
        axes.push(Axis::Write { w });
    }
    // The single-axis sweep.
    for axis in &axes {
        axis.for_each_alt(&g, &mut |dim, alt| {
            for &pivot in axis.pivots() {
                let mut p = CrashPlan {
                    resolve: pivot,
                    ..CrashPlan::default()
                };
                alt.clone().apply(&mut p);
                out.push(dim, p);
            }
            out.open
        });
        if !out.open {
            return;
        }
    }
    let mut rng = Rng::new(seed);
    // The bounded product across axes.
    if axes.len() >= 2 {
        let corners: Vec<Vec<Alt>> = axes.iter().map(Axis::corners).collect();
        let total = corners
            .iter()
            .try_fold(1u64, |acc, c| acc.checked_mul(c.len() as u64))
            .unwrap_or(u64::MAX);
        let budget = u64::from(limits.cross_budget);
        let combo = |idx: &[usize]| {
            let mut p = CrashPlan::newest();
            for (a, &i) in corners.iter().zip(idx) {
                a[i].clone().apply(&mut p);
            }
            p
        };
        if total <= budget {
            let mut idx = vec![0usize; corners.len()];
            'all: loop {
                out.push(Dim::Cross, combo(&idx));
                if !out.open {
                    return;
                }
                for d in 0..idx.len() {
                    idx[d] += 1;
                    if idx[d] < corners[d].len() {
                        continue 'all;
                    }
                    idx[d] = 0;
                }
                break;
            }
        } else {
            for _ in 0..budget {
                let idx: Vec<usize> = corners
                    .iter()
                    .map(|c| rng.below(c.len() as u64) as usize)
                    .collect();
                out.push(Dim::Cross, combo(&idx));
                if !out.open {
                    return;
                }
            }
        }
    }
    // Random states.
    if mode == PlanMode::Full {
        let mut n = u64::from(limits.random_per_point);
        if big {
            n += u64::from(limits.random_min);
        }
        for _ in 0..n {
            out.push(Dim::Random, random_plan(surface, &mut rng));
            if !out.open {
                return;
            }
        }
    }
}

/// A file plan that keeps the current size and sets `picks`.
fn keep_size(f: &FileSurface, picks: BTreeMap<u64, SectorPick>) -> FilePlan {
    FilePlan {
        size: Some(f.size),
        sectors: picks,
        beyond: None,
    }
}

/// The first `j` sectors at their newest content, the rest at their baseline.
fn prefix_picks(secs: &[SectorView], j: usize) -> BTreeMap<u64, SectorPick> {
    secs.iter()
        .enumerate()
        .map(|(i, s)| {
            let v = if i < j { newest(s) } else { 0 };
            (s.index, SectorPick::Version(v))
        })
        .collect()
}

/// The torn states and subsets of an ordinary file beyond its prefixes; `false` once `go` stopped.
fn file_alts(f: &FileSurface, g: &Gen<'_>, go: &mut dyn FnMut(Dim, FilePlan) -> bool) -> bool {
    let secs = &f.sectors;
    let n = secs.len();
    let torn_dim = |s: &SectorView| {
        if s.state == SectorKind::Dirty {
            Dim::Torn
        } else {
            Dim::Poison
        }
    };
    match g.mode {
        PlanMode::Pivots => true,
        PlanMode::Prefix => {
            // Each prefix state with its boundary sector torn (or mixed, if poisoned).
            for (j, s) in secs.iter().enumerate() {
                let mut picks = prefix_picks(secs, j);
                picks.insert(s.index, mixed(HALF_NEW, 0, newest(s)));
                if !go(torn_dim(s), keep_size(f, picks)) {
                    return false;
                }
            }
            true
        }
        PlanMode::Full => {
            // Every subset of the dirty sectors at baseline or newest while there are at most `exhaustive_max` of them
            // ([F15 §6.4]); the poisoned and dirty-over-poison sectors (FM-3.3) all at their oldest, then all at their
            // newest candidate (their sub-sector mixes are the torn states below and the random states).
            let dirty: Vec<usize> = (0..n)
                .filter(|&i| secs[i].state == SectorKind::Dirty)
                .collect();
            let others = dirty.len() < n;
            if dirty.len() as u64 <= u64::from(g.limits.exhaustive_max) {
                for rest_new in [false, true].into_iter().take(if others { 2 } else { 1 }) {
                    for mask in 0u64..(1u64 << dirty.len()) {
                        let mut bit = 0;
                        let picks = secs
                            .iter()
                            .map(|s| {
                                let new = if s.state == SectorKind::Dirty {
                                    bit += 1;
                                    mask & (1 << (bit - 1)) != 0
                                } else {
                                    rest_new
                                };
                                (
                                    s.index,
                                    SectorPick::Version(if new { newest(s) } else { 0 }),
                                )
                            })
                            .collect();
                        if !go(Dim::Subset, keep_size(f, picks)) {
                            return false;
                        }
                    }
                }
            }
            // Intermediate versions (FM-1.1, OP-3), the other sectors at their newest.
            for s in secs {
                for v in 1..newest(s) {
                    let mut picks = prefix_picks(secs, n);
                    picks.insert(s.index, SectorPick::Version(v));
                    if !go(Dim::Subset, keep_size(f, picks)) {
                        return false;
                    }
                }
            }
            // Every sector torn (or mixed) with every mix at its prefix boundary, and with the two half mixes against
            // all-old and all-new.
            let mixes = all_mixes();
            for (j, s) in secs.iter().enumerate() {
                for &mix in &mixes {
                    let mut picks = prefix_picks(secs, j);
                    picks.insert(s.index, mixed(mix, 0, newest(s)));
                    if !go(torn_dim(s), keep_size(f, picks)) {
                        return false;
                    }
                }
                for ctx in [0, n] {
                    for mix in [HALF_NEW, HALF_OLD] {
                        let mut picks = prefix_picks(secs, ctx);
                        picks.insert(s.index, mixed(mix, 0, newest(s)));
                        if !go(torn_dim(s), keep_size(f, picks)) {
                            return false;
                        }
                    }
                }
            }
            true
        }
    }
}

/// The seed of the garbage beyond the old durable size of `node` at `size` (independent of the generation order).
fn garbage_seed(seed: u64, node: u64, size: u64) -> u64 {
    let mut s = seed ^ node.rotate_left(17) ^ size.rotate_left(41);
    splitmix(&mut s)
}

/// Every size of H(f), the sectors at their newest; beyond the old durable size resolved, garbage (and zeros in
/// `Full`). The current size cs(f) with its bytes beyond ds(f) as resolved is the newest pivot; with garbage or zeros
/// beyond ds(f) (FM-2.2, OP-4: ext4 `data=writeback` stale blocks — the most common state of an extended file) it is
/// swept here. `false` once `go` stopped.
fn size_alts(f: &FileSurface, g: &Gen<'_>, go: &mut dyn FnMut(Dim, FilePlan) -> bool) -> bool {
    for &size in &f.sizes {
        let mut fills = Vec::with_capacity(3);
        if size != f.size {
            fills.push(BeyondFill::Resolved);
        }
        if size > f.durable_size {
            fills.push(BeyondFill::Garbage(garbage_seed(g.seed, f.node, size)));
            if g.mode == PlanMode::Full {
                fills.push(BeyondFill::Zeros);
            }
        }
        for beyond in fills {
            let plan = FilePlan {
                size: Some(size),
                sectors: prefix_picks(&f.sectors, f.sectors.len()),
                beyond: Some(beyond),
            };
            if !go(Dim::Size, plan) {
                return false;
            }
        }
    }
    true
}

/// The largest number of slot sectors enumerated as a product; any further non-clean sector of a slot file stays at its
/// newest content in the slot states (a `HEAD` file has two).
const SLOT_SECTORS: usize = 4;

/// The states of a slot file (the two `HEAD` slots, [F15 §6.4] "`HEAD` slots", [F04 §8.2], PLAN WP-32): the product over
/// its non-clean sectors (at most [`SLOT_SECTORS`]) of every version — "old ranges over every version since the slot's
/// durable point" — and the torn mixes: between the oldest and the newest version the mode's (2 in `Prefix`, 14 in
/// `Full`), between every two consecutive versions the two half mixes. The product is built without the states it
/// excludes, so its cost is the number of states it returns.
///
/// **The 9 states.** [F15 §6.4], [F04 §8.2] and PLAN WP-32 name {old, new, torn} × {old, new, torn}: 9 states per
/// barrier. When both slot sectors are `dirty`, (torn, torn) is not a crash state: FM-1.2 and G-5 let at most one dirty
/// sector of a file tear at one crash, and both slots are sectors of the one file `HEAD`. The product therefore keeps at
/// most one torn dirty sector per file and gives 8 states there; (torn, torn) is reached wherever a slot is `poisoned` or
/// `dirty-over-poison` (after a failed `HEAD` flush: FM-3.3, N-12), which the product mixes without bound. This reading
/// — "the 9 states, less (torn, torn) when both slots are dirty (FM-1.2)" — is raised with R-SPEC as a spec finding of
/// WP-32 (the obligation text says 9 unconditionally).
fn slot_states(f: &FileSurface, mode: PlanMode) -> Vec<FilePlan> {
    let secs: Vec<&SectorView> = f.sectors.iter().take(SLOT_SECTORS).collect();
    // Per sector: its versions, and its torn mixes — every mix of the mode between the oldest and the newest version,
    // the two half mixes between every two consecutive versions.
    let versions = |s: &SectorView| -> Vec<SectorPick> {
        (0..s.candidates).map(SectorPick::Version).collect()
    };
    let torn = |s: &SectorView| -> Vec<SectorPick> {
        let last = newest(s);
        let mut t: Vec<SectorPick> = Vec::new();
        if last > 0 {
            t.extend(mixes_of(mode).into_iter().map(|m| mixed(m, 0, last)));
        }
        for v in 0..last {
            if (v, v + 1) != (0, last) {
                t.extend([HALF_NEW, HALF_OLD].into_iter().map(|m| mixed(m, v, v + 1)));
            }
        }
        t
    };
    let base = prefix_picks(&f.sectors, f.sectors.len());
    let mut out = Vec::new();
    // At most one torn dirty sector (FM-1.2): no dirty sector torn, then each one in turn; a poisoned or
    // dirty-over-poison sector takes its torn mixes in every state (FM-3.3).
    let dirty: Vec<usize> = (0..secs.len())
        .filter(|&i| secs[i].state == SectorKind::Dirty)
        .collect();
    for torn_at in std::iter::once(None).chain(dirty.iter().map(|&i| Some(i))) {
        let options: Vec<Vec<SectorPick>> = secs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if torn_at == Some(i) {
                    torn(s)
                } else if s.state == SectorKind::Dirty {
                    versions(s)
                } else {
                    let mut o = versions(s);
                    o.extend(torn(s));
                    o
                }
            })
            .collect();
        if options.iter().any(Vec::is_empty) {
            continue;
        }
        let mut idx = vec![0usize; options.len()];
        'all: loop {
            let mut picks = base.clone();
            for ((i, o), s) in idx.iter().zip(&options).zip(&secs) {
                picks.insert(s.index, o[*i]);
            }
            out.push(keep_size(f, picks));
            for d in 0..idx.len() {
                idx[d] += 1;
                if idx[d] < options[d].len() {
                    continue 'all;
                }
                idx[d] = 0;
            }
            break;
        }
    }
    out
}

/// The namespace corners: the issue-order prefixes, each operation lost alone and each surviving alone.
fn ns_corners(ops: &[OpSurface]) -> Vec<BTreeSet<u64>> {
    let ids: Vec<u64> = ops.iter().map(|o| o.id).collect();
    let n = ids.len();
    let mut out: Vec<BTreeSet<u64>> = Vec::with_capacity(3 * n + 1);
    for j in 0..=n {
        out.push(ids[..j].iter().copied().collect());
    }
    for i in 0..n {
        out.push(
            ids.iter()
                .enumerate()
                .filter(|&(k, _)| k != i)
                .map(|(_, &id)| id)
                .collect(),
        );
        out.push([ids[i]].into_iter().collect());
    }
    out
}

/// The 4 KiB sectors a write in flight touches.
fn sectors_of(w: &WriteSurface) -> u64 {
    if w.len == 0 {
        return 0;
    }
    let first = w.offset / crate::content::SECTOR;
    let last = (w.offset + w.len - 1) / crate::content::SECTOR;
    last - first + 1
}

/// The partial applications of a write in flight.
fn write_alts(w: &WriteSurface, g: &Gen<'_>) -> Vec<PartialWrite> {
    let k = sectors_of(w).min(62);
    let all = if k >= 62 {
        (1u64 << 62) - 1
    } else {
        (1u64 << k) - 1
    };
    let mut alts = vec![
        PartialWrite::Nothing,
        PartialWrite::All,
        PartialWrite::Sectors(1),
    ];
    if k > 1 {
        alts.push(PartialWrite::Sectors(all & !1));
    }
    if g.mode == PlanMode::Full {
        if k <= u64::from(g.limits.exhaustive_max) {
            for m in 0..=all {
                alts.push(PartialWrite::Sectors(m));
            }
        }
        alts.push(PartialWrite::Prefix(w.len / 2));
        let mut s = g.seed ^ w.id.rotate_left(29);
        alts.push(PartialWrite::Mask(splitmix(&mut s) | 2));
    }
    alts
}

/// One random state of every axis: intermediate versions, a torn sector per file half the time, poisoned sub-sector
/// mixes, a random size of H(f) a quarter of the time, a random subset of the pending operations, a random partial write.
fn random_plan(surface: &CrashSurface, rng: &mut Rng) -> CrashPlan {
    let mut p = CrashPlan::newest();
    for f in &surface.files {
        let dirty: Vec<u64> = f.dirty().map(|s| s.index).collect();
        let torn = (!dirty.is_empty() && rng.below(2) == 0)
            .then(|| dirty[rng.below(dirty.len() as u64) as usize]);
        let mut picks = BTreeMap::new();
        for s in &f.sectors {
            let c = s.candidates.max(1);
            let pick = if s.state != SectorKind::Dirty || torn == Some(s.index) {
                let mut subs = [0u64; 8];
                for v in &mut subs {
                    *v = rng.below(c);
                }
                SectorPick::Subsectors(subs)
            } else {
                SectorPick::Version(rng.below(c))
            };
            picks.insert(s.index, pick);
        }
        let size = if f.sizes.len() > 1 && rng.below(4) == 0 {
            f.sizes[rng.below(f.sizes.len() as u64) as usize]
        } else {
            f.size
        };
        let beyond = (size > f.durable_size).then(|| match rng.below(3) {
            0 => BeyondFill::Resolved,
            1 => BeyondFill::Zeros,
            _ => BeyondFill::Garbage(rng.next_u64()),
        });
        p.files.insert(
            f.node,
            FilePlan {
                size: Some(size),
                sectors: picks,
                beyond,
            },
        );
    }
    if !surface.ops.is_empty() {
        p.survivors = Some(
            surface
                .ops
                .iter()
                .filter(|_| rng.below(2) == 1)
                .map(|o| o.id)
                .collect(),
        );
    }
    for w in &surface.writes {
        let pw = if rng.below(2) == 0 {
            PartialWrite::Sectors(rng.next_u64() & ((1u64 << 62) - 1))
        } else {
            PartialWrite::Mask(rng.next_u64() | 2)
        };
        p.in_flight.insert(w.id, pw);
    }
    p
}

/// Whether `plan` mixes the candidates of a poisoned or dirty-over-poison sector within one sector (FM-3.3); `poisoned`
/// lists those sectors of the crash point as (node, sector).
pub(crate) fn mixes_poison(plan: &CrashPlan, poisoned: &BTreeSet<(u64, u64)>) -> bool {
    plan.files.iter().any(|(&node, fp)| {
        fp.sectors.iter().any(|(&s, pick)| {
            matches!(pick, SectorPick::Subsectors(v) if v.iter().any(|&x| x != v[0]))
                && poisoned.contains(&(node, s))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv(index: u64, candidates: u64) -> SectorView {
        SectorView {
            index,
            state: SectorKind::Dirty,
            candidates,
        }
    }

    fn file(node: u64, secs: Vec<SectorView>) -> FileSurface {
        FileSurface {
            node,
            paths: vec![format!("/f{node}")],
            durable_size: 64 * 1024,
            size: 64 * 1024,
            sizes: vec![64 * 1024],
            sectors: secs,
            rewritten: Vec::new(),
        }
    }

    fn surface(files: Vec<FileSurface>) -> CrashSurface {
        CrashSurface {
            point: 1,
            files,
            ops: Vec::new(),
            writes: Vec::new(),
        }
    }

    const LIMITS: PlanLimits = PlanLimits {
        exhaustive_max: 12,
        random_min: 100,
        random_per_point: 4,
        cross_budget: 64,
    };

    fn picks_of(p: &CrashPlan, node: u64) -> Option<&BTreeMap<u64, SectorPick>> {
        p.files.get(&node).map(|f| &f.sectors)
    }

    #[test]
    fn prefix_mode_takes_every_prefix_and_one_torn_boundary_sector() {
        let s = surface(vec![file(9, vec![sv(0, 2), sv(1, 2), sv(2, 3)])]);
        let plans = crash_plans(&s, &BTreeSet::new(), PlanMode::Prefix, &LIMITS, 1);
        // j = 1: sector 0 new, sectors 1 and 2 at baseline.
        assert!(plans.iter().any(|(d, p)| *d == Dim::Prefix
            && picks_of(p, 9).is_some_and(|m| m[&0] == SectorPick::Version(1)
                && m[&1] == SectorPick::Version(0)
                && m[&2] == SectorPick::Version(0))));
        // The boundary sector 1 torn after the prefix of sector 0.
        assert!(plans.iter().any(|(d, p)| *d == Dim::Torn
            && picks_of(p, 9).is_some_and(|m| m[&0] == SectorPick::Version(1)
                && m[&1] == SectorPick::Subsectors([1, 1, 1, 1, 0, 0, 0, 0]))));
        // No state tears two sectors.
        for (_, p) in &plans {
            if let Some(m) = picks_of(p, 9) {
                assert!(
                    m.values()
                        .filter(|v| matches!(v, SectorPick::Subsectors(_)))
                        .count()
                        <= 1
                );
            }
        }
    }

    #[test]
    fn full_mode_enumerates_every_subset_and_later_writes_survive_earlier_losses() {
        let secs: Vec<SectorView> = (0..12).map(|i| sv(i, 2)).collect();
        let s = surface(vec![file(9, secs)]);
        let plans = crash_plans(&s, &BTreeSet::new(), PlanMode::Full, &LIMITS, 1);
        let subsets: BTreeSet<Vec<u64>> = plans
            .iter()
            .filter(|(d, _)| matches!(d, Dim::Subset | Dim::Prefix))
            .filter_map(|(_, p)| picks_of(p, 9))
            .map(|m| {
                m.values()
                    .map(|v| match v {
                        SectorPick::Version(v) => *v,
                        SectorPick::Subsectors(_) => 9,
                    })
                    .collect()
            })
            .collect();
        assert_eq!(
            subsets.len(),
            4096,
            "every subset of 12 dirty sectors (the prefixes among them)"
        );
        // Beyond 12, at least random_min random states.
        let secs: Vec<SectorView> = (0..13).map(|i| sv(i, 3)).collect();
        let s = surface(vec![file(9, secs)]);
        let plans = crash_plans(&s, &BTreeSet::new(), PlanMode::Full, &LIMITS, 1);
        let random = plans.iter().filter(|(d, _)| *d == Dim::Random).count();
        assert!(random >= LIMITS.random_min as usize, "{random}");
        // Intermediate versions are sampled.
        assert!(plans.iter().any(|(_, p)| {
            picks_of(p, 9).is_some_and(|m| m.values().any(|v| *v == SectorPick::Version(1)))
        }));
    }

    #[test]
    fn the_exhaustive_bound_counts_dirty_sectors_only() {
        let mut secs: Vec<SectorView> = (0..12).map(|i| sv(i, 2)).collect();
        secs.push(SectorView {
            index: 12,
            state: SectorKind::Poisoned,
            candidates: 2,
        });
        let plans = crash_plans(
            &surface(vec![file(9, secs)]),
            &BTreeSet::new(),
            PlanMode::Full,
            &LIMITS,
            1,
        );
        let subsets = plans
            .iter()
            .filter(|(d, _)| matches!(d, Dim::Subset | Dim::Prefix))
            .filter_map(|(_, p)| picks_of(p, 9))
            .map(|m| {
                m.iter()
                    .filter(|(i, _)| **i < 12)
                    .map(|(_, v)| *v == SectorPick::Version(1))
                    .collect::<Vec<_>>()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            subsets.len(),
            4096,
            "12 dirty sectors and a poisoned one: every subset of the dirty ones"
        );
        let random = plans.iter().filter(|(d, _)| *d == Dim::Random).count();
        assert_eq!(
            random, LIMITS.random_per_point as usize,
            "not beyond the bound: no 10⁴ random states"
        );
    }

    #[test]
    fn slot_files_take_old_new_and_torn_per_slot_with_one_torn_dirty_slot() {
        let head = file(5, vec![sv(0, 2), sv(1, 2)]);
        let slots: BTreeSet<u64> = [5].into_iter().collect();
        let plans = crash_plans(&surface(vec![head]), &slots, PlanMode::Prefix, &LIMITS, 1);
        let kind = |p: &SectorPick| match p {
            SectorPick::Version(0) => "old",
            SectorPick::Version(_) => "new",
            SectorPick::Subsectors(_) => "torn",
        };
        let states: BTreeSet<(&str, &str)> = plans
            .iter()
            .filter(|(d, _)| *d == Dim::Slot)
            .filter_map(|(_, p)| picks_of(p, 5))
            .map(|m| (kind(&m[&0]), kind(&m[&1])))
            .collect();
        // {old, new, torn}² less (torn, torn): two dirty sectors of one file never both tear (FM-1.2).
        assert_eq!(states.len(), 8, "{states:?}");
        assert!(!states.contains(&("torn", "torn")));
        // A poisoned slot may be mixed while the other tears (FM-3.3): the ninth state.
        let mut poisoned = sv(1, 2);
        poisoned.state = SectorKind::Poisoned;
        let head = file(5, vec![sv(0, 2), poisoned]);
        let plans = crash_plans(&surface(vec![head]), &slots, PlanMode::Prefix, &LIMITS, 1);
        assert!(plans.iter().any(|(_, p)| {
            picks_of(p, 5).is_some_and(|m| kind(&m[&0]) == "torn" && kind(&m[&1]) == "torn")
        }));
    }

    #[test]
    fn slot_states_tear_between_consecutive_versions_too() {
        // Three publishes into slot 0 since its durable point: versions 0..=3.
        let head = file(5, vec![sv(0, 4)]);
        let slots: BTreeSet<u64> = [5].into_iter().collect();
        let plans = crash_plans(&surface(vec![head]), &slots, PlanMode::Prefix, &LIMITS, 1);
        let torn: BTreeSet<(u64, u64)> = plans
            .iter()
            .filter(|(d, _)| *d == Dim::Slot)
            .filter_map(|(_, p)| picks_of(p, 5))
            .filter_map(|m| match m[&0] {
                SectorPick::Subsectors(s) => {
                    Some((*s.iter().min().expect("8"), *s.iter().max().expect("8")))
                }
                SectorPick::Version(_) => None,
            })
            .collect();
        let want: BTreeSet<(u64, u64)> = [(0, 1), (1, 2), (2, 3), (0, 3)].into_iter().collect();
        assert_eq!(torn, want);
        // Every version is an "old" or "new" state.
        let versions = plans
            .iter()
            .filter_map(|(_, p)| picks_of(p, 5))
            .filter_map(|m| match m[&0] {
                SectorPick::Version(v) => Some(v),
                SectorPick::Subsectors(_) => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(versions.len(), 4);
    }

    #[test]
    fn an_extended_file_keeps_its_size_with_garbage_beyond_the_durable_size() {
        let mut f = file(3, vec![sv(0, 2), sv(1, 2)]);
        f.durable_size = 4096;
        f.size = 2 * 4096;
        f.sizes = vec![4096, 2 * 4096];
        for (mode, want) in [(PlanMode::Prefix, 1), (PlanMode::Full, 2)] {
            let plans = crash_plans(
                &surface(vec![f.clone()]),
                &BTreeSet::new(),
                mode,
                &LIMITS,
                1,
            );
            let at_cs: Vec<Option<BeyondFill>> = plans
                .iter()
                .filter(|(d, p)| *d == Dim::Size && p.resolve == Resolve::Newest)
                .filter_map(|(_, p)| p.files.get(&3))
                .filter(|fp| fp.size == Some(2 * 4096))
                .map(|fp| fp.beyond)
                .collect();
            assert_eq!(at_cs.len(), want, "{mode:?}: {at_cs:?}");
            assert!(
                at_cs
                    .iter()
                    .any(|b| matches!(b, Some(BeyondFill::Garbage(_))))
            );
        }
    }

    #[test]
    fn cross_products_are_bounded() {
        let files: Vec<FileSurface> = (1..=4)
            .map(|n| file(n, (0..5).map(|i| sv(i, 2)).collect()))
            .collect();
        let plans = crash_plans(
            &surface(files),
            &BTreeSet::new(),
            PlanMode::Prefix,
            &LIMITS,
            3,
        );
        let cross = plans.iter().filter(|(d, _)| *d == Dim::Cross).count();
        assert!(
            cross > 0 && cross <= LIMITS.cross_budget as usize,
            "{cross}"
        );
        // Every cross plan names every file.
        for (_, p) in plans.iter().filter(|(d, _)| *d == Dim::Cross) {
            assert_eq!(p.files.len(), 4);
        }
        // Two small files: the whole product (6 × 6 prefixes).
        let files: Vec<FileSurface> = (1..=2)
            .map(|n| file(n, (0..5).map(|i| sv(i, 2)).collect()))
            .collect();
        let plans = crash_plans(
            &surface(files),
            &BTreeSet::new(),
            PlanMode::Prefix,
            &LIMITS,
            3,
        );
        assert_eq!(plans.iter().filter(|(d, _)| *d == Dim::Cross).count(), 36);
    }

    #[test]
    fn plans_come_one_at_a_time_without_duplicates_and_stop_on_request() {
        let s = surface(vec![file(9, (0..6).map(|i| sv(i, 3)).collect())]);
        let all = crash_plans(&s, &BTreeSet::new(), PlanMode::Full, &LIMITS, 5);
        let distinct: HashSet<u128> = all.iter().map(|(_, p)| fingerprint(p)).collect();
        assert_eq!(distinct.len(), all.len(), "no plan twice");
        for i in 0..all.len() {
            for j in 0..i.min(40) {
                assert_ne!(all[i].1, all[j].1);
            }
        }
        let mut taken = 0;
        for_each_plan(
            &s,
            &BTreeSet::new(),
            PlanMode::Full,
            &LIMITS,
            5,
            &mut |_, _| {
                taken += 1;
                taken < 7
            },
        );
        assert_eq!(taken, 7, "the generator stops when asked");
        // The same seed gives the same sequence.
        assert_eq!(
            all,
            crash_plans(&s, &BTreeSet::new(), PlanMode::Full, &LIMITS, 5)
        );
    }

    #[test]
    fn poisoned_mixes_are_recognised() {
        let poisoned: BTreeSet<(u64, u64)> = [(9, 2)].into_iter().collect();
        let mut fp = FilePlan::default();
        fp.sectors
            .insert(2, SectorPick::Subsectors([0, 1, 0, 1, 0, 1, 0, 1]));
        let p = CrashPlan::newest().with_file(9, fp.clone());
        assert!(mixes_poison(&p, &poisoned));
        fp.sectors.insert(2, SectorPick::Subsectors([1; 8]));
        assert!(!mixes_poison(
            &CrashPlan::newest().with_file(9, fp),
            &poisoned
        ));
    }

    #[test]
    fn mixes_are_the_fourteen_prefix_and_suffix_tears() {
        let m = all_mixes();
        assert_eq!(m.len(), 14);
        assert!(m.contains(&0b0000_0001) && m.contains(&0b1111_1110) && m.contains(&0b0111_1111));
        let set: BTreeSet<u8> = m.iter().copied().collect();
        assert_eq!(set.len(), 14);
        assert!(!set.contains(&0) && !set.contains(&0xFF));
    }
}
