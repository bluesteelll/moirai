//! Measurement 14 (WP-53e, [60 §5.2] item 14, `60-roadmap.md:770`; `docs/m0/PLAN.md:317`): the layout probes over
//! the F09 segment of [`crate::seg`], at 1e5 and 1e6 nodes, and the T1 and T2 trigger quantities: a point read after
//! a full 4,096-op tail and a delta checkpoint at 0.5 M nodes (T1, `ARCHITECTURE-RESEARCH.md:130`), and the MCP
//! overlay catch-up (T2, `:140`). T2's 16-writer wait is taken from measurement 2's run (R-24 Q7) and is not built
//! here. The tails are u10's ([`crate::tail::synth`]), from [70 S8]'s spec-derived mix (R-24 Q10).
//!
//! **The grid** (18 quantities, names by [MP §4.1]), run with the deciding quantities first: `read-4096.n1e6`,
//! `ckpt-4096.n5e5` and `catchup-16.n1e6`, then T1's read diagnostics `read-op-4096.n1e6`,
//! `read-cost-records-4096.n1e6` and `read-cost-bytes-4096.n1e6` (R-40 O-2, R-42, R-44), then the layout probes (R-34
//! B-3, M-15; R-35 Q2):
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
//! | `read-op-4096.n1e6` | `read.op`, `read.op.at` | T1's diagnostic read on `read-4096.n1e6`'s tail and nodes (its seed, [`seed_name`]): `read.op` reads the status by the frame walk ([`Overlay::get_op`]: n's ops located by their frames, decoded alone); its floor `read.op.at` by the offset read ([`Overlay::get_op_at`]: n's newest frames read from an offset kept beside the node index, in the design the node entry's spare bytes). The paired difference is the cost of locating n's frames in the newest record (R-42 Q2). The setup replays the overlay from the fixture's store, builds the offset index and refuses the run unless the whole-record read, the walk and the offset read agree on every node of the cycle ([`read_ops_agree`]) | t1 |
//! | `read-cost-records-4096.n1e6` | `records` | T1's read cost on the same tail and nodes: each sample is one whole-record read ([`point_read`]) through [`Counted`], and the sample is the records it walks ([MP §4.8]) | t1 |
//! | `read-cost-bytes-4096.n1e6` | `bytes` | the same read through [`Counted`] on the same tail and nodes, and the sample is the bytes it decodes ([MP §4.8]). The records and the bytes are two counts of one read, so they are two quantities of one arm each: [MP §4.1] makes every arm after arm 0 a floor of the operation, and neither count is a floor of the other (R-44 I-1 (b), as R-30 Q1 reads §4.1) | t1 |
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
//! bytes inside the timed region and returned a placeholder of the same type would still pass every test. The run's
//! touch of every page of the mapped base before [`measure`] ([`prefault`]; R-34 M-5) is an in-memory read too: no
//! test sees that it precedes the pilot, and `prefault_reads_one_byte_of_every_page` pins what it reads. The read
//! diagnostics' in-memory setup (the id order, the buffers, the copy of the offset index) is seen only through the reads
//! it leads to: an arm that did the same reads between its ticks but returned a value decoded in its setup would pass
//! every test, since the rig observes reads only (u17 critique I-2).
//!
//! Every run's fixture is the base `seg.base.1` at the quantity's N ([`crate::seg::base`]), written, sealed and
//! mapped, and, for the seven families that read one (the overlay probe, the read and its three diagnostics, the
//! checkpoint and the catch-up), a full `ops` tail at P06 (4,096 at production) written as `log.<n>` extents with
//! `HEAD`; it is removed on every path out of the run. The probe's choices: the ids a probe cycles through
//! ([`PROBE_IDS`] drawn from the quantity's seed; the read's and its diagnostics' are the tail's touched nodes in the
//! order of the read's seed, [`read_cycle`]), the families' single arms, and the catch-up's burst. The sparse sets are the probe's too: A_s = {#N : mix(`SALT_A` ^ #N) mod 32
//! = s} and B_s likewise with `SALT_B`, for s = 0–3, about 2,048 ids per 65,536-id chunk, under the array limit of
//! 4,096 ([`sparse_ids`]); they live in the fixture's memory, not in the base, so the base's sections are unchanged. At
//! the test profile (N / 100) the kind-status sets are arrays too, so the smoke's `bitset-ab` takes the array × array
//! path; the array × bitmap path at production N is pinned by `sparse_bitset_arms_take_the_array_paths`.
//!
//! **The driver** ([MP §9.7] as merged, through the shared [`crate::m1::driver`]): `--host`, `--condition`,
//! `--load-log`, `--quantity`, `--commit` and `--toolchain` (required), `--out` (required unless `--smoke`, refused
//! with it, created when missing), `--smoke` (the requested quantity, or every one, at the test profile, N / 100 and
//! its parameters, each arm once, nothing recorded) and `--help`. Every fixture lies below the working directory
//! ([MP §9.7] "The working directory"; R-34 B-1). A recording run touches every page of its mapped base before the
//! pilot ([`prefault`]), so the pilot's batch is not set by first-touch page faults (R-34 M-5); the smoke does not.
//! Without `--quantity`, each quantity runs in a child process of its own ([MP §4.6]). Exit 0 done, 1 a run or arm
//! failed or a record could not be written, 2 a usage error.

