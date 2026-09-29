//! E4, second half (PLAN §3.2 WP-40, §7 E4; [F16 §17]): with each seeded-bug switch of the toy log on, the crash
//! enumerator of WP-32 reports a violation of the class the catalogue's "Detected by" column names. One test per bug of
//! [`moirai_toylog::CATALOGUE`], each naming the scenario (`tests/common`), the dimension family and the plan mode that
//! reach the bug.
//!
//! - **Reached** bugs run by default (the PR tier's plans reach every one of them: per-file prefixes plus one torn
//!   sector, or their two pivots alone, which the PR tier also enumerates). `MOIRAI_TEST_TIER=nightly` or `exit` widens
//!   the plans.
//! - **Open** E4 items ([`moirai_toylog::OPEN`]) are `#[ignore]`d: the toy masks each of them by another rule it keeps,
//!   so no enumeration reaches it, and each awaits the specification's disposition of its catalogue row. `cargo test -p
//!   moirai-toylog --test bugs -- --ignored` runs them (they fail until then).
//!
//! A test narrows the enumeration to the dimension family that reaches its bug ("a unit test of one dimension turns the
//! others off", `moirai_vfs_sim::enumerate::Dims`), and, where the two pivot states of every crash point already show
//! the bug, to those pivots. GT1's state minimum and the nightly release-delay obligation are enumeration-wide gates of
//! the clean suite, not violations a bug causes, so they are off here.
//!
//! A test passes only when some failure falls in one of the bug's families ([`families`]): the catalogue's, and for a bug
//! of [`moirai_toylog::TOY_DETECTION`] the families that row proposes, which await the specification's disposition. A
//! switch that merely perturbs a run, or a violation of another class, does not count. Each test prints one `E4` line:
//! the bug, the scenario, the time, the failures per family and the first failure of an expected family, marked
//! `[proposed class]` when no failure falls in a catalogue family.

mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use moirai_toylog::{Bug, Bugs, OPEN};
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

/// The assertion families ([F16 §17.2]) a failure message belongs to, by the check that produced it:
/// - **ack**: a recovered state that lacks an acknowledged effect, or shows an acknowledged operation in part;
/// - **fresh**: a first read after a crash that lacks an acknowledged effect or that a writer's recovery contradicts; a
///   read of a durable group no flush has covered (a view that shows it, or a value a reader observed that a crash then
///   takes back: the ledger requires an observed value like an acknowledged one, so a recovered state that lacks one is
///   both); a reader's view that lacks a commit or a lazy row acknowledged before it began;
/// - **chain**: an acknowledged group no longer behind the predecessor it was validated against (I-G3);
/// - **trace**: a trace predicate (I-G4, I-G6), the simulator's protocol-violation checks ([F15 §3.13]), the seam's
///   lock-order refusals, and `doctor --verify`'s I-G6 check of the published segment set;
/// - **model**: the harness's model of the store — the ledger's comparisons of a read or recovered state with the
///   operations' effects (a missing, partial or phantom value), a reader's view against the operations acknowledged
///   before it began — and `doctor --verify`'s invariants over the raw facts;
/// - **ns**: a closed intent whose file is not where it says, or a file location register that does not hold;
/// - **avail**: a refusal of a store the protocol must accept, a pinned file that is missing, an operation that ended
///   `outcome_unknown` in a run with no fault that can lose a group.
fn families(m: &str) -> Vec<&'static str> {
    let mut f = Vec::new();
    if m.contains(": refused: ") || m.starts_with("subject: avail") || m.contains("pinned file") {
        f.push("avail");
        return f;
    }
    let state = m.starts_with("recovered state:");
    let first = m.starts_with("first read:");
    let missing = m.contains("required:") || m.contains("partly applied");
    if state && missing {
        f.push("ack");
    }
    if ((first || state) && missing)
        || m.starts_with("read freshness")
        || m.starts_with("subject: read freshness")
        || m.starts_with("subject: visibility")
    {
        f.push("fresh");
    }
    if state
        || first
        || m.contains("does not show it")
        || (m.starts_with("subject: doctor --verify:") && !m.contains("I-G6"))
    {
        f.push("model");
    }
    if m.starts_with("subject: I-G3") {
        f.push("chain");
    }
    if m.starts_with("trace:")
        || m.starts_with("simulator:")
        || m.contains("grant table:")
        || (m.starts_with("subject: doctor --verify:") && m.contains("I-G6"))
    {
        f.push("trace");
    }
    if m.starts_with("subject: ns:") || m.contains("kind: Other(2)") {
        f.push("ns");
    }
    f
}

