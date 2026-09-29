//! The seeded-bug switches ([F16 §17]; PLAN WP-40, S4): one switch per bug of the catalogue whose vehicle is the toy log
//! — every row of [F16 §17.3] with vehicle **toy** and the three toy rows of [F16 §17.4] — each named after its P-rule
//! (or L-rule) and the source bug it coincides with ([F16 §17.1]: T1–T14, G1–G13, LG, DF).
//!
//! A [`Bugs`] value is part of the toy's [`crate::Config`]; with every switch off the toy follows [F16] exactly. Each
//! switch is the smallest change of the toy that violates its rule, as the catalogue's "Primary seeded bug" column states
//! it; where the toy's form differs from the catalogue's wording, [`BugInfo::toy_form`] says how. The code that a switch
//! changes checks it with [`Bugs::on`] at the one place the rule is enforced.
//!
//! The enumerator's author never reads this module before WP-32 is accepted (PLAN §3.1, `xtask/roles.toml`).

use core::fmt;

/// One seeded bug. The discriminant is the bug's index in [`CATALOGUE`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u8)]
pub enum Bug {
    /// P-1, G4: the appender waits for the flush byte while holding the writer byte.
    P01G4WaitFlushUnderWriter,
    /// P-2: the flush holder flushes the log while holding the writer byte.
    P02FlushUnderWriter,
    /// P-3: a second in-process client is granted the writer byte by the kernel path while the first holds it.
    P03KernelPathSecondClient,
    /// P-4: a `config_gen` bump is written without the writer byte, over a concurrent publish.
    P04BumpWithoutWriter,
    /// P-5: a `Lease` claim is appended with `lazy` = 1 and acknowledged at its publish.
    P05LeaseTaggedLazy,
    /// P-6: a flushed range that spans a rotation flushes only the extent that holds E.
    P06FlushOnlyLastExtent,
    /// P-7: a lazy publish sets `durable_lsn` to its end.
    P07LazyPublishAdvancesDurable,
    /// P-8: the first group is appended into a new extent before `durable+meta` on it.
    P08AppendBeforeExtentDurable,
    /// P-9: a group is written across an extent boundary.
    P09GroupAcrossExtents,
    /// P-10: a `Checkpoint` is appended before its segment's `durable+meta`.
    P10CheckpointBeforeSegmentDurable,
    /// P-12: a publish overwrites the slot that holds the newest valid state.
    P12PublishOverNewestSlot,
    /// P-13, T9: the barrier writes one slot and flushes.
    P13T9SingleSlotBarrier,
    /// P-14, T6: a released file is deleted before the barrier's `HEAD` flush.
    P14T6DeleteBeforeBarrier,
    /// P-15: boot-change recovery publishes before flushing the re-written range.
    P15BootRecoveryPublishBeforeFlush,
    /// P-16: the rename is issued before the `FsIntent` group's identity check.
    P16RenameBeforeIntentDurable,
    /// P-17, T12: the move's commit is appended after the rename without `sync_dir` of both parents.
    P17T12MoveWithoutBothParents,
    /// P-18: the removal's commit is appended before `sync_dir` of the parent.
    P18RemoveBeforeParentSync,
    /// P-19, T14: intent recovery commits a roll-forward without `sync_dir` of both parents.
    P19T14RollForwardWithoutBarrier,
    /// P-24: `HEAD` is renamed into place before log.1 is durable.
    P24HeadBeforeLogDurable,
    /// P-25: phase-1 work runs while the writer byte is held.
    P25PhaseOneUnderWriter,
    /// P-27: on a writer-wait timeout the process appends without the byte.
    P27AppendAfterWriterTimeout,
    /// P-29: an appender appends at the published `committed_lsn` without scanning beyond it.
    P29AppendAtCommitted,
    /// P-30, G10: a pending group is replayed into the read overlay.
    P30G10PendingIntoOverlay,
    /// P-31: `#N` is allocated from `HEAD.next_id` alone, ignoring a pending group.
    P31AllocateFromHeadOnly,
    /// P-32, T4: the idempotency key is evaluated before the scan.
    P32T4IdempotencyBeforeScan,
    /// P-33, G6: an idempotent replay of a pending group is acknowledged before its durability.
    P33G6ReplayBeforeDurable,
    /// P-34: "the candidate stands" when `committed_lsn` = L0 although pending groups exist.
    P34CandidateStandsAtCommitted,
    /// P-35: W3 is not re-checked on the final encoding.
    P35NoFinalSizeCheck,
    /// P-36: a `Checkpoint` advances the HLC sequence.
    P36CheckpointAdvancesHlc,
    /// P-37: the trailer is seeded with the chain value at `committed_lsn` instead of at E_v.
    P37ChainSeedAtCommitted,
    /// P-38, G2: a lazy publish moves `committed_lsn` past a pending durable group.
    P38G2LazyPublishPastDurable,
    /// P-39, G13: a lazy group behind a dead writer's pending durable group is left unpublished.
    P39G13LazyStranded,
    /// P-40, G1: a durable group is treated as covered when `committed_lsn` ≥ E_g.
    P40G1CoveredByCommitted,
    /// P-41: a flush-byte timeout is acknowledged as success.
    P41FlushTimeoutAcked,
    /// P-42, G3, T5: the flush holder flushes without re-writing the pending range.
    P42G3T5FlushWithoutRewrite,
    /// P-43, G8: the pending range is scanned and re-written outside the writer byte.
    P43G8RewriteOutsideWriter,
    /// P-44: a failed flush is retried on the same handle and its success acknowledged.
    P44FlushRetriedAndAcked,
    /// P-45, G5: a publish writes a smaller `durable_lsn`.
    P45G5SmallerDurable,
    /// P-46, G7: acknowledgement by position, without reading the trailer.
    P46G7AckByPosition,
    /// P-47, LG: a writer whose group vanished re-appends its old bytes at the old position.
    P47LgReappendOldBytes,
    /// P-48, G11: a publish from a stale `HEAD` snapshot.
    P48G11StalePublish,
    /// P-49: `committed_lsn` is kept above the valid end after a lost lazy tail.
    P49CommittedAboveValidEnd,
    /// P-50, G12: a covered `Checkpoint` without its segment set published.
    P50G12CheckpointSetUnpublished,
    /// P-51: the delta checkpoint runs while the writer byte is still held.
    P51MaintenanceUnderWriter,
    /// P-52, T10: a commit and its `Marker` records are appended as two groups.
    P52T10MarkerInOwnGroup,
    /// P-53, G9: a group is accepted whose predecessor differs (no chain check).
    P53G9NoChainCheck,
    /// P-54, T13: the position check is skipped.
    P54T13NoPositionCheck,
    /// P-55: a record of another epoch is accepted.
    P55NoEpochCheck,
    /// P-56: an overlay is kept after a lost tail was refilled.
    P56OverlayKeptAfterRefill,
    /// P-57, T3: a reader replays past `committed_lsn`.
    P57T3ReaderPastCommitted,
    /// P-58: a reader treats an invalid group below `durable_lsn` as the end of its view.
    P58CorruptionAsEndOfView,
    /// P-59: a reader falls back to an older segment set when a named file is missing.
    P59FallbackToOlderSet,
    /// P-60, T8: a reader serves a pre-crash view.
    P60T8NoBootCheck,
    /// P-61: a slot that passes its checksum but fails validity is skipped for the other slot.
    P61FatalSlotSkipped,
    /// P-62, G12: the barrier runs before the `Checkpoint` is published.
    P62G12BarrierBeforeCheckpoint,
    /// P-63: `quiet on` is reported before the `HEAD` flush.
    P63FlagReportedBeforeFlush,
    /// P-64, T7: recovery scans from `committed_lsn`.
    P64T7ScanFromCommitted,
    /// P-65, T11: recovery skips a non-commit durable record.
    P65T11SkipNonCommitRecord,
    /// P-66: recovery publishes the new `boot_id` without re-writing `(durable_lsn, E_v]`.
    P66BootRecoveryWithoutRewrite,
    /// P-67: an Unknown-boot publisher writes a `boot_id` other than its slot's.
    P67UnknownBootWritesBootId,
    /// P-69, T2: the ref move is written as a separate record in a later group.
    P69T2RefMoveInLaterGroup,
    /// P-70: a commit whose ref CAS failed moves its ref anyway.
    P70FailedCasMovesRef,
    /// P-71: recovery treats an `Unknown` anchor as Dead and rolls a live move back.
    P71UnknownAnchorAsDead,
    /// P-72: an appender that holds only the writer byte appends the first group of a new extent.
    P72RotateUnderWriterOnly,
    /// P-73: extent n is retired while `checkpoint_lsn` ≤ n·E.
    P73RetireAtBoundary,
    /// P-74, T1: a retired extent's file is zero-filled and reused under its old number.
    P74T1ReuseRetiredExtent,
    /// P-76: a checkpoint runs without the maintenance byte beside another.
    P76MaintenanceWithoutByte,
    /// P-77: a file still referenced by a pin is deleted.
    P77DeletePinnedFile,
    /// P-79: the sweeper deletes a file named only by a pending group.
    P79SweepPendingNamed,
    /// P-81: a fork's `Pin` is written in a later group than its `RefUpdate`.
    P81PinInLaterGroup,
    /// P-82: a rename relies on write-through and skips the directory flush.
    P82WriteThroughWithoutDirFlush,
    /// P-83: a cross-volume move copies, then deletes the source.
    P83CrossVolumeCopy,
    /// P-88: `init`'s `HEAD` keeps `next_ref_id` = 0 after creating `main` with ref id 0.
    P88InitKeepsRefIdZero,
    /// P-89: a lease deadline is evaluated on the wall clock on a known boot.
    P89LeaseOnWallClock,
    /// P-90, DF: the command is acknowledged after `ERROR_DISK_FULL`.
    P90DfAckAfterDiskFull,
    /// P-91: a non-lazy class error from a flush is treated as success (a downgrade).
    P91FlushErrorAsSuccess,
    /// P-92: an appender's scan treats a read error above `durable_lsn` as the end of the log.
    P92ReadErrorAsEnd,
    /// P-96: a rotation appends into a spare without re-issuing `durable+meta` and `durable-name`.
    P96SpareWithoutFlushes,
    /// L-6 ([OS/lock §5.4] I-L4 with I-L2): a grant obtained by one kernel wait is handed to two in-process waiters.
    L06GrantToTwoWaiters,
    /// L-7 ([OS/lock §5.4] I-L6): a grant that arrives after the waiter's deadline is neither returned nor released.
    L07LateGrantLeaked,
    /// L-8 ([F03 §3.1] rule 2): the maintenance decider probes only the first quiet byte.
    L08ProbeFirstQuietOnly,
}

