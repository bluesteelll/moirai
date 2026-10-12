//! Measurement 14 (WP-53e, [60 §5.2] item 14, `60-roadmap.md:770`; `docs/m0/PLAN.md:317`): the layout probes over
//! the F09 segment of [`crate::seg`], at 1e5 and 1e6 nodes, and the T1 and T2 trigger quantities: a point read after
//! a full 4,096-op tail and a delta checkpoint at 0.5 M nodes (T1, `ARCHITECTURE-RESEARCH.md:130`), and the MCP
//! overlay catch-up (T2, `:140`). T2's 16-writer wait is taken from measurement 2's run (R-24 Q7) and is not built
//! here. The tails are u10's ([`crate::tail::synth`]), from [70 S8]'s spec-derived mix (R-24 Q10).
//!
//! **The grid** (15 quantities, names by [MP §4.1]):
//!
//! | Quantity | Arms (arm 0 first) | What one sample does | Minimum tier |
//! |---|---|---|---|
//! | `csr.n1e5`, `csr.n1e6` | `expand` | reads one node's out-list and in-list from the six CSR columns (`09-segments.md:479-505`) | — |
//! | `bitset.n1e5`, `bitset.n1e6` | `and.popcount` | ANDs the kind bitset with one kind-status bitset and counts ([50] F6) | — |
//! | `bitset-ab.n1e5`, `bitset-ab.n1e6` | `and.popcount` | ANDs one kind-status bitset (bitmap chunks at these N) with the sparse set A_s (array chunks) and counts: the array × bitmap path of the frozen AND ([F09 §4.5], `09-segments.md:288-317`; R-30) | — |
//! | `bitset-aa.n1e5`, `bitset-aa.n1e6` | `and.popcount` | ANDs A_s with the second sparse set B_s and counts: the array × array path | — |
//! | `scan.n1e5`, `scan.n1e6` | `scan.status`, `scan.split` | counts the rows of one status in `NODE`'s 60-byte rows; the floor counts the same status in a dense 1-byte column built from `NODE` in the run's setup, the layout a per-field split of `NODE` would give (OP-09-21, `09-segments.md:1369-1372`; `status`, `08-data-model.md:80`). Both cycle the status through 0–3 with their own counters, so they time the same counts | — |
//! | `overlay.n1e5`, `overlay.n1e6` | `probe` | looks one `#N` up in a full tail's node index ("overlay probe ~0.2 µs", `ARCHITECTURE-RESEARCH.md:968`) | — |
//! | `read-4096.n1e6` | `read` | T1's point read: the status of a node the tail touched, through the overlay and the tail's records, else the base row | t1 (read ≤ 50 µs) |
//! | `ckpt-4096.n5e5` | `ckpt`, `ckpt.write` | T1's delta checkpoint over a fresh tail per sample (scoping §4.2): the setup writes sample j's tail ([`ckpt_tail`]) in place over the fixture's extent through a read-write handle kept for the run, zeroes the rest of the previous tail and flushes it (`sync(Data)`), all untimed; the timed op replays tail j from the extent, folds it over the base into a delta of the probe's subset (below), encodes `seg.d1`, then `create_new`, `write_at`, `sync(DataAndMeta)`, `seal`, `sync_dir` (`docs/spec/os/fs.md:411`, `:456`). The floor's sample j folds tail j from memory in its setup and writes and flushes the same bytes, so the pair is matched. The products (the tail, the delta and its bytes) are dropped and the sample's file is unlinked in the teardown, outside the timed region ([MP §4.1]). The setup's generation and rewrite lengthen a sample's untimed part, so the pilot's 1 s ([MP §4.2]) may take fewer than 5 samples; the declared minimum tier t3 bounds that | t3 (≤ 100 ms) |
//! | `catchup-16.n1e6` | `catchup` | T2's MCP model: a long-lived process holds the overlay of a full tail but its last 16 commits (one per writer of T2's burst) and catches up: `HEAD` pread (8,192 B), the new records scanned from the extents, applied to its warm builder, the overlay finished again (`ARCHITECTURE-RESEARCH.md:783`). The warm builder is cloned in the setup and the finished overlay dropped in the teardown, both outside the timed region | t2 (catch-up p50 ≤ 5 ms) |
//!
//! **What the checkpoint omits** (review m-4). `ckpt-4096.n5e5` times a delta of the probe's subset of sections
//! ([`crate::seg`]), not the whole delta of [F09 §16.2] (`09-segments.md:1085-1089`; the sections `:156-206`). Its
//! delta has no `TITLE_BLOB` or `FIELDS_BLOB` for the touched rows, though `SetField` and `SetBody` are 35 % and 5 % of
//! the mix ([`crate::tail::synth`]); no `BLOBTAB`; no `UID` rows for the creates; no `TOMB` rows for the deletes; no
//! new symbols; none of the store-level tables; and it folds none of the tail's settles into the runtime-window
//! sections. It still writes `rt_upto_lsn` = `upto_lsn`, because F09 requires `rt_upto_lsn ≥ upto_lsn` in every
//! main-set layer (`:128`, `:136`), although no runtime section is folded. The timed region ends at `seg.d1`'s
//! `sync_dir`: the `Checkpoint` record's append under the writer byte, its group commit and the publish that makes the
//! new set visible (`ARCHITECTURE-RESEARCH.md:736`, [AR §4.5] step 12; [F16 P-80], `16-protocol.md:721-723`) are not
//! timed. The results analyst reads the quantity as a lower bound of the delta checkpoint of
//! `ARCHITECTURE-RESEARCH.md:130`.
//!
//! **What no test sees** (u12 review r1 m-1, test r1 m-1). The tests watch the fixture's store at each sample's two
//! ticks: its reads, and for the checkpoint also its opens, writes and flushes, and the files' bytes and seals.
//! In-memory work leaves no trace there, so these placements rest on the code alone: the catch-up's clone of its warm
//! builder (setup); the floor `ckpt.write`'s generation, fold and encode of tail j (setup; in its op they would
//! inflate the floor); and the drop of the products (teardown). `staged_arm_times_only_the_operation` pins the
//! mechanism, and each teardown takes its arm's products by type, but an op that dropped the real tail, delta and
//! bytes inside the timed region and returned a placeholder of the same type would still pass every test.
//!
//! Every run's fixture is the base `seg.base.1` at the quantity's N ([`crate::seg::base`]), written, sealed and
//! mapped, and, for the last four families, a full `ops` tail at P06 (4,096 at production) written as `log.<n>`
//! extents with `HEAD`; it is removed on every path out of the run. The probe's choices: the ids a probe cycles
//! through ([`PROBE_IDS`] drawn from the quantity's seed; the read's are the tail's touched nodes), the families'
//! single arms, and the catch-up's burst. The sparse sets are the probe's too: A_s = {#N : mix(`SALT_A` ^ #N) mod 32
//! = s} and B_s likewise with `SALT_B`, for s = 0–3, about 2,048 ids per 65,536-id chunk, under the array limit of
//! 4,096 ([`sparse_ids`]); they live in the fixture's memory, not in the base, so the base's sections are unchanged. At
//! the test profile (N / 100) the kind-status sets are arrays too, so the smoke's `bitset-ab` takes the array × array
//! path; the array × bitmap path at production N is pinned by `sparse_bitset_arms_take_the_array_paths`.
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
use crate::seg::bitset::{Frozen, and_popcount, encode_frozen};
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
    /// AND/popcount of a kind-status bitset with a sparse set: array × bitmap.
    BitsetAb,
    /// AND/popcount of two sparse sets: array × array.
    BitsetAa,
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
            Family::BitsetAb => "bitset-ab",
            Family::BitsetAa => "bitset-aa",
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
            Family::Bitset | Family::BitsetAb | Family::BitsetAa => &["and.popcount"],
            Family::Scan => &["scan.status", "scan.split"],
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

