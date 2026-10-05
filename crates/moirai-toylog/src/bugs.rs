//! The seeded-bug switches ([F16 §17]; PLAN WP-40, S4): one switch per bug of the catalogue whose vehicle is the toy log
//! — the 76 rows of [F16 §17.3] with vehicle **toy** and the rows L-6, L-7 and L-8 of [F16 §17.4], 79 in all ([F16] open
//! point 1) — each named after its P-rule (or L-rule) and the source bug it coincides with ([F16 §17.1]: T1–T14, G1–G13,
//! LG, DF).
//!
//! A [`Bugs`] value is part of the toy's [`crate::Config`]; with every switch off the toy follows [F16] exactly. Each
//! switch is the smallest change of the toy that violates its rule, as the catalogue's "Primary seeded bug" cell states
//! it; where the toy's form needs more words than the cell, [`BugInfo::toy_form`] says how. The code that a switch
//! changes checks it with [`Bugs::on`] at the one place the rule is enforced.
//!
//! [`CATALOGUE`] holds the specification's cells verbatim: the rule, the source bugs, the "Detected by" cell (whose
//! families [`Bug::families`] reads) and the "Primary seeded bug" cell; a unit test compares them with
//! `docs/spec/format/16-protocol.md`. The rows whose vehicle is not the toy carry no switch here: P-45 (G5) is "none
//! (masked)", and P-59 and P-79 are carried by M1's gates ([F16 §17.3]; spec sync 2b S2B-P-41, S2B-P-44, S2B-P-45, which
//! owner question OQ-A-1 confirms or reopens).
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
    /// P-13, T9: a durable publish writes one slot and flushes.
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
    /// P-40, G1: a durable group is acknowledged after its append and identity check, without a covering flush.
    P40G1CoveredByCommitted,
    /// P-41: a flush-byte timeout is acknowledged as success.
    P41FlushTimeoutAcked,
    /// P-42, G3, T5: the flush holder flushes without re-writing the pending range.
    P42G3T5FlushWithoutRewrite,
    /// P-43, G8: the pending range is scanned and re-written outside the writer byte.
    P43G8RewriteOutsideWriter,
    /// P-44: a failed flush is retried on the same handle and its success acknowledged.
    P44FlushRetriedAndAcked,
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
    /// P-97: a rotation begins a new extent without its extent head.
    P97NoExtentHead,
    /// L-6 ([OS/lock §5.4] I-L4 with I-L2): a grant obtained by one kernel wait is handed to two in-process waiters.
    L06GrantToTwoWaiters,
    /// L-7 ([OS/lock §5.4] I-L6): a grant that arrives after the waiter's deadline is neither returned nor released.
    L07LateGrantLeaked,
    /// L-8 ([F03 §3.1] rule 2): the maintenance decider probes only the first quiet byte.
    L08ProbeFirstQuietOnly,
}

/// The number of seeded bugs the toy log carries: the 76 toy rows of [F16 §17.3] and the 3 of [F16 §17.4].
pub const N_BUGS: usize = 79;

/// One row of the catalogue.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BugInfo {
    /// The bug.
    pub bug: Bug,
    /// The rule it violates: `P-n` of [F16] or `L-n` of [F16 §17.4].
    pub rule: &'static str,
    /// The source bugs it coincides with ([F16 §17.1]); empty for none.
    pub source: &'static str,
    /// The catalogue's "Detected by" cell, verbatim.
    pub detected_by: &'static str,
    /// The catalogue's "Primary seeded bug" cell, verbatim.
    pub text: &'static str,
    /// How the toy's switch realises it, when the cell does not say; empty otherwise.
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

    /// The assertion families ([F16 §17.2]) the catalogue's "Detected by" cell names: the first word of each of its
    /// `;`-separated parts (`ack`, `fresh`, `chain`, `trace`, `model`, `ns`, `avail`).
    pub fn families(self) -> Vec<&'static str> {
        classes(self.info().detected_by)
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

