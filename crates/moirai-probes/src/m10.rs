//! Measurement 10 (WP-53d, [60 §5.2] item 10, `60-roadmap.md:766`): full-tail replay at the candidate checkpoint
//! thresholds, decoding every record and applying it into the F09/F11-shaped overlay, with the spec-derived lazy share
//! of [70 S8]; and the open floor it is gated against ("`HEAD` pread + map of 8 files", `60-roadmap.md:790`). It
//! decides `F17-ckpt-ops`, `-ckpt-bytes`, `-ckpt-body` (the open side), `-tail-overlay`, `-tail-overlay-quiet` and
//! `-quiet-mult` (`docs/spec/HOLES.md:41-46`; their constraints `17-store-parameters.md:757-762`).
//!
//! **The grid** (18 quantities, names by [MP §4.1]): `ops` at P06 4,096, 2,048 and 1,024 with N = 1e5 and 1e6;
//! `rec` at P07 4, 2 and 1 MiB; `body` at P08 32, 16 and 8 MiB; `ovl` at P09 1 MiB and 512 KiB; `quiet` at P10 2 MiB and
//! 1.5 MiB × P11 8 and 4; the last four families at N = 1e6 (cut D-3: open at 1e5 is replay(cap) + floor(1e5), the floor
//! measured by the `ops` family at both scales). Every run is measurement 10 with minimum tier t2, the tier of the open
//! budgets ([MP §3.2], `17-store-parameters.md:757`). The test profile's thresholds and E give the smoke's versions.
//!
//! **The run order** (R-34 B-3, M-15; R-35 Q2, Q4): one point of each family first, F17's first candidate at 1e6, the
//! scale of the binding open budget (`17-store-parameters.md:756-761`): `ops-4096`, `rec-4mib`, `body-32mib`, `ovl-1mib`,
//! `quiet-2mib-x8`; then the other 13 in family order. A short window then still holds one point of every family.
//!
//! **The arms** ([MP §4.1]): arm 0 `replay` opens the tail's extents and reads `[checkpoint_lsn, committed_lsn)` through
//! the fixed 64-KiB buffer, scans, validates, decodes, applies and finishes the overlay; arm 1 `floor.open` is
//! [`crate::tail::store::open_floor`]. Open at a grid point = floor + replay (`60-roadmap.md:790`).
//!
//! **The driver** ([MP §9.7] as merged, through the shared [`crate::m1::driver`]): `--host laptop|hosted`,
//! `--condition idle|loaded|synthetic`, `--load-log <file>` (a loaded or synthetic run's condition is the log header's,
//! and a loaded run's replay verdict is read over its window), `--quantity <name>` (one run, in this process),
//! `--commit <sha>` and `--toolchain <text>` (required), `--out <dir>` (required unless `--smoke`, refused with it; each
//! record one JSON file directly there, created when missing, streamed, removed on a failed write, [MP §7.2]),
//! `--smoke` (the requested quantity, or every one, at the test profile, each arm once, nothing recorded), `--help`;
//! and, beside the contract, `m10 --list` alone (one JSON line of tail measures per quantity). Every fixture lies below
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
use crate::tail::{Params, TAIL_READ_CHUNK};
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

/// One grid point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quantity {
    /// The quantity's name.
    pub name: String,
    /// The family.
    pub clause: Clause,
    /// The candidate threshold: P06, P07, P08, P09 or P10.
    pub threshold: u64,
    /// P11 for `quiet`, else 1.
    pub mult: u64,
    /// The base's node count.
    pub n: u32,
}

fn bytes_name(b: u64) -> String {
    if b.is_multiple_of(MIB) {
        format!("{}mib", b / MIB)
    } else {
        format!("{}kib", b / KIB)
    }
}

