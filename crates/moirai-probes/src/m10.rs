//! Measurement 10 (WP-53d, [60 §5.2] item 10, `60-roadmap.md:766`): full-tail replay at the candidate checkpoint
//! thresholds, decoding every record and applying it into the F09/F11-shaped overlay, with the spec-derived lazy share
//! of [70 S8]; and the open floor it is gated against ("`HEAD` pread + map of 8 files", `60-roadmap.md:790`). It
//! decides `F17-ckpt-ops`, `-ckpt-bytes`, `-ckpt-body` (the open side), `-tail-overlay`, `-tail-overlay-quiet` and
//! `-quiet-mult` (`docs/spec/HOLES.md:41-46`; their constraints `17-store-parameters.md:757-762`).
//!
//! **The grid** (49 quantities, names by [MP §4.1]). The night's 18 are F17's candidates: `ops` at P06 4,096, 2,048 and
//! 1,024 with N = 1e5 and 1e6; `rec` at P07 4, 2 and 1 MiB; `body` at P08 32, 16 and 8 MiB; `ovl` at P09 1 MiB and
//! 512 KiB; `quiet` at P10 2 MiB and 1.5 MiB × P11 8 and 4; the last four families at N = 1e6 (cut D-3: open at 1e5 is
//! replay(cap) + floor(1e5), the floor measured by the `ops` family at both scales). The follow-up block of 13 (u17; R-40
//! O-3, R-42 Q1 and Q3) lies below the night's candidates, chosen from its replay rates:
//! - `ops` at P06 2,048 and 1,024 at N = 1e4, F17's unmeasured 1e4 clause (`17-store-parameters.md:757-762`);
//! - `rec` at P07 512 and 256 KiB, `body` at P08 1 MiB and 512 KiB, `ovl` at P09 64 and 96 KiB, all at N = 1e5, the scale
//!   of the 1.5-ms open budget, so their open is one run's replay + floor. Two of them measure a tail of another size
//!   than their cap (R-44 I-2; the production tails, seeded by their names), and both are kept and read as they are:
//!   - `rec-256kib.n1e5` is one commit of 765,160 B of `rec_bytes` (3,390 ops), larger than `rec-512kib.n1e5`'s
//!     523,685 B (69 commits). A tail always holds its first commit, so a commit larger than its cap crosses it, as the
//!     product's checkpoint trigger also lets the crossing record through (`17-store-parameters.md:354`);
//!   - `ovl-96kib.n1e5` stops at an overlay of 27,660 B (15 commits), below `ovl-64kib.n1e5`'s 65,120 B, on the stop
//!     rule's other clause: a tail ends before the first group that would take its measure past the cap, and its 16th
//!     commit, of 3,265 ops, would take the overlay to 99,992 B;
//! - `quiet` at P10 2 MiB with P06 2,048 (P11 4, 8 and 2) and 1,024 (P11 4 and 8): the night's quiet tails all ended on
//!   P06 = 4,096's ops clause (D1). A quiet name with a P06 is `quiet-<P10>-x<P11>-p<P06>`, and that P06 is the target's
//!   `params.p06` at production. P11 = 2 is in P11's range of 1–64 (`17-store-parameters.md:176`) but not one of F17's
//!   candidates (R-42 Q3).
//!
//! u20's block of 18 follows, appended (u20 cut §3.5, §3.10; spec Goal 5): design-A A3's small tails in item 1a's A1 form
//! ([`a3_points`]: `rec` at P07 128 and 256 KiB, `ovl` at P09 48 and 64 KiB and at the quiet cap P10 192 KiB, the
//! CLI's 2× tail, and quiet at P10 192 KiB × P11 2 and 4), every other threshold at A3's drafts ([`Over::A3`]), the 1e4
//! points first. Each production tail reaches its own cap: its seed is `seed(name) ^ k` with the first k < 64 whose tail
//! stops on the clause it targets at 95–100 % of the cap ([`reaches_cap`], [`SEED_K`]; R-44 I-2's tails past or far
//! below their caps are what this excludes). `--list` prints each k.
//!
//! Every run is measurement 10 with minimum tier t2, the tier of the open budgets ([MP §3.2],
//! `17-store-parameters.md:757`). The test profile's thresholds and E give the smoke's versions.
//!
//! **The run order** (R-34 B-3, M-15; R-35 Q2, Q4): one point of each family first, F17's first candidate at 1e6, the
//! scale of the binding open budget (`17-store-parameters.md:756-761`): `ops-4096`, `rec-4mib`, `body-32mib`, `ovl-1mib`,
//! `quiet-2mib-x8`; then the other 13 in family order. A short window then still holds one point of every family. The
//! follow-up block comes after the night's 18 by the same rule: `ops-2048.n1e4`, `quiet-2mib-x4-p2048.n1e6`,
//! `rec-512kib.n1e5`, `body-1mib.n1e5`, `ovl-64kib.n1e5` first, then the rest, and `quiet-2mib-x2-p2048.n1e6` closes it.
//!
//! **The arms** ([MP §4.1]): arm 0 `replay` opens the tail's extents and reads `[checkpoint_lsn, committed_lsn)` through
//! the fixed 64-KiB buffer, scans, validates, decodes, applies and finishes the overlay; arm 1 `floor.open` is
//! [`crate::tail::store::open_floor`]. Open at a grid point = floor + replay (`60-roadmap.md:790`). The A1 points'
//! arm 0 is `replay.at` ([`replay_at_arm`]; design-A's A2, M-A2): the same replay with each node's offset and length in
//! its newest record built inside the timed region ([`crate::overlay::Overlay::replay_at`]), so `replay` is its floor,
//! `replay.at − replay` the build's cost, and their open is `replay.at` + `floor.open`. Both replays drop their overlay
//! inside the timed region (u20 addendum m-5).
//!
//! **The driver** ([MP §9.7] as merged, through the shared [`crate::m1::driver`]): `--host laptop|hosted`,
//! `--condition idle|loaded|synthetic`, `--load-log <file>` (a loaded or synthetic run's condition is the log header's,
//! and a loaded run's replay verdict is read over its window), `--quantity <name>` (one run, in this process),
//! `--commit <sha>` and `--toolchain <text>` (required), `--out <dir>` (required unless `--smoke`, refused with it; each
//! record one JSON file directly there, created when missing, streamed, removed on a failed write, [MP §7.2]),
//! `--smoke` (the requested quantity, or every one, at the test profile, each arm once, nothing recorded), `--help`;
//! and, beside the contract, `m10 --list` alone (one JSON line of tail measures per quantity, item 1a's locator bytes and
//! the seed's k among them). Every fixture lies below
//! the working directory ([MP §9.7] "The working directory"; R-34 B-1). Without `--quantity`, each quantity runs in a
//! child process of its own, in grid order ([MP §4.6]: `private_peak` cannot be reset). Exit 0 done, 1 a run or arm
//! failed or a record could not be written, 2 a usage error.

use crate::arm::{Arm, ArmError, Ticker, TimedFn};
use crate::host::{CommandRunner, HostKind};
use crate::m1::driver::{self, Args};
use crate::run::{Context, RunSpec, measure};
use crate::tail::store::{
    Fixture, HOT_BYTES_PER_NODE, SEALED_FILES, VfsLog, open_floor, remove_fixture, write_fixture,
};
use crate::tail::synth::{Clause, Tail, Target, generate};
use crate::tail::{Locator, Params, TAIL_READ_CHUNK};
use crate::tier::Tier;
use moirai_vfs::{Meter, RootAccess, RootRole, SealedMaps, StoreFs};
use serde_json::json;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The measurement's row number.
pub const MEASUREMENT: u32 = 10;
pub use driver::{EXIT_DONE, EXIT_FAILED, EXIT_USAGE};
/// The sealed files' length at the test profile.
pub const TEST_SEALED_LEN: u64 = 4_096;

const KIB: u64 = 1_024;
const MIB: u64 = 1_024 * 1_024;

/// What a quantity measures (u20 cut §3.5).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A tail's open: the replay arms and the open floor.
    Tail,
}

/// A quantity's production parameters beyond the column's design figures, P06–P11 (u20 cut §3.5); `None` keeps the
/// production column's figure. The test profile keeps its own column.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Over {
    /// P06 `store.checkpoint.ops`.
    pub p06: Option<u64>,
    /// P07 `store.checkpoint.bytes`.
    pub p07: Option<u64>,
    /// P08 `store.checkpoint.body-bytes`.
    pub p08: Option<u64>,
    /// P09 `store.tail.max-overlay-bytes`.
    pub p09: Option<u64>,
    /// P10 `store.tail.max-overlay-bytes.quiet`.
    pub p10: Option<u64>,
    /// P11 `quiet.tail-cap-multiplier`.
    pub p11: Option<u64>,
}

impl Over {
    /// Design-A A3's drafts (`design-A.md:149-156`): P06 1,024, P07 128 KiB, P08 1 MiB, P09 64 KiB, P10 192 KiB, P11 4.
    pub const A3: Over = Over {
        p06: Some(1_024),
        p07: Some(128 * KIB),
        p08: Some(MIB),
        p09: Some(64 * KIB),
        p10: Some(192 * KIB),
        p11: Some(4),
    };

    /// `p` with these figures in place.
    pub fn apply(self, mut p: Params) -> Params {
        p.p06 = self.p06.unwrap_or(p.p06);
        p.p07 = self.p07.unwrap_or(p.p07);
        p.p08 = self.p08.unwrap_or(p.p08);
        p.p09 = self.p09.unwrap_or(p.p09);
        p.p10 = self.p10.unwrap_or(p.p10);
        p.p11 = self.p11.unwrap_or(p.p11);
        p
    }
}