/// The number of seeded bugs the toy log carries: the 78 toy rows of [F16 §17.3] and the 3 of [F16 §17.4].
pub const N_BUGS: usize = 81;

/// One row of the catalogue.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BugInfo {
    /// The bug.
    pub bug: Bug,
    /// The rule it violates: `P-n` of [F16] or `L-n` of [F16 §17.4].
    pub rule: &'static str,
    /// The source bugs it coincides with ([F16 §17.1]); empty for none.
    pub source: &'static str,
    /// The catalogue's "Detected by" column.
    pub detected_by: &'static str,
    /// The primary seeded bug as the catalogue states it.
    pub text: &'static str,
    /// How the toy's switch realises it, when that differs from the catalogue's wording; empty otherwise.
    pub toy_form: &'static str,
}

impl Bug {
    /// Every bug, in catalogue order.
    pub const ALL: [Bug; N_BUGS] = {
        let mut out = [Bug::P01G4WaitFlushUnderWriter; N_BUGS];
        let mut i = 0;
        while i < N_BUGS {
            out[i] = CATALOGUE[i].bug;
            i += 1;
        }
        out
    };

    /// The bug's row of the catalogue.
    pub const fn info(self) -> &'static BugInfo {
        &CATALOGUE[self as usize]
    }

    /// The bug's index (its bit in [`Bugs`]).
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Bug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let i = self.info();
        if i.source.is_empty() {
            write!(f, "{} ({:?})", i.rule, self)
        } else {
            write!(f, "{} {} ({:?})", i.rule, i.source, self)
        }
    }
}

