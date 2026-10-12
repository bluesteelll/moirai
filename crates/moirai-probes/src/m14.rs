//! Measurement 14 (WP-53e, [60 §5.2] item 14, `60-roadmap.md:770`; `docs/m0/PLAN.md:317`): the layout probes over
//! the F09 segment of [`crate::seg`], at 1e5 and 1e6 nodes, and the T1 and T2 trigger quantities: a point read after
//! a full 4,096-op tail and a delta checkpoint at 0.5 M nodes (T1, `ARCHITECTURE-RESEARCH.md:130`), and the MCP
//! overlay catch-up (T2, `:140`). T2's 16-writer wait is taken from measurement 2's run (R-24 Q7) and is not built
//! here. The tails are u10's ([`crate::tail::synth`]), from [70 S8]'s spec-derived mix (R-24 Q10).
//!
//! **The grid** (11 quantities, names by [MP §4.1]):
//!
//! | Quantity | Arms (arm 0 first) | What one sample does | Minimum tier |
//! |---|---|---|---|
//! | `csr.n1e5`, `csr.n1e6` | `expand` | reads one node's out-list and in-list from the six CSR columns (`09-segments.md:479-505`) | — |
//! | `bitset.n1e5`, `bitset.n1e6` | `and.popcount` | ANDs the kind bitset with one kind-status bitset and counts ([50] F6) | — |
//! | `scan.n1e5`, `scan.n1e6` | `scan.status`, `scan.due` | counts the rows of one status in `NODE`'s 60-byte rows; the floor counts the non-zero elements of the dense `DUE` column, the layout a per-field split would give (OP-09-21, `09-segments.md:1369-1372`) | — |
//! | `overlay.n1e5`, `overlay.n1e6` | `probe` | looks one `#N` up in a full tail's node index ("overlay probe ~0.2 µs", `ARCHITECTURE-RESEARCH.md:968`) | — |
//! | `read-4096.n1e6` | `read` | T1's point read: the status of a node the tail touched, through the overlay and the tail's records, else the base row | t1 (read ≤ 50 µs) |
//! | `ckpt-4096.n5e5` | `ckpt`, `ckpt.write` | T1's delta checkpoint: replays the tail from its extents, folds it over the base, encodes `seg.d1`, then `create_new`, `write_at`, `sync(DataAndMeta)`, `seal`, `sync_dir` (`docs/spec/os/fs.md:411`, `:456`); the floor writes and flushes the same bytes without the fold. The unlink of the previous sample's file is setup, outside the timed region ([MP §4.1]) | t3 (≤ 100 ms) |
//! | `catchup-16.n1e6` | `catchup` | T2's MCP model: a long-lived process holds the overlay of a full tail but its last 16 commits (one per writer of T2's burst) and catches up: `HEAD` pread (8,192 B), the new records scanned from the extents, applied to its warm builder, the overlay finished again (`ARCHITECTURE-RESEARCH.md:783`). The warm builder is cloned outside the timed region | t2 (catch-up p50 ≤ 5 ms) |
//!
//! Every run's fixture is the base `seg.base.1` at the quantity's N ([`crate::seg::base`]), written, sealed and
//! mapped, and, for the last four families, a full `ops` tail at P06 (4,096 at production) written as `log.<n>`
//! extents with `HEAD`; it is removed on every path out of the run. The probe's choices: the ids a probe cycles
//! through ([`PROBE_IDS`] drawn from the quantity's seed; the read's are the tail's touched nodes), the families'
//! single arms, and the catch-up's burst.
//!
//! **The driver** ([MP §9.7] as merged, `measurement-protocol.md:777-780`; the flags of `m10` beside it): `--host`,
//! `--condition`, `--load-log`, `--out`, `--quantity`, `--commit`, `--toolchain`, `--scratch`, `--smoke` (every
//! quantity at the test profile, N / 100 and its parameters, each arm once, nothing recorded) and `--help`. Without
//! `--quantity`, each quantity runs in a child process of its own ([MP §4.6]). Exit 0 done, 1 a run or arm failed or a
//! record could not be written, 2 a usage error.

use crate::arm::{Arm, ArmError, Ticker, TimedFn};
use crate::condition::{Condition, ConditionKind};
use crate::host::{CommandRunner, HostKind};
use crate::load::validate::{COVER_WAIT, read_log_file, verdict_when_covered};
use crate::overlay::{Builder, Key, Overlay, Probe, Value, kind_key, kind_status_key};
use crate::run::{Context, RunSpec, measure};
use crate::seg::base::{self, Columns};
use crate::seg::bitset::{Frozen, and_popcount};
use crate::seg::delta;
use crate::seg::{NODE_HDR_LEN, STATUS_AT, u32_at};
use crate::tail::rec::Payload;
use crate::tail::scan::{LogSource, MemLog, scan};
use crate::tail::store::{VfsLog, extent_name};
use crate::tail::synth::{BASE_KIND, Clause, STATUSES, Tail, Target, generate};
use crate::tail::{HEAD_LEN, Params, TAIL_READ_CHUNK};
use crate::tier::Tier;
use moirai_vfs::{
    Access, Meter, OS_SHARE_RETRY_MS, OpenHint, RelPath, RelPathBuf, RootAccess, RootRole,
    SealedMap, SealedMaps, ShareRetry, StoreFs, SyncKind, VfsErrorKind,
};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The measurement's row number.
pub const MEASUREMENT: u32 = 14;
/// Exit: done.
pub const EXIT_DONE: u8 = 0;
/// Exit: a run or arm failed, or a record could not be written.
pub const EXIT_FAILED: u8 = 1;
/// Exit: a usage error.
pub const EXIT_USAGE: u8 = 2;
/// The commits the MCP model catches up: one per writer of T2's 16 (`ARCHITECTURE-RESEARCH.md:140`).
pub const BURST: usize = 16;
/// The ids a probe arm cycles through.
pub const PROBE_IDS: usize = 4_096;
/// The test profile's N is the quantity's N / 100.
pub const TEST_SCALE: u32 = 100;
/// The base's `base_seq`: the tail's first commit has `seq` 1,000 (`tail/synth.rs`).
pub const BASE_SEQ: u64 = 999;
/// The base's file.
pub const BASE_NAME: &str = "seg.base.1";
/// The checkpoint's delta.
pub const DELTA_NAME: &str = "seg.d1";
/// The checkpoint floor's file.
pub const FLOOR_NAME: &str = "seg.d2";

/// A probe family.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Family {
    /// The CSR probe.
    Csr,
    /// Frozen-bitset AND/popcount.
    Bitset,
    /// Column scans.
    Scan,
    /// The overlay probe.
    Overlay,
    /// T1's point read.
    Read,
    /// T1's delta checkpoint.
    Ckpt,
    /// T2's MCP overlay catch-up.
    Catchup,
}

