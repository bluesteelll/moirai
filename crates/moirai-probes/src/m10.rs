//! Measurement 10 (WP-53d, [60 §5.2] item 10, `60-roadmap.md:766`): full-tail replay at the candidate checkpoint
//! thresholds, decoding every record and applying it into the F09/F11-shaped overlay, with the spec-derived lazy share
//! of [70 S8]; and the open floor it is gated against ("`HEAD` pread + map of 8 files", `60-roadmap.md:790`). It
//! decides `F17-ckpt-ops`, `-ckpt-bytes`, `-ckpt-body` (the open side), `-tail-overlay`, `-tail-overlay-quiet` and
//! `-quiet-mult` (`docs/spec/HOLES.md:41-46`; their constraints `17-store-parameters.md:757-762`).
//!
//! **The grid** (64 quantities, names by [MP §4.1]). The night's 18 are F17's candidates: `ops` at P06 4,096, 2,048 and
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
//! **The opens under load** (u20 cut §3.6, spec Goal 6; [`open_points`]), three more, on an A1 point's tail and seed:
//! `a1-load16-rec-128kib.n1e4` (16 concurrent opens, critique-performance A-2), `a1-openmod-rec-128kib.n1e4` (an open
//! after another process modified the active extent, S-1) and `a1-opens10.n1e5` (one command's 10 file opens, M-A6,
//! reported with no minimum tier). The first two run on a pool of 17 children of the driver, `m10 --opener`, held for
//! the run: 16 openers and the modifier, which never opens in a burst ([`ProcessPool`], [`serve`]). The line protocol
//! carries each open's tail parameters ([`Request`], [`Reply`]; addendum I-1). A burst is 16 opens that start at one
//! instant of the system-wide clock, each timed by its child on that clock; its values enter the arm's block in an
//! order that rotates by 7 per burst ([`burst_arm`]). The 16 values of a burst are correlated, so a p99 over 1,000
//! samples rests on about 63 bursts (addendum m-6). What no test sees: the children's open is the same code the
//! in-process tests drive through a fake pool; that the real pool's children run in their own processes rests on the
//! binary's wiring.
//!
//! **B-1** (u20 cut §3.7; `design-B.md:636`; R-50, R-53 Q1 (b); [`b1_points`]), six, before u22's block: Defender's
//! cost on a large file another process has just modified. The fixture is a `pages` file of 1 GiB or 256 MiB (1 MiB at
//! the test profile) and its 64 source pages ([`crate::tail::store::write_pages_fixture`]); at production the run waits
//! 10 s after writing it ([`B1_SETTLE`]). Arm `b1` modifies 16 pages, then bursts: the 16 openers each open the file,
//! map it whole, copy 5 of the 16 pages, drop the map and close it, timed on their own clocks from one start; its floor
//! `b1.floor` bursts with no write since the last. Every copy's hash must be the modification's source pages, so a
//! modifier that writes nothing, late or elsewhere fails the run ([`b1_arms`]). The rule, reported: p99(`b1`) −
//! p99(`b1.floor`) ≤ 1 ms at 1 GiB, for each modifier, the kill criterion for B on Windows. The modifiers
//! ([`Modifier`]) all end with `FlushFileBuffers` (u20 addendum I-2) and differ in the write path and the process: `wf`
//! is the pool's modifier child, `m10.exe`, by `WriteFile`; `psview` is a Windows PowerShell child writing through a
//! read/write view (`FlushViewOfFile`) and `pswf` the same child by `FileStream.Write`, its control ([`ps_program`],
//! [`ps_args`], [`PS_SCRIPT`]). **The confound** (R-53 Q1, R-34 B-4): the owner's Defender excludes `powershell.exe` by
//! process, so the PowerShell child's own opens are never scanned while `m10.exe`'s may be; `psview` against `pswf`
//! isolates the write mechanism and `wf` against `pswf` the exclusion. A view write may not update the file's last
//! write time, and no modifier calls `SetFileTime` (u20 cut F-16), so `psview` measures Defender's reaction to a view
//! write as it is. Every B-1 record's quantity is read with these notes. AMSI and behaviour monitoring may flag
//! `-EncodedCommand` started by an unsigned process; a detection event during the window hits R-34 M-1's log check (u20
//! addendum m-8).
//!
//! **u22's block** (R-64; R-86 ruling 3; [`rb_points`]), six last: R-b's configuration in A1 ([`Over::RB`]: P06 1,024,
//! P07 64 KiB, P08 1 MiB, P09 48 KiB, P10 128 KiB, P11 4) at 1e4, 1e5 and 1e6: `a1-rec-64kib`, at its 64-KiB record
//! cap, then `a1-cli2x-p07-64kib`, a CLI's 2× tail by [F17 §5.2]'s caps, which ends on whichever binds first: 2,048 ops,
//! 128 KiB of records, 2 MiB of bodies, or the overlay cap 48 KiB, which is never multiplied. Each production tail
//! reaches its own cap, its seed `seed(name) ^ k` as for u20's points ([`SEED_K`]: every k below 64).
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
//! `--smoke` (the requested quantity, or every one, at the test profile, each arm once, nothing recorded), `--help`,
//! `--opener` alone (a pool child: the line protocol on stdin and stdout);
//! and, beside the contract, `m10 --list` alone (one JSON line of tail measures per quantity, item 1a's locator bytes and
//! the seed's k among them). Every fixture lies below
//! the working directory ([MP §9.7] "The working directory"; R-34 B-1). Without `--quantity`, each quantity runs in a
//! child process of its own, in grid order ([MP §4.6]: `private_peak` cannot be reset). Exit 0 done, 1 a run or arm
//! failed or a record could not be written, 2 a usage error.

use crate::arm::{Arm, ArmError, BulkFn, Ticker, TimedFn};
use crate::host::{CommandRunner, HostKind};
use crate::m1::driver::{self, Args};
use crate::m14::Staged;
use crate::overlay::{Base, Overlay};
use crate::run::{Context, RunSpec, measure};
use crate::tail::scan::Expect;
use crate::tail::store::{
    Fixture, HOT_BYTES_PER_NODE, LOCK_NAME, PAGE, PAGES_NAME, SEALED_FILES, SEG_HDR_LEN, SRC_PAGES,
    VfsLog, check_seg_header, extent_name, open_floor, pages_src, remove_fixture, spare_extent,
    write_command_files, write_fixture, write_pages_fixture,
};
use crate::tail::synth::{Clause, Tail, Target, generate};
use crate::tail::{Geometry, HEAD_LEN, INIT_PARAMS_LEN, Locator, Params, TAIL_READ_CHUNK};
use crate::tier::Tier;
use moirai_vfs::{
    Access, Meter, OpenHint, RelPath, RelPathBuf, RootAccess, RootRole, SealedMap, SealedMaps,
    StoreFs, SyncKind,
};
use serde_json::json;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

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
    /// 16 concurrent opens of a tail by the pool's openers (u20 cut D-7; critique-performance A-2).
    Load,
    /// An open of a tail whose active extent another process modified since the last open (critique-performance S-1).
    OpenMod,
    /// One command's file opens (M-A6).
    Opens,
    /// B-1 (u20 cut §3.7; `design-B.md:636`): Defender's cost on a large file another process modified, read by 16
    /// processes that each open, map, read 5 pages and close it.
    B1(Modifier),
}

/// Who writes B-1's pages (u20 cut §3.7; R-53 Q1 (b), addendum I-2). All three end with `FlushFileBuffers`; they differ
/// in the write path and in the process, which the owner's Defender may exclude (`powershell.exe`, R-34 B-4).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Modifier {
    /// The pool's modifier child, `m10.exe --opener`: `write_at` per page (`WriteFile`), then `sync(DataAndMeta)`
    /// (`FlushFileBuffers`).
    Wf,
    /// The PowerShell child in view mode: a read/write view of the file (.NET `MemoryMappedFile`, `ReadWrite`), each
    /// page's `WriteArray` and the view's `Flush()` (`FlushViewOfFile`), then `FileStream.Flush($true)`
    /// (`FlushFileBuffers`).
    PsView,
    /// The PowerShell child in write mode, the control: `FileStream.Seek` and `Write` per page, then `Flush($true)`.
    PsWf,
}

impl Modifier {
    /// The name in a quantity and in `--list`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Modifier::Wf => "wf",
            Modifier::PsView => "psview",
            Modifier::PsWf => "pswf",
        }
    }

    /// Whether the PowerShell child writes.
    pub const fn is_ps(self) -> bool {
        matches!(self, Modifier::PsView | Modifier::PsWf)
    }
}

impl Kind {
    /// Whether a run of the kind needs the pool of `m10 --opener` children.
    pub const fn needs_pool(self) -> bool {
        matches!(self, Kind::Load | Kind::OpenMod | Kind::B1(_))
    }

    /// Whether a run of the kind needs the PowerShell modifier.
    pub const fn needs_ps(self) -> bool {
        matches!(self, Kind::B1(m) if m.is_ps())
    }

    /// The minimum tier: m10's t2, the tier of the open budgets; none for M-A6's opens, which are reported.
    pub const fn min_tier(self) -> Option<Tier> {
        match self {
            Kind::Opens => None,
            _ => Some(Tier::T2),
        }
    }
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

    /// R-b's values, the configuration W2 measures (R-64, R-65; R-86 ruling 3): P06 1,024, P07 64 KiB, P08 1 MiB, P09
    /// 48 KiB, P10 128 KiB, P11 4. P06 and P08 are A3's drafts, which W1's R-b table keeps.
    pub const RB: Over = Over {
        p06: Some(1_024),
        p07: Some(64 * KIB),
        p08: Some(MIB),
        p09: Some(48 * KIB),
        p10: Some(128 * KIB),
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
    /// The point whose tail the quantity opens, which names its seed; `None` is its own.
    pub of: Option<&'static str>,
    /// B-1's pages file at production, in bytes; 0 for the other kinds.
    pub pages: u64,
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
/// (R-40 O-3, R-42 Q1 and Q3); then u20's blocks, appended (u20 cut §3.10): design-A A3's small tails in A1 (§3.5), the
/// opens and B-1; then u22's six at R-b's values ([`rb_points`]; R-86 ruling 3), appended.
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
            of: None,
            pages: 0,
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
    lead.extend(open_points());
    lead.extend(b1_points());
    lead.extend(rb_points());
    lead
}

/// B-1's pages at the test profile.
pub const TEST_PAGES: u64 = MIB;
/// The wait after B-1's fixture is written at production, before modification 0 and the discarded burst (u20 addendum
/// m-11): Defender's scan on close of a file `m10.exe` wrote may still run.
pub const B1_SETTLE: Duration = Duration::from_secs(10);
/// The pages a B-1 modification writes.
pub const B1_MODIFIED: usize = 16;
/// The pages a B-1 reader copies.
pub const B1_READ: usize = 5;

/// B-1's six quantities (u20 cut §3.7; R-53 Q1 (b)): the pages file at 1 GiB, which decides the rule, then at 256 MiB,
/// each written by the PowerShell child's view, the pool's modifier child by `WriteFile`, and the PowerShell child by
/// `WriteFile`, in that order. Each holds `a1-rec-128kib.n1e4`'s target for its seed's name; its fixture is the pages
/// alone. Minimum tier t2.
pub fn b1_points() -> Vec<Quantity> {
    let base = a3_points()
        .into_iter()
        .find(|q| q.name == "a1-rec-128kib.n1e4")
        .expect("an A3 point");
    let mut v = Vec::new();
    for (bytes, size) in [(1_024 * MIB, "1gib"), (256 * MIB, "256mib")] {
        for m in [Modifier::PsView, Modifier::Wf, Modifier::PsWf] {
            let mut q = base.clone();
            q.name = format!("b1-{}.{size}", m.as_str());
            q.kind = Kind::B1(m);
            q.of = Some("a1-rec-128kib.n1e4");
            q.pages = bytes;
            v.push(q);
        }
    }
    v
}

/// B-1's pages at `profile`.
pub fn pages_len(q: &Quantity, profile: Profile) -> u64 {
    match profile {
        Profile::Production => q.pages,
        Profile::Test => TEST_PAGES,
    }
}

/// The opens under load and Defender (u20 cut §3.6, spec Goal 6), each on an A1 point's tail and seed:
/// - `a1-load16-rec-128kib.n1e4`: 16 concurrent opens of `a1-rec-128kib.n1e4`'s tail, the A3 point expected to meet
///   1e4 (`design-A.md:266-269`, est. 0.67 ms), one arm `open.c16` (D-7);
/// - `a1-openmod-rec-128kib.n1e4`: an open of the same tail after another process modified its active extent, and its
///   floor, the same open with no write since the last;
/// - `a1-opens10.n1e5`: one command's 10 file opens on `a1-rec-128kib.n1e5`'s fixture with `LOCK` and the spare extent
///   (M-A6), reported, no gate.
pub fn open_points() -> Vec<Quantity> {
    let a3 = a3_points();
    let of = |name: &'static str, new: &str, kind: Kind| {
        let mut q = a3
            .iter()
            .find(|q| q.name == name)
            .expect("an A3 point")
            .clone();
        q.name = new.into();
        q.kind = kind;
        q.of = Some(name);
        q
    };
    vec![
        of(
            "a1-rec-128kib.n1e4",
            "a1-load16-rec-128kib.n1e4",
            Kind::Load,
        ),
        of(
            "a1-rec-128kib.n1e4",
            "a1-openmod-rec-128kib.n1e4",
            Kind::OpenMod,
        ),
        of("a1-rec-128kib.n1e5", "a1-opens10.n1e5", Kind::Opens),
    ]
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
/// | `a1-cli2x-128kib` | the quiet machinery with q = P13 = 2 and P10 := 2 × P09 = 128 KiB: 2,048 ops, 256 KiB of records, 2 MiB of bodies, 128 KiB of overlay, whichever binds first. Label (R-86 ruling 3): its overlay cap 2 × P09 departs from [F17 §5.2], which never multiplies P09; W1's tails of it ended on their record caps with overlays of 44,068 B (1e4) and 64,316 B (1e5), under A3's unmultiplied 64 KiB, and the 1e6 run past it is set aside (R-64), so the label changes no W1 number that decides anything | `design-A.md:152` |
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
            of: None,
            pages: 0,
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

/// R-64's P07 = 64 KiB points (R-86 ruling 3), u22's block of six after B-1, in A1 at R-b's values ([`Over::RB`]): at
/// 1e4, 1e5 and 1e6, the single point `a1-rec-64kib` (`rec`, its record cap 64 KiB) and then the CLI point
/// `a1-cli2x-p07-64kib`, named by the P07 under test, which sets its 2 × P07 record cap. The CLI point is the quiet
/// machinery with q = P13 = 2 and [`Over::RB`] unchanged, and its `threshold` is the overlay cap P09 = 48 KiB: its tail
/// ends on whichever of [F17 §5.2]'s CLI caps binds first, 2,048 ops, 128 KiB of records, 2 MiB of bodies, or the
/// overlay cap 48 KiB, which is never multiplied ([`reaches_cap`] picks the binding one). Each production seed is
/// `seed(name) ^ seed_k` ([`SEED_K`]). At the test profile [`target`] gives them the test column's caps, as for u20's
/// points.
pub fn rb_points() -> Vec<Quantity> {
    let mut g = Vec::new();
    for n in [10_000, 100_000, 1_000_000] {
        for (clause, threshold, mult, stem) in [
            (Clause::Rec, 64 * KIB, 1, "rec-64kib"),
            (Clause::Quiet, 48 * KIB, 2, "cli2x-p07-64kib"),
        ] {
            let name = format!("a1-{stem}.{}", scale(n));
            g.push(Quantity {
                seed_k: SEED_K
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map_or(0, |(_, k)| *k),
                name,
                kind: Kind::Tail,
                clause,
                threshold,
                mult,
                n,
                over: Over::RB,
                form: Locator::A1,
                of: None,
                pages: 0,
            });
        }
    }
    g
}

/// The production k of each A1 point, the first k < 64 whose tail reaches its cap ([`reaches_cap`]; u20 cut D-5),
/// recomputed by `a1_tails_reach_their_caps_and_stop_on_their_clauses`. A 128-KiB `rec` tail of the mix reaches 95 %
/// of its cap for few seeds: one commit in ten is log-uniform up to P05 = 1 MiB, so the refused group is often large.
/// u20's 18 rows come first, then u22's six ([`rb_points`]). The bound 64 may rise only for one of u22's rows that has
/// no k < 64, to that row's first k that reaches its cap (R-87 item 7); none needed it: their k are all below 64.
pub const SEED_K: [(&str, u64); 24] = [
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
    ("a1-rec-64kib.n1e4", 0),
    ("a1-cli2x-p07-64kib.n1e4", 9),
    ("a1-rec-64kib.n1e5", 19),
    ("a1-cli2x-p07-64kib.n1e5", 16),
    ("a1-rec-64kib.n1e6", 0),
    ("a1-cli2x-p07-64kib.n1e6", 56),
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
        seed: seed(q.of.unwrap_or(&q.name)) ^ k,
        params: p,
        rt_bound: None,
    }
}