/// A set of switched-on bugs. `Bugs::NONE` is the correct toy.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct Bugs(u128);

impl Bugs {
    /// Every switch off: the toy follows [F16] exactly.
    pub const NONE: Bugs = Bugs(0);

    /// Exactly `bug` on.
    pub const fn only(bug: Bug) -> Bugs {
        Bugs(1 << bug as u32)
    }

    /// This set with `bug` on as well.
    pub const fn with(self, bug: Bug) -> Bugs {
        Bugs(self.0 | 1 << bug as u32)
    }

    /// Whether `bug` is on.
    #[inline]
    pub const fn on(self, bug: Bug) -> bool {
        self.0 & (1 << bug as u32) != 0
    }

    /// Whether no switch is on.
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    /// The switched-on bugs, in catalogue order.
    pub fn iter(self) -> impl Iterator<Item = Bug> {
        Bug::ALL.into_iter().filter(move |&b| self.on(b))
    }
}

const fn row(
    bug: Bug,
    rule: &'static str,
    source: &'static str,
    detected_by: &'static str,
    text: &'static str,
    toy_form: &'static str,
) -> BugInfo {
    BugInfo {
        bug,
        rule,
        source,
        detected_by,
        text,
        toy_form,
    }
}

/// The catalogue: [F16 §17.3]'s toy rows in rule order, then [F16 §17.4]'s toy rows.
pub const CATALOGUE: [BugInfo; N_BUGS] = [
    row(
        Bug::P01G4WaitFlushUnderWriter,
        "P-1",
        "G4",
        "trace",
        "the appender waits for the flush byte while holding the writer byte",
        "at a rotation (P-72 step 1) the appender waits for the flush byte on its own client without releasing the writer byte",
    ),
    row(
        Bug::P02FlushUnderWriter,
        "P-2",
        "",
        "trace",
        "the flush holder flushes the log while holding the writer byte",
        "",
    ),
    row(
        Bug::P03KernelPathSecondClient,
        "P-3",
        "",
        "ack",
        "a second in-process client is granted the writer byte by the kernel path while the first holds it; both append at one lsn",
        "the toy's lock layer treats an in-process Busy as a kernel grant (the per-process kernel lock is already held) and hands the second client a grant",
    ),
    row(
        Bug::P04BumpWithoutWriter,
        "P-4",
        "",
        "trace (ig6)",
        "a `config_gen` bump written without the writer byte, over a concurrent publish, so `durable_lsn` decreases",
        "",
    ),
    row(
        Bug::P05LeaseTaggedLazy,
        "P-5",
        "",
        "ack",
        "a `Lease` claim appended with `lazy` = 1 and acknowledged at its publish",
        "the kind registry gives `Lease` the class `lazy`, so the writer tags it lazy=1, publishes it at once and acknowledges it at its publish, and the scan accepts the tag",
    ),
    row(
        Bug::P06FlushOnlyLastExtent,
        "P-6",
        "",
        "ack",
        "a flushed range that spans a rotation flushes only the extent that holds E",
        "",
    ),
    row(
        Bug::P07LazyPublishAdvancesDurable,
        "P-7",
        "",
        "trace (ig6); avail (false corruption below `durable_lsn`)",
        "a lazy publish sets `durable_lsn` to its end",
        "",
    ),
    row(
        Bug::P08AppendBeforeExtentDurable,
        "P-8",
        "",
        "ack",
        "the first group is appended into a new extent before `durable+meta` on it",
        "the rotation skips `durable+meta` on the new extent and `durable-name` on the store directory",
    ),
    row(
        Bug::P09GroupAcrossExtents,
        "P-9",
        "",
        "ack",
        "a group is written across an extent boundary",
        "G-3's placement is skipped: a group that does not fit the rest of the extent is written at E_v in one `write_at`, which extends the extent file past E",
    ),
    row(
        Bug::P10CheckpointBeforeSegmentDurable,
        "P-10",
        "",
        "ack; avail",
        "a `Checkpoint` is appended before its segment's `durable+meta`",
        "",
    ),
    row(
        Bug::P12PublishOverNewestSlot,
        "P-12",
        "",
        "ack (a torn publish leaves no valid slot)",
        "a publish overwrites the slot that holds the newest valid state",
        "",
    ),
    row(
        Bug::P13T9SingleSlotBarrier,
        "P-13",
        "T9",
        "ack; avail",
        "the barrier writes one slot and flushes",
        "every durable publish (the barrier, `quiet`, boot-change recovery) writes one slot and flushes `HEAD`; for the barrier itself the bug is masked by P-62 (the covering publish of the maintenance's own `Checkpoint` already wrote the new set into the other slot, and the barrier's flush makes it durable), so the enumerator reaches it through flag changes: after `quiet on` and `quiet off` each wrote one slot, a crash that tears the newer slot while the older reverts brings back the flag state that the acknowledged `quiet off` replaced",
    ),
    row(
        Bug::P14T6DeleteBeforeBarrier,
        "P-14",
        "T6",
        "avail; ack",
        "a released file is deleted before the barrier's `HEAD` flush",
        "",
    ),
    row(
        Bug::P15BootRecoveryPublishBeforeFlush,
        "P-15",
        "",
        "fresh",
        "boot-change recovery publishes before flushing the re-written range",
        "",
    ),
    row(
        Bug::P16RenameBeforeIntentDurable,
        "P-16",
        "",
        "ns",
        "the rename is issued before the `FsIntent` group's identity check",
        "",
    ),
    row(
        Bug::P17T12MoveWithoutBothParents,
        "P-17",
        "T12",
        "ns",
        "the move's commit is appended after the rename without `sync_dir` of both parents",
        "only the destination parent is synced",
    ),
    row(
        Bug::P18RemoveBeforeParentSync,
        "P-18",
        "",
        "ns",
        "the removal's commit is appended before `sync_dir` of the parent",
        "",
    ),
    row(
        Bug::P19T14RollForwardWithoutBarrier,
        "P-19",
        "T14",
        "ns",
        "intent recovery commits a roll-forward without `sync_dir` of both parents",
        "",
    ),
    row(
        Bug::P24HeadBeforeLogDurable,
        "P-24",
        "",
        "avail",
        "`HEAD` is renamed into place before log.1 is durable",
        "",
    ),
    row(
        Bug::P25PhaseOneUnderWriter,
        "P-25",
        "",
        "trace",
        "phase-1 work (the candidate's computation, or a simulated file-system wait) runs while the writer byte is held",
        "",
    ),
    row(
        Bug::P27AppendAfterWriterTimeout,
        "P-27",
        "",
        "ack",
        "on a writer-wait timeout the process appends without the byte",
        "",
    ),
    row(
        Bug::P29AppendAtCommitted,
        "P-29",
        "",
        "ack",
        "an appender appends at the published `committed_lsn` without scanning beyond it, overwriting a durable group whose publish a crash lost",
        "",
    ),
    row(
        Bug::P30G10PendingIntoOverlay,
        "P-30",
        "G10",
        "fresh",
        "a pending group is replayed into the read overlay",
        "",
    ),
    row(
        Bug::P31AllocateFromHeadOnly,
        "P-31",
        "",
        "model (I1)",
        "`#N` is allocated from `HEAD.next_id` alone, ignoring a pending group",
        "",
    ),
    row(
        Bug::P32T4IdempotencyBeforeScan,
        "P-32",
        "T4",
        "model (duplicate commit)",
        "the key is evaluated before the scan",
        "",
    ),
    row(
        Bug::P33G6ReplayBeforeDurable,
        "P-33",
        "G6",
        "ack",
        "an idempotent replay of a pending group is acknowledged before its durability",
        "",
    ),
    row(
        Bug::P34CandidateStandsAtCommitted,
        "P-34",
        "",
        "model",
        "\"the candidate stands\" when `committed_lsn` = L0 although pending groups exist; two exclusive claims succeed",
        "",
    ),
    row(
        Bug::P35NoFinalSizeCheck,
        "P-35",
        "",
        "ack",
        "W3 is not re-checked; a group longer than E is appended and acknowledged",
        "a runtime batch whose final encoding (its symbol definitions, allocated under the writer byte) outgrows W3 is appended at E_v in one `write_at`, which extends the extent file past E",
    ),
    row(
        Bug::P36CheckpointAdvancesHlc,
        "P-36",
        "",
        "model (commit id; I43′)",
        "a `Checkpoint` advances the sequence: a class-I checkpoint appended between two commits in one millisecond raises the next commit's `hlc`",
        "",
    ),
    row(
        Bug::P37ChainSeedAtCommitted,
        "P-37",
        "",
        "ack; chain",
        "the trailer is seeded with the chain value at `committed_lsn` instead of at E_v",
        "",
    ),
    row(
        Bug::P38G2LazyPublishPastDurable,
        "P-38",
        "G2",
        "fresh",
        "a lazy publish moves `committed_lsn` past a pending durable group",
        "",
    ),
    row(
        Bug::P39G13LazyStranded,
        "P-39",
        "G13",
        "fresh",
        "a lazy group behind a dead writer's pending durable group is left unpublished",
        "the appender skips phase 2b for its lazy group and reports it published",
    ),
    row(
        Bug::P40G1CoveredByCommitted,
        "P-40",
        "G1",
        "ack",
        "a durable group is treated as covered when `committed_lsn` ≥ E_g",
        "taken literally the switch changes nothing: the published `committed_lsn` never passes a pending durable group (P-38, P-49). The toy's form takes the committed position of the group's own append (the end of the valid log it extended, which P-49 would publish and which is at least E_g) for the covered test, so the durable group goes to the identity check right after its append, without a covering flush (source bug G1: acknowledge before a covering flush)",
    ),
    row(
        Bug::P41FlushTimeoutAcked,
        "P-41",
        "",
        "ack",
        "a flush-byte timeout is acknowledged as success",
        "",
    ),
    row(
        Bug::P42G3T5FlushWithoutRewrite,
        "P-42",
        "G3, T5",
        "ack",
        "the flush holder flushes without re-writing a dead predecessor's range after a failed flush",
        "",
    ),
    row(
        Bug::P43G8RewriteOutsideWriter,
        "P-43",
        "G8",
        "chain; ack",
        "the pending range is scanned and re-written outside the writer byte",
        "",
    ),
    row(
        Bug::P44FlushRetriedAndAcked,
        "P-44",
        "",
        "ack",
        "a failed flush is retried on the same handle and its success acknowledged",
        "",
    ),
    row(
        Bug::P45G5SmallerDurable,
        "P-45",
        "G5",
        "trace (ig6)",
        "a publish writes a smaller `durable_lsn`",
        "the flush holder's publish sets `durable_lsn` to its flushed end E instead of max(the slot's `durable_lsn`, E)",
    ),
    row(
        Bug::P46G7AckByPosition,
        "P-46",
        "G7",
        "ack",
        "acknowledgement by position, without reading the trailer",
        "",
    ),
    row(
        Bug::P47LgReappendOldBytes,
        "P-47",
        "LG",
        "chain; model",
        "a writer whose group vanished re-appends its old bytes at the old position",
        "",
    ),
    row(
        Bug::P48G11StalePublish,
        "P-48",
        "G11",
        "trace (ig6)",
        "a publish from a stale `HEAD` snapshot",
        "the flush holder publishes from the slot it read before its flush",
    ),
    row(
        Bug::P49CommittedAboveValidEnd,
        "P-49",
        "",
        "fresh",
        "`committed_lsn` is kept above the valid end after a lost lazy tail, so a later unflushed group becomes visible",
        "",
    ),
    row(
        Bug::P50G12CheckpointSetUnpublished,
        "P-50",
        "G12",
        "trace (ig6); avail",
        "a covered `Checkpoint` without its segment set published",
        "",
    ),
    row(
        Bug::P51MaintenanceUnderWriter,
        "P-51",
        "",
        "trace",
        "the delta checkpoint runs while the writer byte is still held",
        "the flush holder runs phase 3 right after its publish, inside its holding of the writer and flush bytes; the maintenance it runs nests its own acquisitions in that holding",
    ),
    row(
        Bug::P52T10MarkerInOwnGroup,
        "P-52",
        "T10",
        "model (markers)",
        "a commit and its `Marker` records are appended as two groups",
        "the marker group follows the commit's group in a `write_at` of its own, its HLC taken in log order",
    ),
    row(
        Bug::P53G9NoChainCheck,
        "P-53",
        "G9",
        "chain; ack",
        "a group is accepted whose predecessor differs (no chain check)",
        "",
    ),
    row(
        Bug::P54T13NoPositionCheck,
        "P-54",
        "T13",
        "chain; model",
        "the position check is skipped; a same-epoch record left at another position is accepted",
        "",
    ),
    row(
        Bug::P55NoEpochCheck,
        "P-55",
        "",
        "chain",
        "a record of another epoch is accepted",
        "",
    ),
    row(
        Bug::P56OverlayKeptAfterRefill,
        "P-56",
        "",
        "model",
        "an overlay is kept after a lost tail was refilled",
        "",
    ),
    row(
        Bug::P57T3ReaderPastCommitted,
        "P-57",
        "T3",
        "fresh",
        "a reader replays past `committed_lsn`",
        "",
    ),
    row(
        Bug::P58CorruptionAsEndOfView,
        "P-58",
        "",
        "fresh; ack",
        "a reader treats an invalid group below `durable_lsn` as the end of its view",
        "a reader treats an invalid group or a failed read below `durable_lsn` as the end of its view (P-92's reader rule)",
    ),
    row(
        Bug::P59FallbackToOlderSet,
        "P-59",
        "",
        "model",
        "a reader falls back to an older segment set when a named file is missing",
        "",
    ),
    row(
        Bug::P60T8NoBootCheck,
        "P-60",
        "T8",
        "fresh",
        "a reader serves a pre-crash view",
        "the reader skips the boot check (and so boot-change recovery) before its first read",
    ),
    row(
        Bug::P61FatalSlotSkipped,
        "P-61",
        "",
        "model",
        "a slot that passes its checksum but fails validity is skipped for the other slot",
        "",
    ),
    row(
        Bug::P62G12BarrierBeforeCheckpoint,
        "P-62",
        "G12",
        "avail; ack",
        "the barrier runs before the `Checkpoint` is published",
        "",
    ),
    row(
        Bug::P63FlagReportedBeforeFlush,
        "P-63",
        "",
        "ack (acknowledged-effect list)",
        "`quiet on` is reported before the `HEAD` flush",
        "the verb returns after its two publishes, before any `HEAD` flush; the flush follows with the handle's next operation (or when the handle is dropped)",
    ),
    row(
        Bug::P64T7ScanFromCommitted,
        "P-64",
        "T7",
        "ack",
        "recovery scans from `committed_lsn`",
        "",
    ),
    row(
        Bug::P65T11SkipNonCommitRecord,
        "P-65",
        "T11",
        "model; ack",
        "recovery skips a non-commit durable record",
        "the adopting process (a flush holder's or boot-change recovery's publish) folds only the `Commit` records of the groups it adopts",
    ),
    row(
        Bug::P66BootRecoveryWithoutRewrite,
        "P-66",
        "",
        "ack; fresh",
        "recovery publishes the new `boot_id` without re-writing `(durable_lsn, E_v]`",
        "",
    ),
    row(
        Bug::P67UnknownBootWritesBootId,
        "P-67",
        "",
        "trace (ig6)",
        "an Unknown-boot publisher writes a `boot_id` other than its slot's",
        "",
    ),
    row(
        Bug::P69T2RefMoveInLaterGroup,
        "P-69",
        "T2",
        "model",
        "the ref move is written as a separate record in a later group",
        "",
    ),
    row(
        Bug::P70FailedCasMovesRef,
        "P-70",
        "",
        "model (I27′)",
        "a commit whose ref CAS failed moves its ref anyway, where the rule appends a `RefUpdate` reason 5 `park` of `orphans/<R>`",
        "",
    ),
    row(
        Bug::P71UnknownAnchorAsDead,
        "P-71",
        "",
        "ns; model",
        "recovery treats an `Unknown` anchor as Dead and rolls a live move back",
        "",
    ),
    row(
        Bug::P72RotateUnderWriterOnly,
        "P-72",
        "",
        "ack",
        "an appender that holds only the writer byte appends the first group into an extent that another process is preparing",
        "the appender prepares and rotates without the flush byte, under the writer byte",
    ),
    row(
        Bug::P73RetireAtBoundary,
        "P-73",
        "",
        "avail",
        "extent n is retired while `checkpoint_lsn` ≤ n·E",
        "EX-5 is checked against the extent's first byte, (n − 1)·E, so the extent that holds `checkpoint_lsn` is retired once the published log reaches past it",
    ),
    row(
        Bug::P74T1ReuseRetiredExtent,
        "P-74",
        "T1",
        "fresh; model",
        "a retired extent's file is zero-filled and reused under its old number while a reader still replays from it",
        "",
    ),
    row(
        Bug::P76MaintenanceWithoutByte,
        "P-76",
        "",
        "model",
        "a checkpoint runs without the maintenance byte beside another; the later publish drops the earlier delta",
        "",
    ),
    row(
        Bug::P77DeletePinnedFile,
        "P-77",
        "",
        "avail",
        "a file still referenced by a pin is deleted",
        "",
    ),
    row(
        Bug::P79SweepPendingNamed,
        "P-79",
        "",
        "avail; ack",
        "the sweeper deletes a file named only by a pending group",
        "",
    ),
    row(
        Bug::P81PinInLaterGroup,
        "P-81",
        "",
        "avail",
        "a fork's `Pin` is written in a later group than its `RefUpdate`; after a crash GC deletes the fork base",
        "",
    ),
    row(
        Bug::P82WriteThroughWithoutDirFlush,
        "P-82",
        "",
        "ns",
        "a Windows rename passes `MOVEFILE_WRITE_THROUGH` and skips the directory flush",
        "the `file rm --trash` rename relies on the write-through and skips `durable-name` on both parents",
    ),
    row(
        Bug::P83CrossVolumeCopy,
        "P-83",
        "",
        "ns",
        "a cross-volume move copies, then deletes the source",
        "",
    ),
    row(
        Bug::P88InitKeepsRefIdZero,
        "P-88",
        "",
        "model",
        "`init`'s `HEAD` keeps `next_ref_id` = 0 after creating `main` with ref id 0; the next branch reuses id 0",
        "",
    ),
    row(
        Bug::P89LeaseOnWallClock,
        "P-89",
        "",
        "model (lease assertion)",
        "a lease deadline is evaluated on the wall clock on a known boot; a ±1 h step expires a live lease",
        "",
    ),
    row(
        Bug::P90DfAckAfterDiskFull,
        "P-90",
        "DF",
        "ack",
        "the command is acknowledged after `ERROR_DISK_FULL`",
        "a `DiskFull` from a write, create, rename or unlink is treated as success",
    ),
    row(
        Bug::P91FlushErrorAsSuccess,
        "P-91",
        "",
        "ack",
        "an `Unsupported` from a flush is treated as success (a downgrade)",
        "a flush error other than `Io` (`DiskFull`, `Unsupported`) is treated as success: the enumerator injects `DiskFull` and `Io` at flushes, never `Unsupported`",
    ),
    row(
        Bug::P92ReadErrorAsEnd,
        "P-92",
        "",
        "ack",
        "an appender's scan treats a read error above `durable_lsn` as the end of the log and appends over an acknowledged group that an OS crash left above a stale `durable_lsn`",
        "",
    ),
    row(
        Bug::P96SpareWithoutFlushes,
        "P-96",
        "",
        "ack; ns",
        "a rotation appends into a spare without re-issuing `durable+meta` and `durable-name`",
        "",
    ),
    row(
        Bug::L06GrantToTwoWaiters,
        "L-6",
        "",
        "ack",
        "a grant obtained by one kernel wait is handed to two waiting clients of one process; both append at one lsn",
        "the toy's lock layer: a client that arrives while a sibling client is inside the process's kernel wait for the byte waits on that wait's outcome and is handed the grant the sibling obtains",
    ),
    row(
        Bug::L07LateGrantLeaked,
        "L-7",
        "",
        "avail",
        "a grant that arrives after the waiter's deadline is neither returned nor released; the byte stays held by a client that returned `Busy`",
        "after a timed-out wait the toy's lock layer takes the byte once more on a fresh client (a late grant) and neither returns nor releases it",
    ),
    row(
        Bug::L08ProbeFirstQuietOnly,
        "L-8",
        "",
        "trace (a checkpoint during quiet mode)",
        "the maintenance decider probes only the first quiet byte; a checkpoint runs while another requester holds a later quiet byte",
        "",
    ),
];

