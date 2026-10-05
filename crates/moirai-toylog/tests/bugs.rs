//! E4, second half (PLAN §3.2 WP-40, §7 E4; [F16 §17]): with each seeded-bug switch of the toy log on, the crash
//! enumerator of WP-32 reports a violation of a class the catalogue's "Detected by" cell names. One test per bug of
//! [`moirai_toylog::CATALOGUE`] — the 76 toy rows of [F16 §17.3] and L-6, L-7, L-8 of §17.4 ([F16] open point 1) — each
//! naming the scenario (`tests/common`), the dimension family and the plan mode that reach the bug. The rows whose
//! vehicle is not the toy (P-45 "none (masked)", P-59 and P-79 at M1) have no switch and no test here (spec sync 2b
//! S2B-P-41, S2B-P-44, S2B-P-45; owner question OQ-A-1).
//!
//! Every test runs by default: the PR tier's plans reach every bug (per-file prefixes plus one torn sector, or the two
//! pivots of every crash point, which the PR tier also enumerates). `MOIRAI_TEST_TIER=nightly` or `exit` widens the
//! plans.
//!
//! A test narrows the enumeration to the dimension family that reaches its bug ("a unit test of one dimension turns the
//! others off", `moirai_vfs_sim::enumerate::Dims`), and, where the two pivot states of every crash point already show
//! the bug, to those pivots. GT1's state minimum and the nightly release-delay obligation are enumeration-wide gates of
//! the clean suite, not violations a bug causes, so they are off here.
//!
//! Every test enumerates the same range of world seeds ([`seeds`]), not a seed picked for its bug, and passes when the
//! bug is detected on at least one of them.
//!
//! A test passes only when some failure falls in one of the bug's families ([`classify`], [`Bug::families`]: the
//! catalogue's "Detected by" cell, which since spec sync 2b S2B-P-46 carries the families the toy shows). A switch that
//! merely perturbs a run, or a violation of another class, does not count. Each failure is credited to its source: the
//! enumerator's generic detectors, or the toy's own checks ([`Source`]). Each test prints one `E4` line: the bug, the
//! scenario, the time, the failures per source and family, the seeds that detected it, whether the enumerator or only
//! the toy's own checks did, and the first failure of an expected family.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use moirai_toylog::{Bug, Bugs};
use moirai_vfs_sim::enumerate::{Dims, EnumConfig, PlanMode, Tier, enumerate};

/// The dimension family a test enumerates.
#[derive(Copy, Clone, Debug)]
enum Family {
    /// The discovery run alone: its recovery after the workload's end, with the crash states there.
    Discovery,
    /// Crash states at every crash point.
    Crash,
    /// A kill at every point.
    Kills,
    /// Disk-full at every write, flush, create and namespace operation.
    DiskFull,
    /// A failed flush at every file flush, then more commits and a crash.
    FlushErrors,
    /// Read errors, mapping faults and external truncation.
    ReadFaults,
}

impl Family {
    fn dims(self) -> Dims {
        Dims {
            crash_points: matches!(self, Family::Crash),
            kills: matches!(self, Family::Kills),
            disk_full: matches!(self, Family::DiskFull),
            flush_errors: matches!(self, Family::FlushErrors),
            read_faults: matches!(self, Family::ReadFaults),
        }
    }
}

/// The crash states per crash point.
#[derive(Copy, Clone, Debug)]
enum Plans {
    /// The two pivots (every non-clean sector at its baseline, or at its newest version).
    Pivots,
    /// The tier's own plans.
    Tier,
}

/// Who reported a failure message.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Source {
    /// The harness of WP-32 (`moirai_vfs_sim::enumerate`): the generic detectors of [F16 §17.2] — the ack, fresh, chain
    /// and avail verdicts, the trace predicates (I-G4, I-G6), the simulator's protocol-violation and namespace checks,
    /// and the seam's grant table.
    Enumerator,
    /// A check of the subject, which the enumerator reports with the prefix `subject: `: the toy's own `doctor
    /// --verify` (`verify`, `Toy::head_fold_problem`, `Toy::missing_pinned_files`), the read visibility of
    /// `checks::ReadWatch`, the kept-view check of `checks::KeptWatch`, and the harness's typing of a recovery's
    /// refusals. [F16 §17.2] makes the toy's own `doctor --verify` the `model` detector on the toy vehicle, and these
    /// checks are reviewed by R-HARN-S as a WP-40 acceptance step; the E4 table reports their detections in a column of
    /// their own.
    Toy,
}