/// Whether `t`'s tail reaches the cap it targets (u20 cut D-5; addendum m-4): it stops on that clause, the refused group
/// passes the cap, and the measure is at least 95 % of it and at most the cap (a first commit past the cap, which a tail
/// always holds, is not a tail at its cap: R-44 I-2). A `rec` or `ovl` point targets its threshold; the quiet
/// points their record cap P11 × P07; a quantity whose `name` holds `cli2x` the binding quiet cap, whichever it is.
/// Returns the clause and the measure's share of the cap in per mille. It reads the target's clause and the quantity's
/// name only, so m14's full checkpoint at its record caps shares it (R-87 item 5).
pub fn reaches_cap(name: &str, t: &Target, tail: &Tail) -> (bool, &'static str, u64) {
    let p = t.params;
    let stop = binding_clause(t, tail);
    let (clause, cap) = match (t.clause, name.contains("cli2x")) {
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

/// The children a run's arms drive (u20 cut §3.6, §3.7): the pool of openers and the modifier, B-1's PowerShell
/// modifier, and the system-wide clock they share.
pub struct Children<'a> {
    /// The pool, for the kinds that need it ([`Kind::needs_pool`]).
    pub pool: Option<&'a mut (dyn Pool + 'a)>,
    /// The PowerShell modifier, for the kinds that need it ([`Kind::needs_ps`]).
    pub ps: Option<&'a mut (dyn Pool + 'a)>,
    /// The system-wide monotonic clock.
    pub mono: &'a dyn Fn() -> u64,
}

/// The arms of `q` over a written fixture, arm 0 first: a tail's `[replay.at, replay, floor.open]` for an A1 point (u20
/// cut §3.5), the night's `[replay, floor.open]`; `open.c16` over the pool; `open.mod` and its floor `open`; `open.10`
/// and its floor `open.1`; B-1's `b1` and its floor `b1.floor`.
pub fn quantity_arms<'a, V: StoreFs + SealedMaps, T: Ticker + Clone + 'a>(
    q: &Quantity,
    vfs: &'a V,
    root: &'a V::Root,
    fx: &'a Fixture,
    tail: &'a Tail,
    ticker: &T,
    kids: Children<'a>,
) -> Result<Vec<Box<dyn Arm + 'a>>, String> {
    let Children { pool, ps, mono } = kids;
    let mut v: Vec<Box<dyn Arm + 'a>> = Vec::new();
    match q.kind {
        Kind::Tail => {
            let (replay, floor) = arms(vfs, root, fx, tail, ticker);
            if q.form == Locator::A1 {
                v.push(Box::new(replay_at_arm(vfs, root, fx, tail, ticker)));
            }
            v.push(Box::new(replay));
            v.push(Box::new(floor));
        }
        Kind::Load => {
            let pool = pool.ok_or("the load quantity needs the pool")?;
            v.push(Box::new(burst_arm(
                pool,
                mono,
                OpenSpec::of(tail),
                fx.dir.clone(),
            )));
        }
        Kind::OpenMod => {
            let pool = pool.ok_or("the open-after-modify quantity needs the pool")?;
            v.extend(openmod_arms(vfs, root, fx, tail, ticker, pool)?);
        }
        Kind::Opens => {
            v.push(Box::new(TimedFn::new(
                "open.10",
                ticker.clone(),
                move || command_opens(vfs, root, fx, tail).map_err(ArmError),
            )));
            v.push(Box::new(TimedFn::new(
                "open.1",
                ticker.clone(),
                move || head_open(vfs, root).map_err(ArmError),
            )));
        }
        Kind::B1(m) => {
            let readers = pool.ok_or("B-1 needs the pool")?;
            let ps = match m.is_ps() {
                true => Some(ps.ok_or("B-1's PowerShell modifier is not started")?),
                false => None,
            };
            let f = vfs
                .open(
                    root,
                    RelPath::literal(PAGES_NAME),
                    Access::Read,
                    OpenHint::Normal,
                )
                .map_err(|e| e.to_string())?;
            let len = vfs.file_size(&f).map_err(|e| e.to_string())?;
            drop(f);
            v.extend(b1_arms(
                readers,
                ps,
                mono,
                fx.dir.join(PAGES_NAME),
                len,
                seed(&q.name),
            )?);
        }
    }
    Ok(v)
}

/// SplitMix64's step: B-1's draws from the run's seed.
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// B-1's modification j (u20 cut §3.7): 16 distinct page-aligned offsets in `[0, len)` drawn from the run's seed and j,
/// and the source rotation k = 16·j mod 64: the page at offset i gets source page (k + i) mod 64, so consecutive
/// modifications write different bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Modification {
    /// The modification's number.
    pub j: u64,
    /// The source rotation.
    pub k: u64,
    /// The 16 page offsets.
    pub offsets: [u64; B1_MODIFIED],
}

impl Modification {
    /// Modification `j` of a file of `len` bytes (at least 16 pages) under `seed`.
    pub fn of(seed: u64, j: u64, len: u64) -> Modification {
        let pages = (len / PAGE).max(1);
        let mut offsets = [0u64; B1_MODIFIED];
        let (mut n, mut x) = (0, 0u64);
        while n < B1_MODIFIED {
            let o = mix(seed ^ (j << 32) ^ x) % pages * PAGE;
            x += 1;
            if !offsets[..n].contains(&o) {
                offsets[n] = o;
                n += 1;
            }
        }
        Modification {
            j,
            k: (B1_MODIFIED as u64 * j) % SRC_PAGES,
            offsets,
        }
    }

    /// Reader `r`'s 5 distinct positions among the 16 pages in burst `burst`, drawn from the seed.
    pub fn reads(&self, seed: u64, burst: u64, r: usize) -> [usize; B1_READ] {
        let mut pos = [0usize; B1_READ];
        let (mut n, mut x) = (0, 0u64);
        while n < B1_READ {
            let p =
                (mix(!seed ^ (burst << 24) ^ ((r as u64) << 16) ^ x) % B1_MODIFIED as u64) as usize;
            x += 1;
            if !pos[..n].contains(&p) {
                pos[n] = p;
                n += 1;
            }
        }
        pos
    }

    /// The xxh3-64 a reader's copy of the pages at `pos` must hash to: the source pages this modification wrote there.
    pub fn model(&self, src: &[u8], pos: &[usize; B1_READ]) -> u64 {
        let mut v = Vec::with_capacity(B1_READ * PAGE as usize);
        for &p in pos {
            let s = (((self.k + p as u64) % SRC_PAGES) * PAGE) as usize;
            v.extend_from_slice(&src[s..s + PAGE as usize]);
        }
        xxhash_rust::xxh3::xxh3_64(&v)
    }
}

/// What B-1's two arms share for the run: the readers' pool, the PowerShell modifier (else the pool's modifier writes),
/// the pages file, the source pages, the last modification and the counts.
struct B1Run<'a> {
    readers: &'a mut (dyn Pool + 'a),
    ps: Option<&'a mut (dyn Pool + 'a)>,
    mono: &'a dyn Fn() -> u64,
    path: PathBuf,
    len: u64,
    seed: u64,
    src: Vec<u8>,
    last: Option<Modification>,
    next: u64,
    bursts: u64,
}

impl B1Run<'_> {
    /// The next modification: the modifier writes its pages, flushes them to disk and closes the file, and only then
    /// replies.
    fn modify(&mut self) -> Result<(), ArmError> {
        let m = Modification::of(self.seed, self.next, self.len);
        self.next += 1;
        let line = Request::Pages {
            k: m.k,
            offsets: m.offsets,
            path: self.path.clone(),
        }
        .line();
        let (pool, child): (&mut dyn Pool, usize) = match self.ps.as_deref_mut() {
            Some(p) => (p, 0),
            None => (&mut *self.readers, MODIFIER),
        };
        pool.send(child, &line).map_err(ArmError)?;
        match pool.recv(REPLY_TIMEOUT).map_err(ArmError)? {
            (i, Reply::Done { op, .. }) if i == child && op == "pages" => {}
            other => return Err(ArmError(format!("B-1's modifier: {other:?}"))),
        }
        self.last = Some(m);
        Ok(())
    }

    /// One burst over the last modification: one start for the 16 readers, each copying 5 of its pages; every copy's
    /// hash must be the modification's model. The values enter in an order rotating by 7 per burst (addendum m-6).
    fn burst(&mut self) -> Result<[u64; OPENERS], ArmError> {
        let m = self
            .last
            .clone()
            .ok_or_else(|| ArmError("a B-1 burst before any modification".into()))?;
        let start_ns = (self.mono)() + START_MARGIN_NS;
        let mut want = [0u64; OPENERS];
        for (r, w) in want.iter_mut().enumerate() {
            let pos = m.reads(self.seed, self.bursts, r);
            *w = m.model(&self.src, &pos);
            let line = Request::B1 {
                start_ns,
                len: self.len,
                offsets: pos.map(|p| m.offsets[p]),
                path: self.path.clone(),
            }
            .line();
            self.readers.send(r, &line).map_err(ArmError)?;
        }
        let mut times = [None; OPENERS];
        for _ in 0..OPENERS {
            let (i, rep) = self.readers.recv(REPLY_TIMEOUT).map_err(ArmError)?;
            match rep {
                Reply::Done {
                    op,
                    t0,
                    t1,
                    h: Some(h),
                } if op == "b1" && i < OPENERS && times[i].is_none() && t0 >= start_ns => {
                    if h != want[i] {
                        return Err(ArmError(format!(
                            "B-1 reader {i}: a hash mismatch, {h:#018x} where modification {}'s pages give {:#018x}",
                            m.j, want[i]
                        )));
                    }
                    times[i] = Some(t1.saturating_sub(t0));
                }
                other => return Err(ArmError(format!("B-1 reader {i}: {other:?}"))),
            }
        }
        let rot = ((self.bursts * 7) % OPENERS as u64) as usize;
        self.bursts += 1;
        let mut out = [0u64; OPENERS];
        for (j, o) in out.iter_mut().enumerate() {
            *o = times[(rot + j) % OPENERS].expect("16 replies");
        }
        Ok(out)
    }
}

/// A block of `len` B-1 values: bursts, each after a modification when `write`.
fn b1_block(
    run: &std::cell::RefCell<B1Run<'_>>,
    len: u32,
    write: bool,
) -> Result<Vec<u64>, ArmError> {
    let mut r = run.borrow_mut();
    let mut v = Vec::with_capacity(len as usize);
    while v.len() < len as usize {
        if write {
            r.modify()?;
        }
        v.extend(r.burst()?);
    }
    v.truncate(len as usize);
    Ok(v)
}

/// B-1's arms (u20 cut §3.7; `design-B.md:636`): `b1`, whose bursts each follow a modification of 16 pages by the
/// modifier (the pool's modifier child for `wf`, the PowerShell child otherwise), and its floor `b1.floor`, the same burst
/// with no write since the last. The setup makes modification 0 and one discarded burst before the pilot, so a floor
/// burst always follows a burst and there is always a last modification. The driver never opens the pages file between
/// a modification and a burst. What no test sees: a reader's page copies are memory reads, so their place between its
/// `t0` and `t1` rests on the code.
pub fn b1_arms<'a>(
    readers: &'a mut (dyn Pool + 'a),
    ps: Option<&'a mut (dyn Pool + 'a)>,
    mono: &'a dyn Fn() -> u64,
    path: PathBuf,
    len: u64,
    seed: u64,
) -> Result<Vec<Box<dyn Arm + 'a>>, String> {
    if len < B1_MODIFIED as u64 * PAGE {
        return Err(format!(
            "a pages file of {len} bytes holds fewer than 16 pages"
        ));
    }
    let run = std::rc::Rc::new(std::cell::RefCell::new(B1Run {
        readers,
        ps,
        mono,
        path,
        len,
        seed,
        src: pages_src(seed),
        last: None,
        next: 0,
        bursts: 0,
    }));
    {
        let mut r = run.borrow_mut();
        r.modify().map_err(|e| e.0)?;
        r.burst().map_err(|e| e.0)?;
    }
    let floor = run.clone();
    Ok(vec![
        Box::new(BulkFn::new("b1", move |len: u32| b1_block(&run, len, true))),
        Box::new(BulkFn::new("b1.floor", move |len: u32| {
            b1_block(&floor, len, false)
        })),
    ])
}

/// The PowerShell modifier's program (u20 cut §3.7; R-53 Q1 (b)): Windows PowerShell 5.1 below `%SystemRoot%`,
/// resolved as [`crate::host::DEFENDER_COMMAND`] resolves it, never from `PATH` and never `pwsh.exe`.
pub const PS_PROGRAM: crate::host::SystemCommand = crate::host::SystemCommand {
    path: crate::host::DEFENDER_COMMAND.path,
    args: &[],
};