/// One grid point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quantity {
    /// The quantity's name.
    pub name: String,
    /// What it measures.
    pub kind: Kind,
    /// The family.
    pub clause: Clause,
    /// The candidate threshold: P06, P07, P08, P09 or P10.
    pub threshold: u64,
    /// P11 for `quiet`, else 1.
    pub mult: u64,
    /// The base's node count.
    pub n: u32,
    /// The production parameters beyond the column's: the P06 of a quiet follow-up point, design-A A3's drafts of the A1
    /// points.
    pub over: Over,
    /// The form of the tail's op frames: the night's, or A1 for u20's points.
    pub form: Locator,
    /// The production seed's k: the seed is `seed(name) ^ k`, the first k < 64 whose tail reaches its cap (u20 cut
    /// D-5); 0 for the night's points and at the test profile.
    pub seed_k: u64,
}

fn bytes_name(b: u64) -> String {
    if b.is_multiple_of(MIB) {
        format!("{}mib", b / MIB)
    } else {
        format!("{}kib", b / KIB)
    }
}

/// The scale in a quantity name.
fn scale(n: u32) -> &'static str {
    match n {
        10_000 => "n1e4",
        100_000 => "n1e5",
        _ => "n1e6",
    }
}

/// The grid in run order: the night's 18 (`17-store-parameters.md:757-762`), one point of each family first, F17's first
/// candidate at 1e6, then the rest in family order (R-34 B-3, M-15); then the follow-up block of 13 by the same rule
/// (R-40 O-3, R-42 Q1 and Q3); then u20's blocks, appended (u20 cut §3.10): design-A A3's small tails in A1 (§3.5).
pub fn grid() -> Vec<Quantity> {
    let q = |clause: Clause, threshold: u64, mult: u64, n: u32, p06: Option<u64>| {
        let t = match (clause, p06) {
            (Clause::Ops, _) => threshold.to_string(),
            (Clause::Quiet, None) => format!("{}-x{mult}", bytes_name(threshold)),
            (Clause::Quiet, Some(p)) => format!("{}-x{mult}-p{p}", bytes_name(threshold)),
            _ => bytes_name(threshold),
        };
        Quantity {
            name: format!("{}-{t}.{}", clause.as_str(), scale(n)),
            kind: Kind::Tail,
            clause,
            threshold,
            mult,
            n,
            over: Over {
                p06,
                ..Over::default()
            },
            form: Locator::None,
            seed_k: 0,
        }
    };
    let mut g = Vec::new();
    for n in [100_000, 1_000_000] {
        for ops in [4_096, 2_048, 1_024] {
            g.push(q(Clause::Ops, ops, 1, n, None));
        }
    }
    for b in [4 * MIB, 2 * MIB, MIB] {
        g.push(q(Clause::Rec, b, 1, 1_000_000, None));
    }
    for b in [32 * MIB, 16 * MIB, 8 * MIB] {
        g.push(q(Clause::Body, b, 1, 1_000_000, None));
    }
    for b in [MIB, 512 * KIB] {
        g.push(q(Clause::Ovl, b, 1, 1_000_000, None));
    }
    for p10 in [2 * MIB, 1_536 * KIB] {
        for m in [8, 4] {
            g.push(q(Clause::Quiet, p10, m, 1_000_000, None));
        }
    }
    let first = |q: &Quantity| {
        [
            "ops-4096.n1e6",
            "rec-4mib.n1e6",
            "body-32mib.n1e6",
            "ovl-1mib.n1e6",
            "quiet-2mib-x8.n1e6",
        ]
        .iter()
        .position(|n| *n == q.name)
    };
    let (mut lead, rest): (Vec<Quantity>, Vec<Quantity>) =
        g.into_iter().partition(|q| first(q).is_some());
    lead.sort_by_key(|q| first(q));
    lead.extend(rest);
    let (e4, e5, e6) = (10_000, 100_000, 1_000_000);
    lead.extend([
        q(Clause::Ops, 2_048, 1, e4, None),
        q(Clause::Quiet, 2 * MIB, 4, e6, Some(2_048)),
        q(Clause::Rec, 512 * KIB, 1, e5, None),
        q(Clause::Body, MIB, 1, e5, None),
        q(Clause::Ovl, 64 * KIB, 1, e5, None),
        q(Clause::Ops, 1_024, 1, e4, None),
        q(Clause::Quiet, 2 * MIB, 8, e6, Some(2_048)),
        q(Clause::Quiet, 2 * MIB, 4, e6, Some(1_024)),
        q(Clause::Quiet, 2 * MIB, 8, e6, Some(1_024)),
        q(Clause::Rec, 256 * KIB, 1, e5, None),
        q(Clause::Body, 512 * KIB, 1, e5, None),
        q(Clause::Ovl, 96 * KIB, 1, e5, None),
        q(Clause::Quiet, 2 * MIB, 2, e6, Some(2_048)),
    ]);
    lead.extend(a3_points());
    lead
}

/// Design-A A3's small tails in A1 (u20 cut §3.5, spec Goal 5): 18 points with arms `replay.at`, `replay` and
/// `floor.open`, every other threshold at A3's drafts ([`Over::A3`]). The 1e4 points first (R-50 rules the 1e4 open clause
/// on them), then the 1e5 and the 1e6 points, each in the table's order, and the two quiet points last:
///
/// | Name | Clause and cap | Derived from |
/// |---|---|---|
/// | `a1-rec-128kib` | `rec`, P07 = 128 KiB | `design-A.md:152` |
/// | `a1-rec-256kib` | `rec`, 256 KiB | spec Goal 5 |
/// | `a1-ovl-48kib` | `ovl`, P09 = 48 KiB | `report2.md:371`, about 54 KiB for 1 ms (est.) |
/// | `a1-ovl-64kib` | `ovl`, P09 = 64 KiB | `design-A.md:154` |
/// | `a1-ovl-192kib.n1e6` | `ovl`, 192 KiB, the quiet cap P10 | `design-A.md:155`; F17:761 |
/// | `a1-cli2x-128kib` | the quiet machinery with q = P13 = 2 and P10 := 2 × P09 = 128 KiB: 2,048 ops, 256 KiB of records, 2 MiB of bodies, 128 KiB of overlay, whichever binds first | `design-A.md:152` |
/// | `a1-quiet-192kib-x2-p1024.n1e6`, `-x4-` | quiet at P10 192 KiB, P11 2 and 4, ending on their record caps of 256 and 512 KiB | spec Goal 5; critique-fit-cost A-1 |
///
/// Each production seed is `seed(name) ^ seed_k`, the first k < 64 whose tail reaches its cap (D-5).
pub fn a3_points() -> Vec<Quantity> {
    let a1 =
        |clause: Clause, threshold: u64, mult: u64, n: u32, over: Over, stem: String| Quantity {
            name: format!("a1-{stem}.{}", scale(n)),
            kind: Kind::Tail,
            clause,
            threshold,
            mult,
            n,
            over,
            form: Locator::A1,
            seed_k: 0,
        };
    let mut g = Vec::new();
    for n in [10_000, 100_000, 1_000_000] {
        g.push(a1(
            Clause::Rec,
            128 * KIB,
            1,
            n,
            Over::A3,
            "rec-128kib".into(),
        ));
        g.push(a1(
            Clause::Rec,
            256 * KIB,
            1,
            n,
            Over::A3,
            "rec-256kib".into(),
        ));
        g.push(a1(
            Clause::Ovl,
            48 * KIB,
            1,
            n,
            Over::A3,
            "ovl-48kib".into(),
        ));
        g.push(a1(
            Clause::Ovl,
            64 * KIB,
            1,
            n,
            Over::A3,
            "ovl-64kib".into(),
        ));
        if n == 1_000_000 {
            g.push(a1(
                Clause::Ovl,
                192 * KIB,
                1,
                n,
                Over::A3,
                "ovl-192kib".into(),
            ));
        }
        let cli = Over {
            p10: Some(128 * KIB),
            p11: Some(2),
            ..Over::A3
        };
        g.push(a1(
            Clause::Quiet,
            128 * KIB,
            2,
            n,
            cli,
            "cli2x-128kib".into(),
        ));
    }
    for x in [2, 4] {
        let quiet = Over {
            p11: Some(x),
            ..Over::A3
        };
        g.push(a1(
            Clause::Quiet,
            192 * KIB,
            x,
            1_000_000,
            quiet,
            format!("quiet-192kib-x{x}-p1024"),
        ));
    }
    let k = |name: &str| -> u64 {
        SEED_K
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(0, |(_, k)| *k)
    };
    for q in &mut g {
        q.seed_k = k(&q.name);
    }
    g
}

/// The production k of each A1 point, the first k < 64 whose tail reaches its cap ([`reaches_cap`]; u20 cut D-5),
/// recomputed by `a1_tails_reach_their_caps_and_stop_on_their_clauses`. A 128-KiB `rec` tail of the mix reaches 95 %
/// of its cap for few seeds: one commit in ten is log-uniform up to P05 = 1 MiB, so the refused group is often large.
pub const SEED_K: [(&str, u64); 18] = [
    ("a1-rec-128kib.n1e4", 34),
    ("a1-rec-256kib.n1e4", 11),
    ("a1-ovl-48kib.n1e4", 1),
    ("a1-ovl-64kib.n1e4", 0),
    ("a1-cli2x-128kib.n1e4", 8),
    ("a1-rec-128kib.n1e5", 4),
    ("a1-rec-256kib.n1e5", 9),
    ("a1-ovl-48kib.n1e5", 1),
    ("a1-ovl-64kib.n1e5", 0),
    ("a1-cli2x-128kib.n1e5", 4),
    ("a1-rec-128kib.n1e6", 16),
    ("a1-rec-256kib.n1e6", 1),
    ("a1-ovl-48kib.n1e6", 1),
    ("a1-ovl-64kib.n1e6", 4),
    ("a1-ovl-192kib.n1e6", 0),
    ("a1-cli2x-128kib.n1e6", 14),
    ("a1-quiet-192kib-x2-p1024.n1e6", 18),
    ("a1-quiet-192kib-x4-p1024.n1e6", 6),
];

