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
//! # Measurement 10 (WP-53d)
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`m10`] | the grid of 18 quantities, the `replay` and `floor.open` arms, the run, the smoke, the list and the driver's command line (`moirai-probes-bin m10`) | [60 §5.2] item 10, [F17 §5], [MP §9.7] |
//! | [`overlay`] | the compact tail overlay: the node index, the ± lists, `ALLOC` and `UIDX`, the runtime index, the tail's symbols, and its charge `ovl(T)` | [AR §4.5] step 1, [F09 §4.6], [F11 §9], [F05 §10.4] |
//! | [`tail`] | the F05/F06 codec of a log tail of commits and settles: records, groups, placement, the `Commit` payload, the runtime records, the streaming scan | [F05], [F06], [F11 §12.4]–§12.5 |
//!
//! # Measurement 14 (WP-53e)
//!
//! | Module | What it holds | Specification |
//! |---|---|---|
//! | [`m14`] | the grid of 11 quantities: the layout probes (CSR, bitset AND/popcount, column scans, overlay probe) at 1e5 and 1e6, T1's point read and delta checkpoint, T2's MCP overlay catch-up model; the arms, the fixture, the smoke and the driver's command line (`moirai-probes-bin m14`) | [60 §5.2] item 14, `ARCHITECTURE-RESEARCH.md:130`, `:140`, [MP §9.7] |
//! | [`seg`] | the F09 segment builder: `SegHdr` and its section table, the 60-byte `NodeHdr`, cold columns, the forward and reverse CSR, frozen bitsets and ± lists, a synthetic base at N nodes | [F09], [F08 §3]–§4, `ARCHITECTURE-RESEARCH.md:1681` |
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