/// Enumerates `sc` with `bug` on and asserts that the enumerator reported a violation of one of the bug's families.
fn detect(bug: Bug, sc: common::Scenario, family: Family, plans: Plans, seeds: &[u64]) {
    let tier = Tier::from_env();
    let name = sc.name;
    let sub = common::ToySubject::new(sc, Bugs::only(bug));
    let mut cfg = EnumConfig::new(tier, seeds.iter().copied());
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
    let want = bug.accepted_families();
    let catalogue = bug.families();
    let mut per: BTreeMap<&str, u64> = BTreeMap::new();
    let mut hit: Option<String> = None;
    let mut in_catalogue = false;
    for f in &r.failures {
        for m in &f.messages {
            let fams = families(m);
            for x in &fams {
                *per.entry(x).or_insert(0) += 1;
            }
            if fams.is_empty() {
                *per.entry("other").or_insert(0) += 1;
            }
            if hit.is_none() && fams.iter().any(|x| want.contains(x)) {
                hit = Some(format!("{:?} | {m}", f.variant));
            }
            in_catalogue |= fams.iter().any(|x| catalogue.contains(x));
        }
    }
    let mark = if hit.is_some() && !in_catalogue {
        " [proposed class]"
    } else {
        ""
    };
    println!(
        "E4 {bug}: scenario {name}, {family:?} ({plans:?}), {secs:.1} s, {} failures, by family {per:?}, expected {want:?}{mark}: {}",
        r.failures_total,
        hit.as_deref().unwrap_or("none")
    );
    assert!(r.failures_total > 0, "{bug} was not found:\n{r}");
    assert!(
        hit.is_some(),
        "{bug} was found only by checks outside its families {want:?}:\n{r}"
    );
}

macro_rules! bug_tests {
    (
        reached { $( $test:ident: $bug:ident in $scenario:ident, $family:ident, $plans:ident, $seeds:expr; )* }
        open { $( $otest:ident: $obug:ident in $oscenario:ident, $ofamily:ident, $oplans:ident, $oseeds:expr; )* }
    ) => {
        $(
            #[test]
            fn $test() {
                detect(Bug::$bug, common::$scenario(), Family::$family, Plans::$plans, &$seeds);
            }
        )*
        $(
            #[test]
            #[ignore = "open E4 item: masked in the toy by another rule it keeps (moirai_toylog::OPEN names it); awaits the specification's disposition of its catalogue row"]
            fn $otest() {
                detect(Bug::$obug, common::$oscenario(), Family::$ofamily, Plans::$oplans, &$oseeds);
            }
        )*

        /// Every bug of the catalogue has exactly one test.
        #[test]
        fn every_bug_has_one_test() {
            let tested = [$(Bug::$bug,)* $(Bug::$obug,)*];
            for b in Bug::ALL {
                assert_eq!(tested.iter().filter(|&&t| t == b).count(), 1, "{b}");
            }
            assert_eq!(tested.len(), moirai_toylog::N_BUGS);
        }

        /// The ignored tests are exactly the open E4 items.
        #[test]
        fn the_ignored_tests_are_the_open_items() {
            let mut ignored = vec![$(Bug::$obug),*];
            let mut open: Vec<Bug> = OPEN.iter().map(|(b, _)| *b).collect();
            ignored.sort_unstable();
            open.sort_unstable();
            assert_eq!(ignored, open);
        }
    };
}