/// The 18 quantities of the grid (`17-store-parameters.md:757-762`) in run order: one point of each family first, F17's
/// first candidate at 1e6, then the rest in family order (R-34 B-3, M-15).
pub fn grid() -> Vec<Quantity> {
    let q = |clause: Clause, threshold: u64, mult: u64, n: u32| {
        let scale = if n == 100_000 { "n1e5" } else { "n1e6" };
        let t = match clause {
            Clause::Ops => threshold.to_string(),
            Clause::Quiet => format!("{}-x{mult}", bytes_name(threshold)),
            _ => bytes_name(threshold),
        };
        Quantity {
            name: format!("{}-{t}.{scale}", clause.as_str()),
            clause,
            threshold,
            mult,
            n,
        }
    };
    let mut g = Vec::new();
    for n in [100_000, 1_000_000] {
        for ops in [4_096, 2_048, 1_024] {
            g.push(q(Clause::Ops, ops, 1, n));
        }
    }
    for b in [4 * MIB, 2 * MIB, MIB] {
        g.push(q(Clause::Rec, b, 1, 1_000_000));
    }
    for b in [32 * MIB, 16 * MIB, 8 * MIB] {
        g.push(q(Clause::Body, b, 1, 1_000_000));
    }
    for b in [MIB, 512 * KIB] {
        g.push(q(Clause::Ovl, b, 1, 1_000_000));
    }
    for p10 in [2 * MIB, 1_536 * KIB] {
        for m in [8, 4] {
            g.push(q(Clause::Quiet, p10, m, 1_000_000));
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
    lead
}

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

/// The generator's target of `q` at `profile`.
pub fn target(q: &Quantity, profile: Profile) -> Target {
    let p = profile.params();
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
        seed: seed(&q.name),
        params: p,
        rt_bound: None,
    }
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
            let (mut replay, mut floor) = arms(vfs, &root, &guard.fx, &tail, ticker);
            let mut lines = Vec::new();
            for arm in [&mut replay as &mut dyn Arm, &mut floor] {
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
    let (mut replay, mut floor) = arms(sys.vfs, &root, &guard.fx, &tail, &sys.ticker);
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
    let mut rec = match measure(&mut ctx, &spec, &mut [&mut replay, &mut floor]) {
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

    /// 18 quantities with valid, unique names ([MP §4.1], `measurement-protocol.md:169`): 6 `ops`, 3 `rec`, 3 `body`,
    /// 2 `ovl`, 4 `quiet`.
    #[test]
    fn grid_has_eighteen_valid_quantities() {
        let g = grid();
        assert_eq!(g.len(), 18);
        assert!(g.iter().all(|q| valid_name(&q.name)), "{g:?}");
        let names: BTreeSet<&str> = g.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(names.len(), 18, "unique");
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
            [6, 3, 3, 2, 4]
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
    }

    /// The thresholds are F17's candidates (`17-store-parameters.md:757-762`; `docs/spec/HOLES.md:41-46`): P06 4,096,
    /// 2,048, 1,024 at 1e5 and 1e6; P07 4, 2, 1 MiB; P08 32, 16, 8 MiB; P09 1 MiB, 512 KiB; P10 2 MiB, 1.5 MiB × P11 8, 4.
    #[test]
    fn grid_thresholds_are_f17_candidates() {
        let g = grid();
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

    /// `--smoke` runs every quantity at the test profile with each arm once: 36 lines, one per (quantity, arm), each
    /// fixture's floor mapping 8 files; with `--quantity` it runs that one alone.
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
        assert_eq!(lines.len(), 36, "{out}");
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
                [
                    (q.name.clone(), "replay".into()),
                    (q.name.clone(), "floor.open".into()),
                ]
            })
            .collect();
        assert_eq!(pairs, expect);
        let log = fs.log();
        assert_eq!(log.maps.len(), 18 * SEALED_FILES);
        assert_eq!(log.opens.iter().filter(|o| *o == "HEAD").count(), 18);
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
        assert_eq!(out.lines().count(), 36);
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
        assert_eq!((code, out.lines().count()), (EXIT_DONE, 18));
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
            assert_eq!(calls.len(), 18);
            for (c, q) in calls.iter().zip(grid()) {
                let mut want = base.clone();
                want.push("--quantity".into());
                want.push(q.name.into());
                assert_eq!(c, &want);
            }
            assert_eq!(String::from_utf8(out).unwrap().lines().count(), 18);
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

    /// `--list` prints one JSON line per quantity, in grid order, with the tail measures of [F17 §5.1], the binding
    /// clause, and the allocator's count (`null` without `CountingAlloc`).
    #[test]
    fn list_prints_the_tail_measures() {
        let fs = MemFs::with_dir(&scratch());
        let (code, out, err) = run(&fs, &["--list"]);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 18);
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
                ("ovl", m.ovl),
                ("commits", m.commits),
                ("settles", m.settles),
                ("bytes", m.bytes),
            ] {
                assert_eq!(v[k].as_u64(), Some(x), "{} {k}", q.name);
            }
            assert!(m.ovl > 0 && m.bytes > 0);
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
}