/// The PowerShell modifier's program below `system_root` (the value of `%SystemRoot%`).
pub fn ps_program(system_root: Option<&std::ffi::OsStr>) -> Result<PathBuf, String> {
    PS_PROGRAM.resolve(system_root)
}

/// The PowerShell modifier's script (u20 cut §3.7), for Windows PowerShell 5.1 on .NET Framework 4.x, after the line that
/// sets `$mode` ([`ps_script`]). It serves `pages` lines on stdin, one reply each on stdout, until `quit` or the end of
/// input. Each line opens the file with `FileStream` (read and write, sharing both) and, in view mode, writes each page
/// through a 4-KiB view of a read/write `MemoryMappedFile` made over that stream (`CreateFromFile(FileStream, String,
/// Int64, MemoryMappedFileAccess, MemoryMappedFileSecurity, HandleInheritability, Boolean)`, an unnamed map: PowerShell
/// passes `[NullString]::Value`, since its `$null` becomes an empty name; the stream left open), the view's `Flush()`
/// being `FlushViewOfFile`; in `wf` mode by `Seek` and `Write`. The script calls no cmdlet, so no module loads. Both end with `Flush($true)`
/// (`FlushFileBuffers`), so the data is on disk before the reply. No `SetFileTime`: a view write may not update the last
/// write time (u20 cut F-16). Any exception replies `err`.
pub const PS_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$src = $null
$srcPath = $null
while ($true) {
  $line = [Console]::In.ReadLine()
  if ($line -eq $null -or $line -eq 'quit') { break }
  try {
    $w = $line.Split(' ', 19)
    if ($w.Length -ne 19 -or $w[0] -ne 'pages') { throw 'not a pages line' }
    $k = [int64]$w[1]
    $path = $w[18]
    if ($srcPath -ne ($path + '.src')) {
      $srcPath = $path + '.src'
      $src = [IO.File]::ReadAllBytes($srcPath)
    }
    $fs = [IO.FileStream]::new($path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::ReadWrite)
    try {
      if ($mode -eq 'view') {
        $m = [IO.MemoryMappedFiles.MemoryMappedFile]::CreateFromFile($fs, [NullString]::Value, 0, [IO.MemoryMappedFiles.MemoryMappedFileAccess]::ReadWrite, $null, [IO.HandleInheritability]::None, $true)
        try {
          for ($i = 0; $i -lt 16; $i++) {
            $o = [int64]$w[2 + $i]
            $s = [int](($k + $i) % 64) * 4096
            $v = $m.CreateViewAccessor($o, 4096, [IO.MemoryMappedFiles.MemoryMappedFileAccess]::ReadWrite)
            try {
              $v.WriteArray([int64]0, $src, $s, 4096)
              $v.Flush()
            } finally { $v.Dispose() }
          }
        } finally { $m.Dispose() }
      } else {
        for ($i = 0; $i -lt 16; $i++) {
          $o = [int64]$w[2 + $i]
          $s = [int](($k + $i) % 64) * 4096
          [void]$fs.Seek($o, [IO.SeekOrigin]::Begin)
          $fs.Write($src, $s, 4096)
        }
      }
      $fs.Flush($true)
    } finally { $fs.Dispose() }
    [Console]::Out.WriteLine('done pages 0 0')
  } catch {
    [Console]::Out.WriteLine('err pages ' + ($_.Exception.Message -replace '\s+', ' '))
  }
  [Console]::Out.Flush()
}
"#;

/// The script with its mode line: `$mode = 'view'` for [`Modifier::PsView`], `$mode = 'wf'` otherwise.
pub fn ps_script(m: Modifier) -> String {
    let mode = match m {
        Modifier::PsView => "view",
        _ => "wf",
    };
    format!("$mode = '{mode}'\n{PS_SCRIPT}")
}

/// The PowerShell modifier's arguments: `-NoLogo -NoProfile -NonInteractive -EncodedCommand <b64>`, `<b64>` the base64
/// of the UTF-16LE script ([`ps_script`]).
pub fn ps_args(m: Modifier) -> Vec<String> {
    let utf16: Vec<u8> = ps_script(m)
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-EncodedCommand".into(),
        base64(&utf16),
    ]
}

/// Base64 with padding (RFC 4648 §4), for `-EncodedCommand`.
pub fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let at = |i: usize| u32::from(c.get(i).copied().unwrap_or(0));
        let n = (at(0) << 16) | (at(1) << 8) | at(2);
        s.push(char::from(A[(n >> 18) as usize & 63]));
        s.push(char::from(A[(n >> 12) as usize & 63]));
        s.push(if c.len() > 1 {
            char::from(A[(n >> 6) as usize & 63])
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            char::from(A[n as usize & 63])
        } else {
            '='
        });
    }
    s
}

/// The openers of a burst (u20 cut D-7: 16 sessions' CLI opens, critique-performance A-2).
pub const OPENERS: usize = 16;
/// The pool's size: the 16 openers and the modifier, which never opens in a burst, so a process that wrote the file is
/// never one that is timed opening it.
pub const POOL: usize = OPENERS + 1;
/// The modifier's index in the pool.
pub const MODIFIER: usize = OPENERS;
/// How far ahead of the system-wide clock a burst starts: room to send its 16 lines.
pub const START_MARGIN_NS: u64 = 20_000_000;
/// How long the driver waits for one child's reply.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
/// The bytes the open-after-modify's modifier writes past the tail's end in its active extent.
pub const MOD_BYTES: u64 = 4_096;
/// The sealed files one command maps (M-A6: `HEAD`, `LOCK`, two extents and 6 segment files make 10).
pub const COMMAND_SEALED: usize = 6;

/// What an opener opens (u20 addendum I-1): the tail's scan parameters, which the fixture's `HEAD` does not hold, and its
/// base.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OpenSpec {
    /// What the scan checks the tail against, the form included.
    pub expect: Expect,
    /// `HEAD.checkpoint_lsn`, the scan's start.
    pub from: u64,
    /// `HEAD.committed_lsn`, its end.
    pub to: u64,
    /// The chain value at `from`.
    pub chain_in: u64,
    /// The base the tail applies to.
    pub base: Base,
}

/// The words of an [`OpenSpec`] in a line.
const SPEC_WORDS: usize = 12;

impl OpenSpec {
    /// The open of `tail`.
    pub fn of(tail: &Tail) -> OpenSpec {
        OpenSpec {
            expect: tail.expect,
            from: tail.start,
            to: tail.end,
            chain_in: tail.chain_in,
            base: tail.base,
        }
    }

    fn words(&self) -> String {
        let x = &self.expect;
        let init: String = x.init.iter().map(|b| format!("{b:02x}")).collect();
        let form = match x.locator {
            Locator::None => 0,
            Locator::A1 => 1,
        };
        format!(
            "{} {} {} {init} {} {form} {} {} {} {} {} {}",
            x.geo.e(),
            x.epoch,
            x.epoch_lsn,
            x.project_oid_algo,
            self.from,
            self.to,
            self.chain_in,
            self.base.n,
            self.base.kind,
            self.base.statuses
        )
    }

    fn parse(w: &[&str]) -> Result<OpenSpec, String> {
        let bad = |what: &str| format!("an open spec's {what}");
        let num = |i: usize, what: &str| -> Result<u64, String> {
            w.get(i)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| bad(what))
        };
        let hex = w.get(3).ok_or_else(|| bad("init"))?;
        if hex.len() != 2 * INIT_PARAMS_LEN {
            return Err(bad("init"));
        }
        let mut init = [0u8; INIT_PARAMS_LEN];
        for (i, b) in init.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|_| bad("init"))?;
        }
        let small = |i: usize, what: &str| -> Result<u8, String> {
            u8::try_from(num(i, what)?).map_err(|_| bad(what))
        };
        Ok(OpenSpec {
            expect: Expect {
                geo: Geometry::new(num(0, "E")?).ok_or_else(|| bad("E"))?,
                epoch: num(1, "epoch")?,
                epoch_lsn: num(2, "epoch_lsn")?,
                init,
                project_oid_algo: small(4, "project_oid_algo")?,
                locator: match num(5, "form")? {
                    0 => Locator::None,
                    1 => Locator::A1,
                    _ => return Err(bad("form")),
                },
            },
            from: num(6, "from")?,
            to: num(7, "to")?,
            chain_in: num(8, "chain_in")?,
            base: Base {
                n: u32::try_from(num(9, "n")?).map_err(|_| bad("n"))?,
                kind: small(10, "kind")?,
                statuses: small(11, "statuses")?,
            },
        })
    }
}

/// A request to a pool child (u20 cut §3.6): one ASCII line; a path is the rest of its line, so it may hold spaces
/// (`P/m2/child.rs:4`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// `open <start_ns> <spec> <dir>`: at `start_ns` on the system-wide clock, `floor.open` and `replay.at` of the
    /// store in `dir`, timed on the child's own clock.
    Open {
        /// The burst's start.
        start_ns: u64,
        /// The tail's scan parameters.
        spec: OpenSpec,
        /// The store.
        dir: PathBuf,
    },
    /// `zero <from> <len> <path>`: `len` zero bytes at `from` of the file, flushed (`sync(Data)`), the file closed.
    Zero {
        /// The offset.
        from: u64,
        /// The bytes.
        len: u64,
        /// The file.
        path: PathBuf,
    },
    /// `pages <k> <o_1> … <o_16> <path>`: B-1's modification (u20 cut §3.7): the page at `o_i` gets source page
    /// (k + i) mod 64 of `<path>.src`, which the child loads on its first pages line; then the file is flushed to disk
    /// (`sync(DataAndMeta)`, addendum I-2) and closed.
    Pages {
        /// The source rotation.
        k: u64,
        /// The 16 page offsets.
        offsets: [u64; B1_MODIFIED],
        /// The pages file.
        path: PathBuf,
    },
    /// `b1 <start_ns> <len> <o_1> … <o_5> <path>`: B-1's reader: at `start_ns` on the system-wide clock, open the file,
    /// map its `len` bytes, copy the 5 pages, drop the map and close, timed on the child's clock; the reply's value is
    /// the copy's xxh3-64, computed after `t1`.
    B1 {
        /// The burst's start.
        start_ns: u64,
        /// The file's length.
        len: u64,
        /// The 5 page offsets.
        offsets: [u64; B1_READ],
        /// The pages file.
        path: PathBuf,
    },
    /// `quit`: the child exits 0.
    Quit,
}

impl Request {
    /// The line, without its line feed.
    pub fn line(&self) -> String {
        match self {
            Request::Open {
                start_ns,
                spec,
                dir,
            } => format!("open {start_ns} {} {}", spec.words(), dir.to_string_lossy()),
            Request::Zero { from, len, path } => {
                format!("zero {from} {len} {}", path.to_string_lossy())
            }
            Request::Pages { k, offsets, path } => {
                format!("pages {k} {} {}", words(offsets), path.to_string_lossy())
            }
            Request::B1 {
                start_ns,
                len,
                offsets,
                path,
            } => format!(
                "b1 {start_ns} {len} {} {}",
                words(offsets),
                path.to_string_lossy()
            ),
            Request::Quit => "quit".into(),
        }
    }

    /// Parses a request line.
    pub fn parse(line: &str) -> Result<Request, String> {
        let bad = || format!("not a request line: '{line}'");
        let (op, rest) = line.split_once(' ').unwrap_or((line, ""));
        let path = |s: Option<&str>| -> Result<PathBuf, String> {
            s.filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .ok_or_else(bad)
        };
        match op {
            "open" => {
                let w: Vec<&str> = rest.splitn(SPEC_WORDS + 2, ' ').collect();
                if w.len() != SPEC_WORDS + 2 {
                    return Err(bad());
                }
                Ok(Request::Open {
                    start_ns: w[0].parse().map_err(|_| bad())?,
                    spec: OpenSpec::parse(&w[1..=SPEC_WORDS])?,
                    dir: path(Some(w[SPEC_WORDS + 1]))?,
                })
            }
            "zero" => {
                let w: Vec<&str> = rest.splitn(3, ' ').collect();
                let num = |i: usize| w.get(i).and_then(|s| s.parse().ok()).ok_or_else(bad);
                Ok(Request::Zero {
                    from: num(0)?,
                    len: num(1)?,
                    path: path(w.get(2).copied())?,
                })
            }
            "pages" => {
                let w: Vec<&str> = rest.splitn(B1_MODIFIED + 2, ' ').collect();
                let num = |i: usize| w.get(i).and_then(|s| s.parse().ok()).ok_or_else(bad);
                let mut offsets = [0u64; B1_MODIFIED];
                for (i, o) in offsets.iter_mut().enumerate() {
                    *o = num(1 + i)?;
                }
                Ok(Request::Pages {
                    k: num(0)?,
                    offsets,
                    path: path(w.get(B1_MODIFIED + 1).copied())?,
                })
            }
            "b1" => {
                let w: Vec<&str> = rest.splitn(B1_READ + 3, ' ').collect();
                let num = |i: usize| w.get(i).and_then(|s| s.parse().ok()).ok_or_else(bad);
                let mut offsets = [0u64; B1_READ];
                for (i, o) in offsets.iter_mut().enumerate() {
                    *o = num(2 + i)?;
                }
                Ok(Request::B1 {
                    start_ns: num(0)?,
                    len: num(1)?,
                    offsets,
                    path: path(w.get(B1_READ + 2).copied())?,
                })
            }
            "quit" if rest.is_empty() => Ok(Request::Quit),
            _ => Err(bad()),
        }
    }
}

/// A child's reply: `done <op> <t0> <t1> [<h>]` or `err <op> <message>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reply {
    /// The request was served: its start and end on the child's clock, and its value (an open's entries).
    Done {
        /// The request's first word.
        op: String,
        /// Its start.
        t0: u64,
        /// Its end.
        t1: u64,
        /// Its value, when it has one.
        h: Option<u64>,
    },
    /// It failed.
    Err {
        /// The request's first word.
        op: String,
        /// Why, on one line.
        message: String,
    },
}

impl Reply {
    /// The line, without its line feed.
    pub fn line(&self) -> String {
        match self {
            Reply::Done { op, t0, t1, h } => match h {
                Some(h) => format!("done {op} {t0} {t1} {h}"),
                None => format!("done {op} {t0} {t1}"),
            },
            Reply::Err { op, message } => {
                format!("err {op} {}", message.replace(['\r', '\n'], " "))
            }
        }
    }

    /// Parses a reply line.
    pub fn parse(line: &str) -> Result<Reply, String> {
        let bad = || format!("not a reply line: '{line}'");
        let w: Vec<&str> = line.splitn(3, ' ').collect();
        match w.as_slice() {
            ["err", op, message] => Ok(Reply::Err {
                op: op.to_string(),
                message: message.to_string(),
            }),
            ["done", op, rest] => {
                let n: Vec<u64> = rest
                    .split(' ')
                    .map(|s| s.parse().map_err(|_| bad()))
                    .collect::<Result<_, _>>()?;
                match n.as_slice() {
                    [t0, t1] => Ok(Reply::Done {
                        op: op.to_string(),
                        t0: *t0,
                        t1: *t1,
                        h: None,
                    }),
                    [t0, t1, h] => Ok(Reply::Done {
                        op: op.to_string(),
                        t0: *t0,
                        t1: *t1,
                        h: Some(*h),
                    }),
                    _ => Err(bad()),
                }
            }
            _ => Err(bad()),
        }
    }
}