bug_tests! {
    reached {
        p01_g4_wait_flush_under_writer: P01G4WaitFlushUnderWriter in rotate, Discovery, Tier, [1];
        p02_flush_under_writer: P02FlushUnderWriter in basic, Discovery, Tier, [1];
        p03_kernel_path_second_client: P03KernelPathSecondClient in inproc, Discovery, Tier, [4];
        p04_bump_without_writer: P04BumpWithoutWriter in bump, Discovery, Tier, [1, 2];
        p05_lease_tagged_lazy: P05LeaseTaggedLazy in leases, Discovery, Tier, [1];
        p06_flush_only_last_extent: P06FlushOnlyLastExtent in rotate, Discovery, Tier, [1];
        p07_lazy_publish_advances_durable: P07LazyPublishAdvancesDurable in server, Discovery, Tier, [1];
        p08_append_before_extent_durable: P08AppendBeforeExtentDurable in rotate, Discovery, Tier, [1];
        p09_group_across_extents: P09GroupAcrossExtents in rotate, Discovery, Tier, [1];
        p10_checkpoint_before_segment_durable: P10CheckpointBeforeSegmentDurable in ckpt, Discovery, Tier, [1];
        p12_publish_over_newest_slot: P12PublishOverNewestSlot in stale, Discovery, Tier, [1];
        p13_t9_single_slot_barrier: P13T9SingleSlotBarrier in flags, Crash, Tier, [1];
        p14_t6_delete_before_barrier: P14T6DeleteBeforeBarrier in gc, Kills, Pivots, [1];
        p15_boot_recovery_publish_before_flush: P15BootRecoveryPublishBeforeFlush in reboot, FlushErrors, Pivots, [1];
        p16_rename_before_intent_durable: P16RenameBeforeIntentDurable in intents, DiskFull, Pivots, [1];
        p17_t12_move_without_both_parents: P17T12MoveWithoutBothParents in intents, Discovery, Tier, [1];
        p18_remove_before_parent_sync: P18RemoveBeforeParentSync in intents, FlushErrors, Pivots, [1];
        p19_t14_roll_forward_without_barrier: P19T14RollForwardWithoutBarrier in intents, Kills, Pivots, [1];
        p24_head_before_log_durable: P24HeadBeforeLogDurable in init_store, Crash, Pivots, [1];
        p25_phase_one_under_writer: P25PhaseOneUnderWriter in basic, Discovery, Tier, [1];
        p27_append_after_writer_timeout: P27AppendAfterWriterTimeout in timeouts, Kills, Pivots, [2];
        p29_append_at_committed: P29AppendAtCommitted in reboot, Discovery, Tier, [1];
        p30_g10_pending_into_overlay: P30G10PendingIntoOverlay in overlay, Kills, Pivots, [3];
        p31_allocate_from_head_only: P31AllocateFromHeadOnly in basic, Discovery, Tier, [1];
        p32_t4_idempotency_before_scan: P32T4IdempotencyBeforeScan in retry, Discovery, Tier, [1];
        p33_g6_replay_before_durable: P33G6ReplayBeforeDurable in retry, Crash, Pivots, [1];
        p34_candidate_stands_at_committed: P34CandidateStandsAtCommitted in leases, Discovery, Tier, [1];
        p35_no_final_size_check: P35NoFinalSizeCheck in sizes, Discovery, Tier, [1];
        p36_checkpoint_advances_hlc: P36CheckpointAdvancesHlc in ckpt, Discovery, Tier, [1];
        p37_chain_seed_at_committed: P37ChainSeedAtCommitted in basic, Kills, Pivots, [1];
        p38_g2_lazy_publish_past_durable: P38G2LazyPublishPastDurable in server, Kills, Pivots, [1];
        p39_g13_lazy_stranded: P39G13LazyStranded in server, Kills, Pivots, [1];
        p40_g1_covered_by_committed: P40G1CoveredByCommitted in basic, Crash, Pivots, [1];
        p41_flush_timeout_acked: P41FlushTimeoutAcked in timeouts, Kills, Pivots, [4];
        p42_g3_t5_flush_without_rewrite: P42G3T5FlushWithoutRewrite in basic, FlushErrors, Tier, [2];
        p43_g8_rewrite_outside_writer: P43G8RewriteOutsideWriter in basic, Discovery, Tier, [1];
        p44_flush_retried_and_acked: P44FlushRetriedAndAcked in basic, FlushErrors, Pivots, [8];
        p46_g7_ack_by_position: P46G7AckByPosition in lost, FlushErrors, Pivots, [6];
        p47_lg_reappend_old_bytes: P47LgReappendOldBytes in lost, FlushErrors, Pivots, [6];
        p48_g11_stale_publish: P48G11StalePublish in admin, Discovery, Tier, [1];
        p49_committed_above_valid_end: P49CommittedAboveValidEnd in lazytail, Kills, Pivots, [1];
        p50_g12_checkpoint_set_unpublished: P50G12CheckpointSetUnpublished in ckpt, Discovery, Tier, [1];
        p51_maintenance_under_writer: P51MaintenanceUnderWriter in auto, Discovery, Tier, [1];
        p52_t10_marker_in_own_group: P52T10MarkerInOwnGroup in leases, Crash, Pivots, [1];
        p53_g9_no_chain_check: P53G9NoChainCheck in lost, DiskFull, Pivots, [1];
        p54_t13_no_position_check: P54T13NoPositionCheck in misplaced, Discovery, Tier, [1];
        p55_no_epoch_check: P55NoEpochCheck in foreign, Discovery, Tier, [1];
        p56_overlay_kept_after_refill: P56OverlayKeptAfterRefill in server, FlushErrors, Pivots, [20];
        p57_t3_reader_past_committed: P57T3ReaderPastCommitted in basic, Discovery, Tier, [7];
        p58_corruption_as_end_of_view: P58CorruptionAsEndOfView in reads, ReadFaults, Pivots, [1];
        p60_t8_no_boot_check: P60T8NoBootCheck in boot, Discovery, Tier, [1];
        p61_fatal_slot_skipped: P61FatalSlotSkipped in fatal, Crash, Tier, [1];
        p62_g12_barrier_before_checkpoint: P62G12BarrierBeforeCheckpoint in barrier, Crash, Tier, [1];
        p63_flag_reported_before_flush: P63FlagReportedBeforeFlush in admin, Crash, Tier, [1];
        p64_t7_scan_from_committed: P64T7ScanFromCommitted in lazytail, Discovery, Tier, [1];
        p65_t11_skip_non_commit_record: P65T11SkipNonCommitRecord in leases, Discovery, Tier, [1];
        p66_boot_recovery_without_rewrite: P66BootRecoveryWithoutRewrite in boot, Kills, Pivots, [1];
        p67_unknown_boot_writes_boot_id: P67UnknownBootWritesBootId in boot, Discovery, Tier, [1];
        p69_t2_ref_move_in_later_group: P69T2RefMoveInLaterGroup in import, Crash, Pivots, [1];
        p70_failed_cas_moves_ref: P70FailedCasMovesRef in import, Discovery, Tier, [1];
        p71_unknown_anchor_as_dead: P71UnknownAnchorAsDead in intents_live, Kills, Pivots, [1];
        p72_rotate_under_writer_only: P72RotateUnderWriterOnly in rotate, Discovery, Tier, [1];
        p73_retire_at_boundary: P73RetireAtBoundary in boundary, Discovery, Tier, [11];
        p74_t1_reuse_retired_extent: P74T1ReuseRetiredExtent in reuse, Discovery, Tier, [7];
        p76_maintenance_without_byte: P76MaintenanceWithoutByte in maint2, Discovery, Tier, [3];
        p77_delete_pinned_file: P77DeletePinnedFile in fork, Discovery, Tier, [1];
        p81_pin_in_later_group: P81PinInLaterGroup in fork, Crash, Pivots, [1];
        p82_write_through_without_dir_flush: P82WriteThroughWithoutDirFlush in trash, Discovery, Tier, [1];
        p83_cross_volume_copy: P83CrossVolumeCopy in xvol, Discovery, Tier, [1];
        p88_init_keeps_ref_id_zero: P88InitKeepsRefIdZero in init_store, Discovery, Tier, [1];
        p89_lease_on_wall_clock: P89LeaseOnWallClock in clock, Discovery, Tier, [1];
        p90_df_ack_after_disk_full: P90DfAckAfterDiskFull in trash, DiskFull, Pivots, [1];
        p91_flush_error_as_success: P91FlushErrorAsSuccess in basic, DiskFull, Pivots, [8];
        p92_read_error_as_end: P92ReadErrorAsEnd in reboot, ReadFaults, Pivots, [1];
        p96_spare_without_flushes: P96SpareWithoutFlushes in spare, Kills, Pivots, [2];
        l06_grant_to_two_waiters: L06GrantToTwoWaiters in inproc, Discovery, Tier, [16];
        l07_late_grant_leaked: L07LateGrantLeaked in timeouts, Kills, Pivots, [1];
        l08_probe_first_quiet_only: L08ProbeFirstQuietOnly in quiet, Discovery, Tier, [3];
    }
    open {
        p45_g5_smaller_durable: P45G5SmallerDurable in admin, Crash, Tier, [1, 2];
        p59_fallback_to_older_set: P59FallbackToOlderSet in gc, ReadFaults, Tier, [1];
        p79_sweep_pending_named: P79SweepPendingNamed in fork, Crash, Tier, [1, 2];
    }
}
