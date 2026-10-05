//! What an enumeration did and found: the state counts the nightly tier reports ([`Report`]'s `Display`), and every
//! failure with what replays it.

use std::collections::BTreeMap;
use std::time::Duration;

use super::Tier;
use super::adversary::PoisonPolicy;
use super::plans::Dim;
use crate::adversary::{PartialWrite, Site};
use crate::crash::CrashPlan;
use crate::world::{CallKind, DeathPlan, PointInfo};

/// Which run a failure came from: the fault the run injected, if any. The same seed and variant replay it.
#[derive(Clone, Debug)]
pub enum Variant {
    /// No injected fault.
    Clean,
    /// Process `victim` killed at the point `at` ([F15 §2.5] "Process death"; FM-11.2 for a flush holder): its own point,
    /// or another process's point while it was inside a flush or a lock wait.
    Kill {
        /// The point.
        at: PointInfo,
        /// The process killed.
        victim: u32,
        /// How its in-flight calls and its lock bytes resolved.
        plan: DeathPlan,
    },
    /// Disk-full at the `nth` choice of `site` (FM-5).
    DiskFull {
        /// The fault site.
        site: Site,
        /// Its 0-based occurrence in the run.
        nth: u64,
        /// The site's answer (for `CreateFault`, 1 leaves the name absent and 2 an empty file).
        value: u64,
        /// For a write, how much of it applied (FM-5.2).
        partial: Option<PartialWrite>,
    },
    /// A failed flush (`Io`) at the `nth` file flush, then the rest of the workload (FM-3, FM-11.3).
    FlushError {
        /// Its 0-based occurrence among file flushes.
        nth: u64,
        /// How reads of the poisoned sectors choose.
        poison: PoisonPolicy,
    },
    /// A read error at the `nth` read (`ReadFault`: 1 transient, 2 persistent, FM-12) or a media fault under the `nth`
    /// mapping (`MapFault`, FM-9.1).
    ReadFault {
        /// The fault site.
        site: Site,
        /// Its 0-based occurrence in the run.
        nth: u64,
        /// The site's answer.
        value: u64,
    },
    /// A sealed file truncated by an external actor (FM-10.1, FM-10.2).
    Truncated {
        /// The file.
        path: std::path::PathBuf,
        /// Its new length.
        len: u64,
        /// The scheduling point of the truncation; `None`: after the workload.
        at: Option<u64>,
    },
}

/// A system crash a failure was found after.
#[derive(Clone, Debug)]
pub struct CrashCase {
    /// The crash point; `None` after the workload's end.
    pub at: Option<PointInfo>,
    /// The dimension of the crash state.
    pub dim: Dim,
    /// The crash state.
    pub plan: CrashPlan,
}

/// One failed check.
#[derive(Clone, Debug)]
pub struct Failure {
    /// The world seed.
    pub seed: u64,
    /// The run.
    pub variant: Variant,
    /// The system crash, if the check followed one (else it followed the run's process deaths only, or it is a problem
    /// of the run itself).
    pub crash: Option<CrashCase>,
    /// What failed.
    pub messages: Vec<String>,
}

impl core::fmt::Display for Failure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "seed {} {:?}", self.seed, self.variant)?;
        match &self.crash {
            Some(c) => write!(
                f,
                ", crash at {} ({:?}): {:?}",
                c.at.map_or_else(
                    || "the end".to_owned(),
                    |p| format!("point {} {:?}/{}", p.point, p.call, p.phase)
                ),
                c.dim,
                c.plan
            )?,
            None => write!(f, ", no system crash")?,
        }
        for m in &self.messages {
            write!(f, "\n  - {m}")?;
        }
        Ok(())
    }
}