/// The scale a run is made at.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Profile {
    /// The production column and the grid's candidates.
    Production,
    /// The test profile's thresholds and E (`17-store-parameters.md:166-178`), the smoke's.
    Test,
}

impl Profile {
    /// The parameters.
    pub fn params(self) -> Params {
        match self {
            Profile::Production => Params::PRODUCTION,
            Profile::Test => Params::TEST,
        }
    }

    /// The length of each sealed file: 99·N/8 at production (`ARCHITECTURE-RESEARCH.md:1681`), 4,096 B in the test
    /// profile.
    pub fn sealed_len(self, n: u32) -> u64 {
        match self {
            Profile::Production => HOT_BYTES_PER_NODE * u64::from(n) / SEALED_FILES as u64,
            Profile::Test => TEST_SEALED_LEN,
        }
    }
}

/// The seed of a quantity: a function of its name, so every run of it replays one tail.
fn seed(name: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(name.as_bytes())
}

/// The generator's target of `q` at `profile`: at production with the point's parameters (a quiet follow-up point's
/// P06, the A1 points' A3 drafts) and its seed's k; in the point's form at both profiles.
pub fn target(q: &Quantity, profile: Profile) -> Target {
    let mut p = profile.params();
    if profile == Profile::Production {
        p = q.over.apply(p);
    }
    p.shape.locator = q.form;
    let k = match profile {
        Profile::Production => q.seed_k,
        Profile::Test => 0,
    };
    let (threshold, mult) = match profile {
        Profile::Production => (q.threshold, q.mult),
        Profile::Test => match q.clause {
            Clause::Ops => (p.p06, 1),
            Clause::Rec => (p.p07, 1),
            Clause::Body => (p.p08, 1),
            Clause::Ovl => (p.p09, 1),
            Clause::Quiet => (p.p10, p.p11),
        },
    };
    Target {
        clause: q.clause,
        threshold,
        mult,
        n: q.n,
        seed: seed(&q.name) ^ k,
        params: p,
        rt_bound: None,
    }
}

/// Whether `t`'s tail reaches the cap it targets (u20 cut D-5; addendum m-4): it stops on that clause, the refused group
/// passes the cap, and the measure is at least 95 % of it and at most the cap (a first commit past the cap, which a tail
/// always holds, is not a tail at its cap: R-44 I-2). A `rec` or `ovl` point targets its threshold; the quiet
/// points their record cap P11 × P07; `a1-cli2x-*` the binding quiet cap, whichever it is. Returns the clause and the
/// measure's share of the cap in per mille.
pub fn reaches_cap(q: &Quantity, t: &Target, tail: &Tail) -> (bool, &'static str, u64) {
    let p = t.params;
    let stop = binding_clause(t, tail);
    let (clause, cap) = match (q.clause, q.name.contains("cli2x")) {
        (Clause::Quiet, true) => (
            stop,
            match stop {
                "ops" => t.mult * p.p06,
                "rec" => t.mult * p.p07,
                "body" => t.mult * p.p08,
                _ => t.threshold,
            },
        ),
        (Clause::Quiet, false) => ("rec", t.mult * p.p07),
        (c, _) => (c.as_str(), t.threshold),
    };
    let of = |m: &crate::tail::synth::Measures| match clause {
        "ops" => m.ops,
        "rec" => m.rec_bytes,
        "body" => m.body_bytes,
        _ => m.ovl,
    };
    let m = of(&tail.measures);
    let past = tail.refused.is_some_and(|r| of(&r) > cap);
    let share = m * 1_000 / cap.max(1);
    (
        stop == clause && past && (950..=1_000).contains(&share) && m <= cap,
        clause,
        share,
    )
}

/// The clause that ended a tail: the family's own, or for `quiet` the first cap the refused group passed.
pub fn binding_clause(t: &Target, tail: &Tail) -> &'static str {
    let p = t.params;
    match (t.clause, tail.refused) {
        (Clause::Quiet, Some(r)) => {
            if r.ops > t.mult * p.p06 {
                "ops"
            } else if r.rec_bytes > t.mult * p.p07 {
                "rec"
            } else if r.body_bytes > t.mult * p.p08 {
                "body"
            } else {
                "ovl"
            }
        }
        (c, _) => c.as_str(),
    }
}

/// The two arms of a run over a written fixture: `replay` and `floor.open`.
pub fn arms<'a, V: StoreFs + SealedMaps, T: Ticker + Clone + 'a>(
    vfs: &'a V,
    root: &'a V::Root,
    fx: &'a Fixture,
    tail: &'a Tail,
    ticker: &T,
) -> (impl Arm + 'a, impl Arm + 'a) {
    let replay = TimedFn::new("replay", ticker.clone(), move || {
        let mut log = VfsLog::new(vfs, root, fx.expect.geo);
        crate::overlay::Overlay::replay(
            &mut log,
            &fx.expect,
            tail.start,
            tail.end,
            tail.chain_in,
            tail.base,
            TAIL_READ_CHUNK,
        )
        .map(|o| o.entries())
        .map_err(|e| ArmError(e.to_string()))
    });
    let floor = TimedFn::new("floor.open", ticker.clone(), move || {
        open_floor(vfs, root, &fx.sealed).map_err(ArmError)
    });
    (replay, floor)
}

/// The entries of an overlay built in the offsets mode; refuses one without an offset for every node (u20 cut D-2,
/// R-29), so a `replay.at` whose op left the offset build out of its timed region fails its sample.
pub fn offsets_of(o: &crate::overlay::Overlay) -> Result<usize, ArmError> {
    if o.at().len() != o.nodes().len() {
        return Err(ArmError(format!(
            "an overlay without offsets: {} of {} nodes",
            o.at().len(),
            o.nodes().len()
        )));
    }
    Ok(o.entries())
}

/// Arm 0 of the A1 points, `replay.at` (u20 cut D-2, M-A2): `replay`'s work in the offsets mode, each node's offset and
/// length in its newest record built inside the timed region. Its op refuses an overlay without them ([`offsets_of`])
/// and drops the overlay inside the timed region, as the floor `replay` does (addendum m-5). `replay.at − replay` is
/// the build's cost. An offset table computed at the arm's construction and attached to a plain replay would pass every
/// test: the build is in-memory work, which the tests see only through the refusal.
pub fn replay_at_arm<'a, V: StoreFs + SealedMaps, T: Ticker + Clone + 'a>(
    vfs: &'a V,
    root: &'a V::Root,
    fx: &'a Fixture,
    tail: &'a Tail,
    ticker: &T,
) -> impl Arm + 'a {
    TimedFn::new("replay.at", ticker.clone(), move || {
        let mut log = VfsLog::new(vfs, root, fx.expect.geo);
        let o = crate::overlay::Overlay::replay_at(
            &mut log,
            &fx.expect,
            tail.start,
            tail.end,
            tail.chain_in,
            tail.base,
            TAIL_READ_CHUNK,
        )
        .map_err(|e| ArmError(e.to_string()))?;
        offsets_of(&o)
    })
}

/// The arms of `q` over a written fixture, arm 0 first: `[replay.at, replay, floor.open]` for an A1 point (u20 cut §3.5),
/// the night's `[replay, floor.open]` for every other.
pub fn quantity_arms<'a, V: StoreFs + SealedMaps, T: Ticker + Clone + 'a>(
    q: &Quantity,
    vfs: &'a V,
    root: &'a V::Root,
    fx: &'a Fixture,
    tail: &'a Tail,
    ticker: &T,
) -> Vec<Box<dyn Arm + 'a>> {
    let (replay, floor) = arms(vfs, root, fx, tail, ticker);
    let mut v: Vec<Box<dyn Arm + 'a>> = Vec::new();
    if q.form == Locator::A1 {
        v.push(Box::new(replay_at_arm(vfs, root, fx, tail, ticker)));
    }
    v.push(Box::new(replay));
    v.push(Box::new(floor));
    v
}

/// Removes its fixture when dropped, on every path out of a run, an unwinding panic included.
struct FixtureGuard<'v, V: StoreFs> {
    vfs: &'v V,
    parent: PathBuf,
    fx: Fixture,
}

impl<V: StoreFs> Drop for FixtureGuard<'_, V> {
    fn drop(&mut self) {
        let _ = remove_fixture(self.vfs, &self.parent, &self.fx);
    }
}

/// What the binary passes in: the seams and the process spawner.
pub struct System<'a, V, M, T, R> {
    /// The store seam.
    pub vfs: &'a V,
    /// The measurement seam.
    pub meter: &'a M,
    /// The timer.
    pub ticker: T,
    /// The host-snapshot command runner.
    pub runner: R,
    /// Runs this driver again with the given arguments and returns its exit code.
    pub spawn: &'a mut dyn FnMut(&[OsString]) -> Result<u8, String>,
    /// The scale.
    pub profile: Profile,
}

/// The usage text.
pub const USAGE: &str =
    "usage: m10 --host laptop|hosted --condition idle|loaded|synthetic [--load-log <file>]
           [--quantity <name>] --commit <sha> --toolchain <text> --out <dir>
       m10 --smoke [--quantity <name>] --commit <sha> --toolchain <text>
       m10 --list
       m10 --help
Every fixture lies below the working directory.
";

/// The fixtures' parent: the working directory ([MP §9.7] "The working directory"; R-34 B-1).
fn work_dir() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("the working directory: {e}"))
}

/// Generates `q`'s tail at `profile` and writes its fixture under `scratch`.
fn fixture<'v, V: StoreFs>(
    vfs: &'v V,
    scratch: &Path,
    q: &Quantity,
    profile: Profile,
) -> Result<(Tail, FixtureGuard<'v, V>), String> {
    let tail = generate(&target(q, profile), false)?;
    let name = format!("moirai-m10-{}-{}", std::process::id(), q.name);
    let fx = write_fixture(vfs, scratch, &name, &tail, profile.sealed_len(q.n))?;
    Ok((
        tail,
        FixtureGuard {
            vfs,
            parent: scratch.to_path_buf(),
            fx,
        },
    ))
}