impl Family {
    /// The quantity name's stem.
    pub const fn stem(self) -> &'static str {
        match self {
            Family::Csr => "csr",
            Family::Bitset => "bitset",
            Family::Scan => "scan",
            Family::Overlay => "overlay",
            Family::Read => "read-4096",
            Family::Ckpt => "ckpt-4096",
            Family::Catchup => "catchup-16",
        }
    }

    /// The arms, arm 0 first.
    pub const fn arms(self) -> &'static [&'static str] {
        match self {
            Family::Csr => &["expand"],
            Family::Bitset => &["and.popcount"],
            Family::Scan => &["scan.status", "scan.due"],
            Family::Overlay => &["probe"],
            Family::Read => &["read"],
            Family::Ckpt => &["ckpt", "ckpt.write"],
            Family::Catchup => &["catchup"],
        }
    }

    /// The minimum tier: the tier of the quantity's budget ([MP §3.2]); the layout probes have none.
    pub const fn min_tier(self) -> Option<Tier> {
        match self {
            Family::Read => Some(Tier::T1),
            Family::Ckpt => Some(Tier::T3),
            Family::Catchup => Some(Tier::T2),
            _ => None,
        }
    }

    /// Whether the fixture holds a tail.
    pub const fn needs_tail(self) -> bool {
        matches!(
            self,
            Family::Overlay | Family::Read | Family::Ckpt | Family::Catchup
        )
    }
}

/// One quantity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quantity {
    /// The name.
    pub name: String,
    /// The family.
    pub family: Family,
    /// N at production.
    pub n: u32,
}

/// The 11 quantities in run order.
pub fn grid() -> Vec<Quantity> {
    let q = |family: Family, n: u32| {
        let scale = match n {
            100_000 => "n1e5",
            500_000 => "n5e5",
            _ => "n1e6",
        };
        Quantity {
            name: format!("{}.{scale}", family.stem()),
            family,
            n,
        }
    };
    let mut g = Vec::new();
    for n in [100_000, 1_000_000] {
        for f in [Family::Csr, Family::Bitset, Family::Scan, Family::Overlay] {
            g.push(q(f, n));
        }
    }
    g.push(q(Family::Read, 1_000_000));
    g.push(q(Family::Ckpt, 500_000));
    g.push(q(Family::Catchup, 1_000_000));
    g
}

/// The scale a run is made at.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Profile {
    /// The quantity's N and the production parameters.
    Production,
    /// N / 100 and the test profile's parameters (`17-store-parameters.md:166-178`), the smoke's.
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

    /// N at this profile.
    pub fn n(self, n: u32) -> u32 {
        match self {
            Profile::Production => n,
            Profile::Test => (n / TEST_SCALE).max(base::MIN_N),
        }
    }
}

/// A quantity's seed: a function of its name.
fn seed(name: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(name.as_bytes())
}

/// The tail of `q` at `profile`: a full `ops` tail at P06 over the base.
pub fn target(q: &Quantity, profile: Profile) -> Target {
    let p = profile.params();
    Target {
        clause: Clause::Ops,
        threshold: p.p06,
        mult: 1,
        n: profile.n(q.n),
        seed: seed(&q.name),
        params: p,
        rt_bound: None,
    }
}

fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `count` ids uniform in `1..=n`.
pub fn draw_ids(n: u32, count: usize, seed: u64) -> Vec<u32> {
    (0..count as u64)
        .map(|i| 1 + (mix(seed ^ i.wrapping_mul(0xA24B_AED4_963E_E407)) % u64::from(n)) as u32)
        .collect()
}

/// The CSR probe: row #`id`'s out-list and in-list read from the six columns, summed so nothing is dropped.
pub fn expand(c: &Columns<'_>, id: u32) -> u64 {
    let r = id as usize - 1;
    let mut acc = 0u64;
    for [off, ids, kinds] in [c.out, c.inn] {
        let (a, b) = (u32_at(off, r) as usize, u32_at(off, r + 1) as usize);
        let (words, _) = ids[4 * a..4 * b].as_chunks::<4>();
        for (k, x) in kinds[a..b].iter().zip(words) {
            acc += u64::from(u32::from_le_bytes(*x)) + u64::from(*k);
        }
    }
    acc
}

/// The column scan over `NODE`: the rows whose status is `s`, read at offset 1 of each 60-byte row.
pub fn scan_status(c: &Columns<'_>, s: u8) -> u64 {
    let (rows, _) = c.node.as_chunks::<NODE_HDR_LEN>();
    rows.iter().filter(|row| row[STATUS_AT] == s).count() as u64
}

/// The floor scan over the dense `DUE` column: its non-zero elements.
pub fn scan_due(c: &Columns<'_>) -> u64 {
    let (due, _) = c.due.as_chunks::<4>();
    due.iter().filter(|e| **e != [0; 4]).count() as u64
}

/// The overlay probe: whether the tail touched `#id`.
pub fn probe(o: &Overlay, id: u32) -> bool {
    o.nodes().binary_search_by_key(&id, |e| e.n).is_ok()
}

/// T1's point read: the status of `#id` after the tail, from the overlay and the records along the owner's chain
/// ([`Overlay::get`]), else from the base row's `NodeHdr.status`.
pub fn point_read<S: LogSource>(
    log: &mut S,
    o: &Overlay,
    c: &Columns<'_>,
    id: u32,
) -> Result<u8, String> {
    match o.get(log, id, &Key::Status)? {
        Probe::Tail(Value::Status(s, _)) => Ok(s),
        Probe::Tail(v) => Err(format!("#{id}'s status probed as {v:?}")),
        Probe::NotInTail if id >= 1 && id as usize <= c.rows => {
            Ok(c.node[NODE_HDR_LEN * (id as usize - 1) + STATUS_AT])
        }
        Probe::NotInTail => Err(format!("#{id} has no base row")),
    }
}

/// The MCP model's warm state: the boundary `x` before the tail's last `burst` commits (keeping at least one commit
/// warm), the chain value there, and the builder of `[start, x)`.
pub fn warm(t: &Tail, burst: usize) -> Result<(u64, u64, Builder), String> {
    let commits: Vec<u64> = t
        .records
        .iter()
        .filter(|r| matches!(r.payload, Payload::Commit(_)))
        .map(|r| r.lsn)
        .collect();
    let k = commits.len();
    let b = burst.min(k.saturating_sub(1)).max(1);
    let x = if b < k { commits[k - b] } else { t.start };
    let mut builder = Builder::new(t.start, t.base);
    let chain = scan(
        &mut MemLog::new(t.start, &t.bytes),
        &t.expect,
        t.start,
        x,
        t.chain_in,
        TAIL_READ_CHUNK,
        &mut |g| builder.apply_group(g),
    )
    .map_err(|e| e.to_string())?;
    Ok((x, chain, builder))
}

