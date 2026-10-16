//! The measurement-protocol framework and its statistics, the permanent layout micro-benchmarks, and the probe
//! bodies of measurements 1–16 and 18–22, generic over `V: Vfs` and `M: Meter`.
//!
//! Test-only library; checked by GT20 (e) on every target. It never depends on `moirai-os`: the composition root
//! `moirai-probes-bin` wires it to the Windows OS layer. Filled by WP-50 (the framework) and WP-51, WP-52 and WP-55
//! to WP-57 (R-HARN-I), and by WP-53a–e and WP-54 (R-HARN-M). Sources: [60 §5.1–§5.2],
//! `docs/spec/measurement-protocol.md`; `docs/m0/PLAN.md` §2.2, §6.2 R10, R17.
//!
//! `[MP §x]` cites `docs/spec/measurement-protocol.md`, the protocol that freezes [60 §5.1].
//!
//! # The framework (WP-50)
//!
//! | Module | What it holds | Protocol |
//! |---|---|---|
//! | [`units`] | sample units, the byte-quantity syntax, display forms | [MP §1.3], [MP §8.2] |
//! | [`stats`] | nearest-rank percentiles, per-repetition summaries, the median over repetitions | [MP §3.4] |
//! | [`tier`] | the tiers `t1`–`t4`, plans, block lengths | [MP §3.1–§3.3], [MP §4.3] |
//! | [`arm`] | arms, blocks, the timer and its resolution, batching, hyperfine exports | [MP §4.1], [MP §4.3–§4.5], [MP §4.9] |
//! | [`condition`] | idle, loaded and synthetic runs and the round-boundary memory check | [MP §2.3] |
//! | [`host`] | the Windows build and Defender snapshots | [MP §2.1–§2.2] |
//! | [`run`] | the pilot, the interleaved runner announcing each block, the tier check, process readings | [MP §3.2–§3.3], [MP §4.2–§4.6] |
//! | [`idle`] | the idle-CPU observation and its record, outside the tiered runner | [MP §4.7], [MP §7.1] |
//! | [`record`] | the run record, its JSON form, raw files, validity and exit grade | [MP §7.1–§7.3] |
//! | [`gate`] | absolute and floor-relative gates | [MP §5] |
//! | [`noise`] | noise bands and their record, baselines, the regression rule, the bands of noise runs (`moirai-probes-bin noise`) | [MP §6], [MP §9.7] |
//! | [`aggregate`] | the committed aggregate of a measurement | [MP §7.4] |
//! | [`guard`] | the pre-run guard's logic and command line (`moirai-probes-bin guard`) | [MP §8] |
//!
//! # Measurement 16 (WP-51)
//!
//! | Module | What it holds | Protocol |
//! |---|---|---|
//! | [`load`] | the load fixture's format, the load generator (`moirai-probes-bin loadgen`), its log and its verdict | [MP §9] |
//!
//! # Measurement 1 (WP-52)
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`m1`] | the CLI commit cycle, the close and the data-only-flush floor over the toy log, the smoke and the driver's command line (`moirai-probes-bin m1`); the store seam [`m1::store`] (the `Rig`, its toy implementation, the generations); the driver contract [`m1::driver`], shared with measurement 2 | [60 §5.2] item 1, `60-roadmap.md:788`, [MP §4.1], [MP §9.7] |
//!
//! # Measurement 2 (WP-52)
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`m2`] | 16 child writers in bursts over the toy log: the burst and its coordinator [`m2::burst`], its runtime checks, the measures (last acknowledgement, flushes, fairness, the flush-byte hand-off, T2's writer wait), the in-lock cost draw [`m2::cost`], the lock-logging `Vfs` [`m2::locks`], the child writers [`m2::child`], the smoke and the driver's command line (`moirai-probes-bin m2`) | [60 §5.2] item 2, `ARCHITECTURE-RESEARCH.md:1797-1799`, `:1947`, [MP §4.1], [MP §9.7] |
//!
//! # Measurement 6 (WP-54)
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`m6`] | the grid of 27 quantities (29 with the `zstd` CLI), every size first; the setup with its round trip of every measured input; the `raw` and `size` value arms and the batched `enc` and `dec` time arms; the smoke (on the build checkout's corpora when present) and the driver's command line (`moirai-probes-bin m6`); the corpora (the owner's notes through their manifest, the plan sections of `docs/**`, where a run finds them, their training and measured split), the probe's own raw-content dictionary trainer, the `hist` frames cut by F10 §4.2 from the tail generator, and the codecs (`lz4_flex` with and without the dictionary, `ruzstd`, the `zstd` CLI's stage) | [60 §5.2] item 6, `90-harness-agnostic-design.md:959-974`, [MP §9.7] |
//!
//! # Measurement 10 (WP-53d)
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`m10`] | the grid of 58 quantities in run order: the night's 18 (F17's candidates), one point of each family first, then the follow-up block of 13 below them (P06 2,048 and 1,024 at 1e4, smaller P07, P08 and P09 at 1e5, the quiet points at P06 2,048 and 1,024) by the same rule, two of which measure a tail of another size than their cap (R-44 I-2): `rec-256kib.n1e5` is one commit of 765,160 B, since a tail holds its first commit even past its cap, and `ovl-96kib.n1e5` stops at an overlay of 27,660 B, before a 16th commit that would take it to 99,992 B; then u20's block of design-A A3's 18 small tails in item 1a's A1 form, each production tail at its own cap (the first seed `seed(name) ^ k` that reaches it), with arm 0 `replay.at` building the node entries' offsets inside the timed replay; then 16 concurrent opens, an open after another process modified the log and one command's file opens, the first two on a pool of 17 `m10 --opener` children and its line protocol; then B-1, 16 readers of a 1-GiB or 256-MiB file just modified by the pool's modifier child or a Windows PowerShell child (a read/write view, or `WriteFile` as its control), each copy checked against the modification; the `replay` and `floor.open` arms, the run, the smoke, the list and the driver's command line (`moirai-probes-bin m10`) on the shared [MP §9.7] contract, with every fixture below the working directory | [60 §5.2] item 10, [F17 §5], [MP §9.7] |
//! | [`overlay`] | the compact tail overlay: the node index, the ± lists, `ALLOC` and `UIDX`, the runtime index, the tail's symbols, and its charge `ovl(T)`; the offsets mode, which builds each node's offset and length in its newest record during the replay (design-A's A2, no 255-frame cap); T1's op-locating reads ([`overlay::op`]): the frame walk, the offset read, and the read through item 1a (one read per hop at each record's locator, validated, the frame walk where it is refused) | [AR §4.5] step 1, [F09 §4.6], [F11 §9], [F05 §10.4]; [F06 §7.1], [F06 §7.9]; `design-A.md:135-144` |
//! | [`tail`] | the F05/F06 codec of a log tail of commits and settles: records, groups, placement, the `Commit` payload with item 1a's locator in the form a tail chooses (none, the night's; or design-A's A1, `prev_at` and `prev_len` in each owner's first frame, written by the generator from its state), the runtime records, the streaming scan; the `Checkpoint` record and its one-record group ([`tail::ckpt`]); the `HEAD` slot, its selection and its publish ([`tail::head`]) | [F04], [F05], [F06], [F11 §12.4]–§12.5; `design-A.md:104-129` (R-47, R-50) |
//!
//! # Measurement 14 (WP-53e)
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`m14`] | the grid of 25 quantities: the layout probes (CSR, bitset AND/popcount over dense sets and over the array paths of sparse ones, column scans with the split-status floor, overlay probe) at 1e5 and 1e6, T1's point read and delta checkpoint, T2's MCP overlay catch-up model, run with these three deciding quantities first, then T1's read diagnostics on the read's tail and nodes: the op-locating read with its offset floor, and the read's cost as two quantities of one arm each, the records walked and the bytes decoded (two counts of one read, neither a floor of the other; R-44); then, appended, T1's read with item 1a in an A1 tail at P06 4,096 and 1,024 by the whole record, the frame walk, the offset read and the locator, and its depth arm at 4, 16 and 64 records (u20; M-A1, R-50; u20's probe shaped like `ready` is retired, R-86 ruling 6); then, appended, T1's full delta checkpoint `ckpt-full-4096` at 0.5 M and 1e6 nodes (u21; M-A4, R-53 Q2), arm `ckpt.full` and its floor `ckpt.full.write`: every section of the window's delta over a full base, its `blobs` file, the `Checkpoint` group and the publish into the other `HEAD` slot. It is a lower bound in seven labelled parts: predicates other than `deleted` (u20 cut F-4); an ASCII tokenizer and untokenized bodies (F-5); no writer byte, re-write or scan (F-7); the probe's store-table sizes (F-8); raw random bodies, a failed compress (u21 cut F-12, whose bytes err the other way); no per-section split (F-16); no base `TERMS` or `POST` (F-17). It is an upper bound in one: `DOCLEN` and `FTSSTAT` (u20 cut F-6); the arms, the fixture, the run's touch of its mapped base before the pilot, the smoke and the driver's command line (`moirai-probes-bin m14`) on the shared [MP §9.7] contract, with every fixture below the working directory | [60 §5.2] item 14, [AR §2.1] (T1), [AR §2.2] (T2), [AR §8.1], [F04 §9.1], [F05 §9.9], [F10 §5], [MP §9.7] |
//! | [`seg`] | the F09 segment builder: `SegHdr` and its section table with F09 §3.3's `derived-optional` flags, the 60-byte `NodeHdr` with its reserved byte 3, cold columns with `PRIO` (R-66), the forward and reverse CSR, frozen bitsets and ± lists, a synthetic base at N nodes, with non-default priorities as a variant; the full base with titles, fields, uids and store tables; the full delta of every section ([`seg::full`]), F11's section bodies ([`seg::rt`]), the text sections ([`seg::text`]) and the `blobs` file ([`seg::blobs`]) | [F09], [F08 §3]–§4, [F10 §5], [F11 §2.1], [AR §8.1] |
//!
//! Every reading of the machine goes through `moirai_vfs::Meter` ([OS/README §4.3]); the framework never reads a
//! counter itself. The load generator is the one exception the protocol names: the system-wide totals it replays and
//! validates are read by `typeperf`, as the fixture was ([MP §9.4]). No measurement runs at M0 until the owner resumes them (`docs/m0/PLAN.md` §5).

pub mod aggregate;
pub mod arm;
pub mod condition;
pub mod gate;
pub mod guard;
pub mod host;
pub mod idle;
pub mod load;
pub mod m1;
pub mod m10;
pub mod m14;
pub mod m2;
pub mod m6;
pub mod noise;
pub mod overlay;
pub mod record;
pub mod run;
pub mod seg;
pub mod stats;
pub mod tail;
pub mod tier;
pub mod units;

#[cfg(test)]
mod testkit;