fn smoke<V: StoreFs + SealedMaps, T: Ticker + Clone>(
    vfs: &V,
    ticker: &T,
    scratch: &Path,
    qs: &[Quantity],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut code = EXIT_DONE;
    for q in qs {
        let run = || -> Result<Vec<(String, u64)>, String> {
            let (tail, guard) = fixture(vfs, scratch, q, Profile::Test)?;
            let root = vfs
                .open_root(&guard.fx.dir, RootRole::Other, RootAccess::Read)
                .map_err(|e| e.to_string())?;
            let mut arms = quantity_arms(q, vfs, &root, &guard.fx, &tail, ticker);
            let mut lines = Vec::new();
            for arm in arms.iter_mut() {
                arm.begin_block(1)
                    .and_then(|()| arm.sample())
                    .map(|ns| lines.push((arm.name().to_string(), ns)))
                    .map_err(|e| format!("{}: {e}", arm.name()))?;
            }
            Ok(lines)
        };
        match run() {
            Ok(lines) => {
                for (arm, ns) in lines {
                    let _ = writeln!(out, "m10 smoke {} {arm} ok {ns} ns", q.name);
                }
            }
            Err(e) => {
                let _ = writeln!(err, "m10 smoke {} FAILED: {e}", q.name);
                code = EXIT_FAILED;
            }
        }
    }
    code
}

fn list<M: Meter>(meter: &M, profile: Profile, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let mut code = EXIT_DONE;
    for q in grid() {
        let t = target(&q, profile);
        let tail = match generate(&t, false) {
            Ok(tail) => tail,
            Err(e) => {
                let _ = writeln!(err, "m10 list {}: {e}", q.name);
                code = EXIT_FAILED;
                continue;
            }
        };
        let before = meter.heap_counts();
        let overlay = tail.overlay();
        let counted = match (before, meter.heap_counts()) {
            (Some(b), Some(a)) => Some(a.live_bytes.saturating_sub(b.live_bytes)),
            _ => None,
        };
        if let Err(e) = &overlay {
            let _ = writeln!(err, "m10 list {}: {e}", q.name);
            code = EXIT_FAILED;
        }
        let m = tail.measures;
        let line = json!({
            "quantity": q.name,
            "family": q.clause.as_str(),
            "threshold": t.threshold,
            "mult": t.mult,
            "n": q.n,
            "stop": binding_clause(&t, &tail),
            "ops": m.ops,
            "rec_bytes": m.rec_bytes,
            "body_bytes": m.body_bytes,
            "rt_bytes": m.rt_bytes,
            "loc_bytes": m.loc_bytes,
            "seed_k": q.seed_k,
            "ovl": m.ovl,
            "ovl_counted": counted,
            "records": m.records,
            "groups": m.groups,
            "commits": m.commits,
            "settles": m.settles,
            "bytes": m.bytes,
        });
        drop(overlay);
        let _ = writeln!(out, "{line}");
    }
    code
}