/// The open E4 items: the seeded bugs the toy cannot make the enumerator report, with the reason. Each is masked in the
/// toy by another rule it keeps, so the switch changes no reachable crash state; each awaits the specification's
/// disposition of its catalogue row ([F16 §17.3]: a reachable form, another vehicle, or a detection class stating the
/// masking). Every other bug of [`CATALOGUE`] is reached (E4).
pub const OPEN: [(Bug, &str); 3] = [
    (
        Bug::P45G5SmallerDurable,
        "masked by the flush byte: every publisher that raises durable_lsn holds it (P-41, P-42, P-66, P-85), and the \
         flush holder's E is the end of a scan that starts at the slot's durable_lsn under that byte, so E is at least the \
         slot's durable_lsn whenever it publishes and max(durable_lsn, E) = E in every reachable state",
    ),
    (
        Bug::P59FallbackToOlderSet,
        "masked by P-13 and P-62: after every barrier both slots name one segment set, so the fallback finds no other set; \
         between a Checkpoint's covering publish and its barrier the other slot names the previous set, whose file still \
         exists and whose replay to the newest committed_lsn gives the same state",
    ),
    (
        Bug::P79SweepPendingNamed,
        "masked by P-34 and P-62: the toy's only records that name files are Checkpoints and fork Pins; the sweeper runs \
         after its own Checkpoint passed its identity check, whose publish covers every group before it, and a fork \
         appended after that Checkpoint re-validates under the writer byte and pins the new set; the pending namer of \
         the product is a bulk writer's cs.<n> (P-78), which the toy does not build",
    ),
];