/// The counts and failures of one enumeration.
#[derive(Clone, Debug)]
pub struct Report {
    /// The tier.
    pub tier: Tier,
    /// Seeds enumerated.
    pub seeds: u64,
    /// Workload runs (discovery, capture, kill and fault runs).
    pub runs: u64,
    /// Crash points (captured images) enumerated.
    pub crash_points: u64,
    /// Crash points per call.
    pub crash_points_by_call: BTreeMap<CallKind, u64>,
    /// Crash points at a write to a slot file (a `HEAD` publish).
    pub publish_points: u64,
    /// Crash states recovered and checked, per dimension.
    pub states: BTreeMap<Dim, u64>,
    /// The most crash states of each dimension checked at one crash point.
    pub point_max: BTreeMap<Dim, u64>,
    /// The most crash states checked at one crash point.
    pub point_max_total: u64,
    /// Crash states, per dimension, that mix the candidates of a poisoned sector within the sector (FM-3.3).
    pub poison_mixed: BTreeMap<Dim, u64>,
    /// Checks after process deaths without a system crash (the end of every run).
    pub death_checks: u64,
    /// Recoveries run.
    pub recoveries: u64,
    /// Kill runs.
    pub kills: u64,
    /// Kill runs inside a flush, by outcome: some member succeeded, failed, not performed (FM-11.2).
    pub kills_in_flush: [u64; 3],
    /// Kill runs inside a `sync_group` (or several flushes of one process) whose members resolved differently (§2.5).
    pub kills_mixed_group: u64,
    /// Kill runs of a process inside a flush or a lock wait at another process's point.
    pub kills_of_holders: u64,
    /// Kill runs followed by crash points: of a flush holder (the next holder adopting its group, [80 §2.4.4]), and of a
    /// process whose death cut a write to a slot file (the next publisher writing over the cut slot, S2B-P-27).
    pub kills_with_crash_points: u64,
    /// Crash points after those deaths.
    pub crash_points_after_kills: u64,
    /// The release-delay classes the runs' deaths drew: (a) measured, (b) beyond every wait bound, (c) never (FM-8.1).
    pub release_classes: [u64; 3],
    /// Disk-full runs per site.
    pub disk_full: BTreeMap<Site, u64>,
    /// Failed-flush runs.
    pub flush_errors: u64,
    /// Reads of poisoned sub-sectors answered by a poison policy or the seeded adversary.
    pub poisoned_reads: u64,
    /// Read-fault and mapping-fault runs per site.
    pub read_faults: BTreeMap<Site, u64>,
    /// External-truncation runs.
    pub truncations: u64,
    /// Recoveries after which a truncated sealed file had to be named by the diagnosis.
    pub diagnoses: u64,
    /// Refusals of recoveries (first reads, writers, refusals answered by repair) that the runs' faults explain (module
    /// `refusal`).
    pub refusals: u64,
    /// Recoveries whose first read an Unknown-boot reader made (judged for consistency only, [F13 §3.8]).
    pub unknown_boot_reads: u64,
    /// Disk-full and death runs that cut a write to a slot file; the end of each takes the slot states in every tier
    /// ([F04 §8.1] "Both slots absent", [F15 §6.4]).
    pub slot_fault_runs: u64,
    /// Successful whole-slot writes the trace predicates judged against the newest valid slot before them (I-G6).
    pub publishes_judged: u64,
    /// Acknowledged groups' identity bytes checked in recovered worlds (chain, I-G3).
    pub chain_checks: u64,
    /// Workload refusals judged against their runs' faults (avail).
    pub workload_refusals: u64,
    /// Failed checks in all.
    pub failures_total: u64,
    /// The first failures, with what replays them.
    pub failures: Vec<Failure>,
    /// Wall time.
    pub elapsed: Duration,
    /// Why the enumeration stopped early (its time budget), if it did: a failure of the tier.
    pub incomplete: Option<String>,
}