use crate::arm::{Arm, ArmError, Ticker, TimedFn, ValueFn};
use crate::host::{CommandRunner, HostKind};
use crate::m1::driver::{self, Args};
use crate::overlay::op::OpIndex;
use crate::overlay::{Builder, Key, Overlay, Probe, Value, kind_key, kind_status_key};
use crate::run::{Context, RunSpec, measure};
use crate::seg::base::{self, Columns};
use crate::seg::bitset::{Frozen, and_popcount, encode_frozen};
use crate::seg::delta;
use crate::seg::{NODE_HDR_LEN, STATUS_AT, u32_at};
use crate::tail::rec::{Payload, RecHdr};
use crate::tail::scan::{LogSource, MemLog, scan};
use crate::tail::store::{VfsLog, extent_name};
use crate::tail::synth::{BASE_KIND, Clause, STATUSES, Tail, Target, generate};
use crate::tail::{HEAD_LEN, Params, REC_HDR_LEN, TAIL_READ_CHUNK, TRAILER_LEN};
use crate::tier::Tier;
use crate::units::Unit;
use moirai_vfs::{
    Access, Meter, OS_SHARE_RETRY_MS, OpenHint, RelPath, RelPathBuf, RootAccess, RootRole,
    SealedMap, SealedMaps, ShareRetry, StoreFs, SyncKind, VfsErrorKind,
};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    /// T1's diagnostic: the read by the frame walk, and its floor by the offset read.
    ReadOp,
    /// T1's diagnostic: the records the read walks.
    ReadCostRecords,
    /// T1's diagnostic: the bytes the read decodes.
    ReadCostBytes,
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
            Family::ReadOp => "read-op-4096",
            Family::ReadCostRecords => "read-cost-records-4096",
            Family::ReadCostBytes => "read-cost-bytes-4096",
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
            Family::ReadOp => &["read.op", "read.op.at"],
            Family::ReadCostRecords => &["records"],
            Family::ReadCostBytes => &["bytes"],
            Family::Ckpt => &["ckpt", "ckpt.write"],
            Family::Catchup => &["catchup"],
        }
    }

    /// The minimum tier: the tier of the quantity's budget ([MP §3.2]); the layout probes have none.
    pub const fn min_tier(self) -> Option<Tier> {
        match self {
            Family::Read | Family::ReadOp | Family::ReadCostRecords | Family::ReadCostBytes => {
                Some(Tier::T1)
            }
            Family::Ckpt => Some(Tier::T3),
            Family::Catchup => Some(Tier::T2),
            _ => None,
        }
    }

    /// Whether the fixture holds a tail.
    pub const fn needs_tail(self) -> bool {
        matches!(
            self,
            Family::Overlay
                | Family::Read
                | Family::ReadOp
                | Family::ReadCostRecords
                | Family::ReadCostBytes
                | Family::Ckpt
                | Family::Catchup
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

/// The 18 quantities in run order: the deciding quantities first (T1's read and checkpoint, T2's catch-up), then T1's
/// read diagnostics, its evidence (R-40 O-2; the read cost's two counts as two quantities, R-44), then the layout
/// probes at 1e5 and 1e6 (R-34 B-3, M-15).
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
    let mut g = vec![
        q(Family::Read, 1_000_000),
        q(Family::Ckpt, 500_000),
        q(Family::Catchup, 1_000_000),
        q(Family::ReadOp, 1_000_000),
        q(Family::ReadCostRecords, 1_000_000),
        q(Family::ReadCostBytes, 1_000_000),
    ];
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

/// The name `q`'s seed is drawn from: its own, except that T1's read diagnostics take the read's at the same scale, so
/// their tail is `read-4096`'s byte for byte and their nodes come in its order (spec Goal 1).
pub fn seed_name(q: &Quantity) -> String {
    match q.family {
        Family::ReadOp | Family::ReadCostRecords | Family::ReadCostBytes => format!(
            "{}{}",
            Family::Read.stem(),
            &q.name[q.family.stem().len()..]
        ),
        _ => q.name.clone(),
    }
}

/// The tail of `q` at `profile`: a full `ops` tail at P06 over the base.
pub fn target(q: &Quantity, profile: Profile) -> Target {
    let p = profile.params();
    Target {
        clause: Clause::Ops,
        threshold: p.p06,
        mult: 1,
        n: profile.n(q.n),
        seed: seed(&seed_name(q)),
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

/// The status a probe gives `#id`: the tail's, else the base row's `NodeHdr.status`.
fn status_of(p: Probe, c: &Columns<'_>, id: u32) -> Result<u8, String> {
    match p {
        Probe::Tail(Value::Status(s, _)) => Ok(s),
        Probe::Tail(v) => Err(format!("#{id}'s status probed as {v:?}")),
        Probe::NotInTail if id >= 1 && id as usize <= c.rows => {
            Ok(c.node[NODE_HDR_LEN * (id as usize - 1) + STATUS_AT])
        }
        Probe::NotInTail => Err(format!("#{id} has no base row")),
    }
}

/// T1's point read by the frame walk ([`Overlay::get_op`], through `buf` in chunks of [`TAIL_READ_CHUNK`]), else the base
/// row: the twin of [`point_read`] that decodes only `#id`'s ops.
pub fn point_read_op<S: LogSource>(
    log: &mut S,
    o: &Overlay,
    c: &Columns<'_>,
    id: u32,
    buf: &mut Vec<u8>,
) -> Result<u8, String> {
    status_of(
        o.get_op(log, id, &Key::Status, TAIL_READ_CHUNK, buf)?,
        c,
        id,
    )
}

/// T1's point read by the offset read ([`Overlay::get_op_at`] over `idx`), else the base row: the floor of
/// [`point_read_op`].
pub fn point_read_op_at<S: LogSource>(
    log: &mut S,
    o: &Overlay,
    idx: &OpIndex,
    c: &Columns<'_>,
    id: u32,
    buf: &mut Vec<u8>,
) -> Result<u8, String> {
    status_of(
        o.get_op_at(idx, log, id, &Key::Status, TAIL_READ_CHUNK, buf)?,
        c,
        id,
    )
}

/// The ids T1's read and its diagnostics cycle through: the node index's, ordered by `mix(seed ^ id)`.
pub fn read_cycle(o: &Overlay, seed: u64) -> Vec<u32> {
    let mut ids: Vec<u32> = o.nodes().iter().map(|e| e.n).collect();
    ids.sort_by_key(|&id| mix(seed ^ u64::from(id)));
    ids
}

/// The setup's check of the read diagnostics (spec Goal 1: the same value for every sampled node): reads every id of
/// `ids` with [`point_read`] through `whole`, [`point_read_op`] through `op` and [`point_read_op_at`] through `at`, and
/// refuses, naming the id, any difference or failure.
pub fn read_ops_agree<A: LogSource, B: LogSource, C: LogSource>(
    o: &Overlay,
    idx: &OpIndex,
    c: &Columns<'_>,
    ids: &[u32],
    whole: &mut A,
    op: (&mut B, &mut Vec<u8>),
    at: (&mut C, &mut Vec<u8>),
) -> Result<(), String> {
    let ((op, op_buf), (at, at_buf)) = (op, at);
    for &id in ids {
        let w = point_read(whole, o, c, id);
        let f = point_read_op(op, o, c, id, op_buf);
        let a = point_read_op_at(at, o, idx, c, id, at_buf);
        if w.is_err() || f != w || a != w {
            return Err(format!(
                "#{id}: the whole-record read gives {w:?}, the frame walk {f:?}, the offset read {a:?}"
            ));
        }
    }
    Ok(())
}

/// A [`LogSource`] that counts what [`Overlay::get`]'s whole-record read walks and decodes, from the reads it asks for
/// ([MP §4.8]: counted, never inferred from time). It follows `get`'s read protocol (`overlay/mod.rs`, `read_record`):
/// a 32-byte header read at a record's lsn, then the record's other `len − 32` bytes in one or more reads. At each
/// header it adds 1 to the records and `len − 32 − t` (t = 8 with `group_end`) to the bytes: the bytes `get` decodes. A
/// read that breaks the protocol is an error.
pub struct Counted<S> {
    inner: S,
    records: u64,
    bytes: u64,
    next: u64,
    rest: u64,
}

impl<S> Counted<S> {
    /// A counter over `inner`, at zero.
    pub fn new(inner: S) -> Counted<S> {
        Counted {
            inner,
            records: 0,
            bytes: 0,
            next: 0,
            rest: 0,
        }
    }

    /// Back to zero, a header due.
    pub fn reset(&mut self) {
        (self.records, self.bytes, self.rest) = (0, 0, 0);
    }

    /// The records walked since the last reset.
    pub fn records(&self) -> u64 {
        self.records
    }

    /// The bytes decoded since the last reset.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl<S: LogSource> LogSource for Counted<S> {
    fn read_at(&mut self, lsn: u64, buf: &mut [u8]) -> Result<usize, String> {
        if self.rest == 0 {
            if buf.len() != REC_HDR_LEN {
                return Err(format!(
                    "a read of {} bytes at lsn {lsn} where a record header is due",
                    buf.len()
                ));
            }
            let got = self.inner.read_at(lsn, buf)?;
            if got != REC_HDR_LEN {
                return Err(format!("a record header cut off at lsn {lsn}"));
            }
            let hdr = RecHdr::decode((&*buf).try_into().expect("32 bytes"));
            let len = u64::from(hdr.len);
            let t = if hdr.group_end() { TRAILER_LEN } else { 0 } as u64;
            self.records += 1;
            self.bytes += len.saturating_sub(REC_HDR_LEN as u64 + t);
            self.rest = len.saturating_sub(REC_HDR_LEN as u64);
            self.next = lsn + REC_HDR_LEN as u64;
            return Ok(got);
        }
        if lsn != self.next || buf.len() as u64 > self.rest {
            return Err(format!(
                "a read of {} bytes at lsn {lsn} where {} bytes at lsn {} are due",
                buf.len(),
                self.rest,
                self.next
            ));
        }
        let got = self.inner.read_at(lsn, buf)?;
        self.next += got as u64;
        self.rest -= got as u64;
        Ok(got)
    }
}

/// The overlay replayed from the fixture's store (`Overlay::replay` over its extents), checked equal to the tail's own,
/// so the replay's reads are the store's (R-29).
fn replayed<V: StoreFs + SealedMaps>(p: &Prepared<'_, V>, t: &Tail) -> Result<Overlay, String> {
    let o = Overlay::replay(
        &mut VfsLog::new(p.vfs, &p.root, t.expect.geo),
        &t.expect,
        t.start,
        t.end,
        t.chain_in,
        t.base,
        TAIL_READ_CHUNK,
    )
    .map_err(|e| e.to_string())?;
    if o != t.overlay()? {
        return Err("the overlay replayed from the store differs from the tail's".into());
    }
    Ok(o)
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
        seed: seed(&seed_name(q)),
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
            let ids = read_cycle(&o, p.seed);
            let mut log = VfsLog::new(p.vfs, &p.root, t.expect.geo);
            arms.push(Box::new(TimedFn::new("read", ticker.clone(), move || {
                i += 1;
                point_read(&mut log, &o, c, ids[i % ids.len()]).map_err(ArmError)
            })));
        }
        Family::ReadOp => {
            let t = tail()?;
            let geo = t.expect.geo;
            let o = replayed(p, t)?;
            let ids = read_cycle(&o, p.seed);
            let (mut log0, mut log1) = (
                VfsLog::new(p.vfs, &p.root, geo),
                VfsLog::new(p.vfs, &p.root, geo),
            );
            let (mut buf0, mut buf1) = (
                Vec::with_capacity(TAIL_READ_CHUNK),
                Vec::with_capacity(TAIL_READ_CHUNK),
            );
            let idx = OpIndex::build(&o, &mut log1, TAIL_READ_CHUNK, &mut buf1)?;
            read_ops_agree(
                &o,
                &idx,
                c,
                &ids,
                &mut VfsLog::new(p.vfs, &p.root, geo),
                (&mut log0, &mut buf0),
                (&mut log1, &mut buf1),
            )?;
            let (o1, ids1, mut j) = (o.clone(), ids.clone(), 0usize);
            arms.push(Box::new(TimedFn::new(
                "read.op",
                ticker.clone(),
                move || {
                    i += 1;
                    point_read_op(&mut log0, &o, c, ids[i % ids.len()], &mut buf0).map_err(ArmError)
                },
            )));
            arms.push(Box::new(TimedFn::new(
                "read.op.at",
                ticker.clone(),
                move || {
                    j += 1;
                    point_read_op_at(&mut log1, &o1, &idx, c, ids1[j % ids1.len()], &mut buf1)
                        .map_err(ArmError)
                },
            )));
        }
        Family::ReadCostRecords | Family::ReadCostBytes => {
            let t = tail()?;
            let o = replayed(p, t)?;
            let ids = read_cycle(&o, p.seed);
            let mut log = Counted::new(VfsLog::new(p.vfs, &p.root, t.expect.geo));
            let (arm, unit, bytes) = match q.family {
                Family::ReadCostBytes => ("bytes", Unit::Bytes, true),
                _ => ("records", Unit::Count, false),
            };
            arms.push(Box::new(ValueFn::new(arm, unit, move || {
                i += 1;
                log.reset();
                point_read(&mut log, &o, c, ids[i % ids.len()]).map_err(ArmError)?;
                Ok(if bytes { log.bytes() } else { log.records() })
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
pub const USAGE: &str =
    "usage: m14 --host laptop|hosted --condition idle|loaded|synthetic [--load-log <file>]
           [--quantity <name>] --commit <sha> --toolchain <text> --out <dir>
       m14 --smoke [--quantity <name>] --commit <sha> --toolchain <text>
       m14 --help
Every fixture lies below the working directory.
";

/// The fixtures' parent: the working directory ([MP §9.7] "The working directory"; R-34 B-1).
fn work_dir() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|e| format!("the working directory: {e}"))
}

/// The page [`prefault`] steps by.
pub const PAGE: usize = 4_096;

/// Reads the first byte of every [`PAGE`]-byte page of `bytes`, through [`std::hint::black_box`], so the pages of a
/// mapped section are resident before the pilot (R-34 M-5); returns the pages read and the sum of the bytes read.
pub fn prefault(bytes: &[u8]) -> (usize, u64) {
    let (mut pages, mut sum) = (0usize, 0u64);
    for page in bytes.chunks(PAGE) {
        sum += u64::from(std::hint::black_box(page[0]));
        pages += 1;
    }
    std::hint::black_box((pages, sum))
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
        let run = || -> Result<Vec<(String, u64, Unit)>, String> {
            let p = prepare(vfs, scratch, q, Profile::Test)?;
            let c = Columns::parse(p.map.bytes())?;
            let mut arms = arms(&p, &c, q, ticker)?;
            let mut lines = Vec::new();
            for arm in arms.iter_mut() {
                arm.begin_block(1)
                    .and_then(|()| arm.sample())
                    .map(|v| lines.push((arm.name().to_string(), v, arm.unit())))
                    .map_err(|e| format!("{}: {e}", arm.name()))?;
            }
            Ok(lines)
        };
        match run() {
            Ok(lines) => {
                for (arm, v, unit) in lines {
                    let _ = writeln!(out, "m14 smoke {} {arm} ok {v} {}", q.name, unit.as_str());
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
    let condition = match driver::condition(a) {
        Ok(c) => c,
        Err((code, e)) => {
            let _ = writeln!(err, "m14: {e}");
            return code;
        }
    };
    let scratch = match work_dir() {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(err, "m14 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
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
    prefault(p.map.bytes());
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
        toolchain: a.toolchain.clone().unwrap_or_default(),
        commit: a.commit.clone().unwrap_or_default(),
    };
    let t0 = driver::now_ms();
    let mut refs: Vec<&mut dyn Arm> = arms.iter_mut().map(|b| &mut **b as &mut dyn Arm).collect();
    let mut rec = match measure(&mut ctx, &spec, &mut refs) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "m14 {}: {e}", q.name);
            return EXIT_FAILED;
        }
    };
    let t1 = driver::now_ms();
    driver::set_load_verdict(a, &mut rec, t0, t1, err, &format!("m14 {}", q.name));
    let dir = a.out.clone().unwrap_or_default();
    match driver::write_record(&rec, &dir) {
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

/// The driver's command line ([MP §9.7] through [`crate::m1::driver`]): parses `args` (without the program name),
/// then runs the smoke, one quantity, or every quantity in a child process of its own; returns the exit code.
pub fn cli<V: StoreFs + SealedMaps, M: Meter, T: Ticker + Clone, R: CommandRunner>(
    args: &[OsString],
    sys: System<'_, V, M, T, R>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let g = grid();
    let names: Vec<&str> = g.iter().map(|q| q.name.as_str()).collect();
    let a: Args = match driver::parse(args, &names) {
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
    let asked = a
        .quantity
        .as_ref()
        .and_then(|n| g.iter().find(|q| &q.name == n));
    if a.smoke {
        let scratch = match work_dir() {
            Ok(d) => d,
            Err(e) => {
                let _ = writeln!(err, "m14 smoke: {e}");
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
    driver::each_in_own_process(args, &names, sys.spawn, out, err, "m14")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm::{MonoTicker, valid_name};
    use crate::condition::Condition;
    use crate::host::tests::Canned;
    use crate::m1::driver;
    use crate::overlay::op::{FRAME_HEAD_MAX, unpack};
    use crate::record::RunRecord;
    use crate::seg::bitset::Container;
    use crate::seg::delta::decode;
    use crate::tail::commit::{Commit, Op};
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

    /// `--commit` and `--toolchain`, which every mode but `--help` needs.
    const CT: [&str; 4] = ["--commit", "0123abcd", "--toolchain", "1.98.1"];

    /// The working directory, the only directory a driver test's store holds: every fixture must lie below it.
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

    /// 18 quantities with valid, unique names ([MP §4.1]): the six layout families at 1e5 and 1e6 (the sparse
    /// AND/popcount pair of R-30 among them), the read, its three diagnostics and the catch-up at 1e6, the checkpoint
    /// at 0.5 M; 22 arms; the minimum tiers of the budgets (t1 read ≤ 50 µs and its diagnostics, t3 checkpoint ≤
    /// 100 ms, t2 catch-up ≤ 5 ms, `ARCHITECTURE-RESEARCH.md:130`, `:140`), none for the layout families and no tail
    /// for the sparse pair; the diagnostics' arms (R-42 Q2) and their tails; the read cost's two counts as two
    /// quantities of one arm each, since every arm after arm 0 is a floor (R-44 I-1 (b)).
    #[test]
    fn grid_has_eighteen_valid_quantities() {
        let g = grid();
        let names: Vec<&str> = g.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "read-4096.n1e6",
                "ckpt-4096.n5e5",
                "catchup-16.n1e6",
                "read-op-4096.n1e6",
                "read-cost-records-4096.n1e6",
                "read-cost-bytes-4096.n1e6",
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
                "overlay.n1e6"
            ],
            "the deciding quantities first (R-34 B-3, M-15)"
        );
        assert!(names.iter().all(|n| valid_name(n)));
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 18);
        let arms: Vec<&str> = g
            .iter()
            .flat_map(|q| q.family.arms().iter().copied())
            .collect();
        assert_eq!(arms.len(), 22);
        assert_eq!(Family::ReadOp.arms(), ["read.op", "read.op.at"]);
        assert_eq!(Family::ReadCostRecords.arms(), ["records"]);
        assert_eq!(Family::ReadCostBytes.arms(), ["bytes"]);
        for f in [
            Family::ReadOp,
            Family::ReadCostRecords,
            Family::ReadCostBytes,
        ] {
            assert_eq!((f.min_tier(), f.needs_tail()), (Some(Tier::T1), true));
        }
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
        assert_eq!(g[1].family, Family::Ckpt);
        let t = target(&g[1], Profile::Production);
        assert_eq!((t.clause, t.threshold, t.n), (Clause::Ops, 4_096, 500_000));
        assert_eq!(target(&g[1], Profile::Test).n, 5_000);
    }

    /// `--smoke` runs every quantity at the test profile with each arm once: 22 lines, one per (quantity, arm), each
    /// ending with the arm's unit; with `--quantity` it runs that one alone.
    #[test]
    fn smoke_exercises_every_arm_once() {
        let fs = MemFs::with_dir(&cwd());
        let one = [&["--smoke", "--quantity", "csr.n1e5"][..], &CT].concat();
        let (code, out, err) = run(&fs, &one);
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1, "{out}");
        assert!(
            lines[0].starts_with("m14 smoke csr.n1e5 expand ok "),
            "{out}"
        );
        fs.clear_log();
        let (code, out, err) = run(&fs, &[&["--smoke"][..], &CT].concat());
        assert_eq!(code, EXIT_DONE, "{err}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 22, "{out}");
        let pairs: BTreeSet<(String, String)> = lines
            .iter()
            .map(|l| {
                let w: Vec<&str> = l.split(' ').collect();
                assert_eq!((w[0], w[1], w[4]), ("m14", "smoke", "ok"), "{l}");
                let unit = match w[3] {
                    "records" => "count",
                    "bytes" => "bytes",
                    _ => "ns",
                };
                assert_eq!((w.len(), w[6]), (7, unit), "{l}");
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
        assert_eq!(fs.log().maps.len(), 18, "each run maps its base once");
    }

    /// `--smoke` with the runner's contract flags records nothing: no line says recorded, every fixture is removed,
    /// the checkpoint's files included, and `--smoke` with `--out` is refused.
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
        assert_eq!(out.lines().count(), 22);
        assert!(!out.contains("recorded"));
        assert!(
            fs.files().is_empty(),
            "every fixture removed: {:?}",
            fs.files()
        );
        assert!(!fs.has_dir(&cwd().join(format!("moirai-m14-{}-csr.n1e5", std::process::id()))));
        let (code, _, err) = run(&fs, &[&args[..], &["--out", "/o"]].concat());
        assert_eq!(code, EXIT_USAGE);
        assert!(err.contains("takes no --out"), "{err}");
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

    /// The read and its three diagnostics share their tail and their nodes (spec Goal 1): at both profiles their
    /// targets are equal; at the test profile `prepare`'s four fixtures hold byte-identical tails, the same seed and the
    /// same cycle of ids; at production `generate` gives four byte-identical tails of 4,096 ops (a production fixture
    /// would also build a 1e6-node base). Every other quantity keeps the seed of its own name.
    #[test]
    fn read_quantities_share_the_reads_tail_and_nodes() {
        let g = grid();
        let of = |f: Family| g.iter().find(|q| q.family == f).unwrap().clone();
        let qs = [
            of(Family::Read),
            of(Family::ReadOp),
            of(Family::ReadCostRecords),
            of(Family::ReadCostBytes),
        ];
        for profile in [Profile::Production, Profile::Test] {
            let t = target(&qs[0], profile);
            for q in &qs[1..] {
                assert_eq!(
                    target(q, profile),
                    t,
                    "the targets of {} and {} differ at {profile:?}",
                    q.name,
                    qs[0].name
                );
            }
        }
        let fs = MemFs::with_dir(&scratch());
        let ps: Vec<Prepared<'_, MemFs>> = qs
            .iter()
            .map(|q| prepare(&fs, &scratch(), q, Profile::Test).unwrap())
            .collect();
        let tail = |p: &Prepared<'_, MemFs>| p.tail.as_ref().unwrap().bytes.clone();
        let cycle = |p: &Prepared<'_, MemFs>| {
            read_cycle(&p.tail.as_ref().unwrap().overlay().unwrap(), p.seed)
        };
        assert!(cycle(&ps[0]).len() > 1);
        for (q, p) in qs.iter().zip(&ps).skip(1) {
            assert_eq!(tail(p), tail(&ps[0]), "{}'s test-profile tail", q.name);
            assert_eq!(p.seed, ps[0].seed, "{}'s seed", q.name);
            assert_eq!(cycle(p), cycle(&ps[0]), "{}'s cycle", q.name);
        }
        let tails: Vec<Tail> = qs
            .iter()
            .map(|q| generate(&target(q, Profile::Production), false).unwrap())
            .collect();
        assert_eq!(tails[0].measures.ops, 4_096);
        for (q, t) in qs.iter().zip(&tails).skip(1) {
            assert!(t.bytes == tails[0].bytes, "{}'s production tail", q.name);
        }
        for q in g.iter().filter(|q| !qs.contains(q)) {
            assert_eq!(seed_name(q), q.name);
            assert_eq!(
                target(q, Profile::Production).seed,
                seed(&q.name),
                "{}",
                q.name
            );
        }
        assert_eq!(target(&qs[0], Profile::Production).seed, seed(&qs[0].name));
    }

    /// The status each touched node's `Create` or `SetStatus` last set, by the decoded records.
    fn naive_status(t: &Tail) -> BTreeMap<u32, u8> {
        let mut naive = BTreeMap::new();
        for r in &t.records {
            let Payload::Commit(cm) = &r.payload else {
                continue;
            };
            for op in &cm.ops {
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
        naive
    }

    /// The read diagnostics return the whole-record read's value (spec Goal 1): for seeds 1–3, as
    /// `t1_read_returns_the_tails_value`, [`point_read_op`] and [`point_read_op_at`] give every touched node the status
    /// the naive map gives, which [`point_read`] gives too, and an untouched node its base row; so do they on every id
    /// of the `read-op` fixture's cycle.
    #[test]
    fn read_op_returns_the_whole_record_reads_value() {
        let mut buf = Vec::new();
        for seed in 1..=3 {
            let (t, b) = tail_and_base(300, 400, seed);
            let c = Columns::parse(&b).unwrap();
            let o = t.overlay().unwrap();
            let idx = OpIndex::build(
                &o,
                &mut MemLog::new(t.start, &t.bytes),
                TAIL_READ_CHUNK,
                &mut buf,
            )
            .unwrap();
            let naive = naive_status(&t);
            let mut log = MemLog::new(t.start, &t.bytes);
            let mut from_base = 0;
            for e in o.nodes() {
                let want = naive.get(&e.n).copied().unwrap_or_else(|| {
                    from_base += 1;
                    (e.n % 4) as u8
                });
                assert_eq!(
                    point_read(&mut log, &o, &c, e.n),
                    Ok(want),
                    "seed {seed} #{}",
                    e.n
                );
                assert_eq!(
                    point_read_op(&mut log, &o, &c, e.n, &mut buf),
                    Ok(want),
                    "seed {seed} #{}",
                    e.n
                );
                assert_eq!(
                    point_read_op_at(&mut log, &o, &idx, &c, e.n, &mut buf),
                    Ok(want),
                    "seed {seed} #{}",
                    e.n
                );
            }
            assert!(naive.len() > 50 && from_base > 20, "both paths");
            let untouched = (1..=300).find(|id| !probe(&o, *id)).unwrap();
            assert_eq!(
                point_read_op_at(&mut log, &o, &idx, &c, untouched, &mut buf),
                Ok((untouched % 4) as u8)
            );
        }
        let fs = MemFs::with_dir(&scratch());
        let (_, p) = fixture(&fs, Family::ReadOp);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let t = p.tail.as_ref().unwrap();
        let o = t.overlay().unwrap();
        let idx = OpIndex::build(
            &o,
            &mut MemLog::new(t.start, &t.bytes),
            TAIL_READ_CHUNK,
            &mut buf,
        )
        .unwrap();
        let mut log = MemLog::new(t.start, &t.bytes);
        for id in read_cycle(&o, p.seed) {
            let want = point_read(&mut log, &o, &c, id);
            assert!(want.is_ok(), "#{id}: {want:?}");
            assert_eq!(point_read_op(&mut log, &o, &c, id, &mut buf), want, "#{id}");
            assert_eq!(
                point_read_op_at(&mut log, &o, &idx, &c, id, &mut buf),
                want,
                "#{id}"
            );
        }
    }

    /// `reads` (lsn, length) of a `MemLog` over `geo`'s extents, as the store logs them: (file, offset, length).
    fn as_store_reads(geo: Geometry, reads: &[(u64, usize)]) -> Vec<Read> {
        reads
            .iter()
            .map(|&(lsn, len)| (extent_name(geo.extent(lsn)), geo.offset(lsn), len))
            .collect()
    }

    /// One cycle of read-op's arm `arm` over a [`LoggedFs`] and an [`FsTicker`] (R-29). Before the first tick the store
    /// has read the whole tail (the setup's replay, index build and pre-pass) and opened the extent. Each sample's reads
    /// between its ticks are not empty and are exactly `reference`'s reads for `ids[(k + 1) mod len]` on a `MemLog`; no
    /// open falls between the ticks. Returns each sample's first read as an lsn.
    fn read_op_cycle(
        arm: usize,
        reference: &dyn Fn(&mut MemLog<'_>, &Overlay, &Columns<'_>, u32),
    ) -> (Vec<u64>, Overlay, Tail) {
        let fs = MemFs::with_dir(&scratch());
        let lfs = LoggedFs::new(&fs);
        let (q, p) = fixture(&lfs, Family::ReadOp);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let t = p.tail.as_ref().unwrap();
        let geo = t.expect.geo;
        let o = t.overlay().unwrap();
        let ids = read_cycle(&o, p.seed);
        assert!(ids.len() > 1, "{} ids", ids.len());
        let ticker = FsTicker::new(&fs, p.dir(), &[]).logging(&lfs);
        let (reads0, calls0) = (fs.log().reads.len(), lfs.calls().len());
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        let mut firsts = Vec::new();
        for k in 0..ids.len() {
            arms[arm].sample().unwrap();
            let (t0, t1) = ticker.last();
            if k == 0 {
                let before = &fs.log().reads[reads0..t0.reads];
                assert_eq!(
                    lsn_cover(geo, before),
                    [(t.start, t.end)],
                    "the setup read the whole tail before the first tick"
                );
                assert!(
                    lfs.calls()[calls0..t0.calls]
                        .iter()
                        .any(|c| c.0.starts_with("log.") && c.2 == Io::Open(false)),
                    "the setup opened the extent"
                );
            }
            let w = ticker.timed_reads();
            assert!(!w.is_empty(), "sample {k}: no read between its ticks");
            let id = ids[(k + 1) % ids.len()];
            let mut mem = MemLog::new(t.start, &t.bytes);
            reference(&mut mem, &o, &c, id);
            assert_eq!(w, as_store_reads(geo, &mem.reads), "sample {k}: #{id}");
            let opens: Vec<Call> = lfs.calls()[t0.calls..t1.calls]
                .iter()
                .filter(|c| matches!(c.2, Io::Open(_)))
                .cloned()
                .collect();
            assert!(
                opens.is_empty(),
                "sample {k}: an open between the ticks: {opens:?}"
            );
            let (name, off, _) = &w[0];
            let n: u64 = name.strip_prefix("log.").unwrap().parse().unwrap();
            firsts.push(geo.extent_start(n) + off);
        }
        assert_eq!(ticker.count(), 2 * ids.len(), "two ticks per sample");
        (firsts, o, t.clone())
    }

    /// `read.op` times one frame-walk read per sample (R-29, spec gate 1): over one cycle its reads between the ticks are
    /// exactly [`point_read_op`]'s for the cycle's next node, the setup's reads come before the first tick, no open falls
    /// between the ticks, and the first reads' lsns are the node index's, as a multiset.
    #[test]
    fn read_op_arm_times_one_located_read_per_sample() {
        let (mut firsts, o, _) = read_op_cycle(0, &|mem, o, c, id| {
            point_read_op(mem, o, c, id, &mut Vec::new()).unwrap();
        });
        let mut want: Vec<u64> = o.nodes().iter().map(|e| e.lsn).collect();
        want.sort_unstable();
        firsts.sort_unstable();
        assert_eq!(
            firsts, want,
            "the cycle's first reads are the node index's lsns"
        );
    }

    /// `read.op.at` reads the newest frames directly (R-29, spec gate 1, R-42 Q2): over one cycle its reads between the
    /// ticks are exactly [`point_read_op_at`]'s, over an offset index built on a `MemLog`, for the cycle's next node; each
    /// first read is [`FRAME_HEAD_MAX`] bytes at the entry's `lsn + off`, never a header at the entry's lsn; and the
    /// first reads' lsns are the index's `lsn + off`, as a multiset.
    #[test]
    fn read_op_floor_reads_the_newest_frames_directly() {
        let (mut firsts, o, t) = read_op_cycle(1, &|mem, o, c, id| {
            let mut buf = Vec::new();
            let idx = OpIndex::build(o, &mut mem.clone(), TAIL_READ_CHUNK, &mut buf).unwrap();
            point_read_op_at(mem, o, &idx, c, id, &mut buf).unwrap();
            assert_eq!(mem.reads[0].1, FRAME_HEAD_MAX, "#{id}");
        });
        let mut buf = Vec::new();
        let idx = OpIndex::build(
            &o,
            &mut MemLog::new(t.start, &t.bytes),
            TAIL_READ_CHUNK,
            &mut buf,
        )
        .unwrap();
        let mut want: Vec<u64> = idx
            .entries()
            .iter()
            .map(|e| e.lsn + unpack(e.at).0)
            .collect();
        assert!(
            idx.entries()
                .iter()
                .all(|e| unpack(e.at).0 > REC_HDR_LEN as u64)
        );
        want.sort_unstable();
        firsts.sort_unstable();
        assert_eq!(
            firsts, want,
            "the cycle's first reads are the index's lsn + off"
        );
    }

    /// The setup refuses a disagreeing read (spec Goal 1): [`read_ops_agree`] is `Ok` over three sources of one tail,
    /// and `Err`, naming the id, when one changed `SetStatus` byte (the new status of a node's `SetStatus` in its newest
    /// record) is in the offset read's source alone, and again when it is in the frame walk's source alone. The two
    /// reads differ there: the frame walk decodes the window `locate` held, the offset read re-reads at the index's
    /// offset, so each comparison is pinned on its own.
    #[test]
    fn read_op_setup_refuses_a_disagreeing_read() {
        let (t, b) = tail_and_base(300, 400, 1);
        let c = Columns::parse(&b).unwrap();
        let o = t.overlay().unwrap();
        let (mut b0, mut b1) = (Vec::new(), Vec::new());
        let idx = OpIndex::build(
            &o,
            &mut MemLog::new(t.start, &t.bytes),
            TAIL_READ_CHUNK,
            &mut b1,
        )
        .unwrap();
        let ids = read_cycle(&o, 7);
        let clean = || MemLog::new(t.start, &t.bytes);
        assert_eq!(
            read_ops_agree(
                &o,
                &idx,
                &c,
                &ids,
                &mut clean(),
                (&mut clean(), &mut b0),
                (&mut clean(), &mut b1)
            ),
            Ok(())
        );
        // A SetStatus in its owner's newest record, found by its encoded frame in the record's bytes.
        let newest: BTreeMap<u32, u64> = o.nodes().iter().map(|e| (e.n, e.lsn)).collect();
        let (n, at, new) = t
            .records
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(cm) => Some((r, &**cm)),
                _ => None,
            })
            .find_map(|(r, cm): (_, &Commit<'_>)| {
                cm.ops.iter().find_map(|op| match op {
                    Op::SetStatus { id, old, new, .. } if newest[id] == r.lsn => {
                        let mut frame = Vec::new();
                        op.encode(&mut frame, &mut Vec::new()).unwrap();
                        let from = (r.lsn - t.start) as usize;
                        let pos = t.bytes[from..]
                            .windows(frame.len())
                            .position(|w| w == frame.as_slice())?;
                        let x = if ((new.0 + 1) % 4, new.1) == *old {
                            (new.0 + 2) % 4
                        } else {
                            (new.0 + 1) % 4
                        };
                        Some((*id, from + pos + frame.len() - 2, x))
                    }
                    _ => None,
                })
            })
            .expect("a SetStatus in its owner's newest record");
        let mut bad = t.bytes.clone();
        bad[at] = new;
        // The changed byte in the offset read's source, then in the frame walk's.
        for (side, op_src, at_src) in [
            ("the offset read's", &t.bytes, &bad),
            ("the frame walk's", &bad, &t.bytes),
        ] {
            let r = read_ops_agree(
                &o,
                &idx,
                &c,
                &ids,
                &mut clean(),
                (&mut MemLog::new(t.start, op_src), &mut b0),
                (&mut MemLog::new(t.start, at_src), &mut b1),
            );
            assert!(
                r.as_ref().is_err_and(|e| e.starts_with(&format!("#{n}:"))),
                "the changed byte in {side} source: {r:?}"
            );
        }
    }

    /// The naive cost of T1's whole-record read of `#n`: from the decoded records, the owner's newest record, then the
    /// records its `prev` names until one sets a status or the chain leaves the tail; the records walked and the Σ of
    /// their `len − 32 − t`.
    fn naive_cost(t: &Tail, o: &Overlay, n: u32) -> (u64, u64) {
        let recs: BTreeMap<u64, (u64, &Commit<'_>)> = t
            .records
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(cm) => {
                    let tl = if r.group_end { TRAILER_LEN } else { 0 } as u64;
                    let len = r.encoded_len().unwrap();
                    Some((r.lsn, (len - REC_HDR_LEN as u64 - tl, &**cm)))
                }
                _ => None,
            })
            .collect();
        let Ok(i) = o.nodes().binary_search_by_key(&n, |e| e.n) else {
            return (0, 0);
        };
        let (mut lsn, mut records, mut bytes) = (o.nodes()[i].lsn, 0, 0);
        loop {
            let (b, cm) = recs[&lsn];
            records += 1;
            bytes += b;
            let own: Vec<&Op<'_>> = cm.ops.iter().filter(|op| op.owner() == n).collect();
            let sets_status = own.iter().any(|op| match op {
                Op::SetStatus { .. } => true,
                Op::Create { image, .. } => image.status.is_some(),
                _ => false,
            });
            let prev = own[0].prev();
            if sets_status || prev == 0 || prev > lsn || lsn - prev < t.start {
                return (records, bytes);
            }
            lsn -= prev;
        }
    }

    /// [`Counted`] counts what the whole-record read walks and decodes ([MP §4.8]): for every touched node of seeds 1–3,
    /// one [`point_read`] through it gives the naive walk's record count and Σ `len − 32 − t`, and some reads walk
    /// more than one record; a read that is not a header where one is due is an error.
    #[test]
    fn read_cost_counts_equal_a_naive_walk() {
        for seed in 1..=3 {
            let (t, b) = tail_and_base(300, 400, seed);
            let c = Columns::parse(&b).unwrap();
            let o = t.overlay().unwrap();
            let mut more = 0;
            let mut cnt = Counted::new(MemLog::new(t.start, &t.bytes));
            for e in o.nodes() {
                cnt.reset();
                point_read(&mut cnt, &o, &c, e.n).unwrap();
                let want = naive_cost(&t, &o, e.n);
                assert_eq!(
                    (cnt.records(), cnt.bytes()),
                    want,
                    "seed {seed} #{}: (records, bytes)",
                    e.n
                );
                more += usize::from(want.0 > 1);
            }
            assert!(
                more > 10,
                "seed {seed}: {more} reads walk more than one record"
            );
            cnt.reset();
            assert!(cnt.read_at(t.start, &mut [0u8; 16]).is_err());
        }
    }

    /// [`Counted`] refuses a continuation read off `get`'s protocol, so a drift fails the run instead of miscounting
    /// (a read past `rest` would wrap `rest` in a release build): after a header read, a read 1 byte past the next lsn
    /// and a read of `rest + 1` bytes at it are errors and change nothing; the record's `rest` bytes at the next lsn
    /// are then read, and the next record's header counts a second record.
    #[test]
    fn read_cost_refuses_an_off_protocol_continuation_read() {
        let (t, _) = tail_and_base(300, 400, 1);
        let mut cnt = Counted::new(MemLog::new(t.start, &t.bytes));
        let mut h = [0u8; REC_HDR_LEN];
        assert_eq!(cnt.read_at(t.start, &mut h), Ok(REC_HDR_LEN));
        let (next, rest) = (t.start + REC_HDR_LEN as u64, cnt.rest);
        assert!(rest > 4, "the first record's rest: {rest}");
        let due = |len: u64, lsn: u64| {
            Err(format!(
                "a read of {len} bytes at lsn {lsn} where {rest} bytes at lsn {next} are due"
            ))
        };
        assert_eq!(cnt.read_at(next + 1, &mut [0u8; 4]), due(4, next + 1));
        let mut long = vec![0u8; rest as usize + 1];
        assert_eq!(cnt.read_at(next, &mut long), due(rest + 1, next));
        let tl = u64::from(RecHdr::decode(&h).group_end()) * TRAILER_LEN as u64;
        assert_eq!((cnt.records(), cnt.bytes()), (1, rest - tl));
        long.truncate(rest as usize);
        assert_eq!(cnt.read_at(next, &mut long), Ok(rest as usize));
        assert_eq!(cnt.read_at(next + rest, &mut h), Ok(REC_HDR_LEN));
        assert_eq!(cnt.records(), 2);
    }

    /// The read cost's two quantities (R-44 I-1 (b)) each have one arm, `read-cost-records`'s `records` in
    /// [`Unit::Count`] and `read-cost-bytes`'s `bytes` in [`Unit::Bytes`], and count the read of the read's nodes: over
    /// one cycle, sample k of each is the naive cost of `ids[(k + 1) mod len]` (the records, the bytes), and the byte
    /// counts differ between nodes, so the cycle read in another order would not pass.
    #[test]
    fn read_cost_arms_sample_the_reads_nodes() {
        let fs = MemFs::with_dir(&scratch());
        let (qr, pr) = fixture(&fs, Family::ReadCostRecords);
        let (qb, pb) = fixture(&fs, Family::ReadCostBytes);
        let (cr, cb) = (
            Columns::parse(pr.map.bytes()).unwrap(),
            Columns::parse(pb.map.bytes()).unwrap(),
        );
        let t = pr.tail.as_ref().unwrap();
        let o = t.overlay().unwrap();
        let ids = read_cycle(&o, pr.seed);
        let mut records = arms(&pr, &cr, &qr, &MonoTicker::new()).unwrap();
        let mut bytes = arms(&pb, &cb, &qb, &MonoTicker::new()).unwrap();
        assert_eq!(
            (records.len(), records[0].name(), records[0].unit()),
            (1, "records", Unit::Count)
        );
        assert_eq!(
            (bytes.len(), bytes[0].name(), bytes[0].unit()),
            (1, "bytes", Unit::Bytes)
        );
        let mut seen = BTreeSet::new();
        for k in 0..ids.len() {
            let id = ids[(k + 1) % ids.len()];
            let (r, b) = naive_cost(t, &o, id);
            assert_eq!(records[0].sample(), Ok(r), "sample {k}: #{id}'s records");
            assert_eq!(bytes[0].sample(), Ok(b), "sample {k}: #{id}'s bytes");
            seen.insert(b);
        }
        assert!(
            ids.len() > 1 && seen.len() > 1,
            "{} ids, {} distinct byte counts",
            ids.len(),
            seen.len()
        );
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

    /// A `--quantity` run writes exactly one record directly in `--out`, which is created when missing, that reads
    /// back: measurement 14, its arm, t1's 10,000 × 5 plan (the probe is fast), the given commit and toolchain; its
    /// fixture is removed.
    #[test]
    fn run_writes_one_valid_record() {
        let root = scratch_dir("m14-run");
        let out_dir = root.join("measurements").join("14");
        let o = out_dir.to_str().unwrap().to_string();
        let fs = MemFs::with_dir(&cwd());
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
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Usage errors exit 2 with the reason, by the shared contract ([MP §9.7]): `--commit` and `--toolchain` required,
    /// a recording run without `--out`, `--host` or `--condition`, a loaded or synthetic run without `--load-log`,
    /// `--smoke` with `--out`, `--scratch` (every fixture lies below the working directory), an unknown flag, host,
    /// condition or quantity, a flag without its value. `--help` exits 0, and the §9.7 contract alone is a valid call.
    #[test]
    fn cli_refuses_usage_errors() {
        let fs = MemFs::with_dir(&cwd());
        let contract = ["--host", "laptop", "--condition", "idle", "--out", "/o"];
        let with = |more: &[&'static str]| -> Vec<&'static str> { [&CT[..], more].concat() };
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (contract.to_vec(), "--commit and --toolchain"),
            (
                [&contract[..], &["--toolchain", "t"]].concat(),
                "--commit and --toolchain",
            ),
            (with(&["--bogus"]), "unknown argument '--bogus'"),
            (
                with(&["--quantity", "csr.n1e7", "--smoke"]),
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
            (with(&["--out"]), "needs a value"),
        ];
        for (args, why) in cases {
            let (code, _, err) = run(&fs, &args);
            assert_eq!(code, EXIT_USAGE, "{args:?}");
            assert!(err.contains(why), "{args:?}: {err}");
            assert!(err.contains("usage: m14"));
        }
        let (code, out, _) = run(&fs, &["--help"]);
        assert_eq!((code, out.as_str()), (EXIT_DONE, USAGE));
        let names: Vec<String> = grid().into_iter().map(|q| q.name).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        assert!(driver::parse(&os(&[&contract[..], &CT].concat()), &refs).is_ok());
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
    /// directory"; R-34 B-1): over a store that holds only the working directory, the smoke makes, maps and removes
    /// its fixture, which it could not do anywhere else; over a store without it, the smoke fails.
    #[test]
    fn fixtures_stay_below_the_working_directory() {
        let args = [&["--smoke", "--quantity", "csr.n1e5"][..], &CT].concat();
        let fs = MemFs::with_dir(&cwd());
        let (code, out, err) = run(&fs, &args);
        assert_eq!(code, EXIT_DONE, "{err}");
        assert_eq!(out.lines().count(), 1);
        assert_eq!(fs.log().maps.len(), 1, "the base was mapped");
        assert!(fs.files().is_empty(), "and removed");
        let elsewhere = MemFs::with_dir(&std::env::temp_dir());
        let (code, _, err) = run(&elsewhere, &args);
        assert_eq!(
            code, EXIT_FAILED,
            "a store without the working directory: {err}"
        );
    }

    /// `prefault` reads one byte of every 4,096-byte page (R-34 M-5): over slices whose pages each start with 1 and
    /// hold 0 elsewhere, it reads ⌈len / 4,096⌉ pages and sums that many ones.
    #[test]
    fn prefault_reads_one_byte_of_every_page() {
        for len in [0, 1, 4_096, 4_097, 3 * 4_096 + 5] {
            let b: Vec<u8> = (0..len).map(|i| u8::from(i % PAGE == 0)).collect();
            let pages = len.div_ceil(PAGE);
            assert_eq!(prefault(&b), (pages, pages as u64), "len {len}");
        }
    }
}