/// The 15 quantities in run order.
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
        for f in [
            Family::Csr,
            Family::Bitset,
            Family::BitsetAb,
            Family::BitsetAa,
            Family::Scan,
            Family::Overlay,
        ] {
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

/// The split status column: byte [`STATUS_AT`] of every 60-byte `NODE` row, in row order, the dense 1-byte column a
/// per-field split of `NODE` would give (OP-09-21, `09-segments.md:1369-1372`; `08-data-model.md:80`).
pub fn split_status(c: &Columns<'_>) -> Vec<u8> {
    let (rows, _) = c.node.as_chunks::<NODE_HDR_LEN>();
    rows.iter().map(|row| row[STATUS_AT]).collect()
}

/// The floor scan over the split status column: the rows whose status is `s`.
pub fn scan_split(col: &[u8], s: u8) -> u64 {
    col.iter().filter(|&&b| b == s).count() as u64
}

/// The sparse sets' modulus: about 65,536 / 32 = 2,048 ids per chunk, so every chunk is an array.
pub const SPARSE_MOD: u64 = 32;
/// The salt of the sparse sets A_s.
pub const SALT_A: u64 = 0x6D31_345F_7370_4131;
/// The salt of the sparse sets B_s.
pub const SALT_B: u64 = 0x6D31_345F_7370_4232;

/// The sparse set of status slot `s` under `salt` in `1..=n`: the ids whose `mix(salt ^ #N)` is `s` modulo
/// [`SPARSE_MOD`], strictly ascending.
pub fn sparse_ids(n: u32, salt: u64, s: u8) -> Vec<u32> {
    (1..=n)
        .filter(|&id| mix(salt ^ u64::from(id)) % SPARSE_MOD == u64::from(s))
        .collect()
}

/// What the counting arms read besides the base's sections, built by [`prepare`] outside every timed region.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Operands {
    /// The split status column of a `scan` fixture ([`split_status`]), else empty.
    pub split: Vec<u8>,
    /// The frozen bitsets of A_0 … A_3 of a `bitset-ab` or `bitset-aa` fixture, else empty.
    pub sparse_a: Vec<Vec<u8>>,
    /// The frozen bitsets of B_0 … B_3 of a `bitset-ab` or `bitset-aa` fixture, else empty.
    pub sparse_b: Vec<Vec<u8>>,
}

/// The operands of `family`'s counting arms over the base `c`.
pub fn operands(family: Family, c: &Columns<'_>) -> Result<Operands, String> {
    let n = u32::try_from(c.rows).map_err(|_| "more than 2^32 rows")?;
    let sets = |salt: u64| -> Result<Vec<Vec<u8>>, String> {
        (0..STATUSES)
            .map(|s| {
                let mut b = Vec::new();
                encode_frozen(&sparse_ids(n, salt, s), &mut b)?;
                Ok(b)
            })
            .collect()
    };
    Ok(match family {
        Family::Scan => Operands {
            split: split_status(c),
            ..Operands::default()
        },
        Family::BitsetAb | Family::BitsetAa => Operands {
            sparse_a: sets(SALT_A)?,
            sparse_b: sets(SALT_B)?,
            ..Operands::default()
        },
        _ => Operands::default(),
    })
}

/// One operation of a counting arm: each call returns the count its timed call computes.
pub type CountOp<'p> = Box<dyn FnMut() -> u64 + 'p>;

/// The operations of the AND/popcount and scan arms of `family` over the base `c` and the operands `ops`, named as
/// [`Family::arms`], arm 0 first. Each cycles the status s through 0–3 with its own counter, so arm 0 and its floor
/// time the same counts. [`arms`] times exactly these calls, so a test that calls them checks the operands each arm
/// passes to its probe: `bitset` ANDs the kind set with kind-status s, `bitset-ab` kind-status s with A_s, `bitset-aa`
/// A_s with B_s.
pub fn count_ops<'p>(
    family: Family,
    c: &'p Columns<'p>,
    ops: &'p Operands,
) -> Result<Vec<(&'static str, CountOp<'p>)>, String> {
    let s_of = |i: &mut usize| {
        *i += 1;
        (*i % usize::from(STATUSES)) as u8
    };
    let (mut i, mut j) = (0usize, 0usize);
    let status_sets = || -> Result<Vec<Frozen<'p>>, String> {
        (0..STATUSES)
            .map(|s| c.frozen(kind_status_key(BASE_KIND, s)))
            .collect()
    };
    let parsed = |sets: &'p [Vec<u8>]| -> Result<Vec<Frozen<'p>>, String> {
        if sets.len() != usize::from(STATUSES) {
            return Err(format!("{} sparse sets, not {STATUSES}", sets.len()));
        }
        sets.iter().map(|b| Frozen::parse(b)).collect()
    };
    let v: Vec<(&'static str, CountOp<'p>)> = match family {
        Family::Bitset => {
            let kind = c.frozen(kind_key(BASE_KIND))?;
            let sets = status_sets()?;
            vec![(
                "and.popcount",
                Box::new(move || and_popcount(&kind, &sets[usize::from(s_of(&mut i))])),
            )]
        }
        Family::BitsetAb => {
            let (x, y) = (status_sets()?, parsed(&ops.sparse_a)?);
            vec![(
                "and.popcount",
                Box::new(move || {
                    let s = usize::from(s_of(&mut i));
                    and_popcount(&x[s], &y[s])
                }),
            )]
        }
        Family::BitsetAa => {
            let (x, y) = (parsed(&ops.sparse_a)?, parsed(&ops.sparse_b)?);
            vec![(
                "and.popcount",
                Box::new(move || {
                    let s = usize::from(s_of(&mut i));
                    and_popcount(&x[s], &y[s])
                }),
            )]
        }
        Family::Scan => vec![
            (
                "scan.status",
                Box::new(move || scan_status(c, s_of(&mut i))),
            ),
            (
                "scan.split",
                Box::new(move || scan_split(&ops.split, s_of(&mut j))),
            ),
        ],
        f => return Err(format!("{} has no counting arm", f.stem())),
    };
    Ok(v)
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

/// Checkpoint sample `j`'s tail (scoping §4.2: "a fresh tail per sample outside the timed region"): a full `ops` tail
/// at P06 over the base like the fixture's, from a seed of its own, so it is deterministic and distinct per `j`.
pub fn ckpt_tail(q: &Quantity, profile: Profile, j: u64) -> Result<Tail, String> {
    generate(
        &Target {
            seed: mix(seed(&q.name) ^ (j + 1)),
            ..target(q, profile)
        },
        false,
    )
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

/// A time arm whose sample runs `setup` untimed, times `op` on its result, then hands the op's product to `teardown`
/// untimed ([MP §4.1]: setup and teardown stay outside the timed region). The teardown takes the product by value, so
/// the product is dropped there, outside the timed region too; a failing setup, op or teardown fails the sample. It
/// cannot batch, so its operation must take 20 × the timer's resolution ([MP §4.5]).
pub struct Staged<S, P, D, T> {
    name: String,
    setup: S,
    op: P,
    teardown: D,
    ticker: T,
}

impl<S, P, D, T> Staged<S, P, D, T> {
    /// A staged arm named `name`.
    pub fn new(
        name: impl Into<String>,
        ticker: T,
        setup: S,
        op: P,
        teardown: D,
    ) -> Staged<S, P, D, T> {
        Staged {
            name: name.into(),
            setup,
            op,
            teardown,
            ticker,
        }
    }
}

impl<S, P, D, T, X, O> Arm for Staged<S, P, D, T>
where
    S: FnMut() -> Result<X, ArmError>,
    P: FnMut(X) -> Result<O, ArmError>,
    D: FnMut(O) -> Result<(), ArmError>,
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
        (self.teardown)(std::hint::black_box(o))?;
        Ok(t1.saturating_sub(t0))
    }
}