/// Catches `b` up over `[from, to)` of `log` (chain value `chain_in` at `from`) and finishes its overlay.
pub fn catch_up<S: LogSource>(
    log: &mut S,
    t: &Tail,
    from: u64,
    chain_in: u64,
    mut b: Builder,
) -> Result<Overlay, String> {
    scan(
        log,
        &t.expect,
        from,
        t.end,
        chain_in,
        TAIL_READ_CHUNK,
        &mut |g| b.apply_group(g),
    )
    .map_err(|e| e.to_string())?;
    Ok(b.finish())
}

/// T1's checkpoint fold: the whole tail of `t`, read from `log`, over the base `c`, into `seg.d1`.
pub fn fold_tail<S: LogSource>(
    log: &mut S,
    t: &Tail,
    c: &Columns<'_>,
) -> Result<delta::Delta, String> {
    delta::fold(
        log,
        &t.expect,
        t.start,
        t.end,
        t.chain_in,
        TAIL_READ_CHUNK,
        c,
        1,
    )
}

/// A time arm whose sample runs `setup` untimed, then times `op` on its result ([MP §4.1]: setup stays outside the
/// timed region). It cannot batch, so its operation must take 20 × the timer's resolution ([MP §4.5]).
pub struct Staged<S, P, T> {
    name: String,
    setup: S,
    op: P,
    ticker: T,
}

impl<S, P, T> Staged<S, P, T> {
    /// A staged arm named `name`.
    pub fn new(name: impl Into<String>, ticker: T, setup: S, op: P) -> Staged<S, P, T> {
        Staged {
            name: name.into(),
            setup,
            op,
            ticker,
        }
    }
}

impl<S, P, T, X, O> Arm for Staged<S, P, T>
where
    S: FnMut() -> Result<X, ArmError>,
    P: FnMut(X) -> Result<O, ArmError>,
    T: Ticker,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn unit(&self) -> crate::units::Unit {
        crate::units::Unit::Ns
    }

    fn sample(&mut self) -> Result<u64, ArmError> {
        let x = (self.setup)()?;
        let t0 = self.ticker.now_ns();
        let o = (self.op)(x)?;
        let t1 = self.ticker.now_ns();
        std::hint::black_box(o);
        Ok(t1.saturating_sub(t0))
    }
}

fn rel(s: &str) -> Result<RelPathBuf, String> {
    RelPathBuf::from_string(s.to_string()).map_err(|e| e.to_string())
}

fn arm_err(e: impl std::fmt::Display) -> ArmError {
    ArmError(e.to_string())
}

/// Removes `name` from `root` if it is there (setup of the checkpoint arms).
fn unlink_if<V: StoreFs>(vfs: &V, root: &V::Root, name: &str) -> Result<(), ArmError> {
    let retry = ShareRetry::Bounded {
        total_ms: OS_SHARE_RETRY_MS,
    };
    match vfs.unlink(root, rel(name).map_err(ArmError)?.as_rel_path(), retry) {
        Err(e) if e.kind != VfsErrorKind::NotFound => Err(arm_err(e)),
        _ => Ok(()),
    }
}

/// A sealed file's write ([OS/fs] `fs.md:411`, `:456`): `create_new` under its final name, `write_at`,
/// `sync(DataAndMeta)`, `seal`, then `sync_dir`. Returns the bytes written.
pub fn seal_new<V: StoreFs>(
    vfs: &V,
    root: &V::Root,
    name: &str,
    bytes: &[u8],
) -> Result<u64, ArmError> {
    let f = vfs
        .create_new(root, rel(name).map_err(ArmError)?.as_rel_path())
        .map_err(arm_err)?;
    vfs.write_at(&f, 0, bytes).map_err(arm_err)?;
    vfs.sync(&f, SyncKind::DataAndMeta).map_err(arm_err)?;
    vfs.seal(&f).map_err(arm_err)?;
    drop(f);
    vfs.sync_dir(root, None).map_err(arm_err)?;
    Ok(bytes.len() as u64)
}

/// Removes its fixture when dropped, on every path out of a run: the checkpoint's files if present, then every file
/// written, then the directory.
struct FixtureGuard<'v, V: StoreFs> {
    vfs: &'v V,
    parent: PathBuf,
    dir: PathBuf,
    name: String,
    files: Vec<String>,
}

impl<V: StoreFs> FixtureGuard<'_, V> {
    fn remove(&self) -> Result<(), String> {
        let io = |e: moirai_vfs::VfsError| e.to_string();
        {
            let root = self
                .vfs
                .open_root(&self.dir, RootRole::Other, RootAccess::ReadWrite)
                .map_err(io)?;
            for f in [DELTA_NAME, FLOOR_NAME] {
                unlink_if(self.vfs, &root, f).map_err(|e| e.0)?;
            }
            let retry = ShareRetry::Bounded {
                total_ms: OS_SHARE_RETRY_MS,
            };
            for f in &self.files {
                self.vfs
                    .unlink(&root, rel(f)?.as_rel_path(), retry)
                    .map_err(io)?;
            }
        }
        let up = self
            .vfs
            .open_root(&self.parent, RootRole::Other, RootAccess::ReadWrite)
            .map_err(io)?;
        self.vfs
            .remove_dir(&up, rel(&self.name)?.as_rel_path())
            .map_err(io)
    }
}

impl<V: StoreFs> Drop for FixtureGuard<'_, V> {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

/// A run's fixture: the base mapped, the root, the tail. Fields drop in order, so the map and the root are released
/// before the guard removes the files.
pub struct Prepared<'v, V: StoreFs + SealedMaps> {
    /// The base, mapped.
    pub map: V::Map,
    /// The fixture's root.
    pub root: V::Root,
    /// The tail, for the families that read one.
    pub tail: Option<Tail>,
    /// N at the run's profile.
    pub n: u32,
    /// The quantity's seed.
    pub seed: u64,
    /// The store seam.
    pub vfs: &'v V,
    guard: FixtureGuard<'v, V>,
}

impl<V: StoreFs + SealedMaps> Prepared<'_, V> {
    /// The fixture's directory.
    pub fn dir(&self) -> &Path {
        &self.guard.dir
    }
}

