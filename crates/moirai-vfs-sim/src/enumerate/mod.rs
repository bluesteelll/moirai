//! The crash enumerator (WP-32): GT1's crash-point enumeration over the in-memory `Vfs` ([60 §3.1] item 3, [60 §3.13]
//! GT1, [F15 §6.4], [80 §2.4.4]), generic over the store code it drives.
//!
//! # Use
//!
//! The code under test implements [`Subject`]: a workload that runs on a [`SimWorld`] and reports every effect it
//! attempts, acknowledges, observes and refuses through a [`Ledger`]; a recovery that reads the store after a crash
//! (first as a reader, before any writer runs, then as the recovering writer); the paths of its slot files (`HEAD`);
//! the store protocol it follows, if any ([`Protocol`]: its log extents and its pure slot decoder); and, optionally,
//! trace predicates of its own. [`enumerate`] then runs the workload again and again under [`EnumConfig`]'s tier and
//! reports what it checked and every failure ([`Report`]). The toy log (WP-40) is the first subject; M1's engine is the
//! next.
//!
//! # Dimensions ([F15 §6.4]; PLAN WP-32)
//!
//! | Dimension | How | Items |
//! |---|---|---|
//! | Crash points at every write, flush, publish, create, rename and unlink, between the appends of one group included | a crash image at every scheduling point of a call that can change what a crash leaves ([`CallKind::changes_storage`]), both at its start and inside it, and after the workload's end | all |
//! | Every subset of ≤ 12 unflushed sectors, ≥ 10⁴ random states beyond | [`PlanMode::Full`] per file (module `plans`): every subset of the dirty sectors at baseline or newest, intermediate versions, and random states that sample versions, torn sectors and poisoned sub-sector mixes — at the crash points of clean runs and after every failed flush ([`Limits::fault_plans`]) | FM-1, FM-3 |
//! | One torn sector | per file, with sub-sector mixes | FM-1.2 |
//! | Both `HEAD` slots, 9 states per barrier | the slot files' product of every version and torn mix (module `plans`, `slot_states`: 8 where both slots are dirty, FM-1.2), at every crash point; and at the end of every run whose disk-full or death cut a slot write, in every tier, with crash points after it in the nightly tier — the second way to leave no valid slot ([F04 §8.1] "Both slots absent", S2B-P-27) | FM-1, FM-3, FM-5.2, §2.5 |
//! | Bounded cross-file products | the product of the axes' corner states, sampled beyond `cross_budget` | FM-1 |
//! | Namespace operations lost in any subset and order | the survivors replayed by the simulator in issue order: every subset while there are ≤ 12 pending operations, ≥ 10⁴ random subsets beyond ([`PlanMode::Full`]); issue-order prefixes, each one lost alone and each one surviving alone ([`PlanMode::Prefix`]) | FM-2 |
//! | Disk-full at every write, flush and create (and namespace operation) | one run per occurrence of each fault site, the write's partial application chosen, then recovery after the deaths and after a crash | FM-5 |
//! | "Flush error, more commits, crash" | one run per file flush failed with `Io`, under each [`PoisonPolicy`], then a crash at every later crash point | FM-3, FM-11 |
//! | Several pending groups and flush holders with reverted, invalidated or evicted pages while appends continue | the failed-flush runs' poison policies while the workload's other writers go on; a kill of the process at every point — inside a flush with each of its three outcomes, every member of a `sync_group` separately ([`Limits::death_vectors`]), a failed outcome under each poison policy — and, in the nightly tier, of every flush holder and lock waiter at the other processes' points, with crash points after a flush holder's death while the next holder adopts its group | FM-3, FM-11, §2.5 |
//! | Lock-release delays | every kill with each configured release class; the nightly tier forces classes (b) and (c) and fails a seed that lacks either ([F15 §3.8]); a writer that cannot recover behind a byte held beyond every bound or never released may refuse | FM-8.1 |
//! | Read errors, transient and persistent; mapping-fault deaths; external truncation of a sealed file | one run per read and per mapping with the fault injected; every sealed file truncated after the workload and while readers still run (at the points of calls on it in the PR tier, at every point after its seal in the nightly tier): its readers may refuse (exit 7), the recovered state must keep every acknowledged commit, and the recovery's diagnosis must name the file ([80 §2.5] rule 8) | FM-9, FM-10, FM-12 |
//! | PR tier: per-file prefixes plus one torn sector | [`Tier::Pr`]: [`PlanMode::Prefix`], ≤ 10 min (its time budget fails the run when exceeded) | FM-1 |
//! | Nightly tier state counts | [`Report`]'s `Display`; the nightly tier fails below GT1's 10⁵ crash states ([AR §8.3]) | — |
//!
//! # Assertions
//!
//! The generic families of [F16 §17.2] are this module's own verdicts at M0 ([F13 §1.4] "The toy vehicle": the seeded
//! bugs' author does not write them, S4). After every crash state and after every run's process deaths, the subject's
//! [`Subject::recover`] runs on the post-crash world, and the recovery and the world's whole trace are judged against
//! the ledger entries recorded before the crash point:
//! - **ack** (module `verdict`): every acknowledged durable effect is present, and no value is a phantom (I-G1,
//!   [60 §4.4] item 4); every operation is all or nothing, so a commit never survives without its markers ([72 M1]);
//!   markers and leases are effects like any other ([`EffectKind`]).
//! - **fresh**: the first read after the crash, before any writer runs, is fresh (I-G2), while every durable value a
//!   reader saw before the crash survives it (F-A1); a reader never sees a durable-class group beyond the `durable_lsn`
//!   of the slot it read ([`Ledger::observe_covered`]). I-G2's post-crash clause is judged as [F13 §3.8] reads it
//!   precisely: an Unknown-boot reader's first read for consistency only ([`Subject::recover_with_boot`]), and a field
//!   kept only in `HEAD` whose durable publish was not acknowledged as a value that may vanish
//!   ([`Ledger::begin_kept_in_head`], FM-3.6).
//! - **chain** (I-G3): the identity bytes of every acknowledged durable group ([`Ledger::group_bytes`]) are still in
//!   place in the recovered world, wherever a file still holds them.
//! - **avail** (module `refusal`): every refusal — the first read's, the recovering writer's, one the recovery answered
//!   with the operator's repair ([`Recovered::answered`]), and every workload operation's ([`Ledger::refused`]) — is a
//!   [`Refusal`] that a fault the run injected explains — an FM-10 act, during the run or declared by the setup, only
//!   through the file it touched ([`Refused::concerning`], [`Ledger::setup_fault`], [`Ledger::setup_ignored`]); the
//!   recovering writer's final answer is never one that repair answers. A diagnosis ([`Recovered::diagnosed`]) that names a file the recovered state references needs an external
//!   act or a read error on that file ([F15] G-13); after an external truncation of a sealed file it must name it
//!   (FM-10.2).
//! - **trace** (module `protocol`): for a subject that declares its [`Protocol`], I-G4 and I-G6 over the lock, flush and
//!   namespace events and the decoded `HEAD` slot writes, and [F03 §3.1] rule 2 over the probe rounds ([F13 §1.4]);
//!   the simulator's own protocol-violation reports ([F15 §3.13]); and the subject's own predicates
//!   ([`Subject::check_trace`]).
//! - **ns**: every file the ledger expects somewhere for the recovered value of a register ([`Ledger::expect_names`])
//!   is there, by node identity.
//!
//! The subject's own findings (`doctor --verify`: the model family on the toy) are failures too.
//!
//! # Determinism
//!
//! Every run of one seed replays the discovery run's choices until its injected fault (module `adversary`): the same seed
//! and variant replay a failure. The enumerator checks the replay by the trace digest.
//!
//! # Changes (S4)
//!
//! The enumerator's author never writes or reads the seeded bugs (PLAN §3.1 S4). A change to this module after a seeded
//! bug was missed names the fault-model item it widens (FM-1 … FM-12, [F15 §3]) in its comment and its commit. The
//! changes of WP-32's review pass 1: FM-3.2/FM-3.6 (a lazy value a first read saw may vanish after a failed flush:
//! module `verdict`); FM-3.3 (full plans after every failed flush); §2.5 and FM-11.2 (per-member `sync_group` outcomes,
//! kills of flush holders and lock waiters at other processes' points, crash points after a flush holder's death, failed
//! outcomes under every poison policy); FM-8.1 (release classes (b) and (c) forced and checked); FM-10.2 (truncation
//! while readers run, the repaired state judged, the diagnosis checked); FM-2.2 (the current size with garbage beyond the
//! durable size). The changes of spec sync 2b: FM-5.2 and §2.5 with FM-1.2 (the slot states at the end of a run whose
//! disk-full or death cut a slot write, and crash points after such a death in the nightly tier: S2B-P-27); FM-3.6 (an
//! unacknowledged field kept only in `HEAD` may vanish: module `verdict`, S2B-P-14); [F13 §3.8] (an Unknown-boot
//! reader's first read judged for consistency only: S2B-P-18). The changes of WP-40's S4 disposition, which move the
//! generic checks out of the subject ([F13 §1.4], [F16 §17.2]), each named by the fault-model items it reads: the trace
//! predicates over tasks, flush and lock bytes and decoded slot writes, with poisoning keyed on the slot file's own
//! sectors (FM-3.2, FM-3.5, FM-11; module `protocol`); typed refusals judged against the run's faults, the no-valid-slot
//! allowance over the slot files (OP-1, FM-3.3, FM-5.2, §2.5), a dead holder's bounded waits for the first read too
//! (FM-8.1), `outcome_unknown` after a failed flush or read (FM-3, FM-12; module `refusal`); a diagnosis of a referenced
//! file only with a cause (FM-10, FM-12; G-13); the chain over acknowledged groups' identity bytes (FM-3.4); fresh over
//! observed positions; ns by node identity (FM-2). The changes of WP-40's closure check, after P-55 was missed:
//! FM-10 (an act, during the run or declared by the setup, explains a refusal only through the file it touched; a setup
//! fault explains no refusal that names no file; a file the setup declares the protocol must ignore explains none:
//! module `refusal`, [`Ledger::setup_ignored`], [`Refused::concerning`]); and [F15 §5.2] with [F16] P-72 step 2 (the
//! steps of `create_extent` and `recycle_extent` are bracketed by the world's notes, [`crate::NOTE_COMPOSITE`], so
//! their zeros are judged by the preparation's rules, P-2 and P-72, and not as log writes: module `protocol`).