fn rel(s: &str) -> Result<RelPathBuf, String> {
    RelPathBuf::from_string(s.to_string()).map_err(|e| e.to_string())
}

fn arm_err(e: impl std::fmt::Display) -> ArmError {
    ArmError(e.to_string())
}

/// Removes `name` from `root` if it is there (the teardown of the checkpoint arms, and the fixture's removal).
fn unlink_if<V: StoreFs>(vfs: &V, root: &V::Root, name: &str) -> Result<(), ArmError> {
    let retry = ShareRetry::Bounded {
        total_ms: OS_SHARE_RETRY_MS,
    };
    match vfs.unlink(root, rel(name).map_err(ArmError)?.as_rel_path(), retry) {
        Err(e) if e.kind != VfsErrorKind::NotFound => Err(arm_err(e)),
        _ => Ok(()),
    }
}

/// What one sample of the checkpoint arm made: the tail, the delta and its bytes. The op returns it and the teardown
/// drops it, outside the timed region (review m-5).
#[derive(Debug)]
pub struct CkptOut {
    /// The sample's tail.
    pub tail: Tail,
    /// The folded delta.
    pub delta: delta::Delta,
    /// Its bytes, written as `seg.d1`.
    pub bytes: Vec<u8>,
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
    /// The run's profile.
    pub profile: Profile,
    /// The quantity's seed.
    pub seed: u64,
    /// What the counting arms read besides the base ([`operands`]).
    pub operands: Operands,
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

/// Writes `q`'s fixture under `scratch` at `profile`: the base sealed, the tail's extents and `HEAD`; maps the base
/// and builds the counting arms' operands from it.
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
    let operands = operands(q.family, &Columns::parse(map.bytes())?)?;
    Ok(Prepared {
        map,
        root,
        tail,
        n,
        profile,
        seed: seed(&q.name),
        operands,
        vfs,
        guard,
    })
}

