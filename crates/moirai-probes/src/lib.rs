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
pub mod noise;
pub mod record;
pub mod run;
pub mod stats;
pub mod tier;
pub mod units;

#[cfg(test)]
mod testkit;