mod adversary;
mod ledger;
mod plans;
mod protocol;
mod refusal;
mod report;
mod verdict;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub use adversary::PoisonPolicy;
pub use ledger::{
    Class, EffectKey, EffectKind, EffectSet, EffectWrite, Ledger, NOTE_ACK, NOTE_BEGIN,
};
pub use plans::{Dim, PlanLimits, PlanMode, crash_plans, for_each_plan};
pub use protocol::{
    MAINT_AUTOMATIC, MAINT_BELOW_CAP, NOTE_MAINT_DECISION, NOTE_PHASE, Protocol, SlotDecode,
    SlotView,
};
pub use refusal::{Diagnosis, Refusal, Refused};
pub use report::{CrashCase, Failure, Report, Variant};

use crate::SimWorld;
use crate::adversary::{FaultRates, PartialWrite, ReleaseDelayLaw, SeededAdversary, Site};
use crate::content::SectorKind;
use crate::crash::{CrashImage, CrashPlan};
use crate::rng::{Rng, splitmix};
use crate::trace::{Event, TraceMode};
use crate::world::{BusyAt, CallKind, DeathPlan, PointInfo, SimConfig, SimUnwind, Watch};

use adversary::{AdvStats, EnumAdversary, Injection, SharedStats, lock_stats};
use ledger::OpTable;
use protocol::TraceJudge;
use refusal::{Allow, Facts, Setup};
use verdict::Verdict;

/// The setup's declarations of `t`, with the subject's slot files `slots` (module `refusal`).
fn setup_of<'a>(slots: &'a [PathBuf], t: &'a OpTable) -> Setup<'a> {
    Setup {
        faults: &t.setup_faults,
        ignored: &t.setup_ignored,
        slots,
    }
}

/// The boot mode of the reader that made a recovery's first read ([OS/proc §5]; [F16 §10] P-67).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum BootMode {
    /// The reader read its boot identity: after a crash its first read reflects every acknowledged record (I-G2).
    #[default]
    Known,
    /// The reader's boot identity is `Unknown`: it never runs boot-change recovery (U2), so its first read reflects
    /// every acknowledged record only after the next writer's flush (U5–U6, P-67; [F13 §3.8] "I-G2's post-crash clause,
    /// read precisely", "Boot mode").
    Unknown,
}

/// What a subject's recovery found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recovered {
    /// The store as a reader's first read after the crash returns it, before any writer runs — after the boot-change
    /// recovery that reader runs itself when the protocol says so ([F16 §8] P-60, [F16 §10] P-66): read freshness,
    /// I-G2. A refusal is judged by module `refusal`.
    pub first_read: Result<EffectSet, Refused>,
    /// The store after the writer's recovery — and after the operator's repair, where the recovery refused with a
    /// refusal repair answers ([`Recovered::answered`]: plain `repair` for no valid or a fatal slot, [F16] P-61, P-85;
    /// `repair --rebuild-from-log` for a damaged sealed file, [80 §2.5] rule 8, [F16] P-68).
    pub state: Result<EffectSet, Refused>,
    /// The refusals the recovery answered with the operator's repair before its final answers, in order: each must be
    /// a correct answer for the run's faults, like any refusal.
    pub answered: Vec<Refused>,
    /// Violations the subject's own checks found (for example `doctor --verify`).
    pub findings: Vec<String>,
    /// The store files the subject's diagnosis names as damaged (`doctor --fsck`, or an exit-7 message): after an
    /// external truncation of a sealed file it must name that file (FM-10.2, [80 §2.5] rule 8); one the recovered state
    /// references needs a fault the run injected on it ([F15] G-13).
    pub diagnosed: Vec<Diagnosis>,
}

impl Recovered {
    /// A recovery whose first read and recovered state are both `state`, with no refusal, no finding and no diagnosis.
    pub fn same(state: EffectSet) -> Recovered {
        Recovered {
            first_read: Ok(state.clone()),
            state: Ok(state),
            answered: Vec::new(),
            findings: Vec::new(),
            diagnosed: Vec::new(),
        }
    }
}

/// The store code under enumeration.
///
/// Every method runs on the enumerator's thread (the simulator's driver). The workload makes its `Vfs` calls from tasks
/// ([`SimWorld::spawn`], [`SimWorld::run`]); a call from the driver thread unwinds with [`SimUnwind::Died`] when the
/// enumerator kills its process there, which ends the workload early (the enumerator treats it as that process's death).
/// A subject must be deterministic: the same world seed and the same choices give the same calls (no clocks, no
/// randomness, no hash-map iteration order outside the simulator). It never queues adversary choices
/// ([`SimWorld::queue_choice`]) — the enumerator's injections count the adversary's choices.
pub trait Subject {
    /// The world configuration for `seed`. The enumerator sets the trace mode (every event: its verdicts read the
    /// trace) and the watch (the slot files and the log extents its predicates follow, [`Watch`]), neither of which
    /// changes a choice, and changes nothing else.
    fn config(&self, seed: u64) -> SimConfig {
        SimConfig::new(seed)
    }

    /// Builds what exists before the workload ([`SimWorld::mkdir_all`], [`SimWorld::put_file`]); an effect the store
    /// holds from the start is recorded here (begun and acknowledged).
    fn setup(&self, world: &SimWorld, ledger: &Ledger);

    /// Runs the workload to its end, reporting every operation through `ledger` ([`Ledger::begin`], [`Ledger::ack`],
    /// [`Ledger::observe`], [`Ledger::refused`], [`Ledger::fail`], and the identity, coverage and namespace hooks).
    /// Several processes, clients and tasks, idempotent retries after a failure, and at least three concurrent writers
    /// make the group-commit dimensions reachable ([80 §2.4.4]).
    fn workload(&self, world: &SimWorld, ledger: &Ledger);

    /// Recovers the store in `world` — after a system crash (a fresh boot) or after the workload's process deaths — with
    /// new processes: first a reader's first read (with whatever that reader runs before it, [F16 §8] P-60), before any
    /// writer runs, then a writer's recovery and the state it leaves, after the operator's repair where the recovery
    /// refused with a refusal repair answers. A refusal is `Err` with its typed reason (exit 7), judged against the
    /// run's faults (module `refusal`). A process that cannot take a role byte within its wait bound because a dead
    /// process holds it beyond every bound or never releases it (FM-8.1 classes (b), (c)) refuses with
    /// [`Refusal::Busy`]; the store as readers see it then is the first read, which after process deaths alone may lag
    /// behind the last publish (module `verdict`). The enumerator gives it a fault-free adversary (poisoned sectors
    /// still read by the run's [`PoisonPolicy`]).
    fn recover(&self, world: &SimWorld) -> Recovered;

    /// [`Subject::recover`], with the boot mode of the reader that made the first read ([OS/proc §5]). The enumerator
    /// calls this method; the default calls `recover` and answers [`BootMode::Known`]. A subject whose first reader may
    /// be in Unknown-boot mode overrides it (and its `recover` returns this method's [`Recovered`]): after a crash, such a
    /// reader's first read is judged for consistency only, not completeness, since it reflects every acknowledged record
    /// only after the next writer's flush ([F13 §3.8] "I-G2's post-crash clause, read precisely", U5–U6, [F16 §10]
    /// P-67; module `verdict`).
    fn recover_with_boot(&self, world: &SimWorld) -> (Recovered, BootMode) {
        (self.recover(world), BootMode::Known)
    }

    /// The absolute paths of the slot files: every non-clean sector of one is enumerated with every version and torn mix
    /// (the two `HEAD` slots, [F15 §1.4]: slot A is sector 0, slot B sector 1, [F04 §2]). The world watches them: every
    /// write to one is captured, and failed flushes and cut writes of them are the no-valid-slot allowance (module
    /// `refusal`).
    fn slot_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// The store protocol the subject follows ([F16]), if it follows moirai's: the trace predicates of module `protocol`
    /// (I-G4, I-G6 over the decoded slot writes, [F03 §3.1] rule 2) then judge every run. A subject that follows none
    /// is judged by the other verdicts alone.
    fn protocol(&self) -> Option<Protocol> {
        None
    }

    /// Whether the subject checks the event trace itself ([`Subject::check_trace`]).
    fn checks_trace(&self) -> bool {
        false
    }

    /// Trace predicates of the subject's own, beyond the enumerator's (module `protocol`), over the simulator's
    /// replayable event trace: the flush, lock, namespace and `Return` events, the harness notes of every `begin` and
    /// `ack` ([`NOTE_BEGIN`], [`NOTE_ACK`]), the world's notes ([`crate::NOTE_CLASS`], [`crate::NOTE_SLOT_WRITE`],
    /// [`crate::NOTE_PROBE`]) and the subject's own ([`SimWorld::note`]). Called after every recovery, with the whole
    /// trace of the recovered world: the workload up to the crash point (or to its end, after process deaths only), the
    /// crash, and the recovery. Each returned string is a violation.
    fn check_trace(&self, events: &[Event]) -> Vec<String> {
        let _ = events;
        Vec::new()
    }
}

