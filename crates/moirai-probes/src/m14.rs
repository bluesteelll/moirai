//! Measurement 14 (WP-53e, [60 §5.2] item 14, `60-roadmap.md:770`; `docs/m0/PLAN.md:317`): the layout probes over
//! the F09 segment of [`crate::seg`], at 1e5 and 1e6 nodes, and the T1 and T2 trigger quantities: a point read after
//! a full 4,096-op tail and a delta checkpoint at 0.5 M nodes (T1, `ARCHITECTURE-RESEARCH.md:130`), and the MCP
//! overlay catch-up (T2, `:140`), and T1's full delta checkpoint at 0.5 M and 1e6 nodes (M-A4, R-53 Q2). T2's 16-writer
//! wait is taken from measurement 2's run (R-24 Q7) and is not built here. The tails are u10's
//! ([`crate::tail::synth`]), from [70 S8]'s spec-derived mix (R-24 Q10).
//!
//! **The grid** (25 quantities, names by [MP §4.1]). The night's 18 run with the deciding quantities first:
//! `read-4096.n1e6`, `ckpt-4096.n5e5` and `catchup-16.n1e6`, then T1's read diagnostics `read-op-4096.n1e6`,
//! `read-cost-records-4096.n1e6` and `read-cost-bytes-4096.n1e6` (R-40 O-2, R-42, R-44), then the layout probes (R-34
//! B-3, M-15; R-35 Q2). u20's blocks follow, appended (u20 cut §3.10): T1's read with item 1a (M-A1, R-50):
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
//! | `read-1a-4096.n1e6`, `read-1a-1024.n1e6` | `read`, `read.op`, `read.op.at`, `read.1a` | T1's read of a node's status in an A1 tail (`design-A.md:104-129`) at P06 4,096, and at 1,024 (design-A A3's draft), from `read-<P06>.n1e6`'s seed, so their plans and read cycle are the whole read's ([`seed_name`]; the locators move lsns, u20 cut F-11). Each arm is a floor of the one before ([MP §4.1]): the whole record ([`Overlay::get`]), the frame walk, u17's offset read over an index built in the setup, and item 1a's read ([`Overlay::get_1a`]: n's newest frames from the offsets [`Overlay::replay_at`] built, then one read per hop at each record's locator). The setup replays the offsets-mode overlay from the fixture's store and refuses the run unless the four agree on every node of the cycle and no locator is refused ([`read_1a_agree`]) | t1 |
//! | `read-1a-d4.n1e6`, `read-1a-d16.n1e6`, `read-1a-d64.n1e6` | the same four | the depth arm: the 4,096-op A1 tail with one base node, #500,000, in each of its first d commits ([`crate::tail::Shape::hot`]), its field 9 set in the first; every sample reads that field, so it walks all d records. The seed is `seed(name) ^ k` with the first k < 64 whose production tail holds at least d commits ([`Family::seed_k`]). Reported, no gate | — |
//!
//! Item 1a's bytes per op (M-A5) are a pinned count, not a time quantity (`a1_bytes_per_op_are_pinned_on_the_read_tails`).
//!
//! u20's probe shaped like `ready` (u20 cut §3.8; OP-09-21) is retired with its 12 quantities: the owner split
//! `priority` into the `PRIO` column (R-66), so its question is decided, and the product's `ready` over `PRIO` is M1's
//! (R-86 ruling 6).
//!
//! **The full delta checkpoint** follows, appended (u21 cut §3.8; M-A4, R-53 Q2): `ckpt-full-4096.n5e5` and
//! `ckpt-full-4096.n1e6`, arm 0 `ckpt.full` and its floor `ckpt.full.write`, minimum tier t3. One sample of arm 0 folds a
//! fresh 4,096-op tail, `ckpt-4096`'s at the same scale ([`seed_name`], [`ckpt_target`]; u21 cut D-2), over the full base
//! ([`crate::seg::base::build_full`]) into every section a delta of its window carries ([`crate::seg::full`]), read
//! from the store; encodes it, compressing the window's bodies into `blobs.15` (`lz4`, or `none` when lz4 is not
//! shorter, R-40 O-1); seals `blobs.15` and `seg.d14` ([`seal_file`]: `create_new`, `write_at`, `sync(DataAndMeta)`,
//! `seal`) and flushes the directory once; appends the `Checkpoint` group ([`full_checkpoint`]) at the tail's end and
//! flushes the extent data-only; publishes into the other `HEAD` slot, not flushed ([`crate::tail::head::publish`]); and
//! reads the group's trailer back, the identity check (`ARCHITECTURE-RESEARCH.md:715-737`). The setup, untimed, writes
//! tail j over the extent, zeroes past it, flushes it and restores `HEAD` ([`restored_head`]); the teardown drops the
//! products and unlinks both files. The floor's setup also folds tail j from the store and encodes it, so its op times
//! the same writes, append, publish and identity read: the pair splits a sample into fold and encode against writes and
//! publish. The bounds are M-A4's (`design-A.md:557`; spec Goal 2): ≤ 50 ms at 0.5 M and ≤ 100 ms at 1e6, after
//! `ARCHITECTURE-RESEARCH.md:1709` (10–50 ms at 1e5, 20–100 ms at 1e6). T1's trigger stays `:130`'s > 100 ms at 0.5 M,
//! so a result between 50 and 100 ms at 0.5 M misses M-A4's bound and not T1's (critique m-1).
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
//! `ARCHITECTURE-RESEARCH.md:130`. `ckpt-full-4096` (below) times the rest.
//!
//! **What the full checkpoint leaves out** (u21 cut §3.9; R-53 Q2, R-59 Q2). `ckpt-full-4096.<N>` times every section
//! F09 §3.1 lists for a delta of its window, the window's `blobs` file, the `Checkpoint` group with its data-only
//! flush, the publish into the other `HEAD` slot and the identity read (`ARCHITECTURE-RESEARCH.md:715-737`). The results
//! analyst reads it as a lower bound of the delta checkpoint of `ARCHITECTURE-RESEARCH.md:130` in seven parts, and as an
//! upper bound in one:
//! - lower bound (u20 cut F-4): of the predicate bitsets only `deleted` is maintained; the others need F08's and F13's
//!   membership rules, which the probe does not have;
//! - lower bound (u20 cut F-5): the tokenizer is tokenizer v1 restricted to ASCII, and the bodies, random bytes, are
//!   not tokenized;
//! - lower bound (u20 cut F-7): the checkpoint takes no writer, flush or maintenance byte and re-writes no pending range
//!   `(durable_lsn, E]`, and the publish applies the `Checkpoint`'s effects without a scan;
//! - lower bound (u20 cut F-8): the store-level tables are at the probe's sizes: 50 refs, 4 trees and 13 file rows, and
//!   no pin, head, lease or marker;
//! - lower bound (u21 cut F-12): the bodies are random, so lz4 never wins and every body is stored raw (codec 0). The
//!   time includes only a compress attempt that fails, the lower-bound side; the `blobs` bytes written and flushed are
//!   incompressible bodies' bytes, about 2–3× a text body's compressed size (est.), the other side (critique m-2). The
//!   codec's real ratio is measurement 6's;
//! - lower bound (u21 cut F-16): no arm times one section alone, so a miss splits only into fold and encode (the
//!   floor's setup) against writes and publish (the floor);
//! - lower bound (u21 cut F-17): the base holds no `TERMS` or `POST`. The checkpoint never reads them, and the fixture
//!   is that much smaller than a store's base;
//! - upper bound (u20 cut F-6): `DOCLEN` and `FTSSTAT` are written as `HOLE(F09-doclen)`'s "kept" candidate
//!   (`09-segments.md:1263`).
//!
//! A field-1 value can exceed F08 §7.1's 200-B title limit. It is stored in `TITLE_BLOB` as it is (u21 cut F-13).
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
//! every test, since the rig observes reads only (u17 critique I-2). The full checkpoint's fold
//! is seen through the log reads it makes, its files, group and publish through the store's calls, bytes and seals at
//! the ticks (the directory's flush included, through `LoggedFs`); its in-memory work is not seen: arm 0's encode and
//! compress (op) and the floor's encode (setup) rest on the code alone, as does the drop of the products (teardown).
//!
//! Every run's fixture is the base `seg.base.1` at the quantity's N ([`crate::seg::base`]), written, sealed and
//! mapped, and, for the thirteen families that read one (the overlay probe, the read and its three diagnostics, the
//! checkpoint and the catch-up, item 1a's five reads, and the full checkpoint), a full `ops` tail at P06 (4,096 at
//! production; the full checkpoint's 64 at the test profile, D-3) written as `log.<n>` extents with `HEAD`; the full
//! checkpoint's base is the full base, with titles, fields, uids and store tables ([`crate::seg::base::build_full`],
//! about 330 B per node, est.); it is removed on every path out of the run. The probe's choices: the ids a probe cycles through
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
use crate::overlay::op::{OpIndex, ReadStats};
use crate::overlay::{Builder, Key, Overlay, Probe, Value, kind_key, kind_status_key};
use crate::run::{Context, RunSpec, measure};
use crate::seg::base::{self, Columns};
use crate::seg::bitset::{Frozen, and_popcount, encode_frozen};
use crate::seg::blobs::Packer;
use crate::seg::delta;
use crate::seg::full::{self, Encoded, Folded, FullBase};
use crate::seg::{NODE_HDR_LEN, STATUS_AT, seg_kind as file_family, u32_at};
use crate::tail::ckpt::{self, Checkpoint, FileEntry, RefList, ckflag};
use crate::tail::commit::Op;
use crate::tail::head::{self, HeadSlot, Published, SegRef, VfsSlot};
use crate::tail::rec::{Payload, RecHdr};
use crate::tail::scan::{LogSource, MemLog, scan};
use crate::tail::store::{VfsLog, extent_name};
use crate::tail::synth::{
    BASE_KIND, Clause, HOT_FIELD, STATUSES, Tail, Target, generate, hot_node,
};
use crate::tail::{
    HEAD_LEN, Locator, MIN_GROUP_LEN, Params, REC_HDR_LEN, Shape, TAIL_READ_CHUNK, TRAILER_LEN,
};
use crate::tier::Tier;
use crate::units::Unit;
use moirai_vfs::{
    Access, Meter, OS_SHARE_RETRY_MS, OpenHint, RelPath, RelPathBuf, RootAccess, RootRole,
    SealedMap, SealedMaps, ShareRetry, StoreFs, SyncKind, VfsErrorKind,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
/// The full checkpoint's delta number K (u21 cut D-5): the base is G = 1 and the full base's `FILES` names `hist.2` …
/// `hist.9` and `blobs.10` … `blobs.13`, so the delta is 14 and its `blobs` file 15.
pub const FULL_DELTA_NO: u32 = 14;
/// The full checkpoint's `blobs` file number.
pub const FULL_BLOBS_NO: u32 = 15;
/// The full checkpoint's delta file.
pub const FULL_DELTA_NAME: &str = "seg.d14";
/// The full checkpoint's `blobs` file.
pub const FULL_BLOBS_NAME: &str = "blobs.15";
/// `HEAD.next_file_no` the full checkpoint's setup restores: the next after the base's files.
pub const RESTORED_NEXT_FILE_NO: u32 = 14;
/// `next_file_no` after the full checkpoint ([F05 §9.9]: greater than every number its record names; [F04 §5.13]).
pub const FULL_NEXT_FILE_NO: u32 = 16;
/// The full checkpoint's P06 at the test profile (u21 cut D-3): 64 ops, so that most samples carry a body.
pub const TEST_FULL_P06: u64 = 64;

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
    /// T1's full delta checkpoint (u21 cut §3.8; M-A4, R-53 Q2): every section of the window's delta over a full base,
    /// its `blobs` file, the `Checkpoint` group and the publish into the other `HEAD` slot.
    CkptFull,
    /// T2's MCP overlay catch-up.
    Catchup,
    /// T1 with item 1a (M-A1; u20 cut §3.4): `read-4096.n1e6`'s tail and nodes in A1, read by the whole record, the
    /// frame walk, the offset read and item 1a's locator.
    Read1a4096,
    /// The same at P06 1,024, design-A A3's draft, from `read-1024.n1e6`'s seed.
    Read1a1024,
    /// The depth arm: one node in each of the 4,096-op A1 tail's first 4 commits, read through all of them.
    Read1aDepth4,
    /// The depth arm at 16 commits.
    Read1aDepth16,
    /// The depth arm at 64 commits.
    Read1aDepth64,
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
            Family::CkptFull => "ckpt-full-4096",
            Family::Catchup => "catchup-16",
            Family::Read1a4096 => "read-1a-4096",
            Family::Read1a1024 => "read-1a-1024",
            Family::Read1aDepth4 => "read-1a-d4",
            Family::Read1aDepth16 => "read-1a-d16",
            Family::Read1aDepth64 => "read-1a-d64",
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
            Family::CkptFull => &["ckpt.full", "ckpt.full.write"],
            Family::Catchup => &["catchup"],
            Family::Read1a4096
            | Family::Read1a1024
            | Family::Read1aDepth4
            | Family::Read1aDepth16
            | Family::Read1aDepth64 => &["read", "read.op", "read.op.at", "read.1a"],
        }
    }

    /// The minimum tier: the tier of the quantity's budget ([MP §3.2]); the layout probes have none.
    pub const fn min_tier(self) -> Option<Tier> {
        match self {
            Family::Read
            | Family::ReadOp
            | Family::ReadCostRecords
            | Family::ReadCostBytes
            | Family::Read1a4096
            | Family::Read1a1024 => Some(Tier::T1),
            Family::Ckpt | Family::CkptFull => Some(Tier::T3),
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
                | Family::CkptFull
                | Family::Catchup
                | Family::Read1a4096
                | Family::Read1a1024
                | Family::Read1aDepth4
                | Family::Read1aDepth16
                | Family::Read1aDepth64
        )
    }

    /// The P06 of item 1a's quantities' A1 tail at production, else `None`.
    pub const fn read_1a_p06(self) -> Option<u64> {
        match self {
            Family::Read1a4096
            | Family::Read1aDepth4
            | Family::Read1aDepth16
            | Family::Read1aDepth64 => Some(4_096),
            Family::Read1a1024 => Some(1_024),
            _ => None,
        }
    }

    /// The depth arm's d: the commits the hot node is in, all of which its read walks.
    pub const fn depth(self) -> Option<u32> {
        match self {
            Family::Read1aDepth4 => Some(4),
            Family::Read1aDepth16 => Some(16),
            Family::Read1aDepth64 => Some(64),
            _ => None,
        }
    }

    /// The production k of the seed `seed(name) ^ k` (u20 cut §3.4): for a depth arm the first k < 64 whose production
    /// tail holds at least d commits, pinned here and recomputed by
    /// `depth_arms_read_the_hot_node_through_d_records`; 0 for every other family and at the test profile. A 4,096-op
    /// tail of the mix holds about 40–90 commits, its long commits carrying hundreds of ops.
    pub const fn seed_k(self) -> u64 {
        match self {
            Family::Read1aDepth64 => 2,
            _ => 0,
        }
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

/// The quantities in run order: the night's 18, the deciding quantities first (T1's read and checkpoint, T2's
/// catch-up), then T1's read diagnostics, its evidence (R-40 O-2; the read cost's two counts as two quantities, R-44),
/// then the layout probes at 1e5 and 1e6 (R-34 B-3, M-15); then u20's block, appended (u20 cut §3.10): T1's read with
/// item 1a at P06 4,096 and 1,024, then its depth arm at 4, 16 and 64 records (§3.4); then u21's two, appended (u21
/// cut §3.8): T1's full delta checkpoint at 0.5 M and 1e6 nodes. u20's `ready` probe (§3.8) is retired (R-86 ruling 6).
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
    for f in [
        Family::Read1a4096,
        Family::Read1a1024,
        Family::Read1aDepth4,
        Family::Read1aDepth16,
        Family::Read1aDepth64,
    ] {
        g.push(q(f, 1_000_000));
    }
    g.push(q(Family::CkptFull, 500_000));
    g.push(q(Family::CkptFull, 1_000_000));
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
/// their tail is `read-4096`'s byte for byte and their nodes come in its order (spec Goal 1); and that item 1a's reads
/// at P06 take `read-<P06>`'s, so their plans and read cycle are the whole read's (u20 cut §3.4); and that the full
/// checkpoint takes `ckpt-4096`'s at the same scale, so at production its fixture tail and its samples' tails are the
/// night's checkpoint's byte for byte (u21 cut D-2). The depth arms keep their own name's, with [`Family::seed_k`] at
/// production.
pub fn seed_name(q: &Quantity) -> String {
    let scale = &q.name[q.family.stem().len()..];
    match q.family {
        Family::ReadOp | Family::ReadCostRecords | Family::ReadCostBytes => {
            format!("{}{scale}", Family::Read.stem())
        }
        Family::Read1a4096 | Family::Read1a1024 => {
            format!("read-{}{scale}", q.family.read_1a_p06().expect("a P06"))
        }
        Family::CkptFull => format!("{}{scale}", Family::Ckpt.stem()),
        _ => q.name.clone(),
    }
}

/// The tail of `q` at `profile`: a full `ops` tail at P06 over the base; for item 1a's quantities in A1, at their P06 at
/// production, with the depth arm's option and its seed's k; for the full checkpoint at the test profile, P06
/// [`TEST_FULL_P06`] (u21 cut D-3) with C2's bound lifted, since a 64-op tail at the test profile's P12 holds more
/// settles than C2's 2 KiB allows (u21 cut D-3, addendum); at production its target is `ckpt-4096`'s.
pub fn target(q: &Quantity, profile: Profile) -> Target {
    let mut p = profile.params();
    let k = match profile {
        Profile::Production => q.family.seed_k(),
        Profile::Test => 0,
    };
    let full_test = q.family == Family::CkptFull && profile == Profile::Test;
    if full_test {
        p.p06 = TEST_FULL_P06;
    }
    if let Some(p06) = q.family.read_1a_p06() {
        if profile == Profile::Production {
            p.p06 = p06;
        }
        p.shape = Shape {
            locator: Locator::A1,
            hot: q.family.depth().unwrap_or(0),
        };
    }
    Target {
        clause: Clause::Ops,
        threshold: p.p06,
        mult: 1,
        n: profile.n(q.n),
        seed: seed(&seed_name(q)) ^ k,
        params: p,
        rt_bound: full_test.then_some(u64::MAX),
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

/// How one of item 1a's four arms reads (u20 cut §3.4): each a floor of the one before it ([MP §4.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ReadPath {
    /// `read`: the whole record ([`Overlay::get`]).
    Whole,
    /// `read.op`: the frame walk ([`Overlay::get_op`]).
    Walk,
    /// `read.op.at`: u17's offset read ([`Overlay::get_op_at`]).
    Offset,
    /// `read.1a`: item 1a's locator ([`Overlay::get_1a`]).
    Item1a,
}

/// The key item 1a's quantities read: the status, or for the depth arm the hot node's field [`HOT_FIELD`].
pub fn read_1a_key(f: Family) -> Key {
    match f.depth() {
        Some(_) => Key::Field(HOT_FIELD),
        None => Key::Status,
    }
}

/// A read's value as one number the arms return and the setup compares: a status byte (the tail's, else the base
/// row's `NodeHdr.status`), or a field's xxh3-64 (0 when the tail does not set it: the base holds no field).
fn value_of(p: Probe, c: &Columns<'_>, id: u32, key: &Key) -> Result<u64, String> {
    match key {
        Key::Status => status_of(p, c, id).map(u64::from),
        Key::Field(_) => match p {
            Probe::Tail(Value::Field(b)) => Ok(xxhash_rust::xxh3::xxh3_64(&b)),
            Probe::NotInTail => Ok(0),
            Probe::Tail(v) => Err(format!("#{id}'s {key:?} probed as {v:?}")),
        },
        k => Err(format!("item 1a's reads read no {k:?}")),
    }
}

/// T1's read of `key` of `#id` by `path` through `log`: the value item 1a's arms time ([`value_of`]). `idx` serves the
/// offset read, `o`'s offsets the read through item 1a; `buf` and `stats` are the arm's own.
#[allow(clippy::too_many_arguments)]
pub fn read_by<S: LogSource>(
    path: ReadPath,
    log: &mut S,
    o: &Overlay,
    idx: &OpIndex,
    c: &Columns<'_>,
    id: u32,
    key: &Key,
    buf: &mut Vec<u8>,
    stats: &mut ReadStats,
) -> Result<u64, String> {
    let p = match path {
        ReadPath::Whole => o.get(log, id, key)?,
        ReadPath::Walk => o.get_op(log, id, key, TAIL_READ_CHUNK, buf)?,
        ReadPath::Offset => o.get_op_at(idx, log, id, key, TAIL_READ_CHUNK, buf)?,
        ReadPath::Item1a => o.get_1a(log, id, key, buf, stats)?,
    };
    value_of(p, c, id, key)
}

/// The setup's check of item 1a's quantities (u20 cut §3.4, `read_1a_agree`): reads `key` of every id of `ids` by the
/// four paths, each through its own source, and refuses, naming the id, any difference, any failure, or a read through
/// item 1a that refused a locator or met a (0, 0) one in the tail.
#[allow(clippy::too_many_arguments)]
pub fn read_1a_agree<A: LogSource, B: LogSource, C: LogSource, D: LogSource>(
    o: &Overlay,
    idx: &OpIndex,
    c: &Columns<'_>,
    ids: &[u32],
    key: &Key,
    whole: &mut A,
    walk: &mut B,
    at: &mut C,
    item: &mut D,
) -> Result<(), String> {
    let mut buf = Vec::new();
    for &id in ids {
        let mut s = ReadStats::default();
        let w = read_by(ReadPath::Whole, whole, o, idx, c, id, key, &mut buf, &mut s);
        let f = read_by(ReadPath::Walk, walk, o, idx, c, id, key, &mut buf, &mut s);
        let a = read_by(ReadPath::Offset, at, o, idx, c, id, key, &mut buf, &mut s);
        let x = read_by(ReadPath::Item1a, item, o, idx, c, id, key, &mut buf, &mut s);
        if w.is_err() || f != w || a != w || x != w || s.fallbacks > 0 || s.unlocated > 0 {
            return Err(format!(
                "#{id}: the whole-record read gives {w:?}, the frame walk {f:?}, the offset read {a:?}, item 1a \
                 {x:?} with {s:?}"
            ));
        }
    }
    Ok(())
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

/// The overlay in the offsets mode replayed from the fixture's store (`Overlay::replay_at` over its extents), checked
/// equal to the offsets-mode replay of the tail's bytes in memory, so the replay's reads are the store's (R-29).
fn replayed_at<V: StoreFs + SealedMaps>(p: &Prepared<'_, V>, t: &Tail) -> Result<Overlay, String> {
    let o = Overlay::replay_at(
        &mut VfsLog::new(p.vfs, &p.root, t.expect.geo),
        &t.expect,
        t.start,
        t.end,
        t.chain_in,
        t.base,
        TAIL_READ_CHUNK,
    )
    .map_err(|e| e.to_string())?;
    let mem = Overlay::replay_at(
        &mut MemLog::new(t.start, &t.bytes),
        &t.expect,
        t.start,
        t.end,
        t.chain_in,
        t.base,
        TAIL_READ_CHUNK,
    )
    .map_err(|e| e.to_string())?;
    if o != mem {
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
    generate(&ckpt_target(q, profile, j), false)
}

/// The target of checkpoint sample `j`'s tail: [`target`]'s with the seed `mix(seed(seed_name(q)) ^ (j + 1))`. For the
/// night's checkpoint [`seed_name`] is its own name, so its tails are the night's; the full checkpoint's are
/// `ckpt-4096`'s at the same scale (u21 cut D-2).
pub fn ckpt_target(q: &Quantity, profile: Profile, j: u64) -> Target {
    Target {
        seed: mix(seed(&seed_name(q)) ^ (j + 1)),
        ..target(q, profile)
    }
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

/// The base's `SegRef` ([F04 §4.1], `04-head.md:94-100`): G = 1, `kind` base, its header's `upto_lsn` and the first 16
/// bytes of its `seg_digest`.
pub fn base_ref(c: &Columns<'_>) -> SegRef {
    SegRef {
        file_no: 1,
        kind: head::seg_kind::BASE,
        upto_lsn: c.view.header.upto_lsn,
        blake3_16: c.view.bytes[72..88].try_into().expect("16 bytes"),
    }
}

/// The `HEAD` the full checkpoint's setup restores before each sample (u21 cut §3.7, D-8; critique I-4), as [A, B]:
/// slot A at `slot_seq` 2 and slot B the same state at 1, both valid by [F04 §7]. The state: `flags` `fts_tier2`; the
/// tail's `epoch`, `epoch_lsn`, `init` and `project_oid_algo` (`Expect`); `committed_lsn` = `durable_lsn` = the tail's
/// end; `checkpoint_lsn` = the tail's start, the base's `upto_lsn`; `segments` = [the base]; `active_log` 1;
/// `next_file_no` [`RESTORED_NEXT_FILE_NO`]; `next_ref_id` the base's `REFS.aux`; `commit_seq`, `next_id` and the HLCs
/// from the tail's records (which must be kept); `next_anchor` and `fence` 1; no `seq_ring` or `image_cursor` entry.
pub fn restored_head(t: &Tail, base: SegRef, next_ref_id: u32) -> Result<[HeadSlot; 2], String> {
    let (mut seq, mut hlc, mut next_id, mut commits) = (0u64, 0u64, t.base.n + 1, 0);
    for r in &t.records {
        if let Payload::Commit(c) = &r.payload {
            (seq, hlc, commits) = (c.seq, c.hlc, commits + 1);
            for op in &c.ops {
                if let Op::Create { id, .. } = op {
                    next_id = next_id.max(id + 1);
                }
            }
        }
    }
    if commits == 0 {
        return Err("the tail's records are not kept, or it holds no commit".into());
    }
    let a = HeadSlot {
        format: 1,
        flags: head::flags::FTS_TIER2,
        slot_seq: 2,
        epoch: t.expect.epoch,
        committed_lsn: t.end,
        durable_lsn: t.end,
        checkpoint_lsn: t.start,
        commit_seq: seq,
        next_id,
        next_anchor: 1,
        fence: 1,
        active_log: 1,
        segments: vec![base],
        init: t.expect.init,
        epoch_lsn: t.expect.epoch_lsn,
        next_file_no: RESTORED_NEXT_FILE_NO,
        next_ref_id,
        project_oid_algo: t.expect.project_oid_algo,
        hlc_seq: hlc,
        hlc_commit: hlc,
        ..HeadSlot::default()
    };
    let b = HeadSlot {
        slot_seq: 1,
        ..a.clone()
    };
    Ok([a, b])
}

/// The full checkpoint's `Checkpoint` group (u21 cut §3.7).
#[derive(Clone, Debug)]
pub struct FullGroup {
    /// The group's bytes, at the tail's end.
    pub bytes: Vec<u8>,
    /// Its trailer, which the identity read checks.
    pub trailer: u64,
    /// The new delta's `SegRef`.
    pub delta: SegRef,
}

/// The `Checkpoint` group of the full checkpoint of `t` ([F05 §9.9], `05-log.md:734-777`; u21 cut §3.7): `ckflags` bits
/// 0, 1 and 5 (5 only with a `blobs` file); `append_hlc` the last commit's `hlc` + 1; `next_file_no`
/// [`FULL_NEXT_FILE_NO`]; `upto_lsn` the tail's end; `active_log` 1; `segments` = [the base, `seg.d14`]; `window_start`
/// the tail's start; one `RefList` per ref with each commit's (lsn, `hlc`); `added` = [`blobs.15`]. One record at the
/// tail's end, chained from the tail's last trailer; refused when it would cross the extent or leave 1–39 bytes in it
/// (G-1, G-4, `05-log.md:189-215`).
pub fn full_checkpoint(
    t: &Tail,
    folded: &Folded,
    files: &Encoded,
    base: SegRef,
) -> Result<FullGroup, String> {
    let delta = SegRef {
        file_no: FULL_DELTA_NO,
        kind: head::seg_kind::DELTA,
        upto_lsn: t.end,
        blake3_16: files.seg[72..88].try_into().expect("16 bytes"),
    };
    let mut lists: BTreeMap<u32, Vec<(u64, u64)>> = BTreeMap::new();
    for m in &folded.marks {
        lists.entry(m.ref_id).or_default().push((m.lsn, m.hlc));
    }
    let last_hlc = folded.marks.iter().map(|m| m.hlc).max().unwrap_or(0);
    let added: Vec<FileEntry> = files
        .blobs
        .iter()
        .map(|(_, row)| FileEntry {
            family: file_family::BLOBS,
            ref_id: 0,
            file_no: row.file_no,
            total_len: row.total_len,
            digest: row.digest16,
        })
        .collect();
    let c = Checkpoint {
        ckflags: ckflag::SET_CHANGE
            | ckflag::WINDOW
            | if added.is_empty() {
                0
            } else {
                ckflag::FILES_ADDED
            },
        append_hlc: last_hlc + 1,
        next_file_no: FULL_NEXT_FILE_NO,
        upto_lsn: t.end,
        active_log: 1,
        segments: vec![base, delta],
        window_start: t.start,
        ref_lists: lists
            .into_iter()
            .map(|(ref_id, entries)| RefList { ref_id, entries })
            .collect(),
        added,
    };
    let tl = t.bytes.len();
    let seed = u64::from_le_bytes(t.bytes[tl - TRAILER_LEN..].try_into().expect("8 bytes"));
    let bytes = ckpt::group(&c, t.end, t.expect.epoch, seed)?;
    let geo = t.expect.geo;
    let ge = t.end + bytes.len() as u64;
    if geo.offset(t.end) == 0 || geo.extent(ge - 1) != geo.extent(t.end) {
        return Err(format!(
            "the Checkpoint group at lsn {} crosses its extent (G-1)",
            t.end
        ));
    }
    let left = geo.extent_end(ge - 1) - ge;
    if left > 0 && left < MIN_GROUP_LEN {
        return Err(format!(
            "the Checkpoint group leaves {left} bytes in its extent (G-4)"
        ));
    }
    let trailer = u64::from_le_bytes(
        bytes[bytes.len() - TRAILER_LEN..]
            .try_into()
            .expect("8 bytes"),
    );
    Ok(FullGroup {
        bytes,
        trailer,
        delta,
    })
}

/// What the full checkpoint's floor's setup makes for its op (u21 cut §3.8): sample j's tail, its fold, both files'
/// bytes and the group.
#[derive(Debug)]
pub struct FullIn {
    /// The sample's tail.
    pub tail: Tail,
    /// Its fold.
    pub folded: Folded,
    /// `seg.d14` and `blobs.15`.
    pub files: Encoded,
    /// The `Checkpoint` group.
    pub group: FullGroup,
}

/// What one sample of the full checkpoint made: its tail, the fold, the files, the group and the published slot. The
/// op returns it and the teardown drops it, outside the timed region.
#[derive(Debug)]
pub struct FullOut {
    /// The sample's tail.
    pub tail: Tail,
    /// Its fold.
    pub folded: Folded,
    /// `seg.d14` and `blobs.15`.
    pub files: Encoded,
    /// The `Checkpoint` group.
    pub group: FullGroup,
    /// The publish.
    pub published: Published,
}

/// The full checkpoint's setup of sample `j` (u21 cut §3.8), untimed: tail j (records kept, for `HEAD`) written over
/// the fixture's extent through the run's handle, the rest up to the high-water `hw` zeroed and the extent flushed
/// (`sync(Data)`), then `HEAD`'s two slots restored with one 8,192-byte `write_at`, not flushed.
#[allow(clippy::too_many_arguments)]
fn full_setup<V: StoreFs>(
    vfs: &V,
    q: &Quantity,
    profile: Profile,
    j: u64,
    start: u64,
    ext: &V::File,
    head_file: &V::File,
    hw: &Cell<u64>,
    base: SegRef,
    next_ref_id: u32,
) -> Result<Tail, ArmError> {
    let tj = generate(&ckpt_target(q, profile, j), true).map_err(ArmError)?;
    let geo = tj.expect.geo;
    if tj.start != start || geo.extent(tj.end - 1) != geo.extent(start) {
        return Err(ArmError(format!(
            "checkpoint tail {j} leaves the fixture's extent"
        )));
    }
    vfs.write_at(ext, geo.offset(tj.start), &tj.bytes)
        .map_err(arm_err)?;
    if hw.get() > tj.end {
        let zeros = vec![0u8; (hw.get() - tj.end) as usize];
        vfs.write_at(ext, geo.offset(tj.end), &zeros)
            .map_err(arm_err)?;
    }
    vfs.sync(ext, SyncKind::Data).map_err(arm_err)?;
    let [a, b] = restored_head(&tj, base, next_ref_id).map_err(ArmError)?;
    vfs.write_at(head_file, 0, &head::image(&a, &b).map_err(ArmError)?)
        .map_err(arm_err)?;
    Ok(tj)
}

/// The full checkpoint's writes (u21 cut §3.7–§3.8; `docs/spec/os/fs.md:409-412`): `blobs.15` and `seg.d14` sealed
/// (`create_new`, `write_at`, `sync(DataAndMeta)`, `seal`) in that order, one `sync_dir`, the `Checkpoint` group
/// written at the tail's end through the run's extent handle and flushed data-only, the publish into the other `HEAD`
/// slot ([F04 §9.1], not flushed), and the identity read of the group's trailer (`ARCHITECTURE-RESEARCH.md:728`). The
/// publish's S′: `segments` = [the base, `seg.d14`], `checkpoint_lsn` the tail's end, `active_log` 1, `next_file_no` 16,
/// `committed_lsn` = `durable_lsn` = the group's end.
#[allow(clippy::too_many_arguments)]
fn full_land<V: StoreFs>(
    vfs: &V,
    root: &V::Root,
    ext: &V::File,
    head_file: &V::File,
    t: &Tail,
    files: &Encoded,
    g: &FullGroup,
    base: SegRef,
) -> Result<Published, ArmError> {
    let geo = t.expect.geo;
    if let Some((b, _)) = &files.blobs {
        seal_file(vfs, root, FULL_BLOBS_NAME, b)?;
    }
    seal_file(vfs, root, FULL_DELTA_NAME, &files.seg)?;
    vfs.sync_dir(root, None).map_err(arm_err)?;
    vfs.write_at(ext, geo.offset(t.end), &g.bytes)
        .map_err(arm_err)?;
    vfs.sync(ext, SyncKind::Data).map_err(arm_err)?;
    let ge = t.end + g.bytes.len() as u64;
    let published = head::publish(
        &mut VfsSlot {
            vfs,
            file: head_file,
        },
        |s| {
            s.segments = vec![base, g.delta];
            s.checkpoint_lsn = t.end;
            s.active_log = 1;
            s.next_file_no = s.next_file_no.max(FULL_NEXT_FILE_NO);
            s.committed_lsn = ge;
            s.durable_lsn = ge;
        },
    )
    .map_err(ArmError)?;
    let mut trailer = [0u8; TRAILER_LEN];
    let got = vfs
        .read_at(ext, geo.offset(ge - TRAILER_LEN as u64), &mut trailer)
        .map_err(arm_err)?;
    if got != TRAILER_LEN || u64::from_le_bytes(trailer) != g.trailer {
        return Err(ArmError(format!(
            "the identity read at lsn {} gives {:#018x}, not the group's trailer {:#018x}",
            ge - TRAILER_LEN as u64,
            u64::from_le_bytes(trailer),
            g.trailer
        )));
    }
    Ok(published)
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
    let n = seal_file(vfs, root, name, bytes)?;
    vfs.sync_dir(root, None).map_err(arm_err)?;
    Ok(n)
}

/// A sealed file's write without the directory's flush: `create_new` under its final name, `write_at`,
/// `sync(DataAndMeta)`, `seal` (`fs.md:411`). The full checkpoint seals its two files and then flushes the directory
/// once (u21 cut D-9). Returns the bytes written.
pub fn seal_file<V: StoreFs>(
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
            for f in [DELTA_NAME, FLOOR_NAME, FULL_DELTA_NAME, FULL_BLOBS_NAME] {
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
    let spec = base::Spec {
        n,
        upto_lsn: p.e,
        base_seq: BASE_SEQ,
    };
    let bytes = match q.family {
        Family::CkptFull => base::build_full(&spec)?,
        _ => base::build(&spec)?,
    };
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
    let columns = Columns::parse(map.bytes())?;
    let operands = operands(q.family, &columns)?;
    drop(columns);
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
/// bytes) runs here, outside every timed region. The AND/popcount and scan arms time the
/// calls of [`count_ops`].
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
        Family::CkptFull => {
            let t = tail()?;
            let geo = t.expect.geo;
            let k = geo.extent(t.start);
            if geo.extent(t.end - 1) != k {
                return Err("the checkpoint's tail spans more than one extent".into());
            }
            let fb = Rc::new(FullBase::parse(c)?);
            let (base, next_ref_id) = (base_ref(c), fb.next_ref_id);
            let open_rw = |name: &str| -> Result<Rc<V::File>, String> {
                let f = p
                    .vfs
                    .open(
                        &p.root,
                        rel(name)?.as_rel_path(),
                        Access::ReadWrite,
                        OpenHint::Normal,
                    )
                    .map_err(|e| e.to_string())?;
                Ok(Rc::new(f))
            };
            // The extent and HEAD are opened once each, for writing, held for the run and shared by both arms, with the
            // high-water of the extent's written bytes (u21 cut D-10).
            let ext = open_rw(&extent_name(k))?;
            let head_file = open_rw("HEAD")?;
            let hw = Rc::new(Cell::new(t.end));
            let (profile, start) = (p.profile, t.start);
            {
                let (q0, e0, h0, w0) = (q.clone(), ext.clone(), head_file.clone(), hw.clone());
                let (e1, h1, fb1, w1) = (ext.clone(), head_file.clone(), fb.clone(), hw.clone());
                let mut j = 0u64;
                let mut log = VfsLog::new(p.vfs, &p.root, geo);
                let mut packer = Packer::default();
                arms.push(Box::new(Staged::new(
                    "ckpt.full",
                    ticker.clone(),
                    move || -> Result<Tail, ArmError> {
                        let tj = full_setup(
                            p.vfs,
                            &q0,
                            profile,
                            j,
                            start,
                            &e0,
                            &h0,
                            &w0,
                            base,
                            next_ref_id,
                        )?;
                        j += 1;
                        Ok(tj)
                    },
                    move |tj: Tail| -> Result<FullOut, ArmError> {
                        let folded = full::fold(
                            &mut log,
                            &tj.expect,
                            tj.start,
                            tj.end,
                            tj.chain_in,
                            TAIL_READ_CHUNK,
                            &fb1,
                            FULL_DELTA_NO,
                        )
                        .map_err(ArmError)?;
                        let files = full::encode(&folded.delta, FULL_BLOBS_NO, &mut packer)
                            .map_err(ArmError)?;
                        let group =
                            full_checkpoint(&tj, &folded, &files, base).map_err(ArmError)?;
                        let published =
                            full_land(p.vfs, &p.root, &e1, &h1, &tj, &files, &group, base)?;
                        Ok(FullOut {
                            tail: tj,
                            folded,
                            files,
                            group,
                            published,
                        })
                    },
                    move |out: FullOut| {
                        w1.set(out.tail.end + out.group.bytes.len() as u64);
                        drop(out);
                        unlink_if(p.vfs, &p.root, FULL_DELTA_NAME)?;
                        unlink_if(p.vfs, &p.root, FULL_BLOBS_NAME)
                    },
                )));
            }
            let (q1, e0, h0, w0, fb0) = (q.clone(), ext.clone(), head_file.clone(), hw.clone(), fb);
            let (e1, h1, w1) = (ext, head_file, hw);
            let mut j = 0u64;
            let mut log = VfsLog::new(p.vfs, &p.root, geo);
            let mut packer = Packer::default();
            arms.push(Box::new(Staged::new(
                "ckpt.full.write",
                ticker.clone(),
                move || -> Result<FullIn, ArmError> {
                    let tj = full_setup(
                        p.vfs,
                        &q1,
                        profile,
                        j,
                        start,
                        &e0,
                        &h0,
                        &w0,
                        base,
                        next_ref_id,
                    )?;
                    j += 1;
                    // The floor folds from the store in its setup, so a fold moved into its op would show as a log
                    // read between its ticks (u21 cut D-11).
                    let folded = full::fold(
                        &mut log,
                        &tj.expect,
                        tj.start,
                        tj.end,
                        tj.chain_in,
                        TAIL_READ_CHUNK,
                        &fb0,
                        FULL_DELTA_NO,
                    )
                    .map_err(ArmError)?;
                    let files = full::encode(&folded.delta, FULL_BLOBS_NO, &mut packer)
                        .map_err(ArmError)?;
                    let group = full_checkpoint(&tj, &folded, &files, base).map_err(ArmError)?;
                    Ok(FullIn {
                        tail: tj,
                        folded,
                        files,
                        group,
                    })
                },
                move |x: FullIn| -> Result<FullOut, ArmError> {
                    let published =
                        full_land(p.vfs, &p.root, &e1, &h1, &x.tail, &x.files, &x.group, base)?;
                    Ok(FullOut {
                        tail: x.tail,
                        folded: x.folded,
                        files: x.files,
                        group: x.group,
                        published,
                    })
                },
                move |out: FullOut| {
                    w1.set(out.tail.end + out.group.bytes.len() as u64);
                    drop(out);
                    unlink_if(p.vfs, &p.root, FULL_DELTA_NAME)?;
                    unlink_if(p.vfs, &p.root, FULL_BLOBS_NAME)
                },
            )));
        }
        Family::Read1a4096
        | Family::Read1a1024
        | Family::Read1aDepth4
        | Family::Read1aDepth16
        | Family::Read1aDepth64 => {
            let t = tail()?;
            let geo = t.expect.geo;
            let o = replayed_at(p, t)?;
            let key = read_1a_key(q.family);
            let ids = match q.family.depth() {
                Some(_) => vec![hot_node(t.base.n)],
                None => read_cycle(&o, p.seed),
            };
            let idx = OpIndex::build(
                &o,
                &mut VfsLog::new(p.vfs, &p.root, geo),
                TAIL_READ_CHUNK,
                &mut Vec::with_capacity(TAIL_READ_CHUNK),
            )?;
            // Each arm's own source; the setup's check reads through them, so every extent a sample reads is open
            // before the pilot.
            let mut logs: [VfsLog<'p, V>; 4] =
                std::array::from_fn(|_| VfsLog::new(p.vfs, &p.root, geo));
            let [l0, l1, l2, l3] = &mut logs;
            read_1a_agree(&o, &idx, c, &ids, &key, l0, l1, l2, l3)?;
            for ((name, path), mut log) in [
                ("read", ReadPath::Whole),
                ("read.op", ReadPath::Walk),
                ("read.op.at", ReadPath::Offset),
                ("read.1a", ReadPath::Item1a),
            ]
            .into_iter()
            .zip(logs)
            {
                let (o, idx, ids, key) = (o.clone(), idx.clone(), ids.clone(), key.clone());
                let mut buf = Vec::with_capacity(TAIL_READ_CHUNK);
                let mut stats = ReadStats::default();
                let mut k = 0usize;
                arms.push(Box::new(TimedFn::new(name, ticker.clone(), move || {
                    k += 1;
                    let id = ids[k % ids.len()];
                    read_by(path, &mut log, &o, &idx, c, id, &key, &mut buf, &mut stats)
                        .map_err(ArmError)
                })));
            }
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
        calls: Calls,
        ticks: Rc<RefCell<Vec<Tick>>>,
    }

    impl<'f> FsTicker<'f> {
        /// A ticker over `fs` that watches the files `watch` of the fixture directory `dir`.
        fn new(fs: &'f MemFs, dir: &Path, watch: &[&str]) -> FsTicker<'f> {
            FsTicker {
                fs,
                dir: dir.to_path_buf(),
                watch: watch.iter().map(|w| w.to_string()).collect(),
                calls: Calls::default(),
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
                calls: self.calls.lock().unwrap().len(),
                files,
            });
            t.len() as u64
        }
    }

    /// A call that [`LoggedFs`] logs on a file: an open (whether its handle may write), a write, a flush or a seal; on the
    /// directory (u21 critique I-3): its flush, under the name `.`; and on a map (u20 cut D-14, addendum m-3): the map, its
    /// first borrow of the bytes, its drop, and a `WillNeed` advice over (offset, length).
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Io {
        Open(bool),
        Write,
        Sync(SyncKind),
        Seal,
        SyncDir,
        Map,
        Borrow,
        Unmap,
        Advise(u64, u64),
    }

    /// A logged call: the file's name, the number of the handle or map it went through, and the call.
    type Call = (String, usize, Io);

    /// The calls a [`LoggedFs`] and its maps log, shared with the maps, which must be `Send + Sync`
    /// (`crates/moirai-vfs/src/map.rs:45`).
    type Calls = std::sync::Arc<std::sync::Mutex<Vec<Call>>>;

    /// A map of a [`LoggedFs`]: [`MemFs`]'s map, logging its first borrow and its drop.
    struct LoggedMap {
        inner: MemMap,
        name: String,
        n: usize,
        calls: Calls,
        borrowed: std::sync::atomic::AtomicBool,
    }

    impl SealedMap for LoggedMap {
        fn bytes(&self) -> &[u8] {
            if !self
                .borrowed
                .swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                self.calls
                    .lock()
                    .unwrap()
                    .push((self.name.clone(), self.n, Io::Borrow));
            }
            self.inner.bytes()
        }

        fn len(&self) -> u64 {
            self.inner.len()
        }
    }

    impl Drop for LoggedMap {
        fn drop(&mut self) {
            self.calls
                .lock()
                .unwrap()
                .push((self.name.clone(), self.n, Io::Unmap));
        }
    }

    /// An open file of a [`LoggedFs`]: [`MemFs`]'s handle, the file's name, and the handle's number (the index of its
    /// open in the log).
    struct LoggedFile {
        file: MemFile,
        name: String,
        n: usize,
    }

    /// [`MemFs`] with every open, write, flush and seal logged by file name and handle, and every flush of the directory
    /// (u12 test r1 I-1, I-2; u21 critique I-3): `MemFs` logs no write, flush or seal, and not which handle a call goes
    /// through. Every call goes on to `fs` unchanged, so
    /// `fs`'s own log, files and seals are as without the wrapper.
    struct LoggedFs<'f> {
        fs: &'f MemFs,
        calls: Calls,
    }

    impl<'f> LoggedFs<'f> {
        fn new(fs: &'f MemFs) -> LoggedFs<'f> {
            LoggedFs {
                fs,
                calls: Calls::default(),
            }
        }

        /// The calls so far.
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn log(&self, f: &LoggedFile, io: Io) {
            self.calls.lock().unwrap().push((f.name.clone(), f.n, io));
        }

        /// `file`, just opened as `rel`, numbered and logged.
        fn opened(&self, file: MemFile, rel: RelPath<'_>, write: bool) -> LoggedFile {
            let f = LoggedFile {
                file,
                name: rel.as_str().to_string(),
                n: self.calls.lock().unwrap().len(),
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
            let name = dir.map_or(".".to_string(), |d| d.as_str().to_string());
            self.calls.lock().unwrap().push((name, 0, Io::SyncDir));
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
            self.log(f, Io::Seal);
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
        type Map = LoggedMap;

        fn map_sealed(
            &self,
            f: &LoggedFile,
            expected_len: u64,
            name: RelPath<'_>,
        ) -> Result<LoggedMap, MapError> {
            let inner = self.fs.map_sealed(&f.file, expected_len, name)?;
            let mut calls = self.calls.lock().unwrap();
            let n = calls.len();
            calls.push((name.as_str().to_string(), n, Io::Map));
            Ok(LoggedMap {
                inner,
                name: name.as_str().to_string(),
                n,
                calls: self.calls.clone(),
                borrowed: false.into(),
            })
        }

        fn advise(&self, map: &LoggedMap, offset: u64, len: u64, advice: Advice) {
            if advice == Advice::WillNeed {
                self.calls
                    .lock()
                    .unwrap()
                    .push((map.name.clone(), map.n, Io::Advise(offset, len)));
            }
            self.fs.advise(&map.inner, offset, len, advice)
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

    /// The grid in run order with valid, unique names ([MP §4.1]): the night's 18 (the six layout families at 1e5 and
    /// 1e6, the sparse AND/popcount pair of R-30 among them, the read, its three diagnostics and the catch-up at 1e6, the
    /// checkpoint at 0.5 M), then u20's appended block of item 1a's 5 reads (u20 cut §3.4, §3.10), then u21's full
    /// checkpoint at 0.5 M and 1e6 (u21 cut §3.8); 46 arms (the `ready` probe's 12 retired, R-86 ruling 6); the minimum
    /// tiers of the budgets (t1 read ≤ 50 µs, its diagnostics and the reads with item 1a; t3 checkpoint ≤ 100 ms, and the
    /// full checkpoint's; t2 catch-up ≤ 5 ms, `ARCHITECTURE-RESEARCH.md:130`, `:140`), none for the layout families and
    /// the depth arms, no tail for the sparse pair; the diagnostics' arms (R-42 Q2) and their tails; the read cost's two
    /// counts as two quantities of one arm each, since every arm after arm 0 is a floor (R-44 I-1 (b)); item 1a's four
    /// arms, each a floor of the one before, and their A1 targets: P06 4,096 or 1,024 at production, the depth option at
    /// d; the full checkpoint's two arms, its tail, and its targets: 4,096 ops at production, 64 at the test profile with
    /// C2's bound lifted.
    #[test]
    fn grid_lists_the_valid_quantities_in_run_order() {
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
                "overlay.n1e6",
                "read-1a-4096.n1e6",
                "read-1a-1024.n1e6",
                "read-1a-d4.n1e6",
                "read-1a-d16.n1e6",
                "read-1a-d64.n1e6",
                "ckpt-full-4096.n5e5",
                "ckpt-full-4096.n1e6"
            ],
            "the night's 18 with the deciding quantities first (R-34 B-3, M-15), then u20's block, then u21's"
        );
        assert!(names.iter().all(|n| valid_name(n)));
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 25);
        let arms: Vec<&str> = g
            .iter()
            .flat_map(|q| q.family.arms().iter().copied())
            .collect();
        assert_eq!(arms.len(), 46);
        for (q, n) in g[23..].iter().zip([500_000, 1_000_000]) {
            assert_eq!(
                (q.family, q.n, q.family.arms()),
                (Family::CkptFull, n, &["ckpt.full", "ckpt.full.write"][..])
            );
            assert_eq!(
                (q.family.min_tier(), q.family.needs_tail()),
                (Some(Tier::T3), true)
            );
            let t = target(q, Profile::Production);
            assert_eq!(
                (t.clause, t.threshold, t.n, t.rt_bound),
                (Clause::Ops, 4_096, n, None)
            );
            let tt = target(q, Profile::Test);
            assert_eq!(
                (tt.threshold, tt.params.p06, tt.n, tt.rt_bound),
                (TEST_FULL_P06, 64, n / TEST_SCALE, Some(u64::MAX))
            );
        }
        for q in &g[18..23] {
            assert_eq!(
                q.family.arms(),
                ["read", "read.op", "read.op.at", "read.1a"],
                "{}",
                q.name
            );
            assert!(q.family.needs_tail(), "{}", q.name);
            let t = target(q, Profile::Production);
            let depth = q.family.depth();
            assert_eq!(
                (t.params.shape.locator, t.params.shape.hot, t.n),
                (Locator::A1, depth.unwrap_or(0), 1_000_000),
                "{}",
                q.name
            );
            assert_eq!(t.threshold, q.family.read_1a_p06().unwrap(), "{}", q.name);
            assert_eq!(t.params.p06, t.threshold, "{}", q.name);
            assert_eq!(
                q.family.min_tier(),
                depth.map_or(Some(Tier::T1), |_| None),
                "{}: t1 for the reads, none for the depth arms",
                q.name
            );
            let tt = target(q, Profile::Test);
            assert_eq!(
                (tt.threshold, tt.n),
                (Params::TEST.p06, 10_000),
                "{}",
                q.name
            );
        }
        assert_eq!(
            g[18..23]
                .iter()
                .map(|q| (q.family.read_1a_p06(), q.family.depth()))
                .collect::<Vec<_>>(),
            [
                (Some(4_096), None),
                (Some(1_024), None),
                (Some(4_096), Some(4)),
                (Some(4_096), Some(16)),
                (Some(4_096), Some(64))
            ]
        );
        assert!(g[..18].iter().all(|q| q.family.read_1a_p06().is_none()));
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

    /// `--smoke` runs every quantity at the test profile with each arm once: 46 lines, one per (quantity, arm), each
    /// ending with the arm's unit; with `--quantity` it runs that one alone; each run maps its base once.
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
        assert_eq!(lines.len(), 46, "{out}");
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
        // Each run maps its base once.
        assert_eq!(fs.log().maps.len(), grid().len());
    }

    /// `--smoke` with the runner's contract flags records nothing: no line says recorded, every fixture is removed,
    /// the checkpoints' files (`seg.d1`, `seg.d2`, `seg.d14`, `blobs.15`) and `HEAD` included, and `--smoke` with `--out`
    /// is refused.
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
        assert_eq!(out.lines().count(), 46);
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

    /// The full checkpoint's quantities (u21 cut §3.8, D-2, D-3): `ckpt-full-4096.n5e5` and `.n1e6` take `ckpt-4096`'s
    /// seed name at their scale, so at production their fixture's target and every sample's target equal
    /// `ckpt-4096.n5e5`'s (n5e5), compared without generating; `ckpt-4096`'s own targets keep the night's seeds. At the
    /// test profile (critique m-7: only there is a base built here) the fixture is the full base at N / 100 with
    /// `tok_ver` 1 and its 50 refs, a 64-op tail and `HEAD`.
    #[test]
    fn ckpt_full_quantities_share_ckpt_4096s_tails_and_hold_the_full_base() {
        let g = grid();
        let of = |name: &str| g.iter().find(|q| q.name == name).unwrap().clone();
        let (full5, full6, ckpt5) = (
            of("ckpt-full-4096.n5e5"),
            of("ckpt-full-4096.n1e6"),
            of("ckpt-4096.n5e5"),
        );
        assert_eq!(
            [seed_name(&full5), seed_name(&full6), seed_name(&ckpt5)],
            ["ckpt-4096.n5e5", "ckpt-4096.n1e6", "ckpt-4096.n5e5"]
        );
        let prod = Profile::Production;
        assert_eq!(
            target(&full5, prod),
            target(&ckpt5, prod),
            "the fixture's tail"
        );
        for j in 0..3 {
            assert_eq!(
                ckpt_target(&full5, prod, j),
                ckpt_target(&ckpt5, prod, j),
                "sample {j}'s tail"
            );
        }
        assert_eq!(
            ckpt_target(&ckpt5, prod, 0).seed,
            mix(seed("ckpt-4096.n5e5") ^ 1),
            "the night's seeds"
        );
        assert_eq!(ckpt_target(&full6, prod, 0).n, 1_000_000);
        let fs = MemFs::with_dir(&scratch());
        let p = prepare(&fs, &scratch(), &full5, Profile::Test).unwrap();
        let c = Columns::parse(p.map.bytes()).unwrap();
        let fb = FullBase::parse(&c).unwrap();
        assert_eq!(
            (c.rows, c.view.header.tok_ver, fb.refs.len()),
            (5_000, 1, 50)
        );
        assert_eq!(p.tail.as_ref().unwrap().measures.ops, 64);
        assert!(fs.bytes(&p.dir().join("HEAD")).is_some());
    }

    /// The handles of the opens for writing among `lfs`'s calls from `before`: the extent's and `HEAD`'s, which [`arms`]
    /// opens for the full checkpoint.
    fn rw_handles(lfs: &LoggedFs<'_>, before: usize) -> (usize, usize) {
        let opened: Vec<Call> = lfs.calls()[before..]
            .iter()
            .filter(|c| c.2 == Io::Open(true))
            .cloned()
            .collect();
        assert_eq!(
            opened.len(),
            2,
            "arms opens two files for writing: {opened:?}"
        );
        (opened[0].1, opened[1].1)
    }

    /// The full checkpoint's fixture over a [`LoggedFs`], its arms over an [`FsTicker`] watching `seg.d14`,
    /// `blobs.15`, `HEAD` and the extent, and the handles [`arms`] opened on the extent and on `HEAD`.
    struct FullRig<'f> {
        ticker: FsTicker<'f>,
        ext: String,
        he: usize,
        hh: usize,
    }

    fn full_rig<'f>(
        fs: &'f MemFs,
        lfs: &LoggedFs<'_>,
        p: &Prepared<'_, LoggedFs<'_>>,
    ) -> FullRig<'f> {
        let t = p.tail.as_ref().unwrap();
        let ext = extent_name(t.expect.geo.extent(t.start));
        let ticker = FsTicker::new(
            fs,
            p.dir(),
            &[FULL_DELTA_NAME, FULL_BLOBS_NAME, "HEAD", &ext],
        )
        .logging(lfs);
        FullRig {
            ticker,
            ext,
            he: 0,
            hh: 0,
        }
    }

    /// The full checkpoint's setup (u21 cut §3.8, D-8, D-10) for both arms: [`arms`] opens the extent and `HEAD` once
    /// each, for writing; in every sample, before t0, the extent holds tail j's image and zeros after it, every write and
    /// flush on it goes through the run's handle and the last is `sync(Data)`; `HEAD` holds the restored slots (A at
    /// `slot_seq` 2, B at 1, both valid), written with one write through the run's `HEAD` handle and never flushed.
    #[test]
    fn ckpt_full_setup_restores_head_and_the_extent_before_t0() {
        let fs = MemFs::with_dir(&scratch());
        let lfs = LoggedFs::new(&fs);
        let (q, p) = fixture(&lfs, Family::CkptFull);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let fb = FullBase::parse(&c).unwrap();
        let mut rig = full_rig(&fs, &lfs, &p);
        let before = lfs.calls().len();
        let mut arms = arms(&p, &c, &q, &rig.ticker).unwrap();
        let mut from = lfs.calls().len();
        let opened: Vec<Call> = lfs.calls()[before..from]
            .iter()
            .filter(|c| matches!(c.2, Io::Open(_)))
            .cloned()
            .collect();
        let [(e, he, Io::Open(true)), (h, hh, Io::Open(true))] = opened.as_slice() else {
            panic!("arms opens the extent and HEAD once each, for writing: {opened:?}");
        };
        assert_eq!((e.as_str(), h.as_str()), (rig.ext.as_str(), "HEAD"));
        (rig.he, rig.hh) = (*he, *hh);
        let geo = p.tail.as_ref().unwrap().expect.geo;
        for k in 0..2 {
            let tj = generate(&ckpt_target(&q, Profile::Test, k), true).unwrap();
            for (arm, name) in [(0, "ckpt.full"), (1, "ckpt.full.write")] {
                arms[arm].sample().unwrap();
                let (t0, _) = rig.ticker.last();
                let mut image = vec![0u8; geo.e() as usize];
                let at = geo.offset(tj.start) as usize;
                image[at..at + tj.bytes.len()].copy_from_slice(&tj.bytes);
                assert!(
                    t0.files[&rig.ext].0 == image,
                    "{name} sample {k}: the extent at t0 is not tail {k}'s image"
                );
                let [a, b] = restored_head(&tj, base_ref(&c), fb.next_ref_id).unwrap();
                assert_eq!(a.slot_seq, 2);
                assert_eq!(
                    t0.files["HEAD"].0,
                    head::image(&a, &b).unwrap(),
                    "{name} sample {k}: HEAD at t0 is not the restored state"
                );
                for slot in [&t0.files["HEAD"].0[..4_096], &t0.files["HEAD"].0[4_096..]] {
                    assert!(matches!(head::classify(slot), head::Class::Valid(_)));
                }
                let ext_calls: Vec<(usize, Io)> = calls_on(&lfs, from, t0.calls, &rig.ext)
                    .into_iter()
                    .filter(|c| !matches!(c.1, Io::Open(false)))
                    .collect();
                assert!(
                    ext_calls.iter().all(|&(n, _)| n == rig.he)
                        && ext_calls.contains(&(rig.he, Io::Write)),
                    "{name} sample {k}: the extent written before t0 through another handle than the run's: {ext_calls:?}"
                );
                assert_eq!(
                    ext_calls.last(),
                    Some(&(rig.he, Io::Sync(SyncKind::Data))),
                    "{name} sample {k}: the setup's last call on the extent is not its flush"
                );
                assert_eq!(
                    calls_on(&lfs, from, t0.calls, "HEAD"),
                    [(rig.hh, Io::Write)],
                    "{name} sample {k}: HEAD restored with one write, not flushed"
                );
                from = lfs.calls().len();
            }
        }
    }

    /// The full checkpoint's arm 0 runs every write inside its timed region (spec gate 1, R-29; critique I-3): at sample
    /// j's t0 `blobs.15` and `seg.d14` are absent and the extent holds no group after tail j; between the ticks the store
    /// reads the log over exactly `[start, end)` (the fold) and the group's trailer (the identity read), and `HEAD` once
    /// (8,192 B at 0); `blobs.15` then `seg.d14` are each created, written, flushed (`DataAndMeta`) and sealed, then the
    /// directory is flushed once, then the group is written at the tail's end through the run's handle and flushed
    /// (`Data`), then `HEAD` is written once through the run's handle and never flushed; at t1 both files are present and
    /// sealed, the extent holds the group, `HEAD`'s slot A (which holds S) is unchanged and slot B is not. Every sample
    /// carries a body, so `blobs.15` exists (D-3).
    #[test]
    fn ckpt_full_arm_writes_every_file_inside_its_timed_region() {
        let fs = MemFs::with_dir(&scratch());
        let lfs = LoggedFs::new(&fs);
        let (q, p) = fixture(&lfs, Family::CkptFull);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let mut rig = full_rig(&fs, &lfs, &p);
        let before = lfs.calls().len();
        let mut arms = arms(&p, &c, &q, &rig.ticker).unwrap();
        (rig.he, rig.hh) = rw_handles(&lfs, before);
        let geo = p.tail.as_ref().unwrap().expect.geo;
        for k in 0..3 {
            let tj = generate(&ckpt_target(&q, Profile::Test, k), true).unwrap();
            arms[0].sample().unwrap();
            let (t0, t1) = rig.ticker.last();
            check_full_timed(&lfs, &rig, &tj, geo, &t0, &t1, "ckpt.full", k, true);
        }
        assert_eq!(rig.ticker.count(), 6, "two ticks per sample");
    }

    /// The checks of one full-checkpoint sample's timed region (both arms): the files at t0 and t1, the reads and the
    /// calls between the ticks, as [`ckpt_full_arm_writes_every_file_inside_its_timed_region`] states. `folds` says
    /// whether the fold's log reads belong between the ticks (arm 0) or not (the floor).
    #[allow(clippy::too_many_arguments)]
    fn check_full_timed(
        lfs: &LoggedFs<'_>,
        rig: &FullRig<'_>,
        tj: &Tail,
        geo: Geometry,
        t0: &Tick,
        t1: &Tick,
        name: &str,
        k: u64,
        folds: bool,
    ) {
        assert!(
            !t0.files.contains_key(FULL_BLOBS_NAME),
            "{name} sample {k}: blobs.15 present at t0"
        );
        assert!(
            !t0.files.contains_key(FULL_DELTA_NAME),
            "{name} sample {k}: seg.d14 present at t0"
        );
        let at = geo.offset(tj.end) as usize;
        assert!(
            t0.files[&rig.ext].0[at..at + REC_HDR_LEN]
                .iter()
                .all(|&b| b == 0),
            "{name} sample {k}: a group in the extent at t0"
        );
        for f in [FULL_BLOBS_NAME, FULL_DELTA_NAME] {
            assert_eq!(
                t1.files.get(f).map(|x| x.1),
                Some(true),
                "{name} sample {k}: {f} sealed at t1"
            );
        }
        let ext1 = &t1.files[&rig.ext].0;
        let len = u32::from_le_bytes(ext1[at..at + 4].try_into().unwrap()) as usize;
        let seed = u64::from_le_bytes(tj.bytes[tj.bytes.len() - 8..].try_into().unwrap());
        ckpt::read_group(&ext1[at..at + len], tj.end, tj.expect.epoch, seed).unwrap_or_else(|e| {
            panic!("{name} sample {k}: no Checkpoint group at the tail's end at t1: {e}")
        });
        let ge = tj.end + len as u64;
        let (h0, h1) = (&t0.files["HEAD"].0, &t1.files["HEAD"].0);
        assert_eq!(
            h1[..4_096],
            h0[..4_096],
            "{name} sample {k}: the slot that holds S written"
        );
        assert!(
            h1[4_096..] != h0[4_096..],
            "{name} sample {k}: HEAD's other slot unchanged at t1"
        );
        let reads = lfs.fs.log().reads[t0.reads..t1.reads].to_vec();
        let head: Vec<&Read> = reads.iter().filter(|r| r.0 == "HEAD").collect();
        assert_eq!(
            head,
            [&("HEAD".to_string(), 0, HEAD_LEN)],
            "{name} sample {k}"
        );
        let log: Vec<Read> = reads.iter().filter(|r| r.0 != "HEAD").cloned().collect();
        assert!(
            log.iter().all(|r| r.0.starts_with("log.")),
            "{name} sample {k}: {log:?}"
        );
        let want: Vec<(u64, u64)> = if folds {
            vec![(tj.start, tj.end), (ge - 8, ge)]
        } else {
            vec![(ge - 8, ge)]
        };
        assert_eq!(
            lsn_cover(geo, &log),
            want,
            "{name} sample {k}: the log reads between the ticks (a log read between the floor's ticks is its fold)"
        );
        let timed = &lfs.calls()[t0.calls..t1.calls];
        let pos = |file: &str, io: Io| {
            timed
                .iter()
                .position(|c| c.0 == file && c.2 == io)
                .unwrap_or_else(|| {
                    panic!("{name} sample {k}: no {io:?} on {file} between the ticks: {timed:?}")
                })
        };
        for f in [FULL_BLOBS_NAME, FULL_DELTA_NAME] {
            let calls: Vec<Io> = timed.iter().filter(|c| c.0 == f).map(|c| c.2).collect();
            assert_eq!(
                calls,
                [
                    Io::Open(true),
                    Io::Write,
                    Io::Sync(SyncKind::DataAndMeta),
                    Io::Seal
                ],
                "{name} sample {k}: {f}"
            );
        }
        let dirs: Vec<&Call> = timed.iter().filter(|c| c.2 == Io::SyncDir).collect();
        assert_eq!(
            dirs.len(),
            1,
            "{name} sample {k}: one sync_dir between the ticks: {dirs:?}"
        );
        let ext: Vec<(usize, Io)> = timed
            .iter()
            .filter(|c| c.0 == rig.ext && c.2 != Io::Open(false))
            .map(|c| (c.1, c.2))
            .collect();
        assert_eq!(
            ext,
            [(rig.he, Io::Write), (rig.he, Io::Sync(SyncKind::Data))],
            "{name} sample {k}: the extent between the ticks"
        );
        let heads: Vec<(usize, Io)> = timed
            .iter()
            .filter(|c| c.0 == "HEAD")
            .map(|c| (c.1, c.2))
            .collect();
        assert_eq!(
            heads,
            [(rig.hh, Io::Write)],
            "{name} sample {k}: HEAD written once, never flushed"
        );
        let order = [
            pos(FULL_BLOBS_NAME, Io::Seal),
            pos(FULL_DELTA_NAME, Io::Open(true)),
            pos(FULL_DELTA_NAME, Io::Seal),
            pos(".", Io::SyncDir),
            pos(&rig.ext, Io::Write),
            pos(&rig.ext, Io::Sync(SyncKind::Data)),
            pos("HEAD", Io::Write),
        ];
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "{name} sample {k}: seal blobs.15, seal seg.d14, sync_dir, append, flush, publish: {order:?}"
        );
    }

    /// The full checkpoint's floor writes what arm 0 writes without folding (u21 cut §3.8, D-11; spec gate 1, R-29):
    /// for samples j = 0, 1 its setup reads the log over `[start, end)` before t0, and between its ticks it reads only
    /// the group's trailer and `HEAD`, with the same calls, in the same order, as arm 0's; at t1 `seg.d14`, `blobs.15`, the
    /// extent and `HEAD` hold exactly what arm 0's sample j left.
    #[test]
    fn ckpt_full_floor_writes_the_same_files_without_folding() {
        let fs = MemFs::with_dir(&scratch());
        let lfs = LoggedFs::new(&fs);
        let (q, p) = fixture(&lfs, Family::CkptFull);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let mut rig = full_rig(&fs, &lfs, &p);
        let before = lfs.calls().len();
        let mut arms = arms(&p, &c, &q, &rig.ticker).unwrap();
        (rig.he, rig.hh) = rw_handles(&lfs, before);
        let geo = p.tail.as_ref().unwrap().expect.geo;
        for k in 0..2 {
            let tj = generate(&ckpt_target(&q, Profile::Test, k), true).unwrap();
            arms[0].sample().unwrap();
            let (_, a1) = rig.ticker.last();
            let reads0 = fs.log().reads.len();
            arms[1].sample().unwrap();
            let (t0, t1) = rig.ticker.last();
            let setup: Vec<Read> = fs.log().reads[reads0..t0.reads].to_vec();
            assert_eq!(
                lsn_cover(geo, &setup),
                [(tj.start, tj.end)],
                "sample {k}: the floor's setup does not read the tail from the store"
            );
            check_full_timed(&lfs, &rig, &tj, geo, &t0, &t1, "ckpt.full.write", k, false);
            for f in [FULL_DELTA_NAME, FULL_BLOBS_NAME, "HEAD", rig.ext.as_str()] {
                assert!(
                    t1.files[f] == a1.files[f],
                    "sample {k}: the floor's {f} differs from arm 0's"
                );
            }
        }
    }

    /// The full checkpoint's products read back (u21 cut §3.6–§3.7; spec gate 1): for samples j = 0, 1, 2 of arm 0,
    /// `seg.d14` and `blobs.15` at t1 decode ([`full::decode`]) to the fold of tail j read from memory, with a body; the
    /// group at the tail's end is the `Checkpoint` of [F05 §9.9] (`ckflags` 0x0023, `append_hlc` the last commit's + 1,
    /// `next_file_no` 16, the set [base, `seg.d14` with its digest], the window from the tail's start, one `RefList` of
    /// every commit's (lsn, `hlc`), `blobs.15` added with its length and digest), chained from the tail's last trailer;
    /// `HEAD` selects slot B, S′ = the restored S with `slot_seq` 3, the set, `checkpoint_lsn` the tail's end,
    /// `committed_lsn` = `durable_lsn` = the group's end and `next_file_no` 16, valid by [F04 §7] as S is (critique I-4).
    #[test]
    fn ckpt_full_products_read_back() {
        let fs = MemFs::with_dir(&scratch());
        let (q, p) = fixture(&fs, Family::CkptFull);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let fb = FullBase::parse(&c).unwrap();
        let base = base_ref(&c);
        let t = p.tail.as_ref().unwrap();
        let (geo, ext) = (t.expect.geo, extent_name(t.expect.geo.extent(t.start)));
        let ticker = FsTicker::new(
            &fs,
            p.dir(),
            &[FULL_DELTA_NAME, FULL_BLOBS_NAME, "HEAD", &ext],
        );
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        for k in 0..3 {
            let tj = generate(&ckpt_target(&q, Profile::Test, k), true).unwrap();
            arms[0].sample().unwrap();
            let (_, t1) = ticker.last();
            let seg = &t1.files[FULL_DELTA_NAME].0;
            let blobs = &t1.files[FULL_BLOBS_NAME].0;
            let want = full::fold(
                &mut MemLog::new(tj.start, &tj.bytes),
                &tj.expect,
                tj.start,
                tj.end,
                tj.chain_in,
                TAIL_READ_CHUNK,
                &fb,
                FULL_DELTA_NO,
            )
            .unwrap();
            assert!(
                !want.delta.bodies.is_empty(),
                "sample {k} carries a body (D-3)"
            );
            assert_eq!(
                full::decode(seg, Some(blobs)).unwrap(),
                want.delta,
                "sample {k}: seg.d14 and blobs.15 are the fold of tail {k}"
            );
            let at = geo.offset(tj.end) as usize;
            let e1 = &t1.files[&ext].0;
            let len = u32::from_le_bytes(e1[at..at + 4].try_into().unwrap()) as usize;
            let seed = u64::from_le_bytes(tj.bytes[tj.bytes.len() - 8..].try_into().unwrap());
            let (ck, trailer) =
                ckpt::read_group(&e1[at..at + len], tj.end, tj.expect.epoch, seed).unwrap();
            let ge = tj.end + len as u64;
            let d14 = SegRef {
                file_no: 14,
                kind: head::seg_kind::DELTA,
                upto_lsn: tj.end,
                blake3_16: seg[72..88].try_into().unwrap(),
            };
            let commits: Vec<(u64, u64)> = tj
                .records
                .iter()
                .filter_map(|r| match &r.payload {
                    Payload::Commit(cm) => Some((r.lsn, cm.hlc)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                ck,
                Checkpoint {
                    ckflags: 0x0023,
                    append_hlc: commits.last().unwrap().1 + 1,
                    next_file_no: 16,
                    upto_lsn: tj.end,
                    active_log: 1,
                    segments: vec![base, d14],
                    window_start: tj.start,
                    ref_lists: vec![RefList {
                        ref_id: 0,
                        entries: commits,
                    }],
                    added: vec![FileEntry {
                        family: 6,
                        ref_id: 0,
                        file_no: 15,
                        total_len: blobs.len() as u64,
                        digest: blobs[72..88].try_into().unwrap(),
                    }],
                },
                "sample {k}: the Checkpoint record"
            );
            assert_eq!(
                trailer,
                u64::from_le_bytes(e1[at + len - 8..at + len].try_into().unwrap()),
                "sample {k}: the trailer the identity read checks"
            );
            let hb = &t1.files["HEAD"].0;
            let [a, _] = restored_head(&tj, base, fb.next_ref_id).unwrap();
            let s2 = HeadSlot {
                slot_seq: 3,
                segments: vec![base, d14],
                checkpoint_lsn: tj.end,
                committed_lsn: ge,
                durable_lsn: ge,
                next_file_no: 16,
                ..a.clone()
            };
            assert_eq!(
                head::select(hb),
                Ok((1, s2)),
                "sample {k}: the published slot"
            );
            assert_eq!(
                head::classify(&hb[..4_096]),
                head::Class::Valid(Box::new(a)),
                "sample {k}: S"
            );
        }
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
    /// would also build a 1e6-node base). Every other quantity of the night's 18 keeps the seed of its own name; u20's
    /// reads with item 1a have their own rule (`read_1a_quantities_share_the_read_tails_seed_and_nodes`).
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
        for q in g[..18].iter().filter(|q| !qs.contains(q)) {
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
            assert_eq!(calls.len(), 25);
            for (c, q) in calls.iter().zip(grid()) {
                let mut want = base.clone();
                want.push("--quantity".into());
                want.push(q.name.into());
                assert_eq!(c, &want);
            }
            assert_eq!(String::from_utf8(out).unwrap().lines().count(), 25);
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

    /// A test-profile A1 tail of `ops` ops over N = `n`, records kept, and its base's bytes.
    fn a1_tail_and_base(n: u32, ops: u64, seed: u64) -> (Tail, Vec<u8>) {
        let mut t = crate::tail::synth::tests::a1(test_target(Clause::Ops, n, seed));
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

    /// The offsets-mode overlay of `t`, replayed from memory.
    fn overlay_at(t: &Tail) -> Overlay {
        Overlay::replay_at(
            &mut MemLog::new(t.start, &t.bytes),
            &t.expect,
            t.start,
            t.end,
            t.chain_in,
            t.base,
            TAIL_READ_CHUNK,
        )
        .unwrap()
    }

    /// The four reads of item 1a's quantities of `key` of `id` through clean sources of `t`, with the stats of the read
    /// through item 1a.
    fn four_reads(
        t: &Tail,
        o: &Overlay,
        idx: &OpIndex,
        c: &Columns<'_>,
        id: u32,
        key: &Key,
    ) -> ([Result<u64, String>; 4], ReadStats) {
        let mut stats = ReadStats::default();
        let mut buf = Vec::new();
        let reads = [
            ReadPath::Whole,
            ReadPath::Walk,
            ReadPath::Offset,
            ReadPath::Item1a,
        ]
        .map(|path| {
            let mut log = MemLog::new(t.start, &t.bytes);
            read_by(path, &mut log, o, idx, c, id, key, &mut buf, &mut stats)
        });
        (reads, stats)
    }

    /// The offset index of `o` over `t`'s bytes.
    fn index_of(t: &Tail, o: &Overlay) -> OpIndex {
        OpIndex::build(
            o,
            &mut MemLog::new(t.start, &t.bytes),
            TAIL_READ_CHUNK,
            &mut Vec::new(),
        )
        .unwrap()
    }

    /// Item 1a's reads share the whole read's plans and nodes (u20 cut §3.4; F-11): `read-1a-4096` draws `read-4096`'s
    /// seed and `read-1a-1024` that of `read-1024.n1e6`, while the depth arms keep their own names'; at production each
    /// A1 read tail holds, commit by commit, the owners and op kinds of the night-form tail of the same target (for
    /// `read-1a-4096` that is `read-4096`'s), so the read cycle is the same; only the A1 tail carries locator bytes.
    #[test]
    fn read_1a_quantities_share_the_read_tails_seed_and_nodes() {
        let g = grid();
        let of = |f: Family| g.iter().find(|q| q.family == f).unwrap().clone();
        let read = of(Family::Read);
        let (r4, r1) = (of(Family::Read1a4096), of(Family::Read1a1024));
        assert_eq!(seed_name(&r4), read.name);
        assert_eq!(seed_name(&r1), "read-1024.n1e6");
        for f in [
            Family::Read1aDepth4,
            Family::Read1aDepth16,
            Family::Read1aDepth64,
        ] {
            assert_eq!(seed_name(&of(f)), of(f).name);
        }
        let plans = |tail: &Tail| -> Vec<Vec<(u32, u8)>> {
            tail.records
                .iter()
                .filter_map(|r| match &r.payload {
                    Payload::Commit(c) => {
                        Some(c.ops.iter().map(|o| (o.owner(), o.tag())).collect())
                    }
                    _ => None,
                })
                .collect()
        };
        for (q, p06) in [(&r4, 4_096), (&r1, 1_024)] {
            let t = target(q, Profile::Production);
            let mut night = t;
            night.params.shape = Shape::NIGHT;
            if p06 == 4_096 {
                assert_eq!(night, target(&read, Profile::Production));
            }
            let (a, b) = (generate(&t, true).unwrap(), generate(&night, true).unwrap());
            assert_eq!((a.measures.ops, b.measures.ops), (p06, p06), "{}", q.name);
            assert_eq!(plans(&a), plans(&b), "{}: the same plans", q.name);
            assert_eq!(
                read_cycle(&a.overlay().unwrap(), t.seed),
                read_cycle(&b.overlay().unwrap(), night.seed),
                "{}: the same read cycle",
                q.name
            );
            assert!(
                a.measures.loc_bytes > 0 && b.measures.loc_bytes == 0,
                "{}",
                q.name
            );
        }
    }

    /// Item 1a's four reads return the whole-record read's value (spec Goal 1, u20 cut §3.4): on A1 tails of seeds 1–3
    /// over N = 300 (owners repeat), every touched node's status is the naive map's or its base row's by all four paths,
    /// with no locator refused; on the `read-1a-4096` test fixture every id of the cycle reads the same by all four; on
    /// the `read-1a-d16` fixture so does the hot node's field 9, which the tail sets.
    #[test]
    fn read_1a_arms_return_the_whole_record_reads_value() {
        for seed in 1..=3 {
            let (t, b) = a1_tail_and_base(300, 400, seed);
            let c = Columns::parse(&b).unwrap();
            let o = overlay_at(&t);
            let idx = index_of(&t, &o);
            let naive = naive_status(&t);
            let (mut from_base, mut hops) = (0, 0);
            for e in o.nodes() {
                let want = naive.get(&e.n).copied().unwrap_or_else(|| {
                    from_base += 1;
                    (e.n % 4) as u8
                });
                let (reads, stats) = four_reads(&t, &o, &idx, &c, e.n, &Key::Status);
                for (i, r) in reads.iter().enumerate() {
                    assert_eq!(r, &Ok(u64::from(want)), "seed {seed} #{} path {i}", e.n);
                }
                assert_eq!((stats.unlocated, stats.fallbacks), (0, 0));
                hops += stats.located;
            }
            assert!(
                naive.len() > 50 && from_base > 20 && hops > 20,
                "both paths and hops"
            );
        }
        let fs = MemFs::with_dir(&scratch());
        for (family, depth) in [(Family::Read1a4096, false), (Family::Read1aDepth16, true)] {
            let (q, p) = fixture(&fs, family);
            let c = Columns::parse(p.map.bytes()).unwrap();
            let t = p.tail.as_ref().unwrap();
            let o = overlay_at(t);
            let idx = index_of(t, &o);
            let key = read_1a_key(family);
            let ids = if depth {
                vec![hot_node(t.base.n)]
            } else {
                read_cycle(&o, p.seed)
            };
            assert!(!ids.is_empty(), "{}", q.name);
            for id in ids {
                let (reads, _) = four_reads(t, &o, &idx, &c, id, &key);
                assert!(reads[0].is_ok(), "{} #{id}: {:?}", q.name, reads[0]);
                assert!(
                    reads.iter().all(|r| r == &reads[0]),
                    "{} #{id}: {reads:?}",
                    q.name
                );
                assert!(!depth || reads[0] != Ok(0), "{}: field 9 is set", q.name);
            }
        }
    }

    /// `read.1a` times one read per hop (R-29, spec gate 1; u20 cut §3.4): over a [`LoggedFs`] and an [`FsTicker`] on
    /// the `read-1a-4096` test fixture, one cycle of arm 3: before the first tick the setup has read the whole tail;
    /// each sample's reads between its ticks are not empty and are exactly [`Overlay::get_1a`]'s on a `MemLog` for the
    /// cycle's next node, the first at its newest frames' offset, never a 32-byte header; no open falls between the
    /// ticks; and the first reads' lsns are the node index's `lsn + off`, as a multiset.
    #[test]
    fn read_1a_arm_times_one_read_per_hop() {
        let fs = MemFs::with_dir(&scratch());
        let lfs = LoggedFs::new(&fs);
        let (q, p) = fixture(&lfs, Family::Read1a4096);
        let c = Columns::parse(p.map.bytes()).unwrap();
        let t = p.tail.as_ref().unwrap();
        let geo = t.expect.geo;
        let o = overlay_at(t);
        let ids = read_cycle(&o, p.seed);
        assert!(ids.len() > 1, "{} ids", ids.len());
        let ticker = FsTicker::new(&fs, p.dir(), &[]).logging(&lfs);
        let reads0 = fs.log().reads.len();
        let mut arms = arms(&p, &c, &q, &ticker).unwrap();
        assert_eq!(arms[3].name(), "read.1a");
        let mut firsts = Vec::new();
        let mut hops = 0;
        for k in 0..ids.len() {
            arms[3].sample().unwrap();
            let (t0, t1) = ticker.last();
            if k == 0 {
                assert_eq!(
                    lsn_cover(geo, &fs.log().reads[reads0..t0.reads]),
                    [(t.start, t.end)],
                    "the setup read the whole tail before the first tick"
                );
            }
            let w = ticker.timed_reads();
            assert!(!w.is_empty(), "sample {k}: no read between its ticks");
            let id = ids[(k + 1) % ids.len()];
            let mut mem = MemLog::new(t.start, &t.bytes);
            let mut stats = ReadStats::default();
            o.get_1a(&mut mem, id, &Key::Status, &mut Vec::new(), &mut stats)
                .unwrap();
            assert_eq!(w, as_store_reads(geo, &mem.reads), "sample {k}: #{id}");
            assert!(
                w.iter().all(|r| r.2 != REC_HDR_LEN),
                "sample {k}: a header read"
            );
            hops += stats.located;
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
        let mut want: Vec<u64> = o
            .nodes()
            .iter()
            .zip(o.at())
            .map(|(e, l)| e.lsn + u64::from(l.off))
            .collect();
        want.sort_unstable();
        firsts.sort_unstable();
        assert_eq!(
            firsts, want,
            "the cycle's first reads are the newest frames' offsets"
        );
        assert_eq!(ticker.count(), 2 * ids.len(), "two ticks per sample");
        let _ = hops;
    }

    /// The setup refuses a disagreeing read (spec Goal 1; u20 cut §3.4): [`read_1a_agree`] is `Ok` over four clean
    /// sources of an A1 tail; it refuses, naming the node, a changed `SetStatus` byte in the read through item 1a's
    /// source alone, and a locator whose `prev_len` is one byte longer in that source alone, which the read refuses and
    /// reads by the frame walk: the same value, one fallback.
    #[test]
    fn read_1a_setup_refuses_a_disagreeing_read() {
        let (t, b) = a1_tail_and_base(300, 400, 1);
        let c = Columns::parse(&b).unwrap();
        let o = overlay_at(&t);
        let idx = index_of(&t, &o);
        let ids = read_cycle(&o, 7);
        let agree = |item: &Vec<u8>| {
            read_1a_agree(
                &o,
                &idx,
                &c,
                &ids,
                &Key::Status,
                &mut MemLog::new(t.start, &t.bytes),
                &mut MemLog::new(t.start, &t.bytes),
                &mut MemLog::new(t.start, &t.bytes),
                &mut MemLog::new(t.start, item),
            )
        };
        assert_eq!(agree(&t.bytes), Ok(()));
        let newest: BTreeMap<u32, u64> = o.nodes().iter().map(|e| (e.n, e.lsn)).collect();
        let commits: Vec<(u64, &Commit<'_>)> = t
            .records
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(cm) => Some((r.lsn, &**cm)),
                _ => None,
            })
            .collect();
        // A SetStatus in its owner's newest record: its new status changed.
        let (n, at) = commits
            .iter()
            .find_map(|(lsn, cm)| {
                cm.ops.iter().find_map(|op| match op {
                    Op::SetStatus { id, .. } if newest[id] == *lsn => {
                        let (_, off, len) =
                            cm.op_spans().unwrap().into_iter().find(|s| s.0 == *id)?;
                        let end = (*lsn - t.start) as usize + REC_HDR_LEN + (off + len) as usize;
                        Some((*id, end - 2))
                    }
                    _ => None,
                })
            })
            .expect("a SetStatus in its owner's newest record");
        let mut bad = t.bytes.clone();
        bad[at] = (bad[at] + 1) % 4;
        let r = agree(&bad);
        assert!(
            r.as_ref().is_err_and(|e| e.starts_with(&format!("#{n}:"))),
            "{r:?}"
        );
        // A node whose status read hops: its newest record's locator, prev_len one byte longer.
        let (n, at) = commits
            .iter()
            .find_map(|(lsn, cm)| {
                cm.locs.iter().find_map(|(id, loc)| {
                    let own: Vec<&Op<'_>> = cm.ops.iter().filter(|o| o.owner() == *id).collect();
                    let sets = own.iter().any(|o| matches!(o, Op::SetStatus { .. }));
                    if newest[id] != *lsn || sets || *lsn - own[0].prev() < t.start || loc.len >= 127 {
                        return None;
                    }
                    let (_, off, _) = cm.op_spans().unwrap().into_iter().find(|s| s.0 == *id)?;
                    let frame = (*lsn - t.start) as usize + REC_HDR_LEN + off as usize;
                    let mut r = crate::tail::enc::Reader::new(&t.bytes[frame..]);
                    r.u8().ok()?;
                    r.uvar32().ok()?;
                    r.uvar32().ok()?;
                    r.uvar64().ok()?;
                    r.uvar32().ok()?;
                    let pos = r.pos();
                    (r.uvar32().ok()? == loc.len).then_some((*id, frame + pos))
                })
            })
            .expect("a node whose newest record does not set its status and whose chain stays in the tail");
        let mut longer = t.bytes.clone();
        longer[at] += 1;
        let r = agree(&longer);
        assert!(
            r.as_ref()
                .is_err_and(|e| e.starts_with(&format!("#{n}:")) && e.contains("fallbacks: 1")),
            "{r:?}"
        );
    }

    /// The setup refuses a hop that item 1a's read took unlocated (triage r1 I-2): over the hand-built A1 tail
    /// ([`crate::tail::testdata::hand_built_with`]; base N = 50) whose writer wrote (0, 0) for #3's `prev` into the
    /// tail, the whole-record read, the frame walk, the offset read and item 1a's read give #3's status alike, and item
    /// 1a's read refuses no locator; [`read_1a_agree`] over the base's nodes of the cycle still refuses, naming #3 and
    /// its one unlocated hop. Over the clean tail it is `Ok`.
    #[test]
    fn read_1a_setup_refuses_an_unlocated_hop() {
        use crate::tail::PrevLoc;
        use crate::tail::testdata::{CHAIN_IN, START, hand_built_with};
        let base = crate::overlay::Base {
            n: 50,
            kind: 1,
            statuses: 4,
        };
        let b = base::build(&base::Spec {
            n: base.n,
            upto_lsn: START,
            base_seq: BASE_SEQ,
        })
        .unwrap();
        let c = Columns::parse(&b).unwrap();
        for zero3 in [false, true] {
            let hb = hand_built_with(
                Locator::A1,
                &move |o: u32, loc: PrevLoc, _: &[(u32, PrevLoc)]| {
                    if zero3 && o == 3 {
                        PrevLoc::default()
                    } else {
                        loc
                    }
                },
            );
            let src = || MemLog::new(START, &hb.bytes);
            let end = START + hb.bytes.len() as u64;
            let o = Overlay::replay_at(
                &mut src(),
                &hb.expect,
                START,
                end,
                CHAIN_IN,
                base,
                TAIL_READ_CHUNK,
            )
            .unwrap();
            let idx = OpIndex::build(&o, &mut src(), TAIL_READ_CHUNK, &mut Vec::new()).unwrap();
            // The base's nodes: a status read of a node the tail created and the base lacks ends in "no base row".
            let mut ids = read_cycle(&o, 7);
            ids.retain(|&id| id <= base.n);
            assert!(ids.contains(&3), "#3 in the cycle");
            let r = read_1a_agree(
                &o,
                &idx,
                &c,
                &ids,
                &Key::Status,
                &mut src(),
                &mut src(),
                &mut src(),
                &mut src(),
            );
            if !zero3 {
                assert_eq!(r, Ok(()), "the clean tail");
                continue;
            }
            let w = read_by(
                ReadPath::Whole,
                &mut src(),
                &o,
                &idx,
                &c,
                3,
                &Key::Status,
                &mut Vec::new(),
                &mut ReadStats::default(),
            );
            assert!(w.is_ok(), "#3: {w:?}");
            let all = format!(
                "gives {w:?}, the frame walk {w:?}, the offset read {w:?}, item 1a {w:?} with"
            );
            assert!(
                r.as_ref().is_err_and(|e| e.starts_with("#3:")
                    && e.contains(&all)
                    && e.contains("unlocated: 1, fallbacks: 0 }")),
                "{r:?}"
            );
        }
    }

    /// The depth arm reads the hot node through d records (u20 cut §3.4; critique m-1): at production each depth arm's
    /// seed `seed(name) ^ k` takes the pinned [`Family::seed_k`], the first k < 64 whose tail holds at least d commits,
    /// and the hot node #500,000 is in exactly its first d; at the test profile (k = 0) #5,000 is in its first
    /// min(d, commits). Item 1a's read of the hot node's field 9 returns the first commit's value after d − 1 (or
    /// min(d, commits) − 1) hops through locators, none refused.
    #[test]
    fn depth_arms_read_the_hot_node_through_d_records() {
        let g = grid();
        let mut buf = Vec::new();
        for f in [
            Family::Read1aDepth4,
            Family::Read1aDepth16,
            Family::Read1aDepth64,
        ] {
            let q = g.iter().find(|q| q.family == f).unwrap();
            let d = f.depth().unwrap() as usize;
            for profile in [Profile::Production, Profile::Test] {
                let t = target(q, profile);
                let tail = generate(&t, true).unwrap();
                let hot = hot_node(t.n);
                let commits: Vec<&Commit<'_>> = tail
                    .records
                    .iter()
                    .filter_map(|r| match &r.payload {
                        Payload::Commit(c) => Some(&**c),
                        _ => None,
                    })
                    .collect();
                if profile == Profile::Production {
                    assert_eq!(hot, 500_000);
                    assert!(commits.len() >= d, "{}: {} commits", q.name, commits.len());
                    let k = f.seed_k();
                    assert_eq!(t.seed, seed(&q.name) ^ k, "{}", q.name);
                    for j in 0..k {
                        let mut tj = t;
                        tj.seed = seed(&q.name) ^ j;
                        let m = generate(&tj, false).unwrap().measures;
                        assert!(
                            m.commits < d as u64,
                            "{}: k = {j} already holds {} commits",
                            q.name,
                            m.commits
                        );
                    }
                }
                let records = d.min(commits.len());
                let with: Vec<bool> = commits
                    .iter()
                    .map(|c| c.ops.iter().any(|o| o.owner() == hot))
                    .collect();
                assert!(
                    with[..records].iter().all(|&w| w) && !with[records..].iter().any(|&w| w),
                    "{} {profile:?}: #{hot} in exactly the first {records} commits",
                    q.name
                );
                let o = overlay_at(&tail);
                let mut stats = ReadStats::default();
                let got = o.get_1a(
                    &mut MemLog::new(tail.start, &tail.bytes),
                    hot,
                    &Key::Field(HOT_FIELD),
                    &mut buf,
                    &mut stats,
                );
                assert!(matches!(got, Ok(Probe::Tail(Value::Field(_)))), "{got:?}");
                assert_eq!(
                    stats,
                    ReadStats {
                        located: records as u64 - 1,
                        unlocated: 0,
                        fallbacks: 0,
                    },
                    "{} {profile:?}",
                    q.name
                );
            }
        }
    }

    /// M-A5, item 1a's bytes per op (u20 cut D-10; `design-A.md:117` estimates 0.5–3 %): on the two production read tails
    /// in A1, (ops, `rec_bytes`, `loc_bytes`) are pinned. `read-1a-4096.n1e6`: 4,096 ops, 1,748,587 B of records of which
    /// 7,564 B are locators, a growth `loc_bytes / (rec_bytes − loc_bytes)` of 0.43 %, 1.85 B per op;
    /// `read-1a-1024.n1e6`: 1,024 ops, 379,323 B and 1,859 B, 0.49 %, 1.82 B per op. The mix's long commits carry most
    /// ops, one locator per touched node, so the growth sits at the low end of design-A's estimate.
    #[test]
    fn a1_bytes_per_op_are_pinned_on_the_read_tails() {
        let g = grid();
        let mut got = Vec::new();
        for f in [Family::Read1a4096, Family::Read1a1024] {
            let q = g.iter().find(|q| q.family == f).unwrap();
            let m = generate(&target(q, Profile::Production), false)
                .unwrap()
                .measures;
            got.push((m.ops, m.rec_bytes, m.loc_bytes));
        }
        assert_eq!(got, [(4_096, 1_748_587, 7_564), (1_024, 379_323, 1_859)]);
    }

    /// The calls of `lfs` from `from` to `to` on the file `name`.
    fn calls_on(lfs: &LoggedFs<'_>, from: usize, to: usize, name: &str) -> Vec<(usize, Io)> {
        lfs.calls()[from..to]
            .iter()
            .filter(|c| c.0 == name)
            .map(|c| (c.1, c.2))
            .collect()
    }
}