/// A pool of children that serve [`Request`] lines, one reply each.
pub trait Pool {
    /// The children.
    fn count(&self) -> usize;
    /// Sends `line` to child `i`.
    fn send(&mut self, i: usize, line: &str) -> Result<(), String>;
    /// The next reply of any child, within `timeout`.
    fn recv(&mut self, timeout: Duration) -> Result<(usize, Reply), String>;
}

/// The pool of `<exe> <args>` children, as measurement 2's writers (`P/m2/child.rs:272-348`; u20 cut F-15): piped stdin
/// and stdout, one reader thread per child forwarding its lines, stderr inherited. Dropping it sends `quit`, closes the
/// pipes, waits up to 5 s for each child and kills the rest.
pub struct ProcessPool {
    children: Vec<Child>,
    stdins: Vec<ChildStdin>,
    rx: Receiver<(usize, Result<String, String>)>,
}

impl ProcessPool {
    /// Starts `n` children of `exe` with `args` in `cwd`.
    pub fn spawn(exe: &Path, args: &[&str], n: usize, cwd: &Path) -> Result<ProcessPool, String> {
        let (tx, rx) = channel();
        let mut pool = ProcessPool {
            children: Vec::with_capacity(n),
            stdins: Vec::with_capacity(n),
            rx,
        };
        for i in 0..n {
            let mut child = Command::new(exe)
                .args(args)
                .current_dir(cwd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| format!("starting pool child {i}: {e}"))?;
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("pool child {i} has no pipes"));
            };
            pool.children.push(child);
            pool.stdins.push(stdin);
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let msg = line.map_err(|e| e.to_string());
                    let end = msg.is_err();
                    if tx.send((i, msg)).is_err() || end {
                        return;
                    }
                }
                let _ = tx.send((i, Err("the child closed its output".into())));
            });
        }
        Ok(pool)
    }
}

impl Pool for ProcessPool {
    fn count(&self) -> usize {
        self.children.len()
    }

    fn send(&mut self, i: usize, line: &str) -> Result<(), String> {
        let s = self
            .stdins
            .get_mut(i)
            .ok_or_else(|| format!("no pool child {i}"))?;
        writeln!(s, "{line}")
            .and_then(|()| s.flush())
            .map_err(|e| format!("pool child {i}: {e}"))
    }

    fn recv(&mut self, timeout: Duration) -> Result<(usize, Reply), String> {
        match self.rx.recv_timeout(timeout) {
            Ok((i, Ok(line))) => Reply::parse(&line).map(|r| (i, r)),
            Ok((i, Err(e))) => Err(format!("pool child {i}: {e}")),
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("no reply within {} s", timeout.as_secs()))
            }
            Err(RecvTimeoutError::Disconnected) => Err("every pool child is gone".into()),
        }
    }
}

impl Drop for ProcessPool {
    fn drop(&mut self) {
        for s in &mut self.stdins {
            let _ = writeln!(s, "quit").and_then(|()| s.flush());
        }
        self.stdins.clear();
        let deadline = Instant::now() + Duration::from_secs(5);
        for c in &mut self.children {
            loop {
                match c.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => {
                        let _ = c.kill();
                        let _ = c.wait();
                        break;
                    }
                }
            }
        }
    }
}

/// The system-wide monotonic clock of a `Vfs` clock seam (`moirai_vfs::Clock::mono_ns`, comparable across the processes
/// of one boot, `docs/spec/os/clock.md`): what the binary passes as [`System::mono`].
pub fn mono_of<C: moirai_vfs::Clock>(clock: &C) -> u64 {
    clock.mono_ns()
}

/// Returns once `mono` reaches `t`: sleeping while more than 2 ms remain, then spinning.
fn wait_until(mono: &dyn Fn() -> u64, t: u64) {
    loop {
        let now = mono();
        if now >= t {
            return;
        }
        let left = t - now;
        if left > 2_000_000 {
            std::thread::sleep(Duration::from_nanos(left - 1_000_000));
        } else {
            std::hint::spin_loop();
        }
    }
}

/// The sealed files' names as relative paths.
fn sealed_rel() -> Result<Vec<RelPathBuf>, String> {
    crate::tail::store::sealed_names()
        .into_iter()
        .map(|s| RelPathBuf::from_string(s).map_err(|e| e.to_string()))
        .collect()
}

/// One open of a store (u20 cut §3.6): `floor.open` ([`open_floor`]) then `replay.at` of the tail `spec` names; returns the
/// overlay's entries, refusing an overlay without offsets.
pub fn open_once<V: StoreFs + SealedMaps>(
    vfs: &V,
    root: &V::Root,
    sealed: &[RelPathBuf],
    spec: &OpenSpec,
) -> Result<usize, String> {
    open_floor(vfs, root, sealed)?;
    let o = Overlay::replay_at(
        &mut VfsLog::new(vfs, root, spec.expect.geo),
        &spec.expect,
        spec.from,
        spec.to,
        spec.chain_in,
        spec.base,
        TAIL_READ_CHUNK,
    )
    .map_err(|e| e.to_string())?;
    offsets_of(&o).map_err(|e| e.0)
}

/// Space-separated numbers.
fn words(v: &[u64]) -> String {
    v.iter().map(u64::to_string).collect::<Vec<_>>().join(" ")
}

/// What a pool child keeps between requests: B-1's source pages, loaded on its first `pages` line.
#[derive(Debug, Default)]
pub struct ChildState {
    src: Option<(PathBuf, Vec<u8>)>,
}

/// The root of `path`'s directory, opened with `access`, and the file's name in it.
fn in_dir<V: StoreFs>(
    vfs: &V,
    path: &Path,
    access: RootAccess,
) -> Result<(V::Root, RelPathBuf), String> {
    let (dir, name) = (
        path.parent().ok_or("a path without a directory")?,
        path.file_name().ok_or("a path without a file name")?,
    );
    let root = vfs
        .open_root(dir, RootRole::Other, access)
        .map_err(|e| e.to_string())?;
    let rel =
        RelPathBuf::from_string(name.to_string_lossy().into_owned()).map_err(|e| e.to_string())?;
    Ok((root, rel))
}

/// The pool child's service of one request line (`m10 --opener`; u20 cut §3.6, §3.7): an open opens its store's root,
/// waits for its start on the system-wide clock `mono`, then times [`open_once`] on `mono`; a zero writes, flushes and
/// closes; a `pages` line writes B-1's 16 pages from its source and flushes them to disk; a `b1` line opens its file's
/// root, waits for the start, and times the open, the map, the 5 pages' copy and the drops, hashing the copy after.
pub fn serve_line<V: StoreFs + SealedMaps>(
    vfs: &V,
    mono: &dyn Fn() -> u64,
    state: &mut ChildState,
    line: &str,
) -> Reply {
    let req = match Request::parse(line) {
        Ok(r) => r,
        Err(message) => {
            return Reply::Err {
                op: line.split(' ').next().unwrap_or("").to_string(),
                message,
            };
        }
    };
    let (op, served) = match req {
        Request::Open {
            start_ns,
            spec,
            dir,
        } => (
            "open",
            (|| -> Result<(u64, u64, Option<u64>), String> {
                let root = vfs
                    .open_root(&dir, RootRole::Other, RootAccess::Read)
                    .map_err(|e| e.to_string())?;
                let sealed = sealed_rel()?;
                wait_until(mono, start_ns);
                let t0 = mono();
                let entries = open_once(vfs, &root, &sealed, &spec)?;
                let t1 = mono();
                Ok((t0, t1, Some(entries as u64)))
            })(),
        ),
        Request::Zero { from, len, path } => (
            "zero",
            (|| -> Result<(u64, u64, Option<u64>), String> {
                let io = |e: moirai_vfs::VfsError| e.to_string();
                let (dir, name) = (
                    path.parent().ok_or("a path without a directory")?,
                    path.file_name().ok_or("a path without a file name")?,
                );
                let root = vfs
                    .open_root(dir, RootRole::Other, RootAccess::ReadWrite)
                    .map_err(io)?;
                let rel = RelPathBuf::from_string(name.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())?;
                let f = vfs
                    .open(
                        &root,
                        rel.as_rel_path(),
                        Access::ReadWrite,
                        OpenHint::Normal,
                    )
                    .map_err(io)?;
                vfs.write_at(&f, from, &vec![0u8; len as usize])
                    .map_err(io)?;
                vfs.sync(&f, SyncKind::Data).map_err(|e| format!("{e:?}"))?;
                drop(f);
                Ok((0, 0, None))
            })(),
        ),
        Request::Pages { k, offsets, path } => (
            "pages",
            (|| -> Result<(u64, u64, Option<u64>), String> {
                let io = |e: moirai_vfs::VfsError| e.to_string();
                let src_path = PathBuf::from(format!("{}.src", path.to_string_lossy()));
                if state.src.as_ref().is_none_or(|(p, _)| *p != src_path) {
                    let (root, rel) = in_dir(vfs, &src_path, RootAccess::Read)?;
                    let f = vfs
                        .open(&root, rel.as_rel_path(), Access::Read, OpenHint::Normal)
                        .map_err(io)?;
                    let mut src = vec![0u8; (SRC_PAGES * PAGE) as usize];
                    vfs.read_exact_at(&f, 0, &mut src).map_err(io)?;
                    state.src = Some((src_path, src));
                }
                let src = &state.src.as_ref().expect("loaded").1;
                let (root, rel) = in_dir(vfs, &path, RootAccess::ReadWrite)?;
                let f = vfs
                    .open(
                        &root,
                        rel.as_rel_path(),
                        Access::ReadWrite,
                        OpenHint::Normal,
                    )
                    .map_err(io)?;
                for (i, &o) in offsets.iter().enumerate() {
                    let s = (((k + i as u64) % SRC_PAGES) * PAGE) as usize;
                    vfs.write_at(&f, o, &src[s..s + PAGE as usize])
                        .map_err(io)?;
                }
                vfs.sync(&f, SyncKind::DataAndMeta)
                    .map_err(|e| format!("{e:?}"))?;
                drop(f);
                Ok((0, 0, None))
            })(),
        ),
        Request::B1 {
            start_ns,
            len,
            offsets,
            path,
        } => (
            "b1",
            (|| -> Result<(u64, u64, Option<u64>), String> {
                if offsets.iter().any(|&o| o + PAGE > len) {
                    return Err(format!("a page past the file's {len} bytes"));
                }
                let (root, rel) = in_dir(vfs, &path, RootAccess::Read)?;
                let mut copy = vec![0u8; B1_READ * PAGE as usize];
                wait_until(mono, start_ns);
                let t0 = mono();
                {
                    let f = vfs
                        .open(&root, rel.as_rel_path(), Access::Read, OpenHint::Normal)
                        .map_err(|e| e.to_string())?;
                    let map = vfs
                        .map_sealed(&f, len, rel.as_rel_path())
                        .map_err(|e| e.to_string())?;
                    let b = map.bytes();
                    for (i, &o) in offsets.iter().enumerate() {
                        let (at, n) = (i * PAGE as usize, PAGE as usize);
                        copy[at..at + n].copy_from_slice(&b[o as usize..o as usize + n]);
                    }
                    drop(map);
                    drop(f);
                }
                let t1 = mono();
                Ok((t0, t1, Some(xxhash_rust::xxh3::xxh3_64(&copy))))
            })(),
        ),
        Request::Quit => ("quit", Ok((0, 0, None))),
    };
    match served {
        Ok((t0, t1, h)) => Reply::Done {
            op: op.into(),
            t0,
            t1,
            h,
        },
        Err(message) => Reply::Err {
            op: op.into(),
            message,
        },
    }
}

/// The pool child's loop (`m10 --opener`): one reply per request line, flushed at once; `quit` or the end of the input
/// ends it with 0, an unreadable input or output with 1.
pub fn serve<V: StoreFs + SealedMaps>(
    vfs: &V,
    mono: &dyn Fn() -> u64,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> u8 {
    let mut line = String::new();
    let mut state = ChildState::default();
    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => return 0,
            Ok(_) => {}
            Err(_) => return 1,
        }
        let l = line.trim_end();
        if l == "quit" {
            return 0;
        }
        let reply = serve_line(vfs, mono, &mut state, l);
        if writeln!(output, "{}", reply.line())
            .and_then(|()| output.flush())
            .is_err()
        {
            return 1;
        }
    }
}

/// `open.c16` (u20 cut D-7; critique-performance A-2; addendum m-6): each burst sends one open request with one start
/// on the system-wide clock to the 16 openers, never the modifier, and takes each opener's open time; a block of `len`
/// values takes ⌈len/16⌉ bursts. A burst's 16 values enter the block in an order that rotates by 7 per burst, so a
/// partial last burst does not always keep the same readers. An opener that started before the burst's start fails it.
pub fn burst_arm<'a>(
    pool: &'a mut (dyn Pool + 'a),
    mono: &'a dyn Fn() -> u64,
    spec: OpenSpec,
    dir: PathBuf,
) -> impl Arm + 'a {
    let mut burst = 0usize;
    BulkFn::new("open.c16", move |len: u32| {
        let mut v = Vec::with_capacity(len as usize);
        while v.len() < len as usize {
            let start_ns = mono() + START_MARGIN_NS;
            let line = Request::Open {
                start_ns,
                spec,
                dir: dir.clone(),
            }
            .line();
            for i in 0..OPENERS {
                pool.send(i, &line).map_err(ArmError)?;
            }
            let mut times = [None; OPENERS];
            for _ in 0..OPENERS {
                let (i, r) = pool.recv(REPLY_TIMEOUT).map_err(ArmError)?;
                match r {
                    Reply::Done { op, t0, t1, .. }
                        if op == "open" && i < OPENERS && times[i].is_none() && t0 >= start_ns =>
                    {
                        times[i] = Some(t1.saturating_sub(t0));
                    }
                    other => return Err(ArmError(format!("opener {i} in a burst: {other:?}"))),
                }
            }
            let rot = (burst * 7) % OPENERS;
            for j in 0..OPENERS {
                v.push(times[(rot + j) % OPENERS].expect("16 replies"));
            }
            burst += 1;
        }
        v.truncate(len as usize);
        Ok(v)
    })
}