/// The test tier (PLAN §2.1: `MOIRAI_TEST_TIER`).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Tier {
    /// Hosted CI: per-file prefixes plus one torn sector, kills at every storage call, disk-full at every fault site;
    /// ≤ 10 minutes.
    Pr,
    /// The laptop's nightly windows: every subset, every torn mix, random states, kills at every point and of every
    /// busy process, every poison policy, crash points after every injected fault and after flush holders' deaths.
    Nightly,
    /// The exit runs: nightly, with more random states per crash point and no window after a death.
    Exit,
}

impl Tier {
    /// The tier `MOIRAI_TEST_TIER` names (`pr` when unset).
    pub fn from_env() -> Tier {
        Tier::parse(std::env::var("MOIRAI_TEST_TIER").ok().as_deref())
    }

    /// The tier a `MOIRAI_TEST_TIER` value names: `nightly`, `exit`, and `pr` for anything else or nothing.
    pub fn parse(name: Option<&str>) -> Tier {
        match name {
            Some("nightly") => Tier::Nightly,
            Some("exit") => Tier::Exit,
            _ => Tier::Pr,
        }
    }

    /// The tier's limits.
    pub fn limits(self) -> Limits {
        let plans = PlanLimits {
            exhaustive_max: 12,
            random_min: 10_000,
            random_per_point: 0,
            cross_budget: 16,
        };
        match self {
            Tier::Pr => Limits {
                plans,
                clean_plans: PlanMode::Prefix,
                fault_plans: PlanMode::Prefix,
                other_plans: PlanMode::Prefix,
                fault_crash_points: false,
                kill_every_point: false,
                kill_holders: false,
                kill_crash_points: false,
                kill_window: None,
                kill_release_classes: vec![0],
                kill_release_ns: 1_000_000,
                death_vectors: 27,
                write_partials: vec![PartialWrite::Sectors(1), PartialWrite::Mask(0x5EED_0001)],
                poison_policies: vec![
                    PoisonPolicy::Revert,
                    PoisonPolicy::Evict,
                    PoisonPolicy::Alternate,
                ],
                truncate_every_point: false,
                release_obligation: false,
                min_states: 0,
                images_per_batch: 64,
                budget: Some(Duration::from_secs(600)),
                failures_kept: 32,
            },
            Tier::Nightly | Tier::Exit => Limits {
                plans: PlanLimits {
                    random_per_point: if self == Tier::Exit { 64 } else { 16 },
                    cross_budget: 4_096,
                    ..plans
                },
                clean_plans: PlanMode::Full,
                fault_plans: PlanMode::Full,
                other_plans: PlanMode::Prefix,
                fault_crash_points: true,
                kill_every_point: true,
                kill_holders: true,
                kill_crash_points: true,
                kill_window: if self == Tier::Exit { None } else { Some(64) },
                kill_release_classes: vec![0, 1, 2],
                kill_release_ns: 1_000_000,
                death_vectors: 243,
                write_partials: vec![
                    PartialWrite::Nothing,
                    PartialWrite::All,
                    PartialWrite::Sectors(1),
                    PartialWrite::Mask(0x5EED_0001),
                ],
                poison_policies: vec![
                    PoisonPolicy::Seeded,
                    PoisonPolicy::Revert,
                    PoisonPolicy::Newest,
                    PoisonPolicy::Alternate,
                    PoisonPolicy::Evict,
                ],
                truncate_every_point: true,
                release_obligation: true,
                min_states: GT1_MIN_STATES,
                images_per_batch: 256,
                budget: None,
                failures_kept: 64,
            },
        }
    }
}

/// GT1's minimum of crash states per nightly enumeration ([AR §8.3] GT1: "≥ 10⁵ crash states").
pub const GT1_MIN_STATES: u64 = 100_000;

/// What an enumeration covers and how far.
#[derive(Clone, Debug)]
pub struct Limits {
    /// The plan generator's numbers.
    pub plans: PlanLimits,
    /// The crash states at the crash points of runs without an injected fault.
    pub clean_plans: PlanMode,
    /// The crash states after a failed flush — an injected one, a disk-full flush, a death's failed outcome — where
    /// poisoned sectors exist ([F15 §6.4]: the random states sample their sub-sector mixes, FM-3.3).
    pub fault_plans: PlanMode,
    /// The crash states after any other injected fault or death (no poisoned sector).
    pub other_plans: PlanMode,
    /// Crash points after every disk-full injection too (else only at the end of the run, where a failed slot write adds
    /// the slot states to the pivots, S2B-P-27).
    pub fault_crash_points: bool,
    /// Kill at every scheduling point (else only at the points of storage calls).
    pub kill_every_point: bool,
    /// Also kill every process that is inside a flush or a lock wait at the other processes' points ([`BusyAt`]:
    /// FM-11.2, a paused flush holder's failed outcome poisons the appends others made during its flush, FM-3.1).
    pub kill_holders: bool,
    /// Crash points after the death of a flush holder — before its flush, inside it, just after it — while the next
    /// holder adopts its group ([80 §2.4.4]), and after a death that cut a write to a slot file while the next publisher
    /// writes over the cut slot (S2B-P-27), for the deaths whose bytes are released after a measured delay.
    pub kill_crash_points: bool,
    /// At most this many crash points after such a death (`None`: every one to the end of the run).
    pub kill_window: Option<usize>,
    /// The release-delay classes of a killed process's lock bytes (FM-8.1: 0 measured, 1 beyond every wait bound, 2
    /// never).
    pub kill_release_classes: Vec<u8>,
    /// The class-0 release delay of a killed process's bytes, in ns.
    pub kill_release_ns: u64,
    /// The outcome vectors of a death inside a `sync_group` (§2.5: each member resolves separately): every vector while
    /// there are at most this many, else this many sampled (the uniform ones always among them).
    pub death_vectors: u32,
    /// How much of a write a death or a disk-full inside it applies (§2.5, FM-5.2).
    pub write_partials: Vec<PartialWrite>,
    /// The poison policies of the failed-flush runs and of the deaths with a failed flush outcome.
    pub poison_policies: Vec<PoisonPolicy>,
    /// Truncate each sealed file at every point after its seal (else at the points of calls on it and the next ones).
    pub truncate_every_point: bool,
    /// Fail a seed whose runs drew no release of class (b) or none of class (c) ([F15 §3.8]).
    pub release_obligation: bool,
    /// Fail an enumeration that checked fewer crash states ([`GT1_MIN_STATES`] in the nightly tiers).
    pub min_states: u64,
    /// Crash images held at once (memory); a longer run is replayed per batch.
    pub images_per_batch: usize,
    /// The wall-time budget; exceeding it stops the enumeration and fails the report ([`Report::incomplete`]).
    pub budget: Option<Duration>,
    /// Failures kept in full in the report (all are counted).
    pub failures_kept: usize,
}

/// Which dimension families run (all by default; a unit test of one dimension turns the others off).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Dims {
    /// Crash states at every crash point of the fault-free run.
    pub crash_points: bool,
    /// A kill at every point.
    pub kills: bool,
    /// Disk-full at every write, flush, create and namespace operation.
    pub disk_full: bool,
    /// A failed flush at every file flush, then more commits and a crash.
    pub flush_errors: bool,
    /// A read error at every read, a media fault at every mapping, and an external truncation of every sealed file.
    pub read_faults: bool,
}

impl Default for Dims {
    fn default() -> Dims {
        Dims {
            crash_points: true,
            kills: true,
            disk_full: true,
            flush_errors: true,
            read_faults: true,
        }
    }
}

/// One enumeration's configuration.
#[derive(Clone, Debug)]
pub struct EnumConfig {
    /// The tier.
    pub tier: Tier,
    /// The world seeds (each gives other interleavings and adversary choices).
    pub seeds: Vec<u64>,
    /// The tier's limits (adjustable).
    pub limits: Limits,
    /// The dimension families.
    pub dims: Dims,
}

impl EnumConfig {
    /// `tier` with its limits over `seeds`, every dimension on.
    pub fn new(tier: Tier, seeds: impl IntoIterator<Item = u64>) -> EnumConfig {
        EnumConfig {
            tier,
            seeds: seeds.into_iter().collect(),
            limits: tier.limits(),
            dims: Dims::default(),
        }
    }
}

/// Enumerates `subject` under `cfg` (see the module documentation).
pub fn enumerate<S: Subject + ?Sized>(subject: &S, cfg: &EnumConfig) -> Report {
    let slots = subject.slot_files();
    let protocol = subject.protocol();
    let watch = (!slots.is_empty() || protocol.is_some()).then(|| Watch {
        slots: slots.clone(),
        log_dir: protocol.as_ref().map(|p| p.log_dir.clone()),
        log_prefix: protocol
            .as_ref()
            .map(|p| p.log_prefix.clone())
            .unwrap_or_default(),
    });
    let mut e = Engine {
        subject,
        cfg,
        report: Report::new(cfg.tier),
        start: Instant::now(),
        mutating: CallKind::ALL
            .into_iter()
            .filter(|c| c.changes_storage())
            .collect(),
        traces: subject.checks_trace(),
        seed_release: [0; 3],
        slots,
        protocol,
        watch,
    };
    for &seed in &cfg.seeds {
        if e.stopped() {
            break;
        }
        e.seed(seed);
    }
    let total = e.report.states_total();
    if e.report.incomplete.is_none() && total < cfg.limits.min_states {
        let ctx = Ctx::new(
            cfg.seeds.first().copied().unwrap_or(0),
            Variant::Clean,
            PoisonPolicy::Seeded,
        );
        e.fail(
            &ctx,
            None,
            vec![format!(
                "GT1: {total} crash states checked, below the tier's minimum of {} ([AR §8.3] GT1)",
                cfg.limits.min_states
            )],
        );
    }
    e.report.elapsed = e.start.elapsed();
    e.report
}