/// Writes `q`'s fixture under `scratch` at `profile`: the base sealed, the tail's extents and `HEAD`; maps the base.
pub fn prepare<'v, V: StoreFs + SealedMaps>(
    vfs: &'v V,
    scratch: &Path,
    q: &Quantity,
    profile: Profile,
) -> Result<Prepared<'v, V>, String> {
    let p = profile.params();
    let n = profile.n(q.n);
    let tail = if q.family.needs_tail() {
        Some(generate(&target(q, profile), true)?)
    } else {
        None
    };
    let name = format!("moirai-m14-{}-{}", std::process::id(), q.name);
    let dir = scratch.join(&name);
    let mut guard = FixtureGuard {
        vfs,
        parent: scratch.to_path_buf(),
        dir: dir.clone(),
        name,
        files: Vec::new(),
    };
    let io = |e: moirai_vfs::VfsError| e.to_string();
    let root = vfs
        .create_root(&dir, RootRole::Other)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let bytes = base::build(&base::Spec {
        n,
        upto_lsn: p.e,
        base_seq: BASE_SEQ,
    })?;
    let len = bytes.len() as u64;
    guard.files.push(BASE_NAME.into());
    seal_new(vfs, &root, BASE_NAME, &bytes).map_err(|e| e.0)?;
    drop(bytes);
    if let Some(t) = &tail {
        let geo = t.expect.geo;
        for k in geo.extent(t.start)..=geo.extent(t.end - 1) {
            let from = geo.extent_start(k).max(t.start);
            let to = geo.extent_start(k + 1).min(t.end);
            let fname = extent_name(k);
            let f = vfs
                .create_new(&root, rel(&fname)?.as_rel_path())
                .map_err(io)?;
            guard.files.push(fname);
            let b = &t.bytes[(from - t.start) as usize..(to - t.start) as usize];
            vfs.write_at(&f, geo.offset(from), b).map_err(io)?;
            if geo.offset(to) != 0 {
                vfs.write_at(&f, geo.e() - 1, &[0]).map_err(io)?;
            }
        }
        let head = vfs
            .create_new(&root, RelPath::literal("HEAD"))
            .map_err(io)?;
        guard.files.push("HEAD".into());
        let mut slots = vec![0u8; HEAD_LEN];
        slots[..4].copy_from_slice(b"MOIR");
        slots[4_096..4_100].copy_from_slice(b"MOIR");
        vfs.write_at(&head, 0, &slots).map_err(io)?;
    }
    let base_rel = rel(BASE_NAME)?;
    let f = vfs
        .open(
            &root,
            base_rel.as_rel_path(),
            Access::Read,
            OpenHint::Normal,
        )
        .map_err(io)?;
    let map = vfs
        .map_sealed(&f, len, base_rel.as_rel_path())
        .map_err(|e| e.to_string())?;
    drop(f);
    Ok(Prepared {
        map,
        root,
        tail,
        n,
        seed: seed(&q.name),
        vfs,
        guard,
    })
}

/// The arms of `q` over a prepared fixture, arm 0 first; their setup (the overlay, the warm builder, the floor's
/// bytes) runs here, outside every timed region.
pub fn arms<'p, V: StoreFs + SealedMaps, T: Ticker + Clone + 'p>(
    p: &'p Prepared<'_, V>,
    c: &'p Columns<'p>,
    q: &Quantity,
    ticker: &T,
) -> Result<Vec<Box<dyn Arm + 'p>>, String> {
    let mut i = 0usize;
    let tail = || p.tail.as_ref().ok_or("the fixture holds no tail");
    let mut arms: Vec<Box<dyn Arm + 'p>> = Vec::new();
    match q.family {
        Family::Csr => {
            let ids = draw_ids(p.n, PROBE_IDS, p.seed);
            arms.push(Box::new(TimedFn::new(
                "expand",
                ticker.clone(),
                move || {
                    i += 1;
                    Ok(expand(c, ids[i % ids.len()]))
                },
            )));
        }
        Family::Bitset => {
            let kind = c.frozen(kind_key(BASE_KIND))?;
            let sets = (0..STATUSES)
                .map(|s| c.frozen(kind_status_key(BASE_KIND, s)))
                .collect::<Result<Vec<Frozen<'p>>, String>>()?;
            arms.push(Box::new(TimedFn::new(
                "and.popcount",
                ticker.clone(),
                move || {
                    i += 1;
                    Ok(and_popcount(&kind, &sets[i % sets.len()]))
                },
            )));
        }
        Family::Scan => {
            arms.push(Box::new(TimedFn::new(
                "scan.status",
                ticker.clone(),
                move || {
                    i += 1;
                    Ok(scan_status(c, (i % usize::from(STATUSES)) as u8))
                },
            )));
            arms.push(Box::new(TimedFn::new(
                "scan.due",
                ticker.clone(),
                move || Ok(scan_due(c)),
            )));
        }
        Family::Overlay => {
            let o = tail()?.overlay()?;
            let ids = draw_ids(p.n, PROBE_IDS, p.seed);
            arms.push(Box::new(TimedFn::new("probe", ticker.clone(), move || {
                i += 1;
                Ok(probe(&o, ids[i % ids.len()]))
            })));
        }
        Family::Read => {
            let t = tail()?;
            let o = t.overlay()?;
            let mut ids: Vec<u32> = o.nodes().iter().map(|e| e.n).collect();
            ids.sort_by_key(|&id| mix(p.seed ^ u64::from(id)));
            let mut log = VfsLog::new(p.vfs, &p.root, t.expect.geo);
            arms.push(Box::new(TimedFn::new("read", ticker.clone(), move || {
                i += 1;
                point_read(&mut log, &o, c, ids[i % ids.len()]).map_err(ArmError)
            })));
        }
        Family::Ckpt => {
            let t = tail()?;
            let bytes = delta::encode(&fold_tail(&mut MemLog::new(t.start, &t.bytes), t, c)?)?;
            let mut log = VfsLog::new(p.vfs, &p.root, t.expect.geo);
            arms.push(Box::new(Staged::new(
                "ckpt",
                ticker.clone(),
                move || unlink_if(p.vfs, &p.root, DELTA_NAME),
                move |()| {
                    let d = fold_tail(&mut log, t, c).map_err(ArmError)?;
                    let b = delta::encode(&d).map_err(ArmError)?;
                    seal_new(p.vfs, &p.root, DELTA_NAME, &b)
                },
            )));
            arms.push(Box::new(Staged::new(
                "ckpt.write",
                ticker.clone(),
                move || unlink_if(p.vfs, &p.root, FLOOR_NAME),
                move |()| seal_new(p.vfs, &p.root, FLOOR_NAME, &bytes),
            )));
        }
        Family::Catchup => {
            let t = tail()?;
            let (x, chain, warm) = warm(t, BURST)?;
            let head = p
                .vfs
                .open(
                    &p.root,
                    RelPath::literal("HEAD"),
                    Access::Read,
                    OpenHint::Normal,
                )
                .map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; HEAD_LEN];
            let mut log = VfsLog::new(p.vfs, &p.root, t.expect.geo);
            arms.push(Box::new(Staged::new(
                "catchup",
                ticker.clone(),
                move || -> Result<Builder, ArmError> { Ok(warm.clone()) },
                move |b: Builder| {
                    let got = p.vfs.read_at(&head, 0, &mut buf).map_err(arm_err)?;
                    if got != HEAD_LEN {
                        return Err(ArmError(format!("HEAD read {got} bytes, not 8,192")));
                    }
                    catch_up(&mut log, t, x, chain, b)
                        .map(|o| o.entries())
                        .map_err(ArmError)
                },
            )));
        }
    }
    Ok(arms)
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
pub const USAGE: &str = "usage: m14 --host laptop|hosted --condition idle|loaded|synthetic [--load-log <file>] --out <dir>
           [--quantity <name>] [--commit <text>] [--toolchain <text>] [--scratch <dir>]
       m14 --smoke [--scratch <dir>]
       m14 --help