/// The source and the assertion families ([F16 §17.2]) of a failure message, by the check that produced it.
///
/// The enumerator's messages:
/// - **ack**: a recovered state that lacks a required (acknowledged or observed) effect, or shows an operation in part;
/// - **fresh**: a first read after a crash that lacks a required effect or shows one in part; a recovered state that
///   lacks a value a reader observed (the ledger requires an observed value like an acknowledged one, so a crash that
///   takes one back is both ack and fresh); a read of a durable group no flush has covered (`fresh (I-G2)`); a first
///   read that a writer's recovery contradicts (`read freshness`);
/// - **model**: a read or recovered value no operation wrote (a phantom: the enumerator's comparison of a state with the
///   operations' effects), and a phantom observation (`read: `); a missing or partial effect is ack or fresh, never
///   model;
/// - **chain**: an acknowledged group's identity bytes no longer in place (`chain (I-G3)`);
/// - **trace**: the trace predicates (`trace:`), the simulator's protocol-violation checks (`simulator:`) and the seam's
///   lock-order refusals (`grant table:`);
/// - **ns**: a file that is not where the recovered state of its intent says (`ns:`), or a recovered file location
///   register that does not hold;
/// - **avail**: a refusal no injected fault explains — a workload operation's (`avail:`), the first read's or the
///   recovering writer's (`: refused: `), one the recovery answered with repair (`repair:`) — and a diagnosis of a
///   referenced file without a cause or one that misses a truncated file (`diagnosis:`).
///
/// The toy's own checks (`subject: `):
/// - `doctor --verify`'s I-G6 check of the published segment set: **trace**; its pinned-file check: **avail**; its other
///   findings (the invariants over the raw facts, the acknowledged groups' chain): **model**;
/// - `ReadWatch`'s reports (`read freshness`, `visibility`): **fresh** and **model** (a reader's view against the
///   operations acknowledged before it began);
/// - `KeptWatch`'s reports (`kept view`: a view a reader or a writer kept, after its refresh, against the replay of the
///   valid log up to the view's bound, the toy's own model of what the view must hold, [F16] P-56): **model**;
/// - a recovery that ended with a command's own refusal: **avail**.
fn classify(m: &str) -> (Source, Vec<&'static str>) {
    let mut f = Vec::new();
    if let Some(sub) = m.strip_prefix("subject: ") {
        if let Some(v) = sub.strip_prefix("doctor --verify: ") {
            f.push(if v.contains("I-G6") {
                "trace"
            } else if v.contains("pinned file") {
                "avail"
            } else {
                "model"
            });
        } else if sub.starts_with("read freshness") || sub.starts_with("visibility") {
            f.extend(["fresh", "model"]);
        } else if sub.starts_with("kept view") {
            f.push("model");
        } else if sub.starts_with("recovery: the command refused") {
            f.push("avail");
        }
        return (Source::Toy, f);
    }
    if m.contains(": refused: ")
        || m.starts_with("avail: ")
        || m.starts_with("repair: ")
        || m.starts_with("diagnosis: ")
    {
        f.push("avail");
        return (Source::Enumerator, f);
    }
    let state = m.starts_with("recovered state:");
    let first = m.starts_with("first read:");
    let missing = m.contains("required:") || m.contains("partly applied");
    if state && missing {
        f.push("ack");
    }
    if ((first || state) && missing)
        || m.starts_with("fresh (I-G2)")
        || m.starts_with("read freshness")
    {
        f.push("fresh");
    }
    if ((state || first) && m.contains("(a phantom)")) || m.starts_with("read: ") {
        f.push("model");
    }
    if m.starts_with("chain (I-G3)") {
        f.push("chain");
    }
    if m.starts_with("trace:") || m.starts_with("simulator:") || m.contains("grant table:") {
        f.push("trace");
    }
    if m.starts_with("ns: ") || ((state || first) && m.contains("kind: Other(2)")) {
        f.push("ns");
    }
    (Source::Enumerator, f)
}