// ---------------------------------------------------------------------------------------------------------------------
// The engine

/// The run a check belongs to.
#[derive(Clone, Debug)]
struct Ctx {
    seed: u64,
    variant: Variant,
    poison: PoisonPolicy,
    /// A sealed file an external actor truncated, and its sealed size: while it is shorter, the recovery's diagnosis
    /// must name it (FM-10.2).
    truncated: Option<(PathBuf, u64)>,
}

impl Ctx {
    fn new(seed: u64, variant: Variant, poison: PoisonPolicy) -> Ctx {
        Ctx {
            seed,
            variant,
            poison,
            truncated: None,
        }
    }
}

/// A death to inject.
#[derive(Clone, Debug)]
struct Kill {
    point: u64,
    proc: u32,
    plan: DeathPlan,
}

/// An external truncation to inject (FM-10.1).
#[derive(Clone, Debug)]
struct Truncation {
    path: PathBuf,
    node: u64,
    len: u64,
    /// The scheduling point; `None`: after the workload.
    at: Option<u64>,
}

/// One workload run's set-up.
#[derive(Clone, Debug)]
struct RunSpec {
    seed: u64,
    inject: Option<Injection>,
    poison: PoisonPolicy,
    kill: Option<Kill>,
    capture: Option<(u64, usize)>,
    record: bool,
    truncate: Option<Truncation>,
}

impl RunSpec {
    fn clean(seed: u64) -> RunSpec {
        RunSpec {
            seed,
            inject: None,
            poison: PoisonPolicy::Seeded,
            kill: None,
            capture: None,
            record: false,
            truncate: None,
        }
    }
}

/// What one run left.
struct RunOut {
    world: SimWorld,
    ledger: Ledger,
    points: Vec<PointInfo>,
    busy: Vec<BusyAt>,
    captures: Vec<CrashImage>,
    end: CrashImage,
    digest: u64,
    ff_end: u64,
    /// The release-delay classes the run's deaths drew (FM-8.1).
    release: [u64; 3],
    problems: Vec<String>,
    stats: AdvStats,
}

/// How a guarded call ended.
enum Ended<R> {
    Returned(R),
    Died,
    Panicked(String),
}

fn guard<R>(f: impl FnOnce() -> R) -> Ended<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ended::Returned(r),
        Err(payload) => match payload.downcast::<SimUnwind>() {
            Ok(u) => match *u {
                SimUnwind::Died => Ended::Died,
                SimUnwind::Deadlock => {
                    Ended::Panicked("every task blocked with no timer (deadlock)".to_owned())
                }
            },
            Err(p) => Ended::Panicked(if let Some(s) = p.downcast_ref::<&str>() {
                (*s).to_owned()
            } else if let Some(s) = p.downcast_ref::<String>() {
                s.clone()
            } else {
                "non-string panic payload".to_owned()
            }),
        },
    }
}

/// The disk-full answers per fault site (FM-5.1): the site's `DiskFull` code, and for a create both of FM-5.4's outcomes.
const DISK_FULL: [(Site, &[u64]); 5] = [
    (Site::WriteFault, &[1]),
    (Site::FlushFault, &[2]),
    (Site::SyncDirFault, &[2]),
    (Site::CreateFault, &[1, 2]),
    (Site::NsFault, &[1]),
];

/// `FlushFault`'s `Io` answer (FM-3.1).
const FLUSH_IO: u64 = 1;

/// The read faults (FM-12.1, FM-12.2: transient and persistent) and the mapping's media fault (FM-9.1).
const READ_FAULTS: [(Site, &[u64]); 2] = [(Site::ReadFault, &[1, 2]), (Site::MapFault, &[1])];

/// What the discovery run of one seed showed.
struct Discovery {
    points: Vec<PointInfo>,
    busy: Vec<BusyAt>,
    counts: BTreeMap<Site, u64>,
    /// Every sealed file at the end: path, size, node.
    sealed: Vec<(PathBuf, u64, u64)>,
}

impl Discovery {
    /// The point log entry of point `n`.
    fn point(&self, n: u64) -> Option<&PointInfo> {
        self.points
            .binary_search_by_key(&n, |p| p.point)
            .ok()
            .map(|i| &self.points[i])
    }

    /// Process `proc`'s busy entry at point `n` (the log is in point order, then process order).
    fn busy_at(&self, n: u64, proc: u32) -> Option<&BusyAt> {
        let from = self.busy.partition_point(|b| b.point < n);
        self.busy[from..]
            .iter()
            .take_while(|b| b.point == n)
            .find(|b| b.proc == proc)
    }
}

struct Engine<'a, S: ?Sized> {
    subject: &'a S,
    cfg: &'a EnumConfig,
    report: Report,
    start: Instant,
    mutating: Vec<CallKind>,
    /// The subject checks the trace itself ([`Subject::check_trace`]).
    traces: bool,
    /// The release classes the current seed's runs drew.
    seed_release: [u64; 3],
    /// The subject's slot files, its protocol, and the watch every run's world gets.
    slots: Vec<PathBuf>,
    protocol: Option<Protocol>,
    watch: Option<Watch>,
}