/// Where the toy's detection differs from the catalogue's "Detected by" column: the assertion families the enumerator
/// reports for the bug in the toy, and why the catalogue's class is not what the toy can show. Each row is a proposed
/// correction of its catalogue row ([F16 §17.3]) that awaits the specification's disposition (the protocol chapter's
/// author, R-SPEC-P); until then a test accepts the catalogue's families and these.
pub const TOY_DETECTION: [(Bug, &str, &str); 16] = [
    (
        Bug::P09GroupAcrossExtents,
        "avail",
        "the group is written at E_v in one write_at, which extends the extent past E; a log file longer than E is corrupt \
         ([F05 §2.2]), so every scan refuses the store, the flush holder's included, and no process acknowledges the group",
    ),
    (
        Bug::P12PublishOverNewestSlot,
        "avail",
        "every publish overwrites the newest slot, so the other slot keeps the state before the first publish; after a torn \
         publish, or for a reader of that slot, it names files that a later checkpoint retired and deleted, and the store \
         refuses; the log itself is intact, so no acknowledged effect is lost",
    ),
    (
        Bug::P15BootRecoveryPublishBeforeFlush,
        "avail",
        "the durable publish makes HEAD name a durable_lsn over a range that a failed flush or a crash then leaves invalid; \
         every later process finds an invalid group below durable_lsn and refuses the store (P-58), so no stale view is served",
    ),
    (
        Bug::P35NoFinalSizeCheck,
        "avail",
        "the outgrown group is written at E_v in one write_at, which extends the extent past E; a log file longer than E is \
         corrupt ([F05 §2.2]), so every scan refuses the store and no process acknowledges the group",
    ),
    (
        Bug::P37ChainSeedAtCommitted,
        "avail",
        "the group's trailer is wrong for its position, so the chain rule rejects it: it is never covered or acknowledged, and \
         while the pending group before it stays (its writer dead) every re-run is lost too, so the operation ends \
         outcome_unknown in a run without a failed flush or read (P-47)",
    ),
    (
        Bug::P42G3T5FlushWithoutRewrite,
        "avail",
        "the unrewritten range keeps the sectors a failed flush poisoned, which read differently on every read (FM-3.2): the \
         holder's publish scans a shorter log than it flushed and refuses to write that fatal slot, and recovery, which \
         also flushes without re-writing, refuses the store",
    ),
    (
        Bug::P43G8RewriteOutsideWriter,
        "trace",
        "I-G4's predicate states P-43 itself: the flush holder scans and re-writes the pending range without holding the \
         writer byte; the chain or acknowledgement damage needs a concurrent append into that range, which the toy's \
         appenders never make below the end the holder scanned",
    ),
    (
        Bug::P53G9NoChainCheck,
        "model",
        "a group behind the wrong predecessor is accepted only where the stale tail of a lost group survives a refill, which \
         revives a value no acknowledged operation wrote and duplicates a retried commit (phantoms, I14′); an acknowledged \
         group never loses its predecessor, which its own flush made durable, so I-G3's chain check cannot fire",
    ),
    (
        Bug::P54T13NoPositionCheck,
        "avail",
        "every scan but one is seeded from HEAD (XXH3-64(epoch) at epoch_lsn, else the 8 bytes before its start), so a group \
         left at another position fails its chain check (P-53) before its position matters; the one start seeded from a \
         record itself is repair's extent head (P-85 step 2, P-97). A repair that accepts a copy of log.2 there as log.1 \
         scans it from lsn 0 and folds its Checkpoint, whose checkpoint_lsn lies above the rebuilt durable_lsn, so both \
         slots it writes fail validity and every process refuses the store (P-61); no rebuilt state is ever served for a \
         model or ack check to compare",
    ),
    (
        Bug::P55NoEpochCheck,
        "avail",
        "every scan but one is seeded from HEAD, so a group of another epoch fails its chain check (P-53) before its epoch \
         matters; the one start seeded from a record itself is repair's extent head (P-85 step 2, P-97). A repair that \
         accepts log.1 of another epoch there scans it and publishes slots of the current epoch over it; every reader then \
         seeds the chain at epoch_lsn with XXH3-64 of the current epoch, finds the first group invalid below durable_lsn \
         and refuses the store (P-58); no rebuilt state is ever served for a model or chain check to compare",
    ),
    (
        Bug::P64T7ScanFromCommitted,
        "avail",
        "after a crash committed_lsn may lie beyond the valid log; boot-change recovery that scans from it publishes a \
         durable_lsn over bytes the log does not hold (or refuses that fatal slot), and every later process refuses the store",
    ),
    (
        Bug::P66BootRecoveryWithoutRewrite,
        "avail",
        "without the re-write the flushed range keeps the sectors a failed flush poisoned (FM-3.2): the recovery's publish \
         scans another end than it flushed and refuses to write that fatal slot, so the first reader refuses the store",
    ),
    (
        Bug::P72RotateUnderWriterOnly,
        "trace",
        "one process at a time holds the writer byte, so two rotators never prepare one extent; what the bug does is the \
         preparation's namespace calls and flushes under the writer byte, which the trace predicates of P-2 report",
    ),
    (
        Bug::P74T1ReuseRetiredExtent,
        "avail",
        "a reader that replays from the zero-filled extent meets an invalid group below durable_lsn and refuses by P-58 (exit \
         7); it never serves the zeros as data",
    ),
    (
        Bug::P81PinInLaterGroup,
        "ack; model",
        "the fork's Pin is one of the fork's acknowledged effects: a crash that keeps the fork's group and loses the later \
         one loses it (ack), and doctor --verify's P-81 check finds the fork without its Pin (model); the deletion of the \
         fork base that the catalogue names needs a later checkpoint",
    ),
    (
        Bug::L06GrantToTwoWaiters,
        "trace",
        "the two clients handed one kernel grant enter phase 2a together: the second's append lands right after the first's, \
         whose identity check then fails (P-46) and which re-runs (P-47), so no acknowledged group is overwritten; the \
         enumerator sees the second client scan and re-write the pending range without a grant of the writer byte (I-G4)",
    ),
];