";

#[derive(Debug, Default)]
struct Args {
    host: Option<HostKind>,
    condition: Option<ConditionKind>,
    load_log: Option<PathBuf>,
    out: Option<PathBuf>,
    quantity: Option<String>,
    commit: Option<String>,
    toolchain: Option<String>,
    scratch: Option<PathBuf>,
    smoke: bool,
    help: bool,
}

fn parse(args: &[OsString]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let flag = flag.to_str().ok_or("an argument is not valid Unicode")?;
        let mut value = |what: &str| -> Result<String, String> {
            it.next()
                .and_then(|v| v.to_str())
                .map(str::to_string)
                .ok_or_else(|| format!("{what} needs a value"))
        };
        match flag {
            "--host" => {
                let v = value("--host")?;
                a.host = Some(HostKind::parse(&v).ok_or(format!("unknown host '{v}'"))?);
            }
            "--condition" => {
                let v = value("--condition")?;
                a.condition =
                    Some(ConditionKind::parse(&v).ok_or(format!("unknown condition '{v}'"))?);
            }
            "--load-log" => a.load_log = Some(value("--load-log")?.into()),
            "--out" => a.out = Some(value("--out")?.into()),
            "--quantity" => a.quantity = Some(value("--quantity")?),
            "--commit" => a.commit = Some(value("--commit")?),
            "--toolchain" => a.toolchain = Some(value("--toolchain")?),
            "--scratch" => a.scratch = Some(value("--scratch")?.into()),
            "--smoke" => a.smoke = true,
            "--help" | "-h" => a.help = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if let Some(q) = &a.quantity
        && !grid().iter().any(|g| &g.name == q)
    {
        return Err(format!("unknown quantity '{q}'"));
    }
    if a.help || a.smoke {
        return Ok(a);
    }
    if a.host.is_none() || a.condition.is_none() || a.out.is_none() {
        return Err("a recording run needs --host, --condition and --out".into());
    }
    if a.condition != Some(ConditionKind::Idle) && a.load_log.is_none() {
        return Err("a loaded or synthetic run needs --load-log".into());
    }
    Ok(a)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Writes `rec` as a new file `<dir>/<raw file name>` ([MP §7.2]): created new, streamed, removed on a failed write.
fn write_record(rec: &crate::record::RunRecord, dir: &Path) -> std::io::Result<PathBuf> {
    let name = rec.raw_file_name().map_err(std::io::Error::other)?;
    let path = dir.join(name);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let finish = |file: std::fs::File| -> std::io::Result<()> {
        let mut w = std::io::BufWriter::new(file);
        rec.write_json(&mut w)?;
        w.write_all(b"\n")?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()
    };
    match finish(file) {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

fn smoke<V: StoreFs + SealedMaps, T: Ticker + Clone>(
    vfs: &V,
    ticker: &T,
    scratch: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let mut code = EXIT_DONE;
    for q in grid() {
        let run = || -> Result<Vec<(String, u64)>, String> {
            let p = prepare(vfs, scratch, &q, Profile::Test)?;
            let c = Columns::parse(p.map.bytes())?;
            let mut arms = arms(&p, &c, &q, ticker)?;
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
                    let _ = writeln!(out, "m14 smoke {} {arm} ok {ns} ns", q.name);
                }
            }
            Err(e) => {
                let _ = writeln!(err, "m14 smoke {} FAILED: {e}", q.name);
                code = EXIT_FAILED;
            }
        }
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
    let condition = match (a.condition, &a.load_log) {
        (Some(ConditionKind::Idle), _) => Condition::Idle,
        (Some(kind), Some(log)) => match read_log_file(log) {
            Ok(l) if l.header.condition().kind() == kind => l.header.condition(),
            Ok(l) => {
                let _ = writeln!(
                    err,
                    "m14: --condition {} but the load log's header gives {}",
                    kind.as_str(),
                    l.header.condition().kind().as_str()
                );
                return EXIT_USAGE;
            }
            Err(e) => {
                let _ = writeln!(err, "m14: the load log: {e}");
                return EXIT_FAILED;
            }
        },
        _ => {
            let _ = writeln!(err, "m14: a loaded or synthetic run needs --load-log");
            return EXIT_USAGE;
        }
    };
    let scratch = a.scratch.clone().unwrap_or_else(std::env::temp_dir);
    let p = match prepare(sys.vfs, &scratch, q, sys.profile) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "m14 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let c = match Columns::parse(p.map.bytes()) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "m14 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let mut arms = match arms(&p, &c, q, &sys.ticker) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "m14 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let spec = RunSpec {
        measurement: MEASUREMENT,
        quantity: q.name.clone(),
        condition,
        min_tier: q.family.min_tier(),
    };
    let mut ctx = Context {
        meter: sys.meter,
        ticker: sys.ticker.clone(),
        runner: sys.runner,
        host: a.host.unwrap_or(HostKind::Laptop),
        toolchain: a.toolchain.clone().unwrap_or_else(|| "unknown".into()),
        commit: a.commit.clone().unwrap_or_else(|| "unknown".into()),
    };
    let t0 = now_ms();
    let mut refs: Vec<&mut dyn Arm> = arms.iter_mut().map(|b| &mut **b as &mut dyn Arm).collect();
    let mut rec = match measure(&mut ctx, &spec, &mut refs) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "m14 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let t1 = now_ms();
    if a.condition == Some(ConditionKind::Loaded)
        && let Some(log) = &a.load_log
    {
        match verdict_when_covered(log, t0, t1, COVER_WAIT) {
            Ok((_, v)) => rec.set_load_replay(v.pass()),
            Err(e) => {
                let _ = writeln!(err, "m14 {}: the load log's verdict: {e}", q.name);
                rec.set_load_replay(false);
            }
        }
    }
    let dir = a.out.clone().unwrap_or_default();
    match write_record(&rec, &dir) {
        Ok(path) => {
            let _ = writeln!(out, "m14 {} recorded {}", q.name, path.display());
            EXIT_DONE
        }
        Err(e) => {
            let _ = writeln!(err, "m14 {}: the record could not be written: {e}", q.name);
            EXIT_FAILED
        }
    }
}

/// The driver's command line: parses `args` (without the program name), then runs the smoke, one quantity, or every
/// quantity in a child process of its own; returns the exit code.
pub fn cli<V: StoreFs + SealedMaps, M: Meter, T: Ticker + Clone, R: CommandRunner>(
    args: &[OsString],
    sys: System<'_, V, M, T, R>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "m14: {e}");
            let _ = err.write_all(USAGE.as_bytes());
            return EXIT_USAGE;
        }
    };
    if a.help {
        let _ = out.write_all(USAGE.as_bytes());
        return EXIT_DONE;
    }
    if a.smoke {
        let scratch = a.scratch.clone().unwrap_or_else(std::env::temp_dir);
        return smoke(sys.vfs, &sys.ticker, &scratch, out, err);
    }
    if let Some(name) = &a.quantity {
        let q = grid()
            .into_iter()
            .find(|g| &g.name == name)
            .expect("checked by parse");
        return run_one(sys, &a, &q, out, err);
    }
    let mut worst = EXIT_DONE;
    for q in grid() {
        let mut child = args.to_vec();
        child.push("--quantity".into());
        child.push(q.name.clone().into());
        let code = match (sys.spawn)(&child) {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(err, "m14 {}: {e}", q.name);
                EXIT_FAILED
            }
        };
        let _ = writeln!(out, "m14 {} exit {code}", q.name);
        worst = worst.max(code);
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm::{MonoTicker, valid_name};
    use crate::host::tests::Canned;
    use crate::record::RunRecord;
    use crate::seg::delta::decode;
    use crate::tail::commit::Op;
    use crate::tail::memfs::MemFs;
    use crate::tail::store::tests::scratch;
    use crate::tail::synth::tests::test_target;
    use crate::testkit::{FakeMeter, FakeTicker, scratch_dir};
    use crate::tier::Plan;
    use crate::units::MS;
    use std::collections::{BTreeMap, BTreeSet};

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn no_spawn(_: &[OsString]) -> Result<u8, String> {
        Err("no child process in this test".into())
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

    /// A test-profile tail of `ops` ops over N = `n`, records kept, and its base's bytes.
    fn tail_and_base(n: u32, ops: u64, seed: u64) -> (Tail, Vec<u8>) {
        let mut t = test_target(Clause::Ops, n, seed);
        t.threshold = ops;
        t.rt_bound = Some(u64::MAX);
        let tail = generate(&t, true).unwrap();
        let b = base::build(&base::Spec {
            n,
            upto_lsn: tail.start,
            base_seq: BASE_SEQ,
        })
        .unwrap();
        (tail, b)
    }

    /// 11 quantities with valid, unique names ([MP §4.1]): the four layout families at 1e5 and 1e6, the read and the
    /// catch-up at 1e6, the checkpoint at 0.5 M; 14 arms; the minimum tiers of the budgets (t1 read ≤ 50 µs, t3
    /// checkpoint ≤ 100 ms, t2 catch-up ≤ 5 ms, `ARCHITECTURE-RESEARCH.md:130`, `:140`).
    #[test]
    fn grid_has_eleven_valid_quantities() {
        let g = grid();
        let names: Vec<&str> = g.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "csr.n1e5",
                "bitset.n1e5",
                "scan.n1e5",
                "overlay.n1e5",
                "csr.n1e6",
                "bitset.n1e6",
                "scan.n1e6",
                "overlay.n1e6",
                "read-4096.n1e6",
                "ckpt-4096.n5e5",
                "catchup-16.n1e6"
            ]
        );
        assert!(names.iter().all(|n| valid_name(n)));
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 11);
        let arms: Vec<&str> = g
            .iter()
            .flat_map(|q| q.family.arms().iter().copied())
            .collect();
        assert_eq!(arms.len(), 14);
        assert!(arms.iter().all(|a| valid_name(a)));
        let n_of = |name: &str| g.iter().find(|q| q.name == name).unwrap().n;
        assert_eq!(
            (
                n_of("csr.n1e5"),
                n_of("read-4096.n1e6"),
                n_of("ckpt-4096.n5e5")
            ),
            (100_000, 1_000_000, 500_000)
        );
        let tiers: Vec<Option<Tier>> = [Family::Read, Family::Ckpt, Family::Catchup, Family::Csr]
            .map(Family::min_tier)
            .to_vec();
        assert_eq!(
            tiers,
            [Some(Tier::T1), Some(Tier::T3), Some(Tier::T2), None]
        );
        let t = target(&g[9], Profile::Production);
        assert_eq!((t.clause, t.threshold, t.n), (Clause::Ops, 4_096, 500_000));
        assert_eq!(target(&g[9], Profile::Test).n, 5_000);
    }

    /// `--smoke` runs every quantity at the test profile with each arm once: 14 lines, one per (quantity, arm).
    #[test]
    fn smoke_exercises_every_arm_once() {
        let fs = MemFs::with_dir(&scratch());
        let (code, out, err) = run(&fs, &["--smoke", "--scratch", "/scratch"]);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 14, "{out}");
        let pairs: BTreeSet<(String, String)> = lines
            .iter()
            .map(|l| {
                let w: Vec<&str> = l.split(' ').collect();
                assert_eq!((w[0], w[1], w[4]), ("m14", "smoke", "ok"), "{l}");
                (w[2].to_string(), w[3].to_string())
            })
            .collect();
        let expect: BTreeSet<(String, String)> = grid()
            .iter()
            .flat_map(|q| {
                q.family
                    .arms()
                    .iter()
                    .map(|a| (q.name.clone(), a.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(pairs, expect);
        assert_eq!(fs.log().maps.len(), 11, "each run maps its base once");
    }

    /// `--smoke` with the contract flags records nothing: no file under `--out`, and every fixture is removed, the
    /// checkpoint's files included.
    #[test]
    fn smoke_records_nothing() {
        let out_dir = scratch_dir("m14-smoke");
        let fs = MemFs::with_dir(&scratch());
        let o = out_dir.to_str().unwrap().to_string();
        let (code, out, err) = run(
            &fs,
            &[
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--out",
                &o,
                "--smoke",
                "--scratch",
                "/scratch",
            ],
        );
        assert_eq!(code, EXIT_DONE, "{err}");
        assert_eq!(out.lines().count(), 14);
        assert_eq!(
            std::fs::read_dir(&out_dir).unwrap().count(),
            0,
            "nothing under --out"
        );
        assert!(
            fs.files().is_empty(),
            "every fixture removed: {:?}",
            fs.files()
        );
        assert!(
            !fs.has_dir(&scratch().join(format!("moirai-m14-{}-csr.n1e5", std::process::id())))
        );
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// The layout probes return the naive counts: the CSR probe the sum over the generator's out-edges and their
    /// reverse; AND/popcount and the status scan the rows of #N mod 4 = s; the due scan every fourth row; the overlay
    /// probe membership in the tail's owners.
    #[test]
    fn layout_arms_equal_naive_counts() {
        let (t, b) = tail_and_base(1_000, 200, 3);
        let c = Columns::parse(&b).unwrap();
        let s = base::Spec {
            n: 1_000,
            upto_lsn: t.start,
            base_seq: BASE_SEQ,
        };
        let mut naive_in: BTreeMap<u32, u64> = BTreeMap::new();
        for src in 1..=s.n {
            for (k, d) in s.out_edges(src) {
                *naive_in.entry(d).or_default() += u64::from(src) + u64::from(k);
            }
        }
        for id in [1, 2, 500, 999, 1_000] {
            let out: u64 = s
                .out_edges(id)
                .iter()
                .map(|&(k, d)| u64::from(d) + u64::from(k))
                .sum();
            assert_eq!(
                expand(&c, id),
                out + naive_in.get(&id).copied().unwrap_or(0),
                "#{id}"
            );
        }
        let kind = c.frozen(kind_key(BASE_KIND)).unwrap();
        for st in 0..STATUSES {
            let naive = (1..=s.n).filter(|id| id % 4 == u32::from(st)).count() as u64;
            let set = c.frozen(kind_status_key(BASE_KIND, st)).unwrap();
            assert_eq!(and_popcount(&kind, &set), naive, "status {st}");
            assert_eq!(scan_status(&c, st), naive, "status {st}");
        }
        assert_eq!(scan_due(&c), 250);
        let o = t.overlay().unwrap();
        let owners: BTreeSet<u32> = t
            .records
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(cm) => Some(cm.ops.iter().map(Op::owner).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect();
        for id in draw_ids(1_100, 300, 9) {
            assert_eq!(probe(&o, id), owners.contains(&id), "#{id}");
        }
        assert!(
            draw_ids(1_000, 4_096, 5)
                .iter()
                .all(|&id| (1..=1_000).contains(&id))
        );
    }

    /// T1's point read returns the tail's value (spec gate 1): for every node the tail touched, the status its last
    /// `Create` or `SetStatus` set, read through the records, and the base row's #N mod 4 for a node whose status the
    /// tail never set.
    #[test]
    fn t1_read_returns_the_tails_value() {
        for seed in 1..=3 {
            let (t, b) = tail_and_base(300, 400, seed);
            let c = Columns::parse(&b).unwrap();
            let o = t.overlay().unwrap();
            let mut naive: BTreeMap<u32, u8> = BTreeMap::new();
            let mut touched = BTreeSet::new();
            for r in &t.records {
                let Payload::Commit(cm) = &r.payload else {
                    continue;
                };
                for op in &cm.ops {
                    touched.insert(op.owner());
                    match op {
                        Op::Create { id, image, .. } => {
                            naive.insert(*id, image.status.unwrap().0);
                        }
                        Op::SetStatus { id, new, .. } => {
                            naive.insert(*id, new.0);
                        }
                        _ => {}
                    }
                }
            }
            let mut from_base = 0;
            let mut log = MemLog::new(t.start, &t.bytes);
            for &id in &touched {
                let want = naive.get(&id).copied().unwrap_or_else(|| {
                    from_base += 1;
                    (id % 4) as u8
                });
                assert_eq!(
                    point_read(&mut log, &o, &c, id),
                    Ok(want),
                    "seed {seed} #{id}"
                );
            }
            assert!(
                naive.len() > 50 && from_base > 20,
                "both paths: {} {from_base}",
                naive.len()
            );
            assert_eq!(
                point_read(&mut log, &o, &c, 7).unwrap(),
                naive.get(&7).copied().unwrap_or(3)
            );
        }
    }

    /// The MCP model: the warm builder of all but the last 16 commits, caught up over the rest from the log, finishes
    /// to the overlay of the whole tail; the arm reads `HEAD` once (8,192 B) and the log only from the boundary.
    #[test]
    fn catchup_equals_a_full_replay() {
        let (t, _) = tail_and_base(400, 600, 4);
        let commits = t
            .records
            .iter()
            .filter(|r| matches!(r.payload, Payload::Commit(_)))
            .count();
        assert!(commits > 2 * BURST);
        let (x, chain, b) = warm(&t, BURST).unwrap();
        assert!(x > t.start && x < t.end);
        let o = catch_up(&mut MemLog::new(t.start, &t.bytes), &t, x, chain, b.clone()).unwrap();
        assert_eq!(o, t.overlay().unwrap());
        assert_ne!(b.finish(), o, "the warm state lacks the burst");
        // Through the arm, on a fixture of the test profile.
        let fs = MemFs::with_dir(&scratch());
        let q = grid()
            .into_iter()
            .find(|q| q.family == Family::Catchup)
            .unwrap();
        let p = prepare(&fs, &scratch(), &q, Profile::Test).unwrap();
        let c = Columns::parse(p.map.bytes()).unwrap();
        let mut arms = arms(&p, &c, &q, &MonoTicker::new()).unwrap();
        let (x, _, _) = warm(p.tail.as_ref().unwrap(), BURST).unwrap();
        fs.clear_log();
        arms[0].sample().unwrap();
        let log = fs.log();
        let head: Vec<_> = log.reads.iter().filter(|r| r.0 == "HEAD").collect();
        assert_eq!(head, [&("HEAD".to_string(), 0, HEAD_LEN)]);
        let geo = p.tail.as_ref().unwrap().expect.geo;
        let xe = geo.extent(x);
        for r in log.reads.iter().filter(|r| r.0 != "HEAD") {
            let k: u64 = r.0.trim_start_matches("log.").parse().unwrap();
            assert!(
                k > xe || (k == xe && r.1 >= geo.offset(x)),
                "{r:?} reads before the boundary"
            );
        }
    }

    /// The checkpoint arm folds the tail from its extents into `seg.d1`, sealed, whose bytes read back to the fold
    /// over the tail in memory; its floor writes the same bytes to `seg.d2`; both arms replace their file each sample.
    #[test]
    fn ckpt_arm_writes_a_sealed_delta_outside_setup() {
        let fs = MemFs::with_dir(&scratch());
        let q = grid()
            .into_iter()
            .find(|q| q.family == Family::Ckpt)
            .unwrap();
        let p = prepare(&fs, &scratch(), &q, Profile::Test).unwrap();
        let c = Columns::parse(p.map.bytes()).unwrap();
        let t = p.tail.as_ref().unwrap();
        let want = fold_tail(&mut MemLog::new(t.start, &t.bytes), t, &c).unwrap();
        assert!(!want.rows.is_empty());
        let mut arms = arms(&p, &c, &q, &MonoTicker::new()).unwrap();
        let names: Vec<&str> = arms.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["ckpt", "ckpt.write"]);
        for _ in 0..2 {
            for a in arms.iter_mut() {
                a.sample().unwrap();
            }
            let d1 = p.dir().join(DELTA_NAME);
            let d2 = p.dir().join(FLOOR_NAME);
            assert!(fs.is_sealed(&d1) && fs.is_sealed(&d2));
            let bytes = fs.bytes(&d1).unwrap();
            assert_eq!(decode(&bytes).unwrap(), want);
            assert_eq!(
                fs.bytes(&d2).unwrap(),
                bytes,
                "the floor writes the same bytes"
            );
        }
        assert!(
            fs.log().opens.iter().any(|o| o.starts_with("log.")),
            "the fold reads the extents"
        );
    }

    /// A staged arm times only its operation: the setup's 5 ms stay outside, the operation's 2 ms are the sample; a
    /// failing setup or operation fails the sample; it never batches.
    #[test]
    fn staged_arm_times_only_the_operation() {
        let clock = FakeTicker::manual();
        let (c1, c2) = (clock.clone(), clock.clone());
        let mut fail = 0;
        let mut a = Staged::new(
            "x",
            clock.clone(),
            move || {
                c1.advance(5 * MS);
                Ok(7u32)
            },
            move |v: u32| {
                c2.advance(2 * MS);
                fail += 1;
                if fail == 3 {
                    Err(ArmError("op".into()))
                } else {
                    Ok(v)
                }
            },
        );
        assert_eq!(a.sample(), Ok(2 * MS));
        a.set_batch(20);
        assert_eq!(a.batch(), 1);
        assert_eq!(a.sample(), Ok(2 * MS));
        assert_eq!(a.sample(), Err(ArmError("op".into())));
        let mut b = Staged::new(
            "y",
            clock,
            || Err::<(), _>(ArmError("setup".into())),
            |()| Ok(0),
        );
        assert_eq!(b.sample(), Err(ArmError("setup".into())));
    }

    /// A `--quantity` run writes exactly one record under `--out` that reads back: measurement 14, its arm, t1's
    /// 10,000 × 5 plan (the probe is fast), the given commit and toolchain; its fixture is removed.
    #[test]
    fn run_writes_one_valid_record() {
        let out_dir = scratch_dir("m14-run");
        let o = out_dir.to_str().unwrap().to_string();
        let fs = MemFs::with_dir(&scratch());
        let (code, out, err) = run(
            &fs,
            &[
                "--quantity",
                "overlay.n1e5",
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--out",
                &o,
                "--scratch",
                "/scratch",
                "--commit",
                "0123abcd",
                "--toolchain",
                "1.98.1",
            ],
        );
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
            (14, "overlay.n1e5")
        );
        let arms: Vec<&str> = rec.arms.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(arms, ["probe"]);
        assert_eq!(rec.plan, Plan::new(10_000, 5));
        assert_eq!(
            (rec.commit.as_str(), rec.toolchain.as_str()),
            ("0123abcd", "1.98.1")
        );
        assert_eq!(rec.condition, Condition::Idle);
        assert!(rec.valid(), "{:?}", rec.disqualifications());
        assert!(fs.files().is_empty(), "the fixture is removed");
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// Usage errors exit 2 with the reason: an unknown flag or quantity, a bad host or condition, a run without
    /// `--host`, `--condition` or `--out`, a loaded or synthetic run without `--load-log`, a flag without its value.
    /// `--help` exits 0, and the §9.7 contract alone is a valid call.
    #[test]
    fn cli_refuses_usage_errors() {
        let fs = MemFs::with_dir(&scratch());
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (vec!["--bogus"], "unknown argument"),
            (
                vec!["--quantity", "csr.n1e7", "--smoke"],
                "unknown quantity",
            ),
            (
                vec!["--host", "desk", "--condition", "idle", "--out", "/o"],
                "unknown host",
            ),
            (
                vec!["--host", "laptop", "--condition", "busy", "--out", "/o"],
                "unknown condition",
            ),
            (vec!["--condition", "idle", "--out", "/o"], "needs --host"),
            (vec!["--host", "laptop", "--out", "/o"], "needs --host"),
            (
                vec!["--host", "laptop", "--condition", "idle"],
                "needs --host",
            ),
            (
                vec!["--host", "laptop", "--condition", "loaded", "--out", "/o"],
                "needs --load-log",
            ),
            (
                vec![
                    "--host",
                    "laptop",
                    "--condition",
                    "synthetic",
                    "--out",
                    "/o",
                ],
                "needs --load-log",
            ),
            (vec!["--out"], "needs a value"),
        ];
        for (args, why) in cases {
            let (code, _, err) = run(&fs, &args);
            assert_eq!(code, EXIT_USAGE, "{args:?}");
            assert!(err.contains(why), "{args:?}: {err}");
            assert!(err.contains("usage: m14"));
        }
        let (code, out, _) = run(&fs, &["--help"]);
        assert_eq!((code, out.as_str()), (EXIT_DONE, USAGE));
        assert!(
            parse(&os(&[
                "--host",
                "laptop",
                "--condition",
                "idle",
                "--out",
                "/o"
            ]))
            .is_ok()
        );
    }

    /// Without `--quantity` each quantity runs in a child of its own, in grid order, with the same flags plus
    /// `--quantity <q>`; the exit is the worst child's, and a child that cannot start counts as failed.
    #[test]
    fn cli_without_quantity_spawns_one_process_per_run() {
        let fs = MemFs::with_dir(&scratch());
        let meter = FakeMeter::new(8_000_000_000);
        let base = os(&["--host", "laptop", "--condition", "idle", "--out", "/o"]);
        for (bad, expect) in [
            (None, EXIT_DONE),
            (Some(Ok(1)), EXIT_FAILED),
            (Some(Err(())), EXIT_FAILED),
        ] {
            let mut calls: Vec<Vec<OsString>> = Vec::new();
            let mut spawn = |a: &[OsString]| -> Result<u8, String> {
                calls.push(a.to_vec());
                match (calls.len(), bad) {
                    (4, Some(Ok(c))) => Ok(c),
                    (4, Some(Err(()))) => Err("cannot start".into()),
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
            assert_eq!(calls.len(), 11);
            for (c, q) in calls.iter().zip(grid()) {
                let mut want = base.clone();
                want.push("--quantity".into());
                want.push(q.name.into());
                assert_eq!(c, &want);
            }
            assert_eq!(String::from_utf8(out).unwrap().lines().count(), 11);
        }
        assert!(fs.files().is_empty(), "the parent writes no fixture");
    }
}