impl<S: Subject + ?Sized> Engine<'_, S> {
    fn stopped(&self) -> bool {
        self.report.incomplete.is_some()
    }

    fn check_budget(&mut self) {
        if let Some(b) = self.cfg.limits.budget
            && self.report.incomplete.is_none()
            && self.start.elapsed() > b
        {
            self.report.incomplete = Some(format!(
                "the {:?} tier's time budget of {} s was exceeded",
                self.cfg.tier,
                b.as_secs()
            ));
        }
    }

    fn fail(&mut self, ctx: &Ctx, crash: Option<CrashCase>, messages: Vec<String>) {
        self.report.failures_total += 1;
        if self.report.failures.len() < self.cfg.limits.failures_kept {
            self.report.failures.push(Failure {
                seed: ctx.seed,
                variant: ctx.variant.clone(),
                crash,
                messages,
            });
        }
    }

    fn run(&mut self, spec: &RunSpec) -> RunOut {
        let mut cfg = self.subject.config(spec.seed);
        cfg.trace = TraceMode::Full;
        cfg.watch.clone_from(&self.watch);
        let stats = SharedStats::default();
        let adv = EnumAdversary::new(
            SeededAdversary::new(cfg.rates, cfg.release_law.clone()),
            spec.poison,
            spec.inject,
            stats.clone(),
        );
        let world = SimWorld::with_adversary(cfg, Box::new(adv));
        let ledger = Ledger::new(&world);
        let mut problems = Vec::new();
        match guard(|| self.subject.setup(&world, &ledger)) {
            Ended::Returned(()) => {}
            Ended::Died => problems.push("setup: the driver's process died".to_owned()),
            Ended::Panicked(m) => problems.push(format!("setup: panicked: {m}")),
        }
        if spec.record {
            world.record_points();
        }
        if let Some(k) = &spec.kill {
            world.kill_proc_at(k.point, k.proc, k.plan.clone());
        }
        if let Some(tr) = &spec.truncate
            && let Some(at) = tr.at
        {
            world.truncate_at(at, tr.node, tr.len);
        }
        if let Some((from, limit)) = spec.capture {
            world.capture_calls(from, &self.mutating, limit);
        }
        match guard(|| self.subject.workload(&world, &ledger)) {
            Ended::Returned(()) | Ended::Died => {}
            Ended::Panicked(m) => problems.push(format!("workload: panicked: {m}")),
        }
        if let Some(tr) = &spec.truncate
            && tr.at.is_none()
            && let Err(e) = world.external_truncate(&tr.path, tr.len)
        {
            problems.push(format!(
                "enumerator: external truncation of {:?}: {e:?}",
                tr.path
            ));
        }
        problems.extend(violations(&world));
        let points = world.take_point_log();
        let busy = world.take_busy_log();
        let captures = world.take_captures().into_iter().map(|(_, i)| i).collect();
        let end = world.crash_image();
        let digest = world.trace_digest();
        let ff_end = world.failed_flushes();
        let release = world.release_classes();
        let stats = lock_stats(&stats).clone();
        self.report.runs += 1;
        self.report.poisoned_reads += stats.poisoned_reads;
        if spec.capture.is_none() {
            for (c, n) in release.iter().enumerate() {
                self.report.release_classes[c] += n;
                self.seed_release[c] += n;
            }
        }
        RunOut {
            world,
            ledger,
            points,
            busy,
            captures,
            end,
            digest,
            ff_end,
            release,
            problems,
            stats,
        }
    }

    /// A judge of a world's trace: the facts, and the trace predicates of the subject's protocol.
    fn new_judge(&self) -> TraceJudge {
        TraceJudge::new(self.protocol.as_ref(), !self.slots.is_empty())
    }

    /// The subject's recovery on `world`, judged against `t`; `busy`: a dead process's byte is never released there.
    /// `prefix` has judged the world's trace before event `from`; its violations are reported here only if
    /// `report_prefix` (a crash state's prefix is a run that `end_checks` judged whole).
    #[allow(clippy::too_many_arguments)] // the run, the world, the ledger prefix, the crash facts and the trace prefix.
    fn recover_and_judge(
        &mut self,
        ctx: &Ctx,
        world: &SimWorld,
        t: &OpTable,
        crash: bool,
        ff: u64,
        busy: bool,
        prefix: TraceJudge,
        from: u64,
        report_prefix: bool,
    ) -> Vec<String> {
        self.report.recoveries += 1;
        // FM-10.2: while the truncated file is shorter than sealed when the recovery starts, its diagnosis must name it.
        let damaged = ctx
            .truncated
            .as_ref()
            .filter(|(p, size)| world.file_len(p).is_some_and(|l| l < *size))
            .map(|(p, _)| p.clone());
        let ended = guard(|| self.subject.recover_with_boot(world));
        let mut judge = prefix;
        if !report_prefix {
            judge.msgs.clear();
        }
        let judged = judge.publishes;
        judge.feed(&world.trace_from(from), &|i| world.slot_capture(i));
        self.report.publishes_judged += judge.publishes - judged;
        let mut msgs = match ended {
            Ended::Returned((rec, boot)) => {
                let m = self.judge_recovered(world, t, (crash, ff, busy), &judge.facts, &rec, boot);
                self.diagnosis_names(damaged.as_deref(), &rec, m)
            }
            Ended::Died => vec!["recovery: the recovering process died".to_owned()],
            Ended::Panicked(m) => vec![format!("recovery: panicked: {m}")],
        };
        msgs.extend(judge.msgs.iter().map(|m| format!("trace: {m}")));
        if self.traces {
            let events = world.trace();
            match guard(|| self.subject.check_trace(&events)) {
                Ended::Returned(v) => msgs.extend(v.into_iter().map(|m| format!("trace: {m}"))),
                Ended::Died => msgs.push("trace: the check died".to_owned()),
                Ended::Panicked(m) => msgs.push(format!("trace: the check panicked: {m}")),
            }
        }
        msgs.extend(violations(world));
        msgs
    }

    /// FM-10.2: a sealed file that was truncated when the recovery started (`damaged`) is named by its diagnosis.
    fn diagnosis_names(
        &mut self,
        damaged: Option<&Path>,
        rec: &Recovered,
        mut m: Vec<String>,
    ) -> Vec<String> {
        if let Some(p) = damaged {
            self.report.diagnoses += 1;
            if !rec.diagnosed.iter().any(|d| d.path == p) {
                m.push(format!(
                    "diagnosis: the sealed file {} is truncated, but the recovery's diagnosis does not name it ([80 \
                     §2.5] rule 8, FM-10.2)",
                    p.display()
                ));
            }
        }
        m
    }

    /// The verdicts on one recovery (module documentation, "Assertions"): ack, fresh and avail through module
    /// `verdict`; the diagnosis of referenced files; chain; ns.
    fn judge_recovered(
        &mut self,
        world: &SimWorld,
        t: &OpTable,
        (crash, ff, busy): (bool, u64, bool),
        facts: &Facts,
        rec: &Recovered,
        boot: BootMode,
    ) -> Vec<String> {
        if boot == BootMode::Unknown {
            self.report.unknown_boot_reads += 1;
        }
        let sealed = |p: &Path| world.is_sealed(p);
        let setup = setup_of(&self.slots, t);
        let v = Verdict::new(t, crash, ff)
            .first_read_boot(boot)
            .faults(facts, busy, setup, &sealed);
        self.report.refusals += v.allowed_refusals(rec);
        let mut m = v.check(rec);
        // A referenced file named as damaged needs a fault on it: without an FM-10 act no store file is modified by
        // anyone but moirai ([F15] G-13), so a correct store named it only once it was durable ([F16] P-10). A file the
        // protocol must ignore is referenced by no correct state.
        for d in rec.diagnosed.iter().filter(|d| d.referenced) {
            if setup.ignores(&d.path) {
                m.push(format!(
                    "diagnosis: the recovered state references {}, a file the setup declared the protocol must ignore \
                     ([F16] P-54, P-55; [F15] FM-10)",
                    d.path.display()
                ));
                continue;
            }
            let caused = facts.faulted(&d.path, world.node_at(&d.path)) || setup.damaged(&d.path);
            if !caused {
                m.push(format!(
                    "diagnosis: the recovery names {}, which the recovered state references, as damaged, but no \
                     external act, read error or setup fault touched it ([F15] G-13, [F16] P-10, P-78)",
                    d.path.display()
                ));
            }
        }
        m.extend(self.chain(world, t, facts));
        if let Ok(state) = &rec.state {
            m.extend(names(world, t, state));
        }
        m
    }

    /// Chain (I-G3, [F16] P-53, P-74): every acknowledged durable group's identity bytes are where they were, in every
    /// file that still exists at their path.
    fn chain(&mut self, world: &SimWorld, t: &OpTable, facts: &Facts) -> Vec<String> {
        let mut out = Vec::new();
        let setup = setup_of(&self.slots, t);
        for g in &t.bytes {
            let op = &t.ops[g.op];
            if op.acked.is_none() || op.class != Class::Durable || op.head {
                continue;
            }
            let Some(node) = world.node_at(&g.path) else {
                continue;
            };
            if facts.modified(&g.path, Some(node))
                || setup.damaged(&g.path)
                || setup.ignores(&g.path)
            {
                continue;
            }
            self.report.chain_checks += 1;
            let got = world
                .peek_range(&g.path, g.offset, g.bytes.len() as u64)
                .unwrap_or_default();
            if got != g.bytes {
                out.push(format!(
                    "chain (I-G3): operation {}'s acknowledged group bytes at {} offset {} ({} bytes) {} in the \
                     recovered store ([F16] P-53, P-74)",
                    op.id,
                    g.path.display(),
                    g.offset,
                    g.bytes.len(),
                    if got.len() < g.bytes.len() {
                        "are cut short"
                    } else {
                        "changed"
                    }
                ));
            }
        }
        out
    }

    /// Avail for the workload's own refusals ([`Ledger::refused`]), against the faults of the run.
    fn workload_refusals(&mut self, t: &OpTable, facts: &Facts, ff: u64) -> Vec<String> {
        let allow = Allow {
            facts,
            busy: false,
            workload: true,
            nothing_required: !Verdict::new(t, false, ff).any_required(),
            setup: setup_of(&self.slots, t),
        };
        self.report.workload_refusals += t.refusals.len() as u64;
        t.refusals
            .iter()
            .filter_map(|(who, r)| {
                allow
                    .check(r)
                    .err()
                    .map(|why| format!("avail: {who} refused with {r}: {why}"))
            })
            .collect()
    }

    fn recovery_adversary(&mut self, poison: PoisonPolicy) -> (EnumAdversary, SharedStats) {
        let stats = SharedStats::default();
        let adv = EnumAdversary::new(
            SeededAdversary::new(FaultRates::default(), ReleaseDelayLaw::default()),
            poison,
            None,
            stats.clone(),
        );
        (adv, stats)
    }

    /// One crash state: materialise, recover, judge; `prefix` has judged the image's trace.
    fn judge(
        &mut self,
        ctx: &Ctx,
        img: &CrashImage,
        (dim, plan): (Dim, CrashPlan),
        t: &OpTable,
        prefix: &TraceJudge,
    ) {
        *self.report.states.entry(dim).or_insert(0) += 1;
        let (adv, stats) = self.recovery_adversary(ctx.poison);
        let msgs = match img.materialize_with(&plan, Box::new(adv)) {
            Ok(w) => self.recover_and_judge(
                ctx,
                &w,
                t,
                true,
                img.failed_flushes(),
                false,
                prefix.clone(),
                img.trace_count(),
                false,
            ),
            Err(e) => vec![format!("enumerator: crash plan rejected: {e}")],
        };
        self.report.poisoned_reads += lock_stats(&stats).poisoned_reads;
        if !msgs.is_empty() {
            self.fail(
                ctx,
                Some(CrashCase {
                    at: img.origin(),
                    dim,
                    plan,
                }),
                msgs,
            );
        }
        self.check_budget();
    }

    /// Every crash state `mode` names at the crash point of `img`; `slots_only`: the pivots and the slot files' states
    /// alone (every other file, the pending namespace operations and the writes in flight at each pivot). `prefix` has
    /// judged the image's trace, and every crash state continues from it.
    fn crash_states(
        &mut self,
        ctx: &Ctx,
        img: &CrashImage,
        t: &OpTable,
        mode: PlanMode,
        slots_only: bool,
        prefix: &TraceJudge,
    ) {
        let mut surface = img.surface();
        let slots: BTreeSet<u64> = self
            .subject
            .slot_files()
            .iter()
            .filter_map(|p| img.node_at(p))
            .collect();
        if slots_only {
            surface.files.retain(|f| slots.contains(&f.node));
            surface.ops.clear();
            surface.writes.clear();
        }
        self.report.crash_points += 1;
        if let Some(at) = img.origin() {
            *self.report.crash_points_by_call.entry(at.call).or_insert(0) += 1;
            if at.call == CallKind::Write && slots.contains(&at.node) {
                self.report.publish_points += 1;
            }
        }
        let poisoned: BTreeSet<(u64, u64)> = surface
            .files
            .iter()
            .flat_map(|f| {
                f.sectors
                    .iter()
                    .filter(|s| s.state != SectorKind::Dirty)
                    .map(move |s| (f.node, s.index))
            })
            .collect();
        let seed = ctx.seed ^ img.point().wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let cfg = self.cfg;
        let mut here: BTreeMap<Dim, u64> = BTreeMap::new();
        for_each_plan(
            &surface,
            &slots,
            mode,
            &cfg.limits.plans,
            seed,
            &mut |dim, plan| {
                if plans::mixes_poison(&plan, &poisoned) {
                    *self.report.poison_mixed.entry(dim).or_insert(0) += 1;
                }
                *here.entry(dim).or_insert(0) += 1;
                self.judge(ctx, img, (dim, plan), t, prefix);
                !self.stopped()
            },
        );
        let total: u64 = here.values().sum();
        self.report.point_max_total = self.report.point_max_total.max(total);
        for (d, n) in here {
            let m = self.report.point_max.entry(d).or_insert(0);
            *m = (*m).max(n);
        }
    }

    /// The nodes of the subject's slot files in `world`.
    fn slot_nodes(&self, world: &SimWorld) -> BTreeSet<u64> {
        self.subject
            .slot_files()
            .iter()
            .filter_map(|p| world.node_at(p))
            .collect()
    }

    /// The checks at the end of a run: its own problems, recovery after its process deaths alone, and recovery after a
    /// system crash at its end, in the states `mode` names. `cut_slot`: the run's disk-full or death cut a write to a slot
    /// file, which leaves that slot any mix of bytes (FM-5.2, [F15 §2.5]); where `mode` takes only the pivots, the end
    /// takes the slot files' states too, so that a crash tearing the other, dirty slot (FM-1.2) is judged: the second way
    /// to leave no valid slot ([F04 §8.1] "Both slots absent", [F15 §6.4]; spec sync 2b S2B-P-27).
    fn end_checks(&mut self, ctx: &Ctx, run: &RunOut, mode: PlanMode, cut_slot: bool) {
        let entries = run.ledger.entries();
        let t = OpTable::build(&entries, u64::MAX);
        let mut msgs = run.problems.clone();
        msgs.extend(t.problems.iter().cloned());
        self.report.death_checks += 1;
        // A byte a dead process holds beyond every wait bound, or never releases, may keep the recovering processes out
        // (FM-8.1 classes (b) and (c)): the first reader's boot-change recovery or repair, and the writer, wait out
        // their bounds and give up, which is no wrong answer.
        let busy = run.release[1] > 0 || run.release[2] > 0;
        // The run's trace, judged whole: its own violations, and the faults its workload's refusals are judged against.
        let mut judge = self.new_judge();
        let from = run.world.event_count();
        judge.feed(&run.world.trace_from(0), &|i| run.world.slot_capture(i));
        self.report.publishes_judged += judge.publishes;
        // The run's end image has the same trace: its crash states continue from this judge.
        let mut end_prefix = judge.clone();
        end_prefix.msgs.clear();
        msgs.extend(self.workload_refusals(&t, &judge.facts, run.ff_end));
        msgs.extend(self.recover_and_judge(
            ctx, &run.world, &t, false, run.ff_end, busy, judge, from, true,
        ));
        if !msgs.is_empty() {
            self.fail(ctx, None, msgs);
        }
        if cut_slot {
            self.report.slot_fault_runs += 1;
        }
        if cut_slot && mode == PlanMode::Pivots {
            let slot_mode = self.cfg.limits.other_plans;
            self.crash_states(ctx, &run.end, &t, slot_mode, true, &end_prefix);
        } else {
            self.crash_states(ctx, &run.end, &t, mode, false, &end_prefix);
        }
        self.check_budget();
    }

    /// Crash states at every crash point from `from` on of the run `spec` describes (at most `window` of them), replayed
    /// in batches.
    fn crash_points(
        &mut self,
        ctx: &Ctx,
        spec: &RunSpec,
        from: u64,
        digest: u64,
        mode: PlanMode,
        window: Option<usize>,
    ) {
        let limit = self.cfg.limits.images_per_batch.max(1);
        let mut left = window.unwrap_or(usize::MAX);
        let mut from = from;
        // The images are captured in point order from replays of one run: one judge follows their traces, each
        // event fed once.
        let mut running = self.new_judge();
        let mut fed = 0u64;
        while left > 0 {
            if self.stopped() {
                return;
            }
            let batch = limit.min(left);
            let mut s = spec.clone();
            s.capture = Some((from, batch));
            s.record = false;
            let run = self.run(&s);
            if run.digest != digest {
                self.fail(
                    ctx,
                    None,
                    vec![format!(
                        "enumerator: the workload does not replay (trace digest {:#x}, expected {digest:#x}); {}",
                        run.digest, "a subject must be deterministic"
                    )],
                );
                return;
            }
            let entries = run.ledger.entries();
            let n = run.captures.len();
            let mut last = from;
            for img in &run.captures {
                let t = OpTable::build(&entries, img.point());
                img.trace_slices_from(fed, &mut |s| running.feed(s, &|i| img.slot_capture(i)));
                fed = img.trace_count();
                running.msgs.clear();
                self.crash_states(ctx, img, &t, mode, false, &running);
                last = img.point();
                if self.stopped() {
                    return;
                }
            }
            if n < batch {
                return;
            }
            left -= n;
            from = last + 1;
        }
    }

    fn seed(&mut self, seed: u64) {
        self.report.seeds += 1;
        self.seed_release = [0; 3];
        let dims = self.cfg.dims;
        let clean = RunSpec::clean(seed);
        let ctx = Ctx::new(seed, Variant::Clean, PoisonPolicy::Seeded);
        let disc_run = self.run(&RunSpec {
            record: true,
            ..clean.clone()
        });
        let sealed = disc_run
            .world
            .sealed_files()
            .into_iter()
            .filter_map(|(p, size)| disc_run.world.node_at(&p).map(|n| (p, size, n)))
            .collect();
        let disc = Discovery {
            points: disc_run.points.clone(),
            busy: disc_run.busy.clone(),
            counts: disc_run.stats.counts.clone(),
            sealed,
        };
        let digest = disc_run.digest;
        self.end_checks(&ctx, &disc_run, self.cfg.limits.clean_plans, false);
        drop(disc_run);
        if dims.crash_points {
            self.crash_points(&ctx, &clean, 0, digest, self.cfg.limits.clean_plans, None);
        }
        if dims.kills {
            self.kills(seed, &disc);
        }
        if dims.disk_full {
            self.disk_full(seed, &disc.counts);
        }
        if dims.flush_errors {
            self.flush_errors(seed, &disc.counts);
        }
        if dims.read_faults {
            self.read_faults(seed, &disc);
        }
        // [F15 §3.8]: every nightly run holds a death whose delay exceeds every wait bound and a byte never released.
        let r = self.seed_release;
        if self.cfg.limits.release_obligation && dims.kills && (r[1] == 0 || r[2] == 0) {
            self.fail(
                &ctx,
                None,
                vec![format!(
                    "FM-8.1 ([F15 §3.8]): seed {seed} drew {} release delays of class (b) and {} of class (c); the \
                     nightly tier needs at least one of each",
                    r[1], r[2]
                )],
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Process deaths (§2.5, FM-8.1, FM-11.2)

/// Whether `plan` fails some in-flight flush (its poisoned sectors are read by each poison policy).
fn fails_a_flush(plan: &DeathPlan) -> bool {
    plan.flush == Some(1) || plan.flush_each.contains(&1)
}

impl<S: Subject + ?Sized> Engine<'_, S> {
    /// A kill at every point (§2.5), and in the nightly tier of every busy process at the other processes' points.
    fn kills(&mut self, seed: u64, disc: &Discovery) {
        let l = self.cfg.limits.clone();
        // The first point of each process after one of its file flushes returned: a death "after its flush".
        let mut after_flush: BTreeSet<u64> = BTreeSet::new();
        let mut flushing: BTreeSet<u32> = BTreeSet::new();
        for p in &disc.points {
            if matches!(p.call, CallKind::Sync | CallKind::SyncGroup) && p.phase == 1 {
                flushing.insert(p.proc);
            } else if flushing.remove(&p.proc) {
                after_flush.insert(p.point);
            }
        }
        for at in disc
            .points
            .iter()
            .filter(|p| p.proc != u32::MAX && (l.kill_every_point || p.call.changes_storage()))
        {
            let holder = matches!(at.call, CallKind::Sync | CallKind::SyncGroup)
                || after_flush.contains(&at.point);
            let own = disc.busy_at(at.point, at.proc).filter(|_| at.phase == 1);
            let plans = death_plans(at, own, &l, seed);
            for plan in plans {
                if self.stopped() {
                    return;
                }
                self.kill(seed, at, at.proc, plan, holder);
            }
        }
        if !l.kill_holders {
            return;
        }
        for b in &disc.busy {
            let Some(at) = disc.point(b.point) else {
                continue;
            };
            if at.proc == b.proc || at.proc == u32::MAX {
                continue;
            }
            for plan in holder_plans(b, &l, seed) {
                if self.stopped() {
                    return;
                }
                self.report.kills_of_holders += 1;
                self.kill(seed, at, b.proc, plan, b.flushes > 0);
            }
        }
    }

    /// One death of process `victim` at point `at` by `plan`, under each poison policy if it fails a flush; with crash
    /// points after a flush holder's death, and after a death that cut a write to a slot file, in the nightly tier.
    fn kill(&mut self, seed: u64, at: &PointInfo, victim: u32, plan: DeathPlan, holder: bool) {
        let l = &self.cfg.limits;
        let policies = if fails_a_flush(&plan) {
            l.poison_policies.clone()
        } else {
            vec![PoisonPolicy::Seeded]
        };
        let (fault_plans, other_plans, window) = (l.fault_plans, l.other_plans, l.kill_window);
        let crash_points_after = l.kill_crash_points && plan.release_class == Some(0);
        // A death inside the victim's own write applies any part of it (§2.5): a slot write cut this way leaves its slot
        // any mix of bytes (S2B-P-27). One that applied nothing left the slot as it was.
        let cuts_write = victim == at.proc
            && at.call == CallKind::Write
            && at.phase == 1
            && plan.write.is_some_and(|w| w != PartialWrite::Nothing);
        for poison in policies {
            if self.stopped() {
                return;
            }
            self.report.kills += 1;
            let outcomes: Vec<u8> = plan.flush_each.iter().copied().chain(plan.flush).collect();
            for o in 0..3u8 {
                if outcomes.contains(&o) {
                    self.report.kills_in_flush[usize::from(o)] += 1;
                }
            }
            if outcomes.len() > 1 && outcomes.iter().any(|&o| o != outcomes[0]) {
                self.report.kills_mixed_group += 1;
            }
            let spec = RunSpec {
                kill: Some(Kill {
                    point: at.point,
                    proc: victim,
                    plan: plan.clone(),
                }),
                poison,
                ..RunSpec::clean(seed)
            };
            let run = self.run(&spec);
            let ctx = Ctx::new(
                seed,
                Variant::Kill {
                    at: *at,
                    victim,
                    plan: plan.clone(),
                },
                poison,
            );
            // A death that left a failed flush leaves poisoned sectors: the random states sample their mixes.
            let mode = if run.ff_end > 0 {
                fault_plans
            } else {
                PlanMode::Pivots
            };
            let (digest, poisoned) = (run.digest, run.ff_end > 0);
            let cut_slot = cuts_write && self.slot_nodes(&run.world).contains(&at.node);
            let after = crash_points_after && (holder || cut_slot);
            self.end_checks(&ctx, &run, mode, cut_slot);
            drop(run);
            if after {
                self.report.kills_with_crash_points += 1;
                let mode = if poisoned { fault_plans } else { other_plans };
                let before = self.report.crash_points;
                self.crash_points(&ctx, &spec, at.point + 1, digest, mode, window);
                self.report.crash_points_after_kills += self.report.crash_points - before;
            }
        }
    }
}

/// The outcome vectors of a death with `n` flushes in flight (bit i of `dir_mask`: flush i is a directory flush): every
/// vector while there are at most `bound`, else `bound` of them — the uniform ones and a sample ([F15 §2.5]: a
/// `sync_group`'s members resolve separately). A file member takes 0 (succeeded), 1 (failed) or 2 (not performed); a
/// directory member 0 (its operations durable) or 2 (left pending, FM-2.3 — a failed `sync_dir` is the same, FM-3.7), so
/// that 1 marks exactly the failures that poison.
fn outcome_vectors(n: u32, dir_mask: u64, bound: u32, seed: u64) -> Vec<Vec<u8>> {
    let dir = |i: u32| i < 64 && dir_mask & (1 << i) != 0;
    let arity = |i: u32| if dir(i) { 2u8 } else { 3u8 };
    let code = |i: u32, x: u8| if dir(i) && x == 1 { 2 } else { x };
    let coded = |v: &[u8]| -> Vec<u8> {
        v.iter()
            .enumerate()
            .map(|(i, &x)| code(i as u32, x))
            .collect()
    };
    let total = (0..n).try_fold(1u64, |acc, i| acc.checked_mul(u64::from(arity(i))));
    let mut out: Vec<Vec<u8>> = Vec::new();
    match total {
        Some(t) if t <= u64::from(bound) => {
            let mut v = vec![0u8; n as usize];
            'all: loop {
                out.push(coded(&v));
                for (i, x) in v.iter_mut().enumerate() {
                    *x += 1;
                    if *x < arity(i as u32) {
                        continue 'all;
                    }
                    *x = 0;
                }
                break;
            }
        }
        _ => {
            let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
            for o in 0..3u8 {
                let v = coded(&(0..n).map(|i| o.min(arity(i) - 1)).collect::<Vec<u8>>());
                if seen.insert(v.clone()) {
                    out.push(v);
                }
            }
            let mut s = seed ^ u64::from(n).rotate_left(23) ^ dir_mask;
            let mut rng = Rng::new(splitmix(&mut s));
            let mut tries = 0u64;
            while out.len() < bound as usize && tries < u64::from(bound) * 8 {
                tries += 1;
                let v: Vec<u8> = (0..n)
                    .map(|i| rng.below(u64::from(arity(i))) as u8)
                    .collect();
                let v = coded(&v);
                if seen.insert(v.clone()) {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// The deaths enumerated at one point of the dying process itself (§2.5, FM-11.2): inside its flushes every outcome
/// vector (one member: each of its three outcomes, two for a `sync_dir`), inside a write each partial application;
/// each with every configured release class. `own` is the process's busy entry at the point (inside a call).
fn death_plans(at: &PointInfo, own: Option<&BusyAt>, l: &Limits, seed: u64) -> Vec<DeathPlan> {
    let mut out = Vec::new();
    for &class in &l.kill_release_classes {
        let base = DeathPlan {
            release_class: Some(class),
            release_delay_ns: (class == 0).then_some(l.kill_release_ns),
            ..DeathPlan::default()
        };
        match (at.call, at.phase, own) {
            (CallKind::Sync | CallKind::SyncGroup | CallKind::SyncDir, 1, Some(b))
                if b.flushes > 0 =>
            {
                for v in outcome_vectors(b.flushes, b.dir_mask, l.death_vectors, seed ^ at.point) {
                    out.push(DeathPlan {
                        flush_each: v,
                        ..base.clone()
                    });
                }
            }
            (CallKind::Write, 1, _) => {
                for &pw in &l.write_partials {
                    out.push(DeathPlan {
                        write: Some(pw),
                        ..base.clone()
                    });
                }
            }
            _ => out.push(base),
        }
    }
    out
}

/// The deaths of a busy process at another process's point: inside its flushes every outcome vector, in a lock wait the
/// plain death; each with every configured release class.
fn holder_plans(b: &BusyAt, l: &Limits, seed: u64) -> Vec<DeathPlan> {
    let mut out = Vec::new();
    for &class in &l.kill_release_classes {
        let base = DeathPlan {
            release_class: Some(class),
            release_delay_ns: (class == 0).then_some(l.kill_release_ns),
            ..DeathPlan::default()
        };
        if b.flushes > 0 {
            for v in outcome_vectors(b.flushes, b.dir_mask, l.death_vectors, seed ^ b.point) {
                out.push(DeathPlan {
                    flush_each: v,
                    ..base.clone()
                });
            }
        } else {
            out.push(base);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// Injected faults (FM-3, FM-5, FM-9, FM-10, FM-12)

impl<S: Subject + ?Sized> Engine<'_, S> {
    fn disk_full(&mut self, seed: u64, counts: &BTreeMap<Site, u64>) {
        for (site, values) in DISK_FULL {
            let n = counts.get(&site).copied().unwrap_or(0);
            let partials: Vec<Option<PartialWrite>> = if site == Site::WriteFault {
                self.cfg
                    .limits
                    .write_partials
                    .iter()
                    .copied()
                    .map(Some)
                    .collect()
            } else {
                vec![None]
            };
            for nth in 0..n {
                for &value in values {
                    for &partial in &partials {
                        if self.stopped() {
                            return;
                        }
                        let inj = Injection {
                            site,
                            nth,
                            value,
                            follow: partial.map(|p| (Site::PartialWrite, p.to_choice())),
                        };
                        let spec = RunSpec {
                            inject: Some(inj),
                            ..RunSpec::clean(seed)
                        };
                        *self.report.disk_full.entry(site).or_insert(0) += 1;
                        let run = self.run(&spec);
                        let ctx = Ctx::new(
                            seed,
                            Variant::DiskFull {
                                site,
                                nth,
                                value,
                                partial,
                            },
                            PoisonPolicy::Seeded,
                        );
                        let (at, digest) = (run.stats.injected_at, run.digest);
                        // A disk-full flush is a failed flush: poisoned sectors (FM-3.1, FM-5.3). Where crash points follow
                        // the injection, the run's end takes the same states; else only the pivots, and the slot states
                        // when the failed write was a slot write, which leaves its slot any mix of bytes (FM-5.2,
                        // S2B-P-27).
                        let later = self.cfg.limits.fault_crash_points;
                        let mode = if run.ff_end > 0 {
                            self.cfg.limits.fault_plans
                        } else {
                            self.cfg.limits.other_plans
                        };
                        let cut_slot = site == Site::WriteFault
                            && run
                                .stats
                                .injected_node
                                .is_some_and(|n| self.slot_nodes(&run.world).contains(&n));
                        self.end_checks(
                            &ctx,
                            &run,
                            if later { mode } else { PlanMode::Pivots },
                            cut_slot,
                        );
                        drop(run);
                        if later && let Some(at) = at {
                            self.crash_points(&ctx, &spec, at + 1, digest, mode, None);
                        }
                    }
                }
            }
        }
    }

    fn flush_errors(&mut self, seed: u64, counts: &BTreeMap<Site, u64>) {
        let n = counts.get(&Site::FlushFault).copied().unwrap_or(0);
        let policies = self.cfg.limits.poison_policies.clone();
        for nth in 0..n {
            for &poison in &policies {
                if self.stopped() {
                    return;
                }
                let spec = RunSpec {
                    inject: Some(Injection {
                        site: Site::FlushFault,
                        nth,
                        value: FLUSH_IO,
                        follow: None,
                    }),
                    poison,
                    ..RunSpec::clean(seed)
                };
                self.report.flush_errors += 1;
                let run = self.run(&spec);
                let ctx = Ctx::new(seed, Variant::FlushError { nth, poison }, poison);
                let (at, digest) = (run.stats.injected_at, run.digest);
                let mode = self.cfg.limits.fault_plans;
                self.end_checks(&ctx, &run, mode, false);
                drop(run);
                if let Some(at) = at {
                    self.crash_points(&ctx, &spec, at + 1, digest, mode, None);
                }
            }
        }
    }

    /// Read errors and mapping faults at every occurrence, and every sealed file truncated after the workload and while
    /// readers still run ([F15 §6.4] "Reads", FM-10.2). The run's faults decide which refusals are correct (module
    /// `refusal`): after a persistent read error an I/O fault; after a truncation a damaged file, except as the
    /// writer's final answer — its recovery, with `repair`, keeps every acknowledged commit — and its diagnosis names the
    /// file ([80 §2.5] rule 8).
    fn read_faults(&mut self, seed: u64, disc: &Discovery) {
        for (site, values) in READ_FAULTS {
            let n = disc.counts.get(&site).copied().unwrap_or(0);
            for nth in 0..n {
                for &value in values {
                    if self.stopped() {
                        return;
                    }
                    let spec = RunSpec {
                        inject: Some(Injection {
                            site,
                            nth,
                            value,
                            follow: None,
                        }),
                        ..RunSpec::clean(seed)
                    };
                    *self.report.read_faults.entry(site).or_insert(0) += 1;
                    let run = self.run(&spec);
                    let ctx = Ctx::new(
                        seed,
                        Variant::ReadFault { site, nth, value },
                        PoisonPolicy::Seeded,
                    );
                    self.end_checks(&ctx, &run, PlanMode::Pivots, false);
                }
            }
        }
        for (path, size, node) in &disc.sealed {
            let sealed_at = disc
                .points
                .iter()
                .find(|p| p.call == CallKind::Seal && p.node == *node)
                .map_or(0, |p| p.point);
            let mut when: BTreeSet<Option<u64>> = BTreeSet::new();
            when.insert(None);
            for p in disc.points.iter().filter(|p| p.point > sealed_at) {
                if self.cfg.limits.truncate_every_point {
                    when.insert(Some(p.point));
                } else if p.node == *node {
                    // At a call on the file, and at the next point (a reader holding its mapping reads it later).
                    when.insert(Some(p.point));
                    when.insert(Some(p.point + 1));
                }
            }
            let mut lens = vec![0, size / 2];
            lens.dedup();
            for at in when {
                for &len in lens.iter().filter(|&&len| len < *size) {
                    if self.stopped() {
                        return;
                    }
                    let spec = RunSpec {
                        truncate: Some(Truncation {
                            path: path.clone(),
                            node: *node,
                            len,
                            at,
                        }),
                        ..RunSpec::clean(seed)
                    };
                    self.report.truncations += 1;
                    let run = self.run(&spec);
                    let mut ctx = Ctx::new(
                        seed,
                        Variant::Truncated {
                            path: path.clone(),
                            len,
                            at,
                        },
                        PoisonPolicy::Seeded,
                    );
                    ctx.truncated = Some((path.clone(), *size));
                    self.end_checks(&ctx, &run, PlanMode::Pivots, false);
                }
            }
        }
    }
}

/// Ns ([F16 §17.2]): every file the ledger expects somewhere for the recovered value of a register
/// ([`Ledger::expect_names`]) is there, by node identity — a file of the same name that is another node does not count.
fn names(world: &SimWorld, t: &OpTable, state: &EffectSet) -> Vec<String> {
    let mut out = Vec::new();
    for n in &t.names {
        let value = state.get(&n.key).copied();
        let (Some(node), Some((_, at))) = (n.node, n.rules.iter().find(|(v, _)| *v == value))
        else {
            continue;
        };
        let ok = if at.is_empty() {
            world.paths_of_node(node).is_empty()
        } else {
            at.iter().any(|p| world.node_at(p) == Some(node))
        };
        if !ok {
            let shown = value.map_or_else(|| "clear".to_owned(), |v| format!("{v:#x}"));
            let now = world.paths_of_node(node);
            out.push(format!(
                "ns: the file that was {} is {}, but the recovered {:?} = {shown} expects it {} ([F16 §17.2] ns, [40 \
                 §3.4])",
                n.path.display(),
                if now.is_empty() {
                    "nowhere".to_owned()
                } else {
                    format!("at {}", now[0].display())
                },
                n.key,
                if at.is_empty() {
                    "gone".to_owned()
                } else {
                    format!(
                        "at {}",
                        at.iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" or ")
                    )
                }
            ));
        }
    }
    out
}

/// The simulator's protocol-violation reports ([F15 §3.13]), drained.
fn violations(world: &SimWorld) -> Vec<String> {
    world
        .take_violations()
        .into_iter()
        .map(|v| {
            format!(
                "simulator: protocol violation {:?} by process {} on node {} at point {}",
                v.kind, v.proc, v.node, v.point
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(call: CallKind, phase: u64) -> PointInfo {
        PointInfo {
            point: 1,
            call,
            phase,
            proc: 0,
            node: 1,
        }
    }

    fn busy(flushes: u32, dir_mask: u64) -> BusyAt {
        BusyAt {
            point: 1,
            proc: 0,
            flushes,
            dir_mask,
            lock_wait: false,
        }
    }

    #[test]
    fn deaths_inside_a_flush_take_each_outcome() {
        let l = Tier::Pr.limits();
        let flush: Vec<Vec<u8>> = death_plans(&at(CallKind::Sync, 1), Some(&busy(1, 0)), &l, 1)
            .iter()
            .map(|p| p.flush_each.clone())
            .collect();
        assert_eq!(flush, [vec![0], vec![1], vec![2]]);
        assert_eq!(death_plans(&at(CallKind::Sync, 0), None, &l, 1).len(), 1);
        assert_eq!(
            death_plans(&at(CallKind::Write, 1), None, &l, 1).len(),
            l.write_partials.len()
        );
        let n = Tier::Nightly.limits();
        assert_eq!(
            death_plans(&at(CallKind::SyncDir, 1), Some(&busy(1, 1)), &n, 1).len(),
            6,
            "two outcomes × three release classes"
        );
    }

    #[test]
    fn sync_group_members_resolve_separately() {
        let l = Tier::Pr.limits();
        // Two files and a directory: 3 × 3 × 2 = 18 vectors, all of them.
        let v = outcome_vectors(3, 0b100, l.death_vectors, 7);
        assert_eq!(v.len(), 18);
        assert!(v.contains(&vec![0, 1, 0]) && v.contains(&vec![2, 0, 2]));
        assert!(
            v.iter().all(|x| x[2] == 0 || x[2] == 2),
            "a directory member is durable or left pending"
        );
        let distinct: BTreeSet<&Vec<u8>> = v.iter().collect();
        assert_eq!(distinct.len(), 18);
        // Five files: 243 > 27 vectors: 27 sampled, the uniform ones among them.
        let v = outcome_vectors(5, 0, l.death_vectors, 7);
        assert_eq!(v.len(), 27);
        for o in 0..3 {
            assert!(v.contains(&vec![o; 5]));
        }
        assert!(
            v.iter().any(|x| x.iter().any(|&o| o != x[0])),
            "mixed vectors"
        );
        // The plans of a sync_group death carry the vectors.
        let plans = death_plans(&at(CallKind::SyncGroup, 1), Some(&busy(2, 0)), &l, 1);
        assert_eq!(plans.len(), 9);
        assert!(plans.iter().any(|p| p.flush_each == [0, 2]));
    }

    #[test]
    fn busy_holders_die_with_every_outcome_or_plainly_in_a_lock_wait() {
        let n = Tier::Nightly.limits();
        assert_eq!(holder_plans(&busy(1, 0), &n, 1).len(), 9);
        let waiting = BusyAt {
            flushes: 0,
            lock_wait: true,
            ..busy(0, 0)
        };
        let plans = holder_plans(&waiting, &n, 1);
        assert_eq!(plans.len(), 3);
        assert_eq!(
            plans.iter().map(|p| p.release_class).collect::<Vec<_>>(),
            [Some(0), Some(1), Some(2)]
        );
        assert!(fails_a_flush(&DeathPlan {
            flush_each: vec![0, 1],
            ..DeathPlan::default()
        }));
        assert!(!fails_a_flush(&DeathPlan {
            flush_each: vec![0, 2],
            ..DeathPlan::default()
        }));
    }

    #[test]
    fn tiers_follow_the_environment_names() {
        assert_eq!(Tier::parse(Some("nightly")), Tier::Nightly);
        assert_eq!(Tier::parse(Some("exit")), Tier::Exit);
        assert_eq!(Tier::parse(Some("pr")), Tier::Pr);
        assert_eq!(Tier::parse(Some("NIGHTLY")), Tier::Pr, "names are exact");
        assert_eq!(Tier::parse(None), Tier::Pr);
        assert_eq!(
            Tier::from_env(),
            Tier::parse(std::env::var("MOIRAI_TEST_TIER").ok().as_deref())
        );
        assert_eq!(Tier::Pr.limits().clean_plans, PlanMode::Prefix);
        assert_eq!(Tier::Nightly.limits().clean_plans, PlanMode::Full);
        assert_eq!(Tier::Nightly.limits().fault_plans, PlanMode::Full);
        assert_eq!(Tier::Exit.limits().fault_plans, PlanMode::Full);
        assert_eq!(Tier::Nightly.limits().plans.random_min, 10_000);
        assert_eq!(Tier::Nightly.limits().plans.exhaustive_max, 12);
        assert_eq!(Tier::Nightly.limits().kill_release_classes, [0, 1, 2]);
        assert_eq!(Tier::Nightly.limits().min_states, GT1_MIN_STATES);
        assert!(
            Tier::Exit.limits().plans.random_per_point
                > Tier::Nightly.limits().plans.random_per_point
        );
        assert!(
            Tier::Pr
                .limits()
                .budget
                .is_some_and(|b| b <= Duration::from_secs(600))
        );
    }
}