/// The open-after-modify arms (u20 cut §3.6; critique-performance S-1): `open.mod`, whose setup has the modifier child
/// write [`MOD_BYTES`] zero bytes past the tail's end in its active extent, flush and close it, and whose op is this
/// process's open ([`open_once`]); and its floor `open`, the same open with no write since the last. The tail's bytes
/// do not change, but another process modified the file since the last open.
pub fn openmod_arms<'a, V: StoreFs + SealedMaps, T: Ticker + Clone + 'a>(
    vfs: &'a V,
    root: &'a V::Root,
    fx: &'a Fixture,
    tail: &'a Tail,
    ticker: &T,
    pool: &'a mut (dyn Pool + 'a),
) -> Result<Vec<Box<dyn Arm + 'a>>, String> {
    let geo = tail.expect.geo;
    let from = geo.offset(tail.end - 1) + 1;
    let len = MOD_BYTES.min(geo.e() - from);
    if len == 0 {
        return Err("the tail fills its last extent: no byte past it to modify".into());
    }
    let path = fx.dir.join(extent_name(geo.extent(tail.end - 1)));
    let spec = OpenSpec::of(tail);
    let zero = Request::Zero { from, len, path }.line();
    let open = Staged::new(
        "open.mod",
        ticker.clone(),
        move || -> Result<(), ArmError> {
            pool.send(MODIFIER, &zero).map_err(ArmError)?;
            match pool.recv(REPLY_TIMEOUT).map_err(ArmError)? {
                (MODIFIER, Reply::Done { op, .. }) if op == "zero" => Ok(()),
                other => Err(ArmError(format!("the modifier: {other:?}"))),
            }
        },
        move |()| open_once(vfs, root, &fx.sealed, &spec).map_err(ArmError),
        |entries: usize| {
            std::hint::black_box(entries);
            Ok(())
        },
    );
    let floor = TimedFn::new("open", ticker.clone(), move || {
        open_once(vfs, root, &fx.sealed, &spec).map_err(ArmError)
    });
    Ok(vec![Box::new(open), Box::new(floor)])
}

/// One command's file opens (M-A6; u20 cut §3.6, `17-store-parameters.md:768`): `HEAD` with one 8,192-B read, `LOCK`
/// opened, the tail's active extent and the spare one with one 4-KiB read each, and [`COMMAND_SEALED`] sealed files
/// each with its header read and checked and a map; every handle and map dropped at the end. Returns the files opened.
pub fn command_opens<V: StoreFs + SealedMaps>(
    vfs: &V,
    root: &V::Root,
    fx: &Fixture,
    tail: &Tail,
) -> Result<u64, String> {
    let io = |e: moirai_vfs::VfsError| e.to_string();
    let open = |name: RelPath<'_>| vfs.open(root, name, Access::Read, OpenHint::Normal);
    let rel = |s: String| RelPathBuf::from_string(s).map_err(|e| e.to_string());
    head_open(vfs, root)?;
    let lock = open(RelPath::literal(LOCK_NAME)).map_err(io)?;
    let mut page = vec![0u8; 4_096];
    let geo = tail.expect.geo;
    let mut extents = Vec::new();
    for k in [geo.extent(tail.end - 1), spare_extent(tail)] {
        let name = rel(extent_name(k))?;
        let f = open(name.as_rel_path()).map_err(io)?;
        vfs.read_at(&f, 0, &mut page).map_err(io)?;
        extents.push(f);
    }
    let mut maps = Vec::new();
    let mut files = Vec::new();
    for name in fx.sealed.iter().take(COMMAND_SEALED) {
        let f = open(name.as_rel_path()).map_err(io)?;
        let mut h = [0u8; SEG_HDR_LEN];
        vfs.read_exact_at(&f, 0, &mut h).map_err(io)?;
        let total = check_seg_header(&h)?;
        maps.push(
            vfs.map_sealed(&f, total, name.as_rel_path())
                .map_err(|e| e.to_string())?,
        );
        files.push(f);
    }
    let opened = 2 + extents.len() + files.len();
    drop((lock, extents, maps, files));
    Ok(opened as u64)
}

/// `open.1`, the floor of one command's opens: `HEAD` opened with one 8,192-B read, its first file.
pub fn head_open<V: StoreFs>(vfs: &V, root: &V::Root) -> Result<u64, String> {
    let head = vfs
        .open(
            root,
            RelPath::literal("HEAD"),
            Access::Read,
            OpenHint::Normal,
        )
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; HEAD_LEN];
    let n = vfs.read_at(&head, 0, &mut buf).map_err(|e| e.to_string())?;
    if n != HEAD_LEN {
        return Err(format!("HEAD read {n} bytes, not 8,192"));
    }
    Ok(1)
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

/// Starts a pool of children in a working directory.
pub type PoolStarter<'a> = dyn FnMut(&Path) -> Result<Box<dyn Pool + 'a>, String> + 'a;

/// Starts B-1's PowerShell modifier, a pool of one child, in a working directory.
pub type PsStarter<'a> = dyn FnMut(&Path, Modifier) -> Result<Box<dyn Pool + 'a>, String> + 'a;

/// What the binary passes in: the seams and the process spawner.
pub struct System<'a, 'p, V, M, T, R> {
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
    /// Starts the pool of [`POOL`] children of this driver with `--opener` in a working directory (u20 cut §3.6).
    pub pool: &'a mut PoolStarter<'p>,
    /// The system-wide monotonic clock, comparable across the pool's processes (u20 addendum I-1).
    pub mono: &'a dyn Fn() -> u64,
    /// The pool child's input (`--opener`).
    pub input: &'a mut dyn BufRead,
    /// Starts B-1's PowerShell modifier in a working directory (u20 cut §3.7).
    pub ps: &'a mut PsStarter<'p>,
}

/// The usage text.
pub const USAGE: &str =
    "usage: m10 --host laptop|hosted --condition idle|loaded|synthetic [--load-log <file>]
           [--quantity <name>] --commit <sha> --toolchain <text> --out <dir>
       m10 --smoke [--quantity <name>] --commit <sha> --toolchain <text>
       m10 --list
       m10 --help
       m10 --opener     (a pool child: the line protocol on stdin and stdout; started by the driver)
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
    let mut fx = match q.kind {
        Kind::B1(_) => {
            let fx = write_pages_fixture(
                vfs,
                scratch,
                &name,
                pages_len(q, profile),
                seed(&q.name),
                tail.expect,
            )?;
            if profile == Profile::Production {
                std::thread::sleep(B1_SETTLE);
            }
            fx
        }
        _ => write_fixture(vfs, scratch, &name, &tail, profile.sealed_len(q.n))?,
    };
    if q.kind == Kind::Opens
        && let Err(e) = write_command_files(vfs, &mut fx, &tail)
    {
        let _ = remove_fixture(vfs, scratch, &fx);
        return Err(e);
    }
    Ok((
        tail,
        FixtureGuard {
            vfs,
            parent: scratch.to_path_buf(),
            fx,
        },
    ))
}

/// The children `q`'s arms drive, started in `scratch`: the pool when its kind needs it, and B-1's PowerShell modifier.
#[allow(clippy::type_complexity)]
fn start_children<'p>(
    q: &Quantity,
    scratch: &Path,
    start_pool: &mut PoolStarter<'p>,
    start_ps: &mut PsStarter<'p>,
) -> Result<(Option<Box<dyn Pool + 'p>>, Option<Box<dyn Pool + 'p>>), String> {
    let pool = match q.kind.needs_pool() {
        true => Some(start_pool(scratch)?),
        false => None,
    };
    let ps = match q.kind {
        Kind::B1(m) if m.is_ps() => Some(start_ps(scratch, m)?),
        _ => None,
    };
    Ok((pool, ps))
}