/// One recorded run of `q` in this process.
fn run_one<V: StoreFs + SealedMaps, M: Meter, T: Ticker + Clone, R: CommandRunner>(
    sys: System<'_, V, M, T, R>,
    a: &Args,
    q: &Quantity,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let condition = match driver::condition(a) {
        Ok(c) => c,
        Err((code, e)) => {
            let _ = writeln!(err, "m10: {e}");
            return code;
        }
    };
    let scratch = match work_dir() {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(err, "m10 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let (tail, guard) = match fixture(sys.vfs, &scratch, q, sys.profile) {
        Ok(f) => f,
        Err(e) => {
            let _ = writeln!(err, "m10 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let root = match sys
        .vfs
        .open_root(&guard.fx.dir, RootRole::Other, RootAccess::Read)
    {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "m10 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let mut arms = quantity_arms(q, sys.vfs, &root, &guard.fx, &tail, &sys.ticker);
    let spec = RunSpec {
        measurement: MEASUREMENT,
        quantity: q.name.clone(),
        condition,
        min_tier: Some(Tier::T2),
    };
    let mut ctx = Context {
        meter: sys.meter,
        ticker: sys.ticker.clone(),
        runner: sys.runner,
        host: a.host.unwrap_or(HostKind::Laptop),
        toolchain: a.toolchain.clone().unwrap_or_default(),
        commit: a.commit.clone().unwrap_or_default(),
    };
    let t0 = driver::now_ms();
    let mut refs: Vec<&mut dyn Arm> = arms.iter_mut().map(|b| &mut **b as &mut dyn Arm).collect();
    let mut rec = match measure(&mut ctx, &spec, &mut refs) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "m10 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let t1 = driver::now_ms();
    driver::set_load_verdict(a, &mut rec, t0, t1, err, &format!("m10 {}", q.name));
    let dir = a.out.clone().unwrap_or_default();
    match driver::write_record(&rec, &dir) {
        Ok(p) => {
            let _ = writeln!(out, "m10 {} recorded {}", q.name, p.display());
            EXIT_DONE
        }
        Err(e) => {
            let _ = writeln!(err, "m10 {}: the record could not be written: {e}", q.name);
            EXIT_FAILED
        }
    }
}

/// The driver's command line ([MP §9.7] through [`crate::m1::driver`]): `--list` alone prints the list; otherwise
/// parses `args` (without the program name) by the shared contract, then runs the smoke, one quantity, or every
/// quantity in a child process of its own; returns the exit code.
pub fn cli<V: StoreFs + SealedMaps, M: Meter, T: Ticker + Clone, R: CommandRunner>(
    args: &[OsString],
    sys: System<'_, V, M, T, R>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if args.len() == 1 && args[0] == "--list" {
        return list(sys.meter, sys.profile, out, err);
    }
    let g = grid();
    let names: Vec<&str> = g.iter().map(|q| q.name.as_str()).collect();
    let a: Args = match driver::parse(args, &names) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "m10: {e}");
            let _ = err.write_all(USAGE.as_bytes());
            return EXIT_USAGE;
        }
    };
    if a.help {
        let _ = out.write_all(USAGE.as_bytes());
        return EXIT_DONE;
    }
    let asked = a
        .quantity
        .as_ref()
        .and_then(|n| g.iter().find(|q| &q.name == n));
    if a.smoke {
        let scratch = match work_dir() {
            Ok(d) => d,
            Err(e) => {
                let _ = writeln!(err, "m10 smoke: {e}");
                return EXIT_FAILED;
            }
        };
        let qs: Vec<Quantity> = match asked {
            Some(q) => vec![q.clone()],
            None => g.clone(),
        };
        return smoke(sys.vfs, &sys.ticker, &scratch, &qs, out, err);
    }
    if let Some(q) = asked {
        return run_one(sys, &a, q, out, err);
    }
    driver::each_in_own_process(args, &names, sys.spawn, out, err, "m10")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm::{MonoTicker, valid_name};
    use crate::condition::Condition;
    use crate::host::tests::Canned;
    use crate::m1::driver;
    use crate::record::RunRecord;
    use crate::tail::memfs::MemFs;
    use crate::tail::store::tests::{multi_extent_tail, scratch};
    use crate::testkit::{FakeMeter, FakeTicker, scratch_dir};
    use crate::tier::Plan;
    use crate::units::MS;
    use std::collections::BTreeSet;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn no_spawn(_: &[OsString]) -> Result<u8, String> {
        Err("no child process in this test".into())
    }

    /// `--commit` and `--toolchain`, which every mode but `--help` needs.
    const CT: [&str; 4] = ["--commit", "0123abcd", "--toolchain", "1.98.1"];

    /// The working directory, the only directory a test's store holds: every fixture must lie directly below it.
    fn cwd() -> PathBuf {
        std::env::current_dir().unwrap()
    }

    /// Runs the command line over `fs` with a real ticker; returns (exit, stdout, stderr).
    fn run(fs: &MemFs, args: &[&str]) -> (u8, String, String) {
        let meter = FakeMeter::new(8_000_000_000);
        let mut spawn = no_spawn;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(
            &os(args),
            System {
                vfs: fs,
                meter: &meter,
                ticker: MonoTicker::new(),
                runner: Canned::good(),
                spawn: &mut spawn,
                profile: Profile::Test,
            },
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// Runs one recorded quantity over a store whose every open takes 2 ms of a fake clock, so both arms pilot in t2.
    fn record(args: &[&str]) -> (u8, String, String, MemFs) {
        let clock = FakeTicker::manual();
        let runner = clock.with_steps(&[1]);
        let c = clock.clone();
        let fs = MemFs::with_hook(&cwd(), move || c.advance(2 * MS));
        let meter = FakeMeter::new(8_000_000_000);
        let mut spawn = no_spawn;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(
            &os(args),
            System {
                vfs: &fs,
                meter: &meter,
                ticker: runner,
                runner: Canned::good(),
                spawn: &mut spawn,
                profile: Profile::Test,
            },
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            fs,
        )
    }

    /// The follow-up block in run order (u17, cut §3.4).
    const FOLLOW_UP: [&str; 13] = [
        "ops-2048.n1e4",
        "quiet-2mib-x4-p2048.n1e6",
        "rec-512kib.n1e5",
        "body-1mib.n1e5",
        "ovl-64kib.n1e5",
        "ops-1024.n1e4",
        "quiet-2mib-x8-p2048.n1e6",
        "quiet-2mib-x4-p1024.n1e6",
        "quiet-2mib-x8-p1024.n1e6",
        "rec-256kib.n1e5",
        "body-512kib.n1e5",
        "ovl-96kib.n1e5",
        "quiet-2mib-x2-p2048.n1e6",
    ];

    /// The A1 block in run order (u20 cut §3.5): the 1e4 points, the 1e5 and the 1e6 points, the two quiet points last.
    const A1_BLOCK: [&str; 18] = [
        "a1-rec-128kib.n1e4",
        "a1-rec-256kib.n1e4",
        "a1-ovl-48kib.n1e4",
        "a1-ovl-64kib.n1e4",
        "a1-cli2x-128kib.n1e4",
        "a1-rec-128kib.n1e5",
        "a1-rec-256kib.n1e5",
        "a1-ovl-48kib.n1e5",
        "a1-ovl-64kib.n1e5",
        "a1-cli2x-128kib.n1e5",
        "a1-rec-128kib.n1e6",
        "a1-rec-256kib.n1e6",
        "a1-ovl-48kib.n1e6",
        "a1-ovl-64kib.n1e6",
        "a1-ovl-192kib.n1e6",
        "a1-cli2x-128kib.n1e6",
        "a1-quiet-192kib-x2-p1024.n1e6",
        "a1-quiet-192kib-x4-p1024.n1e6",
    ];

    /// The grid in run order with valid, unique names ([MP §4.1], `measurement-protocol.md:169`): the night's 18 first,
    /// unchanged and with no P06 of their own, one point of each family first; the follow-up block of 13 in its order;
    /// then u20's appended block of design-A A3's 18 small tails in A1, the 1e4 points first (u20 cut §3.5, §3.10).
    /// By clause, 8 `ops`, 11 `rec`, 5 `body`, 11 `ovl`, 14 `quiet` (the CLI's 2× tails among them).
    #[test]
    fn grid_lists_the_valid_quantities_in_run_order() {
        let g = grid();
        assert_eq!(g.len(), 49);
        assert!(g.iter().all(|q| valid_name(&q.name)), "{g:?}");
        let names: BTreeSet<&str> = g.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(names.len(), 49, "unique");
        let count = |c: Clause| g.iter().filter(|q| q.clause == c).count();
        assert_eq!(
            [
                Clause::Ops,
                Clause::Rec,
                Clause::Body,
                Clause::Ovl,
                Clause::Quiet
            ]
            .map(count),
            [8, 11, 5, 11, 14]
        );
        let lead: Vec<&str> = g[..5].iter().map(|q| q.name.as_str()).collect();
        assert_eq!(
            lead,
            [
                "ops-4096.n1e6",
                "rec-4mib.n1e6",
                "body-32mib.n1e6",
                "ovl-1mib.n1e6",
                "quiet-2mib-x8.n1e6"
            ],
            "one point of each family first (R-34 B-3, M-15)"
        );
        assert_eq!(g[5].name, "ops-4096.n1e5");
        assert_eq!(g[17].name, "quiet-1536kib-x4.n1e6");
        assert!(names.contains("rec-4mib.n1e6") && names.contains("body-32mib.n1e6"));
        assert!(names.contains("ovl-512kib.n1e6") && names.contains("quiet-2mib-x8.n1e6"));
        assert!(g[..18].iter().all(|q| q.over.p06.is_none()));
        let follow: Vec<&str> = g[18..31].iter().map(|q| q.name.as_str()).collect();
        assert_eq!(
            follow, FOLLOW_UP,
            "the follow-up block after the night's 18"
        );
        let a1: Vec<&str> = g[31..].iter().map(|q| q.name.as_str()).collect();
        assert_eq!(a1, A1_BLOCK, "u20's A1 block after the follow-up block");
        assert!(
            g[..31]
                .iter()
                .all(|q| q.form == Locator::None && q.kind == Kind::Tail)
        );
        assert!(
            g[31..]
                .iter()
                .all(|q| q.form == Locator::A1 && q.kind == Kind::Tail)
        );
    }

    /// Each follow-up point has the parameters of cut §3.4, and so has its production target; the night's 18 targets are
    /// unchanged: the production parameters, seeded by their names.
    #[test]
    fn follow_up_points_have_their_parameters() {
        use Clause::{Body, Ops, Ovl, Quiet, Rec};
        // (name, clause, threshold, P11, N, P06)
        type Point = (&'static str, Clause, u64, u64, u32, Option<u64>);
        let want: [Point; 13] = [
            ("ops-2048.n1e4", Ops, 2_048, 1, 10_000, None),
            (
                "quiet-2mib-x4-p2048.n1e6",
                Quiet,
                2 * MIB,
                4,
                1_000_000,
                Some(2_048),
            ),
            ("rec-512kib.n1e5", Rec, 512 * KIB, 1, 100_000, None),
            ("body-1mib.n1e5", Body, MIB, 1, 100_000, None),
            ("ovl-64kib.n1e5", Ovl, 64 * KIB, 1, 100_000, None),
            ("ops-1024.n1e4", Ops, 1_024, 1, 10_000, None),
            (
                "quiet-2mib-x8-p2048.n1e6",
                Quiet,
                2 * MIB,
                8,
                1_000_000,
                Some(2_048),
            ),
            (
                "quiet-2mib-x4-p1024.n1e6",
                Quiet,
                2 * MIB,
                4,
                1_000_000,
                Some(1_024),
            ),
            (
                "quiet-2mib-x8-p1024.n1e6",
                Quiet,
                2 * MIB,
                8,
                1_000_000,
                Some(1_024),
            ),
            ("rec-256kib.n1e5", Rec, 256 * KIB, 1, 100_000, None),
            ("body-512kib.n1e5", Body, 512 * KIB, 1, 100_000, None),
            ("ovl-96kib.n1e5", Ovl, 96 * KIB, 1, 100_000, None),
            (
                "quiet-2mib-x2-p2048.n1e6",
                Quiet,
                2 * MIB,
                2,
                1_000_000,
                Some(2_048),
            ),
        ];
        let g = grid();
        for (name, clause, threshold, mult, n, p06) in want {
            let Some(q) = g.iter().find(|q| q.name == name) else {
                panic!("{name} is not in the grid");
            };
            assert_eq!(
                (q.clause, q.threshold, q.mult, q.n, q.over.p06),
                (clause, threshold, mult, n, p06),
                "{name}"
            );
            let mut params = Params::PRODUCTION;
            params.p06 = p06.unwrap_or(params.p06);
            assert_eq!(
                target(q, Profile::Production),
                Target {
                    clause,
                    threshold,
                    mult,
                    n,
                    seed: seed(name),
                    params,
                    rt_bound: None,
                },
                "{name}"
            );
        }
        for q in &g[..18] {
            assert_eq!(
                target(q, Profile::Production),
                Target {
                    clause: q.clause,
                    threshold: q.threshold,
                    mult: q.mult,
                    n: q.n,
                    seed: seed(&q.name),
                    params: Params::PRODUCTION,
                    rt_bound: None,
                },
                "{}",
                q.name
            );
        }
        assert_eq!(Profile::Production.sealed_len(10_000), 123_750);
    }

    /// Each follow-up point's production tail stops on the clause it targets ([F17 §5.2]–§5.3): `ops` at P06, `body` at
    /// P08, `rec` and `ovl` at most at their threshold with the refused group past it (or one commit), and every quiet
    /// point on its ops clause, `ops` ≤ P11·P06 < the refused group's, with the point's P06 as the target's.
    #[test]
    fn follow_up_tails_stop_on_their_clauses() {
        let g = grid();
        for q in &g[18..31] {
            let t = target(q, Profile::Production);
            let tail = generate(&t, false).unwrap_or_else(|e| panic!("{}: {e}", q.name));
            let (m, r) = (tail.measures, tail.refused);
            let past = |f: fn(&crate::tail::synth::Measures) -> u64| {
                f(&m) <= q.threshold && r.is_some_and(|r| f(&r) > q.threshold) || m.commits == 1
            };
            let ok = match q.clause {
                Clause::Ops => m.ops == q.threshold,
                Clause::Body => m.body_bytes == q.threshold,
                Clause::Rec => past(|m| m.rec_bytes),
                Clause::Ovl => past(|m| m.ovl),
                Clause::Quiet => {
                    let cap = q.mult * q.over.p06.unwrap();
                    binding_clause(&t, &tail) == "ops"
                        && m.ops <= cap
                        && r.is_some_and(|r| r.ops > cap)
                        && t.params.p06 == q.over.p06.unwrap()
                }
            };
            assert!(
                ok,
                "{}: stop {}, {m:?}, refused {r:?}",
                q.name,
                binding_clause(&t, &tail)
            );
        }
    }

    /// The night's 18 thresholds are F17's candidates (`17-store-parameters.md:757-762`; `docs/spec/HOLES.md:41-46`):
    /// P06 4,096, 2,048, 1,024 at 1e5 and 1e6; P07 4, 2, 1 MiB; P08 32, 16, 8 MiB; P09 1 MiB, 512 KiB; P10 2 MiB,
    /// 1.5 MiB × P11 8, 4. The follow-up block after them lies below F17's candidates (cut §3.4).
    #[test]
    fn grid_thresholds_are_f17_candidates() {
        let g = &grid()[..18];
        let of = |c: Clause| -> BTreeSet<(u64, u64, u32)> {
            g.iter()
                .filter(|q| q.clause == c)
                .map(|q| (q.threshold, q.mult, q.n))
                .collect()
        };
        let ops: BTreeSet<(u64, u64, u32)> = [4_096, 2_048, 1_024]
            .iter()
            .flat_map(|&t| [(t, 1, 100_000), (t, 1, 1_000_000)])
            .collect();
        assert_eq!(of(Clause::Ops), ops);
        assert_eq!(
            of(Clause::Rec),
            [
                (4 * MIB, 1, 1_000_000),
                (2 * MIB, 1, 1_000_000),
                (MIB, 1, 1_000_000)
            ]
            .into()
        );
        assert_eq!(
            of(Clause::Body),
            [
                (32 * MIB, 1, 1_000_000),
                (16 * MIB, 1, 1_000_000),
                (8 * MIB, 1, 1_000_000)
            ]
            .into()
        );
        assert_eq!(
            of(Clause::Ovl),
            [(MIB, 1, 1_000_000), (512 * KIB, 1, 1_000_000)].into()
        );
        let quiet: BTreeSet<(u64, u64, u32)> = [2 * MIB, 1_536 * KIB]
            .iter()
            .flat_map(|&p| [(p, 8, 1_000_000), (p, 4, 1_000_000)])
            .collect();
        assert_eq!(of(Clause::Quiet), quiet);
        // At production a run's target is the candidate itself; at the test profile the profile's threshold.
        let q = g.iter().find(|q| q.name == "body-32mib.n1e6").unwrap();
        assert_eq!(target(q, Profile::Production).threshold, q.threshold);
        assert_eq!(target(q, Profile::Test).threshold, Params::TEST.p08);
        assert_eq!(Profile::Production.sealed_len(1_000_000), 12_375_000);
    }

    /// `--smoke` runs every quantity at the test profile with each arm once: 116 lines, one per (quantity, arm), the A1
    /// points with `replay.at` first, each fixture's floor mapping 8 files and reading `HEAD` once; with `--quantity` it
    /// runs that one alone.
    #[test]
    fn smoke_exercises_every_arm_once() {
        let fs = MemFs::with_dir(&cwd());
        let one = [&["--smoke", "--quantity", "ops-4096.n1e5"][..], &CT].concat();
        let (code, out, err) = run(&fs, &one);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(
            lines
                .iter()
                .all(|l| l.starts_with("m10 smoke ops-4096.n1e5 "))
        );
        fs.clear_log();
        let (code, out, err) = run(&fs, &[&["--smoke"][..], &CT].concat());
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 116, "{out}");
        let pairs: BTreeSet<(String, String)> = lines
            .iter()
            .map(|l| {
                let w: Vec<&str> = l.split(' ').collect();
                assert_eq!((w[0], w[1], w[4]), ("m10", "smoke", "ok"), "{l}");
                (w[2].to_string(), w[3].to_string())
            })
            .collect();
        let expect: BTreeSet<(String, String)> = grid()
            .iter()
            .flat_map(|q| {
                let mut arms = vec![
                    (q.name.clone(), "replay".to_string()),
                    (q.name.clone(), "floor.open".to_string()),
                ];
                if q.form == Locator::A1 {
                    arms.push((q.name.clone(), "replay.at".into()));
                }
                arms
            })
            .collect();
        assert_eq!(pairs, expect);
        let log = fs.log();
        assert_eq!(log.maps.len(), grid().len() * SEALED_FILES);
        assert_eq!(
            log.opens.iter().filter(|o| *o == "HEAD").count(),
            grid().len()
        );
    }

    /// `--smoke` with the runner's contract flags records nothing: no line says recorded, every fixture is removed,
    /// and `--smoke` with `--out` is refused.
    #[test]
    fn smoke_records_nothing() {
        let fs = MemFs::with_dir(&cwd());
        let args = [
            &["--host", "laptop", "--condition", "idle", "--smoke"][..],
            &CT,
        ]
        .concat();
        let (code, out, err) = run(&fs, &args);
        assert_eq!(code, EXIT_DONE, "{err}");
        assert_eq!(out.lines().count(), 116);
        assert!(!out.contains("recorded"));
        assert!(fs.files().is_empty(), "every fixture removed");
        let (code, _, err) = run(&fs, &[&args[..], &["--out", "/o"]].concat());
        assert_eq!(code, EXIT_USAGE);
        assert!(err.contains("takes no --out"), "{err}");
    }

    /// The floor arm ([60 §5.3], `60-roadmap.md:790`) reads `HEAD` once with 8,192 bytes, then reads each of 8 sealed
    /// files' 120-byte header and maps it, per sample.
    #[test]
    fn floor_reads_head_once_and_maps_eight_files() {
        let fs = MemFs::with_dir(&scratch());
        let ops = grid()
            .into_iter()
            .find(|q| q.name == "ops-4096.n1e5")
            .unwrap();
        let tail = generate(&target(&ops, Profile::Test), false).unwrap();
        let fx = write_fixture(&fs, &scratch(), "fx", &tail, TEST_SEALED_LEN).unwrap();
        let root = fs
            .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
            .unwrap();
        let (_, mut floor) = arms(&fs, &root, &fx, &tail, &MonoTicker::new());
        assert_eq!(floor.name(), "floor.open");
        for _ in 0..2 {
            fs.clear_log();
            floor.sample().unwrap();
            let log = fs.log();
            let mut names = vec!["HEAD".to_string()];
            names.extend(crate::tail::store::sealed_names());
            assert_eq!(log.opens, names);
            let head_reads: Vec<_> = log.reads.iter().filter(|r| r.0 == "HEAD").collect();
            assert_eq!(head_reads, [&("HEAD".to_string(), 0, 8_192)]);
            assert_eq!(log.reads.len(), 1 + SEALED_FILES);
            assert!(log.reads[1..].iter().all(|r| (r.1, r.2) == (0, 120)));
            assert_eq!(log.maps.len(), SEALED_FILES);
            assert!(log.maps.iter().all(|m| m.1 == TEST_SEALED_LEN));
        }
    }

    /// The replay arm reads the tail through `log.<n>` files in fixed 64-KiB chunks, never more, and reads all of it.
    #[test]
    fn replay_arm_reads_the_tail_in_fixed_chunks() {
        let fs = MemFs::with_dir(&scratch());
        let tail = multi_extent_tail();
        let fx = write_fixture(&fs, &scratch(), "fx", &tail, TEST_SEALED_LEN).unwrap();
        let root = fs
            .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
            .unwrap();
        let (mut replay, _) = arms(&fs, &root, &fx, &tail, &MonoTicker::new());
        assert_eq!(replay.name(), "replay");
        fs.clear_log();
        replay.sample().unwrap();
        let log = fs.log();
        assert!(log.opens.len() >= 4 && log.opens.iter().all(|o| o.starts_with("log.")));
        assert!(
            log.reads
                .iter()
                .all(|r| r.0.starts_with("log.") && r.2 <= TAIL_READ_CHUNK)
        );
        assert!(log.reads.iter().filter(|r| r.2 == TAIL_READ_CHUNK).count() >= 3);
        let asked: usize = log.reads.iter().map(|r| r.2).sum();
        assert_eq!(
            asked as u64,
            tail.end - tail.start,
            "every tail byte read once"
        );
        assert!(log.maps.is_empty());
    }

    /// A `--quantity` run writes exactly one record directly in `--out`, which is created when missing, that reads
    /// back: measurement 10, the two arms, t2's 1,000 × 5 plan, the given commit and toolchain; its fixture is removed.
    #[test]
    fn run_writes_one_valid_record() {
        let root = scratch_dir("m10-run");
        let out_dir = root.join("measurements").join("10");
        let o = out_dir.to_str().unwrap().to_string();
        let (code, out, err, fs) = record(&[
            "--quantity",
            "ops-1024.n1e5",
            "--host",
            "laptop",
            "--condition",
            "idle",
            "--out",
            &o,
            "--commit",
            "0123abcd",
            "--toolchain",
            "1.98.1",
        ]);
        assert_eq!(code, EXIT_DONE, "{err}");
        let files: Vec<_> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(out.contains("recorded"));
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
        let rec = RunRecord::from_json(&v).unwrap();
        assert_eq!(
            (rec.measurement, rec.quantity.as_str()),
            (10, "ops-1024.n1e5")
        );
        let arms: Vec<&str> = rec.arms.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(arms, ["replay", "floor.open"]);
        assert_eq!(rec.plan, Plan::new(1_000, 5));
        assert_eq!(
            (rec.commit.as_str(), rec.toolchain.as_str()),
            ("0123abcd", "1.98.1")
        );
        assert_eq!(rec.condition, Condition::Idle);
        assert!(rec.valid(), "{:?}", rec.disqualifications());
        assert!(fs.files().is_empty(), "the fixture is removed");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Usage errors exit 2 with the reason, by the shared contract ([MP §9.7]): `--commit` and `--toolchain` required,
    /// a recording run without `--out`, `--host` or `--condition`, a loaded run without `--load-log`, `--smoke` with
    /// `--out`, `--scratch` (every fixture lies below the working directory), an unknown flag, host, condition or
    /// quantity. `--help` and `--list` alone exit 0.
    #[test]
    fn cli_refuses_usage_errors() {
        let fs = MemFs::with_dir(&cwd());
        let contract = ["--host", "laptop", "--condition", "idle", "--out", "/o"];
        let with = |more: &[&'static str]| -> Vec<&'static str> { [&CT[..], more].concat() };
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (contract.to_vec(), "--commit and --toolchain"),
            (
                [&contract[..], &["--commit", "c"]].concat(),
                "--commit and --toolchain",
            ),
            (with(&["--bogus"]), "unknown argument '--bogus'"),
            (
                with(&["--quantity", "ops-9.n1e5", "--smoke"]),
                "unknown quantity",
            ),
            (
                with(&["--host", "desk", "--condition", "idle", "--out", "/o"]),
                "unknown host",
            ),
            (
                with(&["--host", "laptop", "--condition", "busy", "--out", "/o"]),
                "unknown condition",
            ),
            (
                with(&["--condition", "idle", "--out", "/o"]),
                "needs --host and --condition",
            ),
            (
                with(&["--host", "laptop", "--out", "/o"]),
                "needs --host and --condition",
            ),
            (
                with(&["--host", "laptop", "--condition", "idle"]),
                "needs --out",
            ),
            (
                with(&["--host", "laptop", "--condition", "loaded", "--out", "/o"]),
                "needs --load-log",
            ),
            (
                with(&[
                    "--host",
                    "laptop",
                    "--condition",
                    "synthetic",
                    "--out",
                    "/o",
                ]),
                "needs --load-log",
            ),
            (with(&["--smoke", "--out", "/o"]), "takes no --out"),
            (
                with(&["--smoke", "--scratch", "/scratch"]),
                "unknown argument '--scratch'",
            ),
            (with(&["--list"]), "unknown argument '--list'"),
            (with(&["--out"]), "needs a value"),
        ];
        for (args, why) in cases {
            let (code, _, err) = run(&fs, &args);
            assert_eq!(code, EXIT_USAGE, "{args:?}");
            assert!(err.contains(why), "{args:?}: {err}");
            assert!(err.contains("usage: m10"));
        }
        let (code, out, _) = run(&fs, &["--help"]);
        assert_eq!((code, out.as_str()), (EXIT_DONE, USAGE));
        let (code, out, _) = run(&fs, &["--list"]);
        assert_eq!((code, out.lines().count()), (EXIT_DONE, 49));
        let names: Vec<String> = grid().into_iter().map(|q| q.name).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        assert!(
            driver::parse(&os(&[&contract[..], &CT].concat()), &refs).is_ok(),
            "the §9.7 contract alone is a valid call"
        );
    }

    /// Without `--quantity` each quantity runs in a child of its own, in grid order, with the same flags plus
    /// `--quantity <q>`; the exit is the worst child's, and a child that cannot start counts as failed.
    #[test]
    fn cli_without_quantity_spawns_one_process_per_run() {
        let fs = MemFs::with_dir(&cwd());
        let meter = FakeMeter::new(8_000_000_000);
        let base = os(&[
            &["--host", "laptop", "--condition", "idle", "--out", "/o"][..],
            &CT,
        ]
        .concat());
        for (bad, expect) in [
            (None, EXIT_DONE),
            (Some(Ok(1)), EXIT_FAILED),
            (Some(Err(())), EXIT_FAILED),
        ] {
            let mut calls: Vec<Vec<OsString>> = Vec::new();
            let mut spawn = |a: &[OsString]| -> Result<u8, String> {
                calls.push(a.to_vec());
                match (calls.len(), bad) {
                    (5, Some(Ok(c))) => Ok(c),
                    (5, Some(Err(()))) => Err("cannot start".into()),
                    _ => Ok(0),
                }
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = cli(
                &base,
                System {
                    vfs: &fs,
                    meter: &meter,
                    ticker: MonoTicker::new(),
                    runner: Canned::good(),
                    spawn: &mut spawn,
                    profile: Profile::Test,
                },
                &mut out,
                &mut err,
            );
            assert_eq!(code, expect);
            assert_eq!(calls.len(), 49);
            for (c, q) in calls.iter().zip(grid()) {
                let mut want = base.clone();
                want.push("--quantity".into());
                want.push(q.name.into());
                assert_eq!(c, &want);
            }
            assert_eq!(String::from_utf8(out).unwrap().lines().count(), 49);
        }
        assert!(fs.files().is_empty(), "the parent writes no fixture");
    }

    /// A synthetic run's condition is the load log header's ([MP §9.7], `measurement-protocol.md:771-776`), and a
    /// `--condition` the header contradicts is a usage error.
    #[test]
    fn cli_condition_comes_from_the_load_log() {
        let dir = scratch_dir("m10-cond");
        let log = dir.join("load.log");
        let header = json!({
            "schema": crate::load::replay::LOG_SCHEMA,
            "source": {"kind": "synthetic", "spec": "seconds=300,cpu=50"},
            "interval_ms": 1000,
            "sample_ms": 1000,
            "started": 0,
        });
        std::fs::write(&log, format!("{header}\n")).unwrap();
        let out_dir = dir.join("out");
        std::fs::create_dir(&out_dir).unwrap();
        let (l, o) = (
            log.to_str().unwrap().to_string(),
            out_dir.to_str().unwrap().to_string(),
        );
        let args = |cond: &'static str| {
            vec![
                "--quantity".to_string(),
                "ovl-1mib.n1e6".into(),
                "--host".into(),
                "hosted".into(),
                "--condition".into(),
                cond.into(),
                "--load-log".into(),
                l.clone(),
                "--out".into(),
                o.clone(),
                "--commit".into(),
                "c".into(),
                "--toolchain".into(),
                "t".into(),
            ]
        };
        let a = args("synthetic");
        let (code, _, err, _) = record(&a.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(code, EXIT_DONE, "{err}");
        let files: Vec<_> = std::fs::read_dir(&out_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
        let rec = RunRecord::from_json(&v).unwrap();
        assert_eq!(
            rec.condition,
            Condition::Synthetic {
                description: "moirai-probes-bin loadgen, synthetic profile seconds=300,cpu=50"
                    .into()
            }
        );
        let a = args("loaded");
        let (code, _, err, _) = record(&a.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(code, EXIT_USAGE);
        assert!(err.contains("header gives synthetic"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--list` prints one JSON line per quantity, in grid order, with the tail measures of [F17 §5.1] and item 1a's
    /// locator bytes, the binding clause, the production seed's k, and the allocator's count (`null` without
    /// `CountingAlloc`).
    #[test]
    fn list_prints_the_tail_measures() {
        let fs = MemFs::with_dir(&scratch());
        let (code, out, err) = run(&fs, &["--list"]);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 49);
        for (v, q) in lines.iter().zip(grid()) {
            assert_eq!(v["quantity"], q.name.as_str());
            assert_eq!(v["family"], q.clause.as_str());
            let t = target(&q, Profile::Test);
            let m = generate(&t, false).unwrap().measures;
            for (k, x) in [
                ("ops", m.ops),
                ("rec_bytes", m.rec_bytes),
                ("body_bytes", m.body_bytes),
                ("rt_bytes", m.rt_bytes),
                ("loc_bytes", m.loc_bytes),
                ("seed_k", q.seed_k),
                ("ovl", m.ovl),
                ("commits", m.commits),
                ("settles", m.settles),
                ("bytes", m.bytes),
            ] {
                assert_eq!(v[k].as_u64(), Some(x), "{} {k}", q.name);
            }
            assert!(m.ovl > 0 && m.bytes > 0);
            assert_eq!(m.loc_bytes > 0, q.form == Locator::A1, "{}", q.name);
            assert!(v["ovl_counted"].is_null());
            assert!(["ops", "rec", "body", "ovl"].contains(&v["stop"].as_str().unwrap()));
            assert_eq!(v["threshold"].as_u64(), Some(t.threshold));
        }
    }

    /// The night's plan lists exactly the grid's names in run order ([MP §4.1], R-27 Q2, R-34 M-13).
    #[test]
    fn grid_equals_the_plans_list() {
        let plan = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../xtask/measure-night.toml");
        let text = std::fs::read_to_string(plan).unwrap();
        let list = driver::plan_quantities(&text, MEASUREMENT).unwrap();
        let names: Vec<String> = grid().into_iter().map(|q| q.name).collect();
        assert_eq!(list, names);
    }

    /// The fixtures lie below the working directory, not the system temporary directory ([MP §9.7] "The working
    /// directory"; R-34 B-1): over a store that holds only the working directory, the smoke and a recording run make
    /// and remove their fixtures, which they could not do anywhere else.
    #[test]
    fn fixtures_stay_below_the_working_directory() {
        let fs = MemFs::with_dir(&cwd());
        let (code, out, err) = run(
            &fs,
            &[&["--smoke", "--quantity", "ops-4096.n1e5"][..], &CT].concat(),
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        assert_eq!(out.lines().count(), 2);
        assert!(
            fs.log().opens.iter().any(|o| o == "HEAD"),
            "the fixture was opened"
        );
        assert!(fs.files().is_empty(), "and removed");
        let elsewhere = MemFs::with_dir(&std::env::temp_dir());
        let (code, _, err) = run(
            &elsewhere,
            &[&["--smoke", "--quantity", "ops-4096.n1e5"][..], &CT].concat(),
        );
        assert_eq!(
            code, EXIT_FAILED,
            "a store without the working directory: {err}"
        );
    }

    /// Each A1 point has the parameters of u20 cut §3.5's table: its clause, cap, P11, N and design-A A3's drafts
    /// (`design-A.md:149-156`; the CLI's 2× tails with P10 := 2 × P09 = 128 KiB and q = 2), in A1; its production target
    /// is the point at those parameters with the seed `seed(name) ^ seed_k`, and its test-profile target the test
    /// column's, in A1, with k = 0.
    #[test]
    fn a1_points_have_their_parameters() {
        use Clause::{Ovl, Quiet, Rec};
        let g = grid();
        let cli = Over {
            p10: Some(128 * KIB),
            p11: Some(2),
            ..Over::A3
        };
        let quiet = |x: u64| Over {
            p11: Some(x),
            ..Over::A3
        };
        assert_eq!(
            (
                Over::A3.p06,
                Over::A3.p07,
                Over::A3.p08,
                Over::A3.p09,
                Over::A3.p10,
                Over::A3.p11
            ),
            (
                Some(1_024),
                Some(128 * KIB),
                Some(MIB),
                Some(64 * KIB),
                Some(192 * KIB),
                Some(4)
            )
        );
        let mut want: Vec<(String, Clause, u64, u64, u32, Over)> = Vec::new();
        for (n, s) in [(10_000, "n1e4"), (100_000, "n1e5"), (1_000_000, "n1e6")] {
            want.push((format!("a1-rec-128kib.{s}"), Rec, 128 * KIB, 1, n, Over::A3));
            want.push((format!("a1-rec-256kib.{s}"), Rec, 256 * KIB, 1, n, Over::A3));
            want.push((format!("a1-ovl-48kib.{s}"), Ovl, 48 * KIB, 1, n, Over::A3));
            want.push((format!("a1-ovl-64kib.{s}"), Ovl, 64 * KIB, 1, n, Over::A3));
            if n == 1_000_000 {
                want.push((format!("a1-ovl-192kib.{s}"), Ovl, 192 * KIB, 1, n, Over::A3));
            }
            want.push((format!("a1-cli2x-128kib.{s}"), Quiet, 128 * KIB, 2, n, cli));
        }
        for x in [2, 4] {
            want.push((
                format!("a1-quiet-192kib-x{x}-p1024.n1e6"),
                Quiet,
                192 * KIB,
                x,
                1_000_000,
                quiet(x),
            ));
        }
        let got: Vec<(String, Clause, u64, u64, u32, Over)> = g[31..]
            .iter()
            .map(|q| (q.name.clone(), q.clause, q.threshold, q.mult, q.n, q.over))
            .collect();
        assert_eq!(got, want);
        for q in &g[31..] {
            let k = SEED_K.iter().find(|(n, _)| *n == q.name).unwrap().1;
            assert_eq!(q.seed_k, k, "{}", q.name);
            let mut params = q.over.apply(Params::PRODUCTION);
            params.shape = crate::tail::Shape::A1;
            assert_eq!(
                target(q, Profile::Production),
                Target {
                    clause: q.clause,
                    threshold: q.threshold,
                    mult: q.mult,
                    n: q.n,
                    seed: seed(&q.name) ^ k,
                    params,
                    rt_bound: None,
                },
                "{}",
                q.name
            );
            let t = target(q, Profile::Test);
            let mut test = Params::TEST;
            test.shape = crate::tail::Shape::A1;
            assert_eq!((t.params, t.seed), (test, seed(&q.name)), "{}", q.name);
        }
    }

    /// Each A1 point's production tail reaches its own cap and stops on the clause it targets (spec Goal 5; u20 cut D-5,
    /// addendum m-4; `m10.md:267-268`): at `seed(name) ^ seed_k` it stops on that clause, the refused group passes the
    /// cap, and the measure is 95–100 % of it; every smaller k fails one of these, so `seed_k` is the first. The quiet
    /// points end on their record caps of 256 and 512 KiB. At the test profile each tail stops on a clause of its family.
    #[test]
    fn a1_tails_reach_their_caps_and_stop_on_their_clauses() {
        let mut failed = Vec::new();
        for q in &grid()[31..] {
            let t = target(q, Profile::Production);
            let tail = generate(&t, false).unwrap_or_else(|e| panic!("{}: {e}", q.name));
            let (ok, clause, share) = reaches_cap(q, &t, &tail);
            if !ok {
                failed.push(format!(
                    "{}: k {} stops {} at {share} per mille of its {clause} cap",
                    q.name,
                    q.seed_k,
                    binding_clause(&t, &tail)
                ));
            }
            if q.name.contains("-quiet-") {
                assert_eq!(clause, "rec", "{}", q.name);
                assert!(
                    tail.measures.rec_bytes <= q.mult * 128 * KIB
                        && tail.refused.unwrap().rec_bytes > q.mult * 128 * KIB,
                    "{}: its record cap",
                    q.name
                );
            }
            for j in 0..q.seed_k {
                let mut tj = t;
                tj.seed = seed(&q.name) ^ j;
                let tail = generate(&tj, false).unwrap();
                if reaches_cap(q, &tj, &tail).0 {
                    failed.push(format!(
                        "{}: k {j} < {} already reaches its cap",
                        q.name, q.seed_k
                    ));
                }
            }
            let tt = target(q, Profile::Test);
            let small = generate(&tt, false).unwrap();
            let stop = binding_clause(&tt, &small);
            assert!(
                small.refused.is_some() && (q.clause == Clause::Quiet || stop == q.clause.as_str()),
                "{} at the test profile: {stop}",
                q.name
            );
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    /// The A1 points' tails are in A1 (u20 cut §3.5): at the test profile and at production (the 1e4 points), the tail's
    /// `Expect` names A1, every commit is in A1, some carry locators, and `loc_bytes` counts them; the night's points'
    /// tails carry none.
    #[test]
    fn a1_tails_carry_the_a1_form() {
        let g = grid();
        for q in &g {
            let profiles: &[Profile] = if q.form == Locator::A1 && q.n == 10_000 {
                &[Profile::Test, Profile::Production]
            } else {
                &[Profile::Test]
            };
            for &profile in profiles {
                let tail = generate(&target(q, profile), true).unwrap();
                assert_eq!(tail.expect.locator, q.form, "{} {profile:?}", q.name);
                let commits: Vec<&crate::tail::commit::Commit<'_>> = tail
                    .records
                    .iter()
                    .filter_map(|r| match &r.payload {
                        crate::tail::rec::Payload::Commit(c) => Some(&**c),
                        _ => None,
                    })
                    .collect();
                assert!(
                    commits.iter().all(|c| c.form == q.form),
                    "{} {profile:?}",
                    q.name
                );
                let locs: usize = commits.iter().map(|c| c.locs.len()).sum();
                assert_eq!(
                    (locs > 0, tail.measures.loc_bytes > 0),
                    (q.form == Locator::A1, q.form == Locator::A1),
                    "{} {profile:?}",
                    q.name
                );
            }
        }
    }

    /// The night's 31 points keep their arms and tails (u20 cut D-3): arms `replay` and `floor.open`, the night's form,
    /// their production targets the column's with only a follow-up point's P06, and the seed of their name; the A1
    /// points add `replay.at` first.
    #[test]
    fn night_points_keep_their_arms_and_tails() {
        let fs = MemFs::with_dir(&scratch());
        let g = grid();
        for (i, q) in g.iter().enumerate() {
            let tail = generate(&target(q, Profile::Test), false).unwrap();
            let fx =
                write_fixture(&fs, &scratch(), &format!("fx{i}"), &tail, TEST_SEALED_LEN).unwrap();
            let root = fs
                .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
                .unwrap();
            let arms = quantity_arms(q, &fs, &root, &fx, &tail, &MonoTicker::new());
            let names: Vec<&str> = arms.iter().map(|a| a.name()).collect();
            if i < 31 {
                assert_eq!(names, ["replay", "floor.open"], "{}", q.name);
                let mut params = Params::PRODUCTION;
                params.p06 = q.over.p06.unwrap_or(params.p06);
                let t = target(q, Profile::Production);
                assert_eq!(
                    (t.params, t.seed, q.form, q.seed_k),
                    (params, seed(&q.name), Locator::None, 0),
                    "{}",
                    q.name
                );
                assert_eq!(t.params.shape, crate::tail::Shape::NIGHT, "{}", q.name);
            } else {
                assert_eq!(names, ["replay.at", "replay", "floor.open"], "{}", q.name);
            }
            drop(arms);
            remove_fixture(&fs, &scratch(), &fx).unwrap();
        }
    }

    /// A ticker over a [`MemFs`] that records how many reads the store had logged at each reading and returns 1, 2, 3, …
    /// (R-29): `TimedFn::sample` reads it exactly at t0 and t1.
    #[derive(Clone)]
    struct ReadTicker<'f> {
        fs: &'f MemFs,
        ticks: std::rc::Rc<std::cell::RefCell<Vec<usize>>>,
    }

    impl Ticker for ReadTicker<'_> {
        fn now_ns(&mut self) -> u64 {
            let mut t = self.ticks.borrow_mut();
            t.push(self.fs.log().reads.len());
            t.len() as u64
        }
    }

    /// `replay.at` reads the tail inside its timed region (R-29; u20 cut D-2): over a multi-extent A1 tail, each sample's
    /// reads between its two ticks are exactly the tail's `[start, end)`, in log order, every one a `log.<n>` read of at
    /// most 64 KiB; none falls before t0; and the sample succeeds, so its offsets were built.
    #[test]
    fn replay_at_arm_reads_the_tail_inside_its_timed_region() {
        let fs = MemFs::with_dir(&scratch());
        let mut t = crate::tail::synth::tests::a1(crate::tail::synth::tests::test_target(
            Clause::Ops,
            1_000,
            21,
        ));
        t.threshold = 1_200;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, false).unwrap();
        let geo = tail.expect.geo;
        assert!(geo.extent(tail.end - 1) > geo.extent(tail.start));
        let fx = write_fixture(&fs, &scratch(), "fx", &tail, TEST_SEALED_LEN).unwrap();
        let root = fs
            .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
            .unwrap();
        let ticker = ReadTicker {
            fs: &fs,
            ticks: Default::default(),
        };
        let mut arm = replay_at_arm(&fs, &root, &fx, &tail, &ticker);
        assert_eq!(arm.name(), "replay.at");
        for k in 0..2 {
            let before = fs.log().reads.len();
            arm.sample().unwrap();
            let ticks = ticker.ticks.borrow().clone();
            let (t0, t1) = (ticks[ticks.len() - 2], ticks[ticks.len() - 1]);
            assert_eq!(t0, before, "sample {k}: a read before t0");
            let w = &fs.log().reads[t0..t1];
            assert!(
                w.iter()
                    .all(|r| r.0.starts_with("log.") && r.2 <= TAIL_READ_CHUNK),
                "sample {k}: {w:?}"
            );
            let mut lsn = tail.start;
            for (name, off, len) in w {
                let n: u64 = name.strip_prefix("log.").unwrap().parse().unwrap();
                assert_eq!(geo.extent_start(n) + off, lsn, "sample {k}: in log order");
                lsn += *len as u64;
            }
            assert_eq!(
                lsn, tail.end,
                "sample {k}: the whole tail between the ticks"
            );
        }
    }

    /// `replay.at` refuses an overlay without offsets (u20 cut D-2, R-29; addendum m-5): [`offsets_of`] gives a plain
    /// replay's overlay of an A1 tail the error "an overlay without offsets" and the offsets-mode overlay its entries, so
    /// a `replay.at` whose op ran the plain replay fails its sample.
    #[test]
    fn replay_at_arm_refuses_an_overlay_without_offsets() {
        let mut t = crate::tail::synth::tests::a1(crate::tail::synth::tests::test_target(
            Clause::Ops,
            1_000,
            5,
        ));
        t.threshold = 200;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, false).unwrap();
        let plain = tail.overlay().unwrap();
        let e = offsets_of(&plain).unwrap_err();
        assert!(e.0.contains("an overlay without offsets"), "{e}");
        let at = crate::overlay::Overlay::replay_at(
            &mut crate::tail::scan::MemLog::new(tail.start, &tail.bytes),
            &tail.expect,
            tail.start,
            tail.end,
            tail.chain_in,
            tail.base,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        assert_eq!(offsets_of(&at), Ok(at.entries()));
        assert!(!at.nodes().is_empty());
    }
}