impl Bug {
    /// Why E4 cannot reach this bug in the toy yet ([`OPEN`]); `None` for a reached bug.
    pub fn open(self) -> Option<&'static str> {
        OPEN.iter().find(|(b, _)| *b == self).map(|(_, why)| *why)
    }

    /// The assertion families ([F16 §17.2]) the catalogue's "Detected by" column names: the first word of each of its
    /// `;`-separated parts (`ack`, `fresh`, `chain`, `trace`, `model`, `ns`, `avail`).
    pub fn families(self) -> Vec<&'static str> {
        classes(self.info().detected_by)
    }

    /// The families the toy's detection adds to the catalogue's ([`TOY_DETECTION`]), with the reason; `None` when the toy
    /// shows the catalogue's class.
    pub fn toy_detection(self) -> Option<(Vec<&'static str>, &'static str)> {
        TOY_DETECTION
            .iter()
            .find(|(b, _, _)| *b == self)
            .map(|(_, f, why)| (classes(f), *why))
    }

    /// Every family a test of the bug accepts: the catalogue's and the toy's.
    pub fn accepted_families(self) -> Vec<&'static str> {
        let mut f = self.families();
        if let Some((extra, _)) = self.toy_detection() {
            for x in extra {
                if !f.contains(&x) {
                    f.push(x);
                }
            }
        }
        f
    }
}