#[allow(clippy::too_many_arguments)]
fn smoke<'p, V: StoreFs + SealedMaps, T: Ticker + Clone>(
    vfs: &V,
    ticker: &T,
    scratch: &Path,
    qs: &[Quantity],
    start_pool: &mut PoolStarter<'p>,
    start_ps: &mut PsStarter<'p>,
    mono: &dyn Fn() -> u64,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut code = EXIT_DONE;
    for q in qs {
        let mut run = || -> Result<Vec<(String, u64)>, String> {
            let (tail, guard) = fixture(vfs, scratch, q, Profile::Test)?;
            let root = vfs
                .open_root(&guard.fx.dir, RootRole::Other, RootAccess::Read)
                .map_err(|e| e.to_string())?;
            let (mut pool, mut ps) = start_children(q, scratch, start_pool, start_ps)?;
            let kids = Children {
                pool: match &mut pool {
                    Some(b) => Some(b.as_mut()),
                    None => None,
                },
                ps: match &mut ps {
                    Some(b) => Some(b.as_mut()),
                    None => None,
                },
                mono,
            };
            let mut arms = quantity_arms(q, vfs, &root, &guard.fx, &tail, ticker, kids)?;
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
            "pages_bytes": matches!(q.kind, Kind::B1(_)).then(|| pages_len(&q, profile)),
            "modifier": match q.kind {
                Kind::B1(m) => Some(m.as_str()),
                _ => None,
            },
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
    sys: System<'_, '_, V, M, T, R>,
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
    let (mut pool, mut ps) = match start_children(q, &scratch, sys.pool, sys.ps) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "m10 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let kids = Children {
        pool: match &mut pool {
            Some(b) => Some(b.as_mut()),
            None => None,
        },
        ps: match &mut ps {
            Some(b) => Some(b.as_mut()),
            None => None,
        },
        mono: sys.mono,
    };
    let mut arms = match quantity_arms(q, sys.vfs, &root, &guard.fx, &tail, &sys.ticker, kids) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "m10 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let spec = RunSpec {
        measurement: MEASUREMENT,
        quantity: q.name.clone(),
        condition,
        min_tier: q.kind.min_tier(),
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
    sys: System<'_, '_, V, M, T, R>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if args.len() == 1 && args[0] == "--list" {
        return list(sys.meter, sys.profile, out, err);
    }
    if args.len() == 1 && args[0] == "--opener" {
        return serve(sys.vfs, sys.mono, sys.input, out);
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
        return smoke(
            sys.vfs,
            &sys.ticker,
            &scratch,
            &qs,
            sys.pool,
            sys.ps,
            sys.mono,
            out,
            err,
        );
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

    /// A monotonic clock in nanoseconds, the tests' stand-in for the system-wide one.
    fn test_mono() -> u64 {
        static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
    }

    /// The requests a fake pool's children got: (child, line).
    type Requests = std::rc::Rc<std::cell::RefCell<Vec<(usize, String)>>>;

    /// A pool of in-process children over a [`MemFs`] (u20 cut §3.6: the tests' pool): each request is served at once
    /// by [`serve_line`] on the store and the clock it was given, and every request is kept.
    struct FakePool<'f> {
        fs: &'f MemFs,
        mono: &'f dyn Fn() -> u64,
        replies: std::collections::VecDeque<(usize, Reply)>,
        log: Requests,
        states: Vec<ChildState>,
        /// Added to a child's index in `log`, so pools that share one log stay apart.
        tag: usize,
    }

    impl<'f> FakePool<'f> {
        fn new(fs: &'f MemFs, mono: &'f dyn Fn() -> u64) -> FakePool<'f> {
            FakePool {
                fs,
                mono,
                replies: Default::default(),
                log: Default::default(),
                states: (0..POOL).map(|_| ChildState::default()).collect(),
                tag: 0,
            }
        }
    }

    impl Pool for FakePool<'_> {
        fn count(&self) -> usize {
            POOL
        }

        fn send(&mut self, i: usize, line: &str) -> Result<(), String> {
            if i >= POOL {
                return Err(format!("no pool child {i}"));
            }
            self.log.borrow_mut().push((self.tag + i, line.to_string()));
            let reply = serve_line(self.fs, self.mono, &mut self.states[i], line);
            self.replies.push_back((i, reply));
            Ok(())
        }

        fn recv(&mut self, _timeout: std::time::Duration) -> Result<(usize, Reply), String> {
            self.replies
                .pop_front()
                .ok_or_else(|| "no reply".to_string())
        }
    }

    /// A starter of fake pools over `fs` on the tests' clock.
    fn fake_pools<'f>(
        fs: &'f MemFs,
    ) -> impl FnMut(&Path) -> Result<Box<dyn Pool + 'f>, String> + 'f {
        move |_: &Path| Ok(Box::new(FakePool::new(fs, &test_mono)) as Box<dyn Pool + 'f>)
    }

    /// A starter that starts no pool.
    fn no_pools<'f>() -> impl FnMut(&Path) -> Result<Box<dyn Pool + 'f>, String> + 'f {
        |_: &Path| Err("no pool in this test".to_string())
    }

    /// A starter of B-1's PowerShell modifier as a fake pool over `fs`: its child 0 serves `pages` lines as the pool's
    /// modifier does.
    fn fake_ps<'f>(
        fs: &'f MemFs,
    ) -> impl FnMut(&Path, Modifier) -> Result<Box<dyn Pool + 'f>, String> + 'f {
        move |_: &Path, _: Modifier| {
            Ok(Box::new(FakePool::new(fs, &test_mono)) as Box<dyn Pool + 'f>)
        }
    }

    /// A starter that starts no PowerShell modifier.
    fn no_ps<'f>() -> impl FnMut(&Path, Modifier) -> Result<Box<dyn Pool + 'f>, String> + 'f {
        |_: &Path, _: Modifier| Err("no PowerShell in this test".to_string())
    }

    /// Runs the command line over `fs` with a real ticker and a fake pool; returns (exit, stdout, stderr).
    fn run(fs: &MemFs, args: &[&str]) -> (u8, String, String) {
        let meter = FakeMeter::new(8_000_000_000);
        let mut spawn = no_spawn;
        let mut pools = fake_pools(fs);
        let mut ps = fake_ps(fs);
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
                pool: &mut pools,
                mono: &test_mono,
                input: &mut std::io::empty(),
                ps: &mut ps,
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
        let mut pools = no_pools();
        let mut ps = no_ps();
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
                pool: &mut pools,
                mono: &test_mono,
                input: &mut std::io::empty(),
                ps: &mut ps,
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
    /// then u20's appended block of design-A A3's 18 small tails in A1, the 1e4 points first (u20 cut §3.5, §3.10), its
    /// opens and B-1; then u22's six at R-b's values last (R-86 ruling 3). By clause, 8 `ops`, 23 `rec` (the opens and
    /// B-1 carry their point's), 5 `body`, 11 `ovl`, 17 `quiet` (the CLI's 2× tails among them).
    #[test]
    fn grid_lists_the_valid_quantities_in_run_order() {
        let g = grid();
        assert_eq!(g.len(), 64);
        assert!(g.iter().all(|q| valid_name(&q.name)), "{g:?}");
        let names: BTreeSet<&str> = g.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(names.len(), 64, "unique");
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
            [8, 23, 5, 11, 17]
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
        let a1: Vec<&str> = g[31..49].iter().map(|q| q.name.as_str()).collect();
        assert_eq!(a1, A1_BLOCK, "u20's A1 block after the follow-up block");
        assert!(
            g[..31]
                .iter()
                .all(|q| q.form == Locator::None && q.kind == Kind::Tail)
        );
        assert!(
            g[31..49]
                .iter()
                .all(|q| q.form == Locator::A1 && q.kind == Kind::Tail)
        );
        let opens: Vec<(&str, Kind, Option<&str>)> = g[49..52]
            .iter()
            .map(|q| (q.name.as_str(), q.kind, q.of))
            .collect();
        assert_eq!(
            opens,
            [
                (
                    "a1-load16-rec-128kib.n1e4",
                    Kind::Load,
                    Some("a1-rec-128kib.n1e4")
                ),
                (
                    "a1-openmod-rec-128kib.n1e4",
                    Kind::OpenMod,
                    Some("a1-rec-128kib.n1e4")
                ),
                ("a1-opens10.n1e5", Kind::Opens, Some("a1-rec-128kib.n1e5")),
            ],
            "u20's opens after the A1 block"
        );
        let b1: Vec<&str> = g[52..58].iter().map(|q| q.name.as_str()).collect();
        assert_eq!(
            b1,
            [
                "b1-psview.1gib",
                "b1-wf.1gib",
                "b1-pswf.1gib",
                "b1-psview.256mib",
                "b1-wf.256mib",
                "b1-pswf.256mib"
            ],
            "B-1, the 1-GiB points first, before u22's six"
        );
        let rb: Vec<&str> = g[58..].iter().map(|q| q.name.as_str()).collect();
        assert_eq!(
            rb,
            [
                "a1-rec-64kib.n1e4",
                "a1-cli2x-p07-64kib.n1e4",
                "a1-rec-64kib.n1e5",
                "a1-cli2x-p07-64kib.n1e5",
                "a1-rec-64kib.n1e6",
                "a1-cli2x-p07-64kib.n1e6"
            ],
            "u22's six last (R-86 ruling 3)"
        );
        for q in &g[49..58] {
            let of = g.iter().find(|p| Some(p.name.as_str()) == q.of).unwrap();
            for profile in [Profile::Production, Profile::Test] {
                assert_eq!(target(q, profile), target(of, profile), "{}", q.name);
            }
            assert_eq!(
                q.kind.min_tier(),
                (q.kind != Kind::Opens).then_some(Tier::T2),
                "{}",
                q.name
            );
        }
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

    /// `--smoke` runs every quantity at the test profile with each arm once: 151 lines, one per (quantity, arm), the A1
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
        assert_eq!(lines.len(), 151, "{out}");
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
                let names: &[&str] = match (q.kind, q.form) {
                    (Kind::Tail, Locator::None) => &["replay", "floor.open"],
                    (Kind::Tail, Locator::A1) => &["replay.at", "replay", "floor.open"],
                    (Kind::Load, _) => &["open.c16"],
                    (Kind::OpenMod, _) => &["open.mod", "open"],
                    (Kind::Opens, _) => &["open.10", "open.1"],
                    (Kind::B1(_), _) => &["b1", "b1.floor"],
                };
                names
                    .iter()
                    .map(|a| (q.name.clone(), a.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(pairs, expect);
        // Each sample of a floor or an open maps the 8 sealed files and reads `HEAD` once; a burst is 16 opens; one
        // command maps 6 and opens `HEAD` in both its arms.
        let (maps, heads): (Vec<usize>, Vec<usize>) = grid()
            .iter()
            .map(|q| match q.kind {
                Kind::Tail => (SEALED_FILES, 1),
                Kind::Load => (OPENERS * SEALED_FILES, OPENERS),
                Kind::OpenMod => (2 * SEALED_FILES, 2),
                Kind::Opens => (COMMAND_SEALED, 2),
                // The setup's discarded burst, then one burst of each arm: 16 maps each, no HEAD.
                Kind::B1(_) => (3 * OPENERS, 0),
            })
            .unzip();
        let log = fs.log();
        assert_eq!(log.maps.len(), maps.iter().sum::<usize>());
        assert_eq!(
            log.opens.iter().filter(|o| *o == "HEAD").count(),
            heads.iter().sum::<usize>()
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
        assert_eq!(out.lines().count(), 151);
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
        assert_eq!((code, out.lines().count()), (EXIT_DONE, 64));
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
                    pool: &mut no_pools(),
                    mono: &test_mono,
                    input: &mut std::io::empty(),
                    ps: &mut no_ps(),
                },
                &mut out,
                &mut err,
            );
            assert_eq!(code, expect);
            assert_eq!(calls.len(), 64);
            for (c, q) in calls.iter().zip(grid()) {
                let mut want = base.clone();
                want.push("--quantity".into());
                want.push(q.name.into());
                assert_eq!(c, &want);
            }
            assert_eq!(String::from_utf8(out).unwrap().lines().count(), 64);
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
        assert_eq!(lines.len(), 64);
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
            match q.kind {
                Kind::B1(m) => {
                    assert_eq!(v["pages_bytes"].as_u64(), Some(TEST_PAGES), "{}", q.name);
                    assert_eq!(v["modifier"], m.as_str(), "{}", q.name);
                }
                _ => assert!(
                    v["pages_bytes"].is_null() && v["modifier"].is_null(),
                    "{}",
                    q.name
                ),
            }
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
        let got: Vec<(String, Clause, u64, u64, u32, Over)> = g[31..49]
            .iter()
            .map(|q| (q.name.clone(), q.clause, q.threshold, q.mult, q.n, q.over))
            .collect();
        assert_eq!(got, want);
        for q in &g[31..49] {
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
        // u22's six (R-86 ruling 3): R-b's values, P06 1,024, P07 64 KiB, P08 1 MiB, P09 48 KiB, P10 128 KiB and P11 4;
        // the single point a `rec` tail at 64 KiB, the CLI point the quiet machinery with q = 2 and its threshold the
        // overlay cap P09 = 48 KiB, R-b's P10 and P11 not overridden.
        let rb = Over {
            p06: Some(1_024),
            p07: Some(64 * KIB),
            p08: Some(MIB),
            p09: Some(48 * KIB),
            p10: Some(128 * KIB),
            p11: Some(4),
        };
        let mut want: Vec<(String, Clause, u64, u64, u32, Over)> = Vec::new();
        for (n, s) in [(10_000, "n1e4"), (100_000, "n1e5"), (1_000_000, "n1e6")] {
            want.push((format!("a1-rec-64kib.{s}"), Rec, 65_536, 1, n, rb));
            want.push((format!("a1-cli2x-p07-64kib.{s}"), Quiet, 49_152, 2, n, rb));
        }
        let six = g
            .get(58..64)
            .unwrap_or_else(|| panic!("no u22 points: {} quantities", g.len()));
        let got: Vec<(String, Clause, u64, u64, u32, Over)> = six
            .iter()
            .map(|q| (q.name.clone(), q.clause, q.threshold, q.mult, q.n, q.over))
            .collect();
        assert_eq!(got, want);
        for q in six {
            assert_eq!(q.form, Locator::A1, "{}", q.name);
            let k = SEED_K.iter().find(|(n, _)| *n == q.name).unwrap().1;
            assert_eq!(q.seed_k, k, "{}", q.name);
            let mut params = rb.apply(Params::PRODUCTION);
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
    /// cap, and the measure is 95–100 % of it; smaller k fail one of these, so `seed_k` is the first. The PR tier checks
    /// the k just below (about 40 production tails in a debug build); the nightly and exit tiers check every smaller k.
    /// The quiet points end on their record caps of 256 and 512 KiB. u22's six (R-86 ruling 3) are checked the same way:
    /// a `cli2x-p07` point's target has the overlay cap 48 KiB as its threshold, q = 2 and P09 48 KiB, and its cap is the
    /// one that binds by [F17 §5.2]'s CLI caps, P09 not multiplied. At the test profile each tail stops on a clause of its
    /// family.
    #[test]
    fn a1_tails_reach_their_caps_and_stop_on_their_clauses() {
        let every = matches!(
            std::env::var("MOIRAI_TEST_TIER").as_deref(),
            Ok("nightly") | Ok("exit")
        );
        let mut failed = Vec::new();
        let g = grid();
        let points: Vec<&Quantity> = g[31..49]
            .iter()
            .chain(g.get(58..64).unwrap_or(&[]))
            .collect();
        assert_eq!(points.len(), 24, "u20's 18 A1 points and u22's six");
        for q in points {
            let t = target(q, Profile::Production);
            if q.name.contains("-cli2x-p07-") {
                assert_eq!(
                    (t.threshold, t.mult, t.params.p09),
                    (49_152, 2, 49_152),
                    "{}: the overlay cap P09, unmultiplied",
                    q.name
                );
            }
            let tail = generate(&t, false).unwrap_or_else(|e| panic!("{}: {e}", q.name));
            let (ok, clause, share) = reaches_cap(&q.name, &t, &tail);
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
            let below = if every { 0 } else { q.seed_k.saturating_sub(1) };
            for j in below..q.seed_k {
                let mut tj = t;
                tj.seed = seed(&q.name) ^ j;
                let tail = generate(&tj, false).unwrap();
                if reaches_cap(&q.name, &tj, &tail).0 {
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
        for (i, q) in g.iter().enumerate().take(49) {
            let tail = generate(&target(q, Profile::Test), false).unwrap();
            let fx =
                write_fixture(&fs, &scratch(), &format!("fx{i}"), &tail, TEST_SEALED_LEN).unwrap();
            let root = fs
                .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
                .unwrap();
            let arms = quantity_arms(
                q,
                &fs,
                &root,
                &fx,
                &tail,
                &MonoTicker::new(),
                Children {
                    pool: None,
                    ps: None,
                    mono: &test_mono,
                },
            )
            .unwrap();
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

    /// The test-profile tail of `name` and its fixture in `fs` below [`scratch`], with `name`'s command files when it is
    /// the opens quantity.
    fn open_fixture(fs: &MemFs, name: &str) -> (Quantity, Tail, Fixture) {
        let q = grid().into_iter().find(|q| q.name == name).unwrap();
        let tail = generate(&target(&q, Profile::Test), false).unwrap();
        let mut fx = write_fixture(fs, &scratch(), "fx", &tail, TEST_SEALED_LEN).unwrap();
        if q.kind == Kind::Opens {
            write_command_files(fs, &mut fx, &tail).unwrap();
        }
        (q, tail, fx)
    }

    /// The pool's line protocol (u20 cut §3.6, addendum I-1): an open request carries the tail's scan parameters, its
    /// base and a directory with spaces, a zero request a file path with spaces, and both round-trip in each form, as do
    /// `quit` and every reply; a reply's message is one line; malformed lines are refused.
    #[test]
    fn opener_lines_round_trip() {
        let fs = MemFs::with_dir(&scratch());
        let (_, tail, _) = open_fixture(&fs, "a1-load16-rec-128kib.n1e4");
        let night = generate(&target(&grid()[0], Profile::Test), false).unwrap();
        for spec in [OpenSpec::of(&tail), OpenSpec::of(&night)] {
            let r = Request::Open {
                start_ns: 1_234_567,
                spec,
                dir: PathBuf::from("C:/a dir/with spaces/fx"),
            };
            assert_eq!(Request::parse(&r.line()), Ok(r));
        }
        assert_eq!(OpenSpec::of(&tail).expect.locator, Locator::A1);
        for r in [
            Request::Zero {
                from: 70_000,
                len: MOD_BYTES,
                path: PathBuf::from("C:/a b/log.2"),
            },
            Request::Quit,
        ] {
            assert_eq!(Request::parse(&r.line()), Ok(r));
        }
        for r in [
            Reply::Done {
                op: "open".into(),
                t0: 5,
                t1: 9,
                h: Some(42),
            },
            Reply::Done {
                op: "zero".into(),
                t0: 0,
                t1: 0,
                h: None,
            },
            Reply::Err {
                op: "open".into(),
                message: "no such file: a b".into(),
            },
        ] {
            assert_eq!(Reply::parse(&r.line()), Ok(r));
        }
        let two = Reply::Err {
            op: "zero".into(),
            message: "one\ntwo".into(),
        };
        assert_eq!(two.line(), "err zero one two");
        for bad in [
            "open 1 2",
            "open",
            "zero x 1 /p",
            "zero 1 2",
            "quit now",
            "nope 1",
        ] {
            assert!(Request::parse(bad).is_err(), "{bad}");
        }
        for bad in ["done open 1", "done open x y", "fine open 1 2", "err"] {
            assert!(Reply::parse(bad).is_err(), "{bad}");
        }
    }

    /// A child times exactly its open (u20 cut §3.6): over a store whose every file open advances the child's clock by
    /// 1 ms, an open request's `t1 − t0` is 1 ms per file the open opened (`HEAD`, the 8 sealed files and the tail's
    /// extents, all logged between its start and its reply), the root's open before `t0` advances nothing, and its value
    /// is the offsets-mode overlay's entries. With a start ahead of the clock, `t0` is not before it.
    #[test]
    fn opener_times_exactly_its_open() {
        let clock = std::rc::Rc::new(std::cell::Cell::new(0u64));
        let c = clock.clone();
        let fs = MemFs::with_hook(&scratch(), move || c.set(c.get() + MS));
        let (_, tail, fx) = open_fixture(&fs, "a1-load16-rec-128kib.n1e4");
        let spec = OpenSpec::of(&tail);
        let line = Request::Open {
            start_ns: 0,
            spec,
            dir: fx.dir.clone(),
        }
        .line();
        fs.clear_log();
        let mono = || clock.get();
        let Reply::Done { op, t0, t1, h } =
            serve_line(&fs, &mono, &mut ChildState::default(), &line)
        else {
            panic!("an open's reply");
        };
        let opens = fs.log().opens;
        let geo = tail.expect.geo;
        let extents = (geo.extent(tail.start)..=geo.extent(tail.end - 1)).count();
        assert_eq!(opens.len(), 1 + SEALED_FILES + extents, "{opens:?}");
        assert_eq!(op, "open");
        assert_eq!(
            t1 - t0,
            opens.len() as u64 * MS,
            "1 ms per file the open opened"
        );
        let o = Overlay::replay_at(
            &mut crate::tail::scan::MemLog::new(tail.start, &tail.bytes),
            &tail.expect,
            tail.start,
            tail.end,
            tail.chain_in,
            tail.base,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        assert_eq!(h, Some(o.entries() as u64));
        let real = MemFs::with_dir(&scratch());
        let (_, tail, fx) = open_fixture(&real, "a1-load16-rec-128kib.n1e4");
        let start = test_mono() + 5 * MS;
        let line = Request::Open {
            start_ns: start,
            spec: OpenSpec::of(&tail),
            dir: fx.dir.clone(),
        }
        .line();
        let Reply::Done { t0, .. } =
            serve_line(&real, &test_mono, &mut ChildState::default(), &line)
        else {
            panic!("an open's reply");
        };
        assert!(t0 >= start, "{t0} before the start {start}");
    }

    /// A pool whose child i replies to an open with `t0` = the start and `t1` = the start + i + 1, or with a start one ns
    /// early: the burst arm's scripted pool.
    struct ScriptedPool {
        log: Vec<(usize, String)>,
        replies: std::collections::VecDeque<(usize, Reply)>,
        early: bool,
    }

    impl Pool for ScriptedPool {
        fn count(&self) -> usize {
            POOL
        }

        fn send(&mut self, i: usize, line: &str) -> Result<(), String> {
            self.log.push((i, line.to_string()));
            let Ok(Request::Open { start_ns, .. }) = Request::parse(line) else {
                return Err(format!("not an open: {line}"));
            };
            let t0 = start_ns - u64::from(self.early);
            self.replies.push_back((
                i,
                Reply::Done {
                    op: "open".into(),
                    t0,
                    t1: start_ns + i as u64 + 1,
                    h: Some(1),
                },
            ));
            Ok(())
        }

        fn recv(&mut self, _: std::time::Duration) -> Result<(usize, Reply), String> {
            self.replies
                .pop_front()
                .ok_or_else(|| "no reply".to_string())
        }
    }

    /// `open.c16` (u20 cut D-7; addendum m-6): over a fake pool, a block of 16 is one burst, one open request to each of
    /// the 16 openers, none to the modifier, all with one start ahead of the clock, and its 16 values; a block of 20 is
    /// two bursts, the second's values in the order rotated by 7, so the partial burst keeps openers 7–10, not 0–3; an
    /// opener that started before the burst fails it.
    #[test]
    fn load16_burst_starts_16_opens_and_returns_16_times() {
        let fs = MemFs::with_dir(&scratch());
        let (_, tail, fx) = open_fixture(&fs, "a1-load16-rec-128kib.n1e4");
        let mut pool = FakePool::new(&fs, &test_mono);
        let log = pool.log.clone();
        {
            let mut arm = burst_arm(&mut pool, &test_mono, OpenSpec::of(&tail), fx.dir.clone());
            assert_eq!(arm.name(), "open.c16");
            let before = test_mono();
            arm.begin_block(16).unwrap();
            let values: Vec<u64> = (0..16).map(|_| arm.sample().unwrap()).collect();
            assert!(values.iter().all(|&v| v > 0), "{values:?}");
            let sent = log.borrow().clone();
            let children: Vec<usize> = sent.iter().map(|s| s.0).collect();
            assert_eq!(
                children,
                (0..OPENERS).collect::<Vec<_>>(),
                "one per opener, none to the modifier"
            );
            let starts: BTreeSet<u64> = sent
                .iter()
                .map(|(_, l)| match Request::parse(l).unwrap() {
                    Request::Open { start_ns, .. } => start_ns,
                    r => panic!("{r:?}"),
                })
                .collect();
            assert_eq!(starts.len(), 1, "one start");
            assert!(starts.iter().all(|&s| s >= before + START_MARGIN_NS));
            arm.begin_block(20).unwrap();
            assert_eq!(
                log.borrow().len(),
                16 + 2 * OPENERS,
                "two bursts for 20 values"
            );
            assert!((0..20).all(|_| arm.sample().is_ok()));
            assert!(arm.sample().is_err(), "20 values");
        }
        let mut scripted = ScriptedPool {
            log: Vec::new(),
            replies: Default::default(),
            early: false,
        };
        let mut arm = burst_arm(
            &mut scripted,
            &test_mono,
            OpenSpec::of(&tail),
            fx.dir.clone(),
        );
        arm.begin_block(16).unwrap();
        let first: Vec<u64> = (0..16).map(|_| arm.sample().unwrap()).collect();
        assert_eq!(
            first,
            (1..=16).collect::<Vec<u64>>(),
            "burst 0 in opener order"
        );
        arm.begin_block(4).unwrap();
        let partial: Vec<u64> = (0..4).map(|_| arm.sample().unwrap()).collect();
        assert_eq!(
            partial,
            [8, 9, 10, 11],
            "burst 1 rotated by 7: openers 7-10"
        );
        drop(arm);
        let mut early = ScriptedPool {
            log: Vec::new(),
            replies: Default::default(),
            early: true,
        };
        let mut arm = burst_arm(&mut early, &test_mono, OpenSpec::of(&tail), fx.dir.clone());
        assert!(
            arm.begin_block(1).is_err(),
            "an opener started before the burst"
        );
    }

    /// A ticker that records, at each reading, how many requests a pool's children had got and how many reads the store
    /// had logged, and returns 1, 2, 3, … (R-29).
    #[derive(Clone)]
    struct PoolTicker<'f> {
        fs: &'f MemFs,
        log: Requests,
        ticks: std::rc::Rc<std::cell::RefCell<Vec<(usize, usize)>>>,
    }

    impl Ticker for PoolTicker<'_> {
        fn now_ns(&mut self) -> u64 {
            let mut t = self.ticks.borrow_mut();
            t.push((self.log.borrow().len(), self.fs.log().reads.len()));
            t.len() as u64
        }
    }

    /// The open after a modify (u20 cut §3.6; critique-performance S-1; R-29): over a fake pool, each `open.mod` sample
    /// sends one zero request, to the modifier alone, before its t0: 4 KiB past the tail's end in its last extent, which
    /// stays zero there while the tail's bytes do not change; between its ticks no request is sent and the open reads
    /// `HEAD` and the whole tail. Its floor `open` sends no request at all.
    #[test]
    fn openmod_setup_writes_the_extent_through_another_process_before_t0() {
        let fs = MemFs::with_dir(&scratch());
        let (q, tail, fx) = open_fixture(&fs, "a1-openmod-rec-128kib.n1e4");
        assert_eq!(q.kind, Kind::OpenMod);
        let root = fs
            .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
            .unwrap();
        let mut pool = FakePool::new(&fs, &test_mono);
        let log = pool.log.clone();
        let ticker = PoolTicker {
            fs: &fs,
            log: log.clone(),
            ticks: Default::default(),
        };
        let geo = tail.expect.geo;
        let ext = fx.dir.join(extent_name(geo.extent(tail.end - 1)));
        let before = fs.bytes(&ext).unwrap();
        let mut arms = openmod_arms(&fs, &root, &fx, &tail, &ticker, &mut pool).unwrap();
        let names: Vec<&str> = arms.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["open.mod", "open"]);
        for k in 0..2 {
            let sent0 = log.borrow().len();
            arms[0].sample().unwrap();
            let ticks = ticker.ticks.borrow().clone();
            let ((s0, r0), (s1, r1)) = (ticks[ticks.len() - 2], ticks[ticks.len() - 1]);
            assert_eq!(s0, sent0 + 1, "sample {k}: one request before t0");
            assert_eq!(s1, s0, "sample {k}: no request between the ticks");
            let (child, line) = log.borrow()[sent0].clone();
            assert_eq!(
                child, MODIFIER,
                "sample {k}: the modifier writes, no opener"
            );
            assert_eq!(
                Request::parse(&line),
                Ok(Request::Zero {
                    from: geo.offset(tail.end - 1) + 1,
                    len: MOD_BYTES,
                    path: ext.clone(),
                }),
                "sample {k}"
            );
            let w = &fs.log().reads[r0..r1];
            assert!(
                w.iter().any(|r| r.0 == "HEAD"),
                "sample {k}: the open's HEAD read"
            );
            let tail_read: u64 = w
                .iter()
                .filter(|r| r.0.starts_with("log."))
                .map(|r| r.2 as u64)
                .sum();
            assert_eq!(
                tail_read,
                tail.end - tail.start,
                "sample {k}: the whole tail"
            );
            assert_eq!(
                fs.bytes(&ext).unwrap(),
                before,
                "sample {k}: the tail's bytes stay"
            );
            arms[1].sample().unwrap();
            assert_eq!(
                log.borrow().len(),
                sent0 + 1,
                "sample {k}: the floor sends nothing"
            );
        }
    }

    /// One command's opens (u20 cut §3.6, M-A6): each `open.10` sample opens exactly `HEAD` (one 8,192-B read), `LOCK`,
    /// the tail's last extent and the spare (one 4-KiB read at 0 each) and 6 sealed files (a 120-B header read and a map
    /// each): 10 files; its floor `open.1` opens `HEAD` alone with its read.
    #[test]
    fn opens10_opens_ten_files_and_its_floor_one() {
        let fs = MemFs::with_dir(&scratch());
        let (q, tail, fx) = open_fixture(&fs, "a1-opens10.n1e5");
        let root = fs
            .open_root(&fx.dir, RootRole::Other, RootAccess::Read)
            .unwrap();
        let mut arms = quantity_arms(
            &q,
            &fs,
            &root,
            &fx,
            &tail,
            &MonoTicker::new(),
            Children {
                pool: None,
                ps: None,
                mono: &test_mono,
            },
        )
        .unwrap();
        let names: Vec<&str> = arms.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["open.10", "open.1"]);
        let geo = tail.expect.geo;
        let (last, spare) = (
            extent_name(geo.extent(tail.end - 1)),
            extent_name(spare_extent(&tail)),
        );
        let sealed: Vec<String> = crate::tail::store::sealed_names()
            .into_iter()
            .take(COMMAND_SEALED)
            .collect();
        for k in 0..2 {
            fs.clear_log();
            arms[0].sample().unwrap();
            let log = fs.log();
            let mut want = vec![
                "HEAD".to_string(),
                LOCK_NAME.to_string(),
                last.clone(),
                spare.clone(),
            ];
            want.extend(sealed.iter().cloned());
            assert_eq!(log.opens, want, "sample {k}: 10 files");
            let mut reads = vec![
                ("HEAD".to_string(), 0, HEAD_LEN),
                (last.clone(), 0, 4_096),
                (spare.clone(), 0, 4_096),
            ];
            reads.extend(sealed.iter().map(|s| (s.clone(), 0, SEG_HDR_LEN)));
            assert_eq!(log.reads, reads, "sample {k}");
            assert_eq!(log.maps.len(), COMMAND_SEALED, "sample {k}");
            fs.clear_log();
            arms[1].sample().unwrap();
            let log = fs.log();
            assert_eq!(log.opens, ["HEAD"], "sample {k}");
            assert_eq!(log.reads, [("HEAD".to_string(), 0, HEAD_LEN)], "sample {k}");
        }
    }

    /// `--opener` alone serves the pool's protocol on its input (u20 cut §3.6): an open of a written fixture gets `done
    /// open` with the offsets-mode overlay's entries, a zero `done zero`, a malformed line `err`; `quit` ends it with 0
    /// and nothing after it is served.
    #[test]
    fn cli_opener_mode_serves_the_protocol() {
        let fs = MemFs::with_dir(&scratch());
        let (_, tail, fx) = open_fixture(&fs, "a1-load16-rec-128kib.n1e4");
        let geo = tail.expect.geo;
        let open = Request::Open {
            start_ns: 0,
            spec: OpenSpec::of(&tail),
            dir: fx.dir.clone(),
        }
        .line();
        let zero = Request::Zero {
            from: geo.offset(tail.end - 1) + 1,
            len: 8,
            path: fx.dir.join(extent_name(geo.extent(tail.end - 1))),
        }
        .line();
        let input = format!("{open}\n{zero}\ngarbage here\nquit\n{open}\n");
        let meter = FakeMeter::new(8_000_000_000);
        let mut spawn = no_spawn;
        let mut pools = no_pools();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli(
            &os(&["--opener"]),
            System {
                vfs: &fs,
                meter: &meter,
                ticker: MonoTicker::new(),
                runner: Canned::good(),
                spawn: &mut spawn,
                profile: Profile::Test,
                pool: &mut pools,
                mono: &test_mono,
                input: &mut input.as_bytes(),
                ps: &mut no_ps(),
            },
            &mut out,
            &mut err,
        );
        assert_eq!(code, EXIT_DONE);
        let out = String::from_utf8(out).unwrap();
        let replies: Vec<Reply> = out.lines().map(|l| Reply::parse(l).unwrap()).collect();
        assert_eq!(replies.len(), 3, "{out}");
        let o = Overlay::replay_at(
            &mut crate::tail::scan::MemLog::new(tail.start, &tail.bytes),
            &tail.expect,
            tail.start,
            tail.end,
            tail.chain_in,
            tail.base,
            TAIL_READ_CHUNK,
        )
        .unwrap();
        assert!(
            matches!(&replies[0], Reply::Done { op, h, .. } if op == "open" && *h == Some(o.entries() as u64)),
            "{replies:?}"
        );
        assert!(
            matches!(&replies[1], Reply::Done { op, .. } if op == "zero"),
            "{replies:?}"
        );
        assert!(
            matches!(&replies[2], Reply::Err { op, .. } if op == "garbage"),
            "{replies:?}"
        );
    }

    /// B-1's six quantities (u20 cut §3.7): `b1-<modifier>.<size>` for the PowerShell view, the pool's `WriteFile` and the
    /// PowerShell `WriteFile` at 1 GiB, then at 256 MiB; minimum tier t2; the pages at production and 1 MiB at the test
    /// profile; the modifier's process: the pool's child for `wf`, PowerShell for the other two.
    #[test]
    fn b1_points_have_their_sizes_and_modifiers() {
        let g = b1_points();
        let got: Vec<(&str, Kind, u64)> = g
            .iter()
            .map(|q| (q.name.as_str(), q.kind, q.pages))
            .collect();
        assert_eq!(
            got,
            [
                ("b1-psview.1gib", Kind::B1(Modifier::PsView), 1 << 30),
                ("b1-wf.1gib", Kind::B1(Modifier::Wf), 1 << 30),
                ("b1-pswf.1gib", Kind::B1(Modifier::PsWf), 1 << 30),
                ("b1-psview.256mib", Kind::B1(Modifier::PsView), 256 << 20),
                ("b1-wf.256mib", Kind::B1(Modifier::Wf), 256 << 20),
                ("b1-pswf.256mib", Kind::B1(Modifier::PsWf), 256 << 20),
            ]
        );
        for q in &g {
            assert!(valid_name(&q.name), "{}", q.name);
            assert_eq!(q.kind.min_tier(), Some(Tier::T2), "{}", q.name);
            assert!(q.kind.needs_pool(), "{}: the readers", q.name);
            let Kind::B1(m) = q.kind else { unreachable!() };
            assert_eq!(q.kind.needs_ps(), m.is_ps(), "{}", q.name);
            assert_eq!(pages_len(q, Profile::Production), q.pages);
            assert_eq!(pages_len(q, Profile::Test), TEST_PAGES);
        }
        assert_eq!(
            [Modifier::Wf, Modifier::PsView, Modifier::PsWf].map(Modifier::as_str),
            ["wf", "psview", "pswf"]
        );
    }

    /// The pages fixture of the B-1 quantity `name` at the test profile, in `fs` below [`scratch`].
    fn b1_fixture(fs: &MemFs, name: &str) -> (Quantity, Fixture) {
        let q = grid().into_iter().find(|q| q.name == name).unwrap();
        let fx = crate::tail::store::write_pages_fixture(
            fs,
            &scratch(),
            "b1",
            TEST_PAGES,
            seed(&q.name),
            generate(&target(&q, Profile::Test), false).unwrap().expect,
        )
        .unwrap();
        (q, fx)
    }

    /// A B-1 reader (u20 cut §3.7): a `b1` line opens the pages file once, maps it whole once, and replies with the
    /// xxh3-64 of the 5 pages at its offsets, in order, and a `t0` not before the start; a page past the file is refused.
    #[test]
    fn b1_opener_maps_and_reads_five_pages() {
        let fs = MemFs::with_dir(&scratch());
        let (_, fx) = b1_fixture(&fs, "b1-wf.256mib");
        let path = fx.dir.join(PAGES_NAME);
        let offsets = [0, 7 * PAGE, 3 * PAGE, 200 * PAGE, 255 * PAGE];
        let start = test_mono() + 2 * MS;
        let line = Request::B1 {
            start_ns: start,
            len: TEST_PAGES,
            offsets,
            path: path.clone(),
        }
        .line();
        fs.clear_log();
        let reply = serve_line(&fs, &test_mono, &mut ChildState::default(), &line);
        let Reply::Done { op, t0, t1, h } = reply else {
            panic!("{reply:?}");
        };
        let bytes = fs.bytes(&path).unwrap();
        let mut want = Vec::new();
        for o in offsets {
            want.extend_from_slice(&bytes[o as usize..(o + PAGE) as usize]);
        }
        assert_eq!(op, "b1");
        assert_eq!(h, Some(xxhash_rust::xxh3::xxh3_64(&want)));
        assert!(t0 >= start && t1 >= t0);
        let log = fs.log();
        assert_eq!(log.opens, [PAGES_NAME]);
        assert_eq!(log.maps, [(PAGES_NAME.to_string(), TEST_PAGES)]);
        let past = Request::B1 {
            start_ns: 0,
            len: TEST_PAGES,
            offsets: [0, 0, 0, 0, 256 * PAGE],
            path,
        }
        .line();
        assert!(matches!(
            serve_line(&fs, &test_mono, &mut ChildState::default(), &past),
            Reply::Err { .. }
        ));
    }

    /// B-1's arms over a fake reader pool and, for a PowerShell quantity, a fake PowerShell modifier sharing its request
    /// log (the modifier's children logged from 100).
    fn b1_rig<'f>(fs: &'f MemFs, name: &str) -> (Fixture, FakePool<'f>, FakePool<'f>, Requests) {
        let (_, fx) = b1_fixture(fs, name);
        let readers = FakePool::new(fs, &test_mono);
        let mut ps = FakePool::new(fs, &test_mono);
        ps.log = readers.log.clone();
        ps.tag = 100;
        let log = readers.log.clone();
        (fx, readers, ps, log)
    }

    /// The modification precedes the burst, through another process (u20 cut §3.7): over fake pools, every `b1` block
    /// sends one `pages` request, to the pool's modifier for `wf` and to the PowerShell modifier for the other two, never
    /// to an opener, before the burst's 16 `b1` requests to the 16 openers, which share one start; the setup made
    /// modification 0 and a burst before the arms were returned.
    #[test]
    fn b1_modifier_writes_through_another_process_before_the_burst() {
        for (name, writer) in [("b1-wf.256mib", MODIFIER), ("b1-psview.256mib", 100)] {
            let fs = MemFs::with_dir(&scratch());
            let (fx, mut readers, mut ps, log) = b1_rig(&fs, name);
            let ps = if writer == 100 {
                Some(&mut ps as &mut dyn Pool)
            } else {
                None
            };
            let mut arms = b1_arms(
                &mut readers,
                ps,
                &test_mono,
                fx.dir.join(PAGES_NAME),
                TEST_PAGES,
                seed(name),
            )
            .unwrap();
            assert_eq!(
                arms.iter().map(|a| a.name()).collect::<Vec<_>>(),
                ["b1", "b1.floor"]
            );
            assert_eq!(
                log.borrow().len(),
                1 + OPENERS,
                "{name}: the setup's modification and burst"
            );
            for k in 0..2 {
                let from = log.borrow().len();
                arms[0].begin_block(OPENERS as u32).unwrap();
                let sent = log.borrow()[from..].to_vec();
                assert_eq!(sent.len(), 1 + OPENERS, "{name} block {k}");
                assert_eq!(sent[0].0, writer, "{name} block {k}: the modifier writes");
                assert!(
                    sent[0].1.starts_with("pages "),
                    "{name} block {k}: {}",
                    sent[0].1
                );
                let readers: Vec<usize> = sent[1..].iter().map(|s| s.0).collect();
                assert_eq!(
                    readers,
                    (0..OPENERS).collect::<Vec<_>>(),
                    "{name} block {k}"
                );
                let starts: BTreeSet<u64> = sent[1..]
                    .iter()
                    .map(|(_, l)| match Request::parse(l).unwrap() {
                        Request::B1 { start_ns, .. } => start_ns,
                        r => panic!("{r:?}"),
                    })
                    .collect();
                assert_eq!(starts.len(), 1, "{name} block {k}: one start");
                assert!((0..OPENERS).all(|_| arms[0].sample().is_ok()));
            }
        }
    }

    /// The floor's bursts follow no write (u20 cut §3.7): a `b1.floor` block sends only `b1` requests, 5 offsets each among
    /// the last modification's 16, and its values check against that modification's pages.
    #[test]
    fn b1_floor_burst_follows_no_write() {
        let fs = MemFs::with_dir(&scratch());
        let (fx, mut readers, _, log) = b1_rig(&fs, "b1-wf.256mib");
        let mut arms = b1_arms(
            &mut readers,
            None,
            &test_mono,
            fx.dir.join(PAGES_NAME),
            TEST_PAGES,
            seed("b1-wf.256mib"),
        )
        .unwrap();
        arms[0].begin_block(3).unwrap();
        let last = log
            .borrow()
            .iter()
            .rev()
            .find_map(|(_, l)| match Request::parse(l).unwrap() {
                Request::Pages { offsets, .. } => Some(offsets),
                _ => None,
            })
            .unwrap();
        let from = log.borrow().len();
        arms[1].begin_block(20).unwrap();
        let sent = log.borrow()[from..].to_vec();
        assert_eq!(
            sent.len(),
            2 * OPENERS,
            "two bursts for 20 values, no write"
        );
        for (_, l) in &sent {
            let Request::B1 { offsets, .. } = Request::parse(l).unwrap() else {
                panic!("{l}");
            };
            assert!(offsets.iter().all(|o| last.contains(o)), "{l}");
            assert_eq!(offsets.iter().collect::<BTreeSet<_>>().len(), B1_READ);
        }
        assert!((0..20).all(|_| arms[1].sample().is_ok()));
    }

    /// A modifier that answers `pages` without writing anything.
    struct SilentModifier {
        replies: std::collections::VecDeque<(usize, Reply)>,
    }

    impl Pool for SilentModifier {
        fn count(&self) -> usize {
            1
        }

        fn send(&mut self, i: usize, _: &str) -> Result<(), String> {
            self.replies.push_back((
                i,
                Reply::Done {
                    op: "pages".into(),
                    t0: 0,
                    t1: 0,
                    h: None,
                },
            ));
            Ok(())
        }

        fn recv(&mut self, _: std::time::Duration) -> Result<(usize, Reply), String> {
            self.replies.pop_front().ok_or_else(|| "no reply".into())
        }
    }

    /// The readers see the modifier's pages (u20 cut §3.7, the content check): after a fake PowerShell modification the
    /// 16 offsets hold the source pages `(k + i) mod 64`; a modifier that writes nothing fails the setup's burst with a
    /// hash mismatch, so a silent, late or misplaced write turns the run red.
    #[test]
    fn b1_readers_see_the_modifiers_pages() {
        let fs = MemFs::with_dir(&scratch());
        let (fx, mut readers, mut ps, log) = b1_rig(&fs, "b1-pswf.256mib");
        let path = fx.dir.join(PAGES_NAME);
        let s = seed("b1-pswf.256mib");
        let arms = b1_arms(
            &mut readers,
            Some(&mut ps),
            &test_mono,
            path.clone(),
            TEST_PAGES,
            s,
        );
        assert!(arms.is_ok(), "{:?}", arms.err());
        let m = Modification::of(s, 0, TEST_PAGES);
        let bytes = fs.bytes(&path).unwrap();
        let src = pages_src(s);
        for (i, o) in m.offsets.iter().enumerate() {
            let at = ((m.k + i as u64) % SRC_PAGES * PAGE) as usize;
            assert!(
                bytes[*o as usize..(*o + PAGE) as usize] == src[at..at + PAGE as usize],
                "page {i} at {o}"
            );
        }
        assert!(
            log.borrow()
                .iter()
                .any(|(c, l)| *c == 100 && l.starts_with("pages "))
        );
        drop(arms);
        let fs = MemFs::with_dir(&scratch());
        let (fx, mut readers, _, _) = b1_rig(&fs, "b1-pswf.256mib");
        let mut silent = SilentModifier {
            replies: Default::default(),
        };
        let e = b1_arms(
            &mut readers,
            Some(&mut silent),
            &test_mono,
            fx.dir.join(PAGES_NAME),
            TEST_PAGES,
            s,
        )
        .err()
        .expect("a modifier that writes nothing");
        assert!(e.contains("hash mismatch"), "{e}");
    }

    /// Base64 with padding (RFC 4648 §10's test vectors).
    #[test]
    fn base64_follows_rfc4648() {
        for (raw, b64) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(raw.as_bytes()), b64, "{raw}");
        }
        assert_eq!(base64(&[0xFB, 0xFF]), "+/8=");
    }

    /// Decodes base64 with padding, for the tests.
    fn unbase64(s: &str) -> Vec<u8> {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let val = |c: u8| A.iter().position(|&a| a == c).unwrap() as u32;
        let mut out = Vec::new();
        for q in s.as_bytes().chunks(4) {
            let pad = q.iter().filter(|&&c| c == b'=').count();
            let n = q
                .iter()
                .map(|&c| if c == b'=' { 0 } else { val(c) })
                .fold(0u32, |acc, v| (acc << 6) | v);
            let b = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&b[..3 - pad]);
        }
        out
    }

    /// The PowerShell modifier (u20 cut §3.7; R-53 Q1 (b)): Windows PowerShell below `%SystemRoot%`, where
    /// `DEFENDER_COMMAND` finds it, refused without `%SystemRoot%`; its arguments `-NoLogo -NoProfile -NonInteractive
    /// -EncodedCommand <b64>`, the base64 decoding to the UTF-16LE script with its mode line.
    #[test]
    fn powershell_modifier_runs_windows_powershell_with_an_encoded_script() {
        let root = std::env::temp_dir();
        let p = ps_program(Some(root.as_os_str())).unwrap();
        assert_eq!(
            p,
            crate::host::DEFENDER_COMMAND
                .resolve(Some(root.as_os_str()))
                .unwrap()
        );
        assert!(p.ends_with("System32/WindowsPowerShell/v1.0/powershell.exe"));
        assert!(ps_program(None).is_err());
        for (m, mode) in [(Modifier::PsView, "view"), (Modifier::PsWf, "wf")] {
            let args = ps_args(m);
            assert_eq!(
                args[..4],
                [
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-EncodedCommand"
                ]
            );
            assert_eq!(args.len(), 5);
            let raw = unbase64(&args[4]);
            let units: Vec<u16> = raw
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let script = String::from_utf16(&units).unwrap();
            assert_eq!(script, format!("$mode = '{mode}'\n{PS_SCRIPT}"));
            assert_eq!(script, ps_script(m));
        }
    }

    /// The PowerShell script's two branches (u20 cut §3.7): view mode makes a read/write `MemoryMappedFile` over the
    /// stream and writes each page through a view (`CreateViewAccessor`, `WriteArray`, the view's `Flush()`), with no
    /// stream `Write`; write mode `Seek`s and `Write`s, with no view; both end with `Flush($true)`.
    #[test]
    fn powershell_script_writes_through_a_read_write_view_in_view_mode() {
        let (view, rest) = PS_SCRIPT
            .split_once("if ($mode -eq 'view') {")
            .expect("a view branch");
        let (view_branch, wf_branch) = rest.split_once("} else {").expect("a wf branch");
        let wf_branch = wf_branch
            .split_once("$fs.Flush($true)")
            .expect("the final flush")
            .0;
        for needle in [
            "CreateFromFile",
            "MemoryMappedFileAccess]::ReadWrite",
            "CreateViewAccessor",
            "WriteArray",
            "$v.Flush()",
        ] {
            assert!(view_branch.contains(needle), "view: {needle}");
        }
        assert!(!view_branch.contains(".Write("), "view: no stream write");
        assert!(wf_branch.contains(".Seek(") && wf_branch.contains(".Write("));
        assert!(!wf_branch.contains("CreateViewAccessor"));
        assert!(PS_SCRIPT.contains("$fs.Flush($true)"));
        assert!(view.contains("ReadAllBytes($srcPath)"));
        assert!(!PS_SCRIPT.contains("SetFileTime"));
    }

    /// The real PowerShell modifier in both modes (u20 cut §3.7), where `%SystemRoot%` is set (Windows): on a 1-MiB pages
    /// file in a scratch directory, one `pages` line gets `done pages`, and its 16 offsets then hold the source pages
    /// `(k + i) mod 64` while every other byte keeps its seeded value. Elsewhere the start is refused.
    #[test]
    fn powershell_modifiers_write_their_pages() {
        let Some(root) = std::env::var_os("SystemRoot") else {
            assert!(ps_program(None).is_err());
            return;
        };
        let exe = ps_program(Some(&root)).unwrap();
        for (k, m) in [Modifier::PsView, Modifier::PsWf].into_iter().enumerate() {
            let dir = crate::testkit::scratch_dir("b1-ps");
            let path = dir.join(PAGES_NAME);
            let s = 11 + k as u64;
            let before = crate::tail::store::pages_bytes(s, 0, TEST_PAGES as usize);
            std::fs::write(&path, &before).unwrap();
            std::fs::write(dir.join(crate::tail::store::PAGES_SRC_NAME), pages_src(s)).unwrap();
            let args = ps_args(m);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let mut child = ProcessPool::spawn(&exe, &args, 1, &dir).unwrap();
            let md = Modification::of(s, 3, TEST_PAGES);
            let line = Request::Pages {
                k: md.k,
                offsets: md.offsets,
                path: path.clone(),
            }
            .line();
            child.send(0, &line).unwrap();
            let reply = child.recv(std::time::Duration::from_secs(120));
            assert!(
                matches!(&reply, Ok((0, Reply::Done { op, .. })) if op == "pages"),
                "{m:?}: {reply:?}"
            );
            drop(child);
            let after = std::fs::read(&path).unwrap();
            let src = pages_src(s);
            let mut want = before.clone();
            for (i, o) in md.offsets.iter().enumerate() {
                let at = ((md.k + i as u64) % SRC_PAGES * PAGE) as usize;
                want[*o as usize..(*o + PAGE) as usize]
                    .copy_from_slice(&src[at..at + PAGE as usize]);
            }
            assert!(after == want, "{m:?}: the file is not the modification's");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