/// The arms of `q` over a prepared fixture, arm 0 first; their setup (the overlay, the warm builder, the floor's
/// bytes) runs here, outside every timed region. The AND/popcount and scan arms time the calls of [`count_ops`].
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
        Family::Bitset | Family::BitsetAb | Family::BitsetAa | Family::Scan => {
            for (name, mut op) in count_ops(q.family, c, &p.operands)? {
                arms.push(Box::new(TimedFn::new(name, ticker.clone(), move || {
                    Ok(op())
                })));
            }
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
            let geo = t.expect.geo;
            let k = geo.extent(t.start);
            if geo.extent(t.end - 1) != k {
                return Err("the checkpoint's tail spans more than one extent".into());
            }
            // Held for the run, so no written file is closed between samples.
            let ext = p
                .vfs
                .open(
                    &p.root,
                    rel(&extent_name(k))?.as_rel_path(),
                    Access::ReadWrite,
                    OpenHint::Normal,
                )
                .map_err(|e| e.to_string())?;
            let (profile, q0, q1) = (p.profile, q.clone(), q.clone());
            let (mut j0, mut j1, mut prev_end) = (0u64, 0u64, t.end);
            let mut log = VfsLog::new(p.vfs, &p.root, geo);
            arms.push(Box::new(Staged::new(
                "ckpt",
                ticker.clone(),
                move || -> Result<Tail, ArmError> {
                    let tj = ckpt_tail(&q0, profile, j0).map_err(ArmError)?;
                    if tj.start != t.start || geo.extent(tj.end - 1) != k {
                        return Err(ArmError(format!(
                            "checkpoint tail {j0} leaves the fixture's extent {k}"
                        )));
                    }
                    j0 += 1;
                    p.vfs
                        .write_at(&ext, geo.offset(tj.start), &tj.bytes)
                        .map_err(arm_err)?;
                    if prev_end > tj.end {
                        let zeros = vec![0u8; (prev_end - tj.end) as usize];
                        p.vfs
                            .write_at(&ext, geo.offset(tj.end), &zeros)
                            .map_err(arm_err)?;
                    }
                    prev_end = tj.end;
                    p.vfs.sync(&ext, SyncKind::Data).map_err(arm_err)?;
                    Ok(tj)
                },
                move |tj: Tail| -> Result<CkptOut, ArmError> {
                    let delta = fold_tail(&mut log, &tj, c).map_err(ArmError)?;
                    let bytes = delta::encode(&delta).map_err(ArmError)?;
                    seal_new(p.vfs, &p.root, DELTA_NAME, &bytes)?;
                    Ok(CkptOut {
                        tail: tj,
                        delta,
                        bytes,
                    })
                },
                move |out: CkptOut| {
                    drop(out);
                    unlink_if(p.vfs, &p.root, DELTA_NAME)
                },
            )));
            arms.push(Box::new(Staged::new(
                "ckpt.write",
                ticker.clone(),
                move || -> Result<Vec<u8>, ArmError> {
                    let tj = ckpt_tail(&q1, profile, j1).map_err(ArmError)?;
                    j1 += 1;
                    let d = fold_tail(&mut MemLog::new(tj.start, &tj.bytes), &tj, c)
                        .map_err(ArmError)?;
                    delta::encode(&d).map_err(ArmError)
                },
                move |bytes: Vec<u8>| -> Result<Vec<u8>, ArmError> {
                    seal_new(p.vfs, &p.root, FLOOR_NAME, &bytes)?;
                    Ok(bytes)
                },
                move |bytes: Vec<u8>| {
                    drop(bytes);
                    unlink_if(p.vfs, &p.root, FLOOR_NAME)
                },
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
                move |b: Builder| -> Result<Overlay, ArmError> {
                    let got = p.vfs.read_at(&head, 0, &mut buf).map_err(arm_err)?;
                    if got != HEAD_LEN {
                        return Err(ArmError(format!("HEAD read {got} bytes, not 8,192")));
                    }
                    catch_up(&mut log, t, x, chain, b).map_err(ArmError)
                },
                |o: Overlay| {
                    drop(o);
                    Ok(())
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
    use crate::seg::bitset::Container;
    use crate::seg::delta::decode;
    use crate::tail::commit::Op;
    use crate::tail::memfs::{MemFile, MemFs, MemMap};
    use crate::tail::store::tests::scratch;
    use crate::tail::synth::tests::test_target;
    use crate::tail::{Geometry, REC_HDR_LEN};
    use crate::testkit::{FakeMeter, FakeTicker, scratch_dir};
    use crate::tier::Plan;
    use crate::units::MS;
    use moirai_vfs::{
        Advice, DirEntry, DurabilityFailure, FileIdentity, FreeSpace, GroupMember, MapError,
        StoreVolume, SwapOutcome, SwapRecovery, VfsCounters, VfsError, VfsTypes,
    };
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::rc::Rc;

    /// A read the store logged: (file name, offset, bytes asked).
    type Read = (String, u64, usize);

    /// One reading of [`FsTicker`]: how many reads the store had logged, how many calls a [`LoggedFs`] over it had
    /// logged, and each watched file's bytes and seal.
    #[derive(Clone, Debug)]
    struct Tick {
        reads: usize,
        calls: usize,
        files: BTreeMap<String, (Vec<u8>, bool)>,
    }

    /// The probe counter (spec Goal 2): a ticker over a [`MemFs`] that records a [`Tick`] at every reading and
    /// returns 1, 2, 3, … `Staged::sample` and `TimedFn::sample` read their ticker exactly at t0 and t1, so the reads
    /// (and a [`LoggedFs`]'s calls) logged between a sample's two ticks are exactly its timed region's I/O, and the two
    /// ticks show the watched files as the timed region found and left them. The product code carries no
    /// instrumentation.
    #[derive(Clone)]
    struct FsTicker<'f> {
        fs: &'f MemFs,
        dir: PathBuf,
        watch: Vec<String>,
        calls: Rc<RefCell<Vec<Call>>>,
        ticks: Rc<RefCell<Vec<Tick>>>,
    }

    impl<'f> FsTicker<'f> {
        /// A ticker over `fs` that watches the files `watch` of the fixture directory `dir`.
        fn new(fs: &'f MemFs, dir: &Path, watch: &[&str]) -> FsTicker<'f> {
            FsTicker {
                fs,
                dir: dir.to_path_buf(),
                watch: watch.iter().map(|w| w.to_string()).collect(),
                calls: Rc::default(),
                ticks: Rc::default(),
            }
        }

        /// The same ticker, counting the calls that `lfs`, a [`LoggedFs`] over its `fs`, logs.
        fn logging(mut self, lfs: &LoggedFs<'_>) -> FsTicker<'f> {
            self.calls = lfs.calls.clone();
            self
        }

        /// The readings so far.
        fn count(&self) -> usize {
            self.ticks.borrow().len()
        }

        /// The last sample's two ticks, (t0, t1).
        fn last(&self) -> (Tick, Tick) {
            let t = self.ticks.borrow();
            assert!(
                t.len() >= 2 && t.len().is_multiple_of(2),
                "{} ticks",
                t.len()
            );
            (t[t.len() - 2].clone(), t[t.len() - 1].clone())
        }

        /// The reads logged between the last sample's ticks.
        fn timed_reads(&self) -> Vec<Read> {
            let (t0, t1) = self.last();
            self.fs.log().reads[t0.reads..t1.reads].to_vec()
        }
    }

    impl Ticker for FsTicker<'_> {
        fn now_ns(&mut self) -> u64 {
            let files = self
                .watch
                .iter()
                .filter_map(|w| {
                    let p = self.dir.join(w);
                    let b = self.fs.bytes(&p)?;
                    Some((w.clone(), (b, self.fs.is_sealed(&p))))
                })
                .collect();
            let mut t = self.ticks.borrow_mut();
            t.push(Tick {
                reads: self.fs.log().reads.len(),
                calls: self.calls.borrow().len(),
                files,
            });
            t.len() as u64
        }
    }

    /// A call that [`LoggedFs`] logs on a file: an open (whether its handle may write), a write or a flush.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Io {
        Open(bool),
        Write,
        Sync(SyncKind),
    }

    /// A logged call: the file's name, the number of the handle it went through, and the call.
    type Call = (String, usize, Io);

    /// An open file of a [`LoggedFs`]: [`MemFs`]'s handle, the file's name, and the handle's number (the index of its
    /// open in the log).
    struct LoggedFile {
        file: MemFile,
        name: String,
        n: usize,
    }

    /// [`MemFs`] with every open, write and flush logged by file name and handle (u12 test r1 I-1, I-2): `MemFs`
    /// logs no write or flush, and not which handle a call goes through. Every call goes on to `fs` unchanged, so
    /// `fs`'s own log, files and seals are as without the wrapper.
    struct LoggedFs<'f> {
        fs: &'f MemFs,
        calls: Rc<RefCell<Vec<Call>>>,
    }

    impl<'f> LoggedFs<'f> {
        fn new(fs: &'f MemFs) -> LoggedFs<'f> {
            LoggedFs {
                fs,
                calls: Rc::default(),
            }
        }

        /// The calls so far.
        fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }

        fn log(&self, f: &LoggedFile, io: Io) {
            self.calls.borrow_mut().push((f.name.clone(), f.n, io));
        }

        /// `file`, just opened as `rel`, numbered and logged.
        fn opened(&self, file: MemFile, rel: RelPath<'_>, write: bool) -> LoggedFile {
            let f = LoggedFile {
                file,
                name: rel.as_str().to_string(),
                n: self.calls.borrow().len(),
            };
            self.log(&f, Io::Open(write));
            f
        }
    }

    impl VfsTypes for LoggedFs<'_> {
        type Root = PathBuf;
        type File = LoggedFile;
    }

    impl StoreFs for LoggedFs<'_> {
        fn open_root(
            &self,
            dir: &Path,
            role: RootRole,
            access: RootAccess,
        ) -> Result<PathBuf, VfsError> {
            self.fs.open_root(dir, role, access)
        }

        fn create_root(&self, dir: &Path, role: RootRole) -> Result<PathBuf, VfsError> {
            self.fs.create_root(dir, role)
        }

        fn open(
            &self,
            root: &PathBuf,
            rel: RelPath<'_>,
            access: Access,
            hint: OpenHint,
        ) -> Result<LoggedFile, VfsError> {
            let file = self.fs.open(root, rel, access, hint)?;
            Ok(self.opened(file, rel, access == Access::ReadWrite))
        }

        fn create_new(&self, root: &PathBuf, rel: RelPath<'_>) -> Result<LoggedFile, VfsError> {
            let file = self.fs.create_new(root, rel)?;
            Ok(self.opened(file, rel, true))
        }

        fn create_dir(&self, root: &PathBuf, rel: RelPath<'_>) -> Result<(), VfsError> {
            self.fs.create_dir(root, rel)
        }

        fn remove_dir(&self, root: &PathBuf, rel: RelPath<'_>) -> Result<(), VfsError> {
            self.fs.remove_dir(root, rel)
        }

        fn list_dir(
            &self,
            root: &PathBuf,
            dir: Option<RelPath<'_>>,
        ) -> Result<Vec<DirEntry>, VfsError> {
            self.fs.list_dir(root, dir)
        }

        fn read_at(&self, f: &LoggedFile, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
            self.fs.read_at(&f.file, offset, buf)
        }

        fn read_exact_at(
            &self,
            f: &LoggedFile,
            offset: u64,
            buf: &mut [u8],
        ) -> Result<(), VfsError> {
            self.fs.read_exact_at(&f.file, offset, buf)
        }

        fn write_at(&self, f: &LoggedFile, offset: u64, buf: &[u8]) -> Result<(), VfsError> {
            self.log(f, Io::Write);
            self.fs.write_at(&f.file, offset, buf)
        }

        fn sync(&self, f: &LoggedFile, kind: SyncKind) -> Result<(), DurabilityFailure> {
            self.log(f, Io::Sync(kind));
            self.fs.sync(&f.file, kind)
        }

        fn sync_dir(
            &self,
            root: &PathBuf,
            dir: Option<RelPath<'_>>,
        ) -> Result<(), DurabilityFailure> {
            self.fs.sync_dir(root, dir)
        }

        fn sync_group(
            &self,
            members: &[GroupMember<'_, PathBuf, LoggedFile>],
        ) -> Result<(), DurabilityFailure> {
            let mut inner = Vec::new();
            for m in members {
                inner.push(match *m {
                    GroupMember::File { file, kind } => {
                        self.log(file, Io::Sync(kind));
                        GroupMember::File {
                            file: &file.file,
                            kind,
                        }
                    }
                    GroupMember::Dir { root, dir } => GroupMember::Dir { root, dir },
                });
            }
            self.fs.sync_group(&inner)
        }

        fn fail_stop(&self, failure: DurabilityFailure) -> ! {
            self.fs.fail_stop(failure)
        }

        fn create_extent(
            &self,
            root: &PathBuf,
            rel: RelPath<'_>,
            len: u64,
            vol: &StoreVolume,
        ) -> Result<LoggedFile, VfsError> {
            let file = self.fs.create_extent(root, rel, len, vol)?;
            Ok(self.opened(file, rel, true))
        }

        fn recycle_extent(
            &self,
            f: &LoggedFile,
            len: u64,
            vol: &StoreVolume,
        ) -> Result<(), VfsError> {
            self.log(f, Io::Write);
            self.fs.recycle_extent(&f.file, len, vol)
        }

        fn seal(&self, f: &LoggedFile) -> Result<(), VfsError> {
            self.fs.seal(&f.file)
        }

        fn unlink(
            &self,
            root: &PathBuf,
            rel: RelPath<'_>,
            retry: ShareRetry,
        ) -> Result<(), VfsError> {
            self.fs.unlink(root, rel, retry)
        }

        fn rename_noreplace(
            &self,
            from_root: &PathBuf,
            from: RelPath<'_>,
            to_root: &PathBuf,
            to: RelPath<'_>,
            retry: ShareRetry,
        ) -> Result<(), VfsError> {
            self.fs
                .rename_noreplace(from_root, from, to_root, to, retry)
        }

        fn rename_replace(
            &self,
            from_root: &PathBuf,
            from: RelPath<'_>,
            to_root: &PathBuf,
            to: RelPath<'_>,
            retry: ShareRetry,
        ) -> Result<(), VfsError> {
            self.fs.rename_replace(from_root, from, to_root, to, retry)
        }

        fn swap_dirs(
            &self,
            a_parent: &PathBuf,
            a: RelPath<'_>,
            b_parent: &PathBuf,
            b: RelPath<'_>,
            retry: ShareRetry,
        ) -> Result<SwapOutcome, VfsError> {
            self.fs.swap_dirs(a_parent, a, b_parent, b, retry)
        }

        fn swap_recover(
            &self,
            a_parent: &PathBuf,
            a: RelPath<'_>,
            retry: ShareRetry,
        ) -> Result<SwapRecovery, VfsError> {
            self.fs.swap_recover(a_parent, a, retry)
        }

        fn file_size(&self, f: &LoggedFile) -> Result<u64, VfsError> {
            self.fs.file_size(&f.file)
        }

        fn identity(&self, f: &LoggedFile) -> Result<FileIdentity, VfsError> {
            self.fs.identity(&f.file)
        }

        fn root_identity(&self, root: &PathBuf) -> Result<FileIdentity, VfsError> {
            self.fs.root_identity(root)
        }

        fn path_identity(
            &self,
            root: &PathBuf,
            rel: RelPath<'_>,
        ) -> Result<FileIdentity, VfsError> {
            self.fs.path_identity(root, rel)
        }

        fn free_space(&self, root: &PathBuf) -> Result<FreeSpace, VfsError> {
            self.fs.free_space(root)
        }

        fn advise_dontneed(&self, f: &LoggedFile, offset: u64, len: u64) {
            self.fs.advise_dontneed(&f.file, offset, len)
        }

        fn counters(&self) -> VfsCounters {
            self.fs.counters()
        }
    }

    impl SealedMaps for LoggedFs<'_> {
        type Map = MemMap;

        fn map_sealed(
            &self,
            f: &LoggedFile,
            expected_len: u64,
            name: RelPath<'_>,
        ) -> Result<MemMap, MapError> {
            self.fs.map_sealed(&f.file, expected_len, name)
        }

        fn advise(&self, map: &MemMap, offset: u64, len: u64, advice: Advice) {
            self.fs.advise(map, offset, len, advice)
        }
    }

    /// The lsn ranges that the reads of `log.<k>` among `reads` cover, merged and sorted.
    fn lsn_cover(geo: Geometry, reads: &[Read]) -> Vec<(u64, u64)> {
        let mut v: Vec<(u64, u64)> = reads
            .iter()
            .filter_map(|(name, off, len)| {
                let k: u64 = name.strip_prefix("log.")?.parse().ok()?;
                let a = geo.extent_start(k) + off;
                Some((a, a + *len as u64))
            })
            .collect();
        v.sort_unstable();
        let mut out: Vec<(u64, u64)> = Vec::new();
        for (a, b) in v {
            match out.last_mut() {
                Some(l) if a <= l.1 => l.1 = l.1.max(b),
                _ => out.push((a, b)),
            }
        }
        out
    }

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

    /// 15 quantities with valid, unique names ([MP §4.1]): the six layout families at 1e5 and 1e6 (the sparse
    /// AND/popcount pair of R-30 among them), the read and the catch-up at 1e6, the checkpoint at 0.5 M; 18 arms; the
    /// minimum tiers of the budgets (t1 read ≤ 50 µs, t3 checkpoint ≤ 100 ms, t2 catch-up ≤ 5 ms,
    /// `ARCHITECTURE-RESEARCH.md:130`, `:140`), none for the layout families and no tail for the sparse pair.
    #[test]
    fn grid_has_fifteen_valid_quantities() {
        let g = grid();
        let names: Vec<&str> = g.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "csr.n1e5",
                "bitset.n1e5",
                "bitset-ab.n1e5",
                "bitset-aa.n1e5",
                "scan.n1e5",
                "overlay.n1e5",
                "csr.n1e6",
                "bitset.n1e6",
                "bitset-ab.n1e6",
                "bitset-aa.n1e6",
                "scan.n1e6",
                "overlay.n1e6",
                "read-4096.n1e6",
                "ckpt-4096.n5e5",
                "catchup-16.n1e6"
            ]
        );
        assert!(names.iter().all(|n| valid_name(n)));
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 15);
        let arms: Vec<&str> = g
            .iter()
            .flat_map(|q| q.family.arms().iter().copied())
            .collect();
        assert_eq!(arms.len(), 18);
        for f in [Family::BitsetAb, Family::BitsetAa] {
            assert_eq!(f.arms(), ["and.popcount"]);
            assert_eq!((f.min_tier(), f.needs_tail()), (None, false));
        }
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
        assert_eq!(g[13].family, Family::Ckpt);
        let t = target(&g[13], Profile::Production);
        assert_eq!((t.clause, t.threshold, t.n), (Clause::Ops, 4_096, 500_000));
        assert_eq!(target(&g[13], Profile::Test).n, 5_000);
    }

    /// `--smoke` runs every quantity at the test profile with each arm once: 18 lines, one per (quantity, arm).
    #[test]
    fn smoke_exercises_every_arm_once() {
        let fs = MemFs::with_dir(&scratch());
        let (code, out, err) = run(&fs, &["--smoke", "--scratch", "/scratch"]);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 18, "{out}");
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
        assert_eq!(fs.log().maps.len(), 15, "each run maps its base once");
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
        assert_eq!(out.lines().count(), 18);
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

    /// The layout probes return the naive counts, at an N whose four status counts differ (1,002: 250, 251, 251,
    /// 250): the CSR probe the sum over the generator's out-edges and their reverse; AND/popcount and both scans the
    /// rows of #N mod 4 = s; the overlay probe membership in the tail's owners. The split column holds `NODE`'s status
    /// bytes. The counting arms' operations (critique I-1) are named as their family's arms and return, call by call,
    /// the counts of s = 1, 2, 3, 0, …: arm 0 from `NODE` and the floor from its own column, which a rotated column
    /// shows.
    #[test]
    fn layout_arms_equal_naive_counts() {
        let (t, b) = tail_and_base(1_002, 200, 3);
        let c = Columns::parse(&b).unwrap();
        let s = base::Spec {
            n: 1_002,
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
        let col = split_status(&c);
        let rows: Vec<u8> = (0..c.rows).map(|i| c.node(i).status).collect();
        assert_eq!(col, rows, "the split column holds NODE's status bytes");
        assert_eq!(col, (1..=s.n).map(|id| (id % 4) as u8).collect::<Vec<_>>());
        let mut naive = Vec::new();
        for st in 0..STATUSES {
            let n = (1..=s.n).filter(|id| id % 4 == u32::from(st)).count() as u64;
            let set = c.frozen(kind_status_key(BASE_KIND, st)).unwrap();
            assert_eq!(and_popcount(&kind, &set), n, "status {st}");
            assert_eq!(scan_status(&c, st), n, "status {st}");
            assert_eq!(scan_split(&col, st), n, "status {st}");
            naive.push(n);
        }
        assert_eq!(naive, [250, 251, 251, 250]);
        // The arms' operations, call by call: s = 1, 2, 3, 0, 1, …
        let calls = |ops: &mut CountOp<'_>| (0..8).map(|_| ops()).collect::<Vec<u64>>();
        let want = |shift: usize| -> Vec<u64> { (1..=8).map(|i| naive[(i + shift) % 4]).collect() };
        let real = Operands {
            split: col.clone(),
            ..Operands::default()
        };
        let mut v = count_ops(Family::Scan, &c, &real).unwrap();
        let names: Vec<&str> = v.iter().map(|o| o.0).collect();
        assert_eq!(names, Family::Scan.arms());
        assert_eq!(calls(&mut v[0].1), want(0), "scan.status");
        assert_eq!(
            calls(&mut v[1].1),
            want(0),
            "scan.split: the same counts as arm 0"
        );
        let rotated = Operands {
            split: col.iter().map(|b| (b + 1) % 4).collect(),
            ..Operands::default()
        };
        let mut v = count_ops(Family::Scan, &c, &rotated).unwrap();
        assert_eq!(calls(&mut v[0].1), want(0), "scan.status reads NODE");
        assert_eq!(
            calls(&mut v[1].1),
            want(3),
            "scan.split reads its own column, not NODE"
        );
        let none = Operands::default();
        let mut v = count_ops(Family::Bitset, &c, &none).unwrap();
        assert_eq!(
            v.iter().map(|o| o.0).collect::<Vec<_>>(),
            Family::Bitset.arms()
        );
        assert_eq!(calls(&mut v[0].1), want(0), "and.popcount");
        assert!(count_ops(Family::Csr, &c, &real).is_err());
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

    /// The split floor scans the column its run builds from `NODE` (u12 review r1 I-1; R-29): for a `scan` quantity,
    /// [`prepare`]'s [`Prepared::operands`] hold one byte per row of the mapped base, each row's `NodeHdr.status` in
    /// row order; [`arms`] hands these operands to [`count_ops`], whose `scan.split` scans them. A run's floor over the
    /// 4-byte `DUE` column, or over an empty one, fails here.
    #[test]
    fn split_floor_scans_the_fixtures_status_bytes() {
        let fs = MemFs::with_dir(&scratch());
        let (q, p) = fixture(&fs, Family::Scan);
        let c = Columns::parse(p.map.bytes()).unwrap();
        assert_eq!(c.rows, p.n as usize, "{}", q.name);
        let rows: Vec<u8> = (0..c.rows).map(|i| c.node(i).status).collect();
        assert!(
            p.operands.split == rows,
            "{}: the run's split column ({} bytes) is not NODE's status bytes ({} rows)",
            q.name,
            p.operands.split.len(),
            c.rows
        );
        assert!(
            rows.iter().any(|&s| s != 0),
            "{}: every row's status is 0, so the column check is vacuous",
            q.name
        );
    }

    /// The sparse AND/popcount quantities take the array paths of the frozen AND (R-30; test m-2): at N = 100,000 and
    /// 1,000,000 every chunk of each sparse set A_s and B_s is an array and every chunk of kind-status s a bitmap, and
    /// both ANDs equal the naive intersections. Through the arms' operations (critique I-1), on a base at N = 20,000,
    /// where kind-status s is already a bitmap and A_s, B_s arrays: `bitset-ab` returns |kind-status s ∩ A_s| and
    /// `bitset-aa` |A_s ∩ B_s|, call by call for s = 1, 2, 3, 0, …
    #[test]
    fn sparse_bitset_arms_take_the_array_paths() {
        let frozen = |ids: &[u32]| -> Vec<u8> {
            let mut b = Vec::new();
            encode_frozen(ids, &mut b).unwrap();
            b
        };
        let arrays = |f: &Frozen<'_>| {
            !f.chunks().is_empty()
                && f.chunks()
                    .iter()
                    .all(|c| matches!(c.2, Container::Array(_)))
        };
        let bitmaps = |f: &Frozen<'_>| {
            !f.chunks().is_empty()
                && f.chunks()
                    .iter()
                    .all(|c| matches!(c.2, Container::Bitmap(_)))
        };
        let meet = |x: &[u32], y: &[u32]| -> u64 {
            let y: BTreeSet<u32> = y.iter().copied().collect();
            x.iter().filter(|v| y.contains(v)).count() as u64
        };
        for n in [100_000u32, 1_000_000] {
            for s in 0..STATUSES {
                let ks: Vec<u32> = (1..=n).filter(|id| id % 4 == u32::from(s)).collect();
                let (a, b) = (sparse_ids(n, SALT_A, s), sparse_ids(n, SALT_B, s));
                assert!(
                    a.len() > (n / 40) as usize && b.len() > (n / 40) as usize,
                    "N {n} s {s}: {} {}",
                    a.len(),
                    b.len()
                );
                let (kb, ab, bb) = (frozen(&ks), frozen(&a), frozen(&b));
                let (kf, af, bf) = (
                    Frozen::parse(&kb).unwrap(),
                    Frozen::parse(&ab).unwrap(),
                    Frozen::parse(&bb).unwrap(),
                );
                assert!(bitmaps(&kf), "N {n} s {s}: kind-status s is a bitmap");
                assert!(
                    arrays(&af) && arrays(&bf),
                    "N {n} s {s}: A_s, B_s are arrays"
                );
                let ab_naive = a.iter().filter(|id| *id % 4 == u32::from(s)).count() as u64;
                assert_eq!(and_popcount(&kf, &af), ab_naive, "N {n} s {s}: ab");
                assert_eq!(and_popcount(&af, &bf), meet(&a, &b), "N {n} s {s}: aa");
            }
        }
        let n = 20_000;
        let b = base::build(&base::Spec {
            n,
            upto_lsn: 1 << 16,
            base_seq: BASE_SEQ,
        })
        .unwrap();
        let c = Columns::parse(&b).unwrap();
        for family in [Family::BitsetAb, Family::BitsetAa] {
            let ops = operands(family, &c).unwrap();
            for s in 0..STATUSES {
                let ks = c.frozen(kind_status_key(BASE_KIND, s)).unwrap();
                assert!(bitmaps(&ks), "N {n} s {s}: kind-status s is a bitmap");
                let i = usize::from(s);
                assert!(arrays(&Frozen::parse(&ops.sparse_a[i]).unwrap()));
                assert!(arrays(&Frozen::parse(&ops.sparse_b[i]).unwrap()));
            }
            let mut v = count_ops(family, &c, &ops).unwrap();
            assert_eq!(v.iter().map(|o| o.0).collect::<Vec<_>>(), family.arms());
            let got: Vec<u64> = (0..8).map(|_| v[0].1()).collect();
            let want: Vec<u64> = (1..=8)
                .map(|i| {
                    let s = (i % 4) as u8;
                    let a = sparse_ids(n, SALT_A, s);
                    match family {
                        Family::BitsetAb => {
                            a.iter().filter(|id| *id % 4 == u32::from(s)).count() as u64
                        }
                        _ => meet(&a, &sparse_ids(n, SALT_B, s)),
                    }
                })
                .collect();
            assert_eq!(got, want, "{family:?}: the arm's operands");
        }
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

    /// The checkpoint arm folds a fresh tail per sample (scoping §4.2): for j = 0, 1, 2, `seg.d1` at sample j's t1
    /// is sealed and reads back to the fold of [`ckpt_tail`] j in memory, and the three deltas differ pairwise; the
    /// floor's sample j writes the same bytes to `seg.d2`; each arm's teardown removes its file, so neither is left
    /// after a sample; the fold reads the extents.
    #[test]
    fn ckpt_arm_writes_a_sealed_delta_outside_setup() {
        let fs = MemFs::with_dir(&scratch());
        let (q, p) = fixture(&fs, Family::Ckpt);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let ticker = FsTicker::new(&fs, p.dir(), &[DELTA_NAME, FLOOR_NAME]);
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        let names: Vec<&str> = arms.iter().map(|a| a.name()).collect();
        assert_eq!(names, ["ckpt", "ckpt.write"]);
        let (d1, d2) = (p.dir().join(DELTA_NAME), p.dir().join(FLOOR_NAME));
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for j in 0..3 {
            let tj = ckpt_tail(&q, Profile::Test, j).unwrap();
            let want = fold_tail(&mut MemLog::new(tj.start, &tj.bytes), &tj, &c).unwrap();
            assert!(!want.rows.is_empty());
            arms[0].sample().unwrap();
            let (bytes, sealed) = ticker.last().1.files[DELTA_NAME].clone();
            assert!(sealed, "sample {j}: seg.d1 sealed at t1");
            assert_eq!(
                decode(&bytes).unwrap(),
                want,
                "sample {j}: seg.d1 is the fold of tail {j}"
            );
            assert!(
                fs.bytes(&d1).is_none(),
                "sample {j}: seg.d1 left after the sample"
            );
            for (i, b) in seen.iter().enumerate() {
                assert_ne!(*b, bytes, "the deltas of samples {i} and {j} are equal");
            }
            arms[1].sample().unwrap();
            let (floor, sealed) = ticker.last().1.files[FLOOR_NAME].clone();
            assert!(sealed, "sample {j}: seg.d2 sealed at t1");
            assert_eq!(floor, bytes, "sample {j}: the floor writes ckpt's bytes");
            assert!(
                fs.bytes(&d2).is_none(),
                "sample {j}: seg.d2 left after the sample"
            );
            seen.push(bytes);
        }
        assert!(
            fs.log().reads.iter().any(|r| r.0.starts_with("log.")),
            "the fold reads the extents"
        );
    }

    /// The fixture of `family`'s first quantity at the test profile.
    fn fixture<'v, V: StoreFs + SealedMaps>(
        fs: &'v V,
        family: Family,
    ) -> (Quantity, Prepared<'v, V>) {
        let q = grid().into_iter().find(|q| q.family == family).unwrap();
        let p = prepare(fs, &scratch(), &q, Profile::Test).unwrap();
        (q, p)
    }

    /// The checkpoint's operation runs inside its timed region (spec Goal 2, audit mutation C): at sample j's t0 the
    /// extent holds tail j's image, as the fixture would write it (scoping §4.2); between the ticks of `ckpt` the store
    /// reads only the log, exactly tail j's `[start, end)` at its extent offsets, and `seg.d1` goes from absent at t0 to
    /// present and sealed at t1; the floor `ckpt.write` reads no log between its ticks and leaves `seg.d2` sealed at
    /// t1. Each file is removed by its arm's teardown, after t1 (review m-5).
    #[test]
    fn ckpt_arm_folds_inside_its_timed_region() {
        let fs = MemFs::with_dir(&scratch());
        let (q, p) = fixture(&fs, Family::Ckpt);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let geo = p.tail.as_ref().unwrap().expect.geo;
        let ext = extent_name(geo.extent(p.tail.as_ref().unwrap().start));
        let ticker = FsTicker::new(&fs, p.dir(), &[DELTA_NAME, FLOOR_NAME, &ext]);
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        let mut tails = BTreeSet::new();
        for k in 0..2 {
            let t = ckpt_tail(&q, Profile::Test, k).unwrap();
            tails.insert(t.bytes.clone());
            arms[0].sample().unwrap();
            let (t0, t1) = ticker.last();
            let w = ticker.timed_reads();
            let mut image = vec![0u8; geo.e() as usize];
            let at = geo.offset(t.start) as usize;
            image[at..at + t.bytes.len()].copy_from_slice(&t.bytes);
            assert!(
                t0.files[&ext].0 == image,
                "sample {k}: the extent at t0 is not tail {k}'s image"
            );
            assert!(
                !t0.files.contains_key(DELTA_NAME),
                "sample {k}: seg.d1 at t0"
            );
            assert!(
                !w.is_empty(),
                "sample {k}: no log.* read between the ticks: the fold left the timed region"
            );
            assert!(
                w.iter().all(|r| r.0.starts_with("log.")),
                "sample {k}: {w:?}"
            );
            assert_eq!(lsn_cover(geo, &w), [(t.start, t.end)], "sample {k}");
            assert_eq!(
                t1.files.get(DELTA_NAME).map(|f| f.1),
                Some(true),
                "sample {k}: seg.d1 sealed at t1"
            );
            assert!(
                fs.bytes(&p.dir().join(DELTA_NAME)).is_none(),
                "sample {k}: seg.d1 left after the sample"
            );
            arms[1].sample().unwrap();
            let (t0, t1) = ticker.last();
            let w = ticker.timed_reads();
            assert!(
                !t0.files.contains_key(FLOOR_NAME),
                "sample {k}: seg.d2 at t0"
            );
            assert!(
                w.iter().all(|r| !r.0.starts_with("log.")),
                "sample {k}: the floor reads the log: {w:?}"
            );
            assert_eq!(
                t1.files.get(FLOOR_NAME).map(|f| f.1),
                Some(true),
                "sample {k}: seg.d2 sealed at t1"
            );
            assert!(
                fs.bytes(&p.dir().join(FLOOR_NAME)).is_none(),
                "sample {k}: seg.d2 left after the sample"
            );
        }
        assert_eq!(tails.len(), 2, "tails 0 and 1 differ");
        assert_eq!(ticker.count(), 8, "two ticks per sample");
    }

    /// The checkpoint's setup rewrites the extent through the read-write handle that [`arms`] opens for the run, and
    /// flushes it before t0 (u12 test r1 I-1, I-2; the module doc's `ckpt` row). Over a [`LoggedFs`]: `arms` opens the
    /// extent once, for writing; in each sample, every call on the extent before t0 goes through that handle, the
    /// setup writes it, and its last call on it is `sync(Data)`; between the ticks the extent is at most opened for
    /// reading (the fold's log), never written or flushed. So no sample opens or closes a handle that writes the
    /// extent, and the timed region finds the rewrite flushed.
    #[test]
    fn ckpt_setup_flushes_the_tail_through_the_runs_handle() {
        let fs = MemFs::with_dir(&scratch());
        let lfs = LoggedFs::new(&fs);
        let (q, p) = fixture(&lfs, Family::Ckpt);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let t = p.tail.as_ref().unwrap();
        let ext = extent_name(t.expect.geo.extent(t.start));
        let ticker = FsTicker::new(&fs, p.dir(), &[]).logging(&lfs);
        let on_ext = |from: usize, to: usize| -> Vec<(usize, Io)> {
            lfs.calls()[from..to]
                .iter()
                .filter(|c| c.0 == ext)
                .map(|c| (c.1, c.2))
                .collect()
        };
        let before = lfs.calls().len();
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        let mut from = lfs.calls().len();
        let opened = on_ext(before, from);
        let &[(h, Io::Open(true))] = opened.as_slice() else {
            panic!("arms opens {ext} once, for writing: {opened:?}");
        };
        for k in 0..3 {
            arms[0].sample().unwrap();
            let (t0, t1) = ticker.last();
            let setup = on_ext(from, t0.calls);
            assert!(
                setup.iter().all(|&(n, _)| n == h),
                "sample {k}: a call on {ext} before t0 through another handle than the run's {h}: {setup:?}"
            );
            assert!(
                setup.contains(&(h, Io::Write)),
                "sample {k}: the setup does not rewrite {ext}: {setup:?}"
            );
            assert_eq!(
                setup.last(),
                Some(&(h, Io::Sync(SyncKind::Data))),
                "sample {k}: the setup's last call on {ext} is not its flush"
            );
            let timed = on_ext(t0.calls, t1.calls);
            assert!(
                timed.iter().all(|&(_, io)| io == Io::Open(false)),
                "sample {k}: {ext} written, flushed or opened for writing between the ticks: {timed:?}"
            );
            from = t1.calls;
        }
        assert_eq!(ticker.count(), 6, "two ticks per sample");
    }

    /// The catch-up runs inside its timed region (spec Goal 2, audit mutation D): between the ticks the store reads
    /// `HEAD` exactly once (8,192 B at 0) and the log exactly over `[x, end)`, x being the warm state's boundary.
    #[test]
    fn catchup_arm_scans_inside_its_timed_region() {
        let fs = MemFs::with_dir(&scratch());
        let (q, p) = fixture(&fs, Family::Catchup);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let t = p.tail.as_ref().unwrap();
        let (x, _, _) = warm(t, BURST).unwrap();
        assert!(x > t.start && x < t.end, "a warm state and a burst");
        let ticker = FsTicker::new(&fs, p.dir(), &[]);
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        for k in 0..2 {
            arms[0].sample().unwrap();
            let w = ticker.timed_reads();
            let head: Vec<&Read> = w.iter().filter(|r| r.0 == "HEAD").collect();
            assert_eq!(
                head,
                [&("HEAD".to_string(), 0, HEAD_LEN)],
                "sample {k}: {w:?}"
            );
            let log: Vec<Read> = w.iter().filter(|r| r.0 != "HEAD").cloned().collect();
            assert!(
                !log.is_empty(),
                "sample {k}: only the HEAD read between the ticks: the scan left the timed region"
            );
            assert!(
                log.iter().all(|r| r.0.starts_with("log.")),
                "sample {k}: {w:?}"
            );
            assert_eq!(lsn_cover(t.expect.geo, &log), [(x, t.end)], "sample {k}");
        }
        assert_eq!(ticker.count(), 4, "two ticks per sample");
    }

    /// T1's read samples the nodes the tail touched (spec Goal 2, audit mutation I): over one cycle of as many
    /// samples as the node index has entries, every sample reads between its ticks, its first read is the 32-byte
    /// record header at a node index lsn, and the first reads' lsns are the node index's, as a multiset.
    #[test]
    fn read_arm_samples_the_nodes_the_tail_touched() {
        let fs = MemFs::with_dir(&scratch());
        let (q, p) = fixture(&fs, Family::Read);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let t = p.tail.as_ref().unwrap();
        let geo = t.expect.geo;
        let mut want: Vec<u64> = t.overlay().unwrap().nodes().iter().map(|e| e.lsn).collect();
        assert!(want.len() > 1, "{} nodes", want.len());
        let ticker = FsTicker::new(&fs, p.dir(), &[]);
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        let mut got = Vec::new();
        for k in 0..want.len() {
            arms[0].sample().unwrap();
            let w = ticker.timed_reads();
            let Some((name, off, len)) = w.first() else {
                panic!("sample {k}: a sample with no read between its ticks");
            };
            assert_eq!(*len, REC_HDR_LEN, "sample {k}: {w:?}");
            let n: u64 = name
                .strip_prefix("log.")
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("sample {k}: first read {name}"));
            got.push(geo.extent_start(n) + off);
        }
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(
            got, want,
            "the cycle's first reads are the node index's lsns"
        );
        assert_eq!(ticker.count(), 2 * want.len(), "two ticks per sample");
    }

    /// A product whose drop advances the fake clock by 4 ms.
    struct Heavy(FakeTicker);

    impl Drop for Heavy {
        fn drop(&mut self) {
            self.0.advance(4 * MS);
        }
    }

    /// A staged arm times only its operation: the setup's 5 ms, the teardown's 3 ms and the 4 ms of the product's
    /// drop stay outside, the operation's 2 ms are the sample; the teardown gets the op's product; a failing setup,
    /// operation or teardown fails the sample; it never batches.
    #[test]
    fn staged_arm_times_only_the_operation() {
        let clock = FakeTicker::manual();
        let (c1, c2, c3) = (clock.clone(), clock.clone(), clock.clone());
        let got = Rc::new(std::cell::Cell::new(0u32));
        let g = got.clone();
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
                    Ok((v + fail, Heavy(c2.clone())))
                }
            },
            move |(v, h): (u32, Heavy)| {
                c3.advance(3 * MS);
                g.set(v);
                drop(h);
                Ok(())
            },
        );
        assert_eq!(a.sample(), Ok(2 * MS));
        assert_eq!(got.get(), 8, "the teardown gets the op's product");
        a.set_batch(20);
        assert_eq!(a.batch(), 1);
        assert_eq!(a.sample(), Ok(2 * MS));
        assert_eq!(got.get(), 9);
        assert_eq!(a.sample(), Err(ArmError("op".into())));
        let mut b = Staged::new(
            "y",
            clock.clone(),
            || Err::<(), _>(ArmError("setup".into())),
            |()| Ok(0),
            |_: u32| Ok(()),
        );
        assert_eq!(b.sample(), Err(ArmError("setup".into())));
        let mut d = Staged::new(
            "z",
            clock,
            || Ok(()),
            |()| Ok(0u8),
            |_: u8| Err(ArmError("teardown".into())),
        );
        assert_eq!(d.sample(), Err(ArmError("teardown".into())));
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
            assert_eq!(calls.len(), 15);
            for (c, q) in calls.iter().zip(grid()) {
                let mut want = base.clone();
                want.push("--quantity".into());
                want.push(q.name.into());
                assert_eq!(c, &want);
            }
            assert_eq!(String::from_utf8(out).unwrap().lines().count(), 15);
        }
        assert!(fs.files().is_empty(), "the parent writes no fixture");
    }
}