/// The first word of each `;`-separated part of a "Detected by" text (a `;` inside parentheses separates nothing).
fn classes(text: &'static str) -> Vec<&'static str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0u32, 0usize);
    for (i, ch) in text.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
        .into_iter()
        .filter_map(|p| p.split_whitespace().next())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn open_items_are_distinct_and_detection_families_known() {
        let open: BTreeSet<Bug> = OPEN.iter().map(|(b, _)| *b).collect();
        assert_eq!(open.len(), OPEN.len());
        assert!(OPEN.iter().all(|(_, why)| !why.is_empty()));
        assert_eq!(
            Bug::P45G5SmallerDurable.open().map(str::len).map(|n| n > 0),
            Some(true)
        );
        assert_eq!(Bug::P01G4WaitFlushUnderWriter.open(), None);
        let known = ["ack", "fresh", "chain", "trace", "model", "ns", "avail"];
        for b in Bug::ALL {
            let f = b.families();
            assert!(!f.is_empty(), "{b}");
            assert!(f.iter().all(|x| known.contains(x)), "{b}: {f:?}");
        }
        assert_eq!(
            Bug::P10CheckpointBeforeSegmentDurable.families(),
            ["ack", "avail"]
        );
        assert_eq!(Bug::P04BumpWithoutWriter.families(), ["trace"]);
        // Every proposed detection row is for a distinct reached bug, adds a known family the catalogue does not name,
        // and gives its reason.
        let rows: BTreeSet<Bug> = TOY_DETECTION.iter().map(|(b, _, _)| *b).collect();
        assert_eq!(rows.len(), TOY_DETECTION.len());
        for (b, f, why) in TOY_DETECTION {
            assert!(b.open().is_none() && !why.is_empty(), "{b}");
            let extra = classes(f);
            assert!(extra.iter().all(|x| known.contains(x)), "{b}: {extra:?}");
            assert!(extra.iter().any(|x| !b.families().contains(x)), "{b}");
            assert!(extra.iter().all(|x| b.accepted_families().contains(x)));
        }
        assert_eq!(
            Bug::P54T13NoPositionCheck.accepted_families(),
            ["chain", "model", "avail"]
        );
    }

    #[test]
    fn the_catalogue_is_in_enum_order_and_complete() {
        for (i, row) in CATALOGUE.iter().enumerate() {
            assert_eq!(row.bug as usize, i, "{:?}", row.bug);
            assert!(!row.text.is_empty() && !row.detected_by.is_empty());
            assert!(row.rule.starts_with("P-") || row.rule.starts_with("L-"));
        }
        let rules: BTreeSet<&str> = CATALOGUE.iter().map(|r| r.rule).collect();
        assert_eq!(rules.len(), N_BUGS, "one bug per rule");
        assert_eq!(
            CATALOGUE
                .iter()
                .filter(|r| r.rule.starts_with("L-"))
                .count(),
            3
        );
        // Every source bug of [F16 §17.1] that the toy carries appears once, except G12 (two halves).
        let sources: Vec<&str> = CATALOGUE
            .iter()
            .flat_map(|r| r.source.split(", "))
            .filter(|s| !s.is_empty())
            .collect();
        for g in 1..=13 {
            let label = format!("G{g}");
            let n = sources.iter().filter(|s| **s == label).count();
            assert_eq!(n, if g == 12 { 2 } else { 1 }, "{label}");
        }
        for t in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14] {
            let label = format!("T{t}");
            assert_eq!(
                sources.iter().filter(|s| **s == label).count(),
                1,
                "{label}"
            );
        }
        assert!(sources.contains(&"LG") && sources.contains(&"DF"));
    }

    #[test]
    fn switches_are_independent_bits() {
        let mut all = Bugs::NONE;
        for b in Bug::ALL {
            assert!(!Bugs::NONE.on(b));
            assert!(Bugs::only(b).on(b));
            assert_eq!(Bugs::only(b).iter().collect::<Vec<_>>(), [b]);
            all = all.with(b);
        }
        assert_eq!(all.iter().count(), N_BUGS);
        assert!(Bugs::NONE.is_none() && !all.is_none());
        assert_eq!(
            Bug::P40G1CoveredByCommitted.to_string(),
            "P-40 G1 (P40G1CoveredByCommitted)"
        );
    }
}