/// The world seeds of a bug test in `tier`: one range for every bug, so no bug depends on a seed picked for it. A
/// different simulator RNG or scheduler changes which seeds of the range reach a bug, not whether one does; the test
/// passes when the bug is detected on at least one seed, and its `E4` line reports on which.
fn seeds(tier: Tier) -> Vec<u64> {
    match tier {
        Tier::Pr => (1..=24).collect(),
        Tier::Nightly => (1..=48).collect(),
        Tier::Exit => (1..=96).collect(),
    }
}

/// Enumerates `sc` with `bug` on, over the tier's seeds, and asserts that a check reported a violation of one of the
/// bug's families on at least one seed. The `E4` line gives the failures per source and family, the seeds that detected
/// the bug, and whether the enumerator itself detected it or only the toy's own checks did.
fn detect(bug: Bug, sc: common::Scenario, family: Family, plans: Plans) {
    let tier = Tier::from_env();
    let name = sc.name;
    let sub = common::ToySubject::new(sc, Bugs::only(bug));
    let all = seeds(tier);
    let mut cfg = EnumConfig::new(tier, all.iter().copied());
    cfg.dims = family.dims();
    if let Plans::Pivots = plans {
        cfg.limits.clean_plans = PlanMode::Pivots;
        cfg.limits.fault_plans = PlanMode::Pivots;
        cfg.limits.other_plans = PlanMode::Pivots;
    }
    cfg.limits.min_states = 0;
    cfg.limits.release_obligation = false;
    cfg.limits.failures_kept = 4_096;
    let t = Instant::now();
    let r = enumerate(&sub, &cfg);
    let secs = t.elapsed().as_secs_f64();
    let want = bug.families();
    let mut per: BTreeMap<(Source, &str), u64> = BTreeMap::new();
    let mut hit: BTreeMap<Source, String> = BTreeMap::new();
    let mut found_on: BTreeSet<u64> = BTreeSet::new();
    for f in &r.failures {
        for m in &f.messages {
            let (src, fams) = classify(m);
            for x in &fams {
                *per.entry((src, x)).or_insert(0) += 1;
            }
            if fams.is_empty() {
                *per.entry((src, "other")).or_insert(0) += 1;
            }
            if fams.iter().any(|x| want.contains(x)) {
                found_on.insert(f.seed);
                hit.entry(src)
                    .or_insert_with(|| format!("seed {} {:?} | {m}", f.seed, f.variant));
            }
        }
    }
    let by = |src: Source| -> BTreeMap<&str, u64> {
        per.iter()
            .filter(|((s, _), _)| *s == src)
            .map(|((_, x), n)| (*x, *n))
            .collect()
    };
    let detector = match (
        hit.contains_key(&Source::Enumerator),
        hit.contains_key(&Source::Toy),
    ) {
        (true, _) => "enumerator",
        (false, true) => "toy-own checks only",
        (false, false) => "none",
    };
    println!(
        "E4 {bug}: scenario {name}, {family:?} ({plans:?}), {secs:.1} s, {} failures, enumerator {:?}, toy-own {:?}, \
         expected {want:?}, detected by {detector} on {} of {} seeds {found_on:?}: {}",
        r.failures_total,
        by(Source::Enumerator),
        by(Source::Toy),
        found_on.len(),
        all.len(),
        hit.values().next().map_or("none", String::as_str)
    );
    assert!(r.failures_total > 0, "{bug} was not found:\n{r}");
    assert!(
        !hit.is_empty(),
        "{bug} was found only by checks outside its families {want:?}:\n{r}"
    );
}

macro_rules! bug_tests {
    ($( $test:ident: $bug:ident in $scenario:ident, $family:ident, $plans:ident; )*) => {
        $(
            #[test]
            fn $test() {
                detect(Bug::$bug, common::$scenario(), Family::$family, Plans::$plans);
            }
        )*

        /// Every bug of the catalogue has exactly one test.
        #[test]
        fn every_bug_has_one_test() {
            let tested = [$(Bug::$bug,)*];
            for b in Bug::ALL {
                assert_eq!(tested.iter().filter(|&&t| t == b).count(), 1, "{b}");
            }
            assert_eq!(tested.len(), moirai_toylog::N_BUGS);
        }
    };
}

