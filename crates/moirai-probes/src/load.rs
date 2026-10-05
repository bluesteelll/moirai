//! Measurement 16's load fixture and its generator ([MP §9]; docs/m0/PLAN.md §3.2 WP-51; [60 §5.1] the Load row,
//! [60 §5.2] item 16).
//!
//! "Loaded" means the 16-agent load fixture — the system-wide counters of one normal 16-agent campaign of the owner,
//! recorded by `cargo xtask loadrec` (WP-51a) into `/private/load/` — replayed on the machine under test with
//! available physical memory held at 1.8 GB ([MP §2.3]). `moirai-probes-bin loadgen` is that replay: it drives the
//! machine's processor time and disk read and write bytes to the profile's, closing the loop on the machine's totals,
//! holds the memory, logs every second, and validates the replay against the profile ([MP §9.5]). The hosted runners'
//! synthetic load (`noise.yml`) is the same generator on a synthetic profile ([MP §9.6]), never the fixture.
//!
//! | Module | What it holds | Protocol |
//! |---|---|---|
//! | [`fixture`] | the fixture format, its decoder and the replayed profile | [MP §9.1], [MP §9.2] |
//! | [`synthetic`] | synthetic profiles | [MP §9.6] |
//! | [`sample`] | the live sampler (typeperf, four counters) | [MP §9.4] |
//! | [`control`] | the controller and the memory hold's step | [MP §9.4] |
//! | [`actuate`] | the load: CPU workers, disk writer and reader, memory holder | [MP §9.4] |
//! | [`replay`] | the replay loop and its log | [MP §9.4] |
//! | [`validate`] | the replay verdict over a run's window | [MP §9.5], [MP §2.3] |
//! | [`cli`] | `moirai-probes-bin loadgen` | [MP §9.4] |
//!
//! A measurement driver that records a loaded run reads the generator's log with [`validate::verdict_when_covered`]
//! over its run's window — it waits, at most [`validate::COVER_WAIT`], for the sample after the run's end — and records
//! the verdict with `RunRecord::set_load_replay`; [`validate::LogHeader::condition`] gives the run's condition
//! ([MP §9.7]).

pub mod actuate;
pub mod cli;
pub mod control;
pub mod fixture;
pub mod replay;
pub mod sample;
pub mod synthetic;
pub mod validate;