/// The catalogue: [F16 §17.3]'s toy rows in rule order, then [F16 §17.4]'s toy rows, with their cells verbatim.
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
        "avail (the extent grows past E, which [F05 §2.2] makes corrupt, so every scan refuses and nothing is acknowledged)",
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
        "avail (after a torn publish the other slot names files a later checkpoint deleted; the log is intact, so nothing acknowledged is lost)",
        "a publish overwrites the slot that holds the newest valid state",
        "",
    ),
    row(
        Bug::P13T9SingleSlotBarrier,
        "P-13",
        "T9",
        "ack",
        "a durable publish of a flag change writes one slot and flushes; a crash that tears the newer slot while the older reverts brings back the replaced flag (the barrier form is masked by P-62: the covering publish of the `Checkpoint` already wrote the new set)",
        "every durable publish (the barrier, `quiet`, boot-change recovery) writes one slot and flushes `HEAD`; the enumerator reaches the flag-change form: after `quiet on` and `quiet off` each wrote one slot, a crash that tears the newer slot while the older reverts to its durable content brings back the replaced flag",
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
        "avail (a later process finds an invalid group below `durable_lsn` and refuses, P-58)",
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
        "avail (the extent grows past E and every scan refuses, [F05 §2.2])",
        "W3 is not re-checked; a group longer than E is appended and acknowledged",
        "a runtime batch whose final encoding (its symbol definitions, allocated under the writer byte) outgrows W3 is appended at E_v in one `write_at`, which extends the extent file past E",
    ),
    row(
        Bug::P36CheckpointAdvancesHlc,
        "P-36",
        "",
        "model (commit id, I43′)",
        "a `Checkpoint` advances the sequence: a class-I checkpoint appended between two commits in one millisecond raises the next commit's `hlc`, so its commit id differs from the model's (pass 1, P1-5)",
        "",
    ),
    row(
        Bug::P37ChainSeedAtCommitted,
        "P-37",
        "",
        "avail (the chain rule rejects the group, so it is never acknowledged and the operation ends `outcome_unknown` without a failed flush)",
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
        "a durable group is acknowledged after its append and identity check, without a covering flush (covered by the committed position of its own append); the form \"covered when `committed_lsn` ≥ E_g\" cannot occur, because the published `committed_lsn` never passes a pending durable group (P-38, P-49)",
        "the covered test takes the committed position of the group's own append (the end of the valid log it extended, at least E_g), so the durable group goes to its identity check right after the append",
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
        "avail (the unrewritten sectors stay poisoned, so the holder's publish scans a shorter log than it flushed and refuses by P-48)",
        "the flush holder flushes without re-writing a dead predecessor's range after a failed flush",
        "",
    ),
    row(
        Bug::P43G8RewriteOutsideWriter,
        "P-43",
        "G8",
        "trace (ig4 states the rule itself; the chain or acknowledgement damage needs an append into the scanned range, which appenders never make)",
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
        "model (only the stale tail of a lost group surviving a refill passes, which revives a value no acknowledged operation wrote, I14′; an acknowledged group never loses its predecessor)",
        "a group is accepted whose predecessor differs (no chain check)",
        "",
    ),
    row(
        Bug::P54T13NoPositionCheck,
        "P-54",
        "T13",
        "avail (every later process refuses the rebuilt store, P-61 or P-58)",
        "the position check is skipped; `repair` without a valid slot (P-85 step 2) takes a copy of another extent, put in place by setup as an external rewrite of the lowest extent's file ([F15] FM-10.1), as the lowest extent of the epoch and rebuilds the slots from it. Every other scan start is seeded from `HEAD`, where the chain rule (P-53) rejects a misplaced group first",
        "the switch skips [F05 §5.2] check 5 wherever a record is validated, `repair`'s own check of the head it starts from included (P-85 step 2); the `misplaced` scenario's setup puts a byte copy of log.2 in place of the retired log.1 ([F15] FM-10.1)",
    ),
    row(
        Bug::P55NoEpochCheck,
        "P-55",
        "",
        "avail (readers seed the chain at `epoch_lsn` with the slot's epoch and refuse the rebuilt store, P-58)",
        "a record of another epoch is accepted; `repair` without a valid slot (P-85 step 2) takes an extent of another epoch, put in place by setup as an external rewrite ([F15] FM-10.1), as the lowest extent of its epoch. Every other scan start is seeded from `HEAD`, where the chain rule rejects such a group first",
        "the switch skips [F05 §5.2] check 6 wherever a record is validated, `repair`'s check of the lower extents' heads against step 1's epoch included (P-85 step 2); the `foreign` scenario's setup puts log.1 of another epoch in place of the retired log.1 ([F15] FM-10.1)",
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
        "a slot that passes its checksum but fails validity is skipped for the other slot; only a defective writer produces such a slot, so setup injects one as an external rewrite of the slot with a checksummed slot that fails [F04 §7] check 5 ([F15] FM-10.1)",
        "the `fatal` scenario's setup rewrites the prefilled store's newest slot, checksum included, with `durable_lsn` above `committed_lsn` ([F04 §7] check 5; [F15] FM-10.1)",
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
        "avail (recovery publishes a `durable_lsn` over bytes the log does not hold, or refuses that fatal slot, and every later process refuses)",
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
        "avail (the flushed range keeps poisoned sectors, so the recovery's publish scans another end than it flushed and refuses by P-48)",
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
        "a local commit carries the toy's `flags` bit 1 (it implies no ref move), and a `RefUpdate` with the toy's reason 6 moves its ref in a group of its own, written after the acknowledgement by the handle's next operation or when the handle is dropped",
    ),
    row(
        Bug::P70FailedCasMovesRef,
        "P-70",
        "",
        "model (I27′)",
        "a commit whose ref CAS failed moves its ref anyway, where the rule appends a `RefUpdate` reason 5 `park` of `orphans/<R>` ([F05 §9.2])",
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
        "trace (the preparation's flushes and namespace calls run under the writer byte, which P-2's predicate reports)",
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
        "avail (the reader meets an invalid group below `durable_lsn` and refuses, P-58)",
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
        Bug::P81PinInLaterGroup,
        "P-81",
        "",
        "ack; model (the Pin is one of the fork's acknowledged effects, lost with the later group; the deletion needs a later checkpoint)",
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
        "an appender's scan treats a read error above `durable_lsn` as the end of the log and appends over an acknowledged group that an OS crash left above a stale `durable_lsn` (pass 1, S1-25)",
        "",
    ),
    row(
        Bug::P96SpareWithoutFlushes,
        "P-96",
        "",
        "ack; ns",
        "a rotation appends into a spare without re-issuing `durable+meta` and `durable-name`; a crash loses the spare's size or name under an acknowledged group (pass 1, P1-7)",
        "",
    ),
    row(
        Bug::P97NoExtentHead,
        "P-97",
        "",
        "avail (every later scan that reaches the extent refuses the store, so the group after the missing head is never acknowledged, and a slot-less `repair` refuses too)",
        "a rotation begins a new extent without its extent head; after one retirement a `repair` without a valid slot (P-85 step 2) cannot validate the active log (pass 1, P1-8). The toy reaches it: it rotates, retires (P-73) and repairs from the extent heads, and [F05 §5.4] makes an extent whose first group is not its head corrupt at every scan",
        "the rotating appender writes the pad (when one is due) and then its own groups from the new extent's first byte, with no extent-head group before them",
    ),
    row(
        Bug::L06GrantToTwoWaiters,
        "L-6",
        "",
        "trace (ig4: the second client scans and re-writes without a grant of the writer byte; the first client's identity check fails and it re-runs, P-46, P-47, so no acknowledged group is overwritten)",
        "a grant obtained by one kernel wait is handed to two waiting clients of one process; both append at one lsn",
        "the toy's lock layer: a client that arrives while a sibling client is inside the process's kernel wait for the byte waits on that wait's outcome and is handed the grant the sibling obtains",
    ),
    row(
        Bug::L07LateGrantLeaked,
        "L-7",
        "",
        "avail",
        "a grant that arrives after the waiter's deadline is neither returned nor released; the byte stays held by a client that returned `Busy`, and every later writer times out",
        "after a timed-out wait the toy's lock layer takes the byte once more on a fresh client (a late grant) and neither returns nor releases it",
    ),
    row(
        Bug::L08ProbeFirstQuietOnly,
        "L-8",
        "",
        "trace (a checkpoint during quiet mode)",
        "the maintenance decider probes only the first quiet byte; a checkpoint runs while another requester holds a later quiet byte (pass 1, P1-10)",
        "",
    ),
];

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
    fn detection_families_are_known() {
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
        // A `;` inside the parentheses of an explanation separates nothing.
        assert_eq!(Bug::P53G9NoChainCheck.families(), ["model"]);
        assert_eq!(Bug::P81PinInLaterGroup.families(), ["ack", "model"]);
        assert_eq!(Bug::P97NoExtentHead.families(), ["avail"]);
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
        // Every source bug of [F16 §17.1] appears once, except G12 (two halves) and G5, whose row (P-45) has vehicle
        // "none (masked)".
        let sources: Vec<&str> = CATALOGUE
            .iter()
            .flat_map(|r| r.source.split(", "))
            .filter(|s| !s.is_empty())
            .collect();
        for g in 1..=13 {
            let label = format!("G{g}");
            let n = sources.iter().filter(|s| **s == label).count();
            let want = match g {
                5 => 0,
                12 => 2,
                _ => 1,
            };
            assert_eq!(n, want, "{label}");
        }
        for t in 1..=14 {
            let label = format!("T{t}");
            assert_eq!(
                sources.iter().filter(|s| **s == label).count(),
                1,
                "{label}"
            );
        }
        assert!(sources.contains(&"LG") && sources.contains(&"DF"));
    }

    /// The cells of one row of a Markdown table: split at `|` outside code spans, trimmed.
    fn cells(line: &str) -> Vec<String> {
        let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
        let mut out = Vec::new();
        let (mut cur, mut code) = (String::new(), false);
        for ch in inner.chars() {
            match ch {
                '`' => {
                    code = !code;
                    cur.push(ch);
                }
                '|' if !code => out.push(core::mem::take(&mut cur).trim().to_owned()),
                _ => cur.push(ch),
            }
        }
        out.push(cur.trim().to_owned());
        out
    }

    /// The catalogue is [F16 §17.3]'s rows with vehicle **toy** and [F16 §17.4]'s rows whose vehicle is the toy, in the
    /// specification's order, each with its rule, source bugs, "Detected by" cell and "Primary seeded bug" cell verbatim
    /// (spec sync 2b S2B-P-46, S2B-P-53; PLAN WP-40b: the bug list equals the chapter's list).
    #[test]
    fn the_catalogue_holds_the_specification_cells_verbatim() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/spec/format/16-protocol.md"
        );
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut rows: Vec<(String, String, String, String)> = Vec::new();
        let mut section = 0;
        for line in text.lines() {
            if line.starts_with("### 17.3") {
                section = 3;
            } else if line.starts_with("### 17.4") {
                section = 4;
            } else if line.starts_with("## ") {
                section = 0;
            }
            match section {
                3 if line.starts_with("| P-") => {
                    let c = cells(line);
                    assert_eq!(c.len(), 6, "{line}");
                    if c[5] == "toy" {
                        let source = if c[3] == "—" {
                            String::new()
                        } else {
                            c[3].clone()
                        };
                        rows.push((c[0].clone(), source, c[4].clone(), c[2].clone()));
                    }
                }
                4 if line.starts_with("| L-") => {
                    let c = cells(line);
                    assert_eq!(c.len(), 5, "{line}");
                    if c[4].starts_with("toy") {
                        rows.push((c[0].clone(), String::new(), c[3].clone(), c[2].clone()));
                    }
                }
                _ => {}
            }
        }
        assert_eq!(rows.len(), N_BUGS, "the toy rows of [F16 §17.3] and §17.4");
        for (row, (rule, source, detected, text)) in CATALOGUE.iter().zip(&rows) {
            assert_eq!(row.rule, rule);
            assert_eq!(row.source, source, "{rule}: source bugs");
            assert_eq!(row.detected_by, detected, "{rule}: Detected by");
            assert_eq!(row.text, text, "{rule}: Primary seeded bug");
        }
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