impl Report {
    pub(crate) fn new(tier: Tier) -> Report {
        Report {
            tier,
            seeds: 0,
            runs: 0,
            crash_points: 0,
            crash_points_by_call: BTreeMap::new(),
            publish_points: 0,
            states: BTreeMap::new(),
            point_max: BTreeMap::new(),
            point_max_total: 0,
            poison_mixed: BTreeMap::new(),
            death_checks: 0,
            recoveries: 0,
            kills: 0,
            kills_in_flush: [0; 3],
            kills_mixed_group: 0,
            kills_of_holders: 0,
            kills_with_crash_points: 0,
            crash_points_after_kills: 0,
            release_classes: [0; 3],
            disk_full: BTreeMap::new(),
            flush_errors: 0,
            poisoned_reads: 0,
            read_faults: BTreeMap::new(),
            truncations: 0,
            diagnoses: 0,
            refusals: 0,
            unknown_boot_reads: 0,
            slot_fault_runs: 0,
            publishes_judged: 0,
            chain_checks: 0,
            workload_refusals: 0,
            failures_total: 0,
            failures: Vec::new(),
            elapsed: Duration::ZERO,
            incomplete: None,
        }
    }

    /// No check failed and the enumeration completed.
    pub fn passed(&self) -> bool {
        self.failures_total == 0 && self.incomplete.is_none()
    }

    /// Crash states checked in all.
    pub fn states_total(&self) -> u64 {
        self.states.values().sum()
    }

    /// Panics with the report if a check failed or the enumeration stopped early.
    pub fn assert_passed(&self) {
        assert!(self.passed(), "crash enumeration failed:\n{self}");
    }
}

/// `Kind n, Kind n, …` of a count map.
fn counts<K: core::fmt::Debug>(m: &BTreeMap<K, u64>) -> String {
    m.iter()
        .map(|(k, n)| format!("{k:?} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl core::fmt::Display for Report {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(
            f,
            "crash enumeration, tier {:?}: {} seeds, {} runs, {:.1} s",
            self.tier,
            self.seeds,
            self.runs,
            self.elapsed.as_secs_f64()
        )?;
        writeln!(
            f,
            "  crash points {} ({} publishes): {}",
            self.crash_points,
            self.publish_points,
            counts(&self.crash_points_by_call)
        )?;
        writeln!(
            f,
            "  crash states {}: {}",
            self.states_total(),
            counts(&self.states)
        )?;
        writeln!(
            f,
            "  most at one crash point {}: {}; mixing a poisoned sector: {}",
            self.point_max_total,
            counts(&self.point_max),
            counts(&self.poison_mixed)
        )?;
        writeln!(
            f,
            "  recoveries {}, death-only checks {}, kills {} (in a flush: succeeded {}, failed {}, not performed {}; \
             members apart {}; busy holders {}; followed by {} crash points in {} runs)",
            self.recoveries,
            self.death_checks,
            self.kills,
            self.kills_in_flush[0],
            self.kills_in_flush[1],
            self.kills_in_flush[2],
            self.kills_mixed_group,
            self.kills_of_holders,
            self.crash_points_after_kills,
            self.kills_with_crash_points
        )?;
        writeln!(
            f,
            "  release classes drawn: (a) {}, (b) {}, (c) {}",
            self.release_classes[0], self.release_classes[1], self.release_classes[2]
        )?;
        writeln!(
            f,
            "  disk-full runs: {}; failed-flush runs {}; poisoned reads {}",
            counts(&self.disk_full),
            self.flush_errors,
            self.poisoned_reads
        )?;
        writeln!(
            f,
            "  read-fault runs: {}; truncation runs {} ({} diagnoses checked); allowed refusals {}",
            counts(&self.read_faults),
            self.truncations,
            self.diagnoses,
            self.refusals
        )?;
        writeln!(
            f,
            "  runs with a cut slot write {}; first reads by an Unknown-boot reader {}",
            self.slot_fault_runs, self.unknown_boot_reads
        )?;
        writeln!(
            f,
            "  publishes judged {}; acknowledged-group identities checked {}; workload refusals judged {}",
            self.publishes_judged, self.chain_checks, self.workload_refusals
        )?;
        write!(f, "  failures {}", self.failures_total)?;
        if let Some(why) = &self.incomplete {
            write!(f, "\n  INCOMPLETE: {why}")?;
        }
        for fl in &self.failures {
            write!(f, "\n{fl}")?;
        }
        Ok(())
    }
}