bug_tests! {
    p01_g4_wait_flush_under_writer: P01G4WaitFlushUnderWriter in rotate, Discovery, Tier;
    p02_flush_under_writer: P02FlushUnderWriter in basic, Discovery, Tier;
    p03_kernel_path_second_client: P03KernelPathSecondClient in inproc, Discovery, Tier;
    p04_bump_without_writer: P04BumpWithoutWriter in bump, Discovery, Tier;
    p05_lease_tagged_lazy: P05LeaseTaggedLazy in leases, Discovery, Tier;
    p06_flush_only_last_extent: P06FlushOnlyLastExtent in rotate, Discovery, Tier;
    p07_lazy_publish_advances_durable: P07LazyPublishAdvancesDurable in server, Discovery, Tier;
    p08_append_before_extent_durable: P08AppendBeforeExtentDurable in rotate, Discovery, Tier;
    p09_group_across_extents: P09GroupAcrossExtents in rotate, Discovery, Tier;
    p10_checkpoint_before_segment_durable: P10CheckpointBeforeSegmentDurable in ckpt, Discovery, Tier;
    p12_publish_over_newest_slot: P12PublishOverNewestSlot in stale, Crash, Tier;
    p13_t9_single_slot_barrier: P13T9SingleSlotBarrier in flags, Crash, Tier;
    p14_t6_delete_before_barrier: P14T6DeleteBeforeBarrier in gc, Kills, Pivots;
    p15_boot_recovery_publish_before_flush: P15BootRecoveryPublishBeforeFlush in reboot, FlushErrors, Pivots;
    p16_rename_before_intent_durable: P16RenameBeforeIntentDurable in intents, DiskFull, Pivots;
    p17_t12_move_without_both_parents: P17T12MoveWithoutBothParents in intents, Discovery, Tier;
    p18_remove_before_parent_sync: P18RemoveBeforeParentSync in intents, FlushErrors, Pivots;
    p19_t14_roll_forward_without_barrier: P19T14RollForwardWithoutBarrier in intents, Kills, Pivots;
    p24_head_before_log_durable: P24HeadBeforeLogDurable in init_store, Crash, Pivots;
    p25_phase_one_under_writer: P25PhaseOneUnderWriter in basic, Discovery, Tier;
    p27_append_after_writer_timeout: P27AppendAfterWriterTimeout in timeouts, Kills, Pivots;
    p29_append_at_committed: P29AppendAtCommitted in reboot, Discovery, Tier;
    p30_g10_pending_into_overlay: P30G10PendingIntoOverlay in overlay, Kills, Pivots;
    p31_allocate_from_head_only: P31AllocateFromHeadOnly in basic, Discovery, Tier;
    p32_t4_idempotency_before_scan: P32T4IdempotencyBeforeScan in retry, Discovery, Tier;
    p33_g6_replay_before_durable: P33G6ReplayBeforeDurable in retry, Crash, Pivots;
    p34_candidate_stands_at_committed: P34CandidateStandsAtCommitted in leases, Discovery, Tier;
    p35_no_final_size_check: P35NoFinalSizeCheck in sizes, Discovery, Tier;
    p36_checkpoint_advances_hlc: P36CheckpointAdvancesHlc in ckpt, Discovery, Tier;
    p37_chain_seed_at_committed: P37ChainSeedAtCommitted in basic, Kills, Pivots;
    p38_g2_lazy_publish_past_durable: P38G2LazyPublishPastDurable in server, Kills, Pivots;
    p39_g13_lazy_stranded: P39G13LazyStranded in server, Kills, Pivots;
    p40_g1_covered_by_committed: P40G1CoveredByCommitted in basic, Crash, Pivots;
    p41_flush_timeout_acked: P41FlushTimeoutAcked in timeouts, Kills, Pivots;
    p42_g3_t5_flush_without_rewrite: P42G3T5FlushWithoutRewrite in basic, FlushErrors, Tier;
    p43_g8_rewrite_outside_writer: P43G8RewriteOutsideWriter in basic, Discovery, Tier;
    p44_flush_retried_and_acked: P44FlushRetriedAndAcked in basic, FlushErrors, Pivots;
    p46_g7_ack_by_position: P46G7AckByPosition in lost, FlushErrors, Pivots;
    p47_lg_reappend_old_bytes: P47LgReappendOldBytes in lost, FlushErrors, Pivots;
    p48_g11_stale_publish: P48G11StalePublish in admin, Discovery, Tier;
    p49_committed_above_valid_end: P49CommittedAboveValidEnd in lazytail, Kills, Pivots;
    p50_g12_checkpoint_set_unpublished: P50G12CheckpointSetUnpublished in ckpt, Discovery, Tier;
    p51_maintenance_under_writer: P51MaintenanceUnderWriter in auto, Discovery, Tier;
    p52_t10_marker_in_own_group: P52T10MarkerInOwnGroup in leases, Crash, Pivots;
    p53_g9_no_chain_check: P53G9NoChainCheck in lost, DiskFull, Pivots;
    p54_t13_no_position_check: P54T13NoPositionCheck in misplaced, Discovery, Tier;
    p55_no_epoch_check: P55NoEpochCheck in foreign, Discovery, Tier;
    p56_overlay_kept_after_refill: P56OverlayKeptAfterRefill in refill, FlushErrors, Pivots;
    p57_t3_reader_past_committed: P57T3ReaderPastCommitted in basic, Discovery, Tier;
    p58_corruption_as_end_of_view: P58CorruptionAsEndOfView in reads, ReadFaults, Pivots;
    p60_t8_no_boot_check: P60T8NoBootCheck in boot, Discovery, Tier;
    p61_fatal_slot_skipped: P61FatalSlotSkipped in fatal, Crash, Tier;
    p62_g12_barrier_before_checkpoint: P62G12BarrierBeforeCheckpoint in barrier, Crash, Tier;
    p63_flag_reported_before_flush: P63FlagReportedBeforeFlush in admin, Crash, Tier;
    p64_t7_scan_from_committed: P64T7ScanFromCommitted in lazytail, Discovery, Tier;
    p65_t11_skip_non_commit_record: P65T11SkipNonCommitRecord in leases, Discovery, Tier;
    p66_boot_recovery_without_rewrite: P66BootRecoveryWithoutRewrite in boot, Kills, Pivots;
    p67_unknown_boot_writes_boot_id: P67UnknownBootWritesBootId in boot, Discovery, Tier;
    p69_t2_ref_move_in_later_group: P69T2RefMoveInLaterGroup in import, Crash, Pivots;
    p70_failed_cas_moves_ref: P70FailedCasMovesRef in import, Discovery, Tier;
    p71_unknown_anchor_as_dead: P71UnknownAnchorAsDead in intents_live, Kills, Pivots;
    p72_rotate_under_writer_only: P72RotateUnderWriterOnly in rotate, Discovery, Tier;
    p73_retire_at_boundary: P73RetireAtBoundary in boundary, Discovery, Tier;
    p74_t1_reuse_retired_extent: P74T1ReuseRetiredExtent in reuse, Discovery, Tier;
    p76_maintenance_without_byte: P76MaintenanceWithoutByte in maint2, Discovery, Tier;
    p77_delete_pinned_file: P77DeletePinnedFile in fork, Discovery, Tier;
    p81_pin_in_later_group: P81PinInLaterGroup in fork, Crash, Pivots;
    p82_write_through_without_dir_flush: P82WriteThroughWithoutDirFlush in trash, Discovery, Tier;
    p83_cross_volume_copy: P83CrossVolumeCopy in xvol, Discovery, Tier;
    p88_init_keeps_ref_id_zero: P88InitKeepsRefIdZero in init_store, Discovery, Tier;
    p89_lease_on_wall_clock: P89LeaseOnWallClock in clock, Discovery, Tier;
    p90_df_ack_after_disk_full: P90DfAckAfterDiskFull in trash, DiskFull, Pivots;
    p91_flush_error_as_success: P91FlushErrorAsSuccess in basic, DiskFull, Pivots;
    p92_read_error_as_end: P92ReadErrorAsEnd in reboot, ReadFaults, Pivots;
    p96_spare_without_flushes: P96SpareWithoutFlushes in spare, Kills, Pivots;
    p97_no_extent_head: P97NoExtentHead in rotate, Discovery, Tier;
    l06_grant_to_two_waiters: L06GrantToTwoWaiters in inproc, Discovery, Tier;
    l07_late_grant_leaked: L07LateGrantLeaked in timeouts, Kills, Pivots;
    l08_probe_first_quiet_only: L08ProbeFirstQuietOnly in quiet, Discovery, Tier;
}
